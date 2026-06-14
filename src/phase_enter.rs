//! Generic phase-enter: gate check + phase_enter() + step counters + return state data.
//!
//! Replaces the per-skill entry ceremony (git worktree list + git branch + Read state +
//! gate check + phase-transition enter + set steps_total) with a single command
//! parameterized by `--phase`.
//!
//! Side effect: after computing `worktree_cwd`, writes the session-keyed
//! phase-anchor marker (`src/phase_anchor.rs`) so a later
//! `--continue-step` resume can recover `worktree_cwd` even after a
//! same-session cwd reset. The write is best-effort and never blocks
//! phase entry.

use std::path::PathBuf;

use clap::Parser;
use serde_json::{json, Value};

use crate::commands::log::append_log;
use crate::flow_paths::FlowPaths;
use crate::git::{project_root, resolve_branch};
use crate::lock::mutate_state;
use crate::phase_config::PHASE_ORDER;
use crate::phase_transition::phase_enter;

#[derive(Parser, Debug)]
#[command(
    name = "phase-enter",
    about = "Generic phase entry: gate + enter + state data"
)]
pub struct Args {
    /// Phase name (e.g. flow-code, flow-review, flow-complete)
    #[arg(long)]
    pub phase: String,

    /// Override branch for state file lookup
    #[arg(long)]
    pub branch: Option<String>,

    /// Number of steps in this phase (sets <phase_short>_steps_total)
    #[arg(long = "steps-total")]
    pub steps_total: Option<i64>,
}

/// Derive the short field prefix from a phase name. Exposed as a
/// public seam so integration tests in `tests/phase_enter.rs` can
/// assert on the prefix-derivation rules directly.
///
/// Strips the `flow-` prefix and replaces `-` with `_`.
/// Example: `flow-review` → `review`
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

    // `branch` here comes from `resolve_branch`, which may return a raw
    // git ref (slash-containing, empty) when a `--branch` override names
    // a non-existent state. Use `try_new` per
    // `.claude/rules/external-input-validation.md` so the CLI surfaces a
    // structured error rather than a Rust panic.
    let paths = match FlowPaths::try_new(&root, &branch) {
        Some(p) => p,
        None => {
            return Err(json!({
                "status": "error",
                "message": format!(
                    "Invalid branch name: '{}' (must be non-empty and contain no '/')",
                    branch
                )
            }));
        }
    };
    let state_path = paths.state_file();
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

/// Testable entry point.
///
/// Returns Ok(json) for both success and application-level errors (status: error).
/// Returns Err(string) only for infrastructure failures.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let (root, branch, state_path) = match resolve_state(args) {
        Ok(v) => v,
        Err(err_json) => return Ok(err_json),
    };

    // Drift guard: phase entry is a state mutation, so it must run
    // from inside the subdirectory the flow was started in. See
    // [`crate::cwd_scope::enforce`].
    let cwd = std::env::current_dir().unwrap_or(std::path::PathBuf::from("."));
    if let Err(msg) = crate::cwd_scope::enforce(&cwd, &root) {
        return Ok(json!({"status": "error", "message": msg}));
    }

    // Read state for gate check and data extraction
    let state_content = std::fs::read_to_string(&state_path)
        .map_err(|e| format!("Could not read state file: {}", e))?;
    let state: Value = serde_json::from_str(&state_content)
        .map_err(|e| format!("Invalid JSON in state file: {}", e))?;

    // Gate: previous phase must be complete
    if let Err(err_json) = gate_check(&state, &args.phase) {
        return Ok(err_json);
    }

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

    // Plan file: read from files.plan.
    let plan_file = state
        .get("files")
        .and_then(|f| f.get("plan"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    // Phase enter via mutate_state
    let enter_result_holder = std::cell::RefCell::new(Value::Null);
    let phase_name = args.phase.clone();

    // `gate_check` above rejects any state that is not an object whose
    // `phases.<prev>.status == "complete"` — by the time we reach the
    // closure, the parsed state is guaranteed to be an object, so no
    // defensive non-object guard is needed here.
    let home = crate::session_metrics::home_dir_or_empty();
    let mutate_result = mutate_state(&state_path, &mut |state| {
        let result = phase_enter(state, &phase_name, None);
        *enter_result_holder.borrow_mut() = result;

        // Capture account-window snapshot AFTER phase_enter has
        // initialized the new phase's PhaseState. `phase_enter`
        // unconditionally creates `phases.<phase_name>` as an
        // object on the same state value, and serde_json's IndexMut
        // auto-vivifies intermediate keys, so the assignment cannot
        // panic from missing scaffolding.
        let snap = crate::per_flow_capture::capture_for_active_state(&home, state, &root);
        state["phases"][&phase_name]["window_at_enter"] =
            serde_json::to_value(&snap).expect("WindowSnapshot must serialize");
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

    // Defense-in-depth: clear any stale shared-config approval
    // markers for this branch on phase advance so a grant issued in
    // an earlier phase cannot bleed forward (the primary guarantee
    // is single-use consumption on the gate-allow path). Best-effort
    // — `clear_all` swallows every IO error and never panics, so a
    // marker-dir anomaly cannot block phase entry.
    crate::shared_config_approval::clear_all(&root, &branch);

    let enter_result = enter_result_holder.into_inner();
    let _ = append_log(
        &root,
        &branch,
        &format!(
            "[Phase] phase-enter --phase {} ({})",
            args.phase, enter_result["status"]
        ),
    );

    // `phase_enter` in `phase_transition.rs` never returns a
    // status="error" payload — it always returns status="ok". The
    // status check below is therefore an unreachable defensive branch
    // and is intentionally omitted to keep the coverage gate clean
    // per `.claude/rules/testability-means-simplicity.md`.

    // Set step counters if --steps-total provided
    if let Some(total) = args.steps_total {
        let prefix = phase_field_prefix(&args.phase);
        let steps_total_field = format!("{}_steps_total", prefix);
        let step_field = format!("{}_step", prefix);

        // Same non-object guard omission rationale as the enter
        // mutation above — gate_check already enforced object state.
        let _ = mutate_state(&state_path, &mut move |state| {
            state[&steps_total_field] = json!(total);
            state[&step_field] = json!(0);
        });
    }

    // Compute worktree path
    let worktree_path = root.join(".worktrees").join(&branch);

    // `relative_cwd` carries the mono-repo subdirectory the flow was
    // started in (empty for root-level flows). `worktree_cwd` joins it
    // onto `worktree_path` so the response carries a copy-pasteable
    // recovery path for sessions that lost cwd through context loss
    // or skill chaining. Consumers: every phase skill that runs
    // `cd "<worktree_cwd>"` after invoking phase-enter.
    //
    // Per `.claude/rules/external-input-path-construction.md`: validate
    // the state-file value before joining. An unsafe `relative_cwd`
    // (containing `..`, an absolute prefix, NUL, or `"`) would let
    // `Path::join` escape the worktree (`..` parents) or replace the
    // base entirely (absolute), and break the `cd "<worktree_cwd>"`
    // shell-bearing instruction. Fail closed: emit a structured
    // error so the user fixes the state file before any cd action.
    let relative_cwd = state
        .get("relative_cwd")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if !FlowPaths::is_safe_relative_cwd(&relative_cwd) {
        return Ok(json!({
            "status": "error",
            "message": format!(
                "Invalid relative_cwd in state file: {:?}. Must be empty or a relative path with no `..` segments, no leading `/`, no NUL bytes, and no `\"` characters.",
                relative_cwd
            ),
        }));
    }
    let worktree_cwd = if relative_cwd.is_empty() {
        worktree_path.clone()
    } else {
        worktree_path.join(&relative_cwd)
    };

    // Write the session-keyed phase-anchor marker so a later
    // `--continue-step` resume can recover `worktree_cwd` even after a
    // same-session cwd reset (`src/phase_anchor.rs` documents the
    // circular-dependency break). Best-effort: `write_anchor_if_resolvable`
    // swallows every error and skips silently when no session_id
    // resolves, so a marker-write problem never blocks phase entry.
    let env_sid = std::env::var("CLAUDE_CODE_SESSION_ID").ok();
    crate::phase_anchor::write_anchor_if_resolvable(
        &home,
        env_sid.as_deref(),
        &branch,
        &worktree_cwd.to_string_lossy(),
        &relative_cwd,
    );

    // Build response with all state data the skill needs
    let mut response = json!({
        "status": "ok",
        "phase": args.phase,
        "project_root": root.to_string_lossy(),
        "branch": branch,
        "worktree_path": worktree_path.to_string_lossy(),
        "relative_cwd": relative_cwd,
        "worktree_cwd": worktree_cwd.to_string_lossy(),
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

    Ok(response)
}
