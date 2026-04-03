//! Update the Start phase step counter in the FLOW state file.
//!
//! Combines step tracking with subcommand execution in a single tool call.
//! When wrapping a subcommand, updates the counter then execs the subcommand
//! via bin/flow. Best-effort: silently skips if the state file is missing
//! or corrupt.

use std::path::Path;

use serde_json::json;

use crate::git::project_root;
use crate::lock::mutate_state;
use crate::output::json_ok;

/// Update start_step in the state file. Returns true if updated.
pub fn update_step(state_path: &Path, step: i64) -> bool {
    if !state_path.exists() {
        return false;
    }
    mutate_state(state_path, |state| {
        state["start_step"] = json!(step);
    })
    .is_ok()
}

/// CLI entry point.
///
/// Updates step counter, then either prints JSON (standalone) or
/// execs into a subcommand via bin/flow.
pub fn run(step: i64, branch: &str, subcommand: Vec<String>) {
    let root = project_root();
    let state_path = root.join(".flow-states").join(format!("{}.json", branch));

    let updated = update_step(&state_path, step);

    if !subcommand.is_empty() {
        // Wrapping mode: exec into bin/flow (the hybrid dispatcher) so
        // Python-only subcommands like `ci` still work via fallback.
        // The binary is at target/{debug,release}/flow-rs — go up 3
        // levels to reach the plugin/repo root, then into bin/flow.
        let flow_bin = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent()?.parent()?.parent().map(|d| d.to_path_buf()))
            .map(|d| d.join("bin").join("flow"))
            .unwrap_or_else(|| {
                // Fallback: find bin/flow relative to project root
                root.join("bin").join("flow")
            });

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::fs;

    fn make_state() -> Value {
        json!({
            "schema_version": 1,
            "branch": "test-feature",
            "current_phase": "flow-start",
            "files": {
                "plan": null,
                "dag": null,
                "log": ".flow-states/test-feature.log",
                "state": ".flow-states/test-feature.json"
            },
            "phases": {}
        })
    }

    #[test]
    fn test_update_step_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, serde_json::to_string_pretty(&make_state()).unwrap()).unwrap();

        let result = update_step(&path, 5);
        assert!(result);

        let content = fs::read_to_string(&path).unwrap();
        let state: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(state["start_step"], 5);
    }

    #[test]
    fn test_update_step_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let result = update_step(&path, 5);
        assert!(!result);
    }

    #[test]
    fn test_update_step_corrupt_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, "not valid json{{{").unwrap();
        let result = update_step(&path, 5);
        assert!(!result);
    }

    #[test]
    fn test_update_step_preserves_other_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let mut state = make_state();
        state["code_task"] = json!(3);
        fs::write(&path, serde_json::to_string_pretty(&state).unwrap()).unwrap();

        update_step(&path, 7);

        let content = fs::read_to_string(&path).unwrap();
        let updated: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(updated["start_step"], 7);
        assert_eq!(updated["code_task"], 3);
        assert_eq!(updated["branch"], "test-feature");
    }

    #[test]
    fn test_update_step_overwrites_previous() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let mut state = make_state();
        state["start_step"] = json!(3);
        fs::write(&path, serde_json::to_string_pretty(&state).unwrap()).unwrap();

        let result = update_step(&path, 8);
        assert!(result);

        let content = fs::read_to_string(&path).unwrap();
        let updated: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(updated["start_step"], 8);
    }
}
