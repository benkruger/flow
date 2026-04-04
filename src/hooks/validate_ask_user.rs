//! PreToolUse hook for AskUserQuestion — enforces auto-continue.
//!
//! When `_auto_continue` is set in the state file, answers AskUserQuestion
//! automatically via `updatedInput` (JSON on stdout with exit 0). This
//! prevents the model from prompting the user when autonomous phase
//! transitions are configured.
//!
//! Exit 0 — allow (optionally with JSON on stdout for updatedInput)

use std::path::Path;

use serde_json::{json, Value};

use super::read_hook_input;
use crate::git::{current_branch, project_root};
use crate::lock::mutate_state;
use crate::utils::now;

/// Write `_blocked` timestamp to the state file.
///
/// Best-effort: any error is silently ignored so the hook never interferes
/// with AskUserQuestion delivery.
pub fn set_blocked(state_path: &Path) {
    if !state_path.exists() {
        return;
    }
    let _ = mutate_state(state_path, |state| {
        state["_blocked"] = Value::String(now());
    });
}

/// Check auto-continue state and return hook response if active.
///
/// Returns `(allowed, message, hook_response)`. When `hook_response` is
/// `Some`, the caller prints it as JSON to stdout so Claude Code receives it
/// as an `updatedInput` answer.
pub fn validate(state_path: Option<&Path>) -> (bool, String, Option<Value>) {
    let state_path = match state_path {
        Some(p) if p.exists() => p,
        _ => return (true, String::new(), None),
    };

    let content = match std::fs::read_to_string(state_path) {
        Ok(c) => c,
        Err(_) => return (true, String::new(), None),
    };

    let state: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return (true, String::new(), None),
    };

    let auto_cmd = state
        .get("_auto_continue")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if auto_cmd.is_empty() {
        return (true, String::new(), None);
    }

    (
        true,
        String::new(),
        Some(json!({
            "permissionDecision": "allow",
            "updatedInput": format!("Yes, proceed. Invoke {} now.", auto_cmd),
        })),
    )
}

/// Run the validate-ask-user hook (entry point from CLI).
pub fn run() {
    // Consume stdin (hook sends JSON but we don't need it)
    if read_hook_input().is_none() {
        std::process::exit(0);
    }

    let branch = match current_branch() {
        Some(b) => b,
        None => std::process::exit(0),
    };

    let state_path = project_root()
        .join(".flow-states")
        .join(format!("{}.json", branch));

    let (_allowed, _message, hook_response) = validate(Some(&state_path));
    if let Some(response) = hook_response {
        println!("{}", serde_json::to_string(&response).unwrap());
        std::process::exit(0);
    }

    set_blocked(&state_path);
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn write_state(dir: &Path, branch: &str, state: &Value) -> std::path::PathBuf {
        let state_dir = dir.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let path = state_dir.join(format!("{}.json", branch));
        fs::write(&path, serde_json::to_string_pretty(state).unwrap()).unwrap();
        path
    }

    // --- validate tests ---

    #[test]
    fn test_validate_allows_no_state_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let (allowed, msg, resp) = validate(Some(&path));
        assert!(allowed);
        assert!(msg.is_empty());
        assert!(resp.is_none());
    }

    #[test]
    fn test_validate_allows_none_state_path() {
        let (allowed, msg, resp) = validate(None);
        assert!(allowed);
        assert!(msg.is_empty());
        assert!(resp.is_none());
    }

    #[test]
    fn test_validate_allows_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let bad_file = dir.path().join("bad.json");
        fs::write(&bad_file, "not json at all").unwrap();
        let (allowed, msg, resp) = validate(Some(&bad_file));
        assert!(allowed);
        assert!(msg.is_empty());
        assert!(resp.is_none());
    }

    #[test]
    fn test_validate_allows_no_auto_continue() {
        let dir = tempfile::tempdir().unwrap();
        let state = json!({"current_phase": "flow-start", "branch": "test"});
        let path = write_state(dir.path(), "test", &state);
        let (allowed, msg, resp) = validate(Some(&path));
        assert!(allowed);
        assert!(msg.is_empty());
        assert!(resp.is_none());
    }

    #[test]
    fn test_validate_allows_empty_auto_continue() {
        let dir = tempfile::tempdir().unwrap();
        let state = json!({
            "current_phase": "flow-start",
            "branch": "test",
            "_auto_continue": "",
        });
        let path = write_state(dir.path(), "test", &state);
        let (allowed, msg, resp) = validate(Some(&path));
        assert!(allowed);
        assert!(msg.is_empty());
        assert!(resp.is_none());
    }

    #[test]
    fn test_validate_auto_continue_returns_hook_response() {
        let dir = tempfile::tempdir().unwrap();
        let state = json!({
            "current_phase": "flow-start",
            "branch": "test",
            "_auto_continue": "/flow:flow-plan",
        });
        let path = write_state(dir.path(), "test", &state);
        let (allowed, _msg, resp) = validate(Some(&path));
        assert!(allowed);
        assert!(resp.is_some());
        let resp = resp.unwrap();
        assert_eq!(resp["permissionDecision"], "allow");
        assert!(resp["updatedInput"]
            .as_str()
            .unwrap()
            .contains("/flow:flow-plan"));
    }

    #[test]
    fn test_validate_auto_continue_includes_command() {
        let dir = tempfile::tempdir().unwrap();
        let state = json!({
            "current_phase": "flow-code",
            "branch": "test",
            "_auto_continue": "/flow:flow-code-review",
        });
        let path = write_state(dir.path(), "test", &state);
        let (allowed, _msg, resp) = validate(Some(&path));
        assert!(allowed);
        assert!(resp.is_some());
        let resp = resp.unwrap();
        assert_eq!(resp["permissionDecision"], "allow");
        assert!(resp["updatedInput"]
            .as_str()
            .unwrap()
            .contains("/flow:flow-code-review"));
    }

    // --- set_blocked tests ---

    #[test]
    fn test_set_blocked_sets_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let state = json!({"current_phase": "flow-code", "branch": "test"});
        let path = write_state(dir.path(), "test", &state);
        set_blocked(&path);
        let updated: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(updated.get("_blocked").is_some());
        assert!(updated["_blocked"].as_str().unwrap().len() > 0);
    }

    #[test]
    fn test_set_blocked_no_state_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        // Should not panic
        set_blocked(&path);
    }

    #[test]
    fn test_set_blocked_corrupt_state() {
        let dir = tempfile::tempdir().unwrap();
        let bad_file = dir.path().join("bad.json");
        fs::write(&bad_file, "{bad json").unwrap();
        // Should not panic
        set_blocked(&bad_file);
    }

    #[test]
    fn test_set_blocked_preserves_other_fields() {
        let dir = tempfile::tempdir().unwrap();
        let state = json!({
            "current_phase": "flow-code",
            "branch": "test",
            "session_id": "existing-session",
            "notes": [{"note": "a correction"}],
        });
        let path = write_state(dir.path(), "test", &state);
        set_blocked(&path);
        let updated: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(updated["session_id"], "existing-session");
        assert_eq!(updated["notes"][0]["note"], "a correction");
        assert!(updated.get("_blocked").is_some());
    }
}
