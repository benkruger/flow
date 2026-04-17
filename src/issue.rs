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

use crate::complete_preflight::LOCAL_TIMEOUT;
use crate::flow_paths::FlowPaths;
use crate::git::{project_root, resolve_branch};
use crate::github::detect_repo;
use crate::output::{json_error, json_ok};

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

    /// Override the Code Review filing ban (requires explicit reason)
    #[arg(long = "override-code-review-ban")]
    pub override_code_review_ban: bool,
}

/// Returns a rejection message when the active flow is in Phase 4
/// Code Review and the override flag is not set. Enforces the
/// code-review-scope rule: Code Review triage has two outcomes (Real,
/// False positive); there is no filing path. The ban ensures real
/// findings are fixed while context is fresh — filing defers work that
/// a future session would rediscover from zero at full lifecycle cost.
/// The override exists as a deliberate-friction escape hatch for
/// exceptional cases the rule allows (e.g., a FLOW process gap raised
/// inside a Code Review that genuinely cannot wait for Phase 5 Learn).
///
/// - `state_json` is the raw contents of the current branch's state
///   file. `None` when no flow is active — the command is also used
///   outside FLOW, so that case passes.
/// - `override_flag` is the value of `--override-code-review-ban`.
///
/// The gate fails CLOSED when a state file exists but its
/// `current_phase` cannot be determined (parse failure, wrong type,
/// missing key). A state file that exists but is unreadable means a
/// flow is running — the safe default is reject, not silent pass.
pub(crate) fn should_reject_for_code_review(
    state_json: Option<&str>,
    override_flag: bool,
) -> Option<String> {
    if override_flag {
        return None;
    }
    let Some(content) = state_json else {
        // No state file — command is running outside an active flow.
        return None;
    };
    // Empty state is treated as "no flow"; any other non-empty content
    // that fails to parse or lacks current_phase is treated as "flow
    // state present but phase unknown" — fail CLOSED.
    if content.trim().is_empty() {
        return None;
    }
    let phase_norm = match serde_json::from_str::<serde_json::Value>(content) {
        Ok(state) => match state.get("current_phase").and_then(|v| v.as_str()) {
            Some(s) => s.replace('\0', "").trim().to_ascii_lowercase(),
            None => {
                return Some(fail_closed_message(
                    "state file exists but current_phase is missing or not a string",
                ));
            }
        },
        Err(_) => {
            return Some(fail_closed_message(
                "state file exists but is not valid JSON",
            ));
        }
    };
    if phase_norm == "flow-code-review" {
        Some(
            "bin/flow issue is disabled during Code Review. All real \
             findings must be fixed in Step 4. If this is a FLOW \
             process gap, file it during Phase 5 Learn. If truly \
             needed, pass --override-code-review-ban with an \
             explicit reason."
                .to_string(),
        )
    } else {
        None
    }
}

fn fail_closed_message(detail: &str) -> String {
    format!(
        "bin/flow issue cannot determine the current FLOW phase ({}). \
         Refusing to file while phase is unknown. Fix the state file, \
         finish the flow, or pass --override-code-review-ban with an \
         explicit reason.",
        detail
    )
}

#[derive(Debug)]
pub struct IssueResult {
    pub url: String,
    pub number: Option<i64>,
    pub id: Option<i64>,
}

/// Type alias for the gh-runner closure used by `_with_runner` seams.
/// Production binds to `&run_gh_cmd`. Tests inject mock closures
/// returning queued `Result<String, String>` responses per call.
pub type GhRunner = dyn Fn(&[&str], Option<Duration>) -> Result<String, String>;

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

    // Best-effort cleanup of the temp body file. The caller has
    // already received the body content, and the file is per-flow
    // scoped, so a deletion error here is non-fatal.
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

/// Fetch the REST API database ID for an issue via an injected runner.
/// Production wraps this with `&run_gh_cmd`. Tests inject mocks.
pub fn fetch_database_id_with_runner(
    repo: &str,
    number: i64,
    runner: &GhRunner,
) -> (Option<i64>, Option<String>) {
    let timeout = Duration::from_secs(LOCAL_TIMEOUT);
    let api_path = format!("repos/{}/issues/{}", repo, number);
    match runner(&["gh", "api", &api_path, "--jq", ".id"], Some(timeout)) {
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

/// Fetch the REST API database ID for an issue.
///
/// The database ID is the integer ID used by REST API endpoints for
/// sub-issues and dependencies. This is NOT the GraphQL node_id.
///
/// Returns (id, error). id is Some(integer) or None.
pub fn fetch_database_id(repo: &str, number: i64) -> (Option<i64>, Option<String>) {
    fetch_database_id_with_runner(repo, number, &run_gh_cmd)
}

/// Create-issue with an injected gh runner (testable seam).
pub fn create_issue_with_runner(
    repo: &str,
    title: &str,
    label: Option<&str>,
    body: Option<&str>,
    milestone: Option<&str>,
    runner: &GhRunner,
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
    match runner(&cmd_refs, Some(timeout)) {
        Ok(url) => Ok(build_issue_result_with_runner(repo, url, runner)),
        Err(error) => {
            // Label-not-found retry logic
            if let Some(l) = label {
                let err_lower = error.to_lowercase();
                if err_lower.contains("label") && err_lower.contains("not found") {
                    return retry_with_label_with_runner(
                        repo, title, l, body, milestone, timeout, runner,
                    );
                }
            }
            Err(error)
        }
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
    create_issue_with_runner(repo, title, label, body, milestone, &run_gh_cmd)
}

/// Retry-with-label with an injected gh runner. Production wraps with
/// `&run_gh_cmd`. Tests drive the label-create success/failure branches
/// and the retry-with/without-label branches via the runner queue.
pub fn retry_with_label_with_runner(
    repo: &str,
    title: &str,
    label: &str,
    body: Option<&str>,
    milestone: Option<&str>,
    timeout: Duration,
    runner: &GhRunner,
) -> Result<IssueResult, String> {
    // Try creating the label
    let label_created = runner(
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
    let url = runner(&retry_refs, Some(timeout))?;
    Ok(build_issue_result_with_runner(repo, url, runner))
}

fn build_issue_result_with_runner(repo: &str, url: String, runner: &GhRunner) -> IssueResult {
    let number = parse_issue_number(&url);
    let db_id = number.and_then(|n| {
        let (id, _) = fetch_database_id_with_runner(repo, n, runner);
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

    // Code Review filing gate: load the current branch's state file
    // (if any) and reject filing when current_phase is flow-code-review,
    // unless --override-code-review-ban was passed. Missing / malformed
    // state file passes through — this command is also used outside an
    // active flow.
    let state_json: Option<String> = resolve_branch(None, &root).and_then(|branch| {
        let state_path = FlowPaths::new(&root, &branch).state_file();
        fs::read_to_string(&state_path).ok()
    });
    if let Some(msg) =
        should_reject_for_code_review(state_json.as_deref(), args.override_code_review_ban)
    {
        json_error(&msg, &[]);
        std::process::exit(1);
    }

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

    // --- should_reject_for_code_review ---

    #[test]
    fn gate_blocks_when_current_phase_is_code_review() {
        let state = r#"{"current_phase":"flow-code-review"}"#;
        let msg = should_reject_for_code_review(Some(state), false);
        assert!(msg.is_some());
        let text = msg.unwrap();
        assert!(text.contains("Code Review"));
        assert!(text.contains("override-code-review-ban"));
    }

    #[test]
    fn gate_allows_with_override_in_code_review() {
        let state = r#"{"current_phase":"flow-code-review"}"#;
        assert!(should_reject_for_code_review(Some(state), true).is_none());
    }

    #[test]
    fn gate_allows_in_learn_phase() {
        let state = r#"{"current_phase":"flow-learn"}"#;
        assert!(should_reject_for_code_review(Some(state), false).is_none());
    }

    #[test]
    fn gate_allows_in_code_phase() {
        let state = r#"{"current_phase":"flow-code"}"#;
        assert!(should_reject_for_code_review(Some(state), false).is_none());
    }

    #[test]
    fn gate_allows_in_start_phase() {
        let state = r#"{"current_phase":"flow-start"}"#;
        assert!(should_reject_for_code_review(Some(state), false).is_none());
    }

    #[test]
    fn gate_allows_when_no_state_file() {
        // No state file means the command is running outside an active flow.
        assert!(should_reject_for_code_review(None, false).is_none());
    }

    #[test]
    fn gate_fails_closed_when_state_malformed() {
        let msg = should_reject_for_code_review(Some("not json"), false);
        assert!(msg.is_some(), "malformed state must fail CLOSED");
        assert!(msg.unwrap().contains("not valid JSON"));
    }

    #[test]
    fn gate_fails_closed_when_current_phase_missing() {
        let state = r#"{"branch":"x"}"#;
        let msg = should_reject_for_code_review(Some(state), false);
        assert!(msg.is_some(), "missing current_phase must fail CLOSED");
        assert!(msg.unwrap().contains("missing or not a string"));
    }

    #[test]
    fn gate_fails_closed_when_current_phase_is_array() {
        let state = r#"{"current_phase":["flow-code-review"]}"#;
        let msg = should_reject_for_code_review(Some(state), false);
        assert!(msg.is_some(), "non-string current_phase must fail CLOSED");
        assert!(msg.unwrap().contains("missing or not a string"));
    }

    #[test]
    fn gate_fails_closed_when_state_has_bom() {
        // UTF-8 BOM prefix breaks serde_json parsing.
        let state = "\u{feff}{\"current_phase\":\"flow-code-review\"}";
        let msg = should_reject_for_code_review(Some(state), false);
        assert!(msg.is_some(), "BOM prefix must fail CLOSED");
        assert!(msg.unwrap().contains("not valid JSON"));
    }

    #[test]
    fn gate_allows_when_state_is_empty_string() {
        // Empty content means "no flow" — the state file may be
        // mid-creation or the file was truncated and rewritten.
        assert!(should_reject_for_code_review(Some(""), false).is_none());
    }

    #[test]
    fn gate_allows_when_state_is_whitespace_only() {
        assert!(should_reject_for_code_review(Some("   \n  "), false).is_none());
    }

    #[test]
    fn gate_blocks_when_current_phase_is_whitespace_padded() {
        let state = r#"{"current_phase":" flow-code-review "}"#;
        let msg = should_reject_for_code_review(Some(state), false);
        assert!(msg.is_some(), "whitespace drift must not bypass the gate");
        assert!(msg.unwrap().contains("Code Review"));
    }

    #[test]
    fn gate_blocks_when_current_phase_is_uppercase() {
        let state = r#"{"current_phase":"FLOW-CODE-REVIEW"}"#;
        let msg = should_reject_for_code_review(Some(state), false);
        assert!(msg.is_some(), "case drift must not bypass the gate");
        assert!(msg.unwrap().contains("Code Review"));
    }

    #[test]
    fn gate_blocks_when_current_phase_has_trailing_nul() {
        let state = "{\"current_phase\":\"flow-code-review\\u0000\"}";
        let msg = should_reject_for_code_review(Some(state), false);
        assert!(msg.is_some(), "embedded NUL must not bypass the gate");
        assert!(msg.unwrap().contains("Code Review"));
    }

    // --- Args override flag ---

    #[test]
    fn args_parses_override_code_review_ban() {
        let args = Args::try_parse_from(["issue", "--title", "Test", "--override-code-review-ban"])
            .unwrap();
        assert!(args.override_code_review_ban);
    }

    #[test]
    fn args_override_defaults_to_false() {
        let args = Args::try_parse_from(["issue", "--title", "Test"]).unwrap();
        assert!(!args.override_code_review_ban);
    }

    // --- _with_runner seams (create_issue, retry_with_label, fetch_database_id) ---

    use std::cell::RefCell;
    use std::collections::VecDeque;

    type GhResult = Result<String, String>;

    fn mock_runner(responses: Vec<GhResult>) -> impl Fn(&[&str], Option<Duration>) -> GhResult {
        let queue = RefCell::new(VecDeque::from(responses));
        move |_args: &[&str], _timeout: Option<Duration>| -> GhResult {
            queue
                .borrow_mut()
                .pop_front()
                .expect("no more mock responses")
        }
    }

    #[test]
    fn create_issue_with_runner_returns_result_on_runner_ok() {
        let runner = mock_runner(vec![
            Ok("https://github.com/owner/name/issues/42".to_string()),
            Ok("12345".to_string()),
        ]);
        let result =
            create_issue_with_runner("owner/name", "Title", None, None, None, &runner).unwrap();
        assert_eq!(result.url, "https://github.com/owner/name/issues/42");
        assert_eq!(result.number, Some(42));
        assert_eq!(result.id, Some(12345));
    }

    #[test]
    fn create_issue_with_runner_propagates_err_when_label_none() {
        let runner = mock_runner(vec![Err("network down".to_string())]);
        let err =
            create_issue_with_runner("owner/name", "Title", None, None, None, &runner).unwrap_err();
        assert!(err.contains("network down"));
    }

    #[test]
    fn create_issue_with_runner_label_not_found_triggers_retry() {
        // Sequence: create fails with "label not found" → label create OK → retry OK → fetch_database_id OK
        let runner = mock_runner(vec![
            Err("could not add label: label not found".to_string()),
            Ok(String::new()),
            Ok("https://github.com/owner/name/issues/77".to_string()),
            Ok("9999".to_string()),
        ]);
        let result =
            create_issue_with_runner("owner/name", "Title", Some("Bug"), None, None, &runner)
                .unwrap();
        assert_eq!(result.number, Some(77));
        assert_eq!(result.id, Some(9999));
    }

    #[test]
    fn create_issue_with_runner_propagates_unrelated_err() {
        let runner = mock_runner(vec![Err("authentication failed".to_string())]);
        let err = create_issue_with_runner("owner/name", "Title", Some("Bug"), None, None, &runner)
            .unwrap_err();
        assert!(err.contains("authentication failed"));
    }

    #[test]
    fn retry_with_label_with_runner_label_created_then_retry_succeeds() {
        let runner = mock_runner(vec![
            Ok(String::new()),
            Ok("https://github.com/owner/name/issues/55".to_string()),
            Ok("5555".to_string()),
        ]);
        let result = retry_with_label_with_runner(
            "owner/name",
            "Title",
            "Flow",
            None,
            None,
            Duration::from_secs(5),
            &runner,
        )
        .unwrap();
        assert_eq!(result.number, Some(55));
    }

    #[test]
    fn retry_with_label_with_runner_label_create_fails_retries_without_label() {
        let runner = mock_runner(vec![
            Err("label create permission denied".to_string()),
            Ok("https://github.com/owner/name/issues/33".to_string()),
            Ok("3333".to_string()),
        ]);
        let result = retry_with_label_with_runner(
            "owner/name",
            "Title",
            "Flow",
            None,
            None,
            Duration::from_secs(5),
            &runner,
        )
        .unwrap();
        assert_eq!(result.number, Some(33));
    }

    #[test]
    fn retry_with_label_with_runner_retry_fails_propagates_err() {
        let runner = mock_runner(vec![Ok(String::new()), Err("retry timeout".to_string())]);
        let err = retry_with_label_with_runner(
            "owner/name",
            "Title",
            "Flow",
            None,
            None,
            Duration::from_secs(5),
            &runner,
        )
        .unwrap_err();
        assert!(err.contains("retry timeout"));
    }

    #[test]
    fn fetch_database_id_with_runner_returns_id_on_ok_numeric() {
        let runner = mock_runner(vec![Ok("42".to_string())]);
        let (id, err) = fetch_database_id_with_runner("owner/name", 1, &runner);
        assert_eq!(id, Some(42));
        assert!(err.is_none());
    }

    #[test]
    fn fetch_database_id_with_runner_returns_err_on_invalid_id() {
        let runner = mock_runner(vec![Ok("not-a-number".to_string())]);
        let (id, err) = fetch_database_id_with_runner("owner/name", 1, &runner);
        assert!(id.is_none());
        assert!(err.unwrap().contains("Invalid ID"));
    }

    #[test]
    fn fetch_database_id_with_runner_propagates_runner_err() {
        let runner = mock_runner(vec![Err("api down".to_string())]);
        let (id, err) = fetch_database_id_with_runner("owner/name", 1, &runner);
        assert!(id.is_none());
        assert!(err.unwrap().contains("api down"));
    }
}
