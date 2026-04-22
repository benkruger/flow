//! Auto-close parent issue and milestone.
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
//!
//! Tests live at `tests/auto_close_parent.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` in
//! this file.

use std::path::Path;
use std::time::Duration;

use clap::Parser;
use serde_json::{json, Value};

use crate::complete_preflight::LOCAL_TIMEOUT;
use crate::utils::run_cmd;

#[derive(Parser, Debug)]
#[command(
    name = "auto-close-parent",
    about = "Auto-close parent issue and milestone"
)]
pub struct Args {
    /// Repository (owner/name)
    #[arg(long)]
    pub repo: String,

    /// Issue number to check
    #[arg(long = "issue-number")]
    pub issue_number: i64,
}

/// Type alias for the gh-api runner closure used by `_with_runner`
/// seams. Production binds to a closure wrapping `run_cmd`. Tests
/// inject mock closures returning queued or fixed
/// `Result<String, String>` responses per call so the test never
/// spawns a real `gh` subprocess.
pub type GhApiRunner = dyn Fn(&[&str], &Path) -> Result<String, String>;

/// Run a gh command, returning stdout on success or an error string on failure.
pub fn run_api(args: &[&str], cwd: &Path) -> Result<String, String> {
    match run_cmd(args, cwd, "api", Some(Duration::from_secs(LOCAL_TIMEOUT))) {
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
/// Tests pass a mock `runner` so they never spawn `gh`; production
/// callers pass `&run_api`. Per `.claude/rules/testability-means-simplicity.md`
/// the runner is the only seam — no separate thin wrapper that binds
/// `&run_api` exists, because it added an unused-in-tests monomorphization
/// with no behavior of its own.
pub fn fetch_issue_fields(
    repo: &str,
    issue_number: i64,
    cwd: &Path,
    runner: &GhApiRunner,
) -> (Option<i64>, Option<i64>) {
    let url = format!("repos/{}/issues/{}", repo, issue_number);
    let stdout = match runner(&["gh", "api", &url], cwd) {
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
/// Best-effort: any failure returns false. Tests pass a mock runner;
/// production passes `&run_api`.
pub fn check_parent_closed(
    repo: &str,
    issue_number: i64,
    parent_number: Option<i64>,
    cwd: &Path,
    runner: &GhApiRunner,
) -> bool {
    let parent = match parent_number {
        Some(n) => n,
        None => {
            // Standalone call — fetch the parent number
            let url = format!("repos/{}/issues/{}", repo, issue_number);
            let stdout = match runner(&["gh", "api", &url, "--jq", ".parent_issue.number"], cwd) {
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
    let stdout = match runner(&["gh", "api", &url], cwd) {
        Ok(s) => s,
        Err(_) => return false,
    };

    if !all_sub_issues_closed(&stdout) {
        return false;
    }

    // All closed — close the parent
    runner(
        &["gh", "issue", "close", &parent.to_string(), "--repo", repo],
        cwd,
    )
    .is_ok()
}

/// Check if all milestone issues are closed; close milestone if so.
///
/// If milestone_number is provided, uses it directly (skips the lookup).
/// Returns true if the milestone was closed, false otherwise.
/// Best-effort: any failure returns false. Tests pass a mock runner;
/// production passes `&run_api`.
pub fn check_milestone_closed(
    repo: &str,
    issue_number: i64,
    milestone_number: Option<i64>,
    cwd: &Path,
    runner: &GhApiRunner,
) -> bool {
    let milestone = match milestone_number {
        Some(n) => n,
        None => {
            // Standalone call — fetch the milestone number
            let url = format!("repos/{}/issues/{}", repo, issue_number);
            let stdout = match runner(&["gh", "api", &url, "--jq", ".milestone.number"], cwd) {
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
    let stdout = match runner(&["gh", "api", &url], cwd) {
        Ok(s) => s,
        Err(_) => return false,
    };

    if !should_close_milestone(&stdout) {
        return false;
    }

    // All closed — close the milestone
    runner(
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

/// Main-arm dispatcher with injected cwd and runner. Always returns
/// `(Value, 0)` — auto-close is best-effort by design and the parent /
/// milestone close decisions surface as boolean fields in the success
/// payload, never as an error exit. Tests pass a mock runner;
/// production passes `&run_api`.
pub fn run_impl_main(args: Args, cwd: &Path, runner: &GhApiRunner) -> (Value, i32) {
    // Fetch both fields in one API call to avoid redundant requests
    let (parent_number, milestone_number) =
        fetch_issue_fields(&args.repo, args.issue_number, cwd, runner);

    let parent_closed =
        check_parent_closed(&args.repo, args.issue_number, parent_number, cwd, runner);
    let milestone_closed =
        check_milestone_closed(&args.repo, args.issue_number, milestone_number, cwd, runner);

    (
        json!({
            "status": "ok",
            "parent_closed": parent_closed,
            "milestone_closed": milestone_closed,
        }),
        0,
    )
}

/// Best-effort safe-default payload when we can't determine cwd —
/// auto-close-parent never fails the caller, so we return ok with
/// both close flags false.
pub fn safe_default_ok() -> (Value, i32) {
    (
        json!({"status": "ok", "parent_closed": false, "milestone_closed": false}),
        0,
    )
}

/// Seam-injected wrapper that dispatches between `run_impl_main` and
/// `safe_default_ok` based on a caller-supplied cwd provider.
/// Production binds `cwd_fn = std::env::current_dir`; tests pass a
/// closure returning `Err` to exercise the safe-default branch
/// without needing to unlink the subprocess cwd via `pre_exec`.
pub fn run_with_current_dir_from<F>(args: Args, cwd_fn: F, runner: &GhApiRunner) -> (Value, i32)
where
    F: FnOnce() -> std::io::Result<std::path::PathBuf>,
{
    match cwd_fn() {
        Ok(cwd) => run_impl_main(args, &cwd, runner),
        Err(_) => safe_default_ok(),
    }
}
