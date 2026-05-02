//! Integration tests for `bin/flow append-note` and its library surface.
//!
//! Migrated from inline `#[cfg(test)]` per
//! `.claude/rules/test-placement.md`.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_git_repo_with_remote, parse_output};
use flow_rs::append_note::{read_current_phase, run_impl_main, Args};
use flow_rs::lock::mutate_state;
use flow_rs::utils::now;
use serde_json::{json, Value};

fn write_state(repo: &Path, branch: &str, state: &Value) -> std::path::PathBuf {
    let branch_dir = repo.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    let path = branch_dir.join("state.json");
    fs::write(&path, serde_json::to_string_pretty(state).unwrap()).unwrap();
    path
}

fn run_append_note(repo: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("append-note")
        .args(args)
        .current_dir(repo)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

#[test]
fn append_note_no_branch_no_git_returns_branch_resolution_error() {
    // Subprocess cwd is a non-git tempdir AND no --branch override.
    // resolve_branch falls back to current_branch() which returns None
    // for non-git dirs → run_impl_main surfaces the branch error.
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("append-note")
        .args(["--note", "n"])
        .current_dir(dir.path())
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .env("GIT_CEILING_DIRECTORIES", dir.path())
        .env_remove("FLOW_SIMULATE_BRANCH")
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Could not determine current branch"));
}

#[test]
fn append_note_records_correction() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({"branch": "b", "current_phase": "flow-plan", "notes": []});
    let state_path = write_state(&repo, "b", &state);

    let output = run_append_note(
        &repo,
        &[
            "--note",
            "Forgot to check the rule file",
            "--type",
            "correction",
            "--branch",
            "b",
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["note_count"], 1);

    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    let notes = on_disk["notes"].as_array().unwrap();
    assert_eq!(notes[0]["type"], "correction");
    assert_eq!(notes[0]["phase"], "flow-plan");
    assert_eq!(notes[0]["phase_name"], "Plan");
    assert_eq!(notes[0]["note"], "Forgot to check the rule file");
}

#[test]
fn append_note_default_type_is_correction() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({"branch": "d", "current_phase": "flow-code", "notes": []});
    let state_path = write_state(&repo, "d", &state);

    let output = run_append_note(&repo, &["--note", "default-typed note", "--branch", "d"]);

    assert_eq!(output.status.code(), Some(0));
    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(on_disk["notes"][0]["type"], "correction");
}

#[test]
fn append_note_learning_type_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({"branch": "l", "current_phase": "flow-learn", "notes": []});
    let state_path = write_state(&repo, "l", &state);

    let output = run_append_note(
        &repo,
        &[
            "--note",
            "New pattern discovered",
            "--type",
            "learning",
            "--branch",
            "l",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(on_disk["notes"][0]["type"], "learning");
}

#[test]
fn append_note_invalid_type_rejected_by_clap() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({"branch": "x", "current_phase": "flow-code", "notes": []});
    write_state(&repo, "x", &state);

    let output = run_append_note(
        &repo,
        &["--note", "n", "--type", "invalid-type", "--branch", "x"],
    );

    assert_ne!(output.status.code(), Some(0));
}

#[test]
fn append_note_no_state_file_returns_no_state() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());

    let output = run_append_note(
        &repo,
        &["--note", "n", "--type", "correction", "--branch", "missing"],
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "no_state");
}

#[test]
fn append_note_creates_array_if_missing() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({"branch": "c", "current_phase": "flow-code"});
    let state_path = write_state(&repo, "c", &state);

    let output = run_append_note(
        &repo,
        &[
            "--note",
            "first note",
            "--type",
            "correction",
            "--branch",
            "c",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(on_disk["notes"].as_array().unwrap().len(), 1);
}

#[test]
fn append_note_missing_current_phase_defaults_to_flow_start() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // State without current_phase — read_current_phase defaults to flow-start.
    let state = json!({"branch": "s", "notes": []});
    let state_path = write_state(&repo, "s", &state);

    let output = run_append_note(
        &repo,
        &["--note", "ok", "--type", "correction", "--branch", "s"],
    );

    assert_eq!(output.status.code(), Some(0));
    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(on_disk["notes"][0]["phase"], "flow-start");
    assert_eq!(on_disk["notes"][0]["phase_name"], "Start");
}

#[test]
fn append_note_corrupt_state_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let branch_dir = repo.join(".flow-states").join("bad");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), "not json").unwrap();

    let output = run_append_note(
        &repo,
        &["--note", "n", "--type", "correction", "--branch", "bad"],
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Could not read state file"));
}

// --- Library-level tests (migrated from inline `#[cfg(test)]`) ---

fn make_state_lib(branch: &str) -> Value {
    json!({
        "schema_version": 1,
        "branch": branch,
        "current_phase": "flow-plan",
        "notes": []
    })
}

fn write_state_lib(dir: &Path, branch: &str, state: &Value) -> std::path::PathBuf {
    let branch_dir = dir.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    let path = branch_dir.join("state.json");
    fs::write(&path, serde_json::to_string_pretty(state).unwrap()).unwrap();
    path
}

fn make_args(branch: Option<&str>) -> Args {
    Args {
        note: "test note".to_string(),
        note_type: "correction".to_string(),
        branch: branch.map(|s| s.to_string()),
    }
}

#[test]
fn append_note_happy_path_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let state = make_state_lib("test-feature");
    let path = write_state_lib(&root, "test-feature", &state);

    let args = Args {
        note: "test note".to_string(),
        note_type: "correction".to_string(),
        branch: Some("test-feature".to_string()),
    };

    let (value, code) = run_impl_main(args, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["note_count"], 1);

    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let notes = on_disk["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0]["phase"], "flow-plan");
    assert_eq!(notes[0]["phase_name"], "Plan");
    assert_eq!(notes[0]["type"], "correction");
    assert_eq!(notes[0]["note"], "test note");
    assert!(notes[0]["timestamp"].as_str().unwrap().contains("T"));
}

#[test]
fn append_note_multiple_accumulate_lib() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_state_lib("test-feature");
    let path = write_state_lib(dir.path(), "test-feature", &state);

    for i in 0..3 {
        mutate_state(&path, &mut |s| {
            s["notes"]
                .as_array_mut()
                .expect("array in fixture")
                .push(json!({
                    "phase": "flow-code",
                    "phase_name": "Code",
                    "timestamp": now(),
                    "type": "correction",
                    "note": format!("note {}", i),
                }));
        })
        .unwrap();
    }

    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(on_disk["notes"].as_array().unwrap().len(), 3);
}

#[test]
fn append_note_creates_array_if_missing_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("test-feature");
    fs::create_dir_all(&branch_dir).unwrap();
    let path = branch_dir.join("state.json");
    fs::write(&path, r#"{"current_phase": "flow-code"}"#).unwrap();

    let args = Args {
        note: "first".to_string(),
        note_type: "correction".to_string(),
        branch: Some("test-feature".to_string()),
    };

    let (value, code) = run_impl_main(args, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["note_count"], 1);
}

#[test]
fn append_note_preserves_existing_lib() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_state_lib("test-feature");
    state["notes"] = json!([{"phase": "flow-start", "note": "existing"}]);
    let path = write_state_lib(dir.path(), "test-feature", &state);

    mutate_state(&path, &mut |s| {
        s["notes"]
            .as_array_mut()
            .expect("array in fixture")
            .push(json!({"phase": "flow-code", "note": "new"}));
    })
    .unwrap();

    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let notes = on_disk["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0]["note"], "existing");
    assert_eq!(notes[1]["note"], "new");
}

#[test]
fn read_current_phase_success_lib() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, r#"{"current_phase": "flow-learn"}"#).unwrap();
    assert_eq!(read_current_phase(&path), Some("flow-learn".to_string()));
}

#[test]
fn read_current_phase_missing_file_lib() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.json");
    assert_eq!(read_current_phase(&path), None);
}

#[test]
fn read_current_phase_missing_key_defaults_to_flow_start_lib() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, r#"{"branch": "test"}"#).unwrap();
    assert_eq!(read_current_phase(&path), Some("flow-start".to_string()));
}

#[test]
fn read_current_phase_corrupt_json_lib() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, "{corrupt").unwrap();
    assert_eq!(read_current_phase(&path), None);
}

#[test]
fn append_note_array_root_state_noop_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("test-feature");
    fs::create_dir_all(&branch_dir).unwrap();
    let path = branch_dir.join("state.json");
    fs::write(&path, "[1, 2, 3]").unwrap();

    let args = Args {
        note: "should not appear".to_string(),
        note_type: "correction".to_string(),
        branch: Some("test-feature".to_string()),
    };

    let (value, code) = run_impl_main(args, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["note_count"], 0);

    let after = fs::read_to_string(&path).unwrap();
    let parsed: Value = serde_json::from_str(&after).unwrap();
    assert!(parsed.is_array());
    assert_eq!(parsed.as_array().unwrap().len(), 3);
}

#[test]
fn append_note_run_impl_main_no_state_returns_no_state_tuple_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let args = make_args(Some("missing-branch"));
    let (value, code) = run_impl_main(args, &root);
    assert_eq!(value["status"], "no_state");
    assert_eq!(code, 0);
}

#[test]
fn append_note_run_impl_main_success_returns_note_count_tuple_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("present-branch");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(
        branch_dir.join("state.json"),
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
fn append_note_run_impl_main_state_read_failure_returns_error_tuple_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("present-branch");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), "{not json").unwrap();
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
fn append_note_run_impl_main_mutate_state_failure_returns_error_tuple_lib() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("present-branch");
    fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    fs::write(&state_path, r#"{"current_phase":"flow-plan","notes":[]}"#).unwrap();
    let mut perms = fs::metadata(&state_path).unwrap().permissions();
    perms.set_mode(0o444);
    fs::set_permissions(&state_path, perms).unwrap();

    let args = make_args(Some("present-branch"));
    let (value, code) = run_impl_main(args, &root);

    let mut p = fs::metadata(&state_path).unwrap().permissions();
    p.set_mode(0o644);
    let _ = fs::set_permissions(&state_path, p);

    assert_eq!(value["status"], "error");
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Failed to append note"));
}

#[test]
fn append_note_run_impl_main_unknown_phase_falls_back_to_phase_string_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("unknown-phase");
    fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    fs::write(
        &state_path,
        r#"{"current_phase":"custom-unknown-phase","notes":[]}"#,
    )
    .unwrap();
    let args = make_args(Some("unknown-phase"));
    let (value, code) = run_impl_main(args, &root);
    assert_eq!(value["status"], "ok");
    assert_eq!(code, 0);
    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(on_disk["notes"][0]["phase_name"], "custom-unknown-phase");
}

#[test]
fn append_note_run_impl_main_wrong_type_resets_to_array_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("wrong-type");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(
        branch_dir.join("state.json"),
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
