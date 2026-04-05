//! Port of lib/close-issues.py — close GitHub issues referenced in the FLOW start prompt.
//!
//! Reads the state file, extracts #N patterns from the prompt field,
//! and closes each issue via gh CLI after the PR is merged.
//!
//! Usage: bin/flow close-issues --state-file <path>
//!
//! Output (JSON to stdout):
//!   {"status": "ok", "closed": [{"number": 83, "url": "..."}], "failed": [{"number": 89, "error": "not found"}]}

use std::fs;
use std::process::Command;
use std::time::Duration;

use clap::Parser;
use serde_json::json;

use crate::output::{json_error, json_ok};
use crate::utils::extract_issue_numbers;

const LOCAL_TIMEOUT: u64 = 30;

#[derive(Parser, Debug)]
#[command(name = "close-issues", about = "Close issues from FLOW prompt")]
pub struct Args {
    /// Path to state JSON file
    #[arg(long = "state-file")]
    pub state_file: String,
}

/// Close each issue via gh CLI. Returns closed and failed lists.
///
/// When repo is provided, closed items include URLs.
pub fn close_issues(
    issue_numbers: &[i64],
    repo: Option<&str>,
) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
    let mut closed = Vec::new();
    let mut failed = Vec::new();
    let timeout = Duration::from_secs(LOCAL_TIMEOUT);

    for &num in issue_numbers {
        match close_single_issue(num, repo, timeout) {
            Ok(()) => {
                let mut entry = serde_json::Map::new();
                entry.insert("number".to_string(), json!(num));
                if let Some(r) = repo {
                    entry.insert(
                        "url".to_string(),
                        json!(format!("https://github.com/{}/issues/{}", r, num)),
                    );
                }
                closed.push(serde_json::Value::Object(entry));
            }
            Err(e) => {
                failed.push(json!({"number": num, "error": e}));
            }
        }
    }

    (closed, failed)
}

/// Run a subprocess with a timeout, returning (exit_code, stdout_bytes, stderr_bytes).
///
/// Drains stdout and stderr in spawned reader threads before the poll loop
/// to prevent pipe buffer deadlock on outputs larger than ~64KB. Joins reader
/// threads on every exit path (success, timeout, try_wait error).
///
/// The `program` parameter is test-injectable — production always passes the gh CLI.
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
                    return Err("timeout".to_string());
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

fn close_single_issue(
    number: i64,
    repo: Option<&str>,
    timeout: Duration,
) -> Result<(), String> {
    let num_str = number.to_string();
    let mut args: Vec<&str> = vec!["issue", "close", &num_str];
    if let Some(r) = repo {
        args.push("--repo");
        args.push(r);
    }

    match run_gh_close_with_timeout("gh", &args, timeout) {
        Ok((0, _, _)) => Ok(()),
        Ok((_, stdout_bytes, stderr_bytes)) => {
            let stderr = String::from_utf8_lossy(&stderr_bytes).trim().to_string();
            if !stderr.is_empty() {
                Err(stderr)
            } else {
                Err(String::from_utf8_lossy(&stdout_bytes).trim().to_string())
            }
        }
        Err(e) => Err(e),
    }
}

pub fn run(args: Args) {
    let content = match fs::read_to_string(&args.state_file) {
        Ok(c) => c,
        Err(e) => {
            json_error(
                &format!("Could not read state file: {}", e),
                &[],
            );
            std::process::exit(1);
        }
    };

    let state: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            json_error(
                &format!("Could not read state file: {}", e),
                &[],
            );
            std::process::exit(1);
        }
    };

    let prompt = state
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let repo = state.get("repo").and_then(|v| v.as_str());
    let issue_numbers = extract_issue_numbers(prompt);

    let (closed, failed) = close_issues(&issue_numbers, repo);

    json_ok(&[
        ("closed", serde_json::Value::Array(closed)),
        ("failed", serde_json::Value::Array(failed)),
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // --- CLI integration: run() reads state file ---

    #[test]
    fn run_no_prompt_outputs_empty_lists() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("state.json");
        fs::write(
            &state_file,
            r#"{"branch": "test"}"#,
        )
        .unwrap();

        // Can't easily test run() because it calls process::exit and json_ok prints to stdout.
        // Verify the logic path instead: extract_issue_numbers on empty prompt.
        let issue_numbers = extract_issue_numbers("");
        assert!(issue_numbers.is_empty());

        let (closed, failed) = close_issues(&issue_numbers, None);
        assert!(closed.is_empty());
        assert!(failed.is_empty());
    }

    #[test]
    fn close_issues_empty_list() {
        let (closed, failed) = close_issues(&[], None);
        assert!(closed.is_empty());
        assert!(failed.is_empty());
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
        assert_eq!(result.unwrap_err(), "timeout");
        assert!(
            elapsed < Duration::from_secs(5),
            "timeout not enforced: elapsed {:?}",
            elapsed
        );
    }
}
