//! Library-level tests for `flow_rs::hooks::stop_failure::capture_failure_data`.
//! Migrated from inline `#[cfg(test)]` per `.claude/rules/test-placement.md`.
//!
//! Subprocess tests for the `run()` entry point live in `tests/hooks.rs`
//! (the `stop_failure` section there).

use std::fs;

use flow_rs::hooks::stop_failure::capture_failure_data;
use serde_json::{json, Value};

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
