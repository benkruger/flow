use std::io::Read;
use std::path::Path;

use serde_json::Value;

use crate::git::{project_root, resolve_branch};
use crate::lock::mutate_state;

/// Capture compaction data into the state file.
///
/// Writes compact_summary (if non-empty), compact_cwd (if present),
/// and increments compact_count. Requires compact_summary key in
/// hook_input to confirm this is a real PostCompact event.
pub fn capture_compact_data(hook_input: &Value, state_path: &Path) {
    if hook_input.get("compact_summary").is_none() {
        return;
    }

    let _ = mutate_state(state_path, |state| {
        // Guard: state must be an object (or Null, which serde_json's
        // IndexMut auto-converts to an empty object) for string-key
        // mutations to succeed. Arrays, bools, numbers, and top-level
        // strings would panic on `state["key"] = v`. Fail-open on
        // any non-writable shape.
        if !(state.is_object() || state.is_null()) {
            return;
        }
        if let Some(summary) = hook_input.get("compact_summary").and_then(|v| v.as_str()) {
            if !summary.is_empty() {
                state["compact_summary"] = Value::String(summary.to_string());
            }
        }
        if let Some(cwd) = hook_input.get("cwd").and_then(|v| v.as_str()) {
            state["compact_cwd"] = Value::String(cwd.to_string());
        }
        // Accept compact_count written by any prior version: integers,
        // floats (3.0 from older Python writes), or strings ("3" from
        // corrupted/foreign edits). All resolve to the same canonical
        // i64 increment instead of silently resetting to 1.
        let count = state
            .get("compact_count")
            .and_then(|v| {
                v.as_i64()
                    .or_else(|| v.as_f64().map(|f| f as i64))
                    .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
            })
            .unwrap_or(0);
        state["compact_count"] = Value::Number((count + 1).into());
    });
}

/// Run the post-compact hook (entry point).
///
/// Uses `resolve_branch` for the `--branch` override support and state
/// file existence check. Falls back to `current_branch()` internally.
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

    let state_path = root.join(".flow-states").join(format!("{}.json", branch));

    if !state_path.exists() {
        return;
    }

    capture_compact_data(&hook_input, &state_path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

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
        assert_eq!(state["compact_summary"], "User was writing tests for webhook handler.");
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
        // Older Python writes or foreign edits may have compact_count
        // as a string "3". Accept it and increment to 4 instead of
        // silently resetting to 1.
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
}
