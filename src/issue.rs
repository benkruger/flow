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
//!
//! Tests live in `tests/issue.rs` per `.claude/rules/test-placement.md` —
//! no inline `#[cfg(test)]` block in this file.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Parser;
use regex::Regex;
use serde_json::json;

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
    let content = state_json?;
    if content.trim().is_empty() {
        return None;
    }
    // Defense in depth: serde_json's default last-wins behavior on
    // duplicate keys lets a crafted state file like
    // `{"current_phase":"flow-code-review","current_phase":"flow-learn"}`
    // bypass the gate when the parsed value is read normally. Scan the
    // raw content for ANY occurrence of `"current_phase"` followed by a
    // value that normalizes to `flow-code-review`. If any match, reject.
    if raw_contains_code_review_phase(content) {
        return Some(code_review_block_message());
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
        Some(code_review_block_message())
    } else {
        None
    }
}

fn code_review_block_message() -> String {
    "bin/flow issue is disabled during Code Review. All real \
     findings must be fixed in Step 4. If this is a FLOW \
     process gap, file it during Phase 5 Learn. If truly \
     needed, pass --override-code-review-ban with an \
     explicit reason."
        .to_string()
}

fn raw_contains_code_review_phase(content: &str) -> bool {
    let needle = "\"current_phase\"";
    let mut start = 0;
    while let Some(pos) = content[start..].find(needle) {
        let key_end = start + pos + needle.len();
        let after_key = content[key_end..].trim_start();
        if let Some(rest) = after_key.strip_prefix(':') {
            let after_colon = rest.trim_start();
            if let Some(value_body) = after_colon.strip_prefix('"') {
                if let Some(end_quote) = value_body.find('"') {
                    let value = &value_body[..end_quote];
                    let normalized = value.replace('\0', "").trim().to_ascii_lowercase();
                    if normalized == "flow-code-review" {
                        return true;
                    }
                }
            }
        }
        start = key_end;
    }
    false
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

struct IssueResult {
    url: String,
    number: Option<i64>,
    id: Option<i64>,
}

/// Read body text from a file and delete the file.
/// Relative paths are resolved against `root`.
pub fn read_body_file(path: &str, root: &Path) -> Result<String, String> {
    let resolved: PathBuf = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        root.join(path)
    };

    let body = fs::read_to_string(&resolved)
        .map_err(|e| format!("Could not read body file '{}': {}", resolved.display(), e))?;

    // Best-effort cleanup of the temp body file.
    let _ = fs::remove_file(&resolved);

    Ok(body)
}

/// Extract issue number from a GitHub issue URL.
pub fn parse_issue_number(url: &str) -> Option<i64> {
    let re = Regex::new(r"/issues/(\d+)").unwrap();
    re.captures(url).and_then(|cap| cap[1].parse().ok())
}

/// Fetch the REST API database ID for an issue. Returns (id, error).
/// Used cross-module by `create_sub_issue.rs` and `link_blocked_by.rs`.
pub fn fetch_database_id(repo: &str, number: i64) -> (Option<i64>, Option<String>) {
    let api_path = format!("repos/{}/issues/{}", repo, number);
    match run_gh_cmd(&["gh", "api", &api_path, "--jq", ".id"]) {
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

/// Run gh issue create and return issue details. Includes label-not-
/// found retry logic: if the label doesn't exist, tries to create it
/// and retries. If label creation fails, retries without the label.
fn create_issue(
    repo: &str,
    title: &str,
    label: Option<&str>,
    body: Option<&str>,
    milestone: Option<&str>,
) -> Result<IssueResult, String> {
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
    match run_gh_cmd(&cmd_refs) {
        Ok(url) => Ok(build_issue_result(repo, url)),
        Err(error) => {
            if let Some(l) = label {
                let err_lower = error.to_lowercase();
                if err_lower.contains("label") && err_lower.contains("not found") {
                    return retry_with_label(repo, title, l, body, milestone);
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
) -> Result<IssueResult, String> {
    let label_created = run_gh_cmd(&["gh", "label", "create", label, "--repo", repo]).is_ok();

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
    let url = run_gh_cmd(&retry_refs)?;
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

/// Run a gh CLI command, returning stdout on success. Used cross-module
/// by `create_sub_issue.rs`, `link_blocked_by.rs`, and `create_milestone.rs`.
/// gh has its own network timeout so no hand-rolled loop is needed per
/// .claude/rules/testability-means-simplicity.md.
pub fn run_gh_cmd(args: &[&str]) -> Result<String, String> {
    let output = Command::new(args[0])
        .args(&args[1..])
        .output()
        .map_err(|e| format!("Failed to spawn: {}", e))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Err(extract_error(&stderr, &stdout))
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

/// Main-arm dispatcher: compute the issue-create result and pair it with
/// an exit code. Returns `(value, 0)` on success, `(error_value, 1)` on
/// any failure path. All previously `process::exit`-bearing branches
/// (Code Review filing block, repo-detect failure, body-file read
/// failure, gh-create failure) return the error tuple instead.
///
/// Closure parameters seam off the production dependencies:
/// - `state_reader` returns the current branch's state file content
///   (or `None` if no flow is active). Production binds it to
///   `resolve_branch + read_to_string`.
/// - `repo_resolver` returns the repo from `git remote` (or `None`).
///   Production binds it to `detect_repo(Some(root))`.
pub fn run_impl_main(
    args: Args,
    root: &Path,
    state_reader: &dyn Fn() -> Option<String>,
    repo_resolver: &dyn Fn() -> Option<String>,
) -> (serde_json::Value, i32) {
    // Code Review filing gate.
    let state_json = state_reader();
    if let Some(msg) =
        should_reject_for_code_review(state_json.as_deref(), args.override_code_review_ban)
    {
        return (json!({"status": "error", "message": msg}), 1);
    }

    // Resolve repo: --repo > --state-file > repo_resolver().
    let repo = if let Some(r) = args.repo {
        r
    } else if let Some(ref sf) = args.state_file {
        match resolve_repo_from_state(sf).or_else(repo_resolver) {
            Some(r) => r,
            None => {
                return (
                    json!({"status": "error", "message": "Could not detect repo from git remote. Use --repo owner/name."}),
                    1,
                )
            }
        }
    } else {
        match repo_resolver() {
            Some(r) => r,
            None => {
                return (
                    json!({"status": "error", "message": "Could not detect repo from git remote. Use --repo owner/name."}),
                    1,
                )
            }
        }
    };

    let body = if let Some(ref bf) = args.body_file {
        match read_body_file(bf, root) {
            Ok(b) => Some(b),
            Err(e) => return (json!({"status": "error", "message": e}), 1),
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
        Ok(result) => (
            json!({
                "status": "ok",
                "url": result.url,
                "number": result.number,
                "id": result.id,
            }),
            0,
        ),
        Err(e) => (json!({"status": "error", "message": e}), 1),
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
