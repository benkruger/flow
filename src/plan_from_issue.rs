//! Sentinel-based plan extractor for `bin/flow plan-from-issue`.
//!
//! The `plan-from-issue` subcommand replaces the heuristic
//! `plan-extract` path with a five-line scan over an issue body's
//! `<!-- FLOW-PLAN-BEGIN -->` / `<!-- FLOW-PLAN-END -->` markers.
//! The bytes between the first BEGIN and the first END after it are
//! the plan, returned verbatim.

use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::flow_paths::FlowPaths;

/// Maximum issue-body size accepted by `extract_plan`.
///
/// 1 MiB bounds the worst-case malicious or runaway issue body so a
/// single oversized fetch cannot exhaust process memory. Issue bodies
/// larger than the cap reject before any marker scan runs.
pub const PLAN_BODY_BYTE_CAP: usize = 1_048_576;

const BEGIN_MARKER: &str = "<!-- FLOW-PLAN-BEGIN -->";
const END_MARKER: &str = "<!-- FLOW-PLAN-END -->";

/// Reasons `extract_plan` rejects an issue body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractError {
    /// Neither sentinel marker appears in the body.
    MarkersMissing,
    /// One marker is present without its pair, or `END` appears with
    /// no following `BEGIN` predecessor.
    MarkersMalformed,
    /// Markers delimit a region that is empty or whitespace-only.
    Empty,
    /// Body exceeds `PLAN_BODY_BYTE_CAP` bytes.
    TooLarge,
}

impl fmt::Display for ExtractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            ExtractError::MarkersMissing => {
                "issue body contains neither FLOW-PLAN-BEGIN nor FLOW-PLAN-END marker"
            }
            ExtractError::MarkersMalformed => {
                "issue body has an unmatched or out-of-order FLOW-PLAN marker pair"
            }
            ExtractError::Empty => "issue body has empty content between FLOW-PLAN markers",
            ExtractError::TooLarge => "issue body exceeds the 1 MiB cap",
        };
        f.write_str(msg)
    }
}

impl Error for ExtractError {}

/// Reasons `fetch_issue_body` rejects the gh subprocess result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// `gh` reported the issue does not exist.
    IssueNotFound { issue: u64 },
    /// `gh` reported the issue exists but is closed.
    IssueClosed { issue: u64 },
    /// Any other failure from `gh` — spawn error, malformed JSON,
    /// authentication failure, etc.
    GhFailed(String),
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FetchError::IssueNotFound { issue } => write!(f, "issue #{} not found", issue),
            FetchError::IssueClosed { issue } => write!(f, "issue #{} is closed", issue),
            FetchError::GhFailed(msg) => write!(f, "gh subprocess failed: {}", msg),
        }
    }
}

impl Error for FetchError {}

/// Fetch the body of `issue_number` via `gh issue view`.
///
/// Returns the body text on success, or a `FetchError` classifying the
/// gh failure. The function spawns `gh issue view <N> --json body,state`
/// and parses the JSON response. Authentication and repo detection are
/// the responsibility of the user's `gh` configuration.
pub fn fetch_issue_body(issue_number: u64) -> Result<String, FetchError> {
    let output = Command::new("gh")
        .args([
            "issue",
            "view",
            &issue_number.to_string(),
            "--json",
            "body,state",
        ])
        .output()
        .map_err(|e| FetchError::GhFailed(format!("spawn failed: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let lower = stderr.to_lowercase();
        if lower.contains("not found")
            || lower.contains("could not resolve")
            || lower.contains("no issue")
        {
            return Err(FetchError::IssueNotFound {
                issue: issue_number,
            });
        }
        return Err(FetchError::GhFailed(stderr.trim().to_string()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| FetchError::GhFailed(format!("parse json: {}", e)))?;

    let state = parsed["state"].as_str().unwrap_or("");
    if state.eq_ignore_ascii_case("closed") {
        return Err(FetchError::IssueClosed {
            issue: issue_number,
        });
    }

    Ok(parsed["body"].as_str().unwrap_or("").to_string())
}

/// Reasons `write_plan` rejects the write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    /// Branch failed `FlowPaths::is_valid_branch` validation.
    InvalidBranch(String),
    /// Filesystem error creating the directory or writing the file.
    Io(String),
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::InvalidBranch(b) => write!(f, "invalid branch name: {:?}", b),
            WriteError::Io(msg) => write!(f, "filesystem error: {}", msg),
        }
    }
}

impl Error for WriteError {}

/// Write `content` to `<root>/.flow-states/<branch>/plan.md`.
///
/// Returns the resolved plan path on success. Validates `branch`
/// through `FlowPaths::try_new` per
/// `.claude/rules/branch-path-safety.md` so a slash-containing or
/// path-traversing branch cannot escape the per-branch subdirectory.
pub fn write_plan(root: &Path, branch: &str, content: &str) -> Result<PathBuf, WriteError> {
    let paths = FlowPaths::try_new(root, branch)
        .ok_or_else(|| WriteError::InvalidBranch(branch.to_string()))?;
    paths
        .ensure_branch_dir()
        .map_err(|e| WriteError::Io(format!("create dir: {}", e)))?;
    let plan_path = paths.plan_file();
    fs::write(&plan_path, content).map_err(|e| WriteError::Io(format!("write: {}", e)))?;
    Ok(plan_path)
}

/// CLI arguments for `bin/flow plan-from-issue`.
#[derive(clap::Parser, Debug)]
#[command(name = "plan-from-issue")]
pub struct Args {
    /// Issue number to fetch.
    #[arg(long)]
    pub issue: u64,
    /// Branch name (used to compute the plan file path).
    #[arg(long)]
    pub branch: String,
}

/// Main-arm dispatcher for `bin/flow plan-from-issue`.
///
/// Returns a JSON envelope and exit code. Success emits
/// `{"status":"ok","plan_path":"...","branch":"..."}`; errors emit
/// `{"status":"error","reason":"<class>","issue":N,"message":"..."}`.
/// Exit code is `0` for success and business errors (the `status`
/// field signals failure) per `.claude/rules/rust-patterns.md`
/// "Exit code convention for business errors". Exit code `1` is
/// reserved for infrastructure failures that escape the JSON
/// contract.
pub fn run_impl_main(args: &Args, root: &Path) -> (serde_json::Value, i32) {
    let body = match fetch_issue_body(args.issue) {
        Ok(b) => b,
        Err(FetchError::IssueNotFound { issue }) => {
            return (
                serde_json::json!({
                    "status": "error",
                    "reason": "issue_not_found",
                    "issue": issue,
                    "message": format!("issue #{} not found via gh", issue),
                }),
                0,
            );
        }
        Err(FetchError::IssueClosed { issue }) => {
            return (
                serde_json::json!({
                    "status": "error",
                    "reason": "issue_closed",
                    "issue": issue,
                    "message": format!("issue #{} is closed", issue),
                }),
                0,
            );
        }
        Err(FetchError::GhFailed(msg)) => {
            return (
                serde_json::json!({
                    "status": "error",
                    "reason": "gh_fetch_failed",
                    "issue": args.issue,
                    "message": format!("gh subprocess failed: {}", msg),
                }),
                0,
            );
        }
    };

    let plan_content = match extract_plan(&body) {
        Ok(c) => c.to_string(),
        Err(ExtractError::MarkersMissing) => {
            return error_envelope(
                args.issue,
                "plan_markers_missing",
                &ExtractError::MarkersMissing,
            );
        }
        Err(ExtractError::MarkersMalformed) => {
            return error_envelope(
                args.issue,
                "plan_markers_malformed",
                &ExtractError::MarkersMalformed,
            );
        }
        Err(ExtractError::Empty) => {
            return error_envelope(args.issue, "plan_empty", &ExtractError::Empty);
        }
        Err(ExtractError::TooLarge) => {
            return error_envelope(args.issue, "plan_too_large", &ExtractError::TooLarge);
        }
    };

    let plan_path = match write_plan(root, &args.branch, &plan_content) {
        Ok(p) => p,
        Err(WriteError::InvalidBranch(b)) => {
            return (
                serde_json::json!({
                    "status": "error",
                    "reason": "invalid_branch",
                    "issue": args.issue,
                    "message": format!("invalid branch name: {:?}", b),
                }),
                0,
            );
        }
        Err(WriteError::Io(msg)) => {
            return (
                serde_json::json!({
                    "status": "error",
                    "reason": "write_failed",
                    "issue": args.issue,
                    "message": format!("filesystem error: {}", msg),
                }),
                0,
            );
        }
    };

    (
        serde_json::json!({
            "status": "ok",
            "plan_path": plan_path.to_string_lossy(),
            "branch": args.branch,
            "issue": args.issue,
        }),
        0,
    )
}

fn error_envelope(issue: u64, reason: &str, err: &ExtractError) -> (serde_json::Value, i32) {
    (
        serde_json::json!({
            "status": "error",
            "reason": reason,
            "issue": issue,
            "message": err.to_string(),
        }),
        0,
    )
}

/// Extract the plan content delimited by FLOW-PLAN markers.
///
/// Returns the slice between the first `<!-- FLOW-PLAN-BEGIN -->` and
/// the first `<!-- FLOW-PLAN-END -->` after it. Rejects bodies with
/// missing markers, malformed pairs, empty content, or sizes over
/// `PLAN_BODY_BYTE_CAP`.
pub fn extract_plan(body: &str) -> Result<&str, ExtractError> {
    if body.len() > PLAN_BODY_BYTE_CAP {
        return Err(ExtractError::TooLarge);
    }

    let begin_pos = body.find(BEGIN_MARKER);
    let has_end = body.contains(END_MARKER);

    let begin_idx = match begin_pos {
        Some(i) => i,
        None => {
            return if has_end {
                Err(ExtractError::MarkersMalformed)
            } else {
                Err(ExtractError::MarkersMissing)
            };
        }
    };

    if !has_end {
        return Err(ExtractError::MarkersMalformed);
    }

    let plan_start = begin_idx + BEGIN_MARKER.len();
    let end_idx = match body[plan_start..].find(END_MARKER) {
        Some(rel) => plan_start + rel,
        None => return Err(ExtractError::MarkersMalformed),
    };

    let content = &body[plan_start..end_idx];
    if content.trim().is_empty() {
        return Err(ExtractError::Empty);
    }
    Ok(content)
}
