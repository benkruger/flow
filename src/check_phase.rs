//! Phase-entry gate logic and `bin/flow check-phase` CLI driver.
//!
//! `check_phase()` is the pure predicate: given a state JSON value
//! and a target phase, return `(allowed, message)`. `run_impl_main()`
//! is the thin CLI driver that loads the state file from disk and
//! routes the predicate's output through the plain-text contract that
//! `bin/flow check-phase` consumers (the `validate-claude-paths` hook
//! and other phase-entry gates) parse from stdout. The driver is the
//! `main.rs` `Commands::CheckPhase` arm's only behaviour — main.rs
//! delegates here and prints the returned string via
//! `dispatch::dispatch_text`.
//!
//! Tests live at `tests/check_phase.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` in
//! this file.

use std::path::Path;

use indexmap::IndexMap;
use serde_json::Value;

use crate::flow_paths::FlowPaths;
use crate::git::resolve_branch;
use crate::output::json_error_string;
use crate::phase_config::{self, load_phase_config, PhaseConfig, PHASE_ORDER};

/// Check if entry into `phase` is allowed given the state JSON.
///
/// Returns `Ok((allowed, message))` where message may be empty if allowed
/// with no note. Returns `Err` if the phase name is invalid.
fn check_phase(
    state: &Value,
    phase: &str,
    phase_config: Option<&PhaseConfig>,
) -> Result<(bool, String), String> {
    let default_order: Vec<String> = PHASE_ORDER.iter().map(|&s| s.to_string()).collect();
    let default_names: IndexMap<String, String> = phase_config::phase_names();
    let default_numbers: IndexMap<String, usize> = phase_config::phase_numbers();
    let default_commands: IndexMap<String, String> = phase_config::commands();

    #[allow(clippy::type_complexity)]
    let (order, names, numbers, commands): (
        &Vec<String>,
        &IndexMap<String, String>,
        &IndexMap<String, usize>,
        &IndexMap<String, String>,
    ) = match phase_config {
        Some(cfg) => (&cfg.order, &cfg.names, &cfg.numbers, &cfg.commands),
        None => (
            &default_order,
            &default_names,
            &default_numbers,
            &default_commands,
        ),
    };

    let phase_idx = match order.iter().position(|p| p == phase) {
        Some(idx) => idx,
        None => {
            return Err(format!(
                "Invalid phase: {}. Must be one of: {}",
                phase,
                order.join(", ")
            ));
        }
    };

    // First phase has no prerequisites
    if phase_idx == 0 {
        return Ok((true, String::new()));
    }

    let prev = &order[phase_idx - 1];
    let phases = state.get("phases").and_then(|v| v.as_object());

    let prev_data = phases.and_then(|p| p.get(prev.as_str()));
    let prev_status = prev_data
        .and_then(|d| d.get("status"))
        .and_then(|s| s.as_str())
        .unwrap_or("pending");

    let prev_name = match names.get(prev.as_str()) {
        Some(n) => n.clone(),
        None => prev.clone(),
    };
    let prev_num = numbers.get(prev.as_str()).copied().unwrap_or(0);
    let prev_cmd = match commands.get(prev.as_str()) {
        Some(c) => c.clone(),
        None => format!("/flow:{}", prev),
    };

    let phase_name = match names.get(phase) {
        Some(n) => n.clone(),
        None => phase.to_string(),
    };
    let phase_num = numbers.get(phase).copied().unwrap_or(0);

    if prev_status != "complete" {
        let msg = format!(
            "BLOCKED: Phase {}: {} must be complete before entering Phase {}: {}.\n\
             Phase {} current status: {}\n\
             Complete it first with: {}",
            prev_num, prev_name, phase_num, phase_name, prev_num, prev_status, prev_cmd
        );
        return Ok((false, msg));
    }

    // Allowed — check if revisiting
    let this_data = phases.and_then(|p| p.get(phase));
    let this_status = this_data
        .and_then(|d| d.get("status"))
        .and_then(|s| s.as_str())
        .unwrap_or("pending");

    if this_status == "complete" {
        let visits = match this_data.and_then(|d| d.get("visit_count")) {
            Some(v) => v.as_i64().unwrap_or(0),
            None => 0,
        };
        let msg = format!(
            "NOTE: Phase {}: {} was previously completed ({} visit(s)). Re-entering.",
            phase_num, phase_name, visits
        );
        return Ok((true, msg));
    }

    Ok((true, String::new()))
}

/// Driver for the `bin/flow check-phase` subcommand.
///
/// Returns `(output, exit_code)`. The output is plain text for the
/// BLOCKED/NOTE/allowed paths and a JSON error object for
/// branch-resolution, file-read, and parse errors — mirroring the
/// mixed output contract of the pre-extraction inline dispatch. The
/// first-phase short-circuit returns `("", 0)`.
///
/// Tests supply `root` as a fixture TempDir containing
/// `.flow-states/<branch>.json`; `branch_override` is required so the
/// helper does not shell out to `git rev-parse` against the host
/// worktree.
pub fn run_impl_main(phase: &str, branch_override: Option<&str>, root: &Path) -> (String, i32) {
    // First phase has no prerequisites — short-circuit before touching
    // the filesystem or resolving a branch.
    if phase == PHASE_ORDER[0] {
        return (String::new(), 0);
    }

    let branch = match resolve_branch(branch_override, root) {
        Some(b) => b,
        None => {
            return (
                "BLOCKED: Could not determine current git branch.".to_string(),
                1,
            );
        }
    };

    // `resolve_branch` may return a raw git ref (slash-containing,
    // empty) when no state file matches. `FlowPaths::new` panics on
    // those; use `try_new` per `.claude/rules/external-input-validation.md`
    // and treat invalid branches as "no active flow" just like the
    // missing-state-file case below.
    let paths = match FlowPaths::try_new(root, &branch) {
        Some(p) => p,
        None => {
            return (
                format!(
                    "BLOCKED: No FLOW feature in progress on branch \"{}\".\nRun /flow:flow-start to begin a new feature.",
                    branch
                ),
                1,
            );
        }
    };
    let state_file = paths.state_file();
    if !state_file.exists() {
        return (
            format!(
                "BLOCKED: No FLOW feature in progress on branch \"{}\".\nRun /flow:flow-start to begin a new feature.",
                branch
            ),
            1,
        );
    }

    let content = match std::fs::read_to_string(&state_file) {
        Ok(c) => c,
        Err(e) => {
            return (format!("BLOCKED: Could not read state file: {}", e), 1);
        }
    };

    let state: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            return (format!("BLOCKED: Could not read state file: {}", e), 1);
        }
    };

    let frozen_path = paths.frozen_phases();
    let frozen_config = if frozen_path.exists() {
        load_phase_config(&frozen_path).ok()
    } else {
        None
    };

    match check_phase(&state, phase, frozen_config.as_ref()) {
        Ok((allowed, output)) => (output, if allowed { 0 } else { 1 }),
        Err(msg) => (json_error_string(&msg, &[]), 1),
    }
}
