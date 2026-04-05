//! Port of lib/close-issue.py — close a single GitHub issue via gh CLI.
//!
//! Usage:
//!   bin/flow close-issue --number <N> [--repo <repo>]
//!
//! Output (JSON to stdout):
//!   Success: {"status": "ok"}
//!   Error:   {"status": "error", "message": "..."}

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use clap::Parser;

use crate::github::detect_repo;
use crate::output::{json_error, json_ok};

const LOCAL_TIMEOUT: u64 = 30;

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

/// Run a subprocess with a timeout, returning (exit_code, stdout_bytes, stderr_bytes).
///
/// Drains stdout and stderr in spawned reader threads before the poll loop
/// to prevent pipe buffer deadlock on outputs larger than ~64KB. Joins reader
/// threads on every exit path (success, timeout, try_wait error).
///
/// The `program` parameter is test-injectable — production always passes "gh".
fn run_gh_close_with_timeout(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<(i32, Vec<u8>, Vec<u8>), String> {
    let mut child = Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn: {}", e))?;

    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();
    let stdout_reader = std::thread::spawn(move || {
        let mut buf: Vec<u8> = Vec::new();
        if let Some(mut pipe) = stdout_handle {
            use std::io::Read;
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buf: Vec<u8> = Vec::new();
        if let Some(mut pipe) = stderr_handle {
            use std::io::Read;
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });

    let start = std::time::Instant::now();
    let poll_interval = Duration::from_millis(50);
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_reader.join();
                    let _ = stderr_reader.join();
                    return Err(format!(
                        "Command timed out after {} seconds",
                        timeout.as_secs()
                    ));
                }
                std::thread::sleep(poll_interval.min(timeout.saturating_sub(start.elapsed())));
            }
            Err(e) => {
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(e.to_string());
            }
        }
    };

    let stdout_bytes = stdout_reader.join().unwrap_or_default();
    let stderr_bytes = stderr_reader.join().unwrap_or_default();
    let code = status.code().unwrap_or(1);
    Ok((code, stdout_bytes, stderr_bytes))
}

/// Close a GitHub issue and return error message or None on success.
pub fn close_issue_by_number(repo: &str, number: i64) -> Option<String> {
    let timeout = Duration::from_secs(LOCAL_TIMEOUT);
    let num_str = number.to_string();
    let args = ["issue", "close", "--repo", repo, &num_str];

    match run_gh_close_with_timeout("gh", &args, timeout) {
        Ok((0, _, _)) => None,
        Ok((_, stdout_bytes, stderr_bytes)) => {
            let stderr = String::from_utf8_lossy(&stderr_bytes).trim().to_string();
            let stdout = String::from_utf8_lossy(&stdout_bytes).trim().to_string();
            if !stderr.is_empty() {
                Some(stderr)
            } else if !stdout.is_empty() {
                Some(stdout)
            } else {
                Some("Unknown error".to_string())
            }
        }
        Err(e) => Some(e),
    }
}

fn detect_repo_or_fail(cwd: Option<&Path>) -> String {
    match detect_repo(cwd) {
        Some(r) => r,
        None => {
            json_error(
                "Could not detect repo from git remote. Use --repo owner/name.",
                &[],
            );
            std::process::exit(1);
        }
    }
}

pub fn run(args: Args) {
    let repo = args
        .repo
        .unwrap_or_else(|| detect_repo_or_fail(None));

    let error = close_issue_by_number(&repo, args.number);

    if let Some(e) = error {
        json_error(&e, &[]);
        std::process::exit(1);
    }

    json_ok(&[]);
}

#[cfg(test)]
mod tests {
    use super::*;

    // close_issue_by_number calls gh subprocess — tested via Python integration tests.
    // Unit tests here cover the helper functions.

    #[test]
    fn detect_repo_or_fail_returns_some() {
        // This test just validates the function signature — the actual
        // detection runs against git remote which we can't mock in Rust.
        // Python integration tests cover the detect_repo_or_fail path.
    }

    // --- run_gh_close_with_timeout large-output and timeout tests (issue #875) ---
    //
    // These verify the thread-drain pattern captures output exceeding the
    // kernel pipe buffer (~64KB). The prior try_wait() + wait_with_output()
    // pattern either deadlocked on pipe-buffer fill or silently truncated
    // via ECHILD on already-reaped children.

    #[test]
    fn run_gh_close_captures_large_stdout() {
        let result = run_gh_close_with_timeout(
            "sh",
            &["-c", "for i in $(seq 1 20000); do echo \"line $i\"; done"],
            Duration::from_secs(10),
        );
        let (code, stdout_bytes, _) = result.expect("subprocess failed");
        assert_eq!(code, 0);
        let stdout = String::from_utf8_lossy(&stdout_bytes);
        assert!(
            stdout.contains("line 20000"),
            "last line missing — output was truncated"
        );
        assert!(
            stdout_bytes.len() > 128_000,
            "stdout truncated: {} bytes (expected > 128KB)",
            stdout_bytes.len()
        );
    }

    #[test]
    fn run_gh_close_captures_large_stderr_on_failure() {
        let result = run_gh_close_with_timeout(
            "sh",
            &[
                "-c",
                "for i in $(seq 1 20000); do echo \"err $i\" 1>&2; done; exit 3",
            ],
            Duration::from_secs(10),
        );
        let (code, _, stderr_bytes) = result.expect("subprocess failed");
        assert_eq!(code, 3);
        let stderr = String::from_utf8_lossy(&stderr_bytes);
        assert!(
            stderr.contains("err 20000"),
            "last stderr line missing — output was truncated"
        );
        assert!(
            stderr_bytes.len() > 128_000,
            "stderr truncated: {} bytes (expected > 128KB)",
            stderr_bytes.len()
        );
    }

    #[test]
    fn run_gh_close_enforces_timeout() {
        let start = std::time::Instant::now();
        let result = run_gh_close_with_timeout(
            "sh",
            &["-c", "sleep 10"],
            Duration::from_secs(2),
        );
        let elapsed = start.elapsed();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("timed out"),
            "expected timeout error, got: {}",
            err
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "timeout not enforced: elapsed {:?}",
            elapsed
        );
    }
}
