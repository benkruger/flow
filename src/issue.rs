//! Port of lib/issue.py — GitHub issue creation wrapper.
//!
//! Usage:
//!   bin/flow issue --title <title> [--repo <repo>] [--label <label>] [--body-file <path>]
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
    match run_gh_cmd(
        &["gh", "api", &api_path, "--jq", ".id"],
        Some(timeout),
    ) {
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

    let cmd_refs: Vec<&str> = cmd_args.iter().map(|s| s.as_str()).collect();
    match run_gh_cmd(&cmd_refs, Some(timeout)) {
        Ok(url) => Ok(build_issue_result(repo, url)),
        Err(error) => {
            // Label-not-found retry logic
            if let Some(l) = label {
                let err_lower = error.to_lowercase();
                if err_lower.contains("label") && err_lower.contains("not found") {
                    return retry_with_label(repo, title, l, body, timeout);
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
    // args[0] is the program name (typically "gh"); args[1..] are its arguments.
    // Delegate to the thread-drain inner helper to prevent pipe buffer deadlock.
    match run_cmd_inner(args[0], &args[1..], timeout) {
        Ok((code, stdout_bytes, stderr_bytes)) => {
            if code != 0 {
                let stderr = String::from_utf8_lossy(&stderr_bytes).trim().to_string();
                let stdout = String::from_utf8_lossy(&stdout_bytes).trim().to_string();
                return Err(extract_error(&stderr, &stdout));
            }
            Ok(String::from_utf8_lossy(&stdout_bytes).trim().to_string())
        }
        Err(e) => Err(e),
    }
}

/// Run a subprocess with an optional timeout, returning (exit_code, stdout_bytes, stderr_bytes).
///
/// Drains stdout and stderr in spawned reader threads before the poll loop (or
/// before the blocking wait, in the no-timeout branch) to prevent pipe buffer
/// deadlock on outputs larger than ~64KB. Joins reader threads on every exit
/// path (success, timeout, try_wait error).
///
/// The `program` parameter is test-injectable — production passes "gh".
fn run_cmd_inner(
    program: &str,
    args: &[&str],
    timeout: Option<Duration>,
) -> Result<(i32, Vec<u8>, Vec<u8>), String> {
    let mut child = Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn: {}", e))?;

    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();
    let stdout_reader = std::thread::spawn(move || {
        let mut buf: Vec<u8> = Vec::new();
        if let Some(mut pipe) = stdout_handle {
            use std::io::Read;
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buf: Vec<u8> = Vec::new();
        if let Some(mut pipe) = stderr_handle {
            use std::io::Read;
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });

    let status = if let Some(dur) = timeout {
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(50);
        loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) => {
                    if start.elapsed() >= dur {
                        let _ = child.kill();
                        let _ = child.wait();
                        let _ = stdout_reader.join();
                        let _ = stderr_reader.join();
                        return Err(format!("Command timed out after {}s", dur.as_secs()));
                    }
                    std::thread::sleep(poll_interval.min(dur - start.elapsed()));
                }
                Err(e) => {
                    let _ = stdout_reader.join();
                    let _ = stderr_reader.join();
                    return Err(e.to_string());
                }
            }
        }
    } else {
        // No-timeout branch: block on wait(), then join the drain threads.
        // The drain threads prevent pipe-buffer deadlock while wait() blocks.
        match child.wait() {
            Ok(s) => s,
            Err(e) => {
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(e.to_string());
            }
        }
    };

    let stdout_bytes = stdout_reader.join().unwrap_or_default();
    let stderr_bytes = stderr_reader.join().unwrap_or_default();
    let code = status.code().unwrap_or(1);
    Ok((code, stdout_bytes, stderr_bytes))
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

    match create_issue(&repo, &args.title, args.label.as_deref(), body.as_deref()) {
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
        fs::write(
            &file,
            r#"{"repo": "cached/repo", "branch": "test"}"#,
        )
        .unwrap();

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
        assert_eq!(
            resolve_repo_from_state("/nonexistent/state.json"),
            None
        );
    }

    // --- run_cmd_inner large-output and timeout tests (issue #875) ---
    //
    // These verify the thread-drain pattern captures output exceeding the
    // kernel pipe buffer (~64KB) on BOTH the timeout and no-timeout branches.
    // The prior try_wait() + wait_with_output() pattern either deadlocked on
    // pipe-buffer fill or silently truncated via ECHILD on already-reaped
    // children. The no-timeout branch (which previously called wait_with_output
    // directly) had the same pipe-drain deadlock risk.

    // Timeout branch

    #[test]
    fn run_cmd_inner_timeout_captures_large_stdout() {
        let result = run_cmd_inner(
            "sh",
            &["-c", "for i in $(seq 1 20000); do echo \"line $i\"; done"],
            Some(Duration::from_secs(10)),
        );
        let (code, stdout_bytes, _) = result.expect("subprocess failed");
        assert_eq!(code, 0);
        let stdout = String::from_utf8_lossy(&stdout_bytes);
        assert!(stdout.contains("line 20000"), "last line missing — truncated");
        assert!(
            stdout_bytes.len() > 128_000,
            "stdout truncated: {} bytes",
            stdout_bytes.len()
        );
    }

    #[test]
    fn run_cmd_inner_timeout_captures_large_stderr_on_failure() {
        let result = run_cmd_inner(
            "sh",
            &[
                "-c",
                "for i in $(seq 1 20000); do echo \"err $i\" 1>&2; done; exit 6",
            ],
            Some(Duration::from_secs(10)),
        );
        let (code, _, stderr_bytes) = result.expect("subprocess failed");
        assert_eq!(code, 6);
        let stderr = String::from_utf8_lossy(&stderr_bytes);
        assert!(stderr.contains("err 20000"), "last stderr line missing");
        assert!(
            stderr_bytes.len() > 128_000,
            "stderr truncated: {} bytes",
            stderr_bytes.len()
        );
    }

    #[test]
    fn run_cmd_inner_enforces_timeout() {
        let start = std::time::Instant::now();
        let result = run_cmd_inner(
            "sh",
            &["-c", "sleep 10"],
            Some(Duration::from_secs(2)),
        );
        let elapsed = start.elapsed();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("timed out"));
        assert!(
            elapsed < Duration::from_secs(5),
            "timeout not enforced: elapsed {:?}",
            elapsed
        );
    }

    // No-timeout branch

    #[test]
    fn run_cmd_inner_no_timeout_captures_large_stdout() {
        let result = run_cmd_inner(
            "sh",
            &["-c", "for i in $(seq 1 20000); do echo \"line $i\"; done"],
            None,
        );
        let (code, stdout_bytes, _) = result.expect("subprocess failed");
        assert_eq!(code, 0);
        let stdout = String::from_utf8_lossy(&stdout_bytes);
        assert!(stdout.contains("line 20000"), "last line missing — truncated");
        assert!(
            stdout_bytes.len() > 128_000,
            "stdout truncated: {} bytes",
            stdout_bytes.len()
        );
    }

    #[test]
    fn run_cmd_inner_no_timeout_captures_large_stderr_on_failure() {
        let result = run_cmd_inner(
            "sh",
            &[
                "-c",
                "for i in $(seq 1 20000); do echo \"err $i\" 1>&2; done; exit 7",
            ],
            None,
        );
        let (code, _, stderr_bytes) = result.expect("subprocess failed");
        assert_eq!(code, 7);
        let stderr = String::from_utf8_lossy(&stderr_bytes);
        assert!(stderr.contains("err 20000"), "last stderr line missing");
        assert!(
            stderr_bytes.len() > 128_000,
            "stderr truncated: {} bytes",
            stderr_bytes.len()
        );
    }

    #[test]
    fn run_cmd_inner_no_timeout_propagates_exit_code() {
        // Verify the no-timeout branch correctly propagates non-zero exit codes
        // without requiring a poll loop.
        let result = run_cmd_inner("sh", &["-c", "exit 42"], None);
        let (code, _, _) = result.expect("subprocess failed");
        assert_eq!(code, 42);
    }
}
