use std::io::Read;
use std::path::Path;

use serde_json::Value;

use crate::git::{current_branch, project_root};
use crate::lock::mutate_state;
use crate::utils::now;

/// Set _blocked flag in the state file. Fail-open: any error exits 0.
pub fn set_blocked(state_path: &Path) {
    if !state_path.exists() {
        return;
    }
    let _ = mutate_state(state_path, |state| {
        // Guard: state must be an object (or Null, which auto-converts)
        // for string-key mutations. Arrays and primitives would panic.
        // Fail-open on any non-writable shape.
        if !(state.is_object() || state.is_null()) {
            return;
        }
        state["_blocked"] = Value::String(now());
    });
}

/// Run the set-blocked command (hook entry point).
pub fn run() {
    // Read stdin best-effort (hook sends JSON context)
    let mut _stdin = String::new();
    let _ = std::io::stdin().read_to_string(&mut _stdin);

    let branch = match current_branch() {
        Some(b) => b,
        None => return,
    };

    let root = project_root();
    let state_path = root.join(".flow-states").join(format!("{}.json", branch));

    set_blocked(&state_path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;
    use serde_json::json;
    use std::fs;

    fn iso_pattern() -> Regex {
        Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[Z+-]").unwrap()
    }

    #[test]
    fn test_set_blocked_sets_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, r#"{"branch": "test", "current_phase": "flow-code"}"#).unwrap();

        set_blocked(&path);

        let content = fs::read_to_string(&path).unwrap();
        let state: Value = serde_json::from_str(&content).unwrap();
        assert!(state.get("_blocked").is_some());
        assert!(iso_pattern().is_match(state["_blocked"].as_str().unwrap()));
    }

    #[test]
    fn test_set_blocked_preserves_other_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let initial = json!({
            "branch": "test",
            "session_id": "existing-session",
            "notes": [{"note": "a correction"}]
        });
        fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

        set_blocked(&path);

        let content = fs::read_to_string(&path).unwrap();
        let state: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(state["session_id"], "existing-session");
        assert_eq!(state["notes"][0]["note"], "a correction");
        assert!(state.get("_blocked").is_some());
    }

    #[test]
    fn test_set_blocked_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let initial = json!({"_blocked": "2026-01-01T10:00:00-08:00"});
        fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

        set_blocked(&path);

        let content = fs::read_to_string(&path).unwrap();
        let state: Value = serde_json::from_str(&content).unwrap();
        assert_ne!(state["_blocked"], "2026-01-01T10:00:00-08:00");
    }

    #[test]
    fn test_set_blocked_no_state_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        // Should not panic
        set_blocked(&path);
    }

    #[test]
    fn test_set_blocked_corrupt_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, "{bad json").unwrap();
        // Should not panic
        set_blocked(&path);
    }
}
