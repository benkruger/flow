use std::io::Read;
use std::path::Path;

use serde_json::{json, Value};

use crate::flow_paths::FlowPaths;
use crate::git::{project_root, resolve_branch};
use crate::lock::mutate_state;
use crate::utils::now;

/// Capture StopFailure event data into the state file.
///
/// Writes `_last_failure` object with type, message, and timestamp.
/// Requires error_type key in hook_input to confirm this is a real
/// StopFailure event.
pub fn capture_failure_data(hook_input: &Value, state_path: &Path) {
    if hook_input.get("error_type").is_none() {
        return;
    }

    let error_type = hook_input
        .get("error_type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let error_message = hook_input
        .get("error_message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let timestamp = now();

    let _ = mutate_state(state_path, |state| {
        // Guard: state must be an object (or Null, which auto-converts)
        // for string-key mutations. Arrays/bools/numbers/strings would
        // panic on `state["_last_failure"] = v`. Fail-open.
        if !(state.is_object() || state.is_null()) {
            return;
        }
        state["_last_failure"] = json!({
            "type": error_type,
            "message": error_message,
            "timestamp": timestamp,
        });
    });
}

/// Run the stop-failure hook (entry point).
///
/// Uses `resolve_branch` for `--branch` override support. Calls
/// `current_branch()` internally — does not scan `.flow-states/`.
pub fn run() {
    let mut stdin_buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut stdin_buf);

    let hook_input: Value = match serde_json::from_str(&stdin_buf) {
        Ok(v) => v,
        Err(_) => return,
    };

    let root = project_root();
    let branch = resolve_branch(None, &root);
    let branch = match branch {
        Some(b) => b,
        None => return,
    };

    let state_path = FlowPaths::new(&root, &branch).state_file();

    if !state_path.exists() {
        return;
    }

    capture_failure_data(&hook_input, &state_path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
}
