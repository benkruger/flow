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
use crate::flow_paths::FlowPaths;
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
        // Guard: Value::IndexMut panics on non-object types (arrays, bools, etc.)
        if !(state.is_object() || state.is_null()) {
            return;
        }
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

    // Block path: when the current phase is configured autonomous
    // (`skills.<current_phase>.continue == "auto"`), refuse the
    // AskUserQuestion tool call. This precedes the `_auto_continue`
    // auto-answer path so the user's explicit per-skill continue=auto
    // config wins over any transient transition-boundary state.
    let current_phase = state
        .get("current_phase")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !current_phase.is_empty() {
        let skill_entry = state.get("skills").and_then(|s| s.get(current_phase));
        let is_auto = match skill_entry {
            // SkillConfig::Simple — `skills.<phase> = "auto"`.
            Some(v) if v.as_str() == Some("auto") => true,
            // SkillConfig::Detailed — `skills.<phase> = {"continue": "auto", ...}`.
            Some(v) => v.get("continue").and_then(|c| c.as_str()) == Some("auto"),
            None => false,
        };
        if is_auto {
            return (
                false,
                format!(
                    "BLOCKED: AskUserQuestion is disabled in autonomous phase \
                     `{}`. Autonomous flows must not pause for user input. \
                     Commit any in-flight work at a natural boundary and \
                     continue with the next skill instruction. To capture a \
                     correction, the user can run `/flow:flow-note`.",
                    current_phase
                ),
                None,
            );
        }
    }

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

    let state_path = FlowPaths::new(project_root(), &branch).state_file();

    let (allowed, message, hook_response) = validate(Some(&state_path));
    // Block path: exit 2 with stderr message so Claude Code feeds it
    // back to the model as a blocked tool call (matches the
    // `validate_pretool::run()` pattern).
    if !allowed {
        eprintln!("{}", message);
        std::process::exit(2);
    }
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

    // --- validate BLOCK path tests ---

    #[test]
    fn test_validate_blocks_when_skills_continue_auto_detailed() {
        let dir = tempfile::tempdir().unwrap();
        let state = json!({
            "current_phase": "flow-code",
            "branch": "test",
            "skills": {"flow-code": {"continue": "auto", "commit": "auto"}},
        });
        let path = write_state(dir.path(), "test", &state);
        let (allowed, msg, resp) = validate(Some(&path));
        assert!(!allowed, "Detailed skills.continue=auto must block");
        assert!(
            msg.contains("flow-code"),
            "block message must name the phase: {}",
            msg
        );
        assert!(resp.is_none(), "block path must not return hook_response");
    }

    #[test]
    fn test_validate_blocks_when_skills_continue_auto_simple() {
        let dir = tempfile::tempdir().unwrap();
        let state = json!({
            "current_phase": "flow-code",
            "branch": "test",
            "skills": {"flow-code": "auto"},
        });
        let path = write_state(dir.path(), "test", &state);
        let (allowed, msg, resp) = validate(Some(&path));
        assert!(!allowed, "Simple skills=auto must block");
        assert!(msg.contains("flow-code"));
        assert!(resp.is_none());
    }

    #[test]
    fn test_validate_allows_when_skills_continue_manual() {
        let dir = tempfile::tempdir().unwrap();
        let state = json!({
            "current_phase": "flow-code",
            "branch": "test",
            "skills": {"flow-code": {"continue": "manual"}},
        });
        let path = write_state(dir.path(), "test", &state);
        let (allowed, msg, resp) = validate(Some(&path));
        assert!(allowed);
        assert!(msg.is_empty());
        assert!(resp.is_none());
    }

    #[test]
    fn test_validate_allows_when_skills_key_missing() {
        let dir = tempfile::tempdir().unwrap();
        let state = json!({"current_phase": "flow-code", "branch": "test"});
        let path = write_state(dir.path(), "test", &state);
        let (allowed, msg, resp) = validate(Some(&path));
        assert!(allowed, "missing skills key must fail-open (legacy state)");
        assert!(msg.is_empty());
        assert!(resp.is_none());
    }

    #[test]
    fn test_validate_allows_when_current_phase_not_in_skills() {
        let dir = tempfile::tempdir().unwrap();
        let state = json!({
            "current_phase": "flow-code",
            "branch": "test",
            "skills": {"flow-start": {"continue": "auto"}},
        });
        let path = write_state(dir.path(), "test", &state);
        let (allowed, _msg, resp) = validate(Some(&path));
        assert!(allowed, "current_phase missing from skills must allow");
        assert!(resp.is_none());
    }

    #[test]
    fn test_validate_block_precedes_auto_continue() {
        let dir = tempfile::tempdir().unwrap();
        let state = json!({
            "current_phase": "flow-code",
            "branch": "test",
            "skills": {"flow-code": {"continue": "auto"}},
            "_auto_continue": "/flow:flow-code-review",
        });
        let path = write_state(dir.path(), "test", &state);
        let (allowed, msg, resp) = validate(Some(&path));
        assert!(
            !allowed,
            "block must take precedence over _auto_continue auto-answer"
        );
        assert!(msg.contains("flow-code"));
        assert!(resp.is_none(), "block path must not auto-answer");
    }

    #[test]
    fn test_validate_auto_continue_without_skills_auto() {
        let dir = tempfile::tempdir().unwrap();
        let state = json!({
            "current_phase": "flow-code",
            "branch": "test",
            "skills": {"flow-code": {"continue": "manual"}},
            "_auto_continue": "/flow:flow-code-review",
        });
        let path = write_state(dir.path(), "test", &state);
        let (allowed, _msg, resp) = validate(Some(&path));
        assert!(
            allowed,
            "_auto_continue without skills-auto must auto-answer"
        );
        assert!(
            resp.is_some(),
            "manual skills + _auto_continue must auto-answer"
        );
        let resp = resp.unwrap();
        assert_eq!(resp["permissionDecision"], "allow");
    }

    #[test]
    fn test_validate_block_message_names_phase() {
        let dir = tempfile::tempdir().unwrap();
        let state = json!({
            "current_phase": "flow-learn",
            "branch": "test",
            "skills": {"flow-learn": {"continue": "auto"}},
        });
        let path = write_state(dir.path(), "test", &state);
        let (_allowed, msg, _resp) = validate(Some(&path));
        assert!(
            msg.contains("flow-learn"),
            "message must name the configured phase for diagnosis: {}",
            msg
        );
    }

    #[test]
    fn test_validate_allows_no_current_phase() {
        let dir = tempfile::tempdir().unwrap();
        let state = json!({
            "branch": "test",
            "skills": {"flow-code": {"continue": "auto"}},
        });
        let path = write_state(dir.path(), "test", &state);
        let (allowed, _msg, resp) = validate(Some(&path));
        assert!(
            allowed,
            "missing current_phase must fail-open — no phase to gate on"
        );
        assert!(resp.is_none());
    }

    #[test]
    fn test_validate_corrupt_skills_value() {
        let dir = tempfile::tempdir().unwrap();
        let state = json!({
            "current_phase": "flow-code",
            "branch": "test",
            "skills": [1, 2, 3],
        });
        let path = write_state(dir.path(), "test", &state);
        let (allowed, _msg, resp) = validate(Some(&path));
        assert!(allowed, "corrupt skills value must fail-open");
        assert!(resp.is_none());
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
        assert!(!updated["_blocked"].as_str().unwrap().is_empty());
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
    fn test_set_blocked_non_object_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("array.json");
        fs::write(&path, "[1, 2, 3]").unwrap();
        // Should not panic — object guard skips the mutation
        set_blocked(&path);
        let content = fs::read_to_string(&path).unwrap();
        let parsed: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(
            parsed,
            json!([1, 2, 3]),
            "non-object state must be unchanged"
        );
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
