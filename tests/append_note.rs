//! Integration tests for `bin/flow append-note`.
//!
//! Appends a note to the current branch's state file.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_git_repo_with_remote, parse_output};
use serde_json::{json, Value};

fn write_state(repo: &Path, branch: &str, state: &Value) -> std::path::PathBuf {
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let path = state_dir.join(format!("{}.json", branch));
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
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("bad.json"), "not json").unwrap();

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
