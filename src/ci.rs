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
//!
//! Tests live at `tests/ci.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` in
//! this file.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use clap::Parser;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::flow_paths::{compute_worktree_root, FlowPaths};

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
    /// Branch-scoped full clean: remove this branch's CI sentinel,
    /// every `*.profraw` under `target/llvm-cov-target/`, and the
    /// `debug/deps/` and `debug/incremental/` subdirectories under
    /// `target/llvm-cov-target/`. Leaves `target/debug/flow-rs`
    /// intact so the `bin/flow` dispatcher doesn't trigger a rebuild.
    /// Short-circuits CI — no format/lint/build/test runs after.
    #[arg(long)]
    pub clean: bool,
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
pub const STUB_MARKER: &str = "FLOW-STUB-UNCONFIGURED";

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
pub fn eprint_summary(phases: &[(String, u64)], total_ms: u64) {
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
pub fn program_stdout(cwd: &Path, program: &str, args: &[&str]) -> String {
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
pub fn write_or_remove_sentinel(path: &Path, snapshot: &str, any_stub: bool) {
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

/// Recursively delete every `*.profraw` file under `dir`. Returns
/// `(count, bytes)` where `count` is the number of files successfully
/// removed and `bytes` is the sum of their sizes. A missing or
/// unreadable `dir` contributes zero. Errors on individual files are
/// swallowed — a partial sweep is strictly better than aborting on
/// the first failure.
///
/// Used by [`run_clean`] to eliminate the profraw pile-up that
/// accumulates across `bin/test` invocations (especially subprocess
/// tests that write profraws after `bin/test`'s start-of-run sweep).
pub fn delete_profraws_recursive(dir: &Path) -> (u64, u64) {
    let mut count: u64 = 0;
    let mut bytes: u64 = 0;
    let mut stack: Vec<PathBuf> = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = match fs::read_dir(&current) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // Single `metadata()` call gates everything: dangling
            // symlinks and other stat failures fall through to the
            // next entry, directories recurse into the stack, and
            // profraw files get their size accumulated AND removed
            // with no second metadata lookup.
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() && path.extension().map(|e| e == "profraw").unwrap_or(false) {
                bytes = bytes.saturating_add(meta.len());
                if fs::remove_file(&path).is_ok() {
                    count = count.saturating_add(1);
                }
            }
        }
    }
    (count, bytes)
}

/// Branch-scoped full clean. Used by `bin/flow ci --clean` and
/// directly testable.
///
/// Removes:
/// - `<root>/.flow-states/<branch>-ci-passed` — this branch's sentinel
/// - every `*.profraw` under `<cwd>/target/llvm-cov-target/` (recursive)
/// - `<cwd>/target/llvm-cov-target/debug/deps/`
/// - `<cwd>/target/llvm-cov-target/debug/incremental/`
///
/// Does NOT touch `<cwd>/target/debug/flow-rs` or
/// `<cwd>/target/release/flow-rs` — the dispatcher needs those to
/// avoid rebuilding itself on the next `bin/flow` invocation.
///
/// When `branch` is None and no branch can be resolved (detached HEAD,
/// bare clone), the sentinel step is a no-op. The remaining work still
/// runs because the profraws and compile artifacts are branch-free.
pub fn run_clean(cwd: &Path, root: &Path, branch: Option<&str>) -> (Value, i32) {
    let start = Instant::now();

    // Branch sentinel — strict-fallible: we never panic on invalid
    // branches; instead we treat "no valid branch" as "nothing to do
    // for the sentinel" and continue with the rest of the clean.
    let resolved = branch
        .map(|s| s.to_string())
        .or_else(|| crate::git::resolve_branch_in(None, cwd, root));
    let mut sentinel_removed = false;
    if let Some(ref b) = resolved {
        if let Some(paths) = FlowPaths::try_new(root, b) {
            let p = paths.ci_sentinel();
            if p.exists() && fs::remove_file(&p).is_ok() {
                sentinel_removed = true;
            }
        }
    }

    // Profraws — recursive under target/llvm-cov-target/. Safe even
    // when deps/ and incremental/ get nuked below (those dirs are
    // handled separately; this sweep catches anything elsewhere).
    let llvm_target = cwd.join("target").join("llvm-cov-target");
    let (profraw_count, profraw_bytes) = delete_profraws_recursive(&llvm_target);

    // Compile cache subdirs. `fs::remove_dir_all` on a missing path
    // returns Err, so we gate on existence to keep the boolean clean.
    let deps = llvm_target.join("debug").join("deps");
    let deps_removed = deps.exists() && fs::remove_dir_all(&deps).is_ok();

    let incremental = llvm_target.join("debug").join("incremental");
    let incremental_removed = incremental.exists() && fs::remove_dir_all(&incremental).is_ok();

    let elapsed_ms = start.elapsed().as_millis() as u64;
    eprintln!(
        "--- clean: sentinel={} profraws={} ({} bytes) deps={} incremental={} ({}) ---",
        sentinel_removed,
        profraw_count,
        profraw_bytes,
        deps_removed,
        incremental_removed,
        format_elapsed(elapsed_ms),
    );

    (
        json!({
            "status": "ok",
            "cleaned": {
                "branch": resolved,
                "sentinel_removed": sentinel_removed,
                "profraw_count": profraw_count,
                "profraw_bytes": profraw_bytes,
                "deps_removed": deps_removed,
                "incremental_removed": incremental_removed,
            },
            "elapsed_ms": elapsed_ms,
        }),
        0,
    )
}

/// Testable CLI entry point.
///
/// Checks the sentinel BEFORE building the tool sequence so callers like
/// `finalize_commit` skip instantly when the tree state is clean. When
/// the sentinel does not match (or force/retry mode), scans `cwd/bin/`
/// for tool scripts and dispatches to [`run_once`] or [`run_with_retry`].
pub fn run_impl(args: &Args, cwd: &Path, root: &Path, flow_ci_running: bool) -> (Value, i32) {
    if args.clean {
        return run_clean(cwd, root, args.branch.as_deref());
    }

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

    // Normalize cwd to the worktree root so monorepo-subdir flows
    // invoke the project's root-level `bin/{format,lint,build,test}`
    // scripts (which can dispatch by diff per project convention).
    // For non-worktree cwds and cwds already at the worktree root,
    // this is a no-op. All downstream consumers — `cwd_scope::enforce`,
    // `bin_tool_sequence`, `tree_snapshot`, sentinel handling —
    // benefit automatically.
    let cwd_str = cwd.to_string_lossy();
    let cwd_buf;
    let cwd: &Path = match compute_worktree_root(&cwd_str) {
        Some(root_str) if root_str.len() < cwd_str.len() => {
            cwd_buf = PathBuf::from(root_str);
            &cwd_buf
        }
        _ => cwd,
    };

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
        // `run_once`'s `force` parameter gates the sentinel skip-if-
        // matches check inside the function. The outer check at the top
        // of `run_impl` already returned early on match, so when
        // `args.force` is false the inner check is a redundant-but-
        // idempotent re-check that sees the same mismatch and proceeds.
        run_once(
            cwd,
            root,
            &tools,
            sentinel_branch,
            args.force,
            args.simulate_branch.as_deref(),
            rebuild,
        )
    }
}
