//! Integration tests for `flow-rs set-blocked` command.

mod common;

use std::fs;
use std::process::{Command, Stdio};

use common::flow_states_dir;
use regex::Regex;
use serde_json::{json, Value};

fn flow_rs() -> Command {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
}

fn iso_pattern() -> Regex {
    Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[Z+-]").unwrap()
}

fn setup_git_and_state(dir: &std::path::Path, branch: &str, state: &Value) {
    let _ = Command::new("git").args(["init"]).current_dir(dir).output();
    let state_dir = flow_states_dir(dir);
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join(format!("{}.json", branch)),
        serde_json::to_string_pretty(state).unwrap(),
    )
    .unwrap();
}

#[test]
fn test_hook_sets_blocked_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({"branch": "test-feature", "current_phase": "flow-code"});
    setup_git_and_state(dir.path(), "test-feature", &state);

    let mut cmd = flow_rs();
    cmd.arg("set-blocked")
        .env("FLOW_SIMULATE_BRANCH", "test-feature")
        .current_dir(dir.path())
        .stdin(Stdio::piped());

    let mut child = cmd.spawn().unwrap();
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(b"{\"tool_name\": \"Bash\"}").unwrap();
    }
    let output = child.wait_with_output().unwrap();

    assert_eq!(output.status.code().unwrap(), 0);
    assert!(output.stdout.is_empty());

    let content =
        fs::read_to_string(flow_states_dir(dir.path()).join("test-feature.json")).unwrap();
    let on_disk: Value = serde_json::from_str(&content).unwrap();
    assert!(on_disk.get("_blocked").is_some());
    assert!(iso_pattern().is_match(on_disk["_blocked"].as_str().unwrap()));
}

#[test]
fn test_hook_no_state_file_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let _ = Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output();

    let mut cmd = flow_rs();
    cmd.arg("set-blocked")
        .env("FLOW_SIMULATE_BRANCH", "test-feature")
        .current_dir(dir.path())
        .stdin(Stdio::piped());

    let mut child = cmd.spawn().unwrap();
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(b"{}").unwrap();
    }
    let output = child.wait_with_output().unwrap();

    assert_eq!(output.status.code().unwrap(), 0);
    assert!(output.stdout.is_empty());
}

#[test]
fn test_hook_no_current_branch_exits_zero() {
    // No git, no FLOW_SIMULATE_BRANCH → current_branch returns None →
    // `None => return` arm in run().
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = flow_rs();
    cmd.arg("set-blocked")
        .current_dir(dir.path())
        .env_remove("FLOW_SIMULATE_BRANCH")
        .stdin(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(b"{}").unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn test_hook_malformed_stdin_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let _ = Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output();

    let mut cmd = flow_rs();
    cmd.arg("set-blocked")
        .env("FLOW_SIMULATE_BRANCH", "test-feature")
        .current_dir(dir.path())
        .stdin(Stdio::piped());

    let mut child = cmd.spawn().unwrap();
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(b"not json").unwrap();
    }
    let output = child.wait_with_output().unwrap();

    assert_eq!(output.status.code().unwrap(), 0);
}
