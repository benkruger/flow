//! Generic phase-enter: gate check + phase_enter() + step counters + return state data.
//!
//! Replaces the per-skill entry ceremony (git worktree list + git branch + Read state +
//! gate check + phase-transition enter + set steps_total) with a single command
//! parameterized by `--phase`.

use std::path::PathBuf;
use std::process;

use clap::Parser;
use serde_json::{json, Value};

use crate::commands::log::append_log;
use crate::git::{project_root, resolve_branch};
use crate::lock::mutate_state;
use crate::output::json_error;
use crate::phase_config::PHASE_ORDER;
use crate::phase_transition::phase_enter;

#[derive(Parser, Debug)]
#[command(
    name = "phase-enter",
    about = "Generic phase entry: gate + enter + state data"
)]
pub struct Args {
    /// Phase name (e.g. flow-code, flow-code-review, flow-learn)
    #[arg(long)]
    pub phase: String,

    /// Override branch for state file lookup
    #[arg(long)]
    pub branch: Option<String>,

    /// Number of steps in this phase (sets <phase_short>_steps_total)
    #[arg(long = "steps-total")]
    pub steps_total: Option<i64>,
}

/// Derive the short field prefix from a phase name.
///
/// Strips the `flow-` prefix and replaces `-` with `_`.
/// Example: `flow-code-review` → `code_review`
fn phase_field_prefix(phase: &str) -> String {
    phase
        .strip_prefix("flow-")
        .unwrap_or(phase)
        .replace('-', "_")
}

/// Resolve state file location from args.
fn resolve_state(args: &Args) -> Result<(PathBuf, String, PathBuf), Value> {
    let root = project_root();
    let branch = match resolve_branch(args.branch.as_deref(), &root) {
        Some(b) => b,
        None => {
            return Err(json!({
                "status": "error",
                "message": "Could not determine current branch"
            }));
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

/// Check that the previous phase in PHASE_ORDER is complete.
fn gate_check(state: &Value, phase: &str) -> Result<(), Value> {
    let idx = PHASE_ORDER.iter().position(|&p| p == phase);
    let prev_phase = match idx {
        Some(i) if i > 0 => PHASE_ORDER[i - 1],
        _ => {
            return Err(json!({
                "status": "error",
                "message": format!("Phase '{}' not found in phase order or has no predecessor", phase)
            }));
        }
    };

    let prev_status = state
        .get("phases")
        .and_then(|p| p.get(prev_phase))
        .and_then(|s| s.get("status"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if prev_status != "complete" {
        return Err(json!({
            "status": "error",
            "message": format!(
                "Phase '{}' must be complete before entering '{}'. Current status: '{}'",
                prev_phase, phase, prev_status
            )
        }));
    }

    Ok(())
}

/// Read mode config from state file's skills section.
///
/// Returns (commit_mode, continue_mode) as strings.
fn resolve_mode(state: &Value, phase: &str) -> (String, String) {
    // Per-phase defaults when no skills config exists
    let (default_commit, default_continue) = match phase {
        "flow-learn" => ("auto", "auto"),
        _ => ("manual", "manual"),
    };

    let skill_config = state.get("skills").and_then(|s| s.get(phase));

    match skill_config {
        Some(cfg) if cfg.is_object() => {
            let commit = cfg
                .get("commit")
                .and_then(|v| v.as_str())
                .unwrap_or(default_commit)
                .to_string();
            let cont = cfg
                .get("continue")
                .and_then(|v| v.as_str())
                .unwrap_or(default_continue)
                .to_string();
            (commit, cont)
        }
        // Simple string config (e.g. "flow-abort": "auto") — applies to both axes
        Some(cfg) if cfg.is_string() => {
            let mode = cfg.as_str().unwrap_or(default_commit).to_string();
            (mode.clone(), mode)
        }
        _ => (default_commit.to_string(), default_continue.to_string()),
    }
}

/// Testable entry point.
///
/// Returns Ok(json) for both success and application-level errors (status: error).
/// Returns Err(string) only for infrastructure failures.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let (root, branch, state_path) = match resolve_state(args) {
        Ok(v) => v,
        Err(err_json) => return Ok(err_json),
    };

    // Read state for gate check and data extraction
    let state_content = std::fs::read_to_string(&state_path)
        .map_err(|e| format!("Could not read state file: {}", e))?;
    let state: Value = serde_json::from_str(&state_content)
        .map_err(|e| format!("Invalid JSON in state file: {}", e))?;

    // Gate: previous phase must be complete
    if let Err(err_json) = gate_check(&state, &args.phase) {
        return Ok(err_json);
    }

    // Resolve mode from state skills config
    let (commit_mode, continue_mode) = resolve_mode(&state, &args.phase);

    // Extract state data before mutation (these don't change during enter)
    let pr_number = state.get("pr_number").and_then(|v| v.as_i64());
    let pr_url = state
        .get("pr_url")
        .and_then(|v| v.as_str())
        .map(String::from);
    let feature = state
        .get("feature")
        .and_then(|v| v.as_str())
        .map(String::from);
    let slack_thread_ts = state
        .get("slack_thread_ts")
        .and_then(|v| v.as_str())
        .map(String::from);

    let framework = state
        .get("framework")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    // Plan file: check files.plan first, fall back to plan_file
    let plan_file = state
        .get("files")
        .and_then(|f| f.get("plan"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| state.get("plan_file").and_then(|v| v.as_str()))
        .map(String::from);

    // Phase enter via mutate_state
    let enter_result_holder = std::cell::RefCell::new(Value::Null);
    let phase_name = args.phase.clone();

    let mutate_result = mutate_state(&state_path, |state| {
        if !(state.is_object() || state.is_null()) {
            return;
        }
        let result = phase_enter(state, &phase_name, None);
        *enter_result_holder.borrow_mut() = result;
    });

    match mutate_result {
        Ok(_) => {}
        Err(e) => {
            return Ok(json!({
                "status": "error",
                "message": format!("State mutation failed: {}", e),
            }));
        }
    }

    let enter_result = enter_result_holder.into_inner();
    let _ = append_log(
        &root,
        &branch,
        &format!(
            "[Phase] phase-enter --phase {} ({})",
            args.phase, enter_result["status"]
        ),
    );

    if enter_result["status"] == "error" {
        return Ok(json!({
            "status": "error",
            "message": enter_result["message"],
        }));
    }

    // Set step counters if --steps-total provided
    if let Some(total) = args.steps_total {
        let prefix = phase_field_prefix(&args.phase);
        let steps_total_field = format!("{}_steps_total", prefix);
        let step_field = format!("{}_step", prefix);

        let _ = mutate_state(&state_path, move |state| {
            if !(state.is_object() || state.is_null()) {
                return;
            }
            state[&steps_total_field] = json!(total);
            state[&step_field] = json!(0);
        });
    }

    // Compute worktree path
    let worktree_path = root.join(".worktrees").join(&branch);

    // Build response with all state data the skill needs
    let mut response = json!({
        "status": "ok",
        "phase": args.phase,
        "project_root": root.to_string_lossy(),
        "branch": branch,
        "worktree_path": worktree_path.to_string_lossy(),
        "mode": {
            "commit": commit_mode,
            "continue": continue_mode,
        },
    });

    // Add optional fields
    if let Some(pr) = pr_number {
        response["pr_number"] = json!(pr);
    }
    if let Some(ref url) = pr_url {
        response["pr_url"] = json!(url);
    }
    if let Some(ref f) = feature {
        response["feature"] = json!(f);
    }
    if let Some(ref ts) = slack_thread_ts {
        response["slack_thread_ts"] = json!(ts);
    }
    if let Some(ref pf) = plan_file {
        response["plan_file"] = json!(pf);
    }
    if let Some(ref fw) = framework {
        response["framework"] = json!(fw);
    }

    Ok(response)
}

/// CLI entry point.
pub fn run(args: Args) {
    match run_impl(&args) {
        Ok(result) => {
            println!("{}", serde_json::to_string(&result).unwrap());
        }
        Err(e) => {
            json_error(&e, &[]);
            process::exit(1);
        }
    }
}
