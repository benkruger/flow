//! Integration tests for `src/hooks/stop_continue.rs`. Drives the public
//! surface (`capture_session_id`, `check_continue`, `set_blocked_idle`,
//! `set_tab_color`, `check_discussion_mode`, `check_first_stop`,
//! `format_block_output`, `format_conditional_continue_reason`,
//! `DISCUSSION_BLOCK_REASON`) and covers `run()` via subprocess tests.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use flow_rs::commands::clear_blocked::clear_blocked;
use flow_rs::hooks::stop_continue::{
    body_has_question_outside_code, capture_session_id, check_autonomous_in_progress,
    check_continue, check_discussion_mode, check_first_stop, check_halt_pending,
    check_in_progress_utility_skill, check_prose_pause_at_task_entry, format_block_output,
    format_conditional_continue_reason, set_blocked_idle, set_tab_color, DISCUSSION_BLOCK_REASON,
    HALT_PAUSE_BLOCK_REASON,
};
use serde_json::{json, Value};

// --- capture_session_id ---

#[test]
fn test_capture_session_id_updates_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test", "current_phase": "flow-start"});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let input = json!({
        "session_id": "abc123",
        "transcript_path": "/path/to/transcript.jsonl"
    });
    capture_session_id(&input, &path);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state["session_id"], "abc123");
    assert_eq!(state["transcript_path"], "/path/to/transcript.jsonl");
}

#[test]
fn test_capture_session_id_skips_when_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"session_id": "abc123"});
    fs::write(&path, serde_json::to_string_pretty(&initial).unwrap()).unwrap();
    let original = fs::read_to_string(&path).unwrap();

    capture_session_id(&json!({"session_id": "abc123"}), &path);
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn test_capture_session_id_empty_session_id_skips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test"});
    fs::write(&path, serde_json::to_string_pretty(&initial).unwrap()).unwrap();
    let original = fs::read_to_string(&path).unwrap();

    capture_session_id(&json!({"session_id": ""}), &path);
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn test_capture_session_id_no_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.json");
    capture_session_id(&json!({"session_id": "abc"}), &path);
}

#[test]
fn test_capture_session_id_no_transcript_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test"});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    capture_session_id(&json!({"session_id": "abc"}), &path);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state["session_id"], "abc");
    assert!(state.get("transcript_path").is_none());
}

// --- check_continue ---

#[test]
fn test_check_continue_blocks_when_pending_set() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({
        "branch": "test",
        "_continue_pending": "commit",
        "_continue_context": "Do the thing"
    });
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_continue(&json!({}), &path);
    assert!(result.should_block);
    assert_eq!(result.skill.unwrap(), "commit");
    assert_eq!(result.context.unwrap(), "Do the thing");

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state["_continue_pending"], "");
    assert_eq!(state["_continue_context"], "");
}

#[test]
fn test_check_continue_no_block_when_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test", "_continue_pending": ""});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_continue(&json!({}), &path);
    assert!(!result.should_block);
    assert!(result.skill.is_none());
    assert!(result.context.is_none());
}

#[test]
fn test_check_continue_no_pending_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test"});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_continue(&json!({}), &path);
    assert!(!result.should_block);
}

#[test]
fn test_check_continue_session_mismatch_clears_and_allows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({
        "branch": "test",
        "_continue_pending": "commit",
        "_continue_context": "stale context",
        "session_id": "old-session"
    });
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_continue(&json!({"session_id": "new-session"}), &path);
    assert!(!result.should_block);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state["_continue_pending"], "");
    assert_eq!(state["_continue_context"], "");
}

#[test]
fn test_check_continue_session_match_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({
        "branch": "test",
        "_continue_pending": "commit",
        "_continue_context": "ctx",
        "session_id": "same-session"
    });
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_continue(&json!({"session_id": "same-session"}), &path);
    assert!(result.should_block);
    assert_eq!(result.context.unwrap(), "ctx");
}

#[test]
fn test_check_continue_no_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.json");
    let result = check_continue(&json!({}), &path);
    assert!(!result.should_block);
}

#[test]
fn test_check_continue_empty_context_becomes_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({
        "_continue_pending": "commit",
        "_continue_context": ""
    });
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_continue(&json!({}), &path);
    assert!(result.should_block);
    assert!(result.context.is_none());
}

// --- set_blocked_idle ---

#[test]
fn test_set_blocked_idle_sets_timestamp() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, r#"{"branch": "test"}"#).unwrap();

    set_blocked_idle(&path);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(state.get("_blocked").is_some());
    assert!(!state["_blocked"].as_str().unwrap().is_empty());
}

#[test]
fn test_set_blocked_idle_no_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.json");
    set_blocked_idle(&path);
}

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

// --- set_tab_color ---

#[test]
fn test_set_tab_color_with_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(state_dir.join("test")).unwrap();
    let state_path = state_dir.join("test").join("state.json");
    fs::write(&state_path, r#"{"repo": "owner/repo"}"#).unwrap();

    set_tab_color(dir.path(), "test", &state_path);
}

#[test]
fn test_set_tab_color_without_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(state_dir.join("test")).unwrap();
    let state_path = state_dir.join("test").join("state.json");

    set_tab_color(dir.path(), "test", &state_path);
}

#[test]
fn test_set_tab_color_corrupt_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(state_dir.join("test")).unwrap();
    let state_path = state_dir.join("test").join("state.json");
    fs::write(&state_path, "{bad json").unwrap();

    set_tab_color(dir.path(), "test", &state_path);
}

// Exercises the `Some((_, state, _))` arm of the `find_state_files`
// fallback inside `set_tab_color`: state_path for the requested branch
// doesn't exist, but another flow's state file does.
#[test]
fn test_set_tab_color_finds_other_active_feature() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(state_dir.join("other-branch")).unwrap();
    // Other feature's state file with a repo field.
    fs::write(
        state_dir.join("other-branch").join("state.json"),
        r#"{"repo": "owner/repo", "branch": "other-branch"}"#,
    )
    .unwrap();

    // Requested state path does NOT exist — triggers the else arm
    // with find_state_files fallback, which locates other-branch.json.
    let state_path = state_dir.join("test").join("state.json");
    set_tab_color(dir.path(), "test", &state_path);
}

#[test]
fn test_set_tab_color_unreadable_state_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(state_dir.join("test")).unwrap();
    let state_path = state_dir.join("test").join("state.json");
    fs::write(&state_path, r#"{"repo": "owner/repo"}"#).unwrap();
    fs::set_permissions(&state_path, fs::Permissions::from_mode(0o000)).unwrap();
    let _guard = PermissionGuard {
        path: state_path.clone(),
        restore_mode: 0o644,
    };
    set_tab_color(dir.path(), "test", &state_path);
}

// --- format_block_output ---

#[test]
fn test_format_block_output_with_context() {
    let out = format_block_output("commit", Some("Do the thing next"));
    assert_eq!(out["decision"], "block");
    let reason = out["reason"].as_str().unwrap();
    assert_eq!(
        reason,
        "Continue parent phase — child skill 'commit' has returned.\n\nNext steps:\nDo the thing next"
    );
}

#[test]
fn test_format_block_output_without_context() {
    let out = format_block_output("commit", None);
    assert_eq!(out["decision"], "block");
    let reason = out["reason"].as_str().unwrap();
    assert_eq!(
        reason,
        "Continue parent phase — child skill 'commit' has returned. Resume the parent skill instructions."
    );
}

#[test]
fn test_format_block_output_empty_context_treated_as_none() {
    let out = format_block_output("commit", Some(""));
    let reason = out["reason"].as_str().unwrap();
    assert!(reason.ends_with("Resume the parent skill instructions."));
    assert!(!reason.contains("Next steps:"));
}

#[test]
fn test_format_block_output_empty_skill_name() {
    let out = format_block_output("", None);
    assert_eq!(out["decision"], "block");
    assert!(out["reason"].as_str().unwrap().contains("child skill ''"));
}

// --- derive_root_branch (via capture_session_id diagnostics) ---
//
// The `derive_root_branch` helper is private. Its two branches are
// exercised indirectly:
//   - canonical `.flow-states/<branch>.json` layout → covered by
//     `test_capture_session_id_corrupt_state_logs_error` which asserts
//     the log file lands in `.flow-states/<branch>.log`.
//   - non-canonical layout (state path outside `.flow-states/`) →
//     covered by `test_capture_session_id_corrupt_state_outside_flow_states`
//     below. The log write is skipped but the stderr diagnostic still
//     fires; we just assert no crash.

// Exercises the `if let Ok(mut f) = OpenOptions::...open(...)` Err
// arm in log_diag: the log_path is pre-created as a directory so
// OpenOptions::open returns Err. log_diag must swallow the error
// silently.
#[test]
fn test_capture_session_id_log_file_path_is_directory() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(state_dir.join("branch-x")).unwrap();
    let state_path = state_dir.join("branch-x").join("state.json");
    fs::write(&state_path, "{bad json").unwrap();
    // Pre-create `log` as a directory — OpenOptions::open
    // cannot open a directory as a writable file.
    fs::create_dir(state_dir.join("branch-x").join("log")).unwrap();

    // Must not panic; log_diag swallows the open Err silently.
    capture_session_id(&json!({"session_id": "abc"}), &state_path);
}

#[test]
fn test_capture_session_id_corrupt_state_outside_flow_states() {
    // State file at <tempdir>/state.json (NOT inside .flow-states/).
    // mutate_state returns Err on the corrupt JSON, and derive_root_branch
    // returns (None, Some(stem)) — log_diag skips the file write because
    // root is None. The function must not panic.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, "{bad json").unwrap();

    capture_session_id(&json!({"session_id": "abc"}), &path);
    // No panic; no log file expected.
    assert!(!dir.path().join("state.log").exists());
}

// --- check_continue log file writes ---

#[test]
fn test_check_continue_block_writes_log_file() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(state_dir.join("test-branch")).unwrap();
    let state_path = state_dir.join("test-branch").join("state.json");
    let initial = json!({
        "_continue_pending": "commit",
        "_continue_context": "Next step"
    });
    fs::write(&state_path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_continue(&json!({}), &state_path);
    assert!(result.should_block);

    let log_path = state_dir.join("test-branch").join("log");
    assert!(
        log_path.exists(),
        "log file must be written after blocking decision"
    );
    let log_content = fs::read_to_string(&log_path).unwrap();
    assert!(log_content.contains("[stop-continue]"));
    assert!(log_content.contains("blocking: pending=commit"));
}

#[test]
fn test_check_continue_session_mismatch_writes_log_file() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(state_dir.join("test-branch")).unwrap();
    let state_path = state_dir.join("test-branch").join("state.json");
    let initial = json!({
        "_continue_pending": "commit",
        "_continue_context": "stale",
        "session_id": "old-session"
    });
    fs::write(&state_path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_continue(&json!({"session_id": "new-session"}), &state_path);
    assert!(!result.should_block);

    let log_path = state_dir.join("test-branch").join("log");
    assert!(log_path.exists());
    let log_content = fs::read_to_string(&log_path).unwrap();
    assert!(log_content.contains("session mismatch"));
    assert!(log_content.contains("cleared pending=commit"));
}

#[test]
fn test_check_continue_no_pending_does_not_write_log_file() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(state_dir.join("test-branch")).unwrap();
    let state_path = state_dir.join("test-branch").join("state.json");
    fs::write(&state_path, r#"{"branch": "test"}"#).unwrap();

    check_continue(&json!({}), &state_path);

    let log_path = state_dir.join("test-branch").join("log");
    assert!(!log_path.exists());
}

// --- capture_session_id error logging ---

#[test]
fn test_capture_session_id_corrupt_state_logs_error() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(state_dir.join("test-branch")).unwrap();
    let state_path = state_dir.join("test-branch").join("state.json");
    fs::write(&state_path, "{bad json").unwrap();

    capture_session_id(&json!({"session_id": "abc123"}), &state_path);

    let log_path = state_dir.join("test-branch").join("log");
    assert!(log_path.exists(), "corrupt-state errors must be logged");
    let log_content = fs::read_to_string(&log_path).unwrap();
    assert!(log_content.contains("capture_session_id error"));
}

// --- array state (adversarial non-crash) ---

#[test]
fn test_check_discussion_mode_array_state_file_does_not_crash() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, r#"["not", "an", "object"]"#).unwrap();
    let result = check_discussion_mode(&path);
    assert!(!result.should_block);
}

#[test]
fn test_check_continue_array_state_file_does_not_crash() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, r#"["not", "an", "object"]"#).unwrap();

    let result = check_continue(&json!({}), &path);
    assert!(!result.should_block);
}

#[test]
fn test_capture_session_id_array_state_file_does_not_crash() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, r#"["not", "an", "object"]"#).unwrap();

    capture_session_id(&json!({"session_id": "abc"}), &path);

    let after: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(after.is_array());
}

#[test]
fn test_check_continue_empty_hook_session_id_still_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({
        "session_id": "existing-session",
        "_continue_pending": "flow-commit",
        "_continue_context": "Next: run tests"
    });
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_continue(&json!({"session_id": ""}), &path);
    assert!(result.should_block);
    assert_eq!(result.skill.unwrap(), "flow-commit");
    assert_eq!(result.context.unwrap(), "Next: run tests");
}

#[test]
fn test_check_continue_empty_state_session_id_still_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({
        "session_id": "",
        "_continue_pending": "flow-commit",
        "_continue_context": "ctx"
    });
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_continue(&json!({"session_id": "abc"}), &path);
    assert!(result.should_block);
}

// --- check_discussion_mode ---

#[test]
fn test_discussion_mode_blocks_first_interrupt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test", "current_phase": "flow-code"});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_discussion_mode(&path);
    assert!(result.should_block);
}

#[test]
fn test_discussion_mode_allows_second_interrupt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test", "_stop_instructed": true});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_discussion_mode(&path);
    assert!(!result.should_block);
}

#[test]
fn test_discussion_mode_skips_no_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.json");

    let result = check_discussion_mode(&path);
    assert!(!result.should_block);
}

#[test]
fn test_discussion_mode_block_reason_contains_flow_note() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test"});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_discussion_mode(&path);
    assert!(result.should_block);
    let reason = DISCUSSION_BLOCK_REASON;
    assert!(reason.contains("flow:flow-note"));
}

#[test]
fn test_discussion_mode_sets_flag_in_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test"});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_discussion_mode(&path);
    assert!(result.should_block);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state["_stop_instructed"], json!(true));
}

#[test]
fn test_discussion_mode_non_bool_flag_self_heals() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test", "_stop_instructed": "true"});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_discussion_mode(&path);
    assert!(result.should_block);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state["_stop_instructed"], json!(true));
}

#[test]
fn test_discussion_mode_clears_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test", "_blocked": "2024-01-01T00:00:00"});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let disc = check_discussion_mode(&path);
    assert!(disc.should_block);

    clear_blocked(&path);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(state.get("_blocked").is_none());
}

// --- format_conditional_continue_reason ---

#[test]
fn test_format_conditional_with_context() {
    let result = format_conditional_continue_reason("commit", Some("Do the thing next"));
    assert!(result.contains("Next steps:"));
    assert!(result.contains("Do the thing next"));
}

#[test]
fn test_format_conditional_without_context() {
    let result = format_conditional_continue_reason("commit", None);
    assert!(result.contains("Resume the parent skill instructions"));
}

#[test]
fn test_format_conditional_mentions_flow_note() {
    let result = format_conditional_continue_reason("commit", Some("ctx"));
    assert!(result.contains("flow:flow-note"));
}

#[test]
fn test_format_conditional_contains_skill_name() {
    let result = format_conditional_continue_reason("my-skill", Some("ctx"));
    assert!(result.contains("my-skill"));
}

// --- check_first_stop ---

#[test]
fn test_first_stop_with_pending_blocks_conditionally() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({
        "branch": "test",
        "_continue_pending": "commit",
        "_continue_context": "Do the thing"
    });
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_first_stop(&json!({}), &path);
    assert!(result.should_block);
    assert!(result.context.as_ref().unwrap().contains("commit"));
}

#[test]
fn test_first_stop_without_pending_blocks_discussion() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test"});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_first_stop(&json!({}), &path);
    assert!(result.should_block);
    assert_eq!(result.context.as_ref().unwrap(), DISCUSSION_BLOCK_REASON);
}

#[test]
fn test_first_stop_already_instructed_allows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({"branch": "test", "_stop_instructed": true});
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_first_stop(&json!({}), &path);
    assert!(!result.should_block);
}

#[test]
fn test_first_stop_consumes_pending() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({
        "branch": "test",
        "_continue_pending": "commit",
        "_continue_context": "Do the thing"
    });
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_first_stop(&json!({}), &path);
    assert!(result.should_block);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state["_continue_pending"], "");
    assert_eq!(state["_continue_context"], "");
}

#[test]
fn test_first_stop_preserves_stop_instructed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({
        "branch": "test",
        "_continue_pending": "commit",
        "_continue_context": "ctx"
    });
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    check_first_stop(&json!({}), &path);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state["_stop_instructed"], json!(true));
}

// Exercises the `ssid == hsid` path of check_first_stop: state and
// hook have matching session ids, so the mismatch branch does NOT
// fire and the pending is consumed normally.
#[test]
fn test_first_stop_session_match_consumes_pending() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({
        "branch": "test",
        "_continue_pending": "commit",
        "_continue_context": "Do the thing",
        "session_id": "same-session"
    });
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_first_stop(&json!({"session_id": "same-session"}), &path);
    assert!(result.should_block);
    assert_eq!(result.skill.unwrap(), "discussion-with-pending");

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state["_continue_pending"], "");
    assert_eq!(state["_continue_context"], "");
}

#[test]
fn test_first_stop_session_mismatch_clears_pending() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({
        "branch": "test",
        "_continue_pending": "commit",
        "_continue_context": "stale",
        "session_id": "old-session"
    });
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_first_stop(&json!({"session_id": "new-session"}), &path);
    assert!(result.should_block);
    assert_eq!(result.context.as_ref().unwrap(), DISCUSSION_BLOCK_REASON);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state["_continue_pending"], "");
    assert_eq!(state["_continue_context"], "");
}

#[test]
fn test_first_stop_no_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.json");

    let result = check_first_stop(&json!({}), &path);
    assert!(!result.should_block);
}

#[test]
fn test_first_stop_array_state_does_not_crash() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, r#"["not", "an", "object"]"#).unwrap();

    let result = check_first_stop(&json!({}), &path);
    assert!(!result.should_block);
}

#[test]
fn test_discussion_mode_cleared_on_continue_pending() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let initial = json!({
        "branch": "test",
        "_continue_pending": "commit",
        "_continue_context": "Do the thing",
        "_stop_instructed": true
    });
    fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_continue(&json!({}), &path);
    assert!(result.should_block);

    let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(state.get("_stop_instructed").is_none());
}

// --- run() subprocess tests ---

/// Spawn `flow-rs hook stop-continue` with the child detached from its
/// controlling terminal via `setsid()`. Without setsid, the child
/// inherits the parent's controlling tty — on an interactive host,
/// `write_tab_sequences`'s `fs::write("/dev/tty", ...)` call then
/// succeeds, leaving the Err arm of `set_tab_color`'s `if let
/// Err(e) = result` uncovered. On a non-tty host (Claude Code, CI
/// agents) the write fails naturally, but that's environmental
/// flakiness — the test shouldn't depend on how the host was
/// launched. `setsid()` makes the child a new session leader
/// with no controlling terminal, so `/dev/tty` access returns
/// `ENXIO` regardless of the host's tty state, forcing the Err
/// path deterministically.
fn run_hook(cwd: &Path, stdin_input: &str) -> (i32, String, String) {
    run_hook_inner(cwd, stdin_input, None)
}

/// Variant that overrides `HOME` for the subprocess so the prose-
/// pause guard's `is_safe_transcript_path` validator resolves
/// `<HOME>/.claude/projects/` against the test fixture's tempdir
/// instead of the real user home. Tests that drive the prose-pause
/// path through the full Stop hook MUST go through this helper.
fn run_hook_with_home(cwd: &Path, stdin_input: &str, home: &Path) -> (i32, String, String) {
    run_hook_inner(cwd, stdin_input, Some(home))
}

fn run_hook_inner(cwd: &Path, stdin_input: &str, home: Option<&Path>) -> (i32, String, String) {
    use std::os::unix::process::CommandExt;
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.args(["hook", "stop-continue"])
        .current_dir(cwd)
        .env_remove("FLOW_CI_RUNNING")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(home_path) = home {
        cmd.env("HOME", home_path);
    }
    // SAFETY: `setsid()` is async-signal-safe. The closure allocates
    // nothing and either returns Ok or propagates the errno error.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn().expect("spawn flow-rs");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(stdin_input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().expect("wait");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

// Outside a git repo, resolve_branch returns None → run() returns early.
#[test]
fn run_subprocess_outside_git_repo_exits_0() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (code, _stdout, _stderr) = run_hook(&root, "{}");
    assert_eq!(code, 0);
}

// A valid git repo with no state file → check_first_stop returns early,
// no block output. run() still calls set_blocked_idle + set_tab_color.
#[test]
fn run_subprocess_git_repo_no_state_exits_0() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    Command::new("git")
        .args(["init", "--initial-branch", "main"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "t@t"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "t"])
        .current_dir(&root)
        .output()
        .unwrap();
    fs::write(root.join("README.md"), "x").unwrap();
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&root)
        .output()
        .unwrap();

    let (code, _stdout, _stderr) = run_hook(&root, r#"{"session_id": "s1"}"#);
    assert_eq!(code, 0);
}

// State file with _continue_pending → check_first_stop blocks and writes
// JSON to stdout.
#[test]
fn run_subprocess_with_pending_blocks_and_writes_stdout() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    Command::new("git")
        .args(["init", "--initial-branch", "main"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "t@t"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "t"])
        .current_dir(&root)
        .output()
        .unwrap();
    fs::write(root.join("README.md"), "x").unwrap();
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&root)
        .output()
        .unwrap();

    let state_dir = root.join(".flow-states");
    fs::create_dir_all(state_dir.join("main")).unwrap();
    let state = json!({
        "branch": "main",
        "_continue_pending": "commit",
        "_continue_context": "Next step"
    });
    fs::write(
        state_dir.join("main").join("state.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();

    let (code, stdout, _stderr) = run_hook(&root, r#"{"session_id": "s1"}"#);
    assert_eq!(code, 0);
    let last = stdout
        .lines()
        .rfind(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON in stdout: {}", stdout));
    let json: Value = serde_json::from_str(last).unwrap();
    assert_eq!(json["decision"], "block");
}

// Drives the full run() → check_prose_pause_at_task_entry path
// with a transcript_path in stdin so the `Some(v) => v.as_str()`
// arm of the match form in run() is exercised. Without this test
// every existing run() subprocess passes hook_input without
// `transcript_path` and the Some arm reads as 0/0.
#[test]
fn run_subprocess_prose_pause_blocks_when_transcript_path_present() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    Command::new("git")
        .args(["init", "--initial-branch", "main"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "t@t"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "t"])
        .current_dir(&root)
        .output()
        .unwrap();
    fs::write(root.join("README.md"), "x").unwrap();
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&root)
        .output()
        .unwrap();

    let state_dir = root.join(".flow-states");
    fs::create_dir_all(state_dir.join("main")).unwrap();
    // _stop_instructed=true keeps check_first_stop past discussion-mode entry;
    // _continue_pending="" keeps check_continue from blocking; flow-code +
    // in_progress + auto + code_task=0 satisfies the first four prose-pause
    // guards; the transcript-content guards are satisfied by the file below.
    let state = json!({
        "branch": "main",
        "current_phase": "flow-code",
        "phases": {"flow-code": {"status": "in_progress"}},
        "skills": {"flow-code": "auto"},
        "code_task": 0,
        "_continue_pending": "",
        "_stop_instructed": true
    });
    fs::write(
        state_dir.join("main").join("state.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();
    // Construct transcript at `<HOME>/.claude/projects/` so the
    // is_safe_transcript_path validator inside
    // `check_prose_pause_at_task_entry` accepts it. Override HOME
    // for the subprocess to point at the test root, isolating from
    // the real user home per
    // `.claude/rules/subprocess-test-hygiene.md`.
    let projects = root.join(".claude").join("projects");
    fs::create_dir_all(&projects).unwrap();
    let transcript = projects.join("transcript.jsonl");
    let assistant = json!({
        "type": "assistant",
        "message": {"content": [{"type": "text", "text": "Should I proceed?"}]}
    });
    fs::write(&transcript, serde_json::to_string(&assistant).unwrap()).unwrap();

    let stdin = format!(
        r#"{{"session_id": "s1", "transcript_path": "{}"}}"#,
        transcript.to_str().unwrap()
    );
    let (code, stdout, _stderr) = run_hook_with_home(&root, &stdin, &root);
    assert_eq!(code, 0);
    // Block output appears as the last JSON line on stdout.
    let last = stdout
        .lines()
        .rfind(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON in stdout: {}", stdout));
    let json: Value = serde_json::from_str(last).unwrap();
    assert_eq!(json["decision"], "block");
    let reason = json["reason"].as_str().unwrap_or("");
    assert!(
        reason.contains("prose-based pause"),
        "expected prose-pause reason, got: {reason}"
    );
}

// Invalid JSON on stdin → `unwrap_or_else(|_| json!({}))` fires and
// treats the hook input as empty. run() continues through its normal
// flow. This test exercises the JSON-parse-failure arm in run().
#[test]
fn run_subprocess_invalid_json_stdin_uses_empty_hook_input() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    Command::new("git")
        .args(["init", "--initial-branch", "main"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "t@t"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "t"])
        .current_dir(&root)
        .output()
        .unwrap();
    fs::write(root.join("README.md"), "x").unwrap();
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&root)
        .output()
        .unwrap();

    let (code, _stdout, _stderr) = run_hook(&root, "{malformed json");
    assert_eq!(code, 0);
}

// Slash-containing branch → FlowPaths::try_new returns None → run()
// takes the `None => return` early-exit arm. Exercised by creating a
// git repo whose current branch contains a slash.
#[test]
fn run_subprocess_slash_branch_exits_0() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    Command::new("git")
        .args(["init", "--initial-branch", "feature/foo"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "t@t"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "t"])
        .current_dir(&root)
        .output()
        .unwrap();
    fs::write(root.join("README.md"), "x").unwrap();
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&root)
        .output()
        .unwrap();

    let (code, _stdout, _stderr) = run_hook(&root, r#"{"session_id": "s1"}"#);
    assert_eq!(code, 0);
}

// State already instructed (_stop_instructed=true) + _continue_pending set
// to a non-discussion skill name → check_first_stop falls through, then
// check_continue blocks with skill=<pending>. run()'s output formatter
// takes the else branch (format_block_output) because the skill name is
// neither "discussion" nor "discussion-with-pending". This exercises the
// non-discussion block path in run() that was previously also covered by
// the now-deleted qa-pending fallback test.
#[test]
fn run_subprocess_continue_pending_uses_format_block_output() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    Command::new("git")
        .args(["init", "--initial-branch", "main"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "t@t"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "t"])
        .current_dir(&root)
        .output()
        .unwrap();
    fs::write(root.join("README.md"), "x").unwrap();
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&root)
        .output()
        .unwrap();

    let state_dir = root.join(".flow-states");
    fs::create_dir_all(state_dir.join("main")).unwrap();
    // _stop_instructed=true forces check_first_stop to fall through, so
    // run() invokes check_continue, which returns skill=<pending>.
    let state = json!({
        "branch": "main",
        "_stop_instructed": true,
        "_continue_pending": "flow-commit",
        "_continue_context": "Resume the parent skill instructions."
    });
    fs::write(
        state_dir.join("main").join("state.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();

    let (code, stdout, _stderr) = run_hook(&root, r#"{"session_id": "s1"}"#);
    assert_eq!(code, 0);
    let last = stdout
        .lines()
        .rfind(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON in stdout: {}", stdout));
    let json: Value = serde_json::from_str(last).unwrap();
    assert_eq!(json["decision"], "block");
    let reason = json["reason"].as_str().unwrap_or("");
    assert!(
        reason.contains("child skill 'flow-commit' has returned"),
        "format_block_output reason must name the pending skill: {}",
        reason
    );
}

// Drives the full run() → check_in_progress_utility_skill path. Sets up a
// git repo with NO state file (the canonical context for the utility
// skills: they run outside any active FLOW phase, so check_first_stop
// and check_continue both early-return on `!state_path.exists()`). The
// predicate gates on two signals — the marker file at
// `<HOME>/.claude/flow/utility-in-progress-<session>.json` AND
// `decompose:decompose` being the most recent Skill call in the
// persisted transcript since the most recent real user turn. Both
// must be present for the run() pipeline to refuse turn-end.
#[test]
fn run_subprocess_blocks_when_utility_marker_present() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    Command::new("git")
        .args(["init", "--initial-branch", "main"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "t@t"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "t"])
        .current_dir(&root)
        .output()
        .unwrap();
    fs::write(root.join("README.md"), "x").unwrap();
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&root)
        .output()
        .unwrap();

    // Build HOME with the marker file present. The marker JSON names a
    // skill in MULTI_STEP_UTILITY_SKILLS and a session_id that matches
    // the stdin payload below.
    let home = root.join("home");
    let marker_dir = home.join(".claude").join("flow");
    fs::create_dir_all(&marker_dir).unwrap();
    let session_id = "abc12345";
    let marker_path = marker_dir.join(format!("utility-in-progress-{}.json", session_id));
    let payload = json!({
        "skill": "flow:flow-decompose-project",
        "session_id": session_id,
        "started_at": "2026-05-09T12:00:00-07:00",
    });
    fs::write(&marker_path, serde_json::to_string(&payload).unwrap()).unwrap();

    // Build a transcript under <home>/.claude/projects/p/session.jsonl
    // so the new predicate's walker validator accepts the path. The
    // transcript ends with a `decompose:decompose` Skill call so the
    // decompose-return gate fires and the predicate blocks.
    let transcript = transcript_with_skill_calls(&home, &["decompose:decompose"]);
    let stdin = format!(
        r#"{{"session_id": "{}", "transcript_path": "{}"}}"#,
        session_id,
        transcript.display(),
    );
    let (code, stdout, _stderr) = run_hook_with_home(&root, &stdin, &home);
    assert_eq!(code, 0);
    let last = stdout
        .lines()
        .rfind(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON in stdout: {}", stdout));
    let json: Value = serde_json::from_str(last).unwrap();
    assert_eq!(json["decision"], "block");
    assert_eq!(
        json["reason"].as_str().unwrap_or(""),
        "Stop Refused: Continue, you can do it. Don't give up, you got this! No excuses!",
        "block reason must be the AC#7 encouraging message",
    );
}

// Sibling to the marker-present test. Same fixture shape (no state file,
// no pending continuation) but the marker file does NOT exist. The
// run() pipeline must NOT block on the utility-skill predicate when no
// marker is present. Regression: a future refactor that inverts the
// marker-presence check or adds a default-block fallback in
// `check_in_progress_utility_skill` would cause every Stop event
// outside an in-progress utility skill to refuse turn-end.
#[test]
fn run_subprocess_no_block_when_utility_marker_absent() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    Command::new("git")
        .args(["init", "--initial-branch", "main"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "t@t"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "t"])
        .current_dir(&root)
        .output()
        .unwrap();
    fs::write(root.join("README.md"), "x").unwrap();
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&root)
        .output()
        .unwrap();

    // HOME exists but no marker file is written.
    let home = root.join("home");
    fs::create_dir_all(home.join(".claude").join("flow")).unwrap();

    let session_id = "abc12345";
    let stdin = format!(r#"{{"session_id": "{}"}}"#, session_id);
    let (code, stdout, _stderr) = run_hook_with_home(&root, &stdin, &home);
    assert_eq!(code, 0);
    // The stdout must not contain any JSON object whose `decision` field is
    // `"block"`. Other JSON lines (e.g., log diagnostics) are tolerated.
    for line in stdout.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('{') {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        assert_ne!(
            value.get("decision").and_then(|d| d.as_str()),
            Some("block"),
            "marker absent → no block expected; got: {}",
            stdout
        );
    }
}

// --- check_autonomous_in_progress ---

#[test]
fn check_autonomous_in_progress_returns_no_block_when_state_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.json");
    let result = check_autonomous_in_progress(&path);
    assert!(!result.should_block);
    assert!(result.skill.is_none());
    assert!(result.context.is_none());
}

#[test]
fn check_autonomous_in_progress_returns_no_block_when_state_unparseable() {
    let dir = tempfile::tempdir().unwrap();
    // Empty file
    let empty = dir.path().join("empty.json");
    fs::write(&empty, "").unwrap();
    assert!(!check_autonomous_in_progress(&empty).should_block);

    // Invalid JSON
    let invalid = dir.path().join("invalid.json");
    fs::write(&invalid, "{not json").unwrap();
    assert!(!check_autonomous_in_progress(&invalid).should_block);

    // Wrong root type (array)
    let array = dir.path().join("array.json");
    fs::write(&array, "[1, 2, 3]").unwrap();
    assert!(!check_autonomous_in_progress(&array).should_block);
}

#[test]
fn check_autonomous_in_progress_returns_no_block_when_current_phase_empty() {
    let dir = tempfile::tempdir().unwrap();
    // Empty current_phase string
    let empty_phase = dir.path().join("empty_phase.json");
    let state = json!({"current_phase": ""});
    fs::write(&empty_phase, serde_json::to_string(&state).unwrap()).unwrap();
    assert!(!check_autonomous_in_progress(&empty_phase).should_block);

    // Missing current_phase key
    let missing_phase = dir.path().join("missing_phase.json");
    let state = json!({"branch": "feat"});
    fs::write(&missing_phase, serde_json::to_string(&state).unwrap()).unwrap();
    assert!(!check_autonomous_in_progress(&missing_phase).should_block);
}

#[test]
fn check_autonomous_in_progress_returns_no_block_when_phase_status_pending() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    // Transition window: current_phase advanced but phase_enter not yet
    // run, so status is still "pending".
    let state = json!({
        "current_phase": "flow-code",
        "phases": {"flow-code": {"status": "pending"}},
        "skills": {"flow-code": "auto"}
    });
    fs::write(&path, serde_json::to_string(&state).unwrap()).unwrap();
    assert!(!check_autonomous_in_progress(&path).should_block);
}

#[test]
fn check_autonomous_in_progress_returns_no_block_when_phase_status_complete() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let state = json!({
        "current_phase": "flow-code",
        "phases": {"flow-code": {"status": "complete"}},
        "skills": {"flow-code": "auto"}
    });
    fs::write(&path, serde_json::to_string(&state).unwrap()).unwrap();
    assert!(!check_autonomous_in_progress(&path).should_block);
}

#[test]
fn check_autonomous_in_progress_returns_no_block_when_skill_config_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let state = json!({
        "current_phase": "flow-code",
        "phases": {"flow-code": {"status": "in_progress"}},
        "skills": {}
    });
    fs::write(&path, serde_json::to_string(&state).unwrap()).unwrap();
    assert!(!check_autonomous_in_progress(&path).should_block);
}

#[test]
fn check_autonomous_in_progress_returns_no_block_when_skill_simple_manual() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let state = json!({
        "current_phase": "flow-code",
        "phases": {"flow-code": {"status": "in_progress"}},
        "skills": {"flow-code": "manual"}
    });
    fs::write(&path, serde_json::to_string(&state).unwrap()).unwrap();
    assert!(!check_autonomous_in_progress(&path).should_block);
}

#[test]
fn check_autonomous_in_progress_returns_no_block_when_skill_detailed_manual() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let state = json!({
        "current_phase": "flow-code",
        "phases": {"flow-code": {"status": "in_progress"}},
        "skills": {"flow-code": {"continue": "manual"}}
    });
    fs::write(&path, serde_json::to_string(&state).unwrap()).unwrap();
    assert!(!check_autonomous_in_progress(&path).should_block);
}

#[test]
fn check_autonomous_in_progress_blocks_when_in_progress_and_skill_simple_auto() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let state = json!({
        "current_phase": "flow-code",
        "phases": {"flow-code": {"status": "in_progress"}},
        "skills": {"flow-code": "auto"}
    });
    fs::write(&path, serde_json::to_string(&state).unwrap()).unwrap();
    let result = check_autonomous_in_progress(&path);
    assert!(result.should_block);
    assert_eq!(result.skill.as_deref(), Some("autonomous-stop-refused"));
    assert!(result.context.is_some());
}

#[test]
fn check_autonomous_in_progress_blocks_when_in_progress_and_skill_detailed_auto() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let state = json!({
        "current_phase": "flow-code",
        "phases": {"flow-code": {"status": "in_progress"}},
        "skills": {"flow-code": {"continue": "auto", "commit": "auto"}}
    });
    fs::write(&path, serde_json::to_string(&state).unwrap()).unwrap();
    let result = check_autonomous_in_progress(&path);
    assert!(result.should_block);
    assert_eq!(result.skill.as_deref(), Some("autonomous-stop-refused"));
    assert!(result.context.is_some());
}

#[test]
fn check_autonomous_in_progress_block_message_names_current_phase() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let state = json!({
        "current_phase": "flow-code",
        "phases": {"flow-code": {"status": "in_progress"}},
        "skills": {"flow-code": "auto"}
    });
    fs::write(&path, serde_json::to_string(&state).unwrap()).unwrap();
    let result = check_autonomous_in_progress(&path);
    let context = result.context.expect("context present");
    assert!(
        context.contains("flow-code"),
        "context must name current phase: {}",
        context
    );
}

#[test]
fn check_autonomous_in_progress_block_message_mentions_flow_abort() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let state = json!({
        "current_phase": "flow-code",
        "phases": {"flow-code": {"status": "in_progress"}},
        "skills": {"flow-code": "auto"}
    });
    fs::write(&path, serde_json::to_string(&state).unwrap()).unwrap();
    let result = check_autonomous_in_progress(&path);
    let context = result.context.expect("context present");
    assert!(
        context.contains("/flow:flow-abort"),
        "context must mention /flow:flow-abort: {}",
        context
    );
}

// State file with current_phase=flow-code, status=in_progress,
// skills.flow-code="auto", _stop_instructed=true (so check_first_stop
// falls through), no _continue_pending → run() invokes
// check_autonomous_in_progress which blocks with skill
// "autonomous-stop-refused". The output-formatting matcher routes the
// block context directly as the reason (bypass format_block_output).
#[test]
fn subprocess_stop_hook_blocks_voluntary_stop_in_autonomous_in_progress_phase() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    Command::new("git")
        .args(["init", "--initial-branch", "main"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "t@t"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "t"])
        .current_dir(&root)
        .output()
        .unwrap();
    fs::write(root.join("README.md"), "x").unwrap();
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&root)
        .output()
        .unwrap();

    let state_dir = root.join(".flow-states");
    fs::create_dir_all(state_dir.join("main")).unwrap();
    let state = json!({
        "branch": "main",
        "current_phase": "flow-code",
        "phases": {"flow-code": {"status": "in_progress"}},
        "skills": {"flow-code": "auto"},
        "_stop_instructed": true
    });
    fs::write(
        state_dir.join("main").join("state.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();

    let (code, stdout, _stderr) = run_hook(&root, r#"{"session_id": "s1"}"#);
    assert_eq!(code, 0);
    let last = stdout
        .lines()
        .rfind(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON in stdout: {}", stdout));
    let json: Value = serde_json::from_str(last).unwrap();
    assert_eq!(json["decision"], "block");
    let reason = json["reason"].as_str().unwrap_or("");
    assert!(
        reason.contains("Autonomous mode"),
        "reason must say `Autonomous mode`: {}",
        reason
    );
    assert!(
        reason.contains("/flow:flow-abort"),
        "reason must mention /flow:flow-abort: {}",
        reason
    );
}

// --- check_autonomous_in_progress normalization (security-gates.md) ---
//
// Each test below asserts the gate still blocks under a state-file value
// that defeats raw byte equality: trailing/leading whitespace, mixed case,
// embedded NUL. Without normalization a hand-edited or hostile state file
// could silently bypass the autonomous-stop gate per security-gates.md
// "Normalize Before Comparing".

fn write_state(path: &std::path::Path, status: Value, skill: Value) {
    let state = json!({
        "current_phase": "flow-code",
        "phases": {"flow-code": {"status": status}},
        "skills": {"flow-code": skill},
    });
    fs::write(path, serde_json::to_string(&state).unwrap()).unwrap();
}

#[test]
fn check_autonomous_in_progress_blocks_when_status_has_trailing_whitespace() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    write_state(&path, json!("in_progress "), json!("auto"));
    assert!(
        check_autonomous_in_progress(&path).should_block,
        "trailing-whitespace status must normalize and still block"
    );
}

#[test]
fn check_autonomous_in_progress_blocks_when_status_has_leading_whitespace() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    write_state(&path, json!(" in_progress"), json!("auto"));
    assert!(
        check_autonomous_in_progress(&path).should_block,
        "leading-whitespace status must normalize and still block"
    );
}

#[test]
fn check_autonomous_in_progress_blocks_when_status_is_mixed_case() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    write_state(&path, json!("In_Progress"), json!("auto"));
    assert!(
        check_autonomous_in_progress(&path).should_block,
        "mixed-case status must normalize (case-fold) and still block"
    );
}

#[test]
fn check_autonomous_in_progress_blocks_when_status_has_embedded_nul() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    write_state(&path, json!("in_progress\u{0000}"), json!("auto"));
    assert!(
        check_autonomous_in_progress(&path).should_block,
        "NUL-padded status must normalize and still block"
    );
}

#[test]
fn check_autonomous_in_progress_blocks_when_simple_skill_has_trailing_whitespace() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    write_state(&path, json!("in_progress"), json!("auto "));
    assert!(
        check_autonomous_in_progress(&path).should_block,
        "trailing-whitespace Simple skill must normalize and still block"
    );
}

#[test]
fn check_autonomous_in_progress_blocks_when_simple_skill_is_uppercase() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    write_state(&path, json!("in_progress"), json!("AUTO"));
    assert!(
        check_autonomous_in_progress(&path).should_block,
        "uppercase Simple skill must normalize and still block"
    );
}

#[test]
fn check_autonomous_in_progress_blocks_when_simple_skill_has_embedded_nul() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    write_state(&path, json!("in_progress"), json!("auto\u{0000}"));
    assert!(
        check_autonomous_in_progress(&path).should_block,
        "NUL-padded Simple skill must normalize and still block"
    );
}

#[test]
fn check_autonomous_in_progress_blocks_when_detailed_skill_continue_is_uppercase() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    write_state(&path, json!("in_progress"), json!({"continue": "AUTO"}));
    assert!(
        check_autonomous_in_progress(&path).should_block,
        "uppercase Detailed-form continue must normalize and still block"
    );
}

#[test]
fn check_autonomous_in_progress_blocks_when_detailed_skill_continue_has_trailing_whitespace() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    write_state(&path, json!("in_progress"), json!({"continue": "auto "}));
    assert!(
        check_autonomous_in_progress(&path).should_block,
        "trailing-whitespace Detailed-form continue must normalize and still block"
    );
}

// --- check_autonomous_in_progress oversized state file (byte cap) ---

#[test]
fn check_autonomous_in_progress_no_block_when_state_file_exceeds_byte_cap() {
    // Write a state file that exceeds STATE_FILE_BYTE_CAP (4 MB). The
    // capped read truncates mid-string, the JSON parse fails, and the
    // function fails open (no block). This proves the cap actually
    // bounds the read — without it, the read would consume the full
    // 5 MB and only fail at parse time after a successful unbounded
    // allocation.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    // 5 MB padding inside a string field, wrapping a state otherwise
    // valid for blocking. The padding pushes the file past the 4 MB cap.
    let padding = "x".repeat(5 * 1024 * 1024);
    let state = json!({
        "current_phase": "flow-code",
        "phases": {"flow-code": {"status": "in_progress"}},
        "skills": {"flow-code": "auto"},
        "padding": padding,
    });
    fs::write(&path, serde_json::to_string(&state).unwrap()).unwrap();
    let result = check_autonomous_in_progress(&path);
    assert!(
        !result.should_block,
        "state file exceeding byte cap must fail-open (truncated read produces malformed JSON)"
    );
}

// --- body_has_question_outside_code (unit tests) ---

#[test]
fn body_with_question_outside_code_detects_question() {
    assert!(body_has_question_outside_code("Should I proceed?"));
}

#[test]
fn body_without_question_returns_false() {
    assert!(!body_has_question_outside_code(
        "Implementing the helper now."
    ));
}

#[test]
fn body_with_question_only_in_fenced_block_returns_false() {
    let body = "Implementing now.\n\n```bash\nif [ -f $X ]; then echo \"?\"; fi\n```\n";
    assert!(!body_has_question_outside_code(body));
}

#[test]
fn body_with_question_only_in_inline_code_returns_false() {
    let body = "The shell `?` glob matches one character.";
    assert!(!body_has_question_outside_code(body));
}

#[test]
fn body_with_interrobang_detects_structural_question() {
    // `?!` is treated as a structural question — the `?` is what the
    // detector keys off, and an interrobang is just a question with
    // emphasis. The autonomous-mode failure mode does not exclude
    // emphatic phrasings.
    assert!(body_has_question_outside_code("Really?! Are you sure?"));
}

#[test]
fn body_empty_returns_false() {
    assert!(!body_has_question_outside_code(""));
}

#[test]
fn body_with_question_after_fenced_block_detects() {
    let body = "Here is the diff:\n\n```diff\n+ added line\n- removed line\n```\n\n\
                Should I commit this now?";
    assert!(body_has_question_outside_code(body));
}

#[test]
fn body_with_url_query_string_returns_false() {
    // The `?` in a URL query string is not a prose question — it is
    // followed by an alphanumeric character (the parameter name).
    // The detector must not falsely fire on URLs in legitimate
    // task-summary text.
    let body = "Done. See https://example.com?foo=bar for details.";
    assert!(
        !body_has_question_outside_code(body),
        "URL query string should NOT register as a prose question"
    );
}

#[test]
fn body_with_url_at_end_of_sentence_returns_false() {
    let body = "Filed at https://github.com/owner/repo/issues/42?tab=comments. Done.";
    assert!(
        !body_has_question_outside_code(body),
        "URL with `?tab=...` mid-prose should not register as a question"
    );
}

#[test]
fn body_with_unclosed_inline_backtick_detects_question() {
    // `Should I use `option_a?` — single backtick opens an inline
    // span that never closes. The previous implementation tracked
    // in_inline state and swallowed the `?` after the unclosed
    // backtick. Counting backticks per line and falling back to
    // no-inline tracking when odd handles this naturally.
    let body = "Should I use `option_a?";
    assert!(
        body_has_question_outside_code(body),
        "Unclosed inline backtick must not swallow trailing `?`"
    );
}

#[test]
fn body_with_question_inside_paired_backticks_returns_false() {
    // Sanity check: paired backticks still suppress the `?` between
    // them (e.g., "the `?` operator"). Only odd counts disable
    // tracking.
    let body = "The `?` operator propagates errors in Rust.";
    assert!(
        !body_has_question_outside_code(body),
        "`?` inside paired backticks must still be suppressed"
    );
}

// --- check_prose_pause_at_task_entry (integration tests) ---

/// Build a minimal state JSON for the prose-pause guard.
/// `code_task` defaults to `0`, `_continue_pending` defaults to empty,
/// and the skill config defaults to `"auto"` shape so individual
/// tests can override only what they need.
fn prose_pause_state(
    current_phase: &str,
    phase_status: &str,
    skill_config: Value,
    code_task: i64,
    continue_pending: &str,
) -> Value {
    json!({
        "branch": "test",
        "current_phase": current_phase,
        "phases": { current_phase: { "status": phase_status } },
        "skills": { current_phase: skill_config },
        "code_task": code_task,
        "_continue_pending": continue_pending,
    })
}

/// Write a single-turn assistant transcript JSONL to `path`. The
/// `text` block carries `body`; `had_tool_use=true` adds a tool_use
/// block so the test can flip condition 7 independently.
fn write_assistant_transcript(path: &Path, body: &str, had_tool_use: bool) {
    let mut content = vec![json!({"type": "text", "text": body})];
    if had_tool_use {
        content.push(json!({
            "type": "tool_use",
            "id": "tool-id",
            "name": "Bash",
            "input": {"command": "echo hi"}
        }));
    }
    let line = json!({
        "type": "assistant",
        "message": { "content": content }
    });
    fs::write(path, serde_json::to_string(&line).unwrap()).unwrap();
}

/// Construct a transcript path under `<home>/.claude/projects/` so
/// `check_prose_pause_at_task_entry`'s `is_safe_transcript_path`
/// validator accepts it. Tests pass `dir.path()` as `home` to
/// isolate from the real `$HOME` per
/// `.claude/rules/testing-gotchas.md` "Rust Parallel Test Env Var
/// Races." Creates the parent directory; caller writes the
/// transcript content (or leaves it absent for missing-file tests).
fn safe_transcript_path(home: &Path, name: &str) -> PathBuf {
    let projects = home.join(".claude").join("projects");
    fs::create_dir_all(&projects).unwrap();
    projects.join(name)
}

#[test]
fn prose_pause_blocks_when_all_seven_conditions_match() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_assistant_transcript(&transcript, "Should I proceed with Task 1?", false);

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(result.should_block);
    let context = result.context.unwrap();
    assert!(context.contains("prose-based pause"));
    assert!(context.contains("autonomous-flow-self-recovery.md"));
    assert!(context.contains("autonomous-phase-discipline.md"));
}

#[test]
fn prose_pause_allows_when_phase_is_not_flow_code() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-learn",
            "in_progress",
            json!("auto"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_assistant_transcript(&transcript, "Should I proceed?", false);

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(!result.should_block);
}

#[test]
fn prose_pause_allows_when_status_not_in_progress() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "complete",
            json!("auto"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_assistant_transcript(&transcript, "Should I proceed?", false);

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(!result.should_block);
}

#[test]
fn prose_pause_allows_when_skill_not_auto() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("manual"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_assistant_transcript(&transcript, "Should I proceed?", false);

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(!result.should_block);
}

#[test]
fn prose_pause_allows_when_code_task_advanced() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            5,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_assistant_transcript(&transcript, "Should I proceed?", false);

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(!result.should_block);
}

#[test]
fn prose_pause_allows_when_continue_pending_set() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            0,
            "commit",
        ))
        .unwrap(),
    )
    .unwrap();
    write_assistant_transcript(&transcript, "Should I proceed?", false);

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(!result.should_block);
}

#[test]
fn prose_pause_allows_when_last_assistant_had_tool_use() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    // Question text but the turn also fired a tool call — the model
    // is making progress, not stalling on a prose question.
    write_assistant_transcript(&transcript, "Should I proceed?", true);

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(!result.should_block);
}

#[test]
fn prose_pause_allows_when_no_question_in_text() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_assistant_transcript(&transcript, "Implementing the helper now.", false);

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(!result.should_block);
}

#[test]
fn prose_pause_allows_when_skill_config_detailed_object_auto() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!({"continue": "auto"}),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_assistant_transcript(&transcript, "Should I proceed?", false);

    // Detailed shape with continue=auto IS auto — guard fires.
    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(result.should_block);
}

#[test]
fn prose_pause_allows_when_no_transcript_path() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();

    let result = check_prose_pause_at_task_entry(&state_path, None, dir.path());
    assert!(!result.should_block);
}

#[test]
fn prose_pause_allows_when_state_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("nonexistent.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    write_assistant_transcript(&transcript, "Should I proceed?", false);

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(!result.should_block);
}

#[test]
fn prose_pause_allows_when_transcript_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    let transcript = safe_transcript_path(dir.path(), "missing.jsonl");

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(!result.should_block);
}

#[test]
fn prose_pause_allows_when_state_unparseable() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    fs::write(&state_path, "{not valid json").unwrap();
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    write_assistant_transcript(&transcript, "Should I proceed?", false);

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(!result.should_block);
}

#[test]
fn prose_pause_allows_when_transcript_file_unreadable() {
    // `is_safe_transcript_path` canonicalize succeeds on a chmod-000
    // file (canonicalize stats components, not opens), but
    // `last_assistant_text_and_tool_use`'s `File::open` returns
    // Err(PermissionDenied). The function returns None and the
    // pause check allows. Covers the File::open `.ok()?` branch in
    // last_assistant_text_and_tool_use which became unreachable
    // through normal validator-passes-but-file-missing paths after
    // is_safe_transcript_path was tightened to canonicalize.
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    fs::write(&transcript, b"{\"type\":\"user\"}\n").unwrap();
    fs::set_permissions(&transcript, fs::Permissions::from_mode(0o000)).unwrap();
    struct PermGuard(std::path::PathBuf);
    impl Drop for PermGuard {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o644));
        }
    }
    let _g = PermGuard(transcript.clone());

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(!result.should_block);
}

#[test]
fn prose_pause_allows_when_transcript_has_no_assistant_turn() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    // Only a user turn — the walker returns None on user-before-assistant.
    let user_line = json!({
        "type": "user",
        "message": { "content": [{"type": "text", "text": "Start the work"}] }
    });
    fs::write(&transcript, serde_json::to_string(&user_line).unwrap()).unwrap();

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(!result.should_block);
}

#[test]
fn prose_pause_allows_when_transcript_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    // Empty transcript file — walker exhausts the buffer without
    // finding any turn, returns None. Covers the bottom `None` arm
    // of last_assistant_text_and_tool_use.
    fs::write(&transcript, "").unwrap();

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(!result.should_block);
}

#[test]
fn prose_pause_blocks_when_transcript_has_blank_lines_before_assistant() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    // Blank lines exercise the `if trimmed.is_empty() { continue; }`
    // arm of the walker — the assistant turn after the blanks is what
    // the walker sees first when scanning backward.
    let assistant = json!({
        "type": "assistant",
        "message": { "content": [{"type": "text", "text": "Should I proceed?"}] }
    });
    let body = format!(
        "\n\n{}\n\n   \n",
        serde_json::to_string(&assistant).unwrap()
    );
    fs::write(&transcript, body).unwrap();

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(result.should_block);
}

#[test]
fn prose_pause_blocks_when_invalid_json_is_after_assistant_turn() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    // Walker scans backward, so the LAST line is seen first. A
    // truncated/corrupted last line forces the walker to consume the
    // Err(_) => continue arm before reaching the valid assistant
    // turn behind it.
    let assistant = json!({
        "type": "assistant",
        "message": { "content": [{"type": "text", "text": "Should I proceed?"}] }
    });
    let body = format!(
        "{}\n{{not-valid-json\n",
        serde_json::to_string(&assistant).unwrap()
    );
    fs::write(&transcript, body).unwrap();

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(result.should_block);
}

#[test]
fn prose_pause_blocks_when_text_block_missing_text_field() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    // First text block lacks the "text" field — exercises the None
    // arm of `if let Some(t) = block.get("text").and_then(...)`. The
    // walker's content loop continues to the next block which carries
    // the question.
    let assistant = json!({
        "type": "assistant",
        "message": { "content": [
            {"type": "text"},
            {"type": "text", "text": "Should I proceed?"}
        ]}
    });
    fs::write(&transcript, serde_json::to_string(&assistant).unwrap()).unwrap();

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(result.should_block);
}

#[test]
fn prose_pause_blocks_when_transcript_has_invalid_json_lines() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    // A corrupted/truncated line followed by a valid assistant turn.
    // The walker skips the unparseable line via Err(_) => continue.
    let assistant = json!({
        "type": "assistant",
        "message": { "content": [{"type": "text", "text": "Should I proceed?"}] }
    });
    let body = format!(
        "{{not-valid-json\n{}\n",
        serde_json::to_string(&assistant).unwrap()
    );
    fs::write(&transcript, body).unwrap();

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(result.should_block);
}

#[test]
fn prose_pause_blocks_when_transcript_has_system_turn_before_assistant() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    // A non-user, non-assistant turn (e.g. "system" or "summary") is
    // skipped by the `turn_type != "assistant"` continue arm.
    let system = json!({"type": "system", "message": "session-start"});
    let assistant = json!({
        "type": "assistant",
        "message": { "content": [{"type": "text", "text": "Should I proceed?"}] }
    });
    let body = format!(
        "{}\n{}\n",
        serde_json::to_string(&assistant).unwrap(),
        serde_json::to_string(&system).unwrap(),
    );
    fs::write(&transcript, body).unwrap();

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(result.should_block);
}

#[test]
fn prose_pause_blocks_when_assistant_turn_has_no_content_array() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    // First assistant turn (most recent, walked-to first) has no
    // `message.content` array. Walker skips it via `None => continue`
    // and reaches the second assistant turn behind it.
    let bad_assistant = json!({"type": "assistant", "message": "string-not-array"});
    let good_assistant = json!({
        "type": "assistant",
        "message": { "content": [{"type": "text", "text": "Should I proceed?"}] }
    });
    let body = format!(
        "{}\n{}\n",
        serde_json::to_string(&good_assistant).unwrap(),
        serde_json::to_string(&bad_assistant).unwrap(),
    );
    fs::write(&transcript, body).unwrap();

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(result.should_block);
}

#[test]
fn prose_pause_blocks_with_multi_text_block_assistant_turn() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    // Multiple text blocks in one assistant turn — exercises the
    // `text.push('\n')` separator arm. The question lives in the
    // second block.
    let assistant = json!({
        "type": "assistant",
        "message": { "content": [
            {"type": "text", "text": "Working on task 1."},
            {"type": "text", "text": "Should I proceed with the next step?"}
        ]}
    });
    fs::write(&transcript, serde_json::to_string(&assistant).unwrap()).unwrap();

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(result.should_block);
}

#[test]
fn prose_pause_allows_when_transcript_has_non_utf8_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    // Write invalid UTF-8 bytes — `read_to_string` returns
    // io::Error (InvalidData), exercising the `.ok()?` failure arm
    // of last_assistant_text_and_tool_use's read step.
    fs::write(&transcript, [0xFFu8, 0xFE, 0xFD, 0xFC]).unwrap();

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(!result.should_block);
}

#[test]
fn prose_pause_blocks_with_thinking_block_alongside_text() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    // A "thinking" block (or any other non-text non-tool_use type)
    // exercises the implicit else arm of the block-type match —
    // neither text accumulation nor had_tool_use is updated, the
    // walker continues to the next block.
    let assistant = json!({
        "type": "assistant",
        "message": { "content": [
            {"type": "thinking", "thinking": "internal monologue"},
            {"type": "text", "text": "Should I proceed?"}
        ]}
    });
    fs::write(&transcript, serde_json::to_string(&assistant).unwrap()).unwrap();

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(result.should_block);
}

#[test]
fn prose_pause_allows_when_transcript_path_outside_safe_prefix() {
    // A transcript path NOT under `<home>/.claude/projects/`
    // must be rejected by `is_safe_transcript_path`. Without this
    // gate, the walker would happily open any user-readable file —
    // an arbitrary-file-read surface per
    // `.claude/rules/external-input-path-construction.md`.
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    // Transcript at `<dir>/evil.jsonl` — outside the canonical
    // `<dir>/.claude/projects/` prefix.
    let evil = dir.path().join("evil.jsonl");
    write_assistant_transcript(&evil, "Should I proceed?", false);

    let result =
        check_prose_pause_at_task_entry(&state_path, Some(evil.to_str().unwrap()), dir.path());
    assert!(
        !result.should_block,
        "transcript_path outside ~/.claude/projects/ must be rejected before any open"
    );
}

#[test]
fn prose_pause_allows_when_transcript_path_uses_parent_dir_traversal() {
    // Even with a path that lexically starts with the canonical
    // prefix, `..` components must be rejected — the validator
    // performs a lexical (non-canonicalizing) prefix check, and
    // ParentDir components would let a hostile path escape via
    // `<home>/.claude/projects/../../etc/passwd`.
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    // Construct path with `..` segments, even though the prefix
    // appears to match.
    let traversal = dir
        .path()
        .join(".claude")
        .join("projects")
        .join("..")
        .join("..")
        .join("etc")
        .join("passwd");

    let result =
        check_prose_pause_at_task_entry(&state_path, Some(traversal.to_str().unwrap()), dir.path());
    assert!(
        !result.should_block,
        "Path with `..` components must be rejected by is_safe_transcript_path"
    );
}

#[cfg(unix)]
#[test]
fn prose_pause_allows_when_transcript_is_fifo_outside_safe_prefix() {
    // A FIFO at a path outside the safe prefix is rejected before
    // any open(2) — preventing the Stop hook from blocking
    // indefinitely on a read-only FIFO with no writer. The validator
    // gate is enough; no FIFO-specific logic is needed.
    use std::ffi::CString;
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    fs::write(
        &state_path,
        serde_json::to_string(&prose_pause_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            0,
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    // Create a FIFO at a path outside the safe prefix.
    let fifo = dir.path().join("evil.fifo");
    let cpath = CString::new(fifo.to_str().unwrap()).unwrap();
    let rc = unsafe { libc::mkfifo(cpath.as_ptr(), 0o644) };
    assert_eq!(rc, 0, "mkfifo failed");

    let result =
        check_prose_pause_at_task_entry(&state_path, Some(fifo.to_str().unwrap()), dir.path());
    assert!(
        !result.should_block,
        "FIFO at path outside safe prefix must be rejected before open"
    );
}

#[test]
fn prose_pause_allows_when_skills_key_missing() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "transcript.jsonl");
    // State has no `skills` key at all — exercises the
    // `None => false` arm of the skill-config match.
    let state = json!({
        "branch": "test",
        "current_phase": "flow-code",
        "phases": { "flow-code": { "status": "in_progress" } },
        "code_task": 0,
        "_continue_pending": ""
    });
    fs::write(&state_path, serde_json::to_string(&state).unwrap()).unwrap();
    write_assistant_transcript(&transcript, "Should I proceed?", false);

    let result = check_prose_pause_at_task_entry(
        &state_path,
        Some(transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(!result.should_block);
}

// --- check_in_progress_utility_skill ---

const UTIL_SKILL: &str = "flow:flow-plan";
const UTIL_SESSION: &str = "abc12345";

fn write_utility_marker(home: &Path, skill: &str, session_id: &str) {
    let dir = home.join(".claude").join("flow");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("utility-in-progress-{}.json", session_id));
    let payload = json!({
        "skill": skill,
        "session_id": session_id,
        "started_at": "2026-05-09T12:00:00-07:00",
    });
    fs::write(&path, serde_json::to_string(&payload).unwrap()).unwrap();
}

#[test]
fn utility_skill_no_marker_no_block() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let result = check_in_progress_utility_skill(UTIL_SESSION, None, &home);
    assert!(
        !result.should_block,
        "no marker → no block (the skill is not running)"
    );
    assert!(result.skill.is_none());
    assert!(result.context.is_none());
}

#[test]
fn check_in_progress_utility_skill_ignores_orphan_marker_from_different_session() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    // Marker for a different session_id (e.g., a crashed concurrent
    // Claude Code session) must not affect THIS session.
    write_utility_marker(&home, UTIL_SKILL, "other_session_id");
    let result = check_in_progress_utility_skill(UTIL_SESSION, None, &home);
    assert!(
        !result.should_block,
        "marker for different session_id must be ignored as orphaned"
    );
}

#[test]
fn check_in_progress_utility_skill_predicate_runs_after_check_continue() {
    // Ordering invariant: the predicate must be composed AFTER
    // `check_continue` (so multi-child-skill chains route through
    // check_continue first) and BEFORE
    // `check_prose_pause_at_task_entry` (so the more specific
    // task-entry message wins for that shape). Per the plan, the
    // predicate slots between those two predicates in `run()`.
    let src_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("hooks")
        .join("stop_continue.rs");
    let content = fs::read_to_string(&src_path).expect("read stop_continue.rs");
    // Bound the search to the body of `pub fn run()` so a stray
    // mention in a doc comment cannot satisfy the assertion.
    let tail = content
        .split_once("pub fn run() {")
        .map(|(_, t)| t)
        .expect("stop_continue.rs must define `pub fn run()`");
    let body = tail.split_once("\n}\n").map(|(b, _)| b).unwrap_or(tail);
    let idx_continue = body
        .find("check_continue(")
        .expect("run() must call check_continue");
    let idx_utility = body
        .find("check_in_progress_utility_skill(")
        .expect("run() must call check_in_progress_utility_skill");
    let idx_prose = body
        .find("check_prose_pause_at_task_entry(")
        .expect("run() must call check_prose_pause_at_task_entry");
    assert!(
        idx_continue < idx_utility,
        "check_in_progress_utility_skill must run AFTER check_continue"
    );
    assert!(
        idx_utility < idx_prose,
        "check_in_progress_utility_skill must run BEFORE check_prose_pause_at_task_entry"
    );
}

#[test]
fn check_in_progress_utility_skill_no_block_when_session_id_empty() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let result = check_in_progress_utility_skill("", None, &home);
    assert!(
        !result.should_block,
        "empty session_id → no marker path → no block"
    );
}

#[test]
fn utility_skill_marker_present_invalid_session_id_no_block() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    // Path-traversal session_id must be rejected by marker_path
    // BEFORE filesystem access so a hostile hook input cannot
    // redirect the read.
    let result = check_in_progress_utility_skill("..", None, &home);
    assert!(!result.should_block);
    let result = check_in_progress_utility_skill("abc/def", None, &home);
    assert!(!result.should_block);
}

#[test]
fn check_in_progress_utility_skill_no_block_when_marker_unparseable() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let marker_dir = home.join(".claude").join("flow");
    fs::create_dir_all(&marker_dir).unwrap();
    let marker = marker_dir.join(format!("utility-in-progress-{}.json", UTIL_SESSION));
    // Corrupted JSON must not cause the predicate to panic or block.
    fs::write(&marker, "{not json").unwrap();
    let result = check_in_progress_utility_skill(UTIL_SESSION, None, &home);
    assert!(
        !result.should_block,
        "corrupted marker must fail-open (no block)"
    );
}

#[test]
fn check_in_progress_utility_skill_no_block_when_skill_not_in_known_set() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    // A marker that names a skill OUTSIDE the MULTI_STEP_UTILITY_SKILLS
    // allowlist must not block — a future skill or a stale marker from
    // a removed feature must not silently keep blocking.
    write_utility_marker(&home, "flow:flow-some-other-skill", UTIL_SESSION);
    let result = check_in_progress_utility_skill(UTIL_SESSION, None, &home);
    assert!(
        !result.should_block,
        "marker naming an unknown skill must not block"
    );
}

#[test]
fn utility_skill_marker_skill_field_normalized_before_allowlist_check() {
    // Per `.claude/rules/security-gates.md` "Normalize Before Comparing",
    // the marker `skill` field is a state-derived string (hand-editable
    // JSON) and must be NUL-stripped, whitespace-trimmed, and
    // ASCII-lowercased before comparison against the canonical
    // allowlist of lowercase skill names. Without normalization, a
    // hand-edit or whitespace-padded marker would silently fail-open
    // and the gate would never fire.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    // Marker skill carries trailing whitespace AND mixed case — both
    // must be normalized to the canonical `flow:flow-plan`.
    write_utility_marker(&home, "  FLOW:Flow-Plan  ", UTIL_SESSION);
    let transcript = transcript_with_skill_calls(&home, &["decompose:decompose"]);
    let result = check_in_progress_utility_skill(UTIL_SESSION, transcript.to_str(), &home);
    assert!(
        result.should_block,
        "marker skill with whitespace/case noise must normalize and match the allowlist",
    );
}

#[test]
fn check_in_progress_utility_skill_no_block_when_marker_session_id_mismatches() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let marker_dir = home.join(".claude").join("flow");
    fs::create_dir_all(&marker_dir).unwrap();
    // Hand-craft a marker file whose path session_id matches but
    // whose internal `session_id` field does not — defends against
    // a hostile or corrupted file that claims to belong to another
    // session.
    let marker = marker_dir.join(format!("utility-in-progress-{}.json", UTIL_SESSION));
    let payload = json!({
        "skill": UTIL_SKILL,
        "session_id": "different",
        "started_at": "2026-05-09T12:00:00-07:00",
    });
    fs::write(&marker, serde_json::to_string(&payload).unwrap()).unwrap();
    let result = check_in_progress_utility_skill(UTIL_SESSION, None, &home);
    assert!(
        !result.should_block,
        "marker whose internal session_id mismatches must not block"
    );
}

// --- new predicate behavior: decompose-return gating ---
//
// The predicate gates on TWO signals: (a) the per-session
// utility-in-progress marker file, AND (b) `decompose:decompose`
// being the most recent Skill `tool_use` in the persisted transcript
// since the most recent real user turn. The marker is a precondition
// — without it, no block under any transcript. The transcript walker
// discriminates "decompose just returned mid-pipeline" (block) from
// "model just sent a normal conversational reply" (no block) so
// discussion-mode replies end the turn cleanly.

/// Write a transcript JSONL fixture containing the supplied skill
/// calls, mirroring the canonical layout under
/// `<home>/.claude/projects/<project_id>/session.jsonl`. Returns the
/// transcript path. Each `skills_after_user` entry produces one
/// assistant Skill tool_use turn after the user turn.
fn transcript_with_skill_calls(home: &Path, skills_after_user: &[&str]) -> PathBuf {
    let mut jsonl =
        String::from("{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n");
    for skill in skills_after_user {
        jsonl.push_str(&format!(
            "{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{{\"skill\":\"{}\"}}}}]}}}}\n",
            skill,
        ));
    }
    crate::common::transcript_fixture(home, "p", &jsonl)
}

#[test]
fn utility_skill_marker_present_no_transcript_no_block() {
    // Marker is present but transcript_path is None (the Stop hook
    // received no transcript_path in its stdin payload). The predicate
    // cannot determine whether decompose returned recently, so it
    // fails-open to no_block — matching the discussion-mode contract
    // that normal replies end the turn cleanly.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    write_utility_marker(&home, UTIL_SKILL, UTIL_SESSION);
    let result = check_in_progress_utility_skill(UTIL_SESSION, None, &home);
    assert!(
        !result.should_block,
        "no transcript_path → no decompose-return detection → no block"
    );
}

#[test]
fn utility_skill_marker_present_no_decompose_call_no_block() {
    // AC#1: marker present, transcript shows the user just typed and
    // the model replied with text (no Skill calls). This is a normal
    // discussion-mode reply; the predicate must not refuse turn-end.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    write_utility_marker(&home, UTIL_SKILL, UTIL_SESSION);
    let transcript = transcript_with_skill_calls(&home, &[]);
    let result = check_in_progress_utility_skill(UTIL_SESSION, transcript.to_str(), &home);
    assert!(
        !result.should_block,
        "marker + no decompose call since user → no block (AC#1)"
    );
}

#[test]
fn utility_skill_marker_present_decompose_most_recent_blocks() {
    // AC#2: marker present, transcript shows `decompose:decompose` as
    // the most recent Skill call since the user typed. The predicate
    // must refuse turn-end so the model continues from decompose's
    // return straight to drafting and filing the issue.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    write_utility_marker(&home, UTIL_SKILL, UTIL_SESSION);
    let transcript = transcript_with_skill_calls(&home, &["decompose:decompose"]);
    let result = check_in_progress_utility_skill(UTIL_SESSION, transcript.to_str(), &home);
    assert!(
        result.should_block,
        "marker + decompose most recent → block (AC#2)"
    );
    assert_eq!(result.skill.as_deref(), Some("utility-in-progress"));
}

#[test]
fn utility_skill_marker_present_pm_after_decompose_no_block() {
    // AC#3: marker present, transcript shows `decompose:decompose`
    // followed by `flow:pm`. The planning-persona sub-agent call is
    // the most recent Skill, so the predicate must NOT block — the
    // user reacts in the next message and discussion continues.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    write_utility_marker(&home, UTIL_SKILL, UTIL_SESSION);
    let transcript = transcript_with_skill_calls(&home, &["decompose:decompose", "flow:pm"]);
    let result = check_in_progress_utility_skill(UTIL_SESSION, transcript.to_str(), &home);
    assert!(
        !result.should_block,
        "marker + pm after decompose → no block (AC#3, last-Skill-wins)"
    );
}

#[test]
fn utility_skill_block_message_is_encouraging_string() {
    // AC#7: when the predicate blocks, the context string MUST be the
    // exact encouraging-tone message — no rule citations, no contract
    // prose, no abort instructions. The full Stop hook routes this
    // string verbatim into the `decision: "block"` envelope's reason.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    write_utility_marker(&home, UTIL_SKILL, UTIL_SESSION);
    let transcript = transcript_with_skill_calls(&home, &["decompose:decompose"]);
    let result = check_in_progress_utility_skill(UTIL_SESSION, transcript.to_str(), &home);
    assert!(result.should_block);
    let context = result.context.expect("context must populate on block");
    assert_eq!(
        context,
        "Stop Refused: Continue, you can do it. Don't give up, you got this! No excuses!",
    );
}

#[test]
fn run_composition_emits_encouraging_message_for_utility_block() {
    // Golden test for the run() composition: when the marker is
    // present AND the transcript shows decompose as the most recent
    // skill, the Stop hook subprocess emits `{"decision":"block",
    // "reason":"<encouraging message>"}` verbatim. The reason field
    // must equal the AC#7 string with no surrounding "child skill
    // returned" framing.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    init_main_repo(&root);
    let session_id = "abc12345";
    // Write marker under HOME=<root> so set-utility-in-progress's
    // marker path resolves to <root>/.claude/flow/.
    let marker_dir = root.join(".claude").join("flow");
    fs::create_dir_all(&marker_dir).unwrap();
    let payload = json!({
        "skill": "flow:flow-decompose-project",
        "session_id": session_id,
        "started_at": "2026-05-09T12:00:00-07:00",
    });
    fs::write(
        marker_dir.join(format!("utility-in-progress-{}.json", session_id)),
        serde_json::to_string(&payload).unwrap(),
    )
    .unwrap();
    // Build a transcript under <root>/.claude/projects/p/session.jsonl
    // so the Stop hook subprocess (with HOME=<root>) passes the
    // validator's prefix check.
    let transcript = transcript_with_skill_calls(&root, &["decompose:decompose"]);
    let stdin = format!(
        r#"{{"session_id": "{}", "transcript_path": "{}"}}"#,
        session_id,
        transcript.display(),
    );
    let (code, stdout, _stderr) = run_hook_with_home(&root, &stdin, &root);
    assert_eq!(code, 0, "Stop hook exits 0 even when blocking");
    let parsed: Value =
        serde_json::from_str(&stdout).expect("Stop hook must emit JSON when blocking");
    assert_eq!(
        parsed["decision"], "block",
        "must refuse turn-end: {}",
        stdout
    );
    assert_eq!(
        parsed["reason"].as_str().unwrap_or(""),
        "Stop Refused: Continue, you can do it. Don't give up, you got this! No excuses!",
        "reason must match AC#7 verbatim with no wrapper framing",
    );
}

#[test]
fn check_in_progress_utility_skill_no_block_when_marker_path_is_symlink() {
    // The predicate uses `fs::symlink_metadata` (which does NOT follow
    // symlinks) and rejects entries that are symlinks or not regular
    // files. A symlink at the marker path — even one pointing at a
    // valid marker JSON elsewhere — must not block. Defends against
    // an attacker placing a symlink under `<home>/.claude/flow/` to
    // redirect the read to an arbitrary file.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let marker_dir = home.join(".claude").join("flow");
    fs::create_dir_all(&marker_dir).unwrap();
    // A real marker JSON elsewhere on disk — symlink target.
    let target = dir.path().join("real-marker.json");
    let payload = json!({
        "skill": UTIL_SKILL,
        "session_id": UTIL_SESSION,
        "started_at": "2026-05-09T12:00:00-07:00",
    });
    fs::write(&target, serde_json::to_string(&payload).unwrap()).unwrap();
    let marker = marker_dir.join(format!("utility-in-progress-{}.json", UTIL_SESSION));
    std::os::unix::fs::symlink(&target, &marker).unwrap();
    let result = check_in_progress_utility_skill(UTIL_SESSION, None, &home);
    assert!(
        !result.should_block,
        "symlink at marker path must not block — only regular files do"
    );
}

// --- end-to-end utility-skill marker integration ---

/// Initialize a git repo with a default branch, an empty author, and
/// an initial commit so `resolve_branch` resolves to `Some("main")`
/// during the Stop hook subprocess. Without an initial commit,
/// `git symbolic-ref HEAD` succeeds but `git branch --show-current`
/// returns empty before any commit lands.
fn init_main_repo(root: &Path) {
    Command::new("git")
        .args(["init", "--initial-branch", "main"])
        .current_dir(root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "t@t"])
        .current_dir(root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "t"])
        .current_dir(root)
        .output()
        .unwrap();
    fs::write(root.join("README.md"), "x").unwrap();
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(root)
        .output()
        .unwrap();
}

/// Spawn `bin/flow <subcommand> --skill X --session-id Y` with HOME
/// set to the test fixture's tempdir. Returns (exit_code, stdout).
fn run_marker_subcommand(
    subcommand: &str,
    home: &Path,
    skill: &str,
    session_id: &str,
) -> (i32, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.args([subcommand, "--skill", skill, "--session-id", session_id])
        .env_remove("FLOW_CI_RUNNING")
        .env("HOME", home)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = cmd.output().expect("spawn flow-rs subcommand");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
    )
}

/// Full end-to-end integration test for the utility-in-progress
/// marker lifecycle through the real CLI surface. Drives the same
/// path production utility skills (flow-explore, flow-plan,
/// flow-decompose-project) take:
/// 1. spawn `set-utility-in-progress` to write the marker
/// 2. spawn the Stop hook with matching session_id → must block
/// 3. spawn `clear-utility-in-progress` to remove the marker
/// 4. spawn the Stop hook again → must NOT block
#[test]
fn utility_marker_full_lifecycle_subprocess() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    init_main_repo(&root);
    let session_id = "abc12345";

    // 1. Write marker via the real CLI. Use `flow:flow-plan` —
    //    it's in `MULTI_STEP_UTILITY_SKILLS` so the Stop hook
    //    predicate honors the marker (flow-explore was removed
    //    because it never invokes `decompose:decompose`).
    let (code, stdout) = run_marker_subcommand(
        "set-utility-in-progress",
        &root,
        "flow:flow-plan",
        session_id,
    );
    assert_eq!(code, 0, "set must succeed: stdout={}", stdout);
    let marker = root
        .join(".claude")
        .join("flow")
        .join(format!("utility-in-progress-{}.json", session_id));
    assert!(marker.exists(), "marker must be written");

    // 2. Stop hook with matching session_id AND a transcript showing
    //    `decompose:decompose` as the most recent Skill call must
    //    block. The new predicate gates on decompose-return detection,
    //    so the transcript_path field is load-bearing.
    let transcript = transcript_with_skill_calls(&root, &["decompose:decompose"]);
    let stdin_input = format!(
        r#"{{"session_id": "{}", "transcript_path": "{}"}}"#,
        session_id,
        transcript.display(),
    );
    let (code, stdout, _stderr) = run_hook_with_home(&root, &stdin_input, &root);
    assert_eq!(code, 0, "Stop hook exits 0 even when blocking");
    let parsed: Value =
        serde_json::from_str(&stdout).expect("Stop hook must emit JSON when blocking");
    assert_eq!(
        parsed["decision"], "block",
        "Stop hook must refuse turn-end while marker present + decompose return: {}",
        stdout
    );
    assert_eq!(
        parsed["reason"].as_str().unwrap_or(""),
        "Stop Refused: Continue, you can do it. Don't give up, you got this! No excuses!",
        "block reason must be the encouraging AC#7 message",
    );

    // 3. Clear marker via the real CLI.
    let (code, stdout) = run_marker_subcommand(
        "clear-utility-in-progress",
        &root,
        "flow:flow-plan",
        session_id,
    );
    assert_eq!(code, 0, "clear must succeed: stdout={}", stdout);
    assert!(!marker.exists(), "marker must be gone after clear");

    // 4. Stop hook with same session_id must NOT block now.
    let (code, stdout, _stderr) = run_hook_with_home(&root, &stdin_input, &root);
    assert_eq!(code, 0);
    // Either empty stdout (no block) or a JSON without decision="block".
    if !stdout.trim().is_empty() {
        let parsed: Value = serde_json::from_str(&stdout).unwrap_or(Value::Null);
        assert_ne!(
            parsed["decision"], "block",
            "Stop hook must NOT block after clear: {}",
            stdout
        );
    }
}

/// End-to-end orphan-marker case: a marker for one session_id must
/// not block a Stop hook spawned for a different session_id. Drives
/// the same CLI surface as the lifecycle test above.
#[test]
fn utility_marker_orphan_from_different_session_subprocess() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    init_main_repo(&root);

    // Write a marker for "other_session" via the CLI.
    let (code, _) = run_marker_subcommand(
        "set-utility-in-progress",
        &root,
        "flow:flow-plan",
        "other_session",
    );
    assert_eq!(code, 0);

    // Spawn the Stop hook with a DIFFERENT session_id — the orphan
    // marker must not block this session's turn-end.
    let stdin_input = r#"{"session_id": "current_session"}"#;
    let (code, stdout, _stderr) = run_hook_with_home(&root, stdin_input, &root);
    assert_eq!(code, 0);
    if !stdout.trim().is_empty() {
        let parsed: Value = serde_json::from_str(&stdout).unwrap_or(Value::Null);
        assert_ne!(
            parsed["decision"], "block",
            "orphan marker (different session_id) must not block: {}",
            stdout
        );
    }
}

// --- check_halt_pending ---
//
// `check_halt_pending` is the Stop-hook predicate that detects user
// pause directives during autonomous flows. When the user has typed
// a prose message since the model's most recent Skill action AND
// the message contains no continue token, the predicate sets
// `_halt_pending=true` and refuses turn-end with the
// conversation-preserving message. The flag persists across
// subsequent Stop events until the user types an explicit continue
// token, which clears both `_halt_pending` and `_stop_instructed`.

/// Build a minimal state JSON for the halt-pending predicate.
fn halt_pending_state(
    current_phase: &str,
    phase_status: &str,
    skill_config: Value,
    halt_pending: Value,
    stop_instructed: Value,
    continue_pending: &str,
) -> Value {
    json!({
        "branch": "test",
        "current_phase": current_phase,
        "phases": { current_phase: { "status": phase_status } },
        "skills": { current_phase: skill_config },
        "_halt_pending": halt_pending,
        "_stop_instructed": stop_instructed,
        "_continue_pending": continue_pending,
    })
}

/// Write a transcript JSONL with an assistant Skill action followed
/// (optionally) by a real user prose message. Used to drive the
/// `most_recent_user_message_since_skill_action` walker.
fn write_halt_transcript(path: &Path, include_skill: bool, user_message: Option<&str>) {
    let mut lines = Vec::new();
    if include_skill {
        lines.push(
            json!({
                "type": "assistant",
                "message": {
                    "content": [{
                        "type": "tool_use",
                        "name": "Skill",
                        "input": {"skill": "flow:flow-code"}
                    }]
                }
            })
            .to_string(),
        );
    }
    if let Some(msg) = user_message {
        lines.push(
            json!({
                "type": "user",
                "message": {"content": msg}
            })
            .to_string(),
        );
    }
    let mut body = lines.join("\n");
    if !body.is_empty() {
        body.push('\n');
    }
    fs::write(path, body).unwrap();
}

/// Read `_halt_pending` from the state file as a bool, defaulting
/// to false when the field is missing or wrong-typed.
fn read_halt_pending(state_path: &Path) -> bool {
    let state: Value =
        serde_json::from_str(&fs::read_to_string(state_path).unwrap()).unwrap_or(Value::Null);
    state
        .get("_halt_pending")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Read `_stop_instructed` from the state file as a bool.
fn read_stop_instructed(state_path: &Path) -> bool {
    let state: Value =
        serde_json::from_str(&fs::read_to_string(state_path).unwrap()).unwrap_or(Value::Null);
    state
        .get("_stop_instructed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Read `_continue_pending` from the state file as a string.
fn read_continue_pending(state_path: &Path) -> String {
    let state: Value =
        serde_json::from_str(&fs::read_to_string(state_path).unwrap()).unwrap_or(Value::Null);
    state
        .get("_continue_pending")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

// --- Precondition matrix ---

#[test]
fn check_halt_pending_sets_when_user_message_after_skill_action_and_phase_in_progress_and_auto() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "halt.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&halt_pending_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(false),
            json!(false),
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_halt_transcript(&transcript, true, Some("please pause"));

    let result = check_halt_pending(&state_path, Some(transcript.to_str().unwrap()), dir.path());

    assert!(result.should_block);
    assert_eq!(result.skill.as_deref(), Some("halt-pending"));
    assert!(read_halt_pending(&state_path));
}

#[test]
fn check_halt_pending_does_not_set_when_phase_not_in_progress() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "halt.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&halt_pending_state(
            "flow-code",
            "complete",
            json!("auto"),
            json!(false),
            json!(false),
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_halt_transcript(&transcript, true, Some("please pause"));

    let result = check_halt_pending(&state_path, Some(transcript.to_str().unwrap()), dir.path());

    assert!(!result.should_block);
    assert!(!read_halt_pending(&state_path));
}

#[test]
fn check_halt_pending_does_not_set_when_skill_not_auto() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "halt.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&halt_pending_state(
            "flow-code",
            "in_progress",
            json!("manual"),
            json!(false),
            json!(false),
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_halt_transcript(&transcript, true, Some("please pause"));

    let result = check_halt_pending(&state_path, Some(transcript.to_str().unwrap()), dir.path());

    assert!(!result.should_block);
    assert!(!read_halt_pending(&state_path));
}

#[test]
fn check_halt_pending_does_not_set_when_no_user_message_after_skill_action() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "halt.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&halt_pending_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(false),
            json!(false),
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    // Skill action with no subsequent user message.
    write_halt_transcript(&transcript, true, None);

    let result = check_halt_pending(&state_path, Some(transcript.to_str().unwrap()), dir.path());

    assert!(!result.should_block);
    assert!(!read_halt_pending(&state_path));
}

// --- Continue-token clearance (one per token) ---

#[test]
fn check_halt_pending_clears_on_continue_token_continue() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "halt.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&halt_pending_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(true),
            json!(true),
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_halt_transcript(&transcript, true, Some("continue"));

    let result = check_halt_pending(&state_path, Some(transcript.to_str().unwrap()), dir.path());

    assert!(!result.should_block);
    assert!(!read_halt_pending(&state_path));
}

#[test]
fn check_halt_pending_clears_on_continue_token_go_ahead() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "halt.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&halt_pending_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(true),
            json!(true),
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_halt_transcript(&transcript, true, Some("go ahead"));

    let result = check_halt_pending(&state_path, Some(transcript.to_str().unwrap()), dir.path());

    assert!(!result.should_block);
    assert!(!read_halt_pending(&state_path));
}

#[test]
fn check_halt_pending_clears_on_continue_token_resume() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "halt.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&halt_pending_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(true),
            json!(true),
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_halt_transcript(&transcript, true, Some("resume"));

    let result = check_halt_pending(&state_path, Some(transcript.to_str().unwrap()), dir.path());

    assert!(!result.should_block);
    assert!(!read_halt_pending(&state_path));
}

#[test]
fn check_halt_pending_clears_on_continue_token_proceed() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "halt.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&halt_pending_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(true),
            json!(true),
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_halt_transcript(&transcript, true, Some("proceed"));

    let result = check_halt_pending(&state_path, Some(transcript.to_str().unwrap()), dir.path());

    assert!(!result.should_block);
    assert!(!read_halt_pending(&state_path));
}

#[test]
fn check_halt_pending_clears_on_continue_token_keep_going() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "halt.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&halt_pending_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(true),
            json!(true),
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_halt_transcript(&transcript, true, Some("keep going"));

    let result = check_halt_pending(&state_path, Some(transcript.to_str().unwrap()), dir.path());

    assert!(!result.should_block);
    assert!(!read_halt_pending(&state_path));
}

// --- Residue clearing ---

#[test]
fn check_halt_pending_clears_stop_instructed_when_clearing_halt_pending() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "halt.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&halt_pending_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(true),
            json!(true),
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_halt_transcript(&transcript, true, Some("continue"));

    let result = check_halt_pending(&state_path, Some(transcript.to_str().unwrap()), dir.path());

    assert!(!result.should_block);
    assert!(!read_halt_pending(&state_path));
    // `_stop_instructed` is the discussion-mode residue. Bug 2 had it
    // carry forward into Complete and authorize invented constraints
    // there. Clearing it alongside `_halt_pending` closes that bug.
    assert!(!read_stop_instructed(&state_path));
}

// --- `_continue_pending` preservation ---

#[test]
fn check_halt_pending_preserves_continue_pending_across_set() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "halt.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&halt_pending_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(false),
            json!(false),
            "flow-code",
        ))
        .unwrap(),
    )
    .unwrap();
    write_halt_transcript(&transcript, true, Some("hold on a sec"));

    let result = check_halt_pending(&state_path, Some(transcript.to_str().unwrap()), dir.path());

    assert!(result.should_block);
    assert!(read_halt_pending(&state_path));
    // Setting `_halt_pending` must not clobber `_continue_pending` —
    // the resume must pick up where it paused once the user types
    // a continue token.
    assert_eq!(read_continue_pending(&state_path), "flow-code");
}

#[test]
fn check_halt_pending_preserves_continue_pending_across_clear() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "halt.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&halt_pending_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(true),
            json!(true),
            "flow-code",
        ))
        .unwrap(),
    )
    .unwrap();
    write_halt_transcript(&transcript, true, Some("continue"));

    let result = check_halt_pending(&state_path, Some(transcript.to_str().unwrap()), dir.path());

    assert!(!result.should_block);
    assert!(!read_halt_pending(&state_path));
    // Clearing `_halt_pending` must not clobber `_continue_pending`
    // — the cascade falls through to the multi-child-skill resume
    // path which reads `_continue_pending`.
    assert_eq!(read_continue_pending(&state_path), "flow-code");
}

// --- Message shape ---

#[test]
fn check_halt_pending_block_message_contains_token_set_and_do_not_advance_guidance() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "halt.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&halt_pending_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(false),
            json!(false),
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_halt_transcript(&transcript, true, Some("hold on"));

    let result = check_halt_pending(&state_path, Some(transcript.to_str().unwrap()), dir.path());

    let context = result.context.expect("block carries context");
    // Every continue token must be named so the model knows the
    // closed grammar that resumes the flow.
    assert!(context.contains("continue"), "names `continue` token");
    assert!(context.contains("resume"), "names `resume` token");
    assert!(context.contains("proceed"), "names `proceed` token");
    assert!(context.contains("go ahead"), "names `go ahead` token");
    assert!(context.contains("keep going"), "names `keep going` token");
    // Do-not-advance guidance prevents the model from misreading
    // the block as "you can't stop, so keep going". Normalize
    // line-wrap whitespace before substring match so the literal
    // message can wrap across newlines for readability without
    // breaking the assertion.
    let collapsed: String = context
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_whitespace() { ' ' } else { c })
        .collect();
    assert!(
        collapsed.contains("do not advance"),
        "carries do-not-advance guidance"
    );
    // Rule citation for traceability.
    assert!(
        context.contains("autonomous-phase-discipline.md"),
        "cites the rule"
    );
}

// --- Skill config shape tolerance ---

#[test]
fn check_halt_pending_simple_and_detailed_skill_config_shapes() {
    let dir = tempfile::tempdir().unwrap();
    // Simple `"auto"` shape.
    let simple_state = dir.path().join("simple.json");
    let simple_transcript = safe_transcript_path(dir.path(), "simple.jsonl");
    fs::write(
        &simple_state,
        serde_json::to_string(&halt_pending_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(false),
            json!(false),
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_halt_transcript(&simple_transcript, true, Some("hold on"));
    let simple = check_halt_pending(
        &simple_state,
        Some(simple_transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(simple.should_block, "Simple `\"auto\"` shape must fire");

    // Detailed `{"continue":"auto"}` shape.
    let detailed_state = dir.path().join("detailed.json");
    let detailed_transcript = safe_transcript_path(dir.path(), "detailed.jsonl");
    fs::write(
        &detailed_state,
        serde_json::to_string(&halt_pending_state(
            "flow-code",
            "in_progress",
            json!({"continue": "auto", "commit": "auto"}),
            json!(false),
            json!(false),
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_halt_transcript(&detailed_transcript, true, Some("hold on"));
    let detailed = check_halt_pending(
        &detailed_state,
        Some(detailed_transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(
        detailed.should_block,
        "Detailed `{{\"continue\":\"auto\"}}` shape must fire"
    );
}

// --- Persistence + short-circuit ---

#[test]
fn check_halt_pending_persists_across_multiple_stops_without_continue_token() {
    // After the user's pause message lands and `_halt_pending=true`
    // is set, subsequent Stop events with NO new user turn must
    // continue to block. Stage 1: user pauses → block, `_halt_pending=true`.
    // Stage 2: model takes another Skill action; no new user turn;
    // the walker returns None for "user message since latest Skill",
    // but `_halt_pending` is still true → persistent block.
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "halt.jsonl");

    // Stage 1: user pauses.
    fs::write(
        &state_path,
        serde_json::to_string(&halt_pending_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(false),
            json!(false),
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_halt_transcript(&transcript, true, Some("hold on"));
    let result_1 = check_halt_pending(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    assert!(result_1.should_block);
    assert!(read_halt_pending(&state_path));

    // Stage 2: model fires another Skill (no new user turn after).
    write_halt_transcript(&transcript, true, None);
    let result_2 = check_halt_pending(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    assert!(
        result_2.should_block,
        "halt persists across stops without a new continue token"
    );
    assert!(read_halt_pending(&state_path));
}

#[test]
fn check_halt_pending_short_circuits_cascade_when_halt_pending_true_no_continue_token() {
    // When the predicate fires, its block is final — callers must
    // not run downstream predicates (which would emit the generic
    // stop-intent message that Bug 1 misreads).
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "halt.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&halt_pending_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(false),
            json!(false),
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_halt_transcript(&transcript, true, Some("wait"));
    let result = check_halt_pending(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    assert!(result.should_block);
    assert_eq!(result.skill.as_deref(), Some("halt-pending"));
    // The context is the verbatim halt-pause reason — not the
    // generic autonomous-stop message.
    assert_eq!(result.context.as_deref(), Some(HALT_PAUSE_BLOCK_REASON));
}

#[test]
fn check_halt_pending_falls_through_when_continue_token_clears_halt_pending() {
    // When the continue token clears `_halt_pending`, the predicate
    // returns non-blocking so the cascade falls through to
    // `check_first_stop` / `check_continue` for normal continuation.
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "halt.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&halt_pending_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(true),
            json!(true),
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_halt_transcript(&transcript, true, Some("continue"));
    let result = check_halt_pending(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    assert!(!result.should_block);
    assert!(result.skill.is_none());
    assert!(result.context.is_none());
}

// --- Tolerance ---

#[test]
fn check_halt_pending_no_state_file_returns_non_blocking() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("nonexistent.json");
    let transcript = safe_transcript_path(dir.path(), "halt.jsonl");
    write_halt_transcript(&transcript, true, Some("hold on"));

    let result = check_halt_pending(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    assert!(!result.should_block);
}

#[test]
fn check_halt_pending_array_root_state_file_does_not_crash() {
    // State file root is a JSON array — the mutate_state closure's
    // object-or-null guard short-circuits. Predicate returns
    // non-blocking; no panic from IndexMut on a non-object.
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "halt.jsonl");
    fs::write(&state_path, r#"["not", "an", "object"]"#).unwrap();
    write_halt_transcript(&transcript, true, Some("please pause"));

    let result = check_halt_pending(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    assert!(!result.should_block);
}

#[test]
fn check_halt_pending_clears_stale_halt_when_phase_no_longer_in_progress() {
    // A halt set in a prior phase that has since completed must be
    // cleared so it does not bleed forward. The current phase is no
    // longer in_progress (complete) but `_halt_pending=true` is
    // stale residue — the predicate clears it and returns
    // non-blocking. Closes the "halt bleeds into Complete" leg of
    // Bug 2.
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "halt.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&halt_pending_state(
            "flow-code",
            "complete",
            json!("auto"),
            json!(true),
            json!(false),
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_halt_transcript(&transcript, true, None);

    let result = check_halt_pending(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    assert!(!result.should_block);
    assert!(!read_halt_pending(&state_path));
}

#[test]
fn check_halt_pending_wrong_type_state_field_defaults_to_false() {
    // `_halt_pending` stored as a string instead of a bool must
    // be treated as false. The transcript determines behavior; with
    // no user message, the wrong-type field cannot resurrect a
    // persistent block.
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = safe_transcript_path(dir.path(), "halt.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&halt_pending_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!("not-a-bool"),
            json!(false),
            "",
        ))
        .unwrap(),
    )
    .unwrap();
    write_halt_transcript(&transcript, true, None);

    let result = check_halt_pending(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    assert!(!result.should_block);
}

// --- Ordering (subprocess test) ---
//
// Drives the full `run()` cascade to verify `check_halt_pending`
// runs BEFORE `check_first_stop` in the production order. A state
// with no `_stop_instructed` would normally trip `check_first_stop`
// into discussion mode; with a halt-pause user message present,
// `check_halt_pending` must fire FIRST and return the halt-pause
// block message, not the discussion-mode message.

#[test]
fn check_halt_pending_runs_before_check_first_stop_in_run() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    Command::new("git")
        .args(["init", "--initial-branch", "main"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "t@t"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "t"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["checkout", "-b", "halttest"])
        .current_dir(&root)
        .output()
        .unwrap();

    let state_dir = root.join(".flow-states").join("halttest");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("state.json");
    let session_id = "halt-session";
    let transcript = safe_transcript_path(&root, "halt-ordering.jsonl");
    let mut state = halt_pending_state(
        "flow-code",
        "in_progress",
        json!("auto"),
        json!(false),
        json!(false),
        "",
    );
    // The subprocess `run()` reads `session_id` and `transcript_path`
    // from state via capture_session_id flow; provide the path so
    // the predicate sees the transcript.
    state["session_id"] = json!(session_id);
    state["transcript_path"] = json!(transcript.to_str().unwrap());
    fs::write(&state_path, serde_json::to_string(&state).unwrap()).unwrap();
    write_halt_transcript(&transcript, true, Some("hold on, let me look"));

    let stdin_input = format!(
        "{{\"session_id\": \"{}\", \"transcript_path\": \"{}\"}}",
        session_id,
        transcript.to_str().unwrap()
    );

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd.args(["hook", "stop-continue"])
        .current_dir(&root)
        .env("HOME", &root)
        .env("GH_TOKEN", "invalid")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn flow-rs hook");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(stdin_input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().expect("wait_with_output");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|_| panic!("hook stdout must be JSON: {}", stdout));
    assert_eq!(parsed["decision"], "block");
    let reason = parsed["reason"].as_str().unwrap_or("");
    // Halt-pause block message wins over discussion-mode message.
    // Discriminator: the halt-pause message names the continue
    // token grammar and cites the autonomous-phase-discipline rule;
    // the discussion-mode message starts with "The user interrupted
    // the session." The halt-pause message must appear and the
    // discussion-mode opening must not.
    assert!(
        reason.contains("user halt is in progress"),
        "halt-pause reason must dominate first-stop discussion reason: {}",
        reason
    );
    assert!(
        !reason.contains("The user interrupted the session"),
        "discussion-mode message must not appear: {}",
        reason
    );
}

// End-to-end regression that closes both bugs from issue #1504 by
// driving the real binary through a halt-pause cycle and then a
// phase advance into Complete. The test exercises three observable
// invariants: (1) halt-pause block fires with the
// conversation-preserving message when the user types a non-
// continue-token prose message, (2) `_halt_pending` and
// `_stop_instructed` both clear when the user types a continue
// token, and (3) the cleared residue does not bleed forward into
// Complete — the autonomous-stop-refused block message appears
// instead of the halt-pause message.
#[test]
fn subprocess_halt_pause_full_flow_blocks_premature_resume_and_clears_on_continue() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    Command::new("git")
        .args(["init", "--initial-branch", "main"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "t@t"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "t"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["checkout", "-b", "halttest"])
        .current_dir(&root)
        .output()
        .unwrap();

    let state_dir = root.join(".flow-states").join("halttest");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("state.json");
    let transcript = safe_transcript_path(&root, "halt-e2e.jsonl");
    let session_id = "halt-e2e-session";

    // --- Stage 1: build initial state with `_continue_pending`
    // set to flow-code (typical mid-flow shape). Transcript begins
    // with an assistant Skill action for flow:flow-code.
    let mut state = halt_pending_state(
        "flow-code",
        "in_progress",
        json!({"continue": "auto"}),
        json!(false),
        json!(false),
        "flow-code",
    );
    state["session_id"] = json!(session_id);
    state["transcript_path"] = json!(transcript.to_str().unwrap());
    fs::write(&state_path, serde_json::to_string(&state).unwrap()).unwrap();
    write_halt_transcript(&transcript, true, Some("wait, hold up"));

    let stdin_input = format!(
        "{{\"session_id\": \"{}\", \"transcript_path\": \"{}\"}}",
        session_id,
        transcript.to_str().unwrap()
    );

    let spawn_hook = |root: &Path, stdin_input: &str| -> (i32, String) {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
        cmd.env_remove("FLOW_CI_RUNNING");
        cmd.args(["hook", "stop-continue"])
            .current_dir(root)
            .env("HOME", root)
            .env("GH_TOKEN", "invalid")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().expect("spawn flow-rs hook");
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(stdin_input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().expect("wait_with_output");
        let code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        (code, stdout)
    };

    // --- Stage 2-3: First Stop event. Halt-pause fires with the
    // conversation-preserving message.
    let (_code, stdout) = spawn_hook(&root, &stdin_input);
    let parsed: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|_| panic!("stage-2 hook stdout must be JSON: {}", stdout));
    assert_eq!(
        parsed["decision"], "block",
        "stage 2: halt-pause must block on user pause directive"
    );
    let reason = parsed["reason"].as_str().unwrap_or("");
    assert!(
        reason.contains("user halt is in progress"),
        "stage 2: halt-pause reason must dominate: {}",
        reason
    );

    // --- Stage 4: state reflects the halt. `_continue_pending` is
    // preserved across the halt set so the resume picks up the right
    // child skill once the user clears it.
    assert!(
        read_halt_pending(&state_path),
        "stage 4: _halt_pending must be set after halt-pause block"
    );
    assert_eq!(
        read_continue_pending(&state_path),
        "flow-code",
        "stage 4: _continue_pending must be preserved across halt set"
    );

    // --- Stage 5: user types a continue token. Rewrite the
    // transcript to include the new message AFTER the previous
    // ones; the walker resolves the LAST user message after the
    // most recent Skill action.
    let jsonl = format!(
        "{}\n{}\n{}\n",
        json!({
            "type": "assistant",
            "message": {
                "content": [{
                    "type": "tool_use",
                    "name": "Skill",
                    "input": {"skill": "flow:flow-code"}
                }]
            }
        }),
        json!({"type": "user", "message": {"content": "wait, hold up"}}),
        json!({"type": "user", "message": {"content": "ok continue"}}),
    );
    fs::write(&transcript, jsonl).unwrap();

    // --- Stage 6: second hook spawn. `check_halt_pending` clears
    // both `_halt_pending` and `_stop_instructed`. The cascade
    // then fires downstream predicates; the test does not assert
    // on the cascade's specific output (this varies with
    // `_continue_pending` and `_stop_instructed` state), only that
    // the halt-pause leg is no longer active.
    let (_code, stdout) = spawn_hook(&root, &stdin_input);
    if !stdout.trim().is_empty() {
        let parsed: Value = serde_json::from_str(&stdout).unwrap_or(Value::Null);
        let reason = parsed["reason"].as_str().unwrap_or("");
        assert!(
            !reason.contains("user halt is in progress"),
            "stage 6: halt-pause message must NOT appear after continue token: {}",
            reason
        );
    }
    assert!(
        !read_halt_pending(&state_path),
        "stage 6: _halt_pending must clear when continue token fires"
    );

    // --- Stage 7-8: Bug-2 simulation. Advance state to
    // flow-complete with `_halt_pending=false` and
    // `_stop_instructed=false` (clean residue from stage 6).
    // Append a fresh Skill action for flow-complete so the walker
    // sees no user message after the latest Skill. The Stop event
    // must NOT produce the conversation-preserving halt-pause
    // message — proving halt residue does not carry into Complete.
    // (Whichever downstream predicate fires — discussion-mode or
    // autonomous-stop-refused, depending on `_stop_instructed`
    // state at the time — the load-bearing invariant is that the
    // halt-pause leg is inactive in the new phase.)
    let mut state = halt_pending_state(
        "flow-complete",
        "in_progress",
        json!("auto"),
        json!(false),
        json!(false),
        "",
    );
    state["session_id"] = json!(session_id);
    state["transcript_path"] = json!(transcript.to_str().unwrap());
    fs::write(&state_path, serde_json::to_string(&state).unwrap()).unwrap();
    let jsonl = format!(
        "{}\n{}\n{}\n{}\n",
        json!({
            "type": "assistant",
            "message": {
                "content": [{
                    "type": "tool_use",
                    "name": "Skill",
                    "input": {"skill": "flow:flow-code"}
                }]
            }
        }),
        json!({"type": "user", "message": {"content": "wait, hold up"}}),
        json!({"type": "user", "message": {"content": "ok continue"}}),
        json!({
            "type": "assistant",
            "message": {
                "content": [{
                    "type": "tool_use",
                    "name": "Skill",
                    "input": {"skill": "flow:flow-complete"}
                }]
            }
        }),
    );
    fs::write(&transcript, jsonl).unwrap();

    let (_code, stdout) = spawn_hook(&root, &stdin_input);
    let parsed: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|_| panic!("stage-8 hook stdout must be JSON: {}", stdout));
    assert_eq!(
        parsed["decision"], "block",
        "stage 8: Complete-phase must still block on voluntary stop"
    );
    let reason = parsed["reason"].as_str().unwrap_or("");
    // Halt-pause message must NOT appear (no halt residue carried).
    // This is the Bug-2 fix verification: the halt-pause leg is
    // inactive in the new phase because `check_halt_pending`
    // cleared `_stop_instructed` alongside `_halt_pending` in
    // stage 6, so no residue authorizes the conversation-
    // preserving message here.
    assert!(
        !reason.contains("user halt is in progress"),
        "stage 8: halt-pause message must NOT appear in Complete: {}",
        reason
    );
    assert!(
        !read_halt_pending(&state_path),
        "stage 8: _halt_pending must remain false in Complete"
    );
}
