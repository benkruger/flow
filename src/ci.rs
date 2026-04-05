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
use std::path::Path;
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
        .to_string();

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
        return (
            json!({"status": "error", "message": "bin/ci not found"}),
            1,
        );
    }

    let sentinel = branch.map(|b| {
        root.join(".flow-states")
            .join(format!("{}-ci-passed", b))
    });

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
        (
            json!({"status": "error", "message": "bin/ci failed"}),
            1,
        )
    }
}

/// CLI entry point — not yet implemented. Subsequent tasks add
/// run_with_retry and run_impl which this delegates to.
pub fn run(_args: Args) {
    unimplemented!("ci::run is built incrementally — see plan tasks 10, 12, 13");
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
            let status = Command::new("git")
                .args(args)
                .current_dir(dir)
                .status()
                .expect("git command failed");
            assert!(status.success(), "git {:?} failed", args);
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
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "add app"])
            .current_dir(dir.path())
            .status()
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
            let status = Command::new("git")
                .args(args)
                .current_dir(&path)
                .status()
                .expect("git command failed");
            assert!(status.success(), "git {:?} failed", args);
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

    fn sentinel_path(f: &CiFixture) -> std::path::PathBuf {
        f.path
            .join(".flow-states")
            .join(format!("{}-ci-passed", f.branch))
    }

    // --- run_once() tests ---

    #[test]
    fn run_once_runs_ci_and_creates_sentinel() {
        let f = make_ci_project();
        let (out, code) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert_eq!(out["skipped"], false);
        assert!(sentinel_path(&f).exists());
    }

    #[test]
    fn run_once_stale_sentinel_does_not_skip() {
        let f = make_ci_project();
        let sentinel = sentinel_path(&f);
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
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "add feature"])
            .current_dir(&f.path)
            .status()
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
        let sentinel = sentinel_path(&f);
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
        assert!(!sentinel_path(&f).exists());
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
        assert!(sentinel_path(&f).exists());
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
        let sentinel = f
            .path
            .join(".flow-states")
            .join("other-feature-ci-passed");
        assert!(sentinel.exists());
    }

    #[test]
    fn run_once_non_bash_ci_script() {
        // Python shebang to ensure we don't force bash
        let f = make_ci_project_with(
            "#!/usr/bin/env python3\nimport sys\nsys.exit(0)\n",
            true,
        );
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
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "add app"])
            .current_dir(&f.path)
            .status()
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
            .status()
            .unwrap();
        let (first, _) = run_once(&f.path, &f.path, &f.bin_ci, Some(&f.branch), false, None);
        assert_eq!(first["skipped"], false);

        fs::write(f.path.join("config.py"), "setting = 'b'\n").unwrap();
        Command::new("git")
            .args(["add", "config.py"])
            .current_dir(&f.path)
            .status()
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
            .status()
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
        assert!(!f
            .path
            .join(".flow-states")
            .join("main-ci-passed")
            .exists());
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
        let sentinel = sentinel_path(&f);

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
}
