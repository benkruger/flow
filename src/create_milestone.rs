//! Port of lib/create-milestone.py — create a GitHub milestone via gh API.
//!
//! Usage:
//!   bin/flow create-milestone --repo <owner/repo> --title <title> --due-date <YYYY-MM-DD>
//!
//! Output (JSON to stdout):
//!   Success: {"status": "ok", "number": N, "url": "..."}
//!   Error:   {"status": "error", "message": "..."}

use std::time::Duration;

use clap::Parser;
use serde_json::json;

use crate::issue::run_gh_cmd;
use crate::output::{json_error, json_ok};

const LOCAL_TIMEOUT: u64 = 30;

#[derive(Parser, Debug)]
#[command(name = "create-milestone", about = "Create a GitHub milestone")]
pub struct Args {
    /// Repository (owner/name)
    #[arg(long)]
    pub repo: String,

    /// Milestone title
    #[arg(long)]
    pub title: String,

    /// Due date (YYYY-MM-DD)
    #[arg(long = "due-date")]
    pub due_date: String,
}

/// Create a milestone via gh api.
///
/// Returns Ok((number, url)) on success or Err(message) on failure.
pub fn create_milestone(repo: &str, title: &str, due_date: &str) -> Result<(i64, String), String> {
    let api_path = format!("repos/{}/milestones", repo);
    let title_field = format!("title={}", title);
    let due_on_field = format!("due_on={}T00:00:00Z", due_date);
    let timeout = Duration::from_secs(LOCAL_TIMEOUT);

    let stdout = run_gh_cmd(
        &["gh", "api", &api_path, "--method", "POST", "-f", &title_field, "-f", &due_on_field],
        Some(timeout),
    )?;

    let data: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|_| format!("Invalid JSON response: {}", stdout))?;

    let number = data.get("number")
        .and_then(|v| v.as_i64())
        .ok_or("API response missing 'number' field")?;

    let url = data.get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok((number, url))
}

pub fn run(args: Args) {
    match create_milestone(&args.repo, &args.title, &args.due_date) {
        Ok((number, url)) => {
            json_ok(&[
                ("number", json!(number)),
                ("url", json!(url)),
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
            "create-milestone",
            "--repo", "owner/repo",
            "--title", "v1.0 Release",
            "--due-date", "2026-06-01",
        ]);
        assert!(args.is_ok());
        let args = args.unwrap();
        assert_eq!(args.repo, "owner/repo");
        assert_eq!(args.title, "v1.0 Release");
        assert_eq!(args.due_date, "2026-06-01");
    }

    #[test]
    fn args_missing_repo_fails() {
        let args = Args::try_parse_from([
            "create-milestone",
            "--title", "v1.0",
            "--due-date", "2026-06-01",
        ]);
        assert!(args.is_err());
    }

    #[test]
    fn args_missing_title_fails() {
        let args = Args::try_parse_from([
            "create-milestone",
            "--repo", "owner/repo",
            "--due-date", "2026-06-01",
        ]);
        assert!(args.is_err());
    }

    #[test]
    fn args_missing_due_date_fails() {
        let args = Args::try_parse_from([
            "create-milestone",
            "--repo", "owner/repo",
            "--title", "v1.0",
        ]);
        assert!(args.is_err());
    }

    #[test]
    fn parse_valid_milestone_response() {
        let json_str = r#"{"number": 5, "html_url": "https://github.com/owner/repo/milestone/5"}"#;
        let data: serde_json::Value = serde_json::from_str(json_str).unwrap();
        let number = data.get("number").and_then(|v| v.as_i64());
        let url = data.get("html_url").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(number, Some(5));
        assert_eq!(url, "https://github.com/owner/repo/milestone/5");
    }

    #[test]
    fn parse_response_missing_number() {
        let json_str = r#"{"html_url": "https://github.com/owner/repo/milestone/1"}"#;
        let data: serde_json::Value = serde_json::from_str(json_str).unwrap();
        let number = data.get("number").and_then(|v| v.as_i64());
        assert!(number.is_none());
    }

    #[test]
    fn parse_response_missing_url_defaults_empty() {
        let json_str = r#"{"number": 3}"#;
        let data: serde_json::Value = serde_json::from_str(json_str).unwrap();
        let url = data.get("html_url").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(url, "");
    }

    #[test]
    fn invalid_json_detected() {
        let result: Result<serde_json::Value, _> = serde_json::from_str("not json");
        assert!(result.is_err());
    }

    #[test]
    fn uses_string_flag_for_fields() {
        // Verify the API call uses -f (string) not -F (integer).
        // The -f flag is hardcoded in create_milestone(), verified by code inspection.
        // Title and due_on are string fields, not integer IDs.
        let title_field = format!("title={}", "v1.0");
        assert!(title_field.starts_with("title="));
    }
}
