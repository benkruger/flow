//! Tests for `flow_rs::hooks::post_compact`.
//!
//! Library-level tests exercise `capture_compact_data` (the pure mutator).
//! Subprocess tests at the bottom drive `run()` end-to-end via
//! `bin/flow hook post-compact` so every branch of the entry point is
//! covered by the per-file gate.

use std::fs;
use std::path::Path;
use std::process::Command;

use flow_rs::hooks::post_compact::capture_compact_data;
use serde_json::{json, Value};

fn run_post_compact(cwd: &Path, stdin: &[u8]) -> std::process::Output {
    crate::common::spawn_hook("post-compact", cwd, stdin, &[])
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
fn test_writes_summary_and_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test", "current_phase": "flow-code"});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let input = json!({
        "compact_summary": "User was writing tests for webhook handler.",
        "cwd": "/Users/ben/code/myapp"
    });
    capture_compact_data(&input, &path);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        state["compact_summary"],
        "User was writing tests for webhook handler."
    );
    assert_eq!(state["compact_cwd"], "/Users/ben/code/myapp");
    assert_eq!(state["compact_count"], 1);
}

#[test]
fn test_increments_count_from_zero() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test", "current_phase": "flow-code"});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let input = json!({"compact_summary": "Working on feature."});
    capture_compact_data(&input, &path);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state["compact_count"], 1);
}

#[test]
fn test_increments_count_from_existing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test", "compact_count": 3});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let input = json!({"compact_summary": "Another compaction."});
    capture_compact_data(&input, &path);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state["compact_count"], 4);
}

#[test]
fn test_summary_only_no_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test"});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let input = json!({"compact_summary": "Just a summary."});
    capture_compact_data(&input, &path);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state["compact_summary"], "Just a summary.");
    assert!(state.get("compact_cwd").is_none());
    assert_eq!(state["compact_count"], 1);
}

#[test]
fn test_empty_summary_still_writes_cwd_and_count() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test"});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let input = json!({"compact_summary": "", "cwd": "/some/path"});
    capture_compact_data(&input, &path);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(state.get("compact_summary").is_none());
    assert_eq!(state["compact_cwd"], "/some/path");
    assert_eq!(state["compact_count"], 1);
}

#[test]
fn test_no_compact_summary_key_skips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test"});
    fs::write(&path, serde_json::to_string_pretty(&initial).unwrap()).unwrap();
    let original = fs::read_to_string(&path).unwrap();

    let input = json!({"cwd": "/some/path"});
    capture_compact_data(&input, &path);

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

    let input = json!({"compact_summary": "Summary."});
    capture_compact_data(&input, &path);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state["session_id"], "existing-session");
    assert_eq!(state["notes"][0]["note"], "a correction");
    assert_eq!(state["compact_summary"], "Summary.");
}

#[test]
fn test_no_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.json");
    let input = json!({"compact_summary": "Summary."});
    // Should not panic — mutate_state returns error, which we ignore
    capture_compact_data(&input, &path);
}

#[test]
fn test_corrupt_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, "{bad json").unwrap();

    let input = json!({"compact_summary": "Summary."});
    // Should not panic
    capture_compact_data(&input, &path);
}

// --- Adversarial findings: state file shape and compact_count type ---

#[test]
fn test_array_state_file_does_not_crash() {
    // An array-shaped state file (corrupted or foreign edit) must
    // not panic. serde_json's IndexMut panics on `value["key"] = v`
    // when value is an Array — the `is_object() || is_null()` guard
    // catches it before the mutation.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, r#"["not", "an", "object"]"#).unwrap();

    let input = json!({"compact_summary": "Testing array state."});
    capture_compact_data(&input, &path);

    // State file unchanged — no mutation happened.
    let after: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(after.is_array());
}

#[test]
fn test_compact_count_string_value_increments() {
    // A state file produced by a hand edit or a foreign tool may
    // store `compact_count` as the string `"3"`. The hook must
    // tolerate it and increment to 4 instead of silently
    // resetting the counter to 1.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"compact_count": "3"});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    capture_compact_data(&json!({"compact_summary": "Test"}), &path);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state["compact_count"], 4);
}

#[test]
fn test_compact_count_float_value_increments() {
    // Floats like 3.0 must increment to 4, not reset to 1.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"compact_count": 3.0});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    capture_compact_data(&json!({"compact_summary": "Test"}), &path);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state["compact_count"], 4);
}

#[test]
fn test_compact_summary_non_string_skips_summary_write() {
    // hook_input passes `is_none()` but the inner
    // `and_then(|v| v.as_str())` returns None for a non-string value,
    // so the if-let's None arm runs — compact_summary write is
    // skipped while compact_count still increments.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test"});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let input = json!({"compact_summary": 42});
    capture_compact_data(&input, &path);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(state.get("compact_summary").is_none());
    assert_eq!(state["compact_count"], 1);
}

#[test]
fn test_compact_count_unparseable_string_defaults_to_one() {
    // A string that cannot be parsed as an integer falls through
    // to the default 0, producing a fresh count of 1. This is
    // still better than panicking.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"compact_count": "not-a-number"});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    capture_compact_data(&json!({"compact_summary": "Test"}), &path);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state["compact_count"], 1);
}

// --- Subprocess tests for `run()` entry point ---

/// `run()` returns silently when stdin is unparseable JSON.
#[test]
fn run_subprocess_exits_0_when_stdin_unparseable() {
    let dir = tempfile::tempdir().unwrap();
    let output = run_post_compact(dir.path(), b"not valid json");
    assert_eq!(output.status.code(), Some(0));
}

/// `run()` returns silently when there is no current git branch.
#[test]
fn run_subprocess_exits_0_when_no_git_branch() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let output = run_post_compact(&root, br#"{"compact_summary":"x"}"#);
    assert_eq!(output.status.code(), Some(0));
}

/// `run()` returns silently when the current branch contains `/` —
/// `FlowPaths::try_new` rejects slash branches.
#[test]
fn run_subprocess_exits_0_when_branch_has_slash() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    init_git(&root, "feature/foo");
    let output = run_post_compact(&root, br#"{"compact_summary":"x"}"#);
    assert_eq!(output.status.code(), Some(0));
}

/// `run()` returns silently when no state file exists for the
/// current branch.
#[test]
fn run_subprocess_exits_0_when_state_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    init_git(&root, "feat");
    let output = run_post_compact(&root, br#"{"compact_summary":"x"}"#);
    assert_eq!(output.status.code(), Some(0));
}

/// `run()` happy path: state file exists; capture_compact_data writes
/// summary, cwd, and increments compact_count in the state file.
#[test]
fn run_subprocess_success_writes_compact_fields_to_state() {
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

    let output = run_post_compact(
        &root,
        br#"{"compact_summary":"after compact","cwd":"/some/dir"}"#,
    );
    assert_eq!(output.status.code(), Some(0));

    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["compact_summary"], "after compact");
    assert_eq!(state["compact_cwd"], "/some/dir");
    assert_eq!(state["compact_count"], 1);
}
