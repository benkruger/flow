//! Close a single GitHub issue via gh CLI.
//!
//! Usage:
//!   bin/flow close-issue --number <N> [--repo <repo>]
//!
//! Output (JSON to stdout):
//!   Success: {"status": "ok"}
//!   Error:   {"status": "error", "message": "..."}
//!
//! Tests live at tests/close_issue.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use std::process::{Child, Command, Stdio};
use std::time::Duration;

use clap::Parser;
use serde_json::{json, Value};

use crate::complete_preflight::LOCAL_TIMEOUT;

#[derive(Parser, Debug)]
#[command(name = "close-issue", about = "Close a GitHub issue")]
pub struct Args {
    /// Repository (owner/name)
    #[arg(long)]
    pub repo: Option<String>,

    /// Issue number
    #[arg(long)]
    pub number: i64,
}

/// Close a GitHub issue via injected child_factory. Returns Some(error)
/// on failure or None on success. Tests inject sh/sleep child factories
/// to exercise the success, non-zero-exit, timeout, and spawn-error
/// branches without spawning real `gh`.
pub fn close_issue_with_runner(
    repo: &str,
    number: i64,
    child_factory: &dyn Fn(&[&str]) -> std::io::Result<Child>,
) -> Option<String> {
    close_issue_with_runner_and_timeout(repo, number, LOCAL_TIMEOUT, child_factory)
}

/// Seam-injected variant of [`close_issue_with_runner`] that accepts a
/// custom timeout (in seconds). Tests pass `0` so the elapsed-time check
/// fires on the first poll and the timeout-arm message is exercised.
pub fn close_issue_with_runner_and_timeout(
    repo: &str,
    number: i64,
    timeout_secs: u64,
    child_factory: &dyn Fn(&[&str]) -> std::io::Result<Child>,
) -> Option<String> {
    let timeout = Duration::from_secs(timeout_secs);

    let args: Vec<String> = vec![
        "issue".to_string(),
        "close".to_string(),
        "--repo".to_string(),
        repo.to_string(),
        number.to_string(),
    ];
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    let mut child = match child_factory(&arg_refs) {
        Ok(c) => c,
        Err(e) => return Some(format!("Failed to spawn: {}", e)),
    };

    let start = std::time::Instant::now();
    let poll_interval = Duration::from_millis(50);
    loop {
        if let Ok(Some(_)) = child.try_wait() {
            let bytes_output = child
                .wait_with_output()
                .map(|o| (o.status.success(), o.stdout, o.stderr))
                .unwrap_or((false, Vec::new(), Vec::new()));
            let (success, stdout_bytes, stderr_bytes) = bytes_output;
            if success {
                return None;
            }
            let stderr = String::from_utf8_lossy(&stderr_bytes).trim().to_string();
            let stdout = String::from_utf8_lossy(&stdout_bytes).trim().to_string();
            if !stderr.is_empty() {
                return Some(stderr);
            }
            if !stdout.is_empty() {
                return Some(stdout);
            }
            return Some("Unknown error".to_string());
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Some(format!("Command timed out after {} seconds", timeout_secs));
        }
        std::thread::sleep(poll_interval.min(timeout - start.elapsed()));
    }
}

/// Close a GitHub issue and return error message or None on success.
pub fn close_issue_by_number(repo: &str, number: i64) -> Option<String> {
    close_issue_with_runner(repo, number, &|args| {
        Command::new("gh")
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    })
}

/// Main-arm dispatcher with injected repo_resolver. Returns
/// `(value, exit_code)`. The repo_resolver closure returns the detected
/// repo (or None when `git remote` has no origin); production binds it
/// to `detect_repo(None)`. Tests pass closures returning Some/None.
pub fn run_impl_main(args: Args, repo_resolver: &dyn Fn() -> Option<String>) -> (Value, i32) {
    let repo = match args.repo {
        Some(r) => r,
        None => match repo_resolver() {
            Some(r) => r,
            None => {
                return (
                    json!({"status": "error", "message": "Could not detect repo from git remote. Use --repo owner/name."}),
                    1,
                );
            }
        },
    };

    if let Some(e) = close_issue_by_number(&repo, args.number) {
        return (json!({"status": "error", "message": e}), 1);
    }

    (json!({"status": "ok"}), 0)
}
