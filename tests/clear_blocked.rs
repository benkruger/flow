//! Integration tests for `flow-rs clear-blocked` command and the
//! `clear_blocked` library function.
//!
//! Subprocess tests exercise `run()` end-to-end; unit tests exercise
//! `clear_blocked()` through the public library surface per
//! `.claude/rules/test-placement.md` (migrated from the pre-existing
//! inline `#[cfg(test)]` module in `src/commands/clear_blocked.rs`).

mod common;

use std::fs;
use std::process::{Command, Stdio};

use common::flow_states_dir;
use flow_rs::commands::clear_blocked::clear_blocked;
use serde_json::{json, Value};

fn flow_rs() -> Command {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
}

fn setup_git_and_state(dir: &std::path::Path, branch: &str, state: &Value) {
    let _ = Command::new("git").args(["init"]).current_dir(dir).output();
    let branch_dir = flow_states_dir(dir).join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(
        branch_dir.join("state.json"),
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
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

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

    let content = fs::read_to_string(
        flow_states_dir(dir.path())
            .join("test-feature")
            .join("state.json"),
    )
    .unwrap();
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
fn test_hook_no_current_branch_exits_zero() {
    // When neither FLOW_SIMULATE_BRANCH nor a git branch is available,
    // `current_branch()` returns None and `run()` returns early via
    // `None => return`. Must exit 0 without touching any state.
    let dir = tempfile::tempdir().unwrap();
    // No `git init` — current_branch() has no branch to report.
    let mut cmd = flow_rs();
    cmd.arg("clear-blocked")
        .current_dir(dir.path())
        .env_remove("FLOW_SIMULATE_BRANCH")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
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

    let content = fs::read_to_string(
        flow_states_dir(dir.path())
            .join("test-feature")
            .join("state.json"),
    )
    .unwrap();
    let on_disk: Value = serde_json::from_str(&content).unwrap();
    assert!(on_disk.get("_blocked").is_none());
    assert_eq!(on_disk["session_id"], "existing-session");
    assert_eq!(on_disk["notes"][0]["note"], "a correction");
}

// --- Library-level unit tests (migrated from src/commands/clear_blocked.rs) ---

#[test]
fn test_clears_blocked_flag() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test", "_blocked": "2026-01-01T10:00:00-08:00"});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    clear_blocked(&path);

    let content = fs::read_to_string(&path).unwrap();
    let state: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(state.get("_blocked").is_none());
}

#[test]
fn test_no_blocked_flag_noop() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test", "current_phase": "flow-code"});
    fs::write(&path, serde_json::to_string_pretty(&initial).unwrap()).unwrap();
    let original = fs::read_to_string(&path).unwrap();

    clear_blocked(&path);

    let after = fs::read_to_string(&path).unwrap();
    assert_eq!(original, after);
}

#[test]
fn test_no_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.json");
    // Should not panic
    clear_blocked(&path);
}

#[test]
fn test_corrupt_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, "{bad json").unwrap();
    // Should not panic
    clear_blocked(&path);
}

#[test]
fn test_preserves_other_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({
        "branch": "test",
        "_blocked": "2026-01-01T10:00:00-08:00",
        "session_id": "existing-session",
        "notes": [{"note": "a correction"}]
    });
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    clear_blocked(&path);

    let content = fs::read_to_string(&path).unwrap();
    let state: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(state.get("_blocked").is_none());
    assert_eq!(state["session_id"], "existing-session");
    assert_eq!(state["notes"][0]["note"], "a correction");
}

#[test]
fn test_array_root_is_safe_noop() {
    // mutate_state pretty-prints any valid JSON including arrays.
    // clear_blocked uses state.as_object_mut() which returns None for
    // arrays, so the closure skips the remove step cleanly.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, "[1, 2, 3]").unwrap();
    // Must not panic — just no-ops.
    clear_blocked(&path);
    // File content is still a valid JSON array.
    let after: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(after.is_array());
}

#[test]
fn test_null_root_is_safe_noop() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, "null").unwrap();
    // Must not panic — `null.as_object_mut()` returns None.
    clear_blocked(&path);
}
