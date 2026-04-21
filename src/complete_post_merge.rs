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
use crate::complete_preflight::{run_cmd_with_timeout, CmdResult, LOCAL_TIMEOUT, NETWORK_TIMEOUT};
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

/// Core post-merge logic with injectable runner. Best-effort throughout.
pub fn post_merge_inner(
    pr_number: i64,
    state_file: &str,
    branch: &str,
    root: &Path,
    bin_flow: &str,
    runner: &dyn Fn(&[&str], u64) -> CmdResult,
) -> Value {
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
    // panicking: post-merge's artifact paths
    // (`.flow-states/<branch>-issues.json` etc.) cannot address a
    // slash-containing branch in the flat `.flow-states/` layout.
    let paths = match FlowPaths::try_new(root, branch) {
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
            let _ = append_log(root, branch, msg);
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

    // Treat both `null` and the empty string `""` as "no repo set" so
    // downstream issue-closing and Slack steps short-circuit cleanly
    // when the state file lacks a repo.
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
        bin_flow,
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
    match runner(&pt_args, NETWORK_TIMEOUT) {
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
                    // Prefer the structured parse error when present;
                    // fall back to the raw stderr text from the
                    // subprocess so the failure surfaces something
                    // human-readable in either case.
                    let msg = parse_err.unwrap_or_else(|| stderr.trim().to_string());
                    log("[Phase 6] complete-post-merge — phase-transition (failed)");
                    failures.insert("phase_transition".to_string(), json!(msg));
                }
            }
        }
    }

    // Render PR body — pass state_file explicitly because render-pr-body's
    // auto-detection uses current_branch(), which returns "main" when the
    // Complete skill runs from the project root after merge.
    let pr_str = pr_number.to_string();
    let render_args = [
        bin_flow,
        "render-pr-body",
        "--pr",
        &pr_str,
        "--state-file",
        state_file,
    ];
    match runner(&render_args, NETWORK_TIMEOUT) {
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
        bin_flow,
        "format-issues-summary",
        "--state-file",
        state_file,
        "--output",
        &issues_output,
    ];
    if let Ok((_code, stdout, _stderr)) = runner(&iss_args, LOCAL_TIMEOUT) {
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
    // Transport errors on format-issues-summary are silently ignored:
    // the issues banner is decorative, and post-merge should not fail
    // because the formatter subprocess returned a non-zero status.

    // --- Step 9: Close referenced issues ---

    let close_args = [bin_flow, "close-issues", "--state-file", state_file];
    let mut closed_issues: Vec<Value> = Vec::new();
    if let Ok((_code, stdout, _stderr)) = runner(&close_args, NETWORK_TIMEOUT) {
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

    // Write closed-issues file if non-empty. Vec<Value> always
    // serializes cleanly — `expect` covers an unreachable branch
    // per `.claude/rules/testability-means-simplicity.md`.
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
        bin_flow,
        "format-complete-summary",
        "--state-file",
        state_file,
    ];
    if !closed_issues.is_empty() {
        sum_args.push("--closed-issues-file");
        sum_args.push(&closed_file_path);
    }
    if let Ok((_code, stdout, _stderr)) = runner(&sum_args, LOCAL_TIMEOUT) {
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
    // Transport errors on format-complete-summary are silently ignored

    // Remove In-Progress labels
    let label_args = [
        bin_flow,
        "label-issues",
        "--state-file",
        state_file,
        "--remove",
    ];
    match runner(&label_args, NETWORK_TIMEOUT) {
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
                    bin_flow,
                    "auto-close-parent",
                    "--repo",
                    repo_str.as_str(),
                    "--issue-number",
                    &issue_num_str,
                ];
                if let Ok((_code, stdout, _stderr)) = runner(&acp_args, NETWORK_TIMEOUT) {
                    let (parsed, _) = parse_json_or(&stdout);
                    if let Some(acp_data) = parsed {
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
    }
    result.insert("parents_closed".to_string(), json!(parents_closed));

    // Slack notification — only post if a non-empty thread_ts is set;
    // both `null` and the empty string `""` mean "no Slack thread to
    // reply to" and skip the notification entirely.
    let slack_thread_ts: Option<String> = state
        .get("slack_thread_ts")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    if let Some(ref thread_ts) = slack_thread_ts {
        let msg = format!("Phase 6: Complete finished for PR #{}", pr_number);
        let slack_args = [
            bin_flow,
            "notify-slack",
            "--phase",
            "flow-complete",
            "--message",
            &msg,
            "--thread-ts",
            thread_ts.as_str(),
        ];
        match runner(&slack_args, NETWORK_TIMEOUT) {
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
                        // Record notification if successful
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
                                    bin_flow,
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
                                let _ = runner(&add_args, LOCAL_TIMEOUT);
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

/// Production wrapper.
pub fn post_merge(pr_number: i64, state_file: &str, branch: &str) -> Value {
    let root = project_root();
    post_merge_inner(
        pr_number,
        state_file,
        branch,
        &root,
        &bin_flow_path(),
        &run_cmd_with_timeout,
    )
}

/// CLI entry point. Always exits 0 — post-merge is best-effort and any
/// downstream failures (Slack, label cleanup, parent issue close) are
/// surfaced inside the JSON `failures` map rather than via the exit
/// code, so the calling skill can continue cleaning up.
/// Main-arm dispatch: always returns exit code 0 (best-effort).
pub fn run_impl_main(args: &Args) -> (serde_json::Value, i32) {
    (post_merge(args.pr, &args.state_file, &args.branch), 0)
}
