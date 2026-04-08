//! Integration tests for `flow-rs clear-blocked` command.

use std::fs;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

fn flow_rs() -> Command {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
}

fn setup_git_and_state(dir: &std::path::Path, branch: &str, state: &Value) {
    let _ = Command::new("git").args(["init"]).current_dir(dir).output();
    let state_dir = dir.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join(format!("{}.json", branch)),
        serde_json::to_string_pretty(state).unwrap(),
    )
    .unwrap();
}

fn run_clear_blocked(
    dir: &std::path::Path,
    branch: &str,
    stdin_data: &[u8],
) -> std::process::Output {
    let mut cmd = flow_rs();
    cmd.arg("clear-blocked")
        .env("FLOW_SIMULATE_BRANCH", branch)
        .current_dir(dir)
        .stdin(Stdio::piped());

    let mut child = cmd.spawn().unwrap();
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(stdin_data).unwrap();
    }
    child.wait_with_output().unwrap()
}

#[test]
fn test_hook_clears_blocked_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "branch": "test-feature",
        "current_phase": "flow-code",
        "_blocked": "2026-01-01T10:00:00-08:00"
    });
    setup_git_and_state(dir.path(), "test-feature", &state);

    let output = run_clear_blocked(
        dir.path(),
        "test-feature",
        b"{\"tool_name\": \"AskUserQuestion\"}",
    );

    assert_eq!(output.status.code().unwrap(), 0);
    assert!(output.stdout.is_empty());

    let content = fs::read_to_string(dir.path().join(".flow-states/test-feature.json")).unwrap();
    let on_disk: Value = serde_json::from_str(&content).unwrap();
    assert!(on_disk.get("_blocked").is_none());
}

#[test]
fn test_hook_no_state_file_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let _ = Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output();

    let output = run_clear_blocked(dir.path(), "test-feature", b"{}");
    assert_eq!(output.status.code().unwrap(), 0);
    assert!(output.stdout.is_empty());
}

#[test]
fn test_hook_malformed_stdin_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let _ = Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output();

    let output = run_clear_blocked(dir.path(), "test-feature", b"not json");
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn test_hook_preserves_other_fields() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "branch": "test-feature",
        "current_phase": "flow-code",
        "_blocked": "2026-01-01T10:00:00-08:00",
        "session_id": "existing-session",
        "notes": [{"note": "a correction"}]
    });
    setup_git_and_state(dir.path(), "test-feature", &state);

    let output = run_clear_blocked(dir.path(), "test-feature", b"{}");
    assert_eq!(output.status.code().unwrap(), 0);

    let content = fs::read_to_string(dir.path().join(".flow-states/test-feature.json")).unwrap();
    let on_disk: Value = serde_json::from_str(&content).unwrap();
    assert!(on_disk.get("_blocked").is_none());
    assert_eq!(on_disk["session_id"], "existing-session");
    assert_eq!(on_disk["notes"][0]["note"], "a correction");
}
