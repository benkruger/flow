//! `bin/flow complete-fast` — consolidated Complete phase happy path.
//!
//! Absorbs SOFT-GATE + preflight + CI dirty check + GitHub CI check + merge
//! into a single process. Returns a JSON `path` indicator so the skill can
//! branch on the result instead of making 10 separate tool calls.
//!
//! Usage: bin/flow complete-fast [--branch <name>] [--auto] [--manual]
//!
//! Output (JSON to stdout):
//!   Merged:       {"status": "ok", "path": "merged", ...}
//!   Already:      {"status": "ok", "path": "already_merged", ...}
//!   Confirm:      {"status": "ok", "path": "confirm", ...}
//!   CI stale:     {"status": "ok", "path": "ci_stale", ...}
//!   CI failed:    {"status": "ok", "path": "ci_failed", ...}
//!   CI pending:   {"status": "ok", "path": "ci_pending", ...}
//!   Conflict:     {"status": "ok", "path": "conflict", ...}
//!   Max retries:  {"status": "ok", "path": "max_retries", ...}
//!   Error:        {"status": "error", "message": "..."}

use std::path::Path;

use clap::Parser;
use serde_json::{json, Value};

use crate::ci;
use crate::complete_preflight::{
    check_learn_phase, check_pr_status, merge_main, resolve_mode, run_cmd_with_timeout,
};
use crate::git::{project_root, resolve_branch};
use crate::lock::mutate_state;
use crate::phase_transition::phase_enter;
use crate::utils::derive_worktree;

/// Step counter total for complete-fast (reduced from 7 to 5).
const COMPLETE_STEPS_TOTAL: i64 = 5;

#[derive(Parser, Debug)]
#[command(name = "complete-fast", about = "FLOW Complete phase fast path")]
pub struct Args {
    /// Override branch for state file lookup
    #[arg(long)]
    pub branch: Option<String>,
    /// Force auto mode
    #[arg(long)]
    pub auto: bool,
    /// Force manual mode
    #[arg(long)]
    pub manual: bool,
}

/// Read and parse a state file, returning (state_value, state_path).
fn read_state(root: &Path, branch: &str) -> Result<(Value, std::path::PathBuf), String> {
    let state_path = root.join(".flow-states").join(format!("{}.json", branch));
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
    Ok((state, state_path))
}

/// Core complete-fast logic. Returns Ok(json) on success paths (including
/// unhappy paths like ci_failed that the skill handles interactively),
/// Err(string) only for infrastructure failures.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let root = project_root();
    let (resolved, _) = resolve_branch(args.branch.as_deref(), &root);
    let branch = resolved.ok_or("Could not determine current branch")?;

    // Read state file
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

    // Resolve mode
    let mode = resolve_mode(args.auto, args.manual, Some(&state));

    // Collect warnings
    let warnings = check_learn_phase(&state);

    // Phase enter + set step counters
    mutate_state(&state_path, |s| {
        if !(s.is_object() || s.is_null()) {
            return;
        }
        phase_enter(s, "flow-complete", None);
        s["complete_steps_total"] = json!(COMPLETE_STEPS_TOTAL);
        s["complete_step"] = json!(1);
    })
    .map_err(|e| format!("Failed to update state: {}", e))?;

    // Extract PR info from state
    let pr_number = state.get("pr_number").and_then(|v| v.as_i64());
    let pr_url = state
        .get("pr_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let worktree = derive_worktree(&branch);

    // --- PR check ---
    let pr_state = match check_pr_status(pr_number, &branch, &run_cmd_with_timeout) {
        Ok(s) => s,
        Err(e) => {
            return Ok(json!({
                "status": "error",
                "message": e,
                "branch": branch,
            }));
        }
    };

    // Already merged — skip to finalize
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

    // --- Merge main into branch ---
    let (merge_status, merge_data) = merge_main(&run_cmd_with_timeout);
    let tree_changed = merge_status == "merged";

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

    // --- CI dirty check (no simulate-branch) ---
    // If main was merged in (tree changed), the sentinel won't match — return ci_stale
    // so the skill runs CI interactively and loops back.
    if tree_changed {
        return Ok(json!({
            "status": "ok",
            "path": "ci_stale",
            "reason": "main merged into branch — tree changed, CI must re-run",
            "mode": mode,
            "pr_number": pr_number,
            "pr_url": pr_url,
            "branch": branch,
            "worktree": worktree,
            "warnings": warnings,
        }));
    }

    // Compute snapshot WITHOUT --simulate-branch so the Code phase sentinel matches
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let snapshot = ci::tree_snapshot(&cwd, None);
    let sentinel_path = root
        .join(".flow-states")
        .join(format!("{}-ci-passed", branch));

    let ci_skipped = if sentinel_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&sentinel_path) {
            content == snapshot
        } else {
            false
        }
    } else {
        false
    };

    if !ci_skipped {
        // Sentinel doesn't match — run CI locally
        let bin_ci = cwd.join("bin").join("ci");
        let (ci_result, ci_code) =
            ci::run_once(&cwd, &root, &bin_ci, Some(&branch), false, None);

        if ci_code != 0 {
            let ci_output = ci_result
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("bin/ci failed")
                .to_string();
            return Ok(json!({
                "status": "ok",
                "path": "ci_failed",
                "output": ci_output,
                "mode": mode,
                "pr_number": pr_number,
                "pr_url": pr_url,
                "branch": branch,
                "worktree": worktree,
                "warnings": warnings,
            }));
        }
    }

    // TODO: Tasks 5-7 will add GH CI check, freshness + merge, and manual confirm here

    Err("not yet implemented: GH CI and merge steps".to_string())
}

/// CLI entry point.
pub fn run(args: Args) {
    match run_impl(&args) {
        Ok(result) => {
            println!("{}", result);
            if result.get("status").and_then(|v| v.as_str()) == Some("error") {
                std::process::exit(1);
            }
        }
        Err(e) => {
            println!("{}", json!({"status": "error", "message": e}));
            std::process::exit(1);
        }
    }
}
