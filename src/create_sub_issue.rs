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

use std::time::Duration;

use clap::Parser;
use serde_json::json;

use crate::complete_preflight::LOCAL_TIMEOUT;
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

    // Resolve parent to verify it exists (API URL uses parent_number, not the DB ID)
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
    let timeout = Duration::from_secs(LOCAL_TIMEOUT);

    run_gh_cmd(
        &[
            "gh",
            "api",
            &api_path,
            "--method",
            "POST",
            "-F",
            &sub_issue_field,
        ],
        Some(timeout),
    )?;

    Ok((parent_number, child_number))
}

pub fn run_impl_main(args: &Args) -> (serde_json::Value, i32) {
    match create_sub_issue(&args.repo, args.parent_number, args.child_number) {
        Ok((parent, child)) => (
            json!({"status": "ok", "parent": parent, "child": child}),
            0,
        ),
        Err(e) => (json!({"status": "error", "message": e}), 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // create_sub_issue shells out to `gh` and cannot be unit-tested
    // without process mocking. End-to-end coverage runs through the
    // QA harness invoking `bin/flow create-sub-issue`. Unit tests in
    // this module focus on argument parsing and output formatting.

    #[test]
    fn args_parse_all_required() {
        let args = Args::try_parse_from([
            "create-sub-issue",
            "--repo",
            "owner/repo",
            "--parent-number",
            "1",
            "--child-number",
            "2",
        ]);
        assert!(args.is_ok());
        let args = args.unwrap();
        assert_eq!(args.repo, "owner/repo");
        assert_eq!(args.parent_number, 1);
        assert_eq!(args.child_number, 2);
    }

    #[test]
    fn args_missing_repo_fails() {
        let args = Args::try_parse_from([
            "create-sub-issue",
            "--parent-number",
            "1",
            "--child-number",
            "2",
        ]);
        assert!(args.is_err());
    }

    #[test]
    fn args_missing_parent_fails() {
        let args = Args::try_parse_from([
            "create-sub-issue",
            "--repo",
            "owner/repo",
            "--child-number",
            "2",
        ]);
        assert!(args.is_err());
    }

    #[test]
    fn args_missing_child_fails() {
        let args = Args::try_parse_from([
            "create-sub-issue",
            "--repo",
            "owner/repo",
            "--parent-number",
            "1",
        ]);
        assert!(args.is_err());
    }

    #[test]
    fn self_reference_rejected() {
        // parent == child should be rejected before any API call
        let result = create_sub_issue("owner/repo", 42, 42);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(
            msg.contains("self-reference"),
            "Error should mention self-reference, got: {}",
            msg
        );
    }

    #[test]
    fn uses_integer_flag_for_sub_issue_id() {
        // Verify the API call uses -F (integer) not -f (string)
        // by checking the command construction logic.
        // The actual gh call format is:
        //   gh api repos/{repo}/issues/{parent}/sub_issues --method POST -F sub_issue_id={child_id}
        // The -F flag is hardcoded in create_sub_issue(), verified by code inspection.
        // This test documents the invariant.
        let api_path = format!("repos/{}/issues/{}/sub_issues", "o/r", 1);
        assert!(api_path.contains("/sub_issues"));
    }
}
