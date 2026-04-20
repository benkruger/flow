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

use std::path::Path;

use clap::Parser;
use serde_json::{json, Value};

use crate::complete_preflight::{run_cmd_with_timeout, CmdResult, NETWORK_TIMEOUT};
use crate::lock::mutate_state;
use crate::utils::bin_flow_path;
const MERGE_STEP: i64 = 5;

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

/// Core complete-merge logic with injectable runner.
pub fn complete_merge_inner(
    pr_number: i64,
    state_file: &str,
    bin_flow: &str,
    runner: &dyn Fn(&[&str], u64) -> CmdResult,
) -> Value {
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

    // Run check-freshness
    let freshness_result = runner(
        &[bin_flow, "check-freshness", "--state-file", state_file],
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
            match runner(&["git", "push"], NETWORK_TIMEOUT) {
                Err(e) => error_result(
                    &format!("Push failed after freshness merge: {}", e),
                    pr_number,
                ),
                Ok((code, _, stderr)) => {
                    if code != 0 {
                        error_result(
                            &format!("Push failed after freshness merge: {}", stderr.trim()),
                            pr_number,
                        )
                    } else {
                        json!({
                            "status": "ci_rerun",
                            "pushed": true,
                            "pr_number": pr_number,
                        })
                    }
                }
            }
        }
        "up_to_date" => {
            // Proceed to squash merge
            let pr_str = pr_number.to_string();
            match runner(&["gh", "pr", "merge", &pr_str, "--squash"], NETWORK_TIMEOUT) {
                Err(e) => error_result(&e, pr_number),
                Ok((code, _, stderr)) => {
                    if code == 0 {
                        json!({"status": "merged", "pr_number": pr_number})
                    } else {
                        let stderr_trim = stderr.trim();
                        if stderr_trim.contains("base branch policy") {
                            json!({"status": "ci_pending", "pr_number": pr_number})
                        } else {
                            error_result(stderr_trim, pr_number)
                        }
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

/// Main-arm dispatch with injectable runner — tests drive the exit
/// code → status mapping by supplying a mock runner; production passes
/// the real `bin/flow` path and `run_cmd_with_timeout`.
pub fn run_impl_main_with_runner(
    args: &Args,
    bin_flow: &str,
    runner: &dyn Fn(&[&str], u64) -> CmdResult,
) -> (Value, i32) {
    let result = complete_merge_inner(args.pr, &args.state_file, bin_flow, runner);
    let code = if result["status"] == "merged" { 0 } else { 1 };
    (result, code)
}

/// Main-arm dispatch: runs complete_merge and returns (value, exit code).
pub fn run_impl_main(args: &Args) -> (Value, i32) {
    run_impl_main_with_runner(args, &bin_flow_path(), &run_cmd_with_timeout)
}
