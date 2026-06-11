//! `bin/flow complete-fast` — consolidated Complete phase happy path.
//!
//! Absorbs SOFT-GATE + preflight + CI dirty check + merge into a single
//! process. Returns a JSON `path` indicator so the skill can branch on
//! the result instead of making 10 separate tool calls.
//!
//! Usage: bin/flow complete-fast [--branch <name>]
//!
//! The autonomy mode (`auto` / `manual`) is resolved from the state
//! file's `skills.flow-complete` block via `resolve_skill_mode` —
//! there are no `--auto` / `--manual` flags.
//!
//! Output (JSON to stdout):
//!   Merged:        {"status": "ok", "path": "merged", ...}
//!   Already:       {"status": "ok", "path": "already_merged", ...}
//!   Confirm:       {"status": "ok", "path": "confirm", ...}
//!   CI stale:      {"status": "ok", "path": "ci_stale", ...}
//!   CI failed:     {"status": "ok", "path": "ci_failed", ...}
//!   Not mergeable: {"status": "ok", "path": "not_mergeable", "reason": "...", ...}
//!   Conflict:      {"status": "ok", "path": "conflict", ...}
//!   Max retries:   {"status": "ok", "path": "max_retries", ...}
//!   Error:         {"status": "error", "message": "..."}
//!
//! `complete-fast` does not make its own determination about GitHub's CI
//! state. The full local CI gate runs at every commit, so the only
//! authority on whether the PR can merge is `gh pr merge --squash`
//! itself. When that command refuses the merge (a required GitHub check
//! is failing or still pending), the verbatim `gh` stderr is surfaced as
//! `path: "not_mergeable"` with a `reason` field, and the skill reports
//! it and stops. `ci_failed` covers only local-CI failure (the `gh pr
//! merge` authority covers the remote side).
//!
//! Tests live in `tests/complete_fast.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` block
//! in this file.

use std::path::{Path, PathBuf};

use clap::Parser;
use serde_json::{json, Value};

use crate::ci;
use crate::complete_preflight::{
    check_learn_phase, check_pr_status, merge_main, resolve_mode, run_cmd_with_timeout,
    COMPLETE_STEPS_TOTAL, NETWORK_TIMEOUT,
};
use crate::flow_paths::FlowPaths;
use crate::git::{project_root, resolve_branch};
use crate::lock::mutate_state;
use crate::phase_transition::phase_enter;
use crate::utils::{bin_flow_path, derive_worktree};

#[derive(Parser, Debug)]
#[command(name = "complete-fast", about = "FLOW Complete phase fast path")]
pub struct Args {
    /// Override branch for state file lookup
    #[arg(long)]
    pub branch: Option<String>,
}

/// Read and parse a state file, returning (state_value, state_path).
fn read_state(root: &Path, branch: &str) -> Result<(Value, PathBuf), String> {
    let state_path = FlowPaths::try_new(root, branch)
        .ok_or_else(|| {
            format!(
                "Branch name '{}' is not a valid FLOW branch (contains '/' or is empty). \
                 FLOW state files use a flat layout that cannot address slash-containing \
                 branches; resume the flow in its canonical branch name.",
                branch
            )
        })?
        .state_file();
    if !state_path.exists() {
        return Err(format!(
            "No state file found for branch '{}'. Run /flow:flow-start first.",
            branch
        ));
    }
    let content = std::fs::read_to_string(&state_path)
        .map_err(|e| format!("Could not read state file: {}", e))?;
    let state: Value = serde_json::from_str(&content)
        .map_err(|_| format!("Could not parse state file: {}", state_path.display()))?;
    if !state.is_object() {
        return Err(format!(
            "Corrupt state file (expected JSON object): {}",
            state_path.display()
        ));
    }
    Ok((state, state_path))
}

/// CI dirty-check decider. Returns `(ci_skipped, ci_failed_output)`:
/// `ci_skipped=true` means the sentinel matches the current tree
/// snapshot (a prior `bin/flow ci` already passed on this tree);
/// `ci_failed_output=Some(msg)` means CI ran and failed.
///
/// The decider runs sentinel-gated CI on every merge point — including
/// the case where the base branch was just merged into the feature
/// branch. A base-branch merge changes the tree, so the snapshot
/// misses the sentinel and CI runs inline; a clean (no-op) merge
/// leaves the tree byte-identical and the sentinel match skips CI.
fn ci_decider(root: &Path, cwd: &Path, branch: &str) -> (bool, Option<String>) {
    let snapshot = ci::tree_snapshot(cwd, None);
    let sentinel = ci::sentinel_path(root, branch);

    let ci_skipped = if sentinel.exists() {
        std::fs::read_to_string(&sentinel)
            .map(|c| c == snapshot)
            .unwrap_or(false)
    } else {
        false
    };

    if ci_skipped {
        return (true, None);
    }

    let ci_args = ci::Args {
        force: false,
        retry: 0,
        branch: Some(branch.to_string()),
        simulate_branch: None,
        format: false,
        lint: false,
        build: false,
        test: false,
        audit: false,
        clean: false,
        trailing: Vec::new(),
        reason: Some("verifying tree is clean before Complete merge".to_string()),
    };
    let (ci_result, ci_code) = ci::run_impl(&ci_args, cwd, root, false);
    if ci_code != 0 {
        let msg = ci_result
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("CI failed")
            .to_string();
        (false, Some(msg))
    } else {
        (false, None)
    }
}

/// Handle the freshness-check + squash-merge + mode-branch dispatch.
///
/// `mode` is the state-resolved `flow-complete` continue-mode:
/// `manual` returns the `confirm` path for the skill to prompt the
/// user; `auto` proceeds to the freshness check and squash-merge.
#[allow(clippy::too_many_arguments)]
fn freshness_and_merge(
    branch: &str,
    state_path: &Path,
    mode: &str,
    pr_number: Option<i64>,
    pr_url: &str,
    worktree: &str,
    warnings: &[String],
    ci_skipped: bool,
    bin_flow: &str,
) -> Value {
    // --- Mode branch: manual returns "confirm", auto proceeds to merge ---
    if mode == "manual" {
        return json!({
            "status": "ok",
            "path": "confirm",
            "mode": mode,
            "pr_number": pr_number,
            "pr_url": pr_url,
            "branch": branch,
            "worktree": worktree,
            "warnings": warnings,
            "ci_skipped": ci_skipped,
        });
    }

    // --- Freshness check + squash merge (auto mode) ---
    let state_file_str = state_path.to_string_lossy().to_string();
    let freshness_result = run_cmd_with_timeout(
        &[bin_flow, "check-freshness", "--state-file", &state_file_str],
        NETWORK_TIMEOUT,
    );

    let (_code, stdout, _stderr) = match freshness_result {
        Err(e) => {
            return json!({
                "status": "error",
                "message": format!("check-freshness failed: {}", e),
                "branch": branch,
            });
        }
        Ok(triple) => triple,
    };

    let freshness: Value = match serde_json::from_str(stdout.trim()) {
        Ok(v) => v,
        Err(_) => {
            return json!({
                "status": "error",
                "message": format!("Invalid JSON from check-freshness: {}", stdout),
                "branch": branch,
            });
        }
    };

    let freshness_status = freshness
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match freshness_status {
        "max_retries" => json!({
            "status": "ok",
            "path": "max_retries",
            "mode": mode,
            "pr_number": pr_number,
            "pr_url": pr_url,
            "branch": branch,
            "worktree": worktree,
            "warnings": warnings,
        }),
        "error" => {
            let msg = freshness
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("check-freshness failed");
            json!({
                "status": "error",
                "message": msg,
                "branch": branch,
            })
        }
        "conflict" => {
            let files = freshness.get("files").cloned().unwrap_or(json!([]));
            json!({
                "status": "ok",
                "path": "conflict",
                "conflict_files": files,
                "mode": mode,
                "pr_number": pr_number,
                "pr_url": pr_url,
                "branch": branch,
                "worktree": worktree,
                "warnings": warnings,
            })
        }
        "merged" => match crate::complete_merge::cmd_failure_message(run_cmd_with_timeout(
            &["git", "push"],
            NETWORK_TIMEOUT,
        )) {
            Some(msg) => json!({
                "status": "error",
                "message": format!("Push failed after freshness merge: {}", msg),
                "branch": branch,
            }),
            None => json!({
                "status": "ok",
                "path": "ci_stale",
                "reason": "main moved during freshness check — pushed, CI must re-run",
                "mode": mode,
                "pr_number": pr_number,
                "pr_url": pr_url,
                "branch": branch,
                "worktree": worktree,
                "warnings": warnings,
            }),
        },
        "up_to_date" => {
            // Reaching here means mode == "auto" (manual returned the
            // `confirm` path above), so the squash-merge proceeds.
            let pr_str = pr_number.unwrap_or(0).to_string();
            match crate::complete_merge::cmd_failure_message(run_cmd_with_timeout(
                &["gh", "pr", "merge", &pr_str, "--squash"],
                NETWORK_TIMEOUT,
            )) {
                None => {
                    let _ = mutate_state(state_path, &mut |s| {
                        s["complete_step"] = json!(5);
                    });
                    json!({
                        "status": "ok",
                        "path": "merged",
                        "mode": mode,
                        "pr_number": pr_number,
                        "pr_url": pr_url,
                        "branch": branch,
                        "worktree": worktree,
                        "warnings": warnings,
                        "ci_skipped": ci_skipped,
                    })
                }
                Some(msg) => {
                    // Case-fold the gh stderr before comparing per
                    // `.claude/rules/security-gates.md` "Normalize Before
                    // Comparing" — the message is external subprocess
                    // output whose casing FLOW does not control.
                    if msg.to_ascii_lowercase().contains("base branch policy") {
                        // `gh pr merge --squash` refused: a required
                        // GitHub check is failing or still pending. The
                        // verbatim gh stderr is the authority — surface
                        // it and stop, rather than guessing pending vs.
                        // failed ourselves.
                        json!({
                            "status": "ok",
                            "path": "not_mergeable",
                            "reason": msg,
                            "mode": mode,
                            "pr_number": pr_number,
                            "pr_url": pr_url,
                            "branch": branch,
                            "worktree": worktree,
                            "warnings": warnings,
                        })
                    } else {
                        json!({
                            "status": "error",
                            "message": msg,
                            "branch": branch,
                        })
                    }
                }
            }
        }
        other => json!({
            "status": "error",
            "message": format!("Unexpected check-freshness status: {}", other),
            "branch": branch,
        }),
    }
}

/// Production CLI entry: runs the full complete-fast sequence.
/// Returns Ok(json) on all path outcomes (including unhappy paths the
/// skill handles interactively), Err(string) only for infrastructure
/// failures that prevent any path determination.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let root = project_root();
    let bin_flow = bin_flow_path();

    let branch = resolve_branch(args.branch.as_deref(), &root)
        .ok_or("Could not determine current branch")?;

    let (state, state_path) = read_state(&root, &branch)?;

    // Gate: Learn phase must be complete
    let learn_status = state
        .get("phases")
        .and_then(|p| p.get("flow-learn"))
        .and_then(|l| l.get("status"))
        .and_then(|s| s.as_str())
        .unwrap_or("pending");
    if learn_status != "complete" {
        return Ok(json!({
            "status": "error",
            "message": format!("Phase 5: Learn must be complete before Complete. Current status: {}", learn_status)
        }));
    }

    // Phase enter + set step counters. read_state validated the file
    // as object already; mutate_state re-reads under lock but cannot
    // observe a non-object here in this single-writer flow.
    //
    // Capture the account-window snapshot inside the same
    // mutate_state closure that calls phase_enter so
    // `format_complete_summary`'s `phase_delta` reads
    // `phases.flow-complete.window_at_enter` when rendering the
    // Complete row. The bare `phase_enter` mutator does not write a
    // snapshot — only the `phase-enter` subcommand wrapper does —
    // so complete-fast's wrapper handles the write itself. The
    // chained IndexMut is safe because `phase_enter` ran first in
    // this closure and heals `state["phases"]` to an object if the
    // on-disk state file held a non-object value.
    let home = crate::session_metrics::home_dir_or_empty();
    mutate_state(&state_path, &mut |s| {
        phase_enter(s, "flow-complete", None);
        s["complete_steps_total"] = json!(COMPLETE_STEPS_TOTAL);
        s["complete_step"] = json!(1);
        let snap = crate::per_flow_capture::capture_for_active_state(&home, s, &root);
        s["phases"]["flow-complete"]["window_at_enter"] =
            serde_json::to_value(&snap).expect("WindowSnapshot must serialize");
    })
    .expect("state file was validated as object by read_state");

    // --- PR check ---
    let pr_state = match check_pr_status(state.get("pr_number").and_then(|v| v.as_i64()), &branch) {
        Ok(s) => s,
        Err(e) => {
            return Ok(json!({
                "status": "error",
                "message": e,
                "branch": branch,
            }));
        }
    };

    let mode = resolve_mode(Some(&state));
    let warnings = check_learn_phase(&state);
    let pr_number = state.get("pr_number").and_then(|v| v.as_i64());
    let pr_url = state
        .get("pr_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let worktree = derive_worktree(&branch);

    if pr_state == "MERGED" {
        return Ok(json!({
            "status": "ok",
            "path": "already_merged",
            "mode": mode,
            "pr_number": pr_number,
            "pr_url": pr_url,
            "branch": branch,
            "worktree": worktree,
            "warnings": warnings,
        }));
    }

    if pr_state == "CLOSED" {
        return Ok(json!({
            "status": "error",
            "message": "PR is closed but not merged. Reopen or create a new PR first.",
            "branch": branch,
        }));
    }

    // --- Merge origin/<base_branch> into branch ---
    // Resolve the integration branch from git (single source of truth).
    // Fail closed via JSON error envelope when git cannot resolve it.
    let base_branch = match crate::git::default_branch_in(&root) {
        Ok(b) => b,
        Err(msg) => {
            return Ok(json!({
                "status": "error",
                "step": "resolve_base_branch",
                "message": msg,
                "branch": branch,
                "worktree": worktree,
            }));
        }
    };
    let (merge_status, merge_data) = merge_main(&base_branch);

    if merge_status == "conflict" {
        return Ok(json!({
            "status": "ok",
            "path": "conflict",
            "conflict_files": merge_data.unwrap_or(json!([])),
            "mode": mode,
            "pr_number": pr_number,
            "pr_url": pr_url,
            "branch": branch,
            "worktree": worktree,
            "warnings": warnings,
        }));
    }

    if merge_status == "error" {
        return Ok(json!({
            "status": "error",
            "message": merge_data.unwrap_or(json!("")),
            "branch": branch,
        }));
    }

    // --- CI dirty check ---
    // Run sentinel-gated CI inline after the base-branch merge. A
    // base-branch merge that changes the tree misses the sentinel and
    // re-runs CI here; a clean no-op merge leaves the sentinel match
    // and skips CI. The pre-merge `ci_stale` deferral is gone — the
    // only remaining `ci_stale` outcome is the freshness-window race
    // in `freshness_and_merge`, where the base branch moves AFTER this
    // gate passes.
    let cwd = std::env::current_dir().unwrap_or(PathBuf::from("."));
    let (ci_skipped, ci_failed_output) = ci_decider(&root, &cwd, &branch);

    if let Some(output) = ci_failed_output {
        return Ok(json!({
            "status": "ok",
            "path": "ci_failed",
            "output": output,
            "mode": mode,
            "pr_number": pr_number,
            "pr_url": pr_url,
            "branch": branch,
            "worktree": worktree,
            "warnings": warnings,
        }));
    }

    Ok(freshness_and_merge(
        &branch,
        &state_path,
        &mode,
        pr_number,
        &pr_url,
        &worktree,
        &warnings,
        ci_skipped,
        &bin_flow,
    ))
}
