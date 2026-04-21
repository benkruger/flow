//! Add or remove the "Flow In-Progress" label on issues referenced in
//! a FLOW start prompt.
//!
//! Usage:
//!   bin/flow label-issues --state-file <path> --add
//!   bin/flow label-issues --state-file <path> --remove
//!
//! Output (JSON to stdout):
//!   {"status": "ok", "labeled": [42], "failed": []}
//!
//! Tests live at tests/label_issues.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use std::path::Path;
use std::process::Command;

use clap::Parser;
use serde_json::{json, Value};

use crate::utils::extract_issue_numbers;

pub const LABEL: &str = "Flow In-Progress";

#[derive(Debug, PartialEq)]
pub struct LabelResult {
    pub labeled: Vec<i64>,
    pub failed: Vec<i64>,
}

/// Add or remove the Flow In-Progress label on each issue via `gh issue
/// edit`. Returns the per-issue success/failure partition. gh has its
/// own network timeout so no hand-rolled loop is needed per
/// .claude/rules/testability-means-simplicity.md.
pub fn label_issues(issue_numbers: &[i64], action: &str) -> LabelResult {
    let mut labeled = Vec::new();
    let mut failed = Vec::new();
    let flag = if action == "add" {
        "--add-label"
    } else {
        "--remove-label"
    };

    for &num in issue_numbers {
        let num_str = num.to_string();
        let args = ["issue", "edit", num_str.as_str(), flag, LABEL];
        match Command::new("gh").args(args).output() {
            Ok(output) if output.status.success() => labeled.push(num),
            _ => failed.push(num),
        }
    }

    LabelResult { labeled, failed }
}

#[derive(Parser, Debug)]
#[command(
    name = "label-issues",
    about = "Add or remove Flow In-Progress label on issues"
)]
#[command(group(clap::ArgGroup::new("action").args(["add", "remove"]).required(true)))]
pub struct Args {
    /// Path to state JSON file
    #[arg(long)]
    pub state_file: String,

    /// Add label
    #[arg(long)]
    pub add: bool,

    /// Remove label
    #[arg(long)]
    pub remove: bool,
}

/// Production main-arm dispatcher: reads the state file, extracts issue
/// numbers from the prompt, and adds or removes the label.
pub fn run_impl_main(args: Args) -> (Value, i32) {
    let state_path = Path::new(&args.state_file);
    if !state_path.exists() {
        return (
            json!({
                "status": "error",
                "step": "read_state",
                "message": format!("State file not found: {}", args.state_file),
            }),
            1,
        );
    }

    let content = match std::fs::read_to_string(state_path) {
        Ok(c) => c,
        Err(e) => {
            return (
                json!({
                    "status": "error",
                    "step": "read_state",
                    "message": format!("Failed to read state file: {}", e),
                }),
                1,
            );
        }
    };

    let state: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            return (
                json!({
                    "status": "error",
                    "step": "parse_state",
                    "message": format!("Failed to parse state file: {}", e),
                }),
                1,
            );
        }
    };

    let prompt = match state.get("prompt") {
        Some(v) => v.as_str().unwrap_or(""),
        None => "",
    };
    let issue_numbers = extract_issue_numbers(prompt);
    let action = if args.add { "add" } else { "remove" };
    let result = label_issues(&issue_numbers, action);

    (
        json!({
            "status": "ok",
            "labeled": result.labeled,
            "failed": result.failed,
        }),
        0,
    )
}
