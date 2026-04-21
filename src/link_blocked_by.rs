//! Create a GitHub blocked-by dependency.
//!
//! Usage:
//!   bin/flow link-blocked-by --repo <owner/repo> --blocked-number N --blocking-number N
//!
//! Resolves both issue numbers to database IDs (required by the REST API),
//! then creates the blocked-by dependency relationship.
//!
//! Output (JSON to stdout):
//!   Success: {"status": "ok", "blocked": N, "blocking": N}
//!   Error:   {"status": "error", "message": "..."}
//!
//! Tests live at tests/link_blocked_by.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use std::time::Duration;

use clap::Parser;
use serde_json::json;

use crate::complete_preflight::LOCAL_TIMEOUT;
use crate::issue::{fetch_database_id_with_runner, run_gh_cmd};

#[derive(Parser, Debug)]
#[command(
    name = "link-blocked-by",
    about = "Create a GitHub blocked-by dependency"
)]
pub struct Args {
    /// Repository (owner/name)
    #[arg(long)]
    pub repo: String,

    /// Issue that is blocked
    #[arg(long = "blocked-number")]
    pub blocked_number: i64,

    /// Issue that blocks
    #[arg(long = "blocking-number")]
    pub blocking_number: i64,
}

/// Create a blocked-by dependency between two issues.
///
/// Returns Ok((blocked, blocking)) on success or Err(message) on failure.
pub fn link_blocked_by(
    repo: &str,
    blocked_number: i64,
    blocking_number: i64,
) -> Result<(i64, i64), String> {
    if blocked_number == blocking_number {
        return Err(format!(
            "Cannot create self-reference: issue #{} as both blocked and blocking",
            blocked_number
        ));
    }

    // Resolve blocked issue to verify it exists (API URL uses blocked_number, not the DB ID)
    let (_, err) = fetch_database_id_with_runner(repo, blocked_number, &run_gh_cmd);
    if let Some(e) = err {
        return Err(format!(
            "Failed to resolve blocked #{}: {}",
            blocked_number, e
        ));
    }

    let (blocking_id, err) = fetch_database_id_with_runner(repo, blocking_number, &run_gh_cmd);
    if let Some(e) = err {
        return Err(format!(
            "Failed to resolve blocking #{}: {}",
            blocking_number, e
        ));
    }
    let blocking_id = blocking_id.unwrap();

    let api_path = format!(
        "repos/{}/issues/{}/dependencies/blocked_by",
        repo, blocked_number
    );
    let issue_id_field = format!("issue_id={}", blocking_id);
    let timeout = Duration::from_secs(LOCAL_TIMEOUT);

    run_gh_cmd(
        &[
            "gh",
            "api",
            &api_path,
            "--method",
            "POST",
            "-F",
            &issue_id_field,
        ],
        Some(timeout),
    )?;

    Ok((blocked_number, blocking_number))
}

pub fn run_impl_main(args: &Args) -> (serde_json::Value, i32) {
    match link_blocked_by(&args.repo, args.blocked_number, args.blocking_number) {
        Ok((blocked, blocking)) => (
            json!({"status": "ok", "blocked": blocked, "blocking": blocking}),
            0,
        ),
        Err(e) => (json!({"status": "error", "message": e}), 1),
    }
}
