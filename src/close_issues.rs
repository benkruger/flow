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

fn close_single_issue(number: i64, repo: Option<&str>, timeout: Duration) -> Result<(), String> {
    let mut cmd_args = vec!["gh", "issue", "close"];
    let num_str = number.to_string();
    cmd_args.push(&num_str);
    if let Some(r) = repo {
        cmd_args.push("--repo");
        cmd_args.push(r);
    }

    let mut child = Command::new(cmd_args[0])
        .args(&cmd_args[1..])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn: {}", e))?;

    let start = std::time::Instant::now();
    let poll_interval = Duration::from_millis(50);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let output = child.wait_with_output().map_err(|e| e.to_string())?;
                if output.status.success() {
                    return Ok(());
                }
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                return Err(stderr);
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("timeout".to_string());
                }
                std::thread::sleep(poll_interval.min(timeout - start.elapsed()));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

pub fn run(args: Args) {
    let content = match fs::read_to_string(&args.state_file) {
        Ok(c) => c,
        Err(e) => {
            json_error(&format!("Could not read state file: {}", e), &[]);
            std::process::exit(1);
        }
    };

    let state: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            json_error(&format!("Could not read state file: {}", e), &[]);
            std::process::exit(1);
        }
    };

    let prompt = state.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
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
        fs::write(&state_file, r#"{"branch": "test"}"#).unwrap();

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
}
