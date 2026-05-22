//! Consolidated Complete phase merge.
//!
//! Absorbs Step 8: freshness check + squash merge.
//!
//! Usage: bin/flow complete-merge --pr <number> --state-file <path>
//!
//! Output (JSON to stdout):
//!   Merged:     {"status": "merged", "pr_number": N}
//!   CI rerun:   {"status": "ci_rerun", "pushed": true, "pr_number": N}
//!   Conflict:   {"status": "conflict", "conflict_files": [...], "pr_number": N}
//!   CI pending: {"status": "ci_pending", "pr_number": N}
//!   Max retry:  {"status": "max_retries", "pr_number": N}
//!   Error:      {"status": "error", "message": "...", "pr_number": N}
//!
//! Tests live in `tests/complete_merge.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` block
//! in this file.

use std::path::Path;

use clap::Parser;
use serde_json::{json, Value};

use crate::complete_preflight::{run_cmd_with_timeout, NETWORK_TIMEOUT};
use crate::lock::mutate_state;
use crate::utils::bin_flow_path;
const MERGE_STEP: i64 = 5;

/// Resolve the configured `flow-complete` autonomy mode from the
/// state file. Fails closed to `manual` when the file is missing,
/// unreadable, or non-JSON — a degraded state file must not silently
/// disable the merge-confirmation gate
/// (`.claude/rules/security-gates.md` "Fail Closed When State Is
/// Unreliable"). A non-object JSON root is handled inside
/// `resolve_skill_mode::resolve`, which also returns `manual` for it.
fn merge_mode(state_file: &str) -> String {
    std::fs::read_to_string(state_file)
        .ok()
        .and_then(|c| serde_json::from_str::<Value>(&c).ok())
        .map(|v| crate::resolve_skill_mode::resolve(&v, "flow-complete"))
        .unwrap_or_else(|| crate::resolve_skill_mode::FALLBACK_MODE.to_string())
}

#[derive(Parser, Debug)]
#[command(name = "complete-merge", about = "FLOW Complete phase merge")]
pub struct Args {
    /// PR number to merge
    #[arg(long, required = true)]
    pub pr: i64,
    /// Path to state file
    #[arg(long = "state-file", required = true)]
    pub state_file: String,
}

/// Build an error result with pr_number.
fn error_result(message: &str, pr_number: i64) -> Value {
    json!({
        "status": "error",
        "message": message,
        "pr_number": pr_number,
    })
}

/// Collapse a `CmdResult` into `None` on success (exit 0) or
/// `Some(message)` on spawn failure or non-zero exit. Tests drive
/// this directly with constructed `Ok`/`Err` values so the spawn-Err
/// branch of the call sites is reachable without an unusable-binary
/// fixture.
pub fn cmd_failure_message(result: crate::complete_preflight::CmdResult) -> Option<String> {
    match result {
        Err(e) => Some(e),
        Ok((0, _, _)) => None,
        Ok((_, _, stderr)) => Some(stderr.trim().to_string()),
    }
}

/// Production logic for complete-merge. Runs check-freshness then
/// dispatches to squash merge or push-after-merge per the freshness
/// result.
fn complete_merge(pr_number: i64, state_file: &str) -> Value {
    let bin_flow = bin_flow_path();

    // Set step counter if state file exists
    let state_path = Path::new(state_file);
    if state_path.exists() {
        let _ = mutate_state(state_path, &mut |s| {
            if !(s.is_object() || s.is_null()) {
                return;
            }
            s["complete_step"] = json!(MERGE_STEP);
        });
    }

    // Merge-approval gate: a manual-configured flow-complete must not
    // squash-merge without an explicit confirmation marker. The marker
    // is consumed here — before the freshness check — so EVERY merge
    // attempt consumes it: a freshness outcome that returns before the
    // squash (`ci_rerun`, `conflict`, `error`) still consumes the
    // marker, so a loop-back through Step 4 requires a fresh
    // confirmation rather than reusing a stale one. The marker sits in
    // the per-branch state directory alongside the state file;
    // `state_file` has no parent only for a filesystem-root path,
    // which folds into the no-marker (refuse) outcome.
    if merge_mode(state_file) == "manual" {
        let approved = state_path
            .parent()
            .map(crate::merge_approval::check_and_consume_approval)
            .unwrap_or(false);
        if !approved {
            return json!({
                "status": "error",
                "reason": "merge_not_confirmed",
                "message": "flow-complete is configured manual; the squash-merge requires a confirmation marker written by `bin/flow confirm-merge` after the user confirms.",
                "pr_number": pr_number,
            });
        }
    }

    let freshness_result = run_cmd_with_timeout(
        &[&bin_flow, "check-freshness", "--state-file", state_file],
        NETWORK_TIMEOUT,
    );

    let (_code, stdout, _stderr) = match freshness_result {
        Err(e) => {
            return error_result(&e, pr_number);
        }
        Ok(triple) => triple,
    };

    let freshness: Value = match serde_json::from_str(stdout.trim()) {
        Ok(v) => v,
        Err(_) => {
            return error_result(
                &format!("Invalid JSON from check-freshness: {}", stdout),
                pr_number,
            );
        }
    };

    let freshness_status = freshness
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match freshness_status {
        "max_retries" => json!({"status": "max_retries", "pr_number": pr_number}),
        "error" => {
            let msg = freshness
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("check-freshness failed");
            error_result(msg, pr_number)
        }
        "conflict" => {
            let files = freshness.get("files").cloned().unwrap_or(json!([]));
            json!({
                "status": "conflict",
                "conflict_files": files,
                "pr_number": pr_number,
            })
        }
        "merged" => {
            // Main had new commits, merged into branch — push
            match cmd_failure_message(run_cmd_with_timeout(&["git", "push"], NETWORK_TIMEOUT)) {
                Some(msg) => error_result(
                    &format!("Push failed after freshness merge: {}", msg),
                    pr_number,
                ),
                None => json!({
                    "status": "ci_rerun",
                    "pushed": true,
                    "pr_number": pr_number,
                }),
            }
        }
        "up_to_date" => {
            // The merge-approval gate already ran before the freshness
            // check, so a manual-configured flow reaching here is
            // confirmed. Proceed to squash merge.
            let pr_str = pr_number.to_string();
            match cmd_failure_message(run_cmd_with_timeout(
                &["gh", "pr", "merge", &pr_str, "--squash"],
                NETWORK_TIMEOUT,
            )) {
                None => json!({"status": "merged", "pr_number": pr_number}),
                Some(msg) => {
                    if msg.contains("base branch policy") {
                        json!({"status": "ci_pending", "pr_number": pr_number})
                    } else {
                        error_result(&msg, pr_number)
                    }
                }
            }
        }
        other => error_result(
            &format!("Unexpected check-freshness status: {}", other),
            pr_number,
        ),
    }
}

/// Main-arm dispatch: runs complete_merge and returns (value, exit code).
pub fn run_impl_main(args: &Args) -> (Value, i32) {
    let result = complete_merge(args.pr, &args.state_file);
    let code = if result["status"] == "merged" { 0 } else { 1 };
    (result, code)
}
