//! Create a GitHub sub-issue relationship.
//!
//! Usage:
//!   bin/flow create-sub-issue --repo <owner/repo> --parent-number N --child-number N
//!
//! Resolves both issue numbers to database IDs (required by the REST API),
//! then creates the sub-issue relationship.
//!
//! Output (JSON to stdout):
//!   Success: {"status": "ok", "parent": N, "child": N}
//!   Error:   {"status": "error", "message": "..."}
//!
//! Tests live at tests/create_sub_issue.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use clap::Parser;
use serde_json::json;

use crate::issue::{fetch_database_id, run_gh_cmd};

#[derive(Parser, Debug)]
#[command(
    name = "create-sub-issue",
    about = "Create a GitHub sub-issue relationship"
)]
pub struct Args {
    /// Repository (owner/name)
    #[arg(long)]
    pub repo: String,

    /// Parent issue number
    #[arg(long = "parent-number")]
    pub parent_number: i64,

    /// Child issue number
    #[arg(long = "child-number")]
    pub child_number: i64,
}

/// Create a sub-issue relationship between two issues.
///
/// Returns Ok((parent, child)) on success or Err(message) on failure.
pub fn create_sub_issue(
    repo: &str,
    parent_number: i64,
    child_number: i64,
) -> Result<(i64, i64), String> {
    if parent_number == child_number {
        return Err(format!(
            "Cannot create self-reference: issue #{} as both parent and child",
            parent_number
        ));
    }

    let (_, err) = fetch_database_id(repo, parent_number);
    if let Some(e) = err {
        return Err(format!(
            "Failed to resolve parent #{}: {}",
            parent_number, e
        ));
    }

    let (child_id, err) = fetch_database_id(repo, child_number);
    if let Some(e) = err {
        return Err(format!("Failed to resolve child #{}: {}", child_number, e));
    }
    let child_id = child_id.unwrap();

    let api_path = format!("repos/{}/issues/{}/sub_issues", repo, parent_number);
    let sub_issue_field = format!("sub_issue_id={}", child_id);

    run_gh_cmd(&[
        "gh",
        "api",
        &api_path,
        "--method",
        "POST",
        "-F",
        &sub_issue_field,
    ])?;

    Ok((parent_number, child_number))
}

pub fn run_impl_main(args: &Args) -> (serde_json::Value, i32) {
    match create_sub_issue(&args.repo, args.parent_number, args.child_number) {
        Ok((parent, child)) => (json!({"status": "ok", "parent": parent, "child": child}), 0),
        Err(e) => (json!({"status": "error", "message": e}), 1),
    }
}
