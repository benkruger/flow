//! GitHub issue creation wrapper.
//!
//! Usage:
//!   bin/flow issue --title <title> [--repo <repo>] [--label <label>] [--milestone <title>] [--body-file <path>]
//!
//! Body text is always passed via a file to avoid shell escaping issues
//! with special characters (|, &&, ;) that trigger the Bash hook validator.
//! The file is read and deleted before the gh call.
//!
//! Output (JSON to stdout):
//!   Success: {"status": "ok", "url": "<issue_url>", "number": N, "id": N}
//!   Error:   {"status": "error", "message": "..."}

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use clap::Parser;
use regex::Regex;
use serde_json::json;

use crate::git::project_root;
use crate::github::detect_repo;
use crate::output::{json_error, json_ok};

const LOCAL_TIMEOUT: u64 = 30;

#[derive(Parser, Debug)]
#[command(name = "issue", about = "Create a GitHub issue")]
pub struct Args {
    /// Repository (owner/name)
    #[arg(long)]
    pub repo: Option<String>,

    /// Issue title
    #[arg(long)]
    pub title: String,

    /// Issue label
    #[arg(long)]
    pub label: Option<String>,

    /// Path to file containing issue body (file is deleted after reading)
    #[arg(long = "body-file")]
    pub body_file: Option<String>,

    /// Path to state file for repo lookup
    #[arg(long = "state-file")]
    pub state_file: Option<String>,

    /// Milestone title to assign the issue to
    #[arg(long)]
    pub milestone: Option<String>,
}

pub struct IssueResult {
    pub url: String,
    pub number: Option<i64>,
    pub id: Option<i64>,
}

/// Read body text from a file and delete the file.
///
/// Returns the body text or an error message.
/// Relative paths are resolved against `root`.
/// The file is always deleted after reading, even if empty.
pub fn read_body_file(path: &str, root: &Path) -> Result<String, String> {
    let resolved: PathBuf = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        root.join(path)
    };

    let body = fs::read_to_string(&resolved)
        .map_err(|e| format!("Could not read body file '{}': {}", resolved.display(), e))?;

    // Delete after reading — ignore errors (matches Python behavior)
    let _ = fs::remove_file(&resolved);

    Ok(body)
}

/// Extract issue number from a GitHub issue URL.
///
/// Returns the integer issue number, or None if the URL doesn't match.
pub fn parse_issue_number(url: &str) -> Option<i64> {
    let re = Regex::new(r"/issues/(\d+)").unwrap();
    re.captures(url).and_then(|cap| cap[1].parse().ok())
}

/// Fetch the REST API database ID for an issue.
///
/// The database ID is the integer ID used by REST API endpoints for
/// sub-issues and dependencies. This is NOT the GraphQL node_id.
///
/// Returns (id, error). id is Some(integer) or None.
pub fn fetch_database_id(repo: &str, number: i64) -> (Option<i64>, Option<String>) {
    let timeout = Duration::from_secs(LOCAL_TIMEOUT);
    let api_path = format!("repos/{}/issues/{}", repo, number);
    match run_gh_cmd(&["gh", "api", &api_path, "--jq", ".id"], Some(timeout)) {
        Ok(stdout) => match stdout.trim().parse::<i64>() {
            Ok(id) => (Some(id), None),
            Err(_) => (
                None,
                Some(format!("Invalid ID from API: {}", stdout.trim())),
            ),
        },
        Err(e) => (None, Some(e)),
    }
}

/// Run gh issue create and return issue details.
///
/// Includes label-not-found retry logic: if the label doesn't exist,
/// tries to create it, then retries. If label creation fails, retries
/// without the label.
pub fn create_issue(
    repo: &str,
    title: &str,
    label: Option<&str>,
    body: Option<&str>,
    milestone: Option<&str>,
) -> Result<IssueResult, String> {
    let timeout = Duration::from_secs(LOCAL_TIMEOUT);

    // Build initial command
    let title_owned = title.to_string();
    let mut cmd_args: Vec<String> = vec![
        "gh".into(),
        "issue".into(),
        "create".into(),
        "--repo".into(),
        repo.into(),
        "--title".into(),
        title_owned,
    ];
    if let Some(l) = label {
        cmd_args.push("--label".into());
        cmd_args.push(l.into());
    }
    if let Some(b) = body {
        cmd_args.push("--body".into());
        cmd_args.push(b.into());
    }
    if let Some(m) = milestone {
        cmd_args.push("--milestone".into());
        cmd_args.push(m.into());
    }

    let cmd_refs: Vec<&str> = cmd_args.iter().map(|s| s.as_str()).collect();
    match run_gh_cmd(&cmd_refs, Some(timeout)) {
        Ok(url) => Ok(build_issue_result(repo, url)),
        Err(error) => {
            // Label-not-found retry logic
            if let Some(l) = label {
                let err_lower = error.to_lowercase();
                if err_lower.contains("label") && err_lower.contains("not found") {
                    return retry_with_label(repo, title, l, body, milestone, timeout);
                }
            }
            Err(error)
        }
    }
}

fn retry_with_label(
    repo: &str,
    title: &str,
    label: &str,
    body: Option<&str>,
    milestone: Option<&str>,
    timeout: Duration,
) -> Result<IssueResult, String> {
    // Try creating the label
    let label_created = run_gh_cmd(
        &["gh", "label", "create", label, "--repo", repo],
        Some(timeout),
    )
    .is_ok();

    // Retry: with label if created, without if not
    let mut retry_args: Vec<String> = vec![
        "gh".into(),
        "issue".into(),
        "create".into(),
        "--repo".into(),
        repo.into(),
        "--title".into(),
        title.into(),
    ];
    if label_created {
        retry_args.push("--label".into());
        retry_args.push(label.into());
    }
    if let Some(b) = body {
        retry_args.push("--body".into());
        retry_args.push(b.into());
    }
    if let Some(m) = milestone {
        retry_args.push("--milestone".into());
        retry_args.push(m.into());
    }

    let retry_refs: Vec<&str> = retry_args.iter().map(|s| s.as_str()).collect();
    let url = run_gh_cmd(&retry_refs, Some(timeout))?;
    Ok(build_issue_result(repo, url))
}

fn build_issue_result(repo: &str, url: String) -> IssueResult {
    let number = parse_issue_number(&url);
    let db_id = number.and_then(|n| {
        let (id, _) = fetch_database_id(repo, n);
        id
    });
    IssueResult {
        url,
        number,
        id: db_id,
    }
}

/// Run a gh CLI command, returning stdout on success.
/// Returns Err with the error message on failure or timeout.
pub fn run_gh_cmd(args: &[&str], timeout: Option<Duration>) -> Result<String, String> {
    let mut child = Command::new(args[0])
        .args(&args[1..])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn: {}", e))?;

    if let Some(dur) = timeout {
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(50);
        loop {
            match child.try_wait() {
                Ok(Some(_status)) => {
                    let output = child.wait_with_output().map_err(|e| e.to_string())?;
                    if !output.status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        return Err(extract_error(&stderr, &stdout));
                    }
                    return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
                }
                Ok(None) => {
                    if start.elapsed() >= dur {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(format!("Command timed out after {}s", dur.as_secs()));
                    }
                    std::thread::sleep(poll_interval.min(dur - start.elapsed()));
                }
                Err(e) => return Err(e.to_string()),
            }
        }
    } else {
        let output = child.wait_with_output().map_err(|e| e.to_string())?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Err(extract_error(&stderr, &stdout));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

pub fn extract_error(stderr: &str, stdout: &str) -> String {
    if !stderr.is_empty() {
        stderr.to_string()
    } else if !stdout.is_empty() {
        stdout.to_string()
    } else {
        "Unknown error".to_string()
    }
}

fn detect_repo_or_fail(root: &Path) -> String {
    match detect_repo(Some(root)) {
        Some(r) => r,
        None => {
            json_error(
                "Could not detect repo from git remote. Use --repo owner/name.",
                &[],
            );
            std::process::exit(1);
        }
    }
}

pub fn run(args: Args) {
    let root = project_root();

    // Resolve repo: --repo > --state-file > detect_repo
    let repo = if let Some(r) = args.repo {
        r
    } else if let Some(ref sf) = args.state_file {
        resolve_repo_from_state(sf).unwrap_or_else(|| detect_repo_or_fail(&root))
    } else {
        detect_repo_or_fail(&root)
    };

    // Read body from file if provided
    let body = if let Some(ref bf) = args.body_file {
        match read_body_file(bf, &root) {
            Ok(b) => Some(b),
            Err(e) => {
                json_error(&e, &[]);
                std::process::exit(1);
            }
        }
    } else {
        None
    };

    match create_issue(
        &repo,
        &args.title,
        args.label.as_deref(),
        body.as_deref(),
        args.milestone.as_deref(),
    ) {
        Ok(result) => {
            json_ok(&[
                ("url", json!(result.url)),
                ("number", json!(result.number)),
                ("id", json!(result.id)),
            ]);
        }
        Err(e) => {
            json_error(&e, &[]);
            std::process::exit(1);
        }
    }
}

fn resolve_repo_from_state(state_file: &str) -> Option<String> {
    let content = fs::read_to_string(state_file).ok()?;
    let state: serde_json::Value = serde_json::from_str(&content).ok()?;
    state
        .get("repo")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // --- read_body_file ---

    #[test]
    fn read_body_file_reads_and_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".flow-issue-body");
        fs::write(&file, "Issue body with | pipes and && ampersands").unwrap();

        let result = read_body_file(file.to_str().unwrap(), dir.path());

        assert_eq!(result.unwrap(), "Issue body with | pipes and && ampersands");
        assert!(!file.exists());
    }

    #[test]
    fn read_body_file_missing_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("nonexistent.md");

        let result = read_body_file(file.to_str().unwrap(), dir.path());

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Could not read body file"));
    }

    #[test]
    fn read_body_file_empty_returns_empty_string() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".flow-issue-body");
        fs::write(&file, "").unwrap();

        let result = read_body_file(file.to_str().unwrap(), dir.path());

        assert_eq!(result.unwrap(), "");
        assert!(!file.exists());
    }

    #[test]
    fn read_body_file_rich_markdown_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".flow-issue-body");
        let content = "## Summary\n\n| Column | Value |\n|--------|-------|\n| A | B |\n";
        fs::write(&file, content).unwrap();

        let result = read_body_file(file.to_str().unwrap(), dir.path());

        assert_eq!(result.unwrap(), content);
    }

    #[test]
    fn read_body_file_relative_resolved_against_root() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path().join("project");
        fs::create_dir_all(&project_dir).unwrap();
        let file = project_dir.join(".flow-issue-body");
        fs::write(&file, "Resolved body").unwrap();

        let result = read_body_file(".flow-issue-body", &project_dir);

        assert_eq!(result.unwrap(), "Resolved body");
        assert!(!file.exists());
    }

    #[test]
    fn read_body_file_absolute_ignores_root() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".flow-issue-body");
        fs::write(&file, "Absolute body").unwrap();

        // Pass a different root — should use the absolute path as-is
        let other_root = dir.path().join("other");
        fs::create_dir_all(&other_root).unwrap();

        let result = read_body_file(file.to_str().unwrap(), &other_root);

        assert_eq!(result.unwrap(), "Absolute body");
    }

    #[test]
    fn read_body_file_relative_missing_returns_error() {
        let dir = tempfile::tempdir().unwrap();

        let result = read_body_file("nonexistent.md", dir.path());

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Could not read body file"));
    }

    // --- parse_issue_number ---

    #[test]
    fn parse_issue_number_standard_url() {
        assert_eq!(
            parse_issue_number("https://github.com/owner/repo/issues/42"),
            Some(42)
        );
    }

    #[test]
    fn parse_issue_number_large_number() {
        assert_eq!(
            parse_issue_number("https://github.com/owner/repo/issues/99999"),
            Some(99999)
        );
    }

    #[test]
    fn parse_issue_number_invalid_url() {
        assert_eq!(parse_issue_number("not a url"), None);
    }

    #[test]
    fn parse_issue_number_empty_string() {
        assert_eq!(parse_issue_number(""), None);
    }

    #[test]
    fn parse_issue_number_pull_request_url() {
        assert_eq!(
            parse_issue_number("https://github.com/owner/repo/pull/42"),
            None
        );
    }

    // --- extract_error ---

    #[test]
    fn extract_error_prefers_stderr() {
        assert_eq!(extract_error("stderr msg", "stdout msg"), "stderr msg");
    }

    #[test]
    fn extract_error_falls_back_to_stdout() {
        assert_eq!(extract_error("", "stdout msg"), "stdout msg");
    }

    #[test]
    fn extract_error_unknown_when_both_empty() {
        assert_eq!(extract_error("", ""), "Unknown error");
    }

    // --- resolve_repo_from_state ---

    #[test]
    fn resolve_repo_from_valid_state() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("state.json");
        fs::write(&file, r#"{"repo": "cached/repo", "branch": "test"}"#).unwrap();

        assert_eq!(
            resolve_repo_from_state(file.to_str().unwrap()),
            Some("cached/repo".to_string())
        );
    }

    #[test]
    fn resolve_repo_from_corrupt_state() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("bad.json");
        fs::write(&file, "{corrupt").unwrap();

        assert_eq!(resolve_repo_from_state(file.to_str().unwrap()), None);
    }

    #[test]
    fn resolve_repo_from_state_no_repo_key() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("state.json");
        fs::write(&file, r#"{"branch": "test"}"#).unwrap();

        assert_eq!(resolve_repo_from_state(file.to_str().unwrap()), None);
    }

    #[test]
    fn resolve_repo_from_missing_state() {
        assert_eq!(resolve_repo_from_state("/nonexistent/state.json"), None);
    }

    // --- Args milestone parsing ---

    #[test]
    fn args_parses_milestone() {
        let args = Args::try_parse_from(["issue", "--title", "Test issue", "--milestone", "v1.0"])
            .unwrap();
        assert_eq!(args.milestone.as_deref(), Some("v1.0"));
    }

    #[test]
    fn args_milestone_defaults_to_none() {
        let args = Args::try_parse_from(["issue", "--title", "Test issue"]).unwrap();
        assert!(args.milestone.is_none());
    }
}
