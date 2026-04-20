//! `bin/flow append-note` — append a note to FLOW state.
//!
//! Tests live at `tests/append_note.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` in
//! this file.

use std::path::Path;

use clap::Parser;
use serde_json::{json, Value};

use crate::flow_paths::FlowPaths;
use crate::git::resolve_branch;
use crate::lock::mutate_state;
use crate::phase_config::phase_names;
use crate::utils::now;

#[derive(Parser, Debug)]
#[command(name = "append-note", about = "Append a note to FLOW state")]
pub struct Args {
    /// Note text
    #[arg(long)]
    pub note: String,

    /// Note type
    #[arg(long = "type", default_value = "correction", value_parser = ["correction", "learning"])]
    pub note_type: String,

    /// Override branch for state file lookup
    #[arg(long)]
    pub branch: Option<String>,
}

/// Applies the notes append transform to the in-memory state.
/// Extracted to a named function so cargo-llvm-cov measures a single
/// monomorphization of the mutation logic regardless of how many
/// tests or production paths call [`run_impl_main`]. See
/// `add_issue::apply_issue_mutation` for the mirror pattern.
fn apply_note_mutation(
    state: &mut Value,
    args: &Args,
    phase: &str,
    phase_name: &str,
    timestamp: &str,
) {
    // Corruption resilience: skip mutation when state root is wrong
    // type (e.g. array from interrupted write) to prevent IndexMut
    // panics. See .claude/rules/rust-patterns.md "State Mutation
    // Object Guards".
    if !(state.is_object() || state.is_null()) {
        return;
    }
    if state.get("notes").is_none() || !state["notes"].is_array() {
        state["notes"] = json!([]);
    }
    // The block above guarantees state["notes"] is an array, so
    // as_array_mut returns Some unconditionally.
    let arr = state["notes"]
        .as_array_mut()
        .expect("notes is always an array here");
    arr.push(json!({
        "phase": phase,
        "phase_name": phase_name,
        "timestamp": timestamp,
        "type": args.note_type,
        "note": args.note,
    }));
}

/// Main-arm dispatcher with injected root. Returns `(value, exit_code)`:
/// `(ok+note_count, 0)` on success, `(no_state, 0)` when the state file
/// is missing, `(error+message, 1)` on resolve-branch failure,
/// state-read failure, or mutate_state failure.
pub fn run_impl_main(args: Args, root: &Path) -> (Value, i32) {
    let branch = match resolve_branch(args.branch.as_deref(), root) {
        Some(b) => b,
        None => {
            return (
                json!({"status": "error", "message": "Could not determine current branch"}),
                1,
            );
        }
    };
    // Branch reaches us either from `current_branch()` (raw git output)
    // or from `--branch` CLI override (raw user input). Both are
    // external inputs per `.claude/rules/external-input-validation.md`,
    // so use the fallible constructor to reject slash-containing or
    // empty branches as a structured error rather than a panic.
    let state_path = match FlowPaths::try_new(root, &branch) {
        Some(p) => p.state_file(),
        None => {
            return (
                json!({"status": "error", "message": format!("Invalid branch '{}'", branch)}),
                1,
            );
        }
    };

    if !state_path.exists() {
        return (json!({"status": "no_state"}), 0);
    }

    // Read current_phase before mutating
    let phase = match read_current_phase(&state_path) {
        Some(p) => p,
        None => {
            return (
                json!({"status": "error", "message": "Could not read state file"}),
                1,
            );
        }
    };

    let names = phase_names();
    let phase_name = match names.get(&phase) {
        Some(n) => n.clone(),
        None => phase.clone(),
    };
    let timestamp = now();

    match mutate_state(&state_path, &mut |state| {
        apply_note_mutation(state, &args, &phase, &phase_name, &timestamp);
    }) {
        Ok(state) => {
            let count = match state["notes"].as_array() {
                Some(a) => a.len(),
                None => 0,
            };
            (json!({"status": "ok", "note_count": count}), 0)
        }
        Err(e) => (
            json!({"status": "error", "message": format!("Failed to append note: {}", e)}),
            1,
        ),
    }
}

/// Read the `current_phase` field from a state file, defaulting to
/// `"flow-start"` when the field is absent. Returns `None` when the
/// file cannot be read or parsed. Exposed pub so tests can drive
/// each branch directly.
pub fn read_current_phase(state_path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(state_path).ok()?;
    let state: Value = serde_json::from_str(&content).ok()?;
    Some(
        state
            .get("current_phase")
            .and_then(|v| v.as_str())
            .unwrap_or("flow-start")
            .to_string(),
    )
}
