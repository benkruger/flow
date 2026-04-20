//! PreToolUse hook for AskUserQuestion — enforces autonomous-phase discipline.
//!
//! Three outcomes, evaluated in order:
//!
//! 1. **Block** (exit 2, stderr message) — when the current phase is
//!    mid-execution (`phases.<current_phase>.status == "in_progress"`)
//!    AND configured autonomous (`skills.<current_phase>.continue ==
//!    "auto"`). This is the mechanical enforcer for
//!    `.claude/rules/autonomous-phase-discipline.md`. Scoped to
//!    in_progress so manual→auto transition approvals (fired after
//!    `phase_complete()` advances `current_phase` but before
//!    `phase_enter()` sets the next phase to in_progress) are not
//!    blocked.
//! 2. **Auto-answer** (exit 0, JSON on stdout) — when `_auto_continue`
//!    is set and the block did not fire. Answers the AskUserQuestion
//!    with the successor skill command so phase transitions advance
//!    even if the skill's HARD-GATE was ignored.
//! 3. **Allow** (exit 0, no stdout) — otherwise. The tool call passes
//!    through to Claude Code's normal permission system.

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
    // Use `symlink_metadata` rather than `Path::exists()` — `exists()`
    // follows symlinks, so a dangling symlink at the state path would
    // return false and the subsequent `mutate_state` write would then
    // follow the symlink to its target. See
    // `.claude/rules/rust-patterns.md` "Symlink-Safe Existence Checks
    // Before Writes."
    if std::fs::symlink_metadata(state_path).is_err() {
        return;
    }
    let _ = mutate_state(state_path, &mut |state| {
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

    // Block path: when the current phase is mid-execution AND configured
    // autonomous (`skills.<current_phase>.continue == "auto"`), refuse the
    // AskUserQuestion tool call. The block is scoped to `phases[current_phase]
    // .status == "in_progress"` so transition-boundary prompts — fired after
    // `phase_complete()` has advanced `current_phase` to the next phase but
    // before `phase_enter()` has set its status to in_progress — remain
    // allowed. Without that scope, a manual→auto transition (e.g., Code=manual
    // with Code Review=auto in the Recommended preset) would deadlock: the
    // completing skill's HARD-GATE fires `AskUserQuestion` to approve the
    // transition, but the hook sees the next phase's auto config and blocks
    // the approval.
    //
    // Precedence over `_auto_continue`: when both `skills.<phase>.continue
    // == "auto"` AND `_auto_continue` are set during an in-progress phase,
    // the block wins (the user's explicit opt-in takes priority over the
    // transient transition-boundary safety net). `_auto_continue` only
    // auto-answers when the phase is not in_progress+auto.
    let current_phase = state
        .get("current_phase")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !current_phase.is_empty() {
        let phase_status = state
            .get("phases")
            .and_then(|p| p.get(current_phase))
            .and_then(|p| p.get("status"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let in_progress = phase_status == "in_progress";
        let skill_entry = state.get("skills").and_then(|s| s.get(current_phase));
        let is_auto = match skill_entry {
            // Bare string form — `skills.<phase> = "auto"`
            // (SkillConfig::Simple in Rust).
            Some(v) if v.as_str() == Some("auto") => true,
            // Object form — `skills.<phase> = {"continue": "auto", ...}`
            // (SkillConfig::Detailed in Rust).
            Some(v) => v.get("continue").and_then(|c| c.as_str()) == Some("auto"),
            None => false,
        };
        if in_progress && is_auto {
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

    // Slash-containing git branches are not valid FLOW branches —
    // treat as "no active flow" and exit 0 rather than panicking.
    let state_path = match FlowPaths::try_new(project_root(), &branch) {
        Some(p) => p.state_file(),
        None => std::process::exit(0),
    };

    let (allowed, message, hook_response) = validate(Some(&state_path));
    // Block path: exit 2 with stderr message so Claude Code feeds it
    // back to the model as a blocked tool call (matches the
    // `validate_pretool::run()` pattern). `set_blocked` is intentionally
    // not called on this path — the hook refused the tool call at the
    // gate, so there is no "blocked-while-executing" timestamp to
    // record. `_blocked` is only written when an AskUserQuestion was
    // actually delivered to the user.
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

    /// RAII guard that restores file permissions on Drop. Protects
    /// chmod-000 tests from leaking a mode-000 file when an assertion
    /// inside the test body panics before the inline restore runs.
    /// Per `.claude/rules/panic-safe-cleanup.md`, any resource whose
    /// released state is not the default must be wrapped in a Drop
    /// impl to guarantee cleanup on panic unwind.
    struct PermissionGuard {
        path: std::path::PathBuf,
        restore_mode: u32,
    }
    impl Drop for PermissionGuard {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(self.restore_mode));
        }
    }

    /// Covers the `Err(_) => return (true, String::new(), None)` arm on
    /// line 67 of `validate`: `state_path.exists()` succeeds but
    /// `read_to_string` fails. A file mode of `0o000` on macOS passes
    /// the `exists()` metadata check but the read returns EACCES.
    #[test]
    fn test_validate_allows_unreadable_state_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let unreadable = dir.path().join("unreadable.json");
        fs::write(&unreadable, "{}").unwrap();
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();
        let _guard = PermissionGuard {
            path: unreadable.clone(),
            restore_mode: 0o644,
        };
        let (allowed, msg, resp) = validate(Some(&unreadable));
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
            "phases": {"flow-code": {"status": "in_progress"}},
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
            "phases": {"flow-code": {"status": "in_progress"}},
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
            "phases": {"flow-code": {"status": "in_progress"}},
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
        let state = json!({
            "current_phase": "flow-code",
            "branch": "test",
            "phases": {"flow-code": {"status": "in_progress"}},
        });
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
            "phases": {"flow-code": {"status": "in_progress"}},
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
            "phases": {"flow-code": {"status": "in_progress"}},
            "_auto_continue": "/flow:flow-code-review",
        });
        let path = write_state(dir.path(), "test", &state);
        let (allowed, msg, resp) = validate(Some(&path));
        assert!(
            !allowed,
            "block must take precedence over _auto_continue auto-answer \
             when the current phase is in_progress+auto"
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
            "phases": {"flow-code": {"status": "in_progress"}},
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
            "phases": {"flow-learn": {"status": "in_progress"}},
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
            "phases": {"flow-code": {"status": "in_progress"}},
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
            "phases": {"flow-code": {"status": "in_progress"}},
        });
        let path = write_state(dir.path(), "test", &state);
        let (allowed, _msg, resp) = validate(Some(&path));
        assert!(allowed, "corrupt skills value must fail-open");
        assert!(resp.is_none());
    }

    // Regression guard for the pre-mortem critical finding: manual→auto
    // transitions (Code=manual with Code Review=auto in the Recommended
    // preset) would deadlock if the hook blocked the skill's HARD-GATE
    // AskUserQuestion fired after `phase_complete()` advanced
    // `current_phase` to the next phase. At that moment the next phase's
    // `phases.<phase>.status` is still `"pending"` — `phase_enter()` has
    // not yet run. The hook must allow AskUserQuestion in that window.
    #[test]
    fn test_validate_allows_at_transition_boundary_pending_phase() {
        let dir = tempfile::tempdir().unwrap();
        let state = json!({
            "current_phase": "flow-code-review",
            "branch": "test",
            "skills": {"flow-code-review": {"continue": "auto"}},
            "phases": {
                "flow-code": {"status": "complete"},
                "flow-code-review": {"status": "pending"},
            },
        });
        let path = write_state(dir.path(), "test", &state);
        let (allowed, msg, resp) = validate(Some(&path));
        assert!(
            allowed,
            "transition boundary (next phase pending) must allow — \
             prevents the manual→auto deadlock on the Recommended preset"
        );
        assert!(msg.is_empty());
        assert!(resp.is_none());
    }

    #[test]
    fn test_validate_allows_when_phase_status_missing() {
        let dir = tempfile::tempdir().unwrap();
        let state = json!({
            "current_phase": "flow-code",
            "branch": "test",
            "skills": {"flow-code": {"continue": "auto"}},
            // no `phases` key at all — legacy state before phase-status
            // tracking was added
        });
        let path = write_state(dir.path(), "test", &state);
        let (allowed, _msg, resp) = validate(Some(&path));
        assert!(
            allowed,
            "missing phases key must fail-open — legacy state tolerance"
        );
        assert!(resp.is_none());
    }

    #[test]
    fn test_validate_allows_when_phase_status_complete() {
        let dir = tempfile::tempdir().unwrap();
        let state = json!({
            "current_phase": "flow-code",
            "branch": "test",
            "skills": {"flow-code": {"continue": "auto"}},
            "phases": {"flow-code": {"status": "complete"}},
        });
        let path = write_state(dir.path(), "test", &state);
        let (allowed, _msg, resp) = validate(Some(&path));
        assert!(
            allowed,
            "completed phase must allow — not currently executing"
        );
        assert!(resp.is_none());
    }

    #[test]
    fn test_validate_corrupt_phases_value() {
        let dir = tempfile::tempdir().unwrap();
        let state = json!({
            "current_phase": "flow-code",
            "branch": "test",
            "skills": {"flow-code": {"continue": "auto"}},
            "phases": "not-an-object",
        });
        let path = write_state(dir.path(), "test", &state);
        let (allowed, _msg, resp) = validate(Some(&path));
        assert!(allowed, "corrupt phases value must fail-open");
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
