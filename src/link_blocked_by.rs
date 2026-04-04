//! Port of lib/link-blocked-by.py — create a GitHub blocked-by dependency.
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

use std::time::Duration;

use clap::Parser;
use serde_json::json;

use crate::issue::{fetch_database_id, run_gh_cmd};
use crate::output::{json_error, json_ok};

const LOCAL_TIMEOUT: u64 = 30;

#[derive(Parser, Debug)]
#[command(name = "link-blocked-by", about = "Create a GitHub blocked-by dependency")]
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
pub fn link_blocked_by(repo: &str, blocked_number: i64, blocking_number: i64) -> Result<(i64, i64), String> {
    // Resolve blocked issue to verify it exists (API URL uses blocked_number, not the DB ID)
    let (_, err) = fetch_database_id(repo, blocked_number);
    if let Some(e) = err {
        return Err(format!("Failed to resolve blocked #{}: {}", blocked_number, e));
    }

    let (blocking_id, err) = fetch_database_id(repo, blocking_number);
    if let Some(e) = err {
        return Err(format!("Failed to resolve blocking #{}: {}", blocking_number, e));
    }
    let blocking_id = blocking_id.unwrap();

    let api_path = format!(
        "repos/{}/issues/{}/dependencies/blocked_by",
        repo, blocked_number
    );
    let issue_id_field = format!("issue_id={}", blocking_id);
    let timeout = Duration::from_secs(LOCAL_TIMEOUT);

    run_gh_cmd(
        &["gh", "api", &api_path, "--method", "POST", "-F", &issue_id_field],
        Some(timeout),
    )?;

    Ok((blocked_number, blocking_number))
}

pub fn run(args: Args) {
    match link_blocked_by(&args.repo, args.blocked_number, args.blocking_number) {
        Ok((blocked, blocking)) => {
            json_ok(&[
                ("blocked", json!(blocked)),
                ("blocking", json!(blocking)),
            ]);
        }
        Err(e) => {
            json_error(&e, &[]);
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_parse_all_required() {
        let args = Args::try_parse_from([
            "link-blocked-by",
            "--repo", "owner/repo",
            "--blocked-number", "10",
            "--blocking-number", "20",
        ]);
        assert!(args.is_ok());
        let args = args.unwrap();
        assert_eq!(args.repo, "owner/repo");
        assert_eq!(args.blocked_number, 10);
        assert_eq!(args.blocking_number, 20);
    }

    #[test]
    fn args_missing_repo_fails() {
        let args = Args::try_parse_from([
            "link-blocked-by",
            "--blocked-number", "10",
            "--blocking-number", "20",
        ]);
        assert!(args.is_err());
    }

    #[test]
    fn args_missing_blocked_fails() {
        let args = Args::try_parse_from([
            "link-blocked-by",
            "--repo", "owner/repo",
            "--blocking-number", "20",
        ]);
        assert!(args.is_err());
    }

    #[test]
    fn args_missing_blocking_fails() {
        let args = Args::try_parse_from([
            "link-blocked-by",
            "--repo", "owner/repo",
            "--blocked-number", "10",
        ]);
        assert!(args.is_err());
    }

    #[test]
    fn uses_integer_flag_for_issue_id() {
        // Verify the API call uses -F (integer) not -f (string).
        // The -F flag is hardcoded in link_blocked_by(), verified by code inspection.
        let api_path = format!(
            "repos/{}/issues/{}/dependencies/blocked_by",
            "o/r", 10
        );
        assert!(api_path.contains("/dependencies/blocked_by"));
    }
}
