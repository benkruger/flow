//! `bin/flow ci` — repo-local CI orchestrator.
//!
//! Runs format → lint → build → test in sequence by execing the
//! corresponding `./bin/<tool>` scripts in the current working
//! directory. Each repo owns its actual command strings; FLOW
//! contributes the sentinel-based dirty-check optimization,
//! retry/flaky classification, the `FLOW_CI_RUNNING` recursion
//! guard, and a stable JSON output contract.
//!
//! By default, skips if nothing changed since the last passing run.
//! With `--force`, always runs regardless of sentinel state.
//! With `--retry N`, runs up to N times with force semantics and
//! classifies failures as flaky (passes on retry) or consistent
//! (all attempts fail). With `--simulate-branch`, sets
//! FLOW_SIMULATE_BRANCH in the child environment so current_branch()
//! returns the simulated name during test execution. The simulated
//! branch name is incorporated into the sentinel snapshot hash so runs
//! with different --simulate-branch values produce distinct sentinels.
//!
//! Output (JSON to stdout):
//!   Success:       {"status": "ok", "skipped": false}
//!   Skipped:       {"status": "ok", "skipped": true, "reason": "..."}
//!   Error:         {"status": "error", "message": "..."}
//!   Retry pass:    {"status": "ok", "attempts": 1}
//!   Retry flaky:   {"status": "ok", "attempts": 2, "flaky": true, "first_failure_output": "..."}
//!   Retry fail:    {"status": "error", "attempts": 3, "consistent": true, "output": "..."}

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use clap::Parser;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::flow_paths::FlowPaths;

/// CLI arguments for `bin/flow ci`.
#[derive(Parser, Debug)]
#[command(name = "ci", about = "Run CI with dirty-check optimization")]
pub struct Args {
    /// Force a run even when the sentinel matches the current snapshot
    #[arg(long)]
    pub force: bool,
    /// Run up to N times, classifying failures as flaky vs consistent
    #[arg(long, default_value_t = 0)]
    pub retry: u32,
    /// Override branch for sentinel naming (otherwise auto-detected from cwd)
    #[arg(long)]
    pub branch: Option<String>,
    /// Set FLOW_SIMULATE_BRANCH in the child env and mix it into the snapshot hash
    #[arg(long = "simulate-branch")]
    pub simulate_branch: Option<String>,
    /// Run only the format step. Mutually exclusive with --lint/--build/--test.
    /// Single-phase runs disable sentinel read+write because one tool passing
    /// does not satisfy the all-four-passed contract the sentinel encodes.
    #[arg(long, group = "phase_filter")]
    pub format: bool,
    /// Run only the lint step. See --format for sentinel semantics.
    #[arg(long, group = "phase_filter")]
    pub lint: bool,
    /// Run only the build step. See --format for sentinel semantics.
    #[arg(long, group = "phase_filter")]
    pub build: bool,
    /// Run only the test step. See --format for sentinel semantics.
    #[arg(long, group = "phase_filter")]
    pub test: bool,
    /// Run the test phase in audit mode: disable fail-fast, collect
    /// every violation (test failures, coverage shortfalls, per-test
    /// timing overruns, full-suite wall-time overruns), print a
    /// summary at the end. Implies --test (format/lint/build have no
    /// coverage or timing to audit); forwards --audit to `bin/test`.
    /// Mutually exclusive with the other phase filters.
    #[arg(long, group = "phase_filter")]
    pub audit: bool,
    /// Trailing args forwarded to the spawned `./bin/<tool>`.
    /// Only meaningful with a single-phase flag (`--format`/`--lint`/
    /// `--build`/`--test`); ignored otherwise. Use `--` to separate:
    /// `bin/flow ci --test -- hooks` or `bin/flow ci --test --file path`.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub trailing: Vec<String>,
}

impl Args {
    /// Returns the selected single phase, or None when all four run.
    ///
    /// `--audit` implies the test phase.
    pub fn selected_phase(&self) -> Option<&'static str> {
        if self.format {
            Some("format")
        } else if self.lint {
            Some("lint")
        } else if self.build {
            Some("build")
        } else if self.test || self.audit {
            Some("test")
        } else {
            None
        }
    }
}

/// A tool in the CI sequence: name for display, program + args for spawning.
pub struct CiTool {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
}

/// The four tool names FLOW orchestrates, in execution order.
///
/// Format runs first for fail-fast (instant check catches trivial errors
/// before compilation).
const TOOL_NAMES: [&str; 4] = ["format", "lint", "build", "test"];

/// Build the CI tool sequence by scanning `cwd/bin/` for executables.
///
/// For each name in [format, lint, build, test], if `cwd/bin/<name>`
/// exists as a file, add a CiTool that execs it directly. Missing
/// scripts are skipped — a repo without a `bin/test` simply has no
/// test step. The user owns the commands; FLOW orchestrates the
/// sequence and the gates.
pub fn bin_tool_sequence(cwd: &Path) -> Vec<CiTool> {
    let mut tools = Vec::new();
    for name in TOOL_NAMES {
        let path = cwd.join("bin").join(name);
        if path.is_file() {
            tools.push(CiTool {
                name: name.to_string(),
                program: path.to_string_lossy().to_string(),
                args: Vec::new(),
            });
        }
    }
    tools
}

/// Marker string used in `assets/bin-stubs/*.sh` to identify an
/// unconfigured stub. Scripts that contain this marker are treated as
/// placeholders by [`any_tool_is_stub`] and suppress sentinel writes
/// so the stderr reminder surfaces on every CI run until the user
/// configures a real command.
const STUB_MARKER: &str = "FLOW-STUB-UNCONFIGURED";

/// Return true if any of the scripts in `tools` contains the stub
/// marker. Used by [`run_once`] and [`run_with_retry`] to suppress
/// sentinel writes when CI "passed" only because the installed stubs
/// exit 0 with a stderr reminder.
///
/// This protects against a subtle failure mode: the stubs are
/// installed by `/flow:flow-prime` with `exit 0` so fresh primes
/// never block CI. Without stub detection, the first `bin/flow ci`
/// run after prime writes a sentinel, and every subsequent run skips
/// with "no changes since last CI pass" — the stderr reminder
/// becomes invisible and users can ship code with no real CI gate.
/// Scanning each script's source for the marker is cheap (four small
/// file reads) and catches the case even when a stub has been renamed
/// or moved, as long as the marker comment is preserved.
pub fn any_tool_is_stub(tools: &[CiTool]) -> bool {
    for tool in tools {
        if let Ok(content) = fs::read_to_string(&tool.program) {
            if content.contains(STUB_MARKER) {
                return true;
            }
        }
    }
    false
}

/// Build the sentinel file path for a given branch: `<root>/.flow-states/<branch>-ci-passed`.
///
/// Centralizes the naming convention so [`run_once`], [`run_with_retry`], and the
/// inline tests all agree on where sentinels live.
///
/// Also used by [`crate::finalize_commit::run_impl`] to refresh the sentinel after a clean commit.
pub fn sentinel_path(root: &Path, branch: &str) -> PathBuf {
    FlowPaths::new(root, branch).ci_sentinel()
}

/// Format an elapsed-ms count as a short human string: `523ms`,
/// `2.3s`, or `3m12s`. Used by the end-of-run summary line.
pub fn format_elapsed(ms: u64) -> String {
    if ms < 1000 {
        format!("{}ms", ms)
    } else if ms < 60_000 {
        format!("{:.1}s", (ms as f64) / 1000.0)
    } else {
        let total_secs = ms / 1000;
        let minutes = total_secs / 60;
        let secs = total_secs % 60;
        format!("{}m{}s", minutes, secs)
    }
}

/// Print the end-of-run summary to stderr:
/// `--- format: 0.5s | lint: 38.6s | build: 8.9s | test: 3m12s | total: 4m00s ---`
fn eprint_summary(phases: &[(String, u64)], total_ms: u64) {
    if phases.is_empty() {
        return;
    }
    let parts: Vec<String> = phases
        .iter()
        .map(|(name, ms)| format!("{}: {}", name, format_elapsed(*ms)))
        .collect();
    eprintln!(
        "\n--- {} | total: {} ---",
        parts.join(" | "),
        format_elapsed(total_ms)
    );
}

/// Run `program args` in `cwd`, returning its stdout as a lossy UTF-8
/// string. Spawn/IO errors produce an empty string — the snapshot hash
/// stays stable even when the program is missing.
fn program_stdout(cwd: &Path, program: &str, args: &[&str]) -> String {
    let bytes = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .map(|o| o.stdout)
        .unwrap_or_default();
    String::from_utf8_lossy(&bytes).to_string()
}

/// Run `git args` in `cwd`, returning its stdout as a lossy UTF-8 string.
fn git_stdout(cwd: &Path, args: &[&str]) -> String {
    program_stdout(cwd, "git", args)
}

/// Hash each path in `paths` (newline-separated) via `git hash-object`
/// and join the resulting object IDs with newlines. Missing paths
/// contribute empty lines.
fn git_hash_object_stdin_paths(cwd: &Path, paths: &str) -> String {
    let hashes: Vec<String> = paths
        .lines()
        .map(|p| git_stdout(cwd, &["hash-object", p]).trim().to_string())
        .collect();
    hashes.join("\n")
}

/// Compute the tree-state snapshot hash.
///
/// Combines four signals into a SHA-256 digest:
///
/// 1. `git rev-parse HEAD` (stripped) — changes after every commit
/// 2. `git diff HEAD` (raw) — captures staged + unstaged tracked changes
/// 3. `git ls-files --others --exclude-standard` (stripped) — untracked file list
/// 4. `git hash-object --stdin-paths` over the untracked list — untracked content
///
/// If `simulate_branch` is Some, the string `"\nsimulate:<name>"` is appended
/// to the combined input so runs with different simulate values produce
/// distinct sentinel hashes.
pub fn tree_snapshot(cwd: &Path, simulate_branch: Option<&str>) -> String {
    let head_trimmed = git_stdout(cwd, &["rev-parse", "HEAD"]).trim().to_string();
    let diff_raw = git_stdout(cwd, &["diff", "HEAD"]);
    let untracked_files = git_stdout(cwd, &["ls-files", "--others", "--exclude-standard"])
        .trim()
        .lines()
        .filter(|l| *l != ".flow-commit-msg")
        .collect::<Vec<_>>()
        .join("\n");

    let untracked_hash = if untracked_files.is_empty() {
        String::new()
    } else {
        git_hash_object_stdin_paths(cwd, &untracked_files)
    };

    let mut combined = format!(
        "{}\n{}\n{}\n{}",
        head_trimmed, diff_raw, untracked_files, untracked_hash
    );
    if let Some(sim) = simulate_branch {
        combined.push_str("\nsimulate:");
        combined.push_str(sim);
    }

    let mut hasher = Sha256::new();
    hasher.update(combined.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Default (non-retry) CI path.
///
/// Runs the tool sequence in `cwd` with inherited stdio so the user sees
/// output in real time. Sets `FLOW_CI_RUNNING=1` in each child's
/// environment.
///
/// Sentinel behavior (dirty-check optimization):
///
/// - When `branch` is Some, the sentinel path is
///   `<root>/.flow-states/<branch>-ci-passed`.
/// - When `!force` and the sentinel content matches the current
///   [`tree_snapshot`], the call returns skipped without running CI.
/// - On success, writes the snapshot to the sentinel (creating parent
///   dirs). On failure, unlinks the sentinel.
/// - Detached HEAD (`branch` is None) disables sentinel writes entirely.
///
/// Returns `(json_value, exit_code)` so the caller can print and exit.
pub fn run_once(
    cwd: &Path,
    root: &Path,
    tools: &[CiTool],
    branch: Option<&str>,
    force: bool,
    simulate_branch: Option<&str>,
    rebuild: bool,
) -> (Value, i32) {
    if tools.is_empty() {
        // A repo with no bin/{format,lint,build,test} scripts has no
        // gate at all, so returning "skipped ok" would silently pass
        // every commit. Fail loudly and tell the user how to fix it.
        return (
            json!({
                "status": "error",
                "message": "No ./bin/{format,lint,build,test} scripts found. Run /flow:flow-prime to install stubs or create the scripts manually.",
            }),
            1,
        );
    }

    // Detect unconfigured stubs up front so we can suppress the
    // sentinel write on success. See [`any_tool_is_stub`].
    let any_stub = any_tool_is_stub(tools);

    let sentinel = branch.map(|b| sentinel_path(root, b));
    let snapshot = tree_snapshot(cwd, simulate_branch);

    if !force {
        if let Some(ref path) = sentinel {
            if path.exists() {
                if let Ok(content) = fs::read_to_string(path) {
                    if content == snapshot {
                        return (
                            json!({
                                "status": "ok",
                                "skipped": true,
                                "reason": "no changes since last CI pass",
                            }),
                            0,
                        );
                    }
                }
            }
        }
    }

    let start = Instant::now();
    let mut phases: Vec<(String, u64)> = Vec::new();

    for tool in tools {
        let elapsed_before = start.elapsed().as_secs_f64();
        eprintln!("\n[{:.1}s] === {} ===", elapsed_before, tool.name);
        let tool_start = Instant::now();

        let mut cmd = Command::new(&tool.program);
        cmd.args(&tool.args)
            .current_dir(cwd)
            .env("FLOW_CI_RUNNING", "1");
        if force {
            cmd.env("FLOW_CI_FORCE", "1");
        }
        if rebuild {
            cmd.env("FLOW_CI_REBUILD", "1");
        }
        if let Some(sim) = simulate_branch {
            cmd.env("FLOW_SIMULATE_BRANCH", sim);
        }

        let status = match cmd.status() {
            Ok(s) => s,
            Err(e) => {
                let tool_ms = tool_start.elapsed().as_millis() as u64;
                phases.push((tool.name.clone(), tool_ms));
                let total_ms = start.elapsed().as_millis() as u64;
                eprint_summary(&phases, total_ms);
                if let Some(ref path) = sentinel {
                    let _ = fs::remove_file(path);
                }
                return (
                    json!({
                        "status": "error",
                        "message": format!("failed to run {} ({}): {}", tool.name, tool.program, e),
                        "elapsed_ms": total_ms,
                        "phases": phases_to_json(&phases),
                    }),
                    1,
                );
            }
        };

        let tool_ms = tool_start.elapsed().as_millis() as u64;
        phases.push((tool.name.clone(), tool_ms));

        if !status.success() {
            let total_ms = start.elapsed().as_millis() as u64;
            eprint_summary(&phases, total_ms);
            if let Some(ref path) = sentinel {
                let _ = fs::remove_file(path);
            }
            return (
                json!({
                    "status": "error",
                    "message": format!("{} failed", tool.name),
                    "elapsed_ms": total_ms,
                    "phases": phases_to_json(&phases),
                }),
                1,
            );
        }
    }

    let total_ms = start.elapsed().as_millis() as u64;
    eprint_summary(&phases, total_ms);

    if let Some(ref path) = sentinel {
        write_or_remove_sentinel(path, &snapshot, any_stub);
    }
    let mut response = json!({
        "status": "ok",
        "skipped": false,
        "elapsed_ms": total_ms,
        "phases": phases_to_json(&phases),
    });
    if any_stub {
        response["stubs_detected"] = json!(true);
    }
    (response, 0)
}

/// If `any_stub`, delete the sentinel (stubs must never lock in a
/// passing sentinel). Otherwise create the parent directory and write
/// the snapshot. Errors are intentionally swallowed — sentinel is a
/// best-effort optimization.
fn write_or_remove_sentinel(path: &Path, snapshot: &str, any_stub: bool) {
    if any_stub {
        let _ = fs::remove_file(path);
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, snapshot);
}

/// Convert `[(name, elapsed_ms)]` into the JSON array shape emitted in
/// `run_once`/`run_with_retry` responses: `[{"name":"format","elapsed_ms":523},...]`.
fn phases_to_json(phases: &[(String, u64)]) -> Value {
    Value::Array(
        phases
            .iter()
            .map(|(name, ms)| json!({"name": name, "elapsed_ms": ms}))
            .collect(),
    )
}

/// Retry CI path with flaky/consistent classification.
///
/// Runs the tool sequence up to `max_attempts` times with captured stdout
/// and stderr so the first failure's combined output can be returned as
/// `first_failure_output` when a retry pass classifies the test as flaky.
/// Does not check the sentinel internally — `run_impl` handles sentinel
/// skipping before dispatching here. Writes the sentinel on success and
/// unlinks on consistent failure.
pub fn run_with_retry(
    cwd: &Path,
    root: &Path,
    tools: &[CiTool],
    branch: Option<&str>,
    max_attempts: u32,
    simulate_branch: Option<&str>,
    rebuild: bool,
) -> (Value, i32) {
    if tools.is_empty() {
        // Mirror [`run_once`]: no gate → fail loudly. A retry run that
        // returned "ok" here would cache a useless sentinel and let
        // every commit bypass CI.
        return (
            json!({
                "status": "error",
                "message": "No ./bin/{format,lint,build,test} scripts found. Run /flow:flow-prime to install stubs or create the scripts manually.",
            }),
            1,
        );
    }

    let any_stub = any_tool_is_stub(tools);
    let sentinel = branch.map(|b| sentinel_path(root, b));
    let mut first_failure_output = String::new();
    let start = Instant::now();
    let mut phases: Vec<(String, u64)> = Vec::new();

    for attempt in 1..=max_attempts {
        let mut attempt_failed = false;
        let mut attempt_output = String::new();
        let mut attempt_phases: Vec<(String, u64)> = Vec::new();

        for tool in tools {
            let elapsed_before = start.elapsed().as_secs_f64();
            eprintln!(
                "\n[{:.1}s] === {} (attempt {}) ===",
                elapsed_before, tool.name, attempt
            );
            let tool_start = Instant::now();

            let mut cmd = Command::new(&tool.program);
            cmd.args(&tool.args)
                .current_dir(cwd)
                .env("FLOW_CI_RUNNING", "1");
            if rebuild {
                cmd.env("FLOW_CI_REBUILD", "1");
            }
            if let Some(sim) = simulate_branch {
                cmd.env("FLOW_SIMULATE_BRANCH", sim);
            }

            let output = match cmd.output() {
                Ok(o) => o,
                Err(e) => {
                    let tool_ms = tool_start.elapsed().as_millis() as u64;
                    attempt_phases.push((tool.name.clone(), tool_ms));
                    phases.extend(attempt_phases);
                    let total_ms = start.elapsed().as_millis() as u64;
                    eprint_summary(&phases, total_ms);
                    return (
                        json!({
                            "status": "error",
                            "message": format!("failed to run {} ({}): {}", tool.name, tool.program, e),
                            "elapsed_ms": total_ms,
                            "phases": phases_to_json(&phases),
                        }),
                        1,
                    );
                }
            };

            let tool_ms = tool_start.elapsed().as_millis() as u64;
            attempt_phases.push((tool.name.clone(), tool_ms));

            if !output.status.success() {
                attempt_output.push_str(&String::from_utf8_lossy(&output.stdout));
                attempt_output.push_str(&String::from_utf8_lossy(&output.stderr));
                attempt_failed = true;
                break;
            }
        }

        phases.extend(attempt_phases);

        if !attempt_failed {
            let snapshot = tree_snapshot(cwd, simulate_branch);
            if let Some(ref path) = sentinel {
                write_or_remove_sentinel(path, &snapshot, any_stub);
            }
            let total_ms = start.elapsed().as_millis() as u64;
            eprint_summary(&phases, total_ms);
            let mut result = json!({
                "status": "ok",
                "attempts": attempt,
                "elapsed_ms": total_ms,
                "phases": phases_to_json(&phases),
            });
            if attempt > 1 {
                result["flaky"] = json!(true);
                result["first_failure_output"] = json!(first_failure_output);
            }
            if any_stub {
                result["stubs_detected"] = json!(true);
            }
            return (result, 0);
        } else {
            if first_failure_output.is_empty() {
                first_failure_output = attempt_output.trim().to_string();
            }
            if let Some(ref path) = sentinel {
                if path.exists() {
                    let _ = fs::remove_file(path);
                }
            }
        }
    }

    let total_ms = start.elapsed().as_millis() as u64;
    eprint_summary(&phases, total_ms);
    (
        json!({
            "status": "error",
            "attempts": max_attempts,
            "consistent": true,
            "output": first_failure_output,
            "elapsed_ms": total_ms,
            "phases": phases_to_json(&phases),
        }),
        1,
    )
}

/// Testable CLI entry point.
///
/// Checks the sentinel BEFORE building the tool sequence so callers like
/// `finalize_commit` skip instantly when the tree state is clean. When
/// the sentinel does not match (or force/retry mode), scans `cwd/bin/`
/// for tool scripts and dispatches to [`run_once`] or [`run_with_retry`].
pub fn run_impl(args: &Args, cwd: &Path, root: &Path, flow_ci_running: bool) -> (Value, i32) {
    if flow_ci_running {
        return (
            json!({
                "status": "ok",
                "skipped": true,
                "reason": "recursion guard",
            }),
            0,
        );
    }

    if let Err(msg) = crate::cwd_scope::enforce(cwd, root) {
        return (json!({"status": "error", "message": msg}), 1);
    }

    let resolved_branch = crate::git::resolve_branch_in(args.branch.as_deref(), cwd, root);
    let selected = args.selected_phase();

    // Sentinel skip check — only when running all four phases.
    // Single-phase runs (--format/--lint/--build/--test) bypass the
    // sentinel because one tool passing does not satisfy the
    // all-four-passed contract the sentinel encodes.
    if selected.is_none() && !args.force {
        if let Some(ref branch) = resolved_branch {
            let snapshot = tree_snapshot(cwd, args.simulate_branch.as_deref());
            let sentinel = sentinel_path(root, branch);
            if sentinel.exists() {
                if let Ok(content) = fs::read_to_string(&sentinel) {
                    if content == snapshot {
                        return (
                            json!({
                                "status": "ok",
                                "skipped": true,
                                "reason": "no changes since last CI pass",
                            }),
                            0,
                        );
                    }
                }
            }
        }
    }

    let mut tools = bin_tool_sequence(cwd);
    if let Some(phase) = selected {
        tools.retain(|t| t.name == phase);
        if tools.is_empty() {
            return (
                json!({
                    "status": "error",
                    "message": format!(
                        "No ./bin/{} script found. Either create it or run /flow:flow-prime to install a stub.",
                        phase
                    ),
                }),
                1,
            );
        }
        // Forward trailing args to the single tool. retain leaves the
        // matching CiTool with empty args from bin_tool_sequence; we
        // extend it with whatever the user passed after the flag (e.g.
        // `--test -- hooks` → `["--", "hooks"]` becomes args; `--test
        // --file path` → `["--file", "path"]`).
        if !args.trailing.is_empty() {
            tools[0].args.extend(args.trailing.iter().cloned());
        }
        // When --audit is set, inject `--audit` as the first arg to
        // bin/test so the runner switches to collect-don't-fail-fast
        // mode. bin/test handles --audit at any position; placing it
        // first keeps the forwarded trailing args (test filters, etc.)
        // undisturbed.
        if args.audit && phase == "test" {
            tools[0].args.insert(0, "--audit".to_string());
        }
    }

    // For single-phase runs, pass branch=None to disable sentinel
    // writes inside run_once/run_with_retry. The all-four-passed
    // contract is the only thing the sentinel records.
    let sentinel_branch = if selected.is_some() {
        None
    } else {
        resolved_branch.as_deref()
    };

    // Force-rebuild semantics: only `--build` sets FLOW_CI_REBUILD=1 on
    // the spawned child. format/lint/test caches are correct and
    // re-running them from scratch wastes time; cargo build is the one
    // phase where the user explicitly wants a clean recompile.
    let rebuild = matches!(selected, Some("build"));

    if args.retry > 0 {
        run_with_retry(
            cwd,
            root,
            &tools,
            sentinel_branch,
            args.retry,
            args.simulate_branch.as_deref(),
            rebuild,
        )
    } else {
        // Force=true since we already checked the sentinel above.
        run_once(
            cwd,
            root,
            &tools,
            sentinel_branch,
            true,
            args.simulate_branch.as_deref(),
            rebuild,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn init_git_repo(dir: &Path, initial_branch: &str) {
        let run = |args: &[&str]| {
            let output = Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .expect("git command failed");
            assert!(output.status.success(), "git {:?} failed", args);
        };
        run(&["init", "--initial-branch", initial_branch]);
        run(&["config", "user.email", "test@test.com"]);
        run(&["config", "user.name", "Test"]);
        run(&["config", "commit.gpgsign", "false"]);
        run(&["commit", "--allow-empty", "-m", "init"]);
    }

    // --- tree_snapshot tests ---

    /// Exercises lines 175-184 of `format_elapsed` — three formatting
    /// branches: sub-1s ms, sub-1m fractional seconds, ≥1m
    /// minutes-and-seconds. The minutes branch is the regression-prone
    /// one (only fires on long CI runs).
    #[test]
    fn format_elapsed_under_one_second_uses_ms() {
        assert_eq!(format_elapsed(0), "0ms");
        assert_eq!(format_elapsed(999), "999ms");
    }

    #[test]
    fn format_elapsed_under_one_minute_uses_fractional_seconds() {
        assert_eq!(format_elapsed(1_000), "1.0s");
        assert_eq!(format_elapsed(38_600), "38.6s");
        assert_eq!(format_elapsed(59_999), "60.0s");
    }

    #[test]
    fn format_elapsed_one_minute_and_above_uses_minutes_seconds() {
        assert_eq!(format_elapsed(60_000), "1m0s");
        assert_eq!(format_elapsed(125_000), "2m5s");
        assert_eq!(format_elapsed(3_605_000), "60m5s");
    }

    /// Exercises line 191 — early return when the phase list is empty.
    /// `eprint_summary` writes to stderr so the test asserts no panic
    /// and trusts the early-return contract via `eprint_summary` being
    /// a noop for the empty input.
    #[test]
    fn eprint_summary_empty_phases_is_noop() {
        // No assertion target other than "does not panic" — the function
        // returns `()` and writes nothing.
        eprint_summary(&[], 0);
    }

    #[test]
    fn tree_snapshot_empty_repo_returns_64_char_hex() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path(), "main");
        let hash = tree_snapshot(dir.path(), None);
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(hash.chars().all(|c| !c.is_ascii_uppercase()));
    }

    #[test]
    fn tree_snapshot_deterministic() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path(), "main");
        let a = tree_snapshot(dir.path(), None);
        let b = tree_snapshot(dir.path(), None);
        assert_eq!(a, b);
    }

    #[test]
    fn tree_snapshot_differs_on_tracked_edit() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path(), "main");
        fs::write(dir.path().join("app.py"), "version = 1\n").unwrap();
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "add app"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let baseline = tree_snapshot(dir.path(), None);
        fs::write(dir.path().join("app.py"), "version = 2\n").unwrap();
        let after = tree_snapshot(dir.path(), None);
        assert_ne!(baseline, after);
    }

    #[test]
    fn tree_snapshot_differs_on_untracked_add() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path(), "main");
        let baseline = tree_snapshot(dir.path(), None);
        fs::write(dir.path().join("new.txt"), "hello\n").unwrap();
        let after = tree_snapshot(dir.path(), None);
        assert_ne!(baseline, after);
    }

    #[test]
    fn tree_snapshot_untracked_content_edit_changes_hash() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path(), "main");
        fs::write(dir.path().join("notes.txt"), "draft 1\n").unwrap();
        let first = tree_snapshot(dir.path(), None);
        fs::write(dir.path().join("notes.txt"), "draft 2\n").unwrap();
        let second = tree_snapshot(dir.path(), None);
        assert_ne!(first, second);
    }

    #[test]
    fn tree_snapshot_untracked_rename_changes_hash() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path(), "main");
        fs::write(dir.path().join("old.txt"), "content\n").unwrap();
        let first = tree_snapshot(dir.path(), None);
        fs::rename(dir.path().join("old.txt"), dir.path().join("new.txt")).unwrap();
        let second = tree_snapshot(dir.path(), None);
        assert_ne!(first, second);
    }

    #[test]
    fn write_or_remove_sentinel_removes_on_any_stub() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sentinel");
        fs::write(&path, "old").unwrap();
        write_or_remove_sentinel(&path, "new", true);
        assert!(!path.exists());
    }

    #[test]
    fn write_or_remove_sentinel_writes_on_not_stub() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subdir").join("sentinel");
        write_or_remove_sentinel(&path, "snapshot", false);
        assert_eq!(fs::read_to_string(&path).unwrap(), "snapshot");
    }

    #[test]
    fn write_or_remove_sentinel_handles_parentless_path() {
        // `Path::new("").parent()` returns None — exercises the None
        // arm of the `if let Some(parent)` guard. fs::write on "" is
        // expected to fail; errors are swallowed by design.
        let empty = Path::new("");
        write_or_remove_sentinel(empty, "snap", false);
    }

    #[test]
    fn program_stdout_missing_binary_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            program_stdout(dir.path(), "/no/such/program-deadbeef", &[]),
            ""
        );
    }

    #[test]
    fn tree_snapshot_simulate_branch_changes_hash() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path(), "main");
        let plain = tree_snapshot(dir.path(), None);
        let simulated = tree_snapshot(dir.path(), Some("other-branch"));
        assert_ne!(plain, simulated);
    }

    #[test]
    fn tree_snapshot_simulate_branch_deterministic() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path(), "main");
        let a = tree_snapshot(dir.path(), Some("feature-x"));
        let b = tree_snapshot(dir.path(), Some("feature-x"));
        assert_eq!(a, b);
    }

    #[test]
    fn tree_snapshot_different_simulate_values_differ() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path(), "main");
        let a = tree_snapshot(dir.path(), Some("branch-a"));
        let b = tree_snapshot(dir.path(), Some("branch-b"));
        assert_ne!(a, b);
    }

    #[test]
    fn tree_snapshot_non_git_dir_returns_stable_hash() {
        let dir = tempfile::tempdir().unwrap();
        let a = tree_snapshot(dir.path(), None);
        let b = tree_snapshot(dir.path(), None);
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }

    // --- CiTool fixture helpers ---

    /// Create a bash script at `path` with given content and make it executable.
    fn write_script(path: &Path, content: &str) {
        use std::os::unix::fs::PermissionsExt;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// CI fixture: a git repo with a configurable tool sequence.
    struct CiFixture {
        _dir: tempfile::TempDir,
        path: PathBuf,
        branch: String,
    }

    fn make_ci_fixture() -> CiFixture {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        init_git_repo(&path, "main");

        let exclude_file = path.join(".git").join("info").join("exclude");
        fs::create_dir_all(exclude_file.parent().unwrap()).unwrap();
        fs::write(&exclude_file, ".flow-states/\n").unwrap();

        CiFixture {
            _dir: dir,
            path,
            branch: "main".to_string(),
        }
    }

    /// Build a single-tool CiTool pointing at a bash script.
    fn single_tool(script_path: &Path) -> Vec<CiTool> {
        vec![CiTool {
            name: "test".to_string(),
            program: script_path.to_string_lossy().to_string(),
            args: vec![],
        }]
    }

    fn fixture_sentinel(f: &CiFixture) -> PathBuf {
        sentinel_path(&f.path, &f.branch)
    }

    // --- bin_tool_sequence tests ---

    #[test]
    fn bin_tool_sequence_empty_when_no_scripts() {
        let f = make_ci_fixture();
        let tools = bin_tool_sequence(&f.path);
        assert!(tools.is_empty());
    }

    #[test]
    fn bin_tool_sequence_picks_up_present_scripts() {
        let f = make_ci_fixture();
        write_script(
            &f.path.join("bin").join("format"),
            "#!/usr/bin/env bash\nexit 0\n",
        );
        write_script(
            &f.path.join("bin").join("test"),
            "#!/usr/bin/env bash\nexit 0\n",
        );
        let tools = bin_tool_sequence(&f.path);
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "format");
        assert_eq!(tools[1].name, "test");
    }

    #[test]
    fn bin_tool_sequence_preserves_order() {
        let f = make_ci_fixture();
        for name in ["test", "build", "lint", "format"] {
            write_script(
                &f.path.join("bin").join(name),
                "#!/usr/bin/env bash\nexit 0\n",
            );
        }
        let tools = bin_tool_sequence(&f.path);
        assert_eq!(tools.len(), 4);
        assert_eq!(tools[0].name, "format");
        assert_eq!(tools[1].name, "lint");
        assert_eq!(tools[2].name, "build");
        assert_eq!(tools[3].name, "test");
    }

    #[test]
    fn bin_tool_sequence_skips_directories() {
        let f = make_ci_fixture();
        // bin/format is a directory, not a file — should be skipped
        fs::create_dir_all(f.path.join("bin").join("format")).unwrap();
        write_script(
            &f.path.join("bin").join("test"),
            "#!/usr/bin/env bash\nexit 0\n",
        );
        let tools = bin_tool_sequence(&f.path);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "test");
    }

    // --- run_once tests ---

    #[test]
    fn run_once_runs_tools_and_creates_sentinel() {
        let f = make_ci_fixture();
        let script = f.path.join("pass.sh");
        write_script(&script, "#!/usr/bin/env bash\nexit 0\n");
        let tools = single_tool(&script);

        let (out, code) = run_once(
            &f.path,
            &f.path,
            &tools,
            Some(&f.branch),
            false,
            None,
            false,
        );
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert_eq!(out["skipped"], false);
        assert!(fixture_sentinel(&f).exists());
    }

    /// Exercises lines 354 and 357 — `run_once` propagates
    /// `FLOW_CI_REBUILD=1` and `FLOW_SIMULATE_BRANCH=<sim>` to the
    /// spawned tool when `rebuild=true` and `simulate_branch=Some`.
    /// The script writes its env-var visibility to a marker file the
    /// test then reads back to confirm propagation.
    #[test]
    fn run_once_propagates_rebuild_and_simulate_branch_env() {
        let f = make_ci_fixture();
        let marker = f.path.join("env-marker");
        let script = f.path.join("env-probe.sh");
        write_script(
            &script,
            &format!(
                "#!/usr/bin/env bash\nprintf 'rebuild=%s sim=%s\\n' \"${{FLOW_CI_REBUILD:-}}\" \"${{FLOW_SIMULATE_BRANCH:-}}\" > {}\nexit 0\n",
                marker.display()
            ),
        );
        let tools = single_tool(&script);

        let (out, code) = run_once(
            &f.path,
            &f.path,
            &tools,
            Some(&f.branch),
            true,
            Some("simulated-feature"),
            true,
        );
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");

        let env_dump = std::fs::read_to_string(&marker).unwrap();
        assert!(
            env_dump.contains("rebuild=1"),
            "FLOW_CI_REBUILD not propagated; got: {}",
            env_dump
        );
        assert!(
            env_dump.contains("sim=simulated-feature"),
            "FLOW_SIMULATE_BRANCH not propagated; got: {}",
            env_dump
        );
    }

    #[test]
    fn run_once_skips_when_sentinel_and_clean() {
        let f = make_ci_fixture();
        let script = f.path.join("pass.sh");
        write_script(&script, "#!/usr/bin/env bash\nexit 0\n");
        let tools = single_tool(&script);

        let (first, _) = run_once(
            &f.path,
            &f.path,
            &tools,
            Some(&f.branch),
            false,
            None,
            false,
        );
        assert_eq!(first["skipped"], false);

        let (second, code) = run_once(
            &f.path,
            &f.path,
            &tools,
            Some(&f.branch),
            false,
            None,
            false,
        );
        assert_eq!(code, 0);
        assert_eq!(second["skipped"], true);
    }

    #[test]
    fn run_once_sentinel_different_content_falls_through() {
        // Sentinel exists with content that does not match the current
        // snapshot → run_once proceeds rather than skipping.
        let f = make_ci_fixture();
        let script = f.path.join("pass.sh");
        write_script(&script, "#!/usr/bin/env bash\nexit 0\n");
        let tools = single_tool(&script);

        // Pre-write a stale sentinel.
        let sentinel = fixture_sentinel(&f);
        fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
        fs::write(&sentinel, "stale-content-that-wont-match").unwrap();

        let (out, code) = run_once(
            &f.path,
            &f.path,
            &tools,
            Some(&f.branch),
            false,
            None,
            false,
        );
        assert_eq!(code, 0);
        assert_eq!(
            out["skipped"], false,
            "stale sentinel must not short-circuit"
        );
    }

    #[test]
    fn run_once_sentinel_unreadable_falls_through() {
        // Sentinel file with no read permissions → fs::read_to_string
        // returns Err → run_once proceeds rather than skipping.
        use std::os::unix::fs::PermissionsExt;
        let f = make_ci_fixture();
        let script = f.path.join("pass.sh");
        write_script(&script, "#!/usr/bin/env bash\nexit 0\n");
        let tools = single_tool(&script);

        let sentinel = fixture_sentinel(&f);
        fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
        fs::write(&sentinel, "anything").unwrap();
        fs::set_permissions(&sentinel, fs::Permissions::from_mode(0o000)).unwrap();

        let (out, code) = run_once(
            &f.path,
            &f.path,
            &tools,
            Some(&f.branch),
            false,
            None,
            false,
        );
        fs::set_permissions(&sentinel, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(code, 0);
        assert_eq!(out["skipped"], false);
    }

    #[test]
    fn run_once_failure_removes_sentinel() {
        let f = make_ci_fixture();
        let pass = f.path.join("pass.sh");
        write_script(&pass, "#!/usr/bin/env bash\nexit 0\n");
        let tools = single_tool(&pass);

        // Create sentinel
        let _ = run_once(
            &f.path,
            &f.path,
            &tools,
            Some(&f.branch),
            false,
            None,
            false,
        );
        assert!(fixture_sentinel(&f).exists());

        // Replace with failing tool
        let fail = f.path.join("fail.sh");
        write_script(&fail, "#!/usr/bin/env bash\nexit 1\n");
        let fail_tools = single_tool(&fail);

        let (out, code) = run_once(
            &f.path,
            &f.path,
            &fail_tools,
            Some(&f.branch),
            true,
            None,
            false,
        );
        assert_eq!(code, 1);
        assert_eq!(out["status"], "error");
        assert!(!fixture_sentinel(&f).exists());
    }

    #[test]
    fn run_once_force_bypasses_sentinel() {
        let f = make_ci_fixture();
        let script = f.path.join("pass.sh");
        write_script(&script, "#!/usr/bin/env bash\nexit 0\n");
        let tools = single_tool(&script);

        let (first, _) = run_once(
            &f.path,
            &f.path,
            &tools,
            Some(&f.branch),
            false,
            None,
            false,
        );
        assert_eq!(first["skipped"], false);

        let (second, code) = run_once(&f.path, &f.path, &tools, Some(&f.branch), true, None, false);
        assert_eq!(code, 0);
        assert_eq!(second["skipped"], false);
    }

    #[test]
    fn run_once_stops_on_first_tool_failure() {
        let f = make_ci_fixture();
        let fail = f.path.join("fail.sh");
        write_script(&fail, "#!/usr/bin/env bash\nexit 1\n");
        let pass = f.path.join("pass.sh");
        write_script(&pass, "#!/usr/bin/env bash\nexit 0\n");

        // marker file proves second tool never ran
        let marker = f.path.join("second-ran");
        let mark_script = f.path.join("mark.sh");
        write_script(
            &mark_script,
            &format!("#!/usr/bin/env bash\ntouch {}\nexit 0\n", marker.display()),
        );

        let tools = vec![
            CiTool {
                name: "format".to_string(),
                program: fail.to_string_lossy().to_string(),
                args: vec![],
            },
            CiTool {
                name: "test".to_string(),
                program: mark_script.to_string_lossy().to_string(),
                args: vec![],
            },
        ];

        let (out, code) = run_once(
            &f.path,
            &f.path,
            &tools,
            Some(&f.branch),
            false,
            None,
            false,
        );
        assert_eq!(code, 1);
        assert_eq!(out["status"], "error");
        assert!(out["message"].as_str().unwrap().contains("format"));
        assert!(!marker.exists(), "second tool should not have run");
    }

    #[test]
    fn run_once_empty_tools_errors() {
        // A repo with no bin/{format,lint,build,test} scripts must
        // fail CI loudly — an "ok skipped" result would silently pass
        // every commit in a non-primed project.
        let f = make_ci_fixture();
        let (out, code) = run_once(&f.path, &f.path, &[], Some(&f.branch), false, None, false);
        assert_eq!(code, 1);
        assert_eq!(out["status"], "error");
        assert!(out["message"]
            .as_str()
            .unwrap()
            .contains("No ./bin/{format,lint,build,test} scripts"));
    }

    #[test]
    fn run_with_retry_empty_tools_errors() {
        // Mirror [`run_once_empty_tools_errors`]: retry mode must not
        // cache a useless sentinel when there are no tools to run.
        let f = make_ci_fixture();
        let (out, code) = run_with_retry(&f.path, &f.path, &[], Some(&f.branch), 3, None, false);
        assert_eq!(code, 1);
        assert_eq!(out["status"], "error");
        assert!(out.get("skipped").is_none());
        assert!(out.get("attempts").is_none());
        assert!(!fixture_sentinel(&f).exists());
    }

    #[test]
    fn run_once_stub_script_suppresses_sentinel() {
        // An unconfigured stub (identified by the FLOW-STUB-UNCONFIGURED
        // marker) must not cause a sentinel write. Otherwise the stub's
        // stderr reminder would be invisible on the next CI run and the
        // user could ship code with no real gate.
        let f = make_ci_fixture();
        let script = f.path.join("stub.sh");
        write_script(
            &script,
            "#!/usr/bin/env bash\n# FLOW-STUB-UNCONFIGURED (remove this line)\necho 'stub' >&2\nexit 0\n",
        );
        let tools = single_tool(&script);

        let (out, code) = run_once(
            &f.path,
            &f.path,
            &tools,
            Some(&f.branch),
            false,
            None,
            false,
        );
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert_eq!(out["stubs_detected"], true);
        assert!(
            !fixture_sentinel(&f).exists(),
            "sentinel must not be written when any tool is a stub"
        );
    }

    #[test]
    fn run_with_retry_stub_script_suppresses_sentinel() {
        let f = make_ci_fixture();
        let script = f.path.join("stub.sh");
        write_script(
            &script,
            "#!/usr/bin/env bash\n# FLOW-STUB-UNCONFIGURED (remove this line)\necho 'stub' >&2\nexit 0\n",
        );
        let tools = single_tool(&script);

        let (out, code) = run_with_retry(&f.path, &f.path, &tools, Some(&f.branch), 3, None, false);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert_eq!(out["stubs_detected"], true);
        assert!(
            !fixture_sentinel(&f).exists(),
            "sentinel must not be written when any tool is a stub"
        );
    }

    #[test]
    fn run_once_detached_head_no_sentinel() {
        let f = make_ci_fixture();
        let script = f.path.join("pass.sh");
        write_script(&script, "#!/usr/bin/env bash\nexit 0\n");
        let tools = single_tool(&script);

        // Pre-create .flow-states/ with an unrelated marker so the iter+
        // filter chain always runs against a real directory. The
        // assertion is that NO entry ending in "-ci-passed" exists; the
        // unrelated marker has a different suffix and must not match.
        let flow_states = f.path.join(".flow-states");
        fs::create_dir_all(&flow_states).unwrap();
        fs::write(flow_states.join("unrelated-marker.txt"), "x").unwrap();

        let (out, code) = run_once(&f.path, &f.path, &tools, None, false, None, false);
        assert_eq!(code, 0);
        assert_eq!(out["skipped"], false);
        let entries: Vec<_> = fs::read_dir(&flow_states)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with("-ci-passed"))
            .collect();
        assert!(entries.is_empty(), "no sentinel expected");
    }

    // --- run_with_retry tests ---

    #[test]
    fn retry_pass_first_attempt() {
        let f = make_ci_fixture();
        let script = f.path.join("pass.sh");
        write_script(&script, "#!/usr/bin/env bash\nexit 0\n");
        let tools = single_tool(&script);

        let (out, code) = run_with_retry(&f.path, &f.path, &tools, Some(&f.branch), 3, None, false);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert_eq!(out["attempts"], 1);
        assert!(out.get("flaky").is_none());
        assert!(fixture_sentinel(&f).exists());
    }

    #[test]
    fn retry_flaky() {
        let f = make_ci_fixture();
        let script = f.path.join("flaky.sh");
        write_script(
            &script,
            &format!(
                r#"#!/usr/bin/env bash
COUNTER_FILE="{}/counter"
if [ -f "$COUNTER_FILE" ]; then
  COUNT=$(($(cat "$COUNTER_FILE") + 1))
else
  COUNT=1
fi
echo "$COUNT" > "$COUNTER_FILE"
if [ "$COUNT" -lt 2 ]; then
  echo "FAIL: flaky" >&2
  exit 1
fi
exit 0
"#,
                f.path.display()
            ),
        );
        let tools = single_tool(&script);

        let (out, code) = run_with_retry(&f.path, &f.path, &tools, Some(&f.branch), 3, None, false);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert_eq!(out["attempts"], 2);
        assert_eq!(out["flaky"], true);
        let first_fail = out["first_failure_output"].as_str().unwrap();
        assert!(first_fail.contains("FAIL"));
    }

    #[test]
    fn retry_consistent_failure() {
        let f = make_ci_fixture();
        let script = f.path.join("fail.sh");
        write_script(
            &script,
            "#!/usr/bin/env bash\necho 'CI FAILED' >&2\nexit 1\n",
        );
        let tools = single_tool(&script);

        let (out, code) = run_with_retry(&f.path, &f.path, &tools, Some(&f.branch), 3, None, false);
        assert_eq!(code, 1);
        assert_eq!(out["status"], "error");
        assert_eq!(out["attempts"], 3);
        assert_eq!(out["consistent"], true);
        assert!(out["output"].as_str().unwrap().contains("CI FAILED"));
    }

    // --- run_impl tests ---

    fn default_args() -> Args {
        Args {
            force: false,
            retry: 0,
            branch: None,
            simulate_branch: None,
            format: false,
            lint: false,
            build: false,
            test: false,
            audit: false,
            trailing: Vec::new(),
        }
    }

    #[test]
    fn cli_recursion_guard() {
        let f = make_ci_fixture();
        let args = Args {
            branch: Some(f.branch.clone()),
            ..default_args()
        };
        let (out, code) = run_impl(&args, &f.path, &f.path, true);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert_eq!(out["skipped"], true);
        assert_eq!(out["reason"], "recursion guard");
    }

    #[test]
    fn run_impl_no_bin_scripts_returns_error() {
        // A repo with no bin/{format,lint,build,test} scripts is not
        // primed (or its prime was rolled back). run_impl must error
        // so the caller sees the actionable message.
        let f = make_ci_fixture();
        let args = Args {
            branch: Some(f.branch.clone()),
            force: true,
            ..default_args()
        };
        let (out, code) = run_impl(&args, &f.path, &f.path, false);
        assert_eq!(code, 1);
        assert_eq!(out["status"], "error");
        assert!(out["message"]
            .as_str()
            .unwrap()
            .contains("No ./bin/{format,lint,build,test} scripts"));
    }

    #[test]
    fn run_impl_runs_present_bin_scripts() {
        let f = make_ci_fixture();
        write_script(
            &f.path.join("bin").join("format"),
            "#!/usr/bin/env bash\nexit 0\n",
        );
        let args = Args {
            branch: Some(f.branch.clone()),
            force: true,
            ..default_args()
        };
        let (out, code) = run_impl(&args, &f.path, &f.path, false);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert_eq!(out["skipped"], false);
    }

    // --- run_impl retry dispatch ---

    #[test]
    fn run_impl_retry_dispatches_to_retry_path() {
        // run_impl with retry > 0 must dispatch to run_with_retry.
        // The "attempts" field in the output is only produced by
        // run_with_retry, so its presence proves the dispatch.
        let f = make_ci_fixture();
        write_script(
            &f.path.join("bin").join("format"),
            "#!/usr/bin/env bash\nexit 0\n",
        );
        let args = Args {
            branch: Some(f.branch.clone()),
            force: false,
            retry: 2,
            simulate_branch: None,
            ..default_args()
        };
        let (out, code) = run_impl(&args, &f.path, &f.path, false);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert!(
            out.get("attempts").is_some(),
            "retry dispatch must produce 'attempts' field"
        );
        assert_eq!(out["attempts"], 1);
    }

    #[test]
    fn run_impl_retry_with_sentinel_skips_before_dispatch() {
        // When retry > 0 but a matching sentinel exists and force is
        // false, run_impl returns "skipped" without dispatching to
        // run_with_retry. The sentinel check at line 470-488 runs
        // before the retry dispatch at line 493.
        let f = make_ci_fixture();
        write_script(
            &f.path.join("bin").join("format"),
            "#!/usr/bin/env bash\nexit 0\n",
        );
        // First run: create the sentinel
        let args_first = Args {
            branch: Some(f.branch.clone()),
            force: false,
            retry: 0,
            simulate_branch: None,
            ..default_args()
        };
        let (first_out, _) = run_impl(&args_first, &f.path, &f.path, false);
        assert_eq!(first_out["skipped"], false);
        assert!(fixture_sentinel(&f).exists());

        // Second run: retry > 0 but sentinel matches → skip
        let args_retry = Args {
            branch: Some(f.branch.clone()),
            force: false,
            retry: 2,
            simulate_branch: None,
            ..default_args()
        };
        let (out, code) = run_impl(&args_retry, &f.path, &f.path, false);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert_eq!(out["skipped"], true);
        assert_eq!(out["reason"], "no changes since last CI pass");
        // No "attempts" field — run_with_retry was never called
        assert!(
            out.get("attempts").is_none(),
            "sentinel skip must prevent retry dispatch"
        );
    }

    // --- run_with_retry inner-loop failure ---

    #[test]
    fn retry_tool_failure_mid_sequence() {
        // Two tools: first passes, second fails. With retry=2, both
        // attempts fail at tool 2 → consistent failure with output
        // captured from the second tool.
        let f = make_ci_fixture();
        let pass = f.path.join("pass.sh");
        write_script(&pass, "#!/usr/bin/env bash\nexit 0\n");
        let fail = f.path.join("fail.sh");
        write_script(
            &fail,
            "#!/usr/bin/env bash\necho 'TOOL2 FAILED' >&2\nexit 1\n",
        );
        let tools = vec![
            CiTool {
                name: "format".to_string(),
                program: pass.to_string_lossy().to_string(),
                args: vec![],
            },
            CiTool {
                name: "test".to_string(),
                program: fail.to_string_lossy().to_string(),
                args: vec![],
            },
        ];
        let (out, code) = run_with_retry(&f.path, &f.path, &tools, Some(&f.branch), 2, None, false);
        assert_eq!(code, 1);
        assert_eq!(out["consistent"], true);
        assert!(out["output"].as_str().unwrap().contains("TOOL2 FAILED"));
    }

    /// Exercises lines 497 and 500 — `run_with_retry` propagates
    /// `FLOW_CI_REBUILD=1` and `FLOW_SIMULATE_BRANCH=<sim>` to the
    /// spawned tool when `rebuild=true` and `simulate_branch=Some`.
    /// Mirror of `run_once_propagates_rebuild_and_simulate_branch_env`.
    #[test]
    fn run_with_retry_propagates_rebuild_and_simulate_branch_env() {
        let f = make_ci_fixture();
        let marker = f.path.join("retry-env-marker");
        let script = f.path.join("retry-env-probe.sh");
        write_script(
            &script,
            &format!(
                "#!/usr/bin/env bash\nprintf 'rebuild=%s sim=%s\\n' \"${{FLOW_CI_REBUILD:-}}\" \"${{FLOW_SIMULATE_BRANCH:-}}\" > {}\nexit 0\n",
                marker.display()
            ),
        );
        let tools = single_tool(&script);

        let (out, code) = run_with_retry(
            &f.path,
            &f.path,
            &tools,
            Some(&f.branch),
            1,
            Some("retry-feature"),
            true,
        );
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");

        let env_dump = std::fs::read_to_string(&marker).unwrap();
        assert!(
            env_dump.contains("rebuild=1"),
            "FLOW_CI_REBUILD not propagated; got: {}",
            env_dump
        );
        assert!(
            env_dump.contains("sim=retry-feature"),
            "FLOW_SIMULATE_BRANCH not propagated; got: {}",
            env_dump
        );
    }

    #[test]
    fn retry_flaky_via_marker_file() {
        // A tool that fails on the first invocation (no marker file)
        // and succeeds on the second (marker file exists). This exercises
        // the flaky classification path where attempt > 1 succeeds.
        let f = make_ci_fixture();
        let marker = f.path.join("flaky-marker");
        let script = f.path.join("flaky-marker.sh");
        write_script(
            &script,
            &format!(
                r#"#!/usr/bin/env bash
MARKER="{}"
if [ -f "$MARKER" ]; then
  exit 0
else
  # `:` is a shell builtin (no fork). Avoids an extra subprocess
  # (`/usr/bin/touch`) that nextest's leak detector occasionally
  # flags when the touch process is briefly unreaped between
  # attempts under heavy parallel load.
  : > "$MARKER"
  echo "FIRST FAIL" >&2
  exit 1
fi
"#,
                marker.display()
            ),
        );
        let tools = single_tool(&script);
        let (out, code) = run_with_retry(&f.path, &f.path, &tools, Some(&f.branch), 3, None, false);
        assert_eq!(code, 0);
        assert_eq!(out["flaky"], true);
        assert_eq!(out["attempts"], 2);
        let first_fail = out["first_failure_output"].as_str().unwrap();
        assert!(first_fail.contains("FIRST FAIL"));
    }

    #[test]
    fn retry_all_attempts_fail_removes_sentinel() {
        // Pre-create a sentinel, then run retry with a failing tool.
        // All attempts fail → sentinel must be removed.
        let f = make_ci_fixture();
        let sentinel = fixture_sentinel(&f);
        fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
        fs::write(&sentinel, "stale-content").unwrap();
        assert!(sentinel.exists());

        let script = f.path.join("always-fail.sh");
        write_script(
            &script,
            "#!/usr/bin/env bash\necho 'ALWAYS FAIL' >&2\nexit 1\n",
        );
        let tools = single_tool(&script);
        let (out, code) = run_with_retry(&f.path, &f.path, &tools, Some(&f.branch), 2, None, false);
        assert_eq!(code, 1);
        assert_eq!(out["consistent"], true);
        assert!(
            !sentinel.exists(),
            "sentinel must be removed after all retry attempts fail"
        );
    }

    // --- stub/sentinel error paths ---

    #[test]
    fn any_tool_is_stub_unreadable_file() {
        // When a tool script cannot be read (e.g. permissions), the
        // function should return false (cannot confirm it's a stub).
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("tool.sh");
        write_script(
            &script,
            &format!("#!/usr/bin/env bash\n# {}\nexit 0\n", STUB_MARKER),
        );
        // Make unreadable
        fs::set_permissions(&script, fs::Permissions::from_mode(0o000)).unwrap();

        let tools = vec![CiTool {
            name: "test".to_string(),
            program: script.to_string_lossy().to_string(),
            args: vec![],
        }];
        let result = any_tool_is_stub(&tools);
        assert!(!result, "unreadable file should not be detected as stub");

        // Restore for cleanup
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn run_once_spawn_failure() {
        // Point tool to a non-existent executable → cmd.status() fails.
        let f = make_ci_fixture();
        let tools = vec![CiTool {
            name: "format".to_string(),
            program: "/nonexistent/path/to/tool".to_string(),
            args: vec![],
        }];
        let (out, code) = run_once(&f.path, &f.path, &tools, Some(&f.branch), true, None, false);
        assert_eq!(code, 1);
        assert_eq!(out["status"], "error");
        assert!(out["message"].as_str().unwrap().contains("failed to run"));
    }

    #[test]
    fn retry_spawn_failure() {
        // Same as run_once_spawn_failure but through run_with_retry.
        let f = make_ci_fixture();
        let tools = vec![CiTool {
            name: "format".to_string(),
            program: "/nonexistent/path/to/tool".to_string(),
            args: vec![],
        }];
        let (out, code) = run_with_retry(&f.path, &f.path, &tools, Some(&f.branch), 2, None, false);
        assert_eq!(code, 1);
        assert_eq!(out["status"], "error");
        assert!(out["message"].as_str().unwrap().contains("failed to run"));
    }

    // --- cwd_scope enforce error ---

    #[test]
    fn run_impl_cwd_scope_rejects_wrong_dir() {
        // Call run_impl with a cwd that doesn't match root. The
        // cwd_scope::enforce guard should return an error.
        let f = make_ci_fixture();
        let wrong_dir = tempfile::tempdir().unwrap();
        let args = Args {
            branch: Some(f.branch.clone()),
            force: true,
            ..default_args()
        };
        let (out, code) = run_impl(&args, wrong_dir.path(), &f.path, false);
        assert_eq!(code, 1);
        assert_eq!(out["status"], "error");
        assert!(!out["message"].as_str().unwrap().is_empty());
    }

    // --- Single-phase flag tests (--format/--lint/--build/--test) ---

    #[test]
    fn args_selected_phase_none_when_no_flag_set() {
        let args = default_args();
        assert_eq!(args.selected_phase(), None);
    }

    #[test]
    fn args_selected_phase_format() {
        let args = Args {
            format: true,
            ..default_args()
        };
        assert_eq!(args.selected_phase(), Some("format"));
    }

    #[test]
    fn args_selected_phase_lint() {
        let args = Args {
            lint: true,
            ..default_args()
        };
        assert_eq!(args.selected_phase(), Some("lint"));
    }

    #[test]
    fn args_selected_phase_build() {
        let args = Args {
            build: true,
            ..default_args()
        };
        assert_eq!(args.selected_phase(), Some("build"));
    }

    #[test]
    fn args_selected_phase_test() {
        let args = Args {
            test: true,
            ..default_args()
        };
        assert_eq!(args.selected_phase(), Some("test"));
    }

    #[test]
    fn run_impl_format_flag_runs_only_format() {
        let f = make_ci_fixture();
        // Install all four scripts, but only format should run.
        for name in ["format", "lint", "build", "test"] {
            let marker = f.path.join(format!("{}-ran", name));
            let marker_str = marker.to_string_lossy().to_string();
            write_script(
                &f.path.join("bin").join(name),
                &format!("#!/usr/bin/env bash\ntouch {}\nexit 0\n", marker_str),
            );
        }
        let args = Args {
            branch: Some(f.branch.clone()),
            format: true,
            ..default_args()
        };
        let (out, code) = run_impl(&args, &f.path, &f.path, false);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert!(f.path.join("format-ran").exists(), "format must have run");
        assert!(!f.path.join("lint-ran").exists(), "lint must not have run");
        assert!(
            !f.path.join("build-ran").exists(),
            "build must not have run"
        );
        assert!(!f.path.join("test-ran").exists(), "test must not have run");
    }

    #[test]
    fn run_impl_test_flag_runs_only_test() {
        let f = make_ci_fixture();
        for name in ["format", "lint", "build", "test"] {
            let marker = f.path.join(format!("{}-ran", name));
            let marker_str = marker.to_string_lossy().to_string();
            write_script(
                &f.path.join("bin").join(name),
                &format!("#!/usr/bin/env bash\ntouch {}\nexit 0\n", marker_str),
            );
        }
        let args = Args {
            branch: Some(f.branch.clone()),
            test: true,
            ..default_args()
        };
        let (out, code) = run_impl(&args, &f.path, &f.path, false);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert!(f.path.join("test-ran").exists(), "test must have run");
        assert!(
            !f.path.join("format-ran").exists(),
            "format must not have run"
        );
    }

    #[test]
    fn run_impl_format_flag_missing_script_returns_specific_error() {
        let f = make_ci_fixture();
        // No bin/format installed.
        let args = Args {
            branch: Some(f.branch.clone()),
            format: true,
            ..default_args()
        };
        let (out, code) = run_impl(&args, &f.path, &f.path, false);
        assert_eq!(code, 1);
        assert_eq!(out["status"], "error");
        let msg = out["message"].as_str().unwrap();
        assert!(
            msg.contains("./bin/format script"),
            "error must name the missing script: {}",
            msg
        );
    }

    #[test]
    fn run_impl_single_phase_does_not_write_sentinel() {
        // A passing single-phase run must NOT write the all-four-passed
        // sentinel — because a single tool passing does not satisfy the
        // contract the sentinel encodes.
        let f = make_ci_fixture();
        write_script(
            &f.path.join("bin").join("format"),
            "#!/usr/bin/env bash\nexit 0\n",
        );
        let args = Args {
            branch: Some(f.branch.clone()),
            format: true,
            ..default_args()
        };
        let (out, code) = run_impl(&args, &f.path, &f.path, false);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert!(
            !fixture_sentinel(&f).exists(),
            "single-phase run must not write the all-passed sentinel"
        );
    }

    #[test]
    fn run_impl_single_phase_ignores_existing_sentinel() {
        // A pre-existing matching sentinel must NOT short-circuit a
        // single-phase run. The user explicitly asked to run that tool;
        // honoring an old all-passed sentinel would silently skip it.
        let f = make_ci_fixture();
        write_script(
            &f.path.join("bin").join("format"),
            "#!/usr/bin/env bash\nexit 0\n",
        );
        // First write the sentinel via a normal full run.
        let full = Args {
            branch: Some(f.branch.clone()),
            ..default_args()
        };
        let _ = run_impl(&full, &f.path, &f.path, false);
        assert!(fixture_sentinel(&f).exists());

        // Now request --format only: should run, not skip.
        let args = Args {
            branch: Some(f.branch.clone()),
            format: true,
            ..default_args()
        };
        let (out, code) = run_impl(&args, &f.path, &f.path, false);
        assert_eq!(code, 0);
        assert_eq!(out["skipped"], false);
    }

    #[test]
    fn run_impl_build_flag_sets_rebuild_env() {
        // --build selects the build phase and also flips `rebuild=true`
        // in run_impl, which propagates to FLOW_CI_REBUILD=1 in the
        // child env. Without this test, the `matches!(selected,
        // Some("build"))` true arm is never executed.
        let f = make_ci_fixture();
        let marker = f.path.join("build-rebuild-marker");
        let build_script = f.path.join("bin").join("build");
        write_script(
            &build_script,
            &format!(
                "#!/usr/bin/env bash\nif [ -n \"${{FLOW_CI_REBUILD:-}}\" ]; then echo rebuilt > {}; fi\nexit 0\n",
                marker.display()
            ),
        );
        let args = Args {
            branch: Some(f.branch.clone()),
            build: true,
            ..default_args()
        };
        let (out, code) = run_impl(&args, &f.path, &f.path, false);
        assert_eq!(code, 0, "out={}", out);
        assert!(
            marker.exists(),
            "--build must set FLOW_CI_REBUILD=1 in the child env"
        );
    }

    #[test]
    fn run_impl_sentinel_unreadable_falls_through_and_runs() {
        // A sentinel file that exists but cannot be read (e.g. perms)
        // must not short-circuit; the run proceeds normally.
        use std::os::unix::fs::PermissionsExt;
        let f = make_ci_fixture();
        write_script(
            &f.path.join("bin").join("format"),
            "#!/usr/bin/env bash\nexit 0\n",
        );
        // Pre-create sentinel with content that would match if readable,
        // then make it unreadable so fs::read_to_string returns Err.
        let sentinel = fixture_sentinel(&f);
        fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
        fs::write(&sentinel, "unreadable").unwrap();
        fs::set_permissions(&sentinel, fs::Permissions::from_mode(0o000)).unwrap();

        let args = Args {
            branch: Some(f.branch.clone()),
            ..default_args()
        };
        let (out, code) = run_impl(&args, &f.path, &f.path, false);
        // Restore before any assertion so tempdir cleans up.
        fs::set_permissions(&sentinel, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(code, 0, "out={}", out);
        assert_eq!(
            out["skipped"], false,
            "unreadable sentinel must not short-circuit"
        );
    }

    #[test]
    fn run_impl_trailing_args_forwarded_to_single_phase_tool() {
        // `--test -- arg1 arg2` passes ["arg1","arg2"] as tool args.
        // The script echoes its $@ into a marker file so the test can
        // confirm they were propagated.
        let f = make_ci_fixture();
        let marker = f.path.join("trailing-marker");
        write_script(
            &f.path.join("bin").join("test"),
            &format!(
                "#!/usr/bin/env bash\nprintf '%s\\n' \"$@\" > {}\nexit 0\n",
                marker.display()
            ),
        );
        let args = Args {
            branch: Some(f.branch.clone()),
            test: true,
            trailing: vec!["--".to_string(), "arg1".to_string()],
            ..default_args()
        };
        let (out, code) = run_impl(&args, &f.path, &f.path, false);
        assert_eq!(code, 0, "out={}", out);
        let dump = fs::read_to_string(&marker).unwrap();
        assert!(
            dump.contains("arg1"),
            "trailing args must reach tool: {}",
            dump
        );
    }

    #[test]
    fn run_impl_force_bypasses_sentinel_skip() {
        // --force must run all four phases even when the sentinel matches.
        let f = make_ci_fixture();
        write_script(
            &f.path.join("bin").join("format"),
            "#!/usr/bin/env bash\nexit 0\n",
        );
        // Seed the sentinel via a normal run.
        let full = Args {
            branch: Some(f.branch.clone()),
            ..default_args()
        };
        let (first, _) = run_impl(&full, &f.path, &f.path, false);
        assert_eq!(first["skipped"], false);

        // Second run without force should skip via sentinel.
        let (skipped_out, _) = run_impl(&full, &f.path, &f.path, false);
        assert_eq!(skipped_out["skipped"], true);

        // Third run WITH --force should not skip.
        let forced = Args {
            branch: Some(f.branch.clone()),
            force: true,
            ..default_args()
        };
        let (forced_out, code) = run_impl(&forced, &f.path, &f.path, false);
        assert_eq!(code, 0);
        assert_eq!(forced_out["skipped"], false);
    }
}
