use std::io::Read;
use std::path::Path;

use crate::flow_paths::FlowPaths;
use crate::git::{current_branch, project_root};
use crate::lock::mutate_state;

/// Clear _blocked flag from the state file. Fail-open: any error exits 0.
pub fn clear_blocked(state_path: &Path) {
    if !state_path.exists() {
        return;
    }
    let _ = mutate_state(state_path, |state| {
        if let Some(obj) = state.as_object_mut() {
            obj.remove("_blocked");
        }
    });
}

/// Run the clear-blocked command (hook entry point).
pub fn run() {
    // Read stdin best-effort (hook sends JSON context)
    let mut _stdin = String::new();
    let _ = std::io::stdin().read_to_string(&mut _stdin);

    let branch = match current_branch() {
        Some(b) => b,
        None => return,
    };

    let root = project_root();
    let state_path = FlowPaths::new(&root, &branch).state_file();

    clear_blocked(&state_path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

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
}
