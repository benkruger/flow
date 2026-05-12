//! Close a single GitHub issue via gh CLI.
//!
//! Usage:
//!   bin/flow close-issue --number <N> [--repo <repo>] [--comment <text>]
//!
//! Output (JSON to stdout):
//!   Success: {"status": "ok"}
//!   Error:   {"status": "error", "message": "..."}
//!
//! Tests live at tests/close_issue.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use std::process::Command;

use clap::Parser;
use serde_json::{json, Value};

#[derive(Parser, Debug)]
#[command(name = "close-issue", about = "Close a GitHub issue")]
pub struct Args {
    /// Repository (owner/name)
    #[arg(long)]
    pub repo: Option<String>,

    /// Issue number
    #[arg(long)]
    pub number: i64,

    /// Optional closing comment forwarded to `gh issue close --comment`.
    /// `allow_hyphen_values = true` lets the value begin with `-`/`--`
    /// (Markdown horizontal rules, dash-prefixed prose) so callers can
    /// pass arbitrary comment text without clap rejecting it as an
    /// unrecognized flag.
    #[arg(long, allow_hyphen_values = true)]
    pub comment: Option<String>,
}

/// Close a GitHub issue and return error message or None on success.
/// When `comment` is `Some(text)`, `--comment <text>` is appended to
/// the gh invocation so the closure carries an explanatory remark.
/// gh has its own network timeout; no hand-rolled loop needed per
/// .claude/rules/testability-means-simplicity.md.
fn close_issue_by_number(repo: &str, number: i64, comment: Option<&str>) -> Option<String> {
    let number_s = number.to_string();
    let mut gh_args: Vec<&str> = vec!["issue", "close", "--repo", repo, &number_s];
    if let Some(c) = comment {
        gh_args.push("--comment");
        gh_args.push(c);
    }
    let output = match Command::new("gh").args(&gh_args).output() {
        Ok(o) => o,
        Err(e) => return Some(format!("Failed to spawn: {}", e)),
    };
    if output.status.success() {
        return None;
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stderr.is_empty() {
        return Some(stderr);
    }
    if !stdout.is_empty() {
        return Some(stdout);
    }
    Some("Unknown error".to_string())
}

/// Main-arm dispatcher with injected repo_resolver. Returns
/// `(value, exit_code)`. The repo_resolver closure returns the detected
/// repo (or None when `git remote` has no origin); production binds it
/// to `detect_repo(None)`. Tests pass closures returning Some/None.
pub fn run_impl_main(args: Args, repo_resolver: &dyn Fn() -> Option<String>) -> (Value, i32) {
    let repo = match args.repo {
        Some(r) => r,
        None => match repo_resolver() {
            Some(r) => r,
            None => {
                return (
                    json!({"status": "error", "message": "Could not detect repo from git remote. Use --repo owner/name."}),
                    1,
                );
            }
        },
    };

    if let Some(e) = close_issue_by_number(&repo, args.number, args.comment.as_deref()) {
        return (json!({"status": "error", "message": e}), 1);
    }

    (json!({"status": "ok"}), 0)
}
