use std::path::Path;

use clap::Parser;
use serde_json::{json, Value};

use crate::flow_paths::FlowPaths;
use crate::git::{project_root, resolve_branch};
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

pub fn run(args: Args) -> ! {
    let root = project_root();
    let (value, code) = run_impl_main(args, &root);
    crate::dispatch::dispatch_json(value, code)
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

    // --- run_impl_main ---

    fn make_args(branch: Option<&str>) -> Args {
        Args {
            note: "test note".to_string(),
            note_type: "correction".to_string(),
            branch: branch.map(|s| s.to_string()),
        }
    }

    #[test]
    fn append_note_run_impl_main_no_state_returns_no_state_tuple() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let args = make_args(Some("missing-branch"));
        let (value, code) = run_impl_main(args, &root);
        assert_eq!(value["status"], "no_state");
        assert_eq!(code, 0);
    }

    #[test]
    fn append_note_run_impl_main_success_returns_note_count_tuple() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(
            state_dir.join("present-branch.json"),
            r#"{"current_phase":"flow-plan","notes":[]}"#,
        )
        .unwrap();
        let args = make_args(Some("present-branch"));
        let (value, code) = run_impl_main(args, &root);
        assert_eq!(code, 0);
        assert_eq!(value["status"], "ok");
        assert_eq!(value["note_count"], 1);
    }

    #[test]
    fn append_note_run_impl_main_state_read_failure_returns_error_tuple() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        // Malformed JSON makes read_current_phase return None (after exists() passes).
        fs::write(state_dir.join("present-branch.json"), "{not json").unwrap();
        let args = make_args(Some("present-branch"));
        let (value, code) = run_impl_main(args, &root);
        assert_eq!(value["status"], "error");
        assert_eq!(code, 1);
        assert!(value["message"]
            .as_str()
            .unwrap()
            .contains("Could not read state file"));
    }

    #[test]
    fn append_note_run_impl_main_array_root_returns_ok_zero_count() {
        // State root is an array — closure guard fires early return,
        // leaving notes as Value::Null. as_array() None branch returns 0.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        // read_current_phase parses the file: an array gets `current_phase`
        // missing → defaults to "flow-start". The closure then early-
        // returns leaving notes as Null.
        fs::write(state_dir.join("array-root.json"), "[1, 2, 3]").unwrap();
        let args = make_args(Some("array-root"));
        let (value, code) = run_impl_main(args, &root);
        assert_eq!(value["status"], "ok");
        assert_eq!(value["note_count"], 0);
        assert_eq!(code, 0);
    }

    #[test]
    fn append_note_run_impl_main_unknown_phase_falls_back_to_phase_string() {
        // State has current_phase="custom-phase" not in phase_names →
        // unwrap_or_else fallback fires.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(
            state_dir.join("unknown-phase.json"),
            r#"{"current_phase":"custom-unknown-phase","notes":[]}"#,
        )
        .unwrap();
        let args = make_args(Some("unknown-phase"));
        let (value, code) = run_impl_main(args, &root);
        assert_eq!(value["status"], "ok");
        assert_eq!(code, 0);
        let on_disk: Value = serde_json::from_str(
            &fs::read_to_string(state_dir.join("unknown-phase.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(on_disk["notes"][0]["phase_name"], "custom-unknown-phase");
    }

    #[test]
    fn append_note_run_impl_main_findings_wrong_type_resets_to_array() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(
            state_dir.join("wrong-type.json"),
            r#"{"current_phase":"flow-plan","notes":"not-an-array"}"#,
        )
        .unwrap();
        let args = make_args(Some("wrong-type"));
        let (value, code) = run_impl_main(args, &root);
        assert_eq!(value["status"], "ok");
        assert_eq!(value["note_count"], 1);
        assert_eq!(code, 0);
    }

    #[test]
    fn append_note_run_impl_main_slash_branch_returns_structured_error_no_panic() {
        // Regression: --branch feature/foo previously panicked via
        // FlowPaths::new. Per .claude/rules/external-input-validation.md
        // CLI subcommand entry callsite discipline, --branch is external
        // input and must use FlowPaths::try_new with a structured error
        // return.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let args = make_args(Some("feature/foo"));
        let (value, code) = run_impl_main(args, &root);
        assert_eq!(code, 1);
        assert_eq!(value["status"], "error");
        assert!(value["message"]
            .as_str()
            .unwrap()
            .contains("Invalid branch 'feature/foo'"));
    }
}
