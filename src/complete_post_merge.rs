//! Consolidated Complete phase post-merge.
//!
//! Absorbs Steps 7 + 9 + 10: phase completion, PR body render, issues summary,
//! close issues, summary generation, label removal, auto-close parents, and
//! Slack notification. All operations are best-effort.
//!
//! Usage: bin/flow complete-post-merge --pr <N> --state-file <path> --branch <name>
//!
//! Tests live in `tests/complete_post_merge.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` block
//! in this file.

use std::path::Path;

use clap::Parser;
use serde_json::{json, Map, Value};

use crate::commands::log::append_log;
use crate::complete_preflight::{run_cmd_with_timeout, LOCAL_TIMEOUT, NETWORK_TIMEOUT};
use crate::flow_paths::FlowPaths;
use crate::git::project_root;
use crate::lock::mutate_state;
use crate::utils::bin_flow_path;
const POST_MERGE_STEP: i64 = 6;

#[derive(Parser, Debug)]
#[command(
    name = "complete-post-merge",
    about = "FLOW Complete phase post-merge operations"
)]
pub struct Args {
    /// PR number
    #[arg(long, required = true)]
    pub pr: i64,
    /// Path to state file
    #[arg(long = "state-file", required = true)]
    pub state_file: String,
    /// Branch name
    #[arg(long, required = true)]
    pub branch: String,
}

/// Parse JSON from stdout. Returns (parsed_value, parse_error).
fn parse_json_or(stdout: &str) -> (Option<Value>, Option<String>) {
    match serde_json::from_str::<Value>(stdout.trim()) {
        Ok(v) => (Some(v), None),
        Err(e) => (None, Some(e.to_string())),
    }
}

/// Collapse a `CmdResult` + JSON-parse into a single `Option<Value>`.
/// Returns `None` when the subprocess spawn failed OR the stdout is
/// not valid JSON; `Some(value)` otherwise. Used by call sites that
/// treat both outcomes as "skip downstream processing."
pub fn ok_stdout_as_json(result: crate::complete_preflight::CmdResult) -> Option<Value> {
    let (_code, stdout, _stderr) = result.ok()?;
    parse_json_or(&stdout).0
}

/// Production wrapper: runs the full post-merge sequence. Best-effort
/// throughout — all subcommand failures land in the JSON `failures`
/// map rather than raising.
pub fn post_merge(pr_number: i64, state_file: &str, branch: &str) -> Value {
    let root = project_root();
    let bin_flow = bin_flow_path();
    let state_path = Path::new(state_file);

    // Initialize result with default fields (preserve_order maintains this order)
    let mut result: Map<String, Value> = Map::new();
    result.insert("status".to_string(), json!("ok"));
    result.insert("formatted_time".to_string(), json!(""));
    result.insert("cumulative_seconds".to_string(), json!(0));
    result.insert("summary".to_string(), json!(""));
    result.insert("issues_links".to_string(), json!(""));
    result.insert("banner_line".to_string(), json!(""));
    result.insert("closed_issues".to_string(), json!([]));
    result.insert("parents_closed".to_string(), json!([]));
    result.insert("slack".to_string(), json!({"status": "skipped"}));
    let mut failures: Map<String, Value> = Map::new();

    // Best-effort logging — `try_new` tolerates slash-containing
    // branches per `.claude/rules/external-input-validation.md` because
    // `--branch` is external CLI input. When the branch is invalid for
    // FlowPaths (contains '/' or is empty), return the initialized
    // result with a single `invalid_branch` failure rather than
    // panicking.
    let paths = match FlowPaths::try_new(&root, branch) {
        Some(p) => p,
        None => {
            failures.insert(
                "invalid_branch".to_string(),
                json!(format!(
                    "Branch '{}' contains '/' or is empty; complete-post-merge artifact paths require a canonical flat branch name",
                    branch
                )),
            );
            result.insert("failures".to_string(), Value::Object(failures));
            return Value::Object(result);
        }
    };
    let log = |msg: &str| {
        if paths.flow_states_dir().is_dir() {
            let _ = append_log(&root, branch, msg);
        }
    };

    // Read state for slack_thread_ts and repo (tolerate corrupt JSON)
    let state: Value = if state_path.exists() {
        match std::fs::read_to_string(state_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or(json!({})),
            Err(_) => json!({}),
        }
    } else {
        json!({})
    };

    let repo: Option<String> = state
        .get("repo")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    // --- Step 7: Archive artifacts to PR ---

    // Set step counter
    if state_path.exists() {
        match mutate_state(state_path, &mut |s| {
            if !(s.is_object() || s.is_null()) {
                return;
            }
            s["complete_step"] = json!(POST_MERGE_STEP);
        }) {
            Ok(_) => {}
            Err(_) => {
                failures.insert(
                    "step_counter".to_string(),
                    json!("could not update step counter"),
                );
            }
        }
    }

    // Phase transition complete
    let pt_args = [
        bin_flow.as_str(),
        "phase-transition",
        "--phase",
        "flow-complete",
        "--action",
        "complete",
        "--next-phase",
        "flow-complete",
        "--branch",
        branch,
    ];
    match run_cmd_with_timeout(&pt_args, NETWORK_TIMEOUT) {
        Err(e) => {
            log("[Phase 6] complete-post-merge — phase-transition (error)");
            failures.insert("phase_transition".to_string(), json!(e));
        }
        Ok((_code, stdout, stderr)) => {
            let (parsed, parse_err) = parse_json_or(&stdout);
            match parsed.as_ref() {
                Some(pt_data) if pt_data.get("status").and_then(|v| v.as_str()) == Some("ok") => {
                    let formatted_time = pt_data
                        .get("formatted_time")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let cumulative_seconds = pt_data
                        .get("cumulative_seconds")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    result.insert("formatted_time".to_string(), json!(formatted_time));
                    result.insert("cumulative_seconds".to_string(), json!(cumulative_seconds));
                    log("[Phase 6] complete-post-merge — phase-transition (ok)");
                }
                _ => {
                    let msg = parse_err.unwrap_or_else(|| stderr.trim().to_string());
                    log("[Phase 6] complete-post-merge — phase-transition (failed)");
                    failures.insert("phase_transition".to_string(), json!(msg));
                }
            }
        }
    }

    // Render PR body
    let pr_str = pr_number.to_string();
    let render_args = [
        bin_flow.as_str(),
        "render-pr-body",
        "--pr",
        &pr_str,
        "--state-file",
        state_file,
    ];
    match run_cmd_with_timeout(&render_args, NETWORK_TIMEOUT) {
        Err(e) => {
            log("[Phase 6] complete-post-merge — render-pr-body (error)");
            failures.insert("render_pr_body".to_string(), json!(e));
        }
        Ok((code, _, stderr)) => {
            if code != 0 {
                log("[Phase 6] complete-post-merge — render-pr-body (failed)");
                failures.insert("render_pr_body".to_string(), json!(stderr.trim()));
            } else {
                log("[Phase 6] complete-post-merge — render-pr-body (ok)");
            }
        }
    }

    // Format issues summary
    let issues_output_path = paths.issues_file();
    let issues_output = issues_output_path.to_string_lossy().to_string();
    let iss_args = [
        bin_flow.as_str(),
        "format-issues-summary",
        "--state-file",
        state_file,
        "--output",
        &issues_output,
    ];
    if let Ok((_code, stdout, _stderr)) = run_cmd_with_timeout(&iss_args, LOCAL_TIMEOUT) {
        let (parsed, _) = parse_json_or(&stdout);
        if let Some(iss_data) = parsed {
            if iss_data.get("has_issues").and_then(|v| v.as_bool()) == Some(true) {
                let banner = iss_data
                    .get("banner_line")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                result.insert("banner_line".to_string(), json!(banner));
            }
        }
    }

    // --- Step 9: Close referenced issues ---

    let close_args = [
        bin_flow.as_str(),
        "close-issues",
        "--state-file",
        state_file,
    ];
    let mut closed_issues: Vec<Value> = Vec::new();
    if let Ok((_code, stdout, _stderr)) = run_cmd_with_timeout(&close_args, NETWORK_TIMEOUT) {
        let (parsed, _) = parse_json_or(&stdout);
        if let Some(close_data) = parsed {
            if let Some(closed_arr) = close_data.get("closed").and_then(|v| v.as_array()) {
                closed_issues = closed_arr.clone();
            }
        }
    }
    result.insert("closed_issues".to_string(), json!(closed_issues.clone()));
    log(&format!(
        "[Phase 6] complete-post-merge — close-issues ({} closed)",
        closed_issues.len(),
    ));

    if !closed_issues.is_empty() {
        let closed_path = paths.closed_issues();
        let closed_json =
            serde_json::to_string(&closed_issues).expect("Vec<Value> to_string is infallible");
        if let Err(e) = std::fs::write(&closed_path, closed_json) {
            failures.insert("closed_issues_file".to_string(), json!(e.to_string()));
        }
    }

    // --- Step 10: Parallel post-merge operations ---

    // Format complete summary
    let closed_file_path_buf = paths.closed_issues();
    let closed_file_path = closed_file_path_buf.to_string_lossy().to_string();
    let mut sum_args: Vec<&str> = vec![
        bin_flow.as_str(),
        "format-complete-summary",
        "--state-file",
        state_file,
    ];
    if !closed_issues.is_empty() {
        sum_args.push("--closed-issues-file");
        sum_args.push(&closed_file_path);
    }
    if let Ok((_code, stdout, _stderr)) = run_cmd_with_timeout(&sum_args, LOCAL_TIMEOUT) {
        let (parsed, _) = parse_json_or(&stdout);
        if let Some(sum_data) = parsed {
            if sum_data.get("status").and_then(|v| v.as_str()) == Some("ok") {
                let summary = sum_data
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let issues_links = sum_data
                    .get("issues_links")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                result.insert("summary".to_string(), json!(summary));
                result.insert("issues_links".to_string(), json!(issues_links));
            }
        }
    }

    // Remove In-Progress labels
    let label_args = [
        bin_flow.as_str(),
        "label-issues",
        "--state-file",
        state_file,
        "--remove",
    ];
    match run_cmd_with_timeout(&label_args, NETWORK_TIMEOUT) {
        Err(e) => {
            failures.insert("label_issues".to_string(), json!(e));
        }
        Ok((code, _, stderr)) => {
            if code != 0 {
                failures.insert("label_issues".to_string(), json!(stderr.trim()));
            }
        }
    }

    // Auto-close parent issues for each closed issue
    let mut parents_closed: Vec<i64> = Vec::new();
    if let Some(ref repo_str) = repo {
        for issue in &closed_issues {
            if let Some(issue_num) = issue.get("number").and_then(|v| v.as_i64()) {
                let issue_num_str = issue_num.to_string();
                let acp_args = [
                    bin_flow.as_str(),
                    "auto-close-parent",
                    "--repo",
                    repo_str.as_str(),
                    "--issue-number",
                    &issue_num_str,
                ];
                if let Some(acp_data) =
                    ok_stdout_as_json(run_cmd_with_timeout(&acp_args, NETWORK_TIMEOUT))
                {
                    let parent_closed = acp_data
                        .get("parent_closed")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let milestone_closed = acp_data
                        .get("milestone_closed")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if parent_closed || milestone_closed {
                        parents_closed.push(issue_num);
                    }
                }
            }
        }
    }
    result.insert("parents_closed".to_string(), json!(parents_closed));

    // Slack notification — only post if a non-empty thread_ts is set.
    let slack_thread_ts: Option<String> = state
        .get("slack_thread_ts")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    if let Some(ref thread_ts) = slack_thread_ts {
        let msg = format!("Phase 6: Complete finished for PR #{}", pr_number);
        let slack_args = [
            bin_flow.as_str(),
            "notify-slack",
            "--phase",
            "flow-complete",
            "--message",
            &msg,
            "--thread-ts",
            thread_ts.as_str(),
        ];
        match run_cmd_with_timeout(&slack_args, NETWORK_TIMEOUT) {
            Err(e) => {
                result.insert(
                    "slack".to_string(),
                    json!({"status": "error", "message": e}),
                );
            }
            Ok((_code, stdout, _stderr)) => {
                let (parsed, _) = parse_json_or(&stdout);
                match parsed {
                    Some(slack_data) => {
                        let status_ok =
                            slack_data.get("status").and_then(|v| v.as_str()) == Some("ok");
                        let ts_opt = slack_data
                            .get("ts")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                            .map(String::from);
                        result.insert("slack".to_string(), slack_data);
                        if status_ok {
                            if let Some(ts) = ts_opt {
                                let add_args = [
                                    bin_flow.as_str(),
                                    "add-notification",
                                    "--phase",
                                    "flow-complete",
                                    "--ts",
                                    ts.as_str(),
                                    "--thread-ts",
                                    thread_ts.as_str(),
                                    "--message",
                                    &msg,
                                ];
                                // Fire-and-forget: the notification
                                // record is best-effort and a failure
                                // here must not roll back the merge.
                                let _ = run_cmd_with_timeout(&add_args, LOCAL_TIMEOUT);
                            }
                        }
                    }
                    None => {
                        result.insert(
                            "slack".to_string(),
                            json!({"status": "error", "message": "invalid slack response"}),
                        );
                    }
                }
            }
        }
    }

    let failure_count = failures.len();
    result.insert("failures".to_string(), Value::Object(failures));
    log(&format!(
        "[Phase 6] complete-post-merge — done ({} failures)",
        failure_count,
    ));
    Value::Object(result)
}

/// Main-arm dispatch: always returns exit code 0 (best-effort).
pub fn run_impl_main(args: &Args) -> (serde_json::Value, i32) {
    (post_merge(args.pr, &args.state_file, &args.branch), 0)
}
