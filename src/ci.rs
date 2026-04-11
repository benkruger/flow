//! `bin/flow ci` — framework-aware CI orchestrator.
//!
//! Runs format → lint → build → test in sequence with dirty-check
//! optimization. Detects the project framework from the state file
//! or project marker files, resolves tool commands via
//! [`framework_tools`], and spawns each in order. Stops on the first
//! failure.
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
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use clap::Parser;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::framework_tools::{self, ToolType};

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
}

/// A tool in the CI sequence: name for display, program + args for spawning.
pub struct CiTool {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
}

/// Build the ordered CI tool sequence for a framework.
///
/// Returns format → lint → build → test, skipping no-ops.
/// Format runs first for fail-fast (instant check catches trivial errors
/// before compilation).
pub fn build_tool_sequence(framework: &str) -> Result<Vec<CiTool>, String> {
    let steps = [
        (ToolType::Format, "format"),
        (ToolType::Lint, "lint"),
        (ToolType::Build, "build"),
        (ToolType::Test, "test"),
    ];
    let mut tools = Vec::new();
    for (tool_type, name) in &steps {
        if let Some(cmd) = framework_tools::tool_command(framework, *tool_type)? {
            tools.push(CiTool {
                name: name.to_string(),
                program: cmd.program,
                args: cmd.args,
            });
        }
    }
    Ok(tools)
}

/// Build the sentinel file path for a given branch: `<root>/.flow-states/<branch>-ci-passed`.
///
/// Centralizes the naming convention so [`run_once`], [`run_with_retry`], and the
/// inline tests all agree on where sentinels live.
///
/// Also used by [`crate::finalize_commit::run_impl`] to refresh the sentinel after a clean commit.
pub fn sentinel_path(root: &Path, branch: &str) -> PathBuf {
    root.join(".flow-states")
        .join(format!("{}-ci-passed", branch))
}

/// Run a git command in `cwd`, returning its stdout as a lossy UTF-8 string.
fn git_stdout(cwd: &Path, args: &[&str]) -> String {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
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

    let untracked_hash = if !untracked_files.is_empty() {
        match Command::new("git")
            .args(["hash-object", "--stdin-paths"])
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
        {
            Ok(mut child) => {
                if let Some(stdin) = child.stdin.as_mut() {
                    let _ = stdin.write_all(untracked_files.as_bytes());
                }
                match child.wait_with_output() {
                    Ok(out) => String::from_utf8_lossy(&out.stdout).to_string(),
                    Err(_) => String::new(),
                }
            }
            Err(_) => String::new(),
        }
    } else {
        String::new()
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
) -> (Value, i32) {
    if tools.is_empty() {
        return (
            json!({"status": "ok", "skipped": true, "reason": "no CI tools for this framework"}),
            0,
        );
    }

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

    for tool in tools {
        let mut cmd = Command::new(&tool.program);
        cmd.args(&tool.args)
            .current_dir(cwd)
            .env("FLOW_CI_RUNNING", "1");
        if let Some(sim) = simulate_branch {
            cmd.env("FLOW_SIMULATE_BRANCH", sim);
        }

        let status = match cmd.status() {
            Ok(s) => s,
            Err(e) => {
                if let Some(ref path) = sentinel {
                    let _ = fs::remove_file(path);
                }
                return (
                    json!({
                        "status": "error",
                        "message": format!("failed to run {} ({}): {}", tool.name, tool.program, e),
                    }),
                    1,
                );
            }
        };

        if !status.success() {
            if let Some(ref path) = sentinel {
                let _ = fs::remove_file(path);
            }
            return (
                json!({"status": "error", "message": format!("{} failed", tool.name)}),
                1,
            );
        }
    }

    if let Some(ref path) = sentinel {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, &snapshot);
    }
    (json!({"status": "ok", "skipped": false}), 0)
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
) -> (Value, i32) {
    let sentinel = branch.map(|b| sentinel_path(root, b));
    let mut first_failure_output: Option<String> = None;

    for attempt in 1..=max_attempts {
        let mut attempt_failed = false;
        let mut attempt_output = String::new();

        for tool in tools {
            let mut cmd = Command::new(&tool.program);
            cmd.args(&tool.args)
                .current_dir(cwd)
                .env("FLOW_CI_RUNNING", "1");
            if let Some(sim) = simulate_branch {
                cmd.env("FLOW_SIMULATE_BRANCH", sim);
            }

            let output = match cmd.output() {
                Ok(o) => o,
                Err(e) => {
                    return (
                        json!({
                            "status": "error",
                            "message": format!("failed to run {} ({}): {}", tool.name, tool.program, e),
                        }),
                        1,
                    );
                }
            };

            if !output.status.success() {
                attempt_output.push_str(&String::from_utf8_lossy(&output.stdout));
                attempt_output.push_str(&String::from_utf8_lossy(&output.stderr));
                attempt_failed = true;
                break;
            }
        }

        if !attempt_failed {
            let snapshot = tree_snapshot(cwd, simulate_branch);
            if let Some(ref path) = sentinel {
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::write(path, &snapshot);
            }
            let mut result = json!({"status": "ok", "attempts": attempt});
            if attempt > 1 {
                result["flaky"] = json!(true);
                result["first_failure_output"] = json!(first_failure_output.unwrap_or_default());
            }
            return (result, 0);
        } else {
            if first_failure_output.is_none() {
                first_failure_output = Some(attempt_output.trim().to_string());
            }
            if let Some(ref path) = sentinel {
                if path.exists() {
                    let _ = fs::remove_file(path);
                }
            }
        }
    }

    (
        json!({
            "status": "error",
            "attempts": max_attempts,
            "consistent": true,
            "output": first_failure_output.unwrap_or_default(),
        }),
        1,
    )
}

/// Testable CLI entry point.
///
/// Checks the sentinel BEFORE framework detection so callers like
/// `finalize_commit` skip instantly when the tree state is clean —
/// no framework marker files needed for the skip path.
///
/// When the sentinel does not match (or force/retry mode), detects the
/// project framework, builds the CI tool sequence, then dispatches to
/// [`run_once`] or [`run_with_retry`].
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

    let resolved_branch = crate::git::resolve_branch_in(args.branch.as_deref(), cwd, root);

    // Sentinel skip check — before framework detection.
    // This allows callers like finalize_commit to skip instantly when the
    // tree state hasn't changed, even in projects without framework marker
    // files (the sentinel was written by a previous successful run).
    // Applies to both retry and non-retry paths: if CI already passed for
    // this exact tree state, retrying would produce the same result.
    if !args.force {
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

    let framework = match framework_tools::detect_framework_for_project(
        cwd,
        root,
        resolved_branch.as_deref(),
    ) {
        Ok(fw) => fw,
        Err(msg) => return (json!({"status": "error", "message": msg}), 1),
    };

    let tools = match build_tool_sequence(&framework) {
        Ok(t) => t,
        Err(msg) => return (json!({"status": "error", "message": msg}), 1),
    };

    if args.retry > 0 {
        run_with_retry(
            cwd,
            root,
            &tools,
            resolved_branch.as_deref(),
            args.retry,
            args.simulate_branch.as_deref(),
        )
    } else {
        // Force=true since we already checked the sentinel above.
        run_once(
            cwd,
            root,
            &tools,
            resolved_branch.as_deref(),
            true,
            args.simulate_branch.as_deref(),
        )
    }
}

/// CLI entry point for `bin/flow ci`.
pub fn run(args: Args) {
    let flow_ci_running = std::env::var("FLOW_CI_RUNNING").is_ok();
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let root = crate::git::project_root();
    let (result, code) = run_impl(&args, &cwd, &root, flow_ci_running);
    println!("{}", serde_json::to_string(&result).unwrap());
    std::process::exit(code);
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

    // --- tree_snapshot tests (unchanged from before) ---

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

    // --- run_once tests ---

    #[test]
    fn run_once_runs_tools_and_creates_sentinel() {
        let f = make_ci_fixture();
        let script = f.path.join("pass.sh");
        write_script(&script, "#!/usr/bin/env bash\nexit 0\n");
        let tools = single_tool(&script);

        let (out, code) = run_once(&f.path, &f.path, &tools, Some(&f.branch), false, None);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert_eq!(out["skipped"], false);
        assert!(fixture_sentinel(&f).exists());
    }

    #[test]
    fn run_once_skips_when_sentinel_and_clean() {
        let f = make_ci_fixture();
        let script = f.path.join("pass.sh");
        write_script(&script, "#!/usr/bin/env bash\nexit 0\n");
        let tools = single_tool(&script);

        let (first, _) = run_once(&f.path, &f.path, &tools, Some(&f.branch), false, None);
        assert_eq!(first["skipped"], false);

        let (second, code) = run_once(&f.path, &f.path, &tools, Some(&f.branch), false, None);
        assert_eq!(code, 0);
        assert_eq!(second["skipped"], true);
    }

    #[test]
    fn run_once_failure_removes_sentinel() {
        let f = make_ci_fixture();
        let pass = f.path.join("pass.sh");
        write_script(&pass, "#!/usr/bin/env bash\nexit 0\n");
        let tools = single_tool(&pass);

        // Create sentinel
        let _ = run_once(&f.path, &f.path, &tools, Some(&f.branch), false, None);
        assert!(fixture_sentinel(&f).exists());

        // Replace with failing tool
        let fail = f.path.join("fail.sh");
        write_script(&fail, "#!/usr/bin/env bash\nexit 1\n");
        let fail_tools = single_tool(&fail);

        let (out, code) = run_once(&f.path, &f.path, &fail_tools, Some(&f.branch), true, None);
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

        let (first, _) = run_once(&f.path, &f.path, &tools, Some(&f.branch), false, None);
        assert_eq!(first["skipped"], false);

        let (second, code) = run_once(&f.path, &f.path, &tools, Some(&f.branch), true, None);
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

        let (out, code) = run_once(&f.path, &f.path, &tools, Some(&f.branch), false, None);
        assert_eq!(code, 1);
        assert_eq!(out["status"], "error");
        assert!(out["message"].as_str().unwrap().contains("format"));
        assert!(!marker.exists(), "second tool should not have run");
    }

    #[test]
    fn run_once_empty_tools_skips() {
        let f = make_ci_fixture();
        let (out, code) = run_once(&f.path, &f.path, &[], Some(&f.branch), false, None);
        assert_eq!(code, 0);
        assert_eq!(out["skipped"], true);
    }

    #[test]
    fn run_once_detached_head_no_sentinel() {
        let f = make_ci_fixture();
        let script = f.path.join("pass.sh");
        write_script(&script, "#!/usr/bin/env bash\nexit 0\n");
        let tools = single_tool(&script);

        let (out, code) = run_once(&f.path, &f.path, &tools, None, false, None);
        assert_eq!(code, 0);
        assert_eq!(out["skipped"], false);
        let flow_states = f.path.join(".flow-states");
        if flow_states.exists() {
            let entries: Vec<_> = fs::read_dir(&flow_states)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().ends_with("-ci-passed"))
                .collect();
            assert!(entries.is_empty(), "no sentinel expected");
        }
    }

    // --- run_with_retry tests ---

    #[test]
    fn retry_pass_first_attempt() {
        let f = make_ci_fixture();
        let script = f.path.join("pass.sh");
        write_script(&script, "#!/usr/bin/env bash\nexit 0\n");
        let tools = single_tool(&script);

        let (out, code) = run_with_retry(&f.path, &f.path, &tools, Some(&f.branch), 3, None);
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

        let (out, code) = run_with_retry(&f.path, &f.path, &tools, Some(&f.branch), 3, None);
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

        let (out, code) = run_with_retry(&f.path, &f.path, &tools, Some(&f.branch), 3, None);
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

    // --- build_tool_sequence tests ---

    #[test]
    fn tool_sequence_rust_has_four_tools() {
        let tools = build_tool_sequence("rust").unwrap();
        assert_eq!(tools.len(), 4);
        assert_eq!(tools[0].name, "format");
        assert_eq!(tools[1].name, "lint");
        assert_eq!(tools[2].name, "build");
        assert_eq!(tools[3].name, "test");
    }

    #[test]
    fn tool_sequence_python_has_three_tools() {
        // Python projects skip the build step (no compile phase),
        // leaving format → lint → test as the three-tool sequence.
        let tools = build_tool_sequence("python").unwrap();
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0].name, "format");
        assert_eq!(tools[1].name, "lint");
        assert_eq!(tools[2].name, "test");
    }

    #[test]
    fn tool_sequence_rails_has_two_tools() {
        // Rails: build and format are no-ops
        let tools = build_tool_sequence("rails").unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "lint");
        assert_eq!(tools[1].name, "test");
    }

    #[test]
    fn tool_sequence_format_is_first() {
        let tools = build_tool_sequence("rust").unwrap();
        assert_eq!(tools[0].name, "format");
    }

    #[test]
    fn tool_sequence_unknown_framework_errors() {
        let result = build_tool_sequence("cobol");
        assert!(result.is_err());
    }
}
