//! Consolidated Complete phase merge.
//!
//! Absorbs Step 8: freshness check + squash merge.
//!
//! Usage: bin/flow complete-merge --pr <number> --state-file <path>
//!
//! Output (JSON to stdout):
//!   Merged:        {"status": "merged", "pr_number": N}
//!   CI rerun:      {"status": "ci_rerun", "pushed": true, "pr_number": N}
//!   Conflict:      {"status": "conflict", "conflict_files": [...], "pr_number": N}
//!   Not mergeable: {"status": "not_mergeable", "message": "...", "pr_number": N}
//!   Max retry:     {"status": "max_retries", "pr_number": N}
//!   Error:         {"status": "error", "message": "...", "pr_number": N}
//!
//! `complete-merge` does not make its own GitHub-CI determination. When
//! `gh pr merge --squash` refuses the merge (a required GitHub check is
//! failing or still pending), the verbatim gh stderr is surfaced as
//! `status: "not_mergeable"` with a `message` field and `run_impl_main`
//! routes the non-`merged` status to exit 1.
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
const MERGE_STEP: i64 = 4;

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
        .map(|v| crate::resolve_skill_mode::resolve(&v, "flow-complete").1)
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
            // Main had new commits, merged into branch — push, then run
            // sentinel-gated CI on the freshly-merged tree before
            // deferring. The merge changed the tree, so CI re-runs here
            // (or skips on a sentinel match) rather than letting the
            // Step-4 loop defer an untested tree. A pass writes the
            // sentinel and proceeds via `ci_rerun` (the next loop's CI
            // skips on the match and squash-merges); a failure surfaces
            // `ci_failed` with the CI output so Step 4 launches ci-fixer
            // directly.
            match cmd_failure_message(run_cmd_with_timeout(&["git", "push"], NETWORK_TIMEOUT)) {
                Some(msg) => error_result(
                    &format!("Push failed after freshness merge: {}", msg),
                    pr_number,
                ),
                None => {
                    // complete-merge runs in the feature-branch worktree,
                    // so the process cwd is the worktree and resolves the
                    // feature branch for sentinel naming. `unwrap_or` (not
                    // `unwrap_or_else`) keeps the default eager so no
                    // closure branch is introduced — mirrors
                    // `complete_fast::run_impl`.
                    let cwd = std::env::current_dir().unwrap_or(std::path::PathBuf::from("."));
                    let root = crate::git::project_root();
                    let ci_args = crate::ci::Args {
                        force: false,
                        retry: 0,
                        branch: crate::git::resolve_branch_in(None, &cwd, &root),
                        simulate_branch: None,
                        format: false,
                        lint: false,
                        build: false,
                        test: false,
                        audit: false,
                        clean: false,
                        trailing: Vec::new(),
                        reason: Some(
                            "verifying merged tree before Complete squash-merge".to_string(),
                        ),
                    };
                    let (ci_result, ci_code) = crate::ci::run_impl(&ci_args, &cwd, &root, false);
                    if ci_code != 0 {
                        json!({
                            "status": "ci_failed",
                            "output": ci_result,
                            "pushed": true,
                            "pr_number": pr_number,
                        })
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
                    // Case-fold the gh stderr before comparing per
                    // `.claude/rules/security-gates.md` "Normalize Before
                    // Comparing" — the message is external subprocess
                    // output whose casing FLOW does not control.
                    if msg.to_ascii_lowercase().contains("base branch policy") {
                        // `gh pr merge --squash` refused: a required
                        // GitHub check is failing or still pending. Carry
                        // the verbatim gh stderr as `message` and stop —
                        // gh is the authority, not a FLOW guess.
                        json!({"status": "not_mergeable", "message": msg, "pr_number": pr_number})
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
