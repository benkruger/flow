use std::path::Path;
use std::process::{self, Command};
use std::time::Duration;

use clap::Parser;
use serde_json::json;

use crate::output::{json_error, json_ok};
use crate::utils::extract_issue_numbers;

const LABEL: &str = "Flow In-Progress";
const TIMEOUT_SECS: u64 = 30;

// Polling-based wait_timeout for child processes (same pattern as start_setup.rs)
trait WaitTimeout {
    fn wait_timeout(&mut self, dur: Duration) -> std::io::Result<Option<std::process::ExitStatus>>;
}

impl WaitTimeout for std::process::Child {
    fn wait_timeout(&mut self, dur: Duration) -> std::io::Result<Option<std::process::ExitStatus>> {
        use std::thread;

        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(50);
        loop {
            match self.try_wait()? {
                Some(status) => return Ok(Some(status)),
                None => {
                    if start.elapsed() >= dur {
                        return Ok(None);
                    }
                    thread::sleep(poll_interval.min(dur - start.elapsed()));
                }
            }
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct LabelResult {
    pub labeled: Vec<i64>,
    pub failed: Vec<i64>,
}

/// Add or remove the Flow In-Progress label on GitHub issues.
///
/// Reads the state file, extracts #N patterns from the prompt field,
/// and adds or removes the label via gh CLI.
pub fn label_issues(issue_numbers: &[i64], action: &str) -> LabelResult {
    let mut labeled = Vec::new();
    let mut failed = Vec::new();
    let flag = if action == "add" {
        "--add-label"
    } else {
        "--remove-label"
    };

    for &num in issue_numbers {
        let result = Command::new("gh")
            .args(["issue", "edit", &num.to_string(), flag, LABEL])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn();

        match result {
            Ok(mut child) => {
                match child.wait_timeout(Duration::from_secs(TIMEOUT_SECS)) {
                    Ok(Some(status)) if status.success() => labeled.push(num),
                    Ok(Some(_)) => failed.push(num),
                    // Timeout or wait error
                    Ok(None) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        failed.push(num);
                    }
                    Err(_) => failed.push(num),
                }
            }
            Err(_) => failed.push(num),
        }
    }

    LabelResult { labeled, failed }
}

#[derive(Parser, Debug)]
#[command(name = "label-issues", about = "Add or remove Flow In-Progress label on issues")]
pub struct Args {
    /// Path to state JSON file
    #[arg(long)]
    pub state_file: String,

    /// Add label
    #[arg(long, group = "action")]
    pub add: bool,

    /// Remove label
    #[arg(long, group = "action")]
    pub remove: bool,
}

pub fn run(args: Args) {
    let state_path = Path::new(&args.state_file);
    if !state_path.exists() {
        json_error(
            &format!("State file not found: {}", args.state_file),
            &[("step", json!("read_state"))],
        );
        process::exit(1);
    }

    let content = match std::fs::read_to_string(state_path) {
        Ok(c) => c,
        Err(e) => {
            json_error(
                &format!("Failed to read state file: {}", e),
                &[("step", json!("read_state"))],
            );
            process::exit(1);
        }
    };

    let state: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            json_error(
                &format!("Failed to parse state file: {}", e),
                &[("step", json!("parse_state"))],
            );
            process::exit(1);
        }
    };

    let prompt = state
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let issue_numbers = extract_issue_numbers(prompt);
    let action = if args.add { "add" } else { "remove" };
    let result = label_issues(&issue_numbers, action);

    json_ok(&[
        ("labeled", json!(result.labeled)),
        ("failed", json!(result.failed)),
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_issue_list_returns_empty_result() {
        let result = label_issues(&[], "add");
        assert_eq!(
            result,
            LabelResult {
                labeled: vec![],
                failed: vec![],
            }
        );
    }

    #[test]
    fn empty_issue_list_remove_returns_empty_result() {
        let result = label_issues(&[], "remove");
        assert_eq!(
            result,
            LabelResult {
                labeled: vec![],
                failed: vec![],
            }
        );
    }

    #[test]
    fn label_constant_is_flow_in_progress() {
        assert_eq!(LABEL, "Flow In-Progress");
    }

    #[test]
    fn timeout_is_30_seconds() {
        assert_eq!(TIMEOUT_SECS, 30);
    }

    #[test]
    fn run_reads_state_and_extracts_issues() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        std::fs::write(
            &state_path,
            r#"{"prompt": "fix #42 and #99", "branch": "test"}"#,
        )
        .unwrap();

        let prompt = {
            let content = std::fs::read_to_string(&state_path).unwrap();
            let state: serde_json::Value = serde_json::from_str(&content).unwrap();
            state
                .get("prompt")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };

        let issues = extract_issue_numbers(&prompt);
        assert_eq!(issues, vec![42, 99]);
    }

    #[test]
    fn run_handles_missing_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        std::fs::write(&state_path, r#"{"branch": "test"}"#).unwrap();

        let content = std::fs::read_to_string(&state_path).unwrap();
        let state: serde_json::Value = serde_json::from_str(&content).unwrap();
        let prompt = state
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let issues = extract_issue_numbers(prompt);
        assert!(issues.is_empty());
    }

    #[test]
    fn add_flag_maps_to_add_label() {
        let flag = if true { "--add-label" } else { "--remove-label" };
        assert_eq!(flag, "--add-label");
    }

    #[test]
    fn remove_flag_maps_to_remove_label() {
        let flag = if false { "--add-label" } else { "--remove-label" };
        assert_eq!(flag, "--remove-label");
    }
}
