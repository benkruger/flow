//! Port of lib/finalize-commit.py — commit, cleanup, pull, push.
//!
//! Enforces CI before committing: calls [`ci::run_once`] as the first step
//! in [`run_impl`]. If CI fails, returns an error and commits nothing.
//! When the CI sentinel is fresh (CI already passed for this tree state),
//! the check noops instantly — no overhead on the happy path.
//!
//! Usage:
//!   bin/flow finalize-commit <message-file> <branch>
//!
//! Output (JSON to stdout):
//!   Success:   {"status": "ok", "sha": "<commit-hash>", "pull_merged": <bool>}
//!   Warning:   {"status": "ok", "sha": "", "pull_merged": true, "warning": "..."}
//!   Conflict:  {"status": "conflict", "files": ["file1.py", ...]}
//!   Error:     {"status": "error", "step": "ci|commit|pull|push", "message": "..."}

use std::fs;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use clap::Parser;
use serde_json::{json, Value};

use crate::output::json_error;
use crate::utils::parse_conflict_files;

const LOCAL_TIMEOUT: u64 = 30;
const NETWORK_TIMEOUT: u64 = 60;

#[derive(Parser, Debug)]
#[command(name = "finalize-commit", about = "Finalize a commit: commit, cleanup, pull, push")]
pub struct Args {
    /// Path to the commit message file
    pub message_file: String,
    /// Branch name for git pull
    pub branch: String,
}

/// Remove the commit message file, ignoring errors.
fn remove_message_file(path: &str) {
    let _ = fs::remove_file(path);
}

/// Run a git command with a timeout. Returns (exit_code, stdout, stderr).
fn run_git_with_timeout(
    args: &[&str],
    timeout_secs: u64,
) -> Result<(i32, String, String), String> {
    let mut child = Command::new("git")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn git: {}", e))?;

    let timeout = Duration::from_secs(timeout_secs);
    let start = Instant::now();
    let poll_interval = Duration::from_millis(50);

    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let output = child.wait_with_output().map_err(|e| e.to_string())?;
                let code = output.status.code().unwrap_or(1);
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                return Ok((code, stdout, stderr));
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("timed out after {}s", timeout_secs));
                }
                let remaining = timeout.saturating_sub(start.elapsed());
                std::thread::sleep(poll_interval.min(remaining));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Core finalize-commit logic with injectable git runner for testing.
pub fn finalize_commit_inner(
    message_file: &str,
    branch: &str,
    git: &dyn Fn(&[&str], u64) -> Result<(i32, String, String), String>,
) -> Value {
    // Step 1: git commit -F <message_file>
    match git(&["commit", "-F", message_file], LOCAL_TIMEOUT) {
        Err(e) => {
            remove_message_file(message_file);
            return json!({
                "status": "error",
                "step": "commit",
                "message": format!("git commit {}", e)
            });
        }
        Ok((code, _, stderr)) => {
            remove_message_file(message_file);
            if code != 0 {
                return json!({
                    "status": "error",
                    "step": "commit",
                    "message": stderr.trim()
                });
            }
        }
    }

    // Capture post-commit SHA for pull_merged detection.
    // If this fails, default to pull_merged=true (safe: don't refresh sentinel).
    let post_commit_sha = git(&["rev-parse", "HEAD"], LOCAL_TIMEOUT)
        .ok()
        .and_then(|(code, stdout, _)| {
            if code == 0 {
                Some(stdout.trim().to_string())
            } else {
                None
            }
        });

    // Step 2: git pull origin <branch>
    match git(&["pull", "origin", branch], NETWORK_TIMEOUT) {
        Err(e) => {
            return json!({
                "status": "error",
                "step": "pull",
                "message": format!("git pull {}", e)
            });
        }
        Ok((code, _, stderr)) if code != 0 => {
            // Check for merge conflicts
            match git(&["status", "--porcelain"], LOCAL_TIMEOUT) {
                Err(_) => {
                    return json!({
                        "status": "error",
                        "step": "pull",
                        "message": stderr.trim()
                    });
                }
                Ok((_, stdout, _)) => {
                    let conflicts = parse_conflict_files(&stdout);
                    if !conflicts.is_empty() {
                        return json!({"status": "conflict", "files": conflicts});
                    }
                    return json!({
                        "status": "error",
                        "step": "pull",
                        "message": stderr.trim()
                    });
                }
            }
        }
        Ok(_) => {} // pull succeeded
    }

    // Step 3: git push
    match git(&["push"], NETWORK_TIMEOUT) {
        Err(e) => {
            return json!({
                "status": "error",
                "step": "push",
                "message": format!("git push {}", e)
            });
        }
        Ok((code, _, stderr)) if code != 0 => {
            return json!({
                "status": "error",
                "step": "push",
                "message": stderr.trim()
            });
        }
        Ok(_) => {}
    }

    // Step 4: git rev-parse HEAD
    match git(&["rev-parse", "HEAD"], LOCAL_TIMEOUT) {
        Err(_) => json!({
            "status": "ok",
            "sha": "",
            "pull_merged": true,
            "warning": "commit succeeded but SHA retrieval timed out"
        }),
        Ok((code, _, _)) if code != 0 => json!({
            "status": "ok",
            "sha": "",
            "pull_merged": true,
            "warning": "commit succeeded but SHA retrieval failed"
        }),
        Ok((_, stdout, _)) => {
            let final_sha = stdout.trim();
            let pull_merged = post_commit_sha
                .as_deref()
                .map_or(true, |post| post != final_sha);
            json!({"status": "ok", "sha": final_sha, "pull_merged": pull_merged})
        }
    }
}

/// Run finalize-commit with real git subprocess.
pub fn finalize_commit(message_file: &str, branch: &str) -> Value {
    finalize_commit_inner(message_file, branch, &run_git_with_timeout)
}

/// Adapter: prepends `-C <cwd>` to git args so `run_impl` can target a
/// specific directory without `set_current_dir` (which races in parallel tests).
/// Wraps `run_git_with_timeout` to match the `(args, timeout)` closure shape
/// expected by `finalize_commit_inner`.
fn run_git_in_dir(
    cwd: &std::path::Path,
    args: &[&str],
    timeout_secs: u64,
) -> Result<(i32, String, String), String> {
    let mut cmd_args = vec!["-C", cwd.to_str().unwrap_or(".")];
    cmd_args.extend_from_slice(args);
    run_git_with_timeout(&cmd_args, timeout_secs)
}

/// Testable entry point: runs finalize-commit, then refreshes the CI sentinel
/// when pull did not introduce new content (pull_merged == false).
///
/// `cwd` and `root` are passed explicitly so integration tests can avoid
/// `set_current_dir` (which is process-wide and races with parallel tests).
/// Returns Ok(result_json) on success, Err(message) on infrastructure errors.
pub fn run_impl(
    args: &Args,
    cwd: &std::path::Path,
    root: &std::path::Path,
) -> Result<Value, String> {
    if args.message_file.is_empty() || args.branch.is_empty() {
        return Err("Usage: bin/flow finalize-commit <message-file> <branch>".to_string());
    }

    // Enforce CI before committing. run_once checks the sentinel first —
    // if CI already passed for this tree state, it noops instantly.
    let bin_ci = cwd.join("bin").join("ci");
    let (ci_result, ci_code) =
        crate::ci::run_once(cwd, root, &bin_ci, Some(&args.branch), false, None);
    if ci_code != 0 {
        let msg = ci_result["message"]
            .as_str()
            .unwrap_or("bin/flow ci failed");
        return Ok(json!({
            "status": "error",
            "step": "ci",
            "message": msg,
        }));
    }

    let cwd_owned = cwd.to_path_buf();
    let git = |git_args: &[&str], timeout: u64| -> Result<(i32, String, String), String> {
        run_git_in_dir(&cwd_owned, git_args, timeout)
    };

    let result = finalize_commit_inner(&args.message_file, &args.branch, &git);

    // Sentinel maintenance after commit:
    // - pull_merged == false: tree unchanged by pull → refresh sentinel to current snapshot.
    // - pull_merged == true: pull brought in new content → remove stale sentinel so the
    //   next CI run re-tests. (CI's run_once created the sentinel before the commit;
    //   the pull invalidated it.)
    if result["status"] == "ok" {
        let sentinel = crate::ci::sentinel_path(root, &args.branch);
        if result.get("pull_merged") == Some(&json!(false)) {
            let snapshot = crate::ci::tree_snapshot(cwd, None);
            let _ = fs::write(&sentinel, &snapshot);
        } else {
            let _ = fs::remove_file(&sentinel);
        }
    }

    Ok(result)
}

pub fn run(args: Args) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let root = crate::git::project_root();
    match run_impl(&args, &cwd, &root) {
        Err(msg) => {
            json_error(&msg, &[("step", json!("args"))]);
            std::process::exit(1);
        }
        Ok(result) => {
            println!("{}", result);
            if result["status"] != "ok" {
                std::process::exit(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    type GitResult = Result<(i32, String, String), String>;

    fn mock_git(responses: Vec<GitResult>) -> impl Fn(&[&str], u64) -> GitResult {
        let queue = RefCell::new(VecDeque::from(responses));
        move |_args: &[&str], _timeout: u64| -> GitResult {
            queue
                .borrow_mut()
                .pop_front()
                .expect("no more mock responses")
        }
    }

    fn ok(stdout: &str) -> GitResult {
        Ok((0, stdout.to_string(), String::new()))
    }

    fn fail(stderr: &str) -> GitResult {
        Ok((1, String::new(), stderr.to_string()))
    }

    fn timeout(msg: &str) -> GitResult {
        Err(msg.to_string())
    }

    #[test]
    fn happy_path() {
        let dir = tempfile::tempdir().unwrap();
        let msg = dir.path().join(".flow-commit-msg");
        std::fs::write(&msg, "Test commit.").unwrap();

        let git = mock_git(vec![
            ok(""),                // git commit
            ok("abc123\n"),        // git rev-parse HEAD (post-commit)
            ok(""),                // git pull
            ok(""),                // git push
            ok("abc123\n"),        // git rev-parse HEAD (final)
        ]);

        let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
        assert_eq!(result["status"], "ok");
        assert_eq!(result["sha"], "abc123");
        assert_eq!(result["pull_merged"], false);
        assert!(!msg.exists());
    }

    #[test]
    fn commit_failure() {
        let dir = tempfile::tempdir().unwrap();
        let msg = dir.path().join(".flow-commit-msg");
        std::fs::write(&msg, "Test commit.").unwrap();

        let git = mock_git(vec![fail("nothing to commit")]);

        let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
        assert_eq!(result["status"], "error");
        assert_eq!(result["step"], "commit");
        assert_eq!(result["message"], "nothing to commit");
        assert!(!msg.exists());
    }

    #[test]
    fn pull_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let msg = dir.path().join(".flow-commit-msg");
        std::fs::write(&msg, "Test commit.").unwrap();

        let git = mock_git(vec![
            ok(""),                                  // git commit
            ok("commit_sha\n"),                      // git rev-parse HEAD (post-commit)
            fail("CONFLICT"),                        // git pull
            Ok((0, "UU file1.py\nAA file2.py\n".to_string(), String::new())), // git status
        ]);

        let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
        assert_eq!(result["status"], "conflict");
        let files: Vec<String> = result["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(files, vec!["file1.py", "file2.py"]);
    }

    #[test]
    fn pull_error_non_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let msg = dir.path().join(".flow-commit-msg");
        std::fs::write(&msg, "Test commit.").unwrap();

        let git = mock_git(vec![
            ok(""),                          // git commit
            ok("commit_sha\n"),              // git rev-parse HEAD (post-commit)
            fail("Could not resolve host"),  // git pull
            ok(""),                          // git status (clean)
        ]);

        let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
        assert_eq!(result["status"], "error");
        assert_eq!(result["step"], "pull");
        assert_eq!(result["message"], "Could not resolve host");
    }

    #[test]
    fn push_failure() {
        let dir = tempfile::tempdir().unwrap();
        let msg = dir.path().join(".flow-commit-msg");
        std::fs::write(&msg, "Test commit.").unwrap();

        let git = mock_git(vec![
            ok(""),                       // git commit
            ok("commit_sha\n"),           // git rev-parse HEAD (post-commit)
            ok(""),                       // git pull
            fail("permission denied"),    // git push
        ]);

        let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
        assert_eq!(result["status"], "error");
        assert_eq!(result["step"], "push");
        assert_eq!(result["message"], "permission denied");
    }

    #[test]
    fn message_file_missing_ok() {
        let dir = tempfile::tempdir().unwrap();
        let msg = dir.path().join(".flow-commit-msg");
        // Don't create the file — simulate it already being gone

        let git = mock_git(vec![
            ok(""),                // git commit
            ok("def456\n"),        // git rev-parse HEAD (post-commit)
            ok(""),                // git pull
            ok(""),                // git push
            ok("def456\n"),        // git rev-parse HEAD (final)
        ]);

        let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
        assert_eq!(result["status"], "ok");
        assert_eq!(result["pull_merged"], false);
    }

    #[test]
    fn rev_parse_failure() {
        let dir = tempfile::tempdir().unwrap();
        let msg = dir.path().join(".flow-commit-msg");
        std::fs::write(&msg, "Test commit.").unwrap();

        let git = mock_git(vec![
            ok(""),               // git commit
            ok("commit_sha\n"),   // git rev-parse HEAD (post-commit)
            ok(""),               // git pull
            ok(""),               // git push
            fail("bad HEAD"),     // git rev-parse HEAD (final)
        ]);

        let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
        assert_eq!(result["status"], "ok");
        assert_eq!(result["sha"], "");
        assert_eq!(result["pull_merged"], true);
        assert_eq!(
            result["warning"],
            "commit succeeded but SHA retrieval failed"
        );
    }

    #[test]
    fn commit_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let msg = dir.path().join(".flow-commit-msg");
        std::fs::write(&msg, "Test commit.").unwrap();

        let git = mock_git(vec![timeout("timed out after 30s")]);

        let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
        assert_eq!(result["status"], "error");
        assert_eq!(result["step"], "commit");
        assert!(result["message"].as_str().unwrap().contains("timed out"));
        assert!(!msg.exists());
    }

    #[test]
    fn pull_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let msg = dir.path().join(".flow-commit-msg");
        std::fs::write(&msg, "Test commit.").unwrap();

        let git = mock_git(vec![
            ok(""),                                // git commit
            ok("commit_sha\n"),                    // git rev-parse HEAD (post-commit)
            timeout("timed out after 60s"),        // git pull
        ]);

        let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
        assert_eq!(result["status"], "error");
        assert_eq!(result["step"], "pull");
        assert!(result["message"].as_str().unwrap().contains("timed out"));
    }

    #[test]
    fn push_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let msg = dir.path().join(".flow-commit-msg");
        std::fs::write(&msg, "Test commit.").unwrap();

        let git = mock_git(vec![
            ok(""),                                // git commit
            ok("commit_sha\n"),                    // git rev-parse HEAD (post-commit)
            ok(""),                                // git pull
            timeout("timed out after 60s"),        // git push
        ]);

        let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
        assert_eq!(result["status"], "error");
        assert_eq!(result["step"], "push");
        assert!(result["message"].as_str().unwrap().contains("timed out"));
    }

    #[test]
    fn rev_parse_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let msg = dir.path().join(".flow-commit-msg");
        std::fs::write(&msg, "Test commit.").unwrap();

        let git = mock_git(vec![
            ok(""),                                // git commit
            ok("commit_sha\n"),                    // git rev-parse HEAD (post-commit)
            ok(""),                                // git pull
            ok(""),                                // git push
            timeout("timed out after 30s"),        // git rev-parse HEAD (final)
        ]);

        let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
        assert_eq!(result["status"], "ok");
        assert_eq!(result["sha"], "");
        assert_eq!(result["pull_merged"], true);
        assert_eq!(
            result["warning"],
            "commit succeeded but SHA retrieval timed out"
        );
    }

    #[test]
    fn status_porcelain_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let msg = dir.path().join(".flow-commit-msg");
        std::fs::write(&msg, "Test commit.").unwrap();

        let git = mock_git(vec![
            ok(""),                                       // git commit
            ok("commit_sha\n"),                           // git rev-parse HEAD (post-commit)
            fail("Could not resolve host"),               // git pull
            timeout("timed out after 30s"),               // git status --porcelain
        ]);

        let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
        assert_eq!(result["status"], "error");
        assert_eq!(result["step"], "pull");
        assert_eq!(result["message"], "Could not resolve host");
    }

    #[test]
    fn dd_conflict_detected() {
        let dir = tempfile::tempdir().unwrap();
        let msg = dir.path().join(".flow-commit-msg");
        std::fs::write(&msg, "Test commit.").unwrap();

        let git = mock_git(vec![
            ok(""),                                                    // git commit
            ok("commit_sha\n"),                                        // git rev-parse HEAD (post-commit)
            fail("CONFLICT"),                                          // git pull
            Ok((0, "DD deleted.py\n".to_string(), String::new())),     // git status
        ]);

        let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
        assert_eq!(result["status"], "conflict");
        let files: Vec<String> = result["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(files, vec!["deleted.py"]);
    }

    #[test]
    fn pull_merged_false_when_shas_match() {
        let dir = tempfile::tempdir().unwrap();
        let msg = dir.path().join(".flow-commit-msg");
        std::fs::write(&msg, "Test commit.").unwrap();

        let git = mock_git(vec![
            ok(""),                // git commit
            ok("same_sha\n"),     // git rev-parse HEAD (post-commit)
            ok(""),                // git pull (no new content)
            ok(""),                // git push
            ok("same_sha\n"),     // git rev-parse HEAD (final — unchanged)
        ]);

        let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
        assert_eq!(result["status"], "ok");
        assert_eq!(result["sha"], "same_sha");
        assert_eq!(result["pull_merged"], false);
    }

    #[test]
    fn pull_merged_true_when_shas_differ() {
        let dir = tempfile::tempdir().unwrap();
        let msg = dir.path().join(".flow-commit-msg");
        std::fs::write(&msg, "Test commit.").unwrap();

        let git = mock_git(vec![
            ok(""),                // git commit
            ok("aaa\n"),           // git rev-parse HEAD (post-commit)
            ok(""),                // git pull (merged remote changes)
            ok(""),                // git push
            ok("bbb\n"),           // git rev-parse HEAD (final — changed by pull)
        ]);

        let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
        assert_eq!(result["status"], "ok");
        assert_eq!(result["sha"], "bbb");
        assert_eq!(result["pull_merged"], true);
    }

    #[test]
    fn pull_merged_true_when_post_commit_revparse_fails() {
        let dir = tempfile::tempdir().unwrap();
        let msg = dir.path().join(".flow-commit-msg");
        std::fs::write(&msg, "Test commit.").unwrap();

        let git = mock_git(vec![
            ok(""),                // git commit
            fail("bad HEAD"),      // git rev-parse HEAD (post-commit — fails)
            ok(""),                // git pull
            ok(""),                // git push
            ok("final_sha\n"),    // git rev-parse HEAD (final)
        ]);

        let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
        assert_eq!(result["status"], "ok");
        assert_eq!(result["sha"], "final_sha");
        assert_eq!(result["pull_merged"], true);
    }

    #[test]
    fn pull_merged_true_when_final_revparse_fails() {
        let dir = tempfile::tempdir().unwrap();
        let msg = dir.path().join(".flow-commit-msg");
        std::fs::write(&msg, "Test commit.").unwrap();

        let git = mock_git(vec![
            ok(""),                // git commit
            ok("post_sha\n"),      // git rev-parse HEAD (post-commit)
            ok(""),                // git pull
            ok(""),                // git push
            fail("bad HEAD"),      // git rev-parse HEAD (final — fails)
        ]);

        let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
        assert_eq!(result["status"], "ok");
        assert_eq!(result["sha"], "");
        assert_eq!(result["pull_merged"], true);
    }

    // --- Integration tests for run_impl CI enforcement ---

    /// Set up a bare remote + clone with a configurable bin/ci script and .flow-states dir.
    ///
    /// The bin/ci script checks `.ci-should-fail` in the project root:
    /// - If the file exists and contains "1", bin/ci exits 1 (CI fails).
    /// - Otherwise, bin/ci exits 0 (CI passes).
    ///
    /// Additionally, each invocation appends a line to `.ci-invocation-marker`
    /// so tests can verify whether CI actually ran (vs. being skipped by sentinel).
    ///
    /// Returns (clone_dir, bare_dir) as TempDirs that must be kept alive.
    fn setup_integration_repo_with_ci() -> (tempfile::TempDir, tempfile::TempDir) {
        let (clone_dir, bare_dir) = setup_integration_repo();
        let clone_str = clone_dir.path().to_str().unwrap();

        // Create bin/ci script with pass/fail control and invocation marker
        let bin_dir = clone_dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let bin_ci = bin_dir.join("ci");
        let script = r#"#!/usr/bin/env bash
# Append to marker file so tests can count invocations
echo "invoked" >> "$(dirname "$0")/../.ci-invocation-marker"
# Check control file for pass/fail
if [ -f "$(dirname "$0")/../.ci-should-fail" ] && [ "$(cat "$(dirname "$0")/../.ci-should-fail")" = "1" ]; then
  exit 1
fi
exit 0
"#;
        fs::write(&bin_ci, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&bin_ci, fs::Permissions::from_mode(0o755)).unwrap();
        }

        // Exclude CI control/marker files from git so they don't affect
        // the tree snapshot used for sentinel matching in tests.
        let exclude_file = clone_dir.path().join(".git").join("info").join("exclude");
        let existing = fs::read_to_string(&exclude_file).unwrap_or_default();
        fs::write(
            &exclude_file,
            format!("{}.ci-invocation-marker\n.ci-should-fail\n", existing),
        )
        .unwrap();

        // Commit bin/ci so it's tracked (avoids untracked-file snapshot changes)
        Command::new("git")
            .args(["-C", clone_str, "add", "bin/ci"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        Command::new("git")
            .args(["-C", clone_str, "commit", "-m", "Add bin/ci"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        Command::new("git")
            .args(["-C", clone_str, "push"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();

        (clone_dir, bare_dir)
    }

    #[test]
    fn test_ci_fails_blocks_commit() {
        let (clone_dir, _bare_dir) = setup_integration_repo_with_ci();
        let clone_path = clone_dir.path();

        // Configure bin/ci to fail
        fs::write(clone_path.join(".ci-should-fail"), "1").unwrap();

        // Create a file to commit
        fs::write(clone_path.join("feature.rs"), "fn main() {}\n").unwrap();
        Command::new("git")
            .args(["-C", clone_path.to_str().unwrap(), "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();

        // Write commit message file
        let msg_path = clone_path.join(".flow-commit-msg");
        fs::write(&msg_path, "Add feature.rs").unwrap();

        // Count commits before
        let before = Command::new("git")
            .args(["-C", clone_path.to_str().unwrap(), "log", "--oneline"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        let commits_before = String::from_utf8_lossy(&before.stdout)
            .lines()
            .count();

        let args = Args {
            message_file: msg_path.to_str().unwrap().to_string(),
            branch: "main".to_string(),
        };

        let result = run_impl(&args, clone_path, clone_path).unwrap();
        assert_eq!(result["status"], "error");
        assert_eq!(result["step"], "ci");

        // Verify no new commit was created
        let after = Command::new("git")
            .args(["-C", clone_path.to_str().unwrap(), "log", "--oneline"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        let commits_after = String::from_utf8_lossy(&after.stdout)
            .lines()
            .count();
        assert_eq!(commits_before, commits_after, "no commit should have been created when CI fails");
    }

    #[test]
    fn test_ci_sentinel_fresh_skips_ci() {
        let (clone_dir, _bare_dir) = setup_integration_repo_with_ci();
        let clone_path = clone_dir.path();

        // Stage a file to commit first — this changes the tree snapshot,
        // so the sentinel must be created AFTER staging.
        fs::write(clone_path.join("feature.rs"), "fn main() {}\n").unwrap();
        Command::new("git")
            .args(["-C", clone_path.to_str().unwrap(), "add", "feature.rs"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();

        // Write commit message (must exist before CI so the untracked-file
        // list is the same when run_impl re-checks the snapshot)
        let msg_path = clone_path.join(".flow-commit-msg");
        fs::write(&msg_path, "Add feature.rs").unwrap();

        // Run CI once to create a passing sentinel for THIS tree state
        let bin_ci = clone_path.join("bin").join("ci");
        let (_ci_result, ci_code) =
            crate::ci::run_once(clone_path, clone_path, &bin_ci, Some("main"), false, None);
        assert_eq!(ci_code, 0, "initial CI run should pass");

        // Marker should have exactly 1 line from the CI invocation
        let marker = clone_path.join(".ci-invocation-marker");
        let lines_before = fs::read_to_string(&marker)
            .unwrap()
            .lines()
            .count();
        assert_eq!(lines_before, 1, "CI should have been invoked once");

        // Now call run_impl — sentinel matches the current tree state,
        // so ci::run_once inside run_impl should skip without invoking bin/ci.
        let args = Args {
            message_file: msg_path.to_str().unwrap().to_string(),
            branch: "main".to_string(),
        };

        let result = run_impl(&args, clone_path, clone_path).unwrap();
        assert_eq!(result["status"], "ok", "commit should succeed");
        assert!(!result["sha"].as_str().unwrap().is_empty(), "should have a commit SHA");

        // Marker should still have only 1 line — CI was skipped via sentinel
        let lines_after = fs::read_to_string(&marker)
            .unwrap()
            .lines()
            .count();
        assert_eq!(
            lines_after, 1,
            "CI should not have been invoked again (sentinel was fresh)"
        );
    }

    // --- Integration tests for run_impl sentinel refresh ---

    /// Set up a bare remote + clone with a passing bin/ci script and .flow-states dir.
    /// Returns (clone_dir, bare_dir) as TempDirs that must be kept alive.
    fn setup_integration_repo() -> (tempfile::TempDir, tempfile::TempDir) {
        let bare_dir = tempfile::tempdir().unwrap();
        let clone_dir = tempfile::tempdir().unwrap();

        // Create bare remote with explicit branch name — without --initial-branch,
        // the default branch depends on the system git config (master vs main),
        // causing test failures on CI runners where the default is master.
        Command::new("git")
            .args(["init", "--bare", "--initial-branch", "main"])
            .arg(bare_dir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();

        // Clone it
        Command::new("git")
            .args(["clone"])
            .arg(bare_dir.path())
            .arg(clone_dir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();

        // Configure git user and merge behavior in clone
        let clone_str = clone_dir.path().to_str().unwrap();
        Command::new("git")
            .args(["-C", clone_str, "config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .args(["-C", clone_str, "config", "user.name", "Test"])
            .output()
            .unwrap();
        // Force merge on pull (not rebase) so divergent pulls always create merge commits
        Command::new("git")
            .args(["-C", clone_str, "config", "pull.rebase", "false"])
            .output()
            .unwrap();

        // Create .flow-states dir (gitignored, as in real FLOW projects)
        let flow_states = clone_dir.path().join(".flow-states");
        fs::create_dir_all(&flow_states).unwrap();
        let gitignore = clone_dir.path().join(".gitignore");
        fs::write(&gitignore, ".flow-states/\n").unwrap();

        // Create an initial commit so the branch exists
        let readme = clone_dir.path().join("README.md");
        fs::write(&readme, "# Test\n").unwrap();
        Command::new("git")
            .args(["-C", clone_dir.path().to_str().unwrap(), "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        Command::new("git")
            .args(["-C", clone_dir.path().to_str().unwrap(), "commit", "-m", "Initial commit"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        Command::new("git")
            .args(["-C", clone_dir.path().to_str().unwrap(), "push", "-u", "origin", "main"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();

        (clone_dir, bare_dir)
    }

    #[test]
    fn run_impl_refreshes_sentinel_after_commit() {
        let (clone_dir, _bare_dir) = setup_integration_repo_with_ci();
        let clone_path = clone_dir.path().to_str().unwrap().to_string();

        // Create a file to commit
        let src = clone_dir.path().join("src.rs");
        fs::write(&src, "fn main() {}\n").unwrap();
        Command::new("git")
            .args(["-C", &clone_path, "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();

        // Write commit message file (absolute path since git runs via -C)
        let msg_path = clone_dir.path().join(".flow-commit-msg");
        fs::write(&msg_path, "Add src.rs").unwrap();

        let args = Args {
            message_file: msg_path.to_str().unwrap().to_string(),
            branch: "main".to_string(),
        };

        // Pass clone_dir as both cwd and root (standalone repo, not a worktree)
        let result = run_impl(&args, clone_dir.path(), clone_dir.path()).unwrap();
        assert_eq!(result["status"], "ok");
        assert_eq!(result["pull_merged"], false);

        // Sentinel should exist and match tree_snapshot for new HEAD
        let sentinel = crate::ci::sentinel_path(clone_dir.path(), "main");
        assert!(sentinel.exists(), "sentinel file should exist after clean commit");

        // Verify sentinel contains a valid SHA-256 hex string (structural check).
        // Comparing against a live tree_snapshot() call would be tautological —
        // both compute the same hash from the same post-commit state.
        let sentinel_content = fs::read_to_string(&sentinel).unwrap();
        assert_eq!(sentinel_content.len(), 64, "sentinel should be a SHA-256 hex string");
        assert!(sentinel_content.chars().all(|c| c.is_ascii_hexdigit()),
            "sentinel should contain only hex digits");
    }

    #[test]
    fn run_impl_no_sentinel_refresh_when_pull_merges() {
        let (clone_dir, bare_dir) = setup_integration_repo_with_ci();
        let clone_path = clone_dir.path().to_str().unwrap().to_string();

        // Create a second clone to push a divergent commit
        let clone2_dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["clone"])
            .arg(bare_dir.path())
            .arg(clone2_dir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        Command::new("git")
            .args(["-C", clone2_dir.path().to_str().unwrap(), "config", "user.email", "other@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .args(["-C", clone2_dir.path().to_str().unwrap(), "config", "user.name", "Other"])
            .output()
            .unwrap();

        // Push a different commit from clone2
        let other_file = clone2_dir.path().join("other.txt");
        fs::write(&other_file, "other content\n").unwrap();
        Command::new("git")
            .args(["-C", clone2_dir.path().to_str().unwrap(), "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        Command::new("git")
            .args(["-C", clone2_dir.path().to_str().unwrap(), "commit", "-m", "Other commit"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        Command::new("git")
            .args(["-C", clone2_dir.path().to_str().unwrap(), "push"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();

        // Now create a commit in clone1 (divergent from remote)
        let src = clone_dir.path().join("local.txt");
        fs::write(&src, "local content\n").unwrap();
        Command::new("git")
            .args(["-C", &clone_path, "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();

        let msg_path = clone_dir.path().join(".flow-commit-msg");
        fs::write(&msg_path, "Add local.txt").unwrap();

        let args = Args {
            message_file: msg_path.to_str().unwrap().to_string(),
            branch: "main".to_string(),
        };

        let result = run_impl(&args, clone_dir.path(), clone_dir.path()).unwrap();
        assert_eq!(result["status"], "ok");
        assert_eq!(result["pull_merged"], true);

        // Sentinel should NOT exist — pull merged remote changes
        let sentinel = crate::ci::sentinel_path(clone_dir.path(), "main");
        assert!(!sentinel.exists(), "sentinel should not exist when pull merged");
    }
}
