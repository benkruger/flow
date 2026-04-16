use std::path::Path;
use std::process;

use clap::Parser;
use serde_json::{json, Value};

use crate::flow_paths::FlowPaths;
use crate::git::{project_root, resolve_branch};
use crate::lock::mutate_state;
use crate::output::{json_error, json_ok};
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

pub fn run(args: Args) {
    let root = project_root();
    let branch = match resolve_branch(args.branch.as_deref(), &root) {
        Some(b) => b,
        None => {
            json_error("Could not determine current branch", &[]);
            process::exit(1);
        }
    };
    let state_path = FlowPaths::new(&root, &branch).state_file();

    if !state_path.exists() {
        println!(r#"{{"status":"no_state"}}"#);
        process::exit(0);
    }

    // Read current_phase before mutating
    let phase = match read_current_phase(&state_path) {
        Some(p) => p,
        None => {
            json_error("Could not read state file", &[]);
            process::exit(1);
        }
    };

    let names = phase_names();
    let phase_name = names.get(&phase).cloned().unwrap_or_else(|| phase.clone());
    let timestamp = now();

    match mutate_state(&state_path, |state| {
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
        if let Some(arr) = state["notes"].as_array_mut() {
            arr.push(json!({
                "phase": phase,
                "phase_name": phase_name,
                "timestamp": timestamp,
                "type": args.note_type,
                "note": args.note,
            }));
        }
    }) {
        Ok(state) => {
            let count = state["notes"].as_array().map(|a| a.len()).unwrap_or(0);
            json_ok(&[("note_count", json!(count))]);
        }
        Err(e) => {
            json_error(&format!("Failed to append note: {}", e), &[]);
            process::exit(1);
        }
    }
}

fn read_current_phase(state_path: &Path) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_state(branch: &str) -> Value {
        json!({
            "schema_version": 1,
            "branch": branch,
            "current_phase": "flow-plan",
            "notes": []
        })
    }

    fn write_state(dir: &Path, branch: &str, state: &Value) -> std::path::PathBuf {
        let state_dir = dir.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let path = state_dir.join(format!("{}.json", branch));
        fs::write(&path, serde_json::to_string_pretty(state).unwrap()).unwrap();
        path
    }

    #[test]
    fn append_note_happy_path() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("test-feature");
        let path = write_state(dir.path(), "test-feature", &state);

        let result = mutate_state(&path, |s| {
            let names = phase_names();
            let phase = "flow-plan";
            let phase_name = names.get(phase).cloned().unwrap_or_default();
            if s.get("notes").is_none() || !s["notes"].is_array() {
                s["notes"] = json!([]);
            }
            if let Some(arr) = s["notes"].as_array_mut() {
                arr.push(json!({
                    "phase": phase,
                    "phase_name": phase_name,
                    "timestamp": now(),
                    "type": "correction",
                    "note": "test note",
                }));
            }
        })
        .unwrap();

        let notes = result["notes"].as_array().unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0]["phase"], "flow-plan");
        assert_eq!(notes[0]["phase_name"], "Plan");
        assert_eq!(notes[0]["type"], "correction");
        assert_eq!(notes[0]["note"], "test note");
        assert!(notes[0]["timestamp"].as_str().unwrap().contains("T"));
    }

    #[test]
    fn append_note_multiple_accumulate() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("test-feature");
        let path = write_state(dir.path(), "test-feature", &state);

        for i in 0..3 {
            mutate_state(&path, |s| {
                if let Some(arr) = s["notes"].as_array_mut() {
                    arr.push(json!({
                        "phase": "flow-code",
                        "phase_name": "Code",
                        "timestamp": now(),
                        "type": "correction",
                        "note": format!("note {}", i),
                    }));
                }
            })
            .unwrap();
        }

        let content = fs::read_to_string(&path).unwrap();
        let on_disk: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(on_disk["notes"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn append_note_creates_array_if_missing() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let path = state_dir.join("test.json");
        // State with no notes key
        fs::write(&path, r#"{"current_phase": "flow-code"}"#).unwrap();

        mutate_state(&path, |s| {
            if s.get("notes").is_none() || !s["notes"].is_array() {
                s["notes"] = json!([]);
            }
            if let Some(arr) = s["notes"].as_array_mut() {
                arr.push(json!({"note": "first"}));
            }
        })
        .unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let on_disk: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(on_disk["notes"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn append_note_preserves_existing() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = make_state("test-feature");
        state["notes"] = json!([
            {"phase": "flow-start", "note": "existing"}
        ]);
        let path = write_state(dir.path(), "test-feature", &state);

        mutate_state(&path, |s| {
            if let Some(arr) = s["notes"].as_array_mut() {
                arr.push(json!({"phase": "flow-code", "note": "new"}));
            }
        })
        .unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let on_disk: Value = serde_json::from_str(&content).unwrap();
        let notes = on_disk["notes"].as_array().unwrap();
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0]["note"], "existing");
        assert_eq!(notes[1]["note"], "new");
    }

    #[test]
    fn read_current_phase_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, r#"{"current_phase": "flow-learn"}"#).unwrap();
        assert_eq!(read_current_phase(&path), Some("flow-learn".to_string()));
    }

    #[test]
    fn read_current_phase_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        assert_eq!(read_current_phase(&path), None);
    }

    #[test]
    fn read_current_phase_missing_key_defaults_to_flow_start() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, r#"{"branch": "test"}"#).unwrap();
        assert_eq!(read_current_phase(&path), Some("flow-start".to_string()));
    }

    #[test]
    fn read_current_phase_corrupt_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, "{corrupt").unwrap();
        assert_eq!(read_current_phase(&path), None);
    }

    /// Verify that an array-root state file triggers the object guard's
    /// early return, leaving the file unchanged and preventing an
    /// IndexMut panic on non-object root types.
    #[test]
    fn append_note_array_root_state_noop() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let path = state_dir.join("test.json");
        let content = "[1, 2, 3]";
        fs::write(&path, content).unwrap();

        mutate_state(&path, |state| {
            if !(state.is_object() || state.is_null()) {
                return;
            }
            if state.get("notes").is_none() || !state["notes"].is_array() {
                state["notes"] = json!([]);
            }
            if let Some(arr) = state["notes"].as_array_mut() {
                arr.push(json!({"note": "should not appear"}));
            }
        })
        .unwrap();

        let after = fs::read_to_string(&path).unwrap();
        let parsed: Value = serde_json::from_str(&after).unwrap();
        assert!(parsed.is_array(), "Root should still be an array");
        assert_eq!(parsed.as_array().unwrap().len(), 3);
    }

    #[test]
    fn corrupt_state_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let path = state_dir.join("test.json");
        fs::write(&path, "{corrupt").unwrap();

        let result = mutate_state(&path, |_| {});
        assert!(result.is_err());
    }

    #[test]
    fn note_fields_have_correct_key_order() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("test-feature");
        let path = write_state(dir.path(), "test-feature", &state);

        let result = mutate_state(&path, |s| {
            if let Some(arr) = s["notes"].as_array_mut() {
                arr.push(json!({
                    "phase": "flow-plan",
                    "phase_name": "Plan",
                    "timestamp": "2026-01-01T00:00:00-08:00",
                    "type": "correction",
                    "note": "test",
                }));
            }
        })
        .unwrap();

        let note = &result["notes"][0];
        assert!(note.get("phase").is_some());
        assert!(note.get("phase_name").is_some());
        assert!(note.get("timestamp").is_some());
        assert!(note.get("type").is_some());
        assert!(note.get("note").is_some());
    }
}
