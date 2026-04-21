//! Close GitHub issues referenced in the FLOW start prompt.
//!
//! Reads the state file, extracts #N patterns from the prompt field,
//! and closes each issue via gh CLI after the PR is merged.
//!
//! Usage: bin/flow close-issues --state-file <path>
//!
//! Output (JSON to stdout):
//!   {"status": "ok", "closed": [{"number": 83, "url": "..."}], "failed": [{"number": 89, "error": "not found"}]}
//!
//! Tests live at tests/close_issues.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use std::fs;
use std::process::Command;

use clap::Parser;
use serde_json::{json, Value};

use crate::utils::extract_issue_numbers;

#[derive(Parser, Debug)]
#[command(name = "close-issues", about = "Close issues from FLOW prompt")]
pub struct Args {
    /// Path to state JSON file
    #[arg(long = "state-file")]
    pub state_file: String,
}

/// Close a single issue via gh CLI and return Ok on success or Err with
/// the stderr text or spawn-failure message. gh has its own network
/// timeout so no hand-rolled loop is needed per
/// .claude/rules/testability-means-simplicity.md.
fn close_single_issue(number: i64, repo: Option<&str>) -> Result<(), String> {
    let mut cmd_args = vec!["issue", "close"];
    let num_str = number.to_string();
    cmd_args.push(&num_str);
    if let Some(r) = repo {
        cmd_args.push("--repo");
        cmd_args.push(r);
    }

    let output = match Command::new("gh").args(&cmd_args).output() {
        Ok(o) => o,
        Err(e) => return Err(format!("Failed to spawn: {}", e)),
    };
    if output.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
}

/// Production main-arm dispatcher: reads the state file, extracts issue
/// numbers from the prompt, and closes each via `gh issue close`.
pub fn run_impl_main(args: Args) -> (Value, i32) {
    let content = match fs::read_to_string(&args.state_file) {
        Ok(c) => c,
        Err(e) => {
            return (
                json!({"status": "error", "message": format!("Could not read state file: {}", e)}),
                1,
            );
        }
    };

    let state: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            return (
                json!({"status": "error", "message": format!("Could not read state file: {}", e)}),
                1,
            );
        }
    };

    let prompt = match state.get("prompt") {
        Some(v) => v.as_str().unwrap_or(""),
        None => "",
    };
    let repo = match state.get("repo") {
        Some(v) => v.as_str(),
        None => None,
    };
    let issue_numbers = extract_issue_numbers(prompt);

    let mut closed = Vec::new();
    let mut failed = Vec::new();
    for num in issue_numbers {
        match close_single_issue(num, repo) {
            Ok(()) => {
                let mut entry = serde_json::Map::new();
                entry.insert("number".to_string(), json!(num));
                if let Some(r) = repo {
                    entry.insert(
                        "url".to_string(),
                        json!(format!("https://github.com/{}/issues/{}", r, num)),
                    );
                }
                closed.push(Value::Object(entry));
            }
            Err(e) => {
                failed.push(json!({"number": num, "error": e}));
            }
        }
    }

    (
        json!({
            "status": "ok",
            "closed": closed,
            "failed": failed,
        }),
        0,
    )
}
