//! Integration tests for the start-step subcommand.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_git_repo_with_remote, parse_output};
use flow_rs::commands::start_step::{resolve_flow_bin, update_step};
use serde_json::{json, Value};

fn write_state(repo: &Path, branch: &str, state: &Value) -> std::path::PathBuf {
    let branch_dir = repo.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    let path = branch_dir.join("state.json");
    fs::write(&path, serde_json::to_string_pretty(state).unwrap()).unwrap();
    path
}

fn run_start_step(repo: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("start-step")
        .args(args)
        .current_dir(repo)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap()
}

#[test]
fn start_step_updates_state_and_prints_json() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({"branch": "feature", "current_phase": "flow-start"});
    let state_path = write_state(&repo, "feature", &state);

    let output = run_start_step(&repo, &["--step", "3", "--branch", "feature"]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["step"], 3);

    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(on_disk["start_step"], 3);
}

#[test]
fn start_step_no_state_file_reports_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());

    let output = run_start_step(&repo, &["--step", "2", "--branch", "missing"]);

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "skipped");
    assert!(data["reason"]
        .as_str()
        .unwrap_or("")
        .contains("no state file"));
}

#[test]
fn start_step_overwrites_previous_value() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({
        "branch": "b",
        "current_phase": "flow-start",
        "start_step": 1
    });
    let state_path = write_state(&repo, "b", &state);

    let output = run_start_step(&repo, &["--step", "5", "--branch", "b"]);

    assert_eq!(output.status.code(), Some(0));
    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(on_disk["start_step"], 5);
}

#[test]
fn start_step_preserves_other_state_fields() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({
        "branch": "p",
        "current_phase": "flow-start",
        "feature": "my feature",
        "prompt": "the prompt",
        "phases": {"flow-start": {"status": "in_progress"}},
    });
    let state_path = write_state(&repo, "p", &state);

    let output = run_start_step(&repo, &["--step", "4", "--branch", "p"]);

    assert_eq!(output.status.code(), Some(0));
    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(on_disk["start_step"], 4);
    assert_eq!(on_disk["feature"], "my feature");
    assert_eq!(on_disk["prompt"], "the prompt");
    assert_eq!(on_disk["phases"]["flow-start"]["status"], "in_progress");
}

#[test]
fn start_step_handles_corrupt_state_without_crash() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let branch_dir = repo.join(".flow-states").join("bad");
    fs::create_dir_all(&branch_dir).unwrap();
    // Non-JSON content — update_step should fail gracefully and report skipped.
    fs::write(branch_dir.join("state.json"), "not json").unwrap();

    let output = run_start_step(&repo, &["--step", "1", "--branch", "bad"]);

    // update_step returns false on mutate_state error, so run() reports skipped.
    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "skipped");
}

#[test]
fn start_step_exec_wrapping_enters_exec_path() {
    // Exercises the exec() wrapping path (lines 42-63 in start_step.rs).
    // In the test environment the binary lives under target/llvm-cov-target/
    // so the 3-parent bin/flow resolution points at a nonexistent path.
    // exec() fails and the error handler at lines 62-63 fires (eprintln +
    // exit 1). This covers the subcommand-wrapping branch and the exec
    // error handler.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({"branch": "feat", "current_phase": "flow-start"});
    write_state(&repo, "feat", &state);

    let output = run_start_step(&repo, &["--step", "1", "--branch", "feat", "--", "version"]);

    // exec() fails → eprintln! "Failed to exec" → exit 1
    assert_eq!(
        output.status.code(),
        Some(1),
        "exec should fail in test env; stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Failed to exec"),
        "stderr should contain the exec error message, got: {}",
        stderr
    );
}

// --- Library-level tests (migrated from src/commands/start_step.rs) ---

fn make_state_lib() -> Value {
    json!({
        "schema_version": 1,
        "branch": "test-feature",
        "current_phase": "flow-start",
        "files": {
            "plan": null,
            "dag": null,
            "log": ".flow-states/test-feature/log",
            "state": ".flow-states/test-feature/state.json"
        },
        "phases": {}
    })
}

#[test]
fn test_update_step_success() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(
        &path,
        serde_json::to_string_pretty(&make_state_lib()).unwrap(),
    )
    .unwrap();

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
fn update_step_array_state_returns_true_but_preserves_array() {
    // Exercises the non-object guard. When the state root is an array,
    // the guard fires and returns without writing start_step.
    // mutate_state itself succeeds (no IO error) so update_step returns
    // true — but the array root is preserved.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, "[]").unwrap();
    let result = update_step(&path, 5);
    assert!(result);
    let content = fs::read_to_string(&path).unwrap();
    let val: Value = serde_json::from_str(&content).unwrap();
    assert!(val.is_array(), "array root must be preserved");
}

#[test]
fn update_step_string_state_returns_true_but_preserves_string() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, "\"hello\"").unwrap();
    let result = update_step(&path, 5);
    assert!(result);
    let content = fs::read_to_string(&path).unwrap();
    let val: Value = serde_json::from_str(&content).unwrap();
    assert!(val.is_string(), "string root must be preserved");
}

#[test]
fn test_update_step_preserves_other_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let mut state = make_state_lib();
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
fn resolve_flow_bin_uses_three_parents_up_when_exe_ok() {
    let root = std::path::Path::new("/fallback");
    let exe = Ok(std::path::PathBuf::from("/repo/target/debug/flow-rs"));
    let resolved = resolve_flow_bin(exe, root);
    assert_eq!(resolved, std::path::PathBuf::from("/repo/bin/flow"));
}

#[test]
fn resolve_flow_bin_falls_back_to_project_root_when_exe_err() {
    let root = std::path::Path::new("/fallback");
    let exe = Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "simulated",
    ));
    let resolved = resolve_flow_bin(exe, root);
    assert_eq!(resolved, std::path::PathBuf::from("/fallback/bin/flow"));
}

#[test]
fn resolve_flow_bin_falls_back_to_project_root_when_exe_too_shallow() {
    // `ancestors().nth(3)` on a 2-component path returns None —
    // exercise the inner if-let fallback.
    let root = std::path::Path::new("/fallback");
    let exe = Ok(std::path::PathBuf::from("/flow-rs"));
    let resolved = resolve_flow_bin(exe, root);
    assert_eq!(resolved, std::path::PathBuf::from("/fallback/bin/flow"));
}

#[test]
fn test_update_step_overwrites_previous() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let mut state = make_state_lib();
    state["start_step"] = json!(3);
    fs::write(&path, serde_json::to_string_pretty(&state).unwrap()).unwrap();

    let result = update_step(&path, 8);
    assert!(result);

    let content = fs::read_to_string(&path).unwrap();
    let updated: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(updated["start_step"], 8);
}
