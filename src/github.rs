//! GitHub remote URL helpers.
//!
//! Tests live at `tests/github.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` in
//! this file.

use std::path::Path;
use std::process::Command;

use regex::Regex;

/// Extract `owner/repo` from a GitHub remote URL (SSH or HTTPS).
///
/// Returns `None` for non-GitHub URLs or unparseable input.
/// Exposed as a pure function so both production (`detect_repo`)
/// and tests share one parser — no regex duplication.
pub fn parse_github_url(url: &str) -> Option<String> {
    let re = Regex::new(r"github\.com[:/]([^/]+/[^/]+?)(?:\.git)?$").unwrap();
    re.captures(url).map(|cap| cap[1].to_string())
}

/// Auto-detect GitHub repo from git remote origin URL.
///
/// Returns `owner/repo` string or None if detection fails.
/// Optional cwd parameter for running git in a specific directory.
pub fn detect_repo(cwd: Option<&Path>) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.args(["remote", "get-url", "origin"]);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if url.is_empty() {
        return None;
    }

    parse_github_url(&url)
}
