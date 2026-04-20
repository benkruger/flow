//! Create a GitHub milestone via gh API.
//!
//! Usage:
//!   bin/flow create-milestone --repo <owner/repo> --title <title> --due-date <YYYY-MM-DD>
//!
//! Output (JSON to stdout):
//!   Success: {"status": "ok", "number": N, "url": "..."}
//!   Error:   {"status": "error", "message": "..."}
//!
//! Tests live at tests/create_milestone.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use std::time::Duration;

use clap::Parser;
use serde_json::json;

use crate::complete_preflight::LOCAL_TIMEOUT;
use crate::issue::run_gh_cmd;

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
        &[
            "gh",
            "api",
            &api_path,
            "--method",
            "POST",
            "-f",
            &title_field,
            "-f",
            &due_on_field,
        ],
        Some(timeout),
    )?;

    let data: serde_json::Value =
        serde_json::from_str(&stdout).map_err(|_| format!("Invalid JSON response: {}", stdout))?;

    let number = data
        .get("number")
        .and_then(|v| v.as_i64())
        .ok_or("API response missing 'number' field")?;

    let url = data
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok((number, url))
}

pub fn run_impl_main(args: &Args) -> (serde_json::Value, i32) {
    match create_milestone(&args.repo, &args.title, &args.due_date) {
        Ok((number, url)) => (json!({"status": "ok", "number": number, "url": url}), 0),
        Err(e) => (json!({"status": "error", "message": e}), 1),
    }
}
