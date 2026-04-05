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

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use clap::Parser;
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

/// CLI entry point — not yet implemented. Subsequent tasks add run_once,
/// run_with_retry, and run_impl which this delegates to.
pub fn run(_args: Args) {
    unimplemented!("ci::run is built incrementally — see plan tasks 8, 10, 12, 13");
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
}
