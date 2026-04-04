use std::io::Read;
use std::path::Path;

use serde_json::Value;

use crate::git::{current_branch, project_root};
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
        if let Some(summary) = hook_input.get("compact_summary").and_then(|v| v.as_str()) {
            if !summary.is_empty() {
                state["compact_summary"] = Value::String(summary.to_string());
            }
        }
        if let Some(cwd) = hook_input.get("cwd").and_then(|v| v.as_str()) {
            state["compact_cwd"] = Value::String(cwd.to_string());
        }
        let count = state
            .get("compact_count")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        state["compact_count"] = Value::Number((count + 1).into());
    });
}

/// Run the post-compact hook (entry point).
pub fn run() {
    let mut stdin_buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut stdin_buf);

    let hook_input: Value = match serde_json::from_str(&stdin_buf) {
        Ok(v) => v,
        Err(_) => return,
    };

    let branch = match current_branch() {
        Some(b) => b,
        None => return,
    };

    let root = project_root();
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
}
