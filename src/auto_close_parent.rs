//! Port of lib/auto-close-parent.py — auto-close parent issue and milestone.
//!
//! Usage:
//!   bin/flow auto-close-parent --repo <owner/repo> --issue-number N
//!
//! Checks if the issue has a parent (sub-issue relationship). If so, checks
//! whether all sibling sub-issues are closed. If all closed, closes the parent.
//! Also checks the issue's milestone — if all milestone issues are closed,
//! closes the milestone.
//!
//! Best-effort throughout — any failure continues silently.
//!
//! Output (JSON to stdout):
//!   {"status": "ok", "parent_closed": bool, "milestone_closed": bool}

use std::path::Path;
use std::time::Duration;

use clap::Parser;
use serde_json::json;

use crate::output::json_ok;
use crate::start_setup::run_cmd;

/// Timeout for local subprocess calls (matches Python LOCAL_TIMEOUT = 30).
const LOCAL_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Parser, Debug)]
#[command(name = "auto-close-parent", about = "Auto-close parent issue and milestone")]
pub struct Args {
    /// Repository (owner/name)
    #[arg(long)]
    pub repo: String,

    /// Issue number to check
    #[arg(long = "issue-number")]
    pub issue_number: i64,
}

/// Run a gh command, returning stdout on success or an error string on failure.
fn run_api(args: &[&str], cwd: &Path) -> Result<String, String> {
    match run_cmd(args, cwd, "api", Some(LOCAL_TIMEOUT)) {
        Ok((stdout, _stderr)) => Ok(stdout),
        Err(e) => Err(e.message),
    }
}

/// Parse parent_issue.number and milestone.number from a JSON issue response.
///
/// Returns (parent_number_or_None, milestone_number_or_None).
pub fn parse_issue_fields(json_str: &str) -> (Option<i64>, Option<i64>) {
    let data: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return (None, None),
    };

    let parent_number = data
        .get("parent_issue")
        .and_then(|p| p.as_object())
        .and_then(|obj| obj.get("number"))
        .and_then(|n| n.as_i64());

    let milestone_number = data
        .get("milestone")
        .and_then(|m| m.as_object())
        .and_then(|obj| obj.get("number"))
        .and_then(|n| n.as_i64());

    (parent_number, milestone_number)
}

/// Fetch parent_issue.number and milestone.number in one API call.
///
/// Returns (parent_number_or_None, milestone_number_or_None).
/// Best-effort: returns (None, None) on any failure.
pub fn fetch_issue_fields(repo: &str, issue_number: i64, cwd: &Path) -> (Option<i64>, Option<i64>) {
    let url = format!("repos/{}/issues/{}", repo, issue_number);
    let stdout = match run_api(&["gh", "api", &url], cwd) {
        Ok(s) => s,
        Err(_) => return (None, None),
    };
    parse_issue_fields(&stdout)
}

/// Check if all sub-issues are closed from a JSON array response.
///
/// Returns true if the list is non-empty and every item has state "closed".
pub fn all_sub_issues_closed(json_str: &str) -> bool {
    let sub_issues: Vec<serde_json::Value> = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return false,
    };

    if sub_issues.is_empty() {
        return false;
    }

    sub_issues
        .iter()
        .all(|si| si.get("state").and_then(|s| s.as_str()) == Some("closed"))
}

/// Check if a milestone should be closed based on its JSON response.
///
/// Returns true if open_issues is 0.
pub fn should_close_milestone(json_str: &str) -> bool {
    let data: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return false,
    };

    // Default to 1 so a missing field is treated as open, never accidentally closing
    let open_issues = data
        .get("open_issues")
        .and_then(|v| v.as_i64())
        .unwrap_or(1);

    open_issues == 0
}

/// Check if all sub-issues of the parent are closed; close parent if so.
///
/// If parent_number is provided, uses it directly (skips the lookup).
/// Returns true if the parent was closed, false otherwise.
/// Best-effort: any failure returns false.
pub fn check_parent_closed(
    repo: &str,
    issue_number: i64,
    parent_number: Option<i64>,
    cwd: &Path,
) -> bool {
    let parent = match parent_number {
        Some(n) => n,
        None => {
            // Standalone call — fetch the parent number
            let url = format!("repos/{}/issues/{}", repo, issue_number);
            let stdout = match run_api(
                &["gh", "api", &url, "--jq", ".parent_issue.number"],
                cwd,
            ) {
                Ok(s) => s,
                Err(_) => return false,
            };
            let trimmed = stdout.trim();
            if trimmed.is_empty() || trimmed == "null" {
                return false;
            }
            match trimmed.parse::<i64>() {
                Ok(n) => n,
                Err(_) => return false,
            }
        }
    };

    // Get all sub-issues of the parent
    let url = format!("repos/{}/issues/{}/sub_issues", repo, parent);
    let stdout = match run_api(&["gh", "api", &url], cwd) {
        Ok(s) => s,
        Err(_) => return false,
    };

    if !all_sub_issues_closed(&stdout) {
        return false;
    }

    // All closed — close the parent
    run_api(
        &[
            "gh",
            "issue",
            "close",
            &parent.to_string(),
            "--repo",
            repo,
        ],
        cwd,
    )
    .is_ok()
}

/// Check if all milestone issues are closed; close milestone if so.
///
/// If milestone_number is provided, uses it directly (skips the lookup).
/// Returns true if the milestone was closed, false otherwise.
/// Best-effort: any failure returns false.
pub fn check_milestone_closed(
    repo: &str,
    issue_number: i64,
    milestone_number: Option<i64>,
    cwd: &Path,
) -> bool {
    let milestone = match milestone_number {
        Some(n) => n,
        None => {
            // Standalone call — fetch the milestone number
            let url = format!("repos/{}/issues/{}", repo, issue_number);
            let stdout = match run_api(
                &["gh", "api", &url, "--jq", ".milestone.number"],
                cwd,
            ) {
                Ok(s) => s,
                Err(_) => return false,
            };
            let trimmed = stdout.trim();
            if trimmed.is_empty() || trimmed == "null" {
                return false;
            }
            match trimmed.parse::<i64>() {
                Ok(n) => n,
                Err(_) => return false,
            }
        }
    };

    // Check milestone open_issues count
    let url = format!("repos/{}/milestones/{}", repo, milestone);
    let stdout = match run_api(&["gh", "api", &url], cwd) {
        Ok(s) => s,
        Err(_) => return false,
    };

    if !should_close_milestone(&stdout) {
        return false;
    }

    // All closed — close the milestone
    run_api(
        &[
            "gh",
            "api",
            &format!("repos/{}/milestones/{}", repo, milestone),
            "--method",
            "PATCH",
            "-f",
            "state=closed",
        ],
        cwd,
    )
    .is_ok()
}

/// CLI entry point for auto-close-parent.
pub fn run(args: Args) {
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(_) => {
            println!(
                "{}",
                json!({"status": "ok", "parent_closed": false, "milestone_closed": false})
            );
            return;
        }
    };

    // Fetch both fields in one API call to avoid redundant requests
    let (parent_number, milestone_number) = fetch_issue_fields(&args.repo, args.issue_number, &cwd);

    let parent_closed = check_parent_closed(&args.repo, args.issue_number, parent_number, &cwd);
    let milestone_closed =
        check_milestone_closed(&args.repo, args.issue_number, milestone_number, &cwd);

    json_ok(&[
        ("parent_closed", json!(parent_closed)),
        ("milestone_closed", json!(milestone_closed)),
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_issue_fields() ---

    #[test]
    fn parse_issue_fields_both_present() {
        let json = r#"{"parent_issue": {"number": 10}, "milestone": {"number": 3}}"#;
        let (parent, milestone) = parse_issue_fields(json);
        assert_eq!(parent, Some(10));
        assert_eq!(milestone, Some(3));
    }

    #[test]
    fn parse_issue_fields_absent() {
        let json = "{}";
        let (parent, milestone) = parse_issue_fields(json);
        assert_eq!(parent, None);
        assert_eq!(milestone, None);
    }

    #[test]
    fn parse_issue_fields_invalid_json() {
        let (parent, milestone) = parse_issue_fields("not json");
        assert_eq!(parent, None);
        assert_eq!(milestone, None);
    }

    #[test]
    fn parse_issue_fields_parent_not_dict() {
        let json = r#"{"parent_issue": "not_a_dict", "milestone": {"number": 3}}"#;
        let (parent, milestone) = parse_issue_fields(json);
        assert_eq!(parent, None);
        assert_eq!(milestone, Some(3));
    }

    #[test]
    fn parse_issue_fields_milestone_number_not_int() {
        let json = r#"{"parent_issue": {"number": 10}, "milestone": {"number": "not_int"}}"#;
        let (parent, milestone) = parse_issue_fields(json);
        assert_eq!(parent, Some(10));
        assert_eq!(milestone, None);
    }

    #[test]
    fn parse_issue_fields_null_values() {
        let json = r#"{"parent_issue": null, "milestone": null}"#;
        let (parent, milestone) = parse_issue_fields(json);
        assert_eq!(parent, None);
        assert_eq!(milestone, None);
    }

    // --- all_sub_issues_closed() ---

    #[test]
    fn all_sub_issues_closed_all_closed() {
        let json = r#"[{"number": 5, "state": "closed"}, {"number": 6, "state": "closed"}]"#;
        assert!(all_sub_issues_closed(json));
    }

    #[test]
    fn all_sub_issues_closed_some_open() {
        let json = r#"[{"number": 5, "state": "closed"}, {"number": 6, "state": "open"}]"#;
        assert!(!all_sub_issues_closed(json));
    }

    #[test]
    fn all_sub_issues_closed_empty() {
        assert!(!all_sub_issues_closed("[]"));
    }

    #[test]
    fn all_sub_issues_closed_invalid_json() {
        assert!(!all_sub_issues_closed("not json"));
    }

    #[test]
    fn all_sub_issues_closed_missing_state_field() {
        let json = r#"[{"number": 5}]"#;
        assert!(!all_sub_issues_closed(json));
    }

    // --- should_close_milestone() ---

    #[test]
    fn should_close_milestone_zero_open() {
        let json = r#"{"open_issues": 0, "closed_issues": 5}"#;
        assert!(should_close_milestone(json));
    }

    #[test]
    fn should_close_milestone_has_open() {
        let json = r#"{"open_issues": 2, "closed_issues": 3}"#;
        assert!(!should_close_milestone(json));
    }

    #[test]
    fn should_close_milestone_missing_field() {
        // Missing open_issues defaults to 1 (not closing)
        let json = r#"{"closed_issues": 5}"#;
        assert!(!should_close_milestone(json));
    }

    #[test]
    fn should_close_milestone_invalid_json() {
        assert!(!should_close_milestone("not json"));
    }

    #[test]
    fn should_close_milestone_null_open_issues() {
        // null defaults to 1 via unwrap_or
        let json = r#"{"open_issues": null}"#;
        assert!(!should_close_milestone(json));
    }
}
