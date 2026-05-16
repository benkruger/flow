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
/// Returns `owner/repo` string or None if detection fails. Optional
/// cwd parameter for running git in a specific directory.
///
/// Resolution order:
/// 1. `git remote get-url origin` + `parse_github_url` regex — fast
///    path for standard `github.com` URLs (HTTPS or SSH).
/// 2. `gh repo view --json nameWithOwner -q .nameWithOwner` fallback
///    — invoked when the regex returns None. Uses the `gh` CLI's
///    authenticated session, so SSH host aliases (e.g.
///    `git@github-pt:owner/repo.git`) resolve correctly via the user's
///    GitHub auth rather than via the remote URL's literal text.
///    Returns None if `gh` is missing, exits non-zero, or prints a
///    string without `/`.
pub fn detect_repo(cwd: Option<&Path>) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.args(["remote", "get-url", "origin"]);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    if let Ok(output) = cmd.output() {
        if output.status.success() {
            let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if let Some(owner_repo) = parse_github_url(&url) {
                return Some(owner_repo);
            }
        }
    }

    let mut gh = Command::new("gh");
    gh.args([
        "repo",
        "view",
        "--json",
        "nameWithOwner",
        "-q",
        ".nameWithOwner",
    ]);
    if let Some(dir) = cwd {
        gh.current_dir(dir);
    }
    let output = gh.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.contains('/') {
        Some(s)
    } else {
        None
    }
}
