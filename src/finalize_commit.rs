//! Port of lib/finalize-commit.py — commit, cleanup, pull, push.
//!
//! Usage:
//!   bin/flow finalize-commit <message-file> <branch>
//!
//! Output (JSON to stdout):
//!   Success:   {"status": "ok", "sha": "<commit-hash>"}
//!   Warning:   {"status": "ok", "sha": "", "warning": "..."}
//!   Conflict:  {"status": "conflict", "files": ["file1.py", ...]}
//!   Error:     {"status": "error", "step": "commit|pull|push", "message": "..."}

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
            "warning": "commit succeeded but SHA retrieval timed out"
        }),
        Ok((code, _, _)) if code != 0 => json!({
            "status": "ok",
            "sha": "",
            "warning": "commit succeeded but SHA retrieval failed"
        }),
        Ok((_, stdout, _)) => json!({"status": "ok", "sha": stdout.trim()}),
    }
}

/// Run finalize-commit with real git subprocess.
pub fn finalize_commit(message_file: &str, branch: &str) -> Value {
    finalize_commit_inner(message_file, branch, &run_git_with_timeout)
}

pub fn run(args: Args) {
    if args.message_file.is_empty() || args.branch.is_empty() {
        json_error(
            "Usage: bin/flow finalize-commit <message-file> <branch>",
            &[("step", json!("args"))],
        );
        std::process::exit(1);
    }
    let result = finalize_commit(&args.message_file, &args.branch);
    println!("{}", result);
    if result["status"] != "ok" {
        std::process::exit(1);
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
            ok(""),                // git pull
            ok(""),                // git push
            ok("abc123\n"),        // git rev-parse HEAD
        ]);

        let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
        assert_eq!(result["status"], "ok");
        assert_eq!(result["sha"], "abc123");
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
            ok(""),                // git pull
            ok(""),                // git push
            ok("def456\n"),        // git rev-parse HEAD
        ]);

        let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
        assert_eq!(result["status"], "ok");
    }

    #[test]
    fn rev_parse_failure() {
        let dir = tempfile::tempdir().unwrap();
        let msg = dir.path().join(".flow-commit-msg");
        std::fs::write(&msg, "Test commit.").unwrap();

        let git = mock_git(vec![
            ok(""),               // git commit
            ok(""),               // git pull
            ok(""),               // git push
            fail("bad HEAD"),     // git rev-parse HEAD
        ]);

        let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
        assert_eq!(result["status"], "ok");
        assert_eq!(result["sha"], "");
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
            ok(""),                                // git pull
            ok(""),                                // git push
            timeout("timed out after 30s"),        // git rev-parse HEAD
        ]);

        let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
        assert_eq!(result["status"], "ok");
        assert_eq!(result["sha"], "");
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
}
