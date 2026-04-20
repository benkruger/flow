//! `bin/flow add-issue` — record a filed issue in FLOW state.
//!
//! Tests live at `tests/add_issue.rs` per
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
#[command(name = "add-issue", about = "Record a filed issue in FLOW state")]
pub struct Args {
    /// Issue label (e.g. Rule, Flow, Flaky Test)
    #[arg(long)]
    pub label: String,

    /// Issue title
    #[arg(long)]
    pub title: String,

    /// Issue URL
    #[arg(long)]
    pub url: String,

    /// Phase that filed the issue
    #[arg(long)]
    pub phase: String,

    /// Override branch for state file lookup
    #[arg(long)]
    pub branch: Option<String>,
}

/// Applies the issues_filed append transform to the in-memory state.
///
/// Extracted to a named function so cargo-llvm-cov measures a single
/// monomorphization of the mutation logic regardless of how many
/// tests or production paths call [`run_impl_main`]. The closure
/// passed to [`mutate_state`] becomes a thin delegator, while every
/// region inside `apply_issue_mutation` (object-guard early return,
/// missing-array auto-create, array push) reaches 100% merged
/// coverage under the test suite.
fn apply_issue_mutation(state: &mut Value, args: &Args, phase_name: &str, timestamp: &str) {
    // Corruption resilience: skip mutation when state root is wrong
    // type (e.g. array from interrupted write) to prevent IndexMut
    // panics. See .claude/rules/rust-patterns.md "State Mutation
    // Object Guards".
    if !(state.is_object() || state.is_null()) {
        return;
    }
    if state.get("issues_filed").is_none() || !state["issues_filed"].is_array() {
        state["issues_filed"] = json!([]);
    }
    // The block above guarantees state["issues_filed"] is an array,
    // so as_array_mut returns Some unconditionally.
    let arr = state["issues_filed"]
        .as_array_mut()
        .expect("issues_filed is always an array here");
    arr.push(json!({
        "label": args.label,
        "title": args.title,
        "url": args.url,
        "phase": args.phase,
        "phase_name": phase_name,
        "timestamp": timestamp,
    }));
}

/// Main-arm dispatcher with injected root. Returns `(value, exit_code)`:
/// `(ok+issue_count, 0)` on success, `(no_state, 0)` when the state file
/// is missing, `(error+message, 1)` on resolve-branch failure or
/// mutate_state failure. Tests pass tempdir paths and `--branch` args
/// to bypass git resolution.
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

    let names = phase_names();
    let phase_name = match names.get(&args.phase) {
        Some(n) => n.clone(),
        None => args.phase.clone(),
    };
    let timestamp = now();

    match mutate_state(&state_path, &mut |state| {
        apply_issue_mutation(state, &args, &phase_name, &timestamp);
    }) {
        Ok(state) => {
            let count = match state["issues_filed"].as_array() {
                Some(a) => a.len(),
                None => 0,
            };
            (json!({"status": "ok", "issue_count": count}), 0)
        }
        Err(e) => (
            json!({"status": "error", "message": format!("Failed to add issue: {}", e)}),
            1,
        ),
    }
}
