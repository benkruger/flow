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

/// Validate that `gh repo view` stdout is a well-formed `owner/repo`
/// slug before returning it from `detect_repo`'s gh fallback.
///
/// Accepts non-empty `<owner>/<repo>` strings where each segment
/// matches `[A-Za-z0-9._-]+`. Rejects multi-line strings (newline
/// injection), bare `/`, three-or-more-segment paths (`owner/repo/extra`),
/// empty input, and any string containing whitespace or characters
/// outside the allowed set. Run on the trimmed stdout of
/// `gh repo view --json nameWithOwner -q .nameWithOwner` to ensure
/// the returned identity is safe to interpolate into downstream
/// `gh` args, state-file writes, and Markdown URLs.
pub fn validate_gh_repo_output(s: &str) -> Option<String> {
    let re = Regex::new(r"^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$").unwrap();
    if re.is_match(s) {
        Some(s.to_string())
    } else {
        None
    }
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
///    GitHub auth rather than via the remote URL's literal text. The
///    trimmed stdout is run through `validate_gh_repo_output` so only
///    well-formed `owner/repo` slugs propagate to callers — multi-line
///    output, bare `/`, three-component paths, and strings with
///    whitespace or unsafe characters return None.
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
    validate_gh_repo_output(&s)
}
