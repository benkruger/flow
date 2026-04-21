//! Phase state transitions and `bin/flow phase-transition` CLI driver.
//!
//! `phase_enter()` and `phase_complete()` are the pure mutators that
//! apply state changes inside a `mutate_state` lock. `run_impl_main()`
//! is the thin CLI driver that loads the state file, calls the
//! mutator, writes the `[Phase N] phase-transition --action X --phase Y
//! ("status")` log entry on both success and mutation-failure paths,
//! and returns the JSON result for `dispatch::dispatch_json`. The
//! `main.rs` `Commands::PhaseTransition` arm delegates here and prints
//! the returned Value via `dispatch::dispatch_json`.
//!
//! Tests live in `tests/phase_transition.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` block
//! in this file.

use std::path::Path;
use std::process::Command;

use indexmap::IndexMap;
use serde_json::{json, Value};

use crate::commands::log::append_log;
use crate::cwd_scope;
use crate::flow_paths::FlowPaths;
use crate::git::resolve_branch;
use crate::lock::mutate_state;
use crate::output::json_error_string;
use crate::phase_config::{self, load_phase_config, phase_number, PHASE_ORDER};
use crate::utils::{elapsed_since, format_time, now, tolerant_i64};

/// Apply phase entry mutations to the state Value in-place.
///
/// Returns the result JSON to print to stdout.
pub fn phase_enter(state: &mut Value, phase: &str, reason: Option<&str>) -> Value {
    let prev_phase = state
        .get("current_phase")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Guard: reset "phases" to an empty object if it is not an object or null.
    // IndexMut panics on string key access to arrays, strings, bools, and numbers.
    if let Some(phases) = state.get("phases") {
        if !phases.is_object() && !phases.is_null() {
            state["phases"] = json!({});
        }
    }

    let phase_data = &mut state["phases"][phase];

    phase_data["status"] = json!("in_progress");
    if phase_data["started_at"].is_null() {
        phase_data["started_at"] = json!(now());
    }
    phase_data["session_started_at"] = json!(now());

    let visit_count = tolerant_i64(&phase_data["visit_count"]).saturating_add(1);
    phase_data["visit_count"] = json!(visit_count);

    state["current_phase"] = json!(phase);

    // Record phase transition
    let mut transition = json!({
        "from": prev_phase,
        "to": phase,
        "timestamp": now(),
    });
    if let Some(r) = reason {
        transition["reason"] = json!(r);
    }

    if !state.get("phase_transitions").is_some_and(|v| v.is_array()) {
        state["phase_transitions"] = json!([]);
    }
    state["phase_transitions"]
        .as_array_mut()
        .unwrap()
        .push(transition);

    // Clear auto-continue, discussion-mode, and stale continuation flags
    // from previous phase. State is guaranteed to be an object here —
    // earlier `state["phases"][phase]` IndexMut accesses would have
    // panicked on any non-object input. `.expect` covers the truly-
    // unreachable None arm per `.claude/rules/testability-means-simplicity.md`.
    let obj = state
        .as_object_mut()
        .expect("phase_enter requires object state");
    obj.remove("_auto_continue");
    obj.remove("_stop_instructed");
    obj.remove("_continue_pending");
    obj.remove("_continue_context");

    let first_visit = visit_count == 1;

    json!({
        "status": "ok",
        "phase": phase,
        "action": "enter",
        "visit_count": visit_count,
        "first_visit": first_visit,
    })
}

/// Apply phase completion mutations to the state Value in-place.
///
/// Returns the result JSON to print to stdout.
pub fn phase_complete(
    state: &mut Value,
    phase: &str,
    next_phase: Option<&str>,
    phase_order: Option<&[String]>,
    phase_commands: Option<&IndexMap<String, String>>,
) -> Value {
    let default_order: Vec<String> = PHASE_ORDER.iter().map(|&s| s.to_string()).collect();
    let default_commands = phase_config::commands();

    let order = phase_order.unwrap_or(&default_order);
    let commands = phase_commands.unwrap_or(&default_commands);

    // Determine next phase
    let next = match next_phase {
        Some(np) => np.to_string(),
        None => {
            let phase_idx = order.iter().position(|p| p == phase).unwrap_or(0);
            if phase_idx + 1 < order.len() {
                order[phase_idx + 1].clone()
            } else {
                phase.to_string() // terminal phase points to itself
            }
        }
    };

    // Guard: reset "phases" to an empty object if it is not an object or null.
    // Mirrors the same guard in phase_enter — both functions access
    // state["phases"][phase] via IndexMut, which panics on non-object types.
    if let Some(phases) = state.get("phases") {
        if !phases.is_object() && !phases.is_null() {
            state["phases"] = json!({});
        }
    }

    // Compute elapsed time
    let session_started = state["phases"][phase]["session_started_at"]
        .as_str()
        .map(String::from);
    let elapsed = elapsed_since(session_started.as_deref(), None);

    let existing = tolerant_i64(&state["phases"][phase]["cumulative_seconds"]);
    let cumulative = existing.saturating_add(elapsed);

    // Update phase state
    state["phases"][phase]["cumulative_seconds"] = json!(cumulative);
    state["phases"][phase]["status"] = json!("complete");
    state["phases"][phase]["completed_at"] = json!(now());
    state["phases"][phase]["session_started_at"] = json!(null);
    state["current_phase"] = json!(&next);

    // Determine continue mode from skills config
    let continue_mode = state
        .get("skills")
        .and_then(|skills| skills.get(phase))
        .and_then(|cfg| {
            // String config (e.g. "auto")
            if let Some(s) = cfg.as_str() {
                return Some(s.to_string());
            }
            // Dict config (e.g. {"continue": "auto"})
            if let Some(obj) = cfg.as_object() {
                return obj
                    .get("continue")
                    .and_then(|v| v.as_str())
                    .map(String::from);
            }
            None
        });

    let next_command = commands.get(&next).cloned();

    let (continue_action, should_set_auto_continue) =
        if continue_mode.as_deref() == Some("auto") && next_command.is_some() {
            ("invoke", true)
        } else {
            ("ask", false)
        };

    if should_set_auto_continue {
        state["_auto_continue"] = json!(next_command.as_ref().unwrap());
    } else {
        // State is guaranteed to be an object here — earlier
        // `state["phases"][phase]` accesses would have panicked
        // on any non-object input.
        state
            .as_object_mut()
            .expect("phase_complete requires object state")
            .remove("_auto_continue");
    }

    // Capture diff stats for code phase
    if phase == "flow-code" {
        state["diff_stats"] = capture_diff_stats();
    }

    let mut result = json!({
        "status": "ok",
        "phase": phase,
        "action": "complete",
        "cumulative_seconds": cumulative,
        "formatted_time": format_time(cumulative),
        "next_phase": next,
        "continue_action": continue_action,
    });

    if continue_action == "invoke" {
        result["continue_target"] = json!(next_command.unwrap());
    }

    result
}

/// Parse a `git diff --stat` summary line into (files_changed,
/// insertions, deletions). Pure function exposed as a public seam so
/// `tests/phase_transition.rs` can exercise every comma-separated
/// branch without spawning git.
pub fn parse_diff_stat_summary(summary: &str) -> (i64, i64, i64) {
    let mut files_changed: i64 = 0;
    let mut insertions: i64 = 0;
    let mut deletions: i64 = 0;

    for part in summary.split(',') {
        let part = part.trim();
        if part.contains("file") {
            if let Some(n) = part.split_whitespace().next().and_then(|s| s.parse().ok()) {
                files_changed = n;
            }
        } else if part.contains("insertion") {
            if let Some(n) = part.split_whitespace().next().and_then(|s| s.parse().ok()) {
                insertions = n;
            }
        } else if part.contains("deletion") {
            if let Some(n) = part.split_whitespace().next().and_then(|s| s.parse().ok()) {
                deletions = n;
            }
        }
    }

    (files_changed, insertions, deletions)
}

/// Build a diff_stats JSON object from parsed counts.
fn diff_stats_json(files_changed: i64, insertions: i64, deletions: i64) -> Value {
    json!({
        "files_changed": files_changed,
        "insertions": insertions,
        "deletions": deletions,
        "captured_at": now(),
    })
}

/// Build a diff_stats JSON object from the raw `git diff --stat`
/// subprocess result. Public seam: tests pass constructed
/// `io::Result<Output>` values to drive the Err and non-success Ok
/// branches without spawning real git.
pub fn capture_diff_stats_from_result(result: std::io::Result<std::process::Output>) -> Value {
    let output = match result {
        Ok(o) if o.status.success() => o,
        _ => return diff_stats_json(0, 0, 0),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return diff_stats_json(0, 0, 0);
    }
    let summary = trimmed.lines().last().unwrap_or("");
    let (files, ins, del) = parse_diff_stat_summary(summary);
    diff_stats_json(files, ins, del)
}

/// Capture git diff --stat summary for the current branch vs main.
///
/// Returns a JSON object with files_changed, insertions, deletions, captured_at.
/// Best-effort: returns zeros if git fails.
pub fn capture_diff_stats() -> Value {
    capture_diff_stats_from_result(
        Command::new("git")
            .args(["diff", "--stat", "main...HEAD"])
            .output(),
    )
}

/// Driver for the `bin/flow phase-transition` subcommand.
///
/// Returns `(output_json, exit_code)`. Output is always a JSON Value:
/// the normal phase_enter/phase_complete result on success, or the
/// `json_error` shape on validation/IO/state errors. Success and
/// mutation-failure paths both append a one-line entry to
/// `.flow-states/<branch>.log`.
///
/// Tests supply `root` as a fixture TempDir and `cwd` equal to the
/// root so the cwd-drift guard passes; `branch_override` is required
/// so the helper does not shell out to `git rev-parse` against the
/// host worktree.
pub fn run_impl_main(
    phase: &str,
    action: &str,
    next_phase: Option<&str>,
    branch_override: Option<&str>,
    reason: Option<&str>,
    root: &Path,
    cwd: &Path,
) -> (Value, i32) {
    run_impl_main_with_resolver(
        phase,
        action,
        next_phase,
        branch_override,
        reason,
        root,
        cwd,
        &resolve_branch,
    )
}

/// Seam-injected variant of `run_impl_main`. Tests pass a mock
/// branch resolver to drive the `resolve_branch` None path without
/// racing on the process's git env.
#[allow(clippy::too_many_arguments)]
pub fn run_impl_main_with_resolver(
    phase: &str,
    action: &str,
    next_phase: Option<&str>,
    branch_override: Option<&str>,
    reason: Option<&str>,
    root: &Path,
    cwd: &Path,
    resolve_branch_fn: &dyn Fn(Option<&str>, &Path) -> Option<String>,
) -> (Value, i32) {
    if !PHASE_ORDER.contains(&phase) {
        let msg = format!(
            "Invalid phase: {}. Must be one of: {}",
            phase,
            PHASE_ORDER.join(", ")
        );
        return (json_error_value(&msg), 1);
    }

    if action != "enter" && action != "complete" {
        let msg = format!("Invalid action: {}. Must be 'enter' or 'complete'", action);
        return (json_error_value(&msg), 1);
    }

    if let Err(msg) = cwd_scope::enforce(cwd, root) {
        return (json_error_value(&msg), 1);
    }

    let branch = match resolve_branch_fn(branch_override, root) {
        Some(b) => b,
        None => {
            return (json_error_value("Could not determine current branch"), 1);
        }
    };
    // `resolve_branch` may return a raw git ref (slash-containing,
    // empty) when no state file matches the override. `FlowPaths::new`
    // panics on those; use `try_new` per `.claude/rules/external-input-validation.md`
    // and surface the invalid branch as a structured error rather than
    // a crashing panic.
    let paths = match FlowPaths::try_new(root, &branch) {
        Some(p) => p,
        None => {
            return (
                json_error_value(&format!(
                    "Invalid branch name: \"{}\" (contains '/' or is empty)",
                    branch
                )),
                1,
            );
        }
    };
    let state_path = paths.state_file();

    if !state_path.exists() {
        return (
            json_error_value(&format!("No state file found: {}", state_path.display())),
            1,
        );
    }

    let content = match std::fs::read_to_string(&state_path) {
        Ok(c) => c,
        Err(e) => {
            return (
                json_error_value(&format!("Could not read state file: {}", e)),
                1,
            );
        }
    };

    let state: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            return (
                json_error_value(&format!("Could not read state file: {}", e)),
                1,
            );
        }
    };

    if state.get("phases").is_none() || state["phases"].get(phase).is_none() {
        return (
            json_error_value(&format!("Phase {} not found in state file", phase)),
            1,
        );
    }

    let frozen_path = paths.frozen_phases();
    let frozen_config = if frozen_path.exists() {
        load_phase_config(&frozen_path).ok()
    } else {
        None
    };
    let frozen_order: Option<Vec<String>> = frozen_config.as_ref().map(|c| c.order.clone());
    let frozen_commands = frozen_config.as_ref().map(|c| c.commands.clone());

    let result_holder = std::cell::RefCell::new(Value::Null);

    let action_owned = action.to_string();
    let phase_owned = phase.to_string();
    let next_phase_owned = next_phase.map(|s| s.to_string());
    let reason_owned = reason.map(|s| s.to_string());

    let mutate_result = mutate_state(&state_path, &mut |state| {
        let result = if action_owned == "enter" {
            phase_enter(state, &phase_owned, reason_owned.as_deref())
        } else {
            phase_complete(
                state,
                &phase_owned,
                next_phase_owned.as_deref(),
                frozen_order.as_deref(),
                frozen_commands.as_ref(),
            )
        };
        *result_holder.borrow_mut() = result;
    });

    let pn = phase_number(phase);

    match mutate_result {
        Ok(_) => {
            let result = result_holder.into_inner();
            let status = result
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let _ = append_log(
                root,
                &branch,
                &format!(
                    "[Phase {}] phase-transition --action {} --phase {} (\"{}\")",
                    pn, action, phase, status
                ),
            );
            (result, 0)
        }
        Err(e) => {
            let _ = append_log(
                root,
                &branch,
                &format!(
                    "[Phase {}] phase-transition --action {} --phase {} (\"error\")",
                    pn, action, phase
                ),
            );
            (
                json_error_value(&format!("State mutation failed: {}", e)),
                1,
            )
        }
    }
}

/// Build a `json_error`-shaped Value (parsed from `json_error_string`)
/// so `run_impl_main` can return `(Value, i32)` while matching the
/// pre-extraction `json_error` output contract exactly. The parse
/// cannot fail — `json_error_string` constructs the JSON from a valid
/// serde_json::Map — so `.expect` is correct per
/// `.claude/rules/testability-means-simplicity.md`.
fn json_error_value(message: &str) -> Value {
    serde_json::from_str::<Value>(&json_error_string(message, &[]))
        .expect("json_error_string produces valid JSON")
}
