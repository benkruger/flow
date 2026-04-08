//! Port of lib/ci.py — the `bin/flow ci` subcommand.
//!
//! Runs the target project's `bin/ci` with dirty-check optimization.
//! By default, skips if nothing changed since the last passing run.
//! With `--force`, always runs bin/ci regardless of sentinel state.
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

/// CLI arguments for `bin/flow ci`.
#[derive(Parser, Debug)]
#[command(name = "ci", about = "Run bin/ci with dirty-check optimization")]
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

/// Build the sentinel file path for a given branch: `<root>/.flow-states/<branch>-ci-passed`.
///
/// Centralizes the naming convention so [`run_once`], [`run_with_retry`], and the
/// inline tests all agree on where sentinels live.
///
/// Also used by [`finalize_commit::run_impl`] to refresh the sentinel after a clean commit.
pub fn sentinel_path(root: &Path, branch: &str) -> PathBuf {
    root.join(".flow-states")
        .join(format!("{}-ci-passed", branch))
}

/// Run a git command in `cwd`, returning its stdout as a lossy UTF-8 string.
/// On launch or exec failure, returns an empty string — mirrors Python's
/// subprocess.run(capture_output=True) behavior, which doesn't raise on
/// non-zero exit.
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
///
/// The byte layout of the hashed input is pinned to match `lib/ci.py`
/// exactly — any divergence invalidates every existing sentinel in the
/// repository.
pub fn tree_snapshot(cwd: &Path, simulate_branch: Option<&str>) -> String {
    let head_trimmed = git_stdout(cwd, &["rev-parse", "HEAD"]).trim().to_string();
    let diff_raw = git_stdout(cwd, &["diff", "HEAD"]);
    let untracked_files = git_stdout(cwd, &["ls-files", "--others", "--exclude-standard"])
        .trim()
        .lines()
        // .flow-commit-msg is ephemeral (written by commit skill, deleted by
        // finalize-commit). Including it poisons the sentinel — CI re-runs on
        // every commit even when nothing meaningful changed.
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
/// Runs `bin/ci` once in `cwd` with the child inheriting stdio so the
/// user sees test output in real time. Sets `FLOW_CI_RUNNING=1` in the
/// child environment to short-circuit recursive pytest→bin/flow ci
/// calls, and optionally `FLOW_SIMULATE_BRANCH` when provided.
///
/// Sentinel behavior (dirty-check optimization):
///
/// - When `branch` is Some, the sentinel path is
///   `<root>/.flow-states/<branch>-ci-passed`.
/// - When `!force` and the sentinel content matches the current
///   [`tree_snapshot`], the call returns skipped without running CI.
/// - On success, writes the snapshot to the sentinel (creating parent
///   dirs). On failure, unlinks the sentinel.
/// - Detached HEAD (`branch` is None) disables sentinel writes entirely
///   — CI still runs, but there is no branch to name the sentinel after.
///
/// Returns `(json_value, exit_code)` so the caller can print and exit.
pub fn run_once(
    cwd: &Path,
    root: &Path,
    bin_ci: &Path,
    branch: Option<&str>,
    force: bool,
    simulate_branch: Option<&str>,
) -> (Value, i32) {
    if !bin_ci.exists() {
        return (json!({"status": "error", "message": "bin/ci not found"}), 1);
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

    let mut cmd = Command::new(bin_ci);
    cmd.current_dir(cwd).env("FLOW_CI_RUNNING", "1");
    if let Some(sim) = simulate_branch {
        cmd.env("FLOW_SIMULATE_BRANCH", sim);
    }

    let status = match cmd.status() {
        Ok(s) => s,
        Err(e) => {
            return (
                json!({
                    "status": "error",
                    "message": format!("failed to run bin/ci: {}", e),
                }),
                1,
            );
        }
    };

    if status.success() {
        if let Some(ref path) = sentinel {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(path, &snapshot);
        }
        (json!({"status": "ok", "skipped": false}), 0)
    } else {
        if let Some(ref path) = sentinel {
            let _ = fs::remove_file(path);
        }
        (json!({"status": "error", "message": "bin/ci failed"}), 1)
    }
}

/// Retry CI path with flaky/consistent classification.
///
/// Runs `bin/ci` up to `max_attempts` times with captured stdout and
/// stderr (via `Command::output()`) so the first failure's combined
/// output can be returned as `first_failure_output` when a retry pass
/// classifies the test as flaky. Force semantics — sentinel is NEVER
/// checked for a skip, but is written on success and unlinked on
/// consistent failure so downstream dirty-check runs behave correctly.
///
/// Return shapes:
///
/// - First attempt passes: `{"status":"ok","attempts":1}`
/// - Retry pass (flaky):   `{"status":"ok","attempts":N,"flaky":true,"first_failure_output":"..."}`
/// - All N attempts fail:  `{"status":"error","attempts":N,"consistent":true,"output":"..."}`
pub fn run_with_retry(
    cwd: &Path,
    root: &Path,
    bin_ci: &Path,
    branch: Option<&str>,
    max_attempts: u32,
    simulate_branch: Option<&str>,
) -> (Value, i32) {
    let sentinel = branch.map(|b| sentinel_path(root, b));

    let mut first_failure_output: Option<String> = None;

    for attempt in 1..=max_attempts {
        let mut cmd = Command::new(bin_ci);
        cmd.current_dir(cwd).env("FLOW_CI_RUNNING", "1");
        if let Some(sim) = simulate_branch {
            cmd.env("FLOW_SIMULATE_BRANCH", sim);
        }

        let output = match cmd.output() {
            Ok(o) => o,
            Err(e) => {
                return (
                    json!({
                        "status": "error",
                        "message": format!("failed to run bin/ci: {}", e),
                    }),
                    1,
                );
            }
        };

        if output.status.success() {
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
                let mut combined = String::from_utf8_lossy(&output.stdout).to_string();
                combined.push_str(&String::from_utf8_lossy(&output.stderr));
                first_failure_output = Some(combined.trim().to_string());
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

/// Testable CLI entry point. Extracted from [`run`] so tests can inject
/// `cwd`, `root`, and the recursion-guard env var without mutating the
/// test process environment.
///
/// Dispatches to [`run_once`] when `args.retry == 0` and to
/// [`run_with_retry`] otherwise. Resolves the branch from `args.branch`
/// or via [`crate::git::resolve_branch_in`] using the given `cwd`
/// (does not scan `.flow-states/` — see PR #924).
///
/// Also performs the `bin/ci not found` pre-check here (before
/// dispatch) so both retry and non-retry paths return the same error
/// message, matching Python's single upfront check in `main()`.
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

    let bin_ci = cwd.join("bin").join("ci");
    if !bin_ci.exists() {
        return (json!({"status": "error", "message": "bin/ci not found"}), 1);
    }

    let resolved_branch = crate::git::resolve_branch_in(args.branch.as_deref(), cwd, root);

    if args.retry > 0 {
        run_with_retry(
            cwd,
            root,
            &bin_ci,
            resolved_branch.as_deref(),
            args.retry,
            args.simulate_branch.as_deref(),
        )
    } else {
        run_once(
            cwd,
            root,
            &bin_ci,
            resolved_branch.as_deref(),
            args.force,
            args.simulate_branch.as_deref(),
        )
    }
}

/// CLI entry point for `bin/flow ci`.
///
/// Reads `FLOW_CI_RUNNING` from the parent environment (for recursion
/// detection), resolves `cwd` via [`std::env::current_dir`] and the
/// project root via [`crate::git::project_root`], then delegates to
/// [`run_impl`]. Prints the JSON result as the last line of stdout
/// (following the "last-line JSON parsing" convention) and exits with
/// the returned code.
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

    /// Initialize a git repo in the given directory with an initial commit.
    /// Duplicated from git.rs tests — inline helpers keep modules
    /// independent and avoid a shared test-utilities module for four
    /// simple lines.
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
        // Commit a tracked file
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

        // Modify tracked file — goes into git diff HEAD
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
        // Non-git dir: all four git commands fail and produce empty output.
        // The hash is still deterministic (hash of four empty strings joined
        // by newlines) but meaningless for sentinel purposes. Document the
        // behavior rather than hiding it — the CLI callers gate sentinel
        // writes on branch existence, not on snapshot validity.
        let dir = tempfile::tempdir().unwrap();
        let a = tree_snapshot(dir.path(), None);
        let b = tree_snapshot(dir.path(), None);
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }

    // --- run_once() fixtures ---

    /// Test fixture: a git repo with a committed bin/ci script.
    ///
    /// The TempDir is held by the struct so it lives for the full test;
    /// dropping the fixture cleans up the directory. `path` is both
    /// `cwd` and `root` in run_once calls — in real usage they may
    /// differ, but tests use a single tempdir as both.
    struct CiFixture {
        _dir: tempfile::TempDir,
        path: std::path::PathBuf,
        bin_ci: std::path::PathBuf,
        branch: String,
    }

    /// Default CI fixture: `bin/ci` exits 0, `.flow-states/` excluded
    /// from git status so sentinel writes don't pollute the snapshot.
    fn make_ci_project() -> CiFixture {
        make_ci_project_with("#!/usr/bin/env bash\nexit 0\n", true)
    }

    /// Customizable CI fixture.
    fn make_ci_project_with(bin_ci_script: &str, exclude_flow_states: bool) -> CiFixture {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();

        let run_git = |args: &[&str]| {
            let output = Command::new("git")
                .args(args)
                .current_dir(&path)
                .output()
                .expect("git command failed");
            assert!(output.status.success(), "git {:?} failed", args);
        };
        run_git(&["init", "--initial-branch", "main"]);
        run_git(&["config", "user.email", "test@test.com"]);
        run_git(&["config", "user.name", "Test"]);
        run_git(&["config", "commit.gpgsign", "false"]);

        let bin_dir = path.join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let bin_ci = bin_dir.join("ci");
        fs::write(&bin_ci, bin_ci_script).unwrap();
        fs::set_permissions(&bin_ci, fs::Permissions::from_mode(0o755)).unwrap();

        run_git(&["add", "-A"]);
        run_git(&["commit", "-m", "add bin/ci"]);

        if exclude_flow_states {
            let exclude_file = path.join(".git").join("info").join("exclude");
            fs::create_dir_all(exclude_file.parent().unwrap()).unwrap();
            fs::write(&exclude_file, ".flow-states/\n").unwrap();
        }

        CiFixture {
            _dir: dir,
            path,
            bin_ci,
            branch: "main".to_string(),
        }
    }

    fn fixture_sentinel(f: &CiFixture) -> std::path::PathBuf {
        sentinel_path(&f.path, &f.branch)
    }

    // --- run_once() tests ---

    #[test]
    fn run_once_runs_ci_and_creates_sentinel() {
        let f = make_ci_project();
        let (out, code) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert_eq!(out["skipped"], false);
        assert!(fixture_sentinel(&f).exists());
    }

    #[test]
    fn run_once_stale_sentinel_does_not_skip() {
        let f = make_ci_project();
        let sentinel = fixture_sentinel(&f);
        fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
        fs::write(&sentinel, "stale-snapshot-content").unwrap();

        let (out, code) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(code, 0);
        assert_eq!(out["skipped"], false);
    }

    #[test]
    fn run_once_skips_when_sentinel_and_clean() {
        let f = make_ci_project();
        let (first, _) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(first["skipped"], false);

        let (second, code) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(code, 0);
        assert_eq!(second["skipped"], true);
        assert!(second["reason"].as_str().unwrap().contains("no changes"));
    }

    #[test]
    fn run_once_runs_when_no_sentinel() {
        let f = make_ci_project();
        let (out, code) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(code, 0);
        assert_eq!(out["skipped"], false);
    }

    #[test]
    fn run_once_runs_when_dirty() {
        let f = make_ci_project();
        let (first, _) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(first["skipped"], false);

        fs::write(f.path.join("untracked.txt"), "dirty\n").unwrap();
        let (second, code) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(code, 0);
        assert_eq!(second["skipped"], false);
    }

    #[test]
    fn run_once_skips_after_commit() {
        let f = make_ci_project();

        // Create and commit a feature file
        fs::write(f.path.join("feature.py"), "# new feature\n").unwrap();
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(&f.path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "add feature"])
            .current_dir(&f.path)
            .output()
            .unwrap();

        let (first, _) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(first["skipped"], false);

        let (second, code) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(code, 0);
        assert_eq!(second["skipped"], true);
        assert!(second["reason"].as_str().unwrap().contains("no changes"));
    }

    #[test]
    fn run_once_detached_head_no_sentinel() {
        let f = make_ci_project();
        // Detached HEAD: branch=None disables sentinel entirely
        let (out, code) = run_once(&f.path, &f.path, &f.bin_ci, None, false, None);
        assert_eq!(code, 0);
        assert_eq!(out["skipped"], false);
        // No sentinel file should exist (no branch to name it after)
        let flow_states = f.path.join(".flow-states");
        if flow_states.exists() {
            let entries: Vec<_> = fs::read_dir(&flow_states)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().ends_with("-ci-passed"))
                .collect();
            assert!(entries.is_empty(), "no sentinel expected but found one");
        }
    }

    #[test]
    fn run_once_failure_exits_1_and_removes_sentinel() {
        let f = make_ci_project();
        let sentinel = fixture_sentinel(&f);
        fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
        fs::write(&sentinel, "pre-existing-content").unwrap();

        // Replace bin/ci with a failing version
        fs::write(&f.bin_ci, "#!/usr/bin/env bash\nexit 1\n").unwrap();

        let (out, code) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(code, 1);
        assert_eq!(out["status"], "error");
        assert!(!sentinel.exists());
    }

    #[test]
    fn run_once_failure_without_sentinel() {
        let f = make_ci_project();
        fs::write(&f.bin_ci, "#!/usr/bin/env bash\nexit 1\n").unwrap();

        let (out, code) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(code, 1);
        assert_eq!(out["status"], "error");
        assert!(!fixture_sentinel(&f).exists());
    }

    #[test]
    fn run_once_force_bypasses_sentinel() {
        let f = make_ci_project();
        let (first, _) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(first["skipped"], false);
        // Sentinel now matches — normally would skip
        let (second, code) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), true, None);
        assert_eq!(code, 0);
        assert_eq!(second["skipped"], false);
    }

    #[test]
    fn run_once_force_creates_sentinel() {
        let f = make_ci_project();
        let (out, code) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), true, None);
        assert_eq!(code, 0);
        assert_eq!(out["skipped"], false);
        assert!(fixture_sentinel(&f).exists());
    }

    #[test]
    fn run_once_missing_bin_ci_error() {
        let dir = tempfile::tempdir().unwrap();
        // No bin/ci created
        let bin_ci = dir.path().join("bin").join("ci");
        let (out, code) = run_once(dir.path(), dir.path(), &bin_ci, Some("main"), false, None);
        assert_eq!(code, 1);
        assert_eq!(out["status"], "error");
        assert!(out["message"].as_str().unwrap().contains("not found"));
    }

    #[test]
    fn run_once_branch_flag_uses_specified_sentinel() {
        let f = make_ci_project();
        let (out, code) = run_once(
            &f.path,
            &f.path,
            &f.bin_ci,
            Some("other-feature"),
            false,
            None,
        );
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        let sentinel = f.path.join(".flow-states").join("other-feature-ci-passed");
        assert!(sentinel.exists());
    }

    #[test]
    fn run_once_non_bash_ci_script() {
        // Python shebang to ensure we don't force bash
        let f = make_ci_project_with("#!/usr/bin/env python3\nimport sys\nsys.exit(0)\n", true);
        let (out, code) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
    }

    #[test]
    fn run_once_detects_tracked_file_content_change() {
        let f = make_ci_project();
        // Commit a tracked file
        fs::write(f.path.join("app.py"), "version = 1\n").unwrap();
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(&f.path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "add app"])
            .current_dir(&f.path)
            .output()
            .unwrap();

        // Modify (status M)
        fs::write(f.path.join("app.py"), "version = 2\n").unwrap();
        let (first, _) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(first["skipped"], false);

        // Modify again with different content — still M but content differs
        fs::write(f.path.join("app.py"), "version = 3\n").unwrap();
        let (second, _) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(second["skipped"], false);
    }

    #[test]
    fn run_once_detects_untracked_file_content_change() {
        let f = make_ci_project();
        fs::write(f.path.join("notes.txt"), "draft 1\n").unwrap();
        let (first, _) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(first["skipped"], false);

        fs::write(f.path.join("notes.txt"), "draft 2\n").unwrap();
        let (second, _) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(second["skipped"], false);
    }

    #[test]
    fn run_once_detects_staged_content_change() {
        let f = make_ci_project();
        fs::write(f.path.join("config.py"), "setting = 'a'\n").unwrap();
        Command::new("git")
            .args(["add", "config.py"])
            .current_dir(&f.path)
            .output()
            .unwrap();
        let (first, _) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(first["skipped"], false);

        fs::write(f.path.join("config.py"), "setting = 'b'\n").unwrap();
        Command::new("git")
            .args(["add", "config.py"])
            .current_dir(&f.path)
            .output()
            .unwrap();
        let (second, _) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(second["skipped"], false);
    }

    #[test]
    fn run_once_detects_untracked_file_rename() {
        let f = make_ci_project();
        fs::write(f.path.join("old_name.txt"), "same content\n").unwrap();
        let (first, _) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(first["skipped"], false);

        fs::rename(f.path.join("old_name.txt"), f.path.join("new_name.txt")).unwrap();
        let (second, _) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(second["skipped"], false);
    }

    #[test]
    fn run_once_simulate_branch_sets_child_env() {
        // bin/ci writes FLOW_SIMULATE_BRANCH to a file so the test can
        // verify the child saw the env var — avoids needing to capture
        // inherited stdout from the child process.
        let f = make_ci_project_with(
            "#!/usr/bin/env bash\necho \"SIM=$FLOW_SIMULATE_BRANCH\" > .ci-env-check\nexit 0\n",
            true,
        );
        let (out, code) = run_once(
            &f.path,
            &f.path,
            &f.bin_ci,
            Some(&f.branch),
            true,
            Some("main"),
        );
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        let env_check = fs::read_to_string(f.path.join(".ci-env-check")).unwrap();
        assert_eq!(env_check.trim(), "SIM=main");
    }

    #[test]
    fn run_once_simulate_branch_does_not_affect_sentinel_name() {
        // Create a feature branch so real != simulated
        let f = make_ci_project();
        Command::new("git")
            .args(["switch", "-c", "my-feature"])
            .current_dir(&f.path)
            .output()
            .unwrap();

        let (_out, code) = run_once(
            &f.path,
            &f.path,
            &f.bin_ci,
            Some("my-feature"),
            true,
            Some("main"),
        );
        assert_eq!(code, 0);
        // Sentinel must be named after the real branch param, not "main"
        assert!(f
            .path
            .join(".flow-states")
            .join("my-feature-ci-passed")
            .exists());
        assert!(!f.path.join(".flow-states").join("main-ci-passed").exists());
    }

    #[test]
    fn run_once_simulate_branch_with_force() {
        let f = make_ci_project();
        let (first, _) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(first["skipped"], false);
        let (second, code) = run_once(
            &f.path,
            &f.path,
            &f.bin_ci,
            Some(&f.branch),
            true,
            Some("main"),
        );
        assert_eq!(code, 0);
        assert_eq!(second["skipped"], false);
    }

    #[test]
    fn run_once_simulate_branch_different_snapshot() {
        let f = make_ci_project();
        let sentinel = fixture_sentinel(&f);

        let (_first, _) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        let plain_hash = fs::read_to_string(&sentinel).unwrap();

        let (_second, _) = run_once(
            &f.path,
            &f.path,
            &f.bin_ci,
            Some(&f.branch),
            false,
            Some("main"),
        );
        let simulate_hash = fs::read_to_string(&sentinel).unwrap();
        assert_ne!(plain_hash, simulate_hash);
    }

    #[test]
    fn run_once_simulate_branch_skips_on_matching_sentinel() {
        let f = make_ci_project();
        let (first, _) = run_once(
            &f.path,
            &f.path,
            &f.bin_ci,
            Some(&f.branch),
            false,
            Some("main"),
        );
        assert_eq!(first["skipped"], false);

        let (second, code) = run_once(
            &f.path,
            &f.path,
            &f.bin_ci,
            Some(&f.branch),
            false,
            Some("main"),
        );
        assert_eq!(code, 0);
        assert_eq!(second["skipped"], true);
        assert!(second["reason"].as_str().unwrap().contains("no changes"));
    }

    #[test]
    fn run_once_simulate_branch_no_skip_after_plain_run() {
        let f = make_ci_project();
        let (first, _) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(first["skipped"], false);

        let (second, code) = run_once(
            &f.path,
            &f.path,
            &f.bin_ci,
            Some(&f.branch),
            false,
            Some("main"),
        );
        assert_eq!(code, 0);
        assert_eq!(second["skipped"], false);
    }

    // --- run_with_retry() tests ---

    /// Fixture: bin/ci that fails on the first attempt and passes on the
    /// second by tracking a counter file in the project root.
    fn make_flaky_ci_project() -> CiFixture {
        let script = r#"#!/usr/bin/env bash
COUNTER_FILE="$(pwd)/.ci-attempt-counter"
if [ -f "$COUNTER_FILE" ]; then
  COUNT=$(($(cat "$COUNTER_FILE") + 1))
else
  COUNT=1
fi
echo "$COUNT" > "$COUNTER_FILE"
if [ "$COUNT" -lt 2 ]; then
  echo "FAIL: flaky test on attempt $COUNT" >&2
  exit 1
fi
echo "PASS: attempt $COUNT"
exit 0
"#;
        make_ci_project_with(script, true)
    }

    /// Fixture: bin/ci that always fails with stderr output.
    fn make_failing_ci_project() -> CiFixture {
        make_ci_project_with(
            "#!/usr/bin/env bash\necho 'CI FAILED: assertion error in test_foo' >&2\nexit 1\n",
            true,
        )
    }

    #[test]
    fn retry_pass_first_attempt() {
        let f = make_ci_project();
        let (out, code) = run_with_retry(&f.path, &f.path, &f.bin_ci, Some(&f.branch), 3, None);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert_eq!(out["attempts"], 1);
        assert!(out.get("flaky").is_none());
        assert!(fixture_sentinel(&f).exists());
    }

    #[test]
    fn retry_flaky() {
        let f = make_flaky_ci_project();
        let (out, code) = run_with_retry(&f.path, &f.path, &f.bin_ci, Some(&f.branch), 3, None);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert_eq!(out["attempts"], 2);
        assert_eq!(out["flaky"], true);
        let first_fail = out["first_failure_output"].as_str().unwrap();
        assert!(!first_fail.is_empty());
        assert!(first_fail.contains("FAIL"));
    }

    #[test]
    fn retry_consistent_failure() {
        let f = make_failing_ci_project();
        let (out, code) = run_with_retry(&f.path, &f.path, &f.bin_ci, Some(&f.branch), 3, None);
        assert_eq!(code, 1);
        assert_eq!(out["status"], "error");
        assert_eq!(out["attempts"], 3);
        assert_eq!(out["consistent"], true);
        let output = out["output"].as_str().unwrap();
        assert!(!output.is_empty());
        assert!(output.contains("CI FAILED"));
    }

    #[test]
    fn retry_with_branch_flag() {
        let f = make_ci_project();
        let (out, code) = run_with_retry(&f.path, &f.path, &f.bin_ci, Some("main"), 2, None);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert_eq!(out["attempts"], 1);
    }

    #[test]
    fn retry_failure_removes_sentinel() {
        let f = make_ci_project();
        // Create a sentinel from a passing run
        let (_first, _) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert!(fixture_sentinel(&f).exists());

        // Swap to failing bin/ci
        fs::write(&f.bin_ci, "#!/usr/bin/env bash\necho 'FAIL' >&2\nexit 1\n").unwrap();

        let (out, code) = run_with_retry(&f.path, &f.path, &f.bin_ci, Some(&f.branch), 2, None);
        assert_eq!(code, 1);
        assert_eq!(out["consistent"], true);
        assert!(!fixture_sentinel(&f).exists());
    }

    #[test]
    fn retry_ignores_sentinel() {
        let f = make_ci_project();
        // Run once to create sentinel
        let (first, _) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(first["skipped"], false);
        // Retry must NOT skip even though sentinel matches
        let (second, code) = run_with_retry(&f.path, &f.path, &f.bin_ci, Some(&f.branch), 3, None);
        assert_eq!(code, 0);
        assert_eq!(second["attempts"], 1);
        assert!(second.get("skipped").is_none());
    }

    // --- run_impl() tests ---

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
        let f = make_ci_project();
        let args = Args {
            branch: Some(f.branch.clone()),
            ..default_args()
        };
        let (out, code) = run_impl(&args, &f.path, &f.path, true);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert_eq!(out["skipped"], true);
        assert_eq!(out["reason"], "recursion guard");
        // No sentinel should be created — CI never ran
        assert!(!fixture_sentinel(&f).exists());
    }

    #[test]
    fn cli_dispatches_to_run_once_without_retry() {
        let f = make_ci_project();
        let args = Args {
            branch: Some(f.branch.clone()),
            ..default_args()
        };
        let (out, code) = run_impl(&args, &f.path, &f.path, false);
        assert_eq!(code, 0);
        // run_once response shape has "skipped" key; run_with_retry has "attempts"
        assert_eq!(out["skipped"], false);
        assert!(out.get("attempts").is_none());
    }

    #[test]
    fn cli_dispatches_to_run_with_retry_when_retry_gt_zero() {
        let f = make_ci_project();
        let args = Args {
            retry: 3,
            branch: Some(f.branch.clone()),
            ..default_args()
        };
        let (out, code) = run_impl(&args, &f.path, &f.path, false);
        assert_eq!(code, 0);
        // run_with_retry response has "attempts" key; run_once has "skipped"
        assert_eq!(out["attempts"], 1);
        assert!(out.get("skipped").is_none());
    }

    #[test]
    fn cli_branch_override_threads_through() {
        let f = make_ci_project();
        let args = Args {
            branch: Some("other-feature".to_string()),
            ..default_args()
        };
        let (_out, code) = run_impl(&args, &f.path, &f.path, false);
        assert_eq!(code, 0);
        let sentinel = f.path.join(".flow-states").join("other-feature-ci-passed");
        assert!(sentinel.exists());
    }

    #[test]
    fn cli_auto_detects_branch_from_cwd() {
        // No args.branch → run_impl must fall back to current_branch_in(cwd)
        let f = make_ci_project();
        let args = default_args();
        let (_out, code) = run_impl(&args, &f.path, &f.path, false);
        assert_eq!(code, 0);
        // The fixture's branch is "main" and that's what current_branch_in
        // should return from the cwd. Sentinel should land at main-ci-passed.
        assert!(fixture_sentinel(&f).exists());
    }

    #[test]
    fn cli_missing_bin_ci_uniform_error_across_retry_modes() {
        // Both retry=0 and retry>0 paths must return the same
        // "bin/ci not found" error — matching Python's single upfront
        // check in main() before dispatching to either path.
        let dir = tempfile::tempdir().unwrap();
        // Initialize a git repo so resolve_branch_in succeeds; no bin/ci though
        let run = |args: &[&str]| {
            let output = Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .output()
                .expect("git command failed");
            assert!(output.status.success(), "git {:?} failed", args);
        };
        run(&["init", "--initial-branch", "main"]);
        run(&["config", "user.email", "test@test.com"]);
        run(&["config", "user.name", "Test"]);
        run(&["config", "commit.gpgsign", "false"]);
        run(&["commit", "--allow-empty", "-m", "init"]);

        // Non-retry path
        let (out_once, code_once) = run_impl(&default_args(), dir.path(), dir.path(), false);
        assert_eq!(code_once, 1);
        assert_eq!(out_once["status"], "error");
        assert!(out_once["message"].as_str().unwrap().contains("not found"));

        // Retry path
        let args = Args {
            retry: 3,
            ..default_args()
        };
        let (out_retry, code_retry) = run_impl(&args, dir.path(), dir.path(), false);
        assert_eq!(code_retry, 1);
        assert_eq!(out_retry["status"], "error");
        assert!(out_retry["message"].as_str().unwrap().contains("not found"));

        // Same error message on both paths
        assert_eq!(out_once["message"], out_retry["message"]);
    }

    #[test]
    fn cli_force_threads_through() {
        let f = make_ci_project();
        // First run creates the sentinel
        let (first, _) = run_impl(
            &Args {
                branch: Some(f.branch.clone()),
                ..default_args()
            },
            &f.path,
            &f.path,
            false,
        );
        assert_eq!(first["skipped"], false);
        // Second run without force — should skip
        let (second, _) = run_impl(
            &Args {
                branch: Some(f.branch.clone()),
                ..default_args()
            },
            &f.path,
            &f.path,
            false,
        );
        assert_eq!(second["skipped"], true);
        // Third run with force — should NOT skip
        let (third, _) = run_impl(
            &Args {
                force: true,
                branch: Some(f.branch.clone()),
                ..default_args()
            },
            &f.path,
            &f.path,
            false,
        );
        assert_eq!(third["skipped"], false);
    }
}
