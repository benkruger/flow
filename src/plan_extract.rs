//! Plan extraction command — accelerates the Plan phase for pre-decomposed issues.
//!
//! Consolidates the Plan phase ceremony (gate check, phase enter, issue fetch,
//! DAG/plan file creation, state mutations, logging, PR render, phase complete)
//! into a single process. For decomposed issues with an Implementation Plan
//! section, this eliminates ~12-19 model round trips.
//!
//! Three response paths:
//! - `extracted`: decomposed issue with Implementation Plan — phase completed in one call
//! - `standard`: not decomposed or no plan section — model takes over for decompose/explore/write
//! - `resumed`: plan already exists in state — phase completed in one call
//!
//! Error responses use `Ok(json!({"status": "error", ...}))` so the caller
//! receives structured JSON on stdout. Infrastructure errors (file I/O, lock
//! failures) return `Err(String)` for `run()` to wrap in `json_error`.

use std::path::PathBuf;

use clap::Parser;
use serde_json::{json, Value};

use crate::git::{project_root, resolve_branch};
use crate::lock::mutate_state;
use crate::output::json_error;
use crate::phase_config::load_phase_config;
use crate::phase_transition::{phase_complete, phase_enter};

/// Extract and fast-track pre-decomposed plans, or prepare state for model-driven planning.
#[derive(Parser, Debug)]
#[command(name = "plan-extract")]
pub struct Args {
    /// Override branch for state file lookup
    #[arg(long)]
    pub branch: Option<String>,

    /// PR number (read from state file if omitted)
    #[arg(long)]
    pub pr: Option<i64>,
}

pub fn run(args: Args) {
    match run_impl(&args) {
        Ok(result) => {
            println!("{}", result);
        }
        Err(e) => {
            json_error(&e, &[]);
            std::process::exit(1);
        }
    }
}

/// Resolve the project root and state file path for the given branch.
///
/// Returns (root, branch_name, state_path). Returns Err if branch cannot
/// be resolved or state file does not exist.
fn resolve_state(args: &Args) -> Result<(PathBuf, String, PathBuf), Value> {
    let root = project_root();
    let (branch, candidates) = resolve_branch(args.branch.as_deref(), &root);

    let branch = match branch {
        Some(b) => b,
        None => {
            let msg = if !candidates.is_empty() {
                "Multiple active features. Pass --branch."
            } else {
                "Could not determine current branch"
            };
            return Err(json!({"status": "error", "message": msg}));
        }
    };

    let state_path = root.join(".flow-states").join(format!("{}.json", branch));
    if !state_path.exists() {
        return Err(json!({
            "status": "error",
            "message": format!("No state file found: {}", state_path.display())
        }));
    }

    Ok((root, branch, state_path))
}

/// Check that flow-start is complete. Returns error JSON if not.
fn gate_check(state: &Value) -> Result<(), Value> {
    let start_status = state
        .get("phases")
        .and_then(|p| p.get("flow-start"))
        .and_then(|s| s.get("status"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if start_status != "complete" {
        return Err(json!({
            "status": "error",
            "message": "Phase 1: Start must be complete. Run /flow:flow-start first."
        }));
    }
    Ok(())
}

/// Load frozen phase config if available, for phase_complete.
fn load_frozen_config(root: &PathBuf, branch: &str) -> (Option<Vec<String>>, Option<indexmap::IndexMap<String, String>>) {
    let frozen_path = root
        .join(".flow-states")
        .join(format!("{}-phases.json", branch));
    let frozen_config = if frozen_path.exists() {
        load_phase_config(&frozen_path).ok()
    } else {
        None
    };
    let frozen_order = frozen_config.as_ref().map(|c| c.order.clone());
    let frozen_commands = frozen_config.as_ref().map(|c| c.commands.clone());
    (frozen_order, frozen_commands)
}

/// Run phase_complete via mutate_state and return the result JSON.
fn complete_plan_phase(state_path: &PathBuf, root: &PathBuf, branch: &str) -> Result<Value, String> {
    let (frozen_order, frozen_commands) = load_frozen_config(root, branch);
    let result_holder = std::cell::RefCell::new(Value::Null);

    mutate_state(state_path, |state| {
        let result = phase_complete(
            state,
            "flow-plan",
            None,
            frozen_order.as_deref(),
            frozen_commands.as_ref(),
        );
        *result_holder.borrow_mut() = result;
    })
    .map_err(|e| format!("Failed to complete phase: {}", e))?;

    Ok(result_holder.into_inner())
}

/// Fallible entry point for plan extraction.
///
/// Returns structured JSON as `Ok(Value)` for all business responses
/// (including status-error responses like gate failures). Returns
/// `Err(String)` only for infrastructure failures (file I/O, lock errors).
pub fn run_impl(args: &Args) -> Result<Value, String> {
    // --- Resolve state file ---
    let (root, branch, state_path) = resolve_state(args).map_err(|v| v.to_string())?;

    // --- Read state for gate and resume checks ---
    let state_content = std::fs::read_to_string(&state_path)
        .map_err(|e| format!("Could not read state file: {}", e))?;
    let state: Value = serde_json::from_str(&state_content)
        .map_err(|e| format!("Invalid JSON in state file: {}", e))?;

    // --- Gate: flow-start must be complete ---
    if let Err(err_json) = gate_check(&state) {
        return Ok(err_json);
    }

    // --- Resume check ---
    let plan_path = state
        .get("files")
        .and_then(|f| f.get("plan"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    let dag_path = state
        .get("files")
        .and_then(|f| f.get("dag"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    // Resume: plan already exists → enter phase, complete immediately, return "resumed"
    if let Some(plan_rel) = plan_path {
        let plan_abs = root.join(plan_rel);

        // Enter the phase (idempotent if already in_progress)
        mutate_state(&state_path, |state| {
            phase_enter(state, "flow-plan", None);
        })
        .map_err(|e| format!("Failed to enter phase: {}", e))?;

        // Complete the phase
        let complete_result = complete_plan_phase(&state_path, &root, &branch)?;

        let plan_content = std::fs::read_to_string(&plan_abs)
            .map_err(|e| format!("Could not read plan file: {}", e))?;

        let formatted_time = complete_result
            .get("formatted_time")
            .and_then(|v| v.as_str())
            .unwrap_or("< 1m")
            .to_string();
        let continue_action = complete_result
            .get("continue_action")
            .and_then(|v| v.as_str())
            .unwrap_or("ask")
            .to_string();

        return Ok(json!({
            "status": "ok",
            "path": "resumed",
            "plan_content": plan_content,
            "plan_file": plan_rel,
            "formatted_time": formatted_time,
            "continue_action": continue_action,
        }));
    }

    // Resume: DAG exists but no plan → enter phase, skip to extraction (Task 4 logic)
    // For now, fall through to the standard enter-phase path.
    // Extraction logic will be added in Task 4.

    // --- Phase enter ---
    mutate_state(&state_path, |state| {
        phase_enter(state, "flow-plan", None);
        // Set step tracking for TUI
        state["plan_steps_total"] = json!(4);
        state["plan_step"] = json!(1);
    })
    .map_err(|e| format!("Failed to enter phase: {}", e))?;

    // TODO: Tasks 3-5 continue from here (issue fetch, extraction, completion)
    // For now, return standard path so the model takes over
    let pr = args
        .pr
        .or_else(|| state.get("pr_number").and_then(|v| v.as_i64()));

    Ok(json!({
        "status": "ok",
        "path": "standard",
        "issue_body": null,
        "issue_number": null,
        "dag_mode": "auto",
    }))
}
