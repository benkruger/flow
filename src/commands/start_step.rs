//! Update the Start phase step counter in the FLOW state file.
//!
//! Combines step tracking with subcommand execution in a single tool call.
//! When wrapping a subcommand, updates the counter then execs the subcommand
//! via bin/flow. Best-effort: silently skips if the state file is missing
//! or corrupt.
//!
//! Tests live at tests/start_step.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use std::path::Path;

use serde_json::json;

use crate::flow_paths::FlowPaths;
use crate::git::project_root;
use crate::lock::mutate_state;
use crate::output::json_ok;

/// Update start_step in the state file. Returns true if updated.
/// Guards against non-object state files to avoid IndexMut panics.
pub fn update_step(state_path: &Path, step: i64) -> bool {
    if !state_path.exists() {
        return false;
    }
    mutate_state(state_path, |state| {
        if !(state.is_object() || state.is_null()) {
            return;
        }
        state["start_step"] = json!(step);
    })
    .is_ok()
}

/// Resolve the `bin/flow` dispatcher path from `current_exe`.
///
/// Binary lives at `<root>/target/{debug,release}/flow-rs`, so the
/// plugin/repo root is 3 parents up. Falls back to `<project_root>/bin/flow`
/// when `current_exe` is unavailable or too shallow (unusual platforms,
/// test fixtures that relocate the binary).
///
/// Accepts `exe_result` as a parameter so tests can force each branch
/// without mutating process state — production always passes
/// `std::env::current_exe()`.
pub fn resolve_flow_bin(
    exe_result: std::io::Result<std::path::PathBuf>,
    project_root: &Path,
) -> std::path::PathBuf {
    if let Ok(exe) = exe_result {
        if let Some(repo_root) = exe.ancestors().nth(3) {
            return repo_root.join("bin").join("flow");
        }
    }
    project_root.join("bin").join("flow")
}

/// CLI entry point.
///
/// Updates step counter, then either prints JSON (standalone) or
/// execs into a subcommand via bin/flow.
pub fn run(step: i64, branch: &str, subcommand: Vec<String>) {
    let root = project_root();
    let state_path = FlowPaths::new(&root, branch).state_file();

    let updated = update_step(&state_path, step);

    if !subcommand.is_empty() {
        let flow_bin = resolve_flow_bin(std::env::current_exe(), &root);

        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(&flow_bin)
            .args(&subcommand)
            .exec();
        // exec() only returns on error
        eprintln!("Failed to exec {:?}: {}", flow_bin, err);
        std::process::exit(1);
    } else if updated {
        json_ok(&[("step", json!(step))]);
    } else {
        println!(
            "{}",
            json!({"status": "skipped", "reason": "no state file"})
        );
    }
}
