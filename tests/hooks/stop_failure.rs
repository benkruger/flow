//! Tests for `flow_rs::hooks::stop_failure`.
//!
//! Library-level tests exercise `capture_failure_data`. Subprocess tests
//! at the bottom drive `run()` end-to-end via `bin/flow hook
//! stop-failure` so every branch of the entry point is covered by the
//! per-file gate.

use std::fs;
use std::path::Path;
use std::process::Command;

use flow_rs::hooks::stop_failure::capture_failure_data;
use serde_json::{json, Value};

fn run_stop_failure(cwd: &Path, stdin: &[u8]) -> std::process::Output {
    crate::common::spawn_hook("stop-failure", cwd, stdin, &[])
}

fn init_git(dir: &Path, branch: &str) {
    let run = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git command");
    };
    run(&["init", "--initial-branch", branch]);
    run(&["config", "user.email", "a@b"]);
    run(&["config", "user.name", "t"]);
    run(&["config", "commit.gpgsign", "false"]);
    run(&["commit", "--allow-empty", "-m", "init"]);
}

#[test]
fn test_writes_failure_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test", "current_phase": "flow-code"});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let input = json!({
        "error_type": "rate_limit",
        "error_message": "429 Too Many Requests"
    });
    capture_failure_data(&input, &path);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let failure = &state["_last_failure"];
    assert_eq!(failure["type"], "rate_limit");
    assert_eq!(failure["message"], "429 Too Many Requests");
    assert!(failure.get("timestamp").is_some());
    assert!(!failure["timestamp"].as_str().unwrap().is_empty());
}

#[test]
fn test_no_error_type_key_skips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test"});
    fs::write(&path, serde_json::to_string_pretty(&initial).unwrap()).unwrap();
    let original = fs::read_to_string(&path).unwrap();

    let input = json!({"error_message": "some error"});
    capture_failure_data(&input, &path);

    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn test_preserves_existing_state_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({
        "branch": "test",
        "session_id": "existing-session",
        "notes": [{"note": "a correction"}]
    });
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let input = json!({
        "error_type": "rate_limit",
        "error_message": "429 Too Many Requests"
    });
    capture_failure_data(&input, &path);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state["session_id"], "existing-session");
    assert_eq!(state["notes"][0]["note"], "a correction");
    assert_eq!(state["_last_failure"]["type"], "rate_limit");
}

#[test]
fn test_overwrites_previous_failure() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({
        "branch": "test",
        "_last_failure": {
            "type": "old_error",
            "message": "Old message",
            "timestamp": "2026-01-01T00:00:00-08:00"
        }
    });
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let input = json!({
        "error_type": "network_timeout",
        "error_message": "Connection timed out"
    });
    capture_failure_data(&input, &path);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state["_last_failure"]["type"], "network_timeout");
    assert_eq!(state["_last_failure"]["message"], "Connection timed out");
}

#[test]
fn test_missing_error_message_defaults_to_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test"});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let input = json!({"error_type": "auth_failure"});
    capture_failure_data(&input, &path);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state["_last_failure"]["type"], "auth_failure");
    assert_eq!(state["_last_failure"]["message"], "");
}

#[test]
fn test_no_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.json");
    let input = json!({"error_type": "rate_limit", "error_message": "429"});
    // Should not panic
    capture_failure_data(&input, &path);
}

#[test]
fn test_corrupt_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, "{bad json").unwrap();

    let input = json!({"error_type": "rate_limit", "error_message": "429"});
    // Should not panic
    capture_failure_data(&input, &path);
}

#[test]
fn test_array_state_file_does_not_crash() {
    // An array-shaped state file must not panic. The
    // `is_object() || is_null()` guard catches it before the
    // mutation attempt that would otherwise panic in serde_json's
    // IndexMut on `value["_last_failure"] = v`.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, r#"["not", "an", "object"]"#).unwrap();

    let input = json!({"error_type": "rate_limit", "error_message": "429"});
    capture_failure_data(&input, &path);

    let after: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(after.is_array());
}

// --- Subprocess tests for `run()` entry point ---

/// `run()` returns silently when stdin is unparseable JSON.
#[test]
fn run_subprocess_exits_0_when_stdin_unparseable() {
    let dir = tempfile::tempdir().unwrap();
    let output = run_stop_failure(dir.path(), b"not valid json");
    assert_eq!(output.status.code(), Some(0));
}

/// `run()` returns silently when there is no current git branch.
#[test]
fn run_subprocess_exits_0_when_no_git_branch() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let output = run_stop_failure(&root, br#"{"error_type":"rate_limit"}"#);
    assert_eq!(output.status.code(), Some(0));
}

/// `run()` returns silently when the current branch contains `/` —
/// `FlowPaths::try_new` rejects slash branches.
#[test]
fn run_subprocess_exits_0_when_branch_has_slash() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    init_git(&root, "feature/foo");
    let output = run_stop_failure(&root, br#"{"error_type":"rate_limit"}"#);
    assert_eq!(output.status.code(), Some(0));
}

/// `run()` returns silently when no state file exists for the
/// current branch.
#[test]
fn run_subprocess_exits_0_when_state_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    init_git(&root, "feat");
    let output = run_stop_failure(&root, br#"{"error_type":"rate_limit"}"#);
    assert_eq!(output.status.code(), Some(0));
}

/// `run()` happy path: state file exists; capture_failure_data writes
/// `_last_failure` with type/message/timestamp.
#[test]
fn run_subprocess_success_writes_last_failure_to_state() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    init_git(&root, "feat");
    let branch_dir = root.join(".flow-states").join("feat");
    fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    fs::write(
        &state_path,
        serde_json::to_string(&json!({"branch": "feat"})).unwrap(),
    )
    .unwrap();

    let output = run_stop_failure(
        &root,
        br#"{"error_type":"rate_limit","error_message":"429 too many"}"#,
    );
    assert_eq!(output.status.code(), Some(0));

    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["_last_failure"]["type"], "rate_limit");
    assert_eq!(state["_last_failure"]["message"], "429 too many");
    assert!(state["_last_failure"]["timestamp"].is_string());
}
