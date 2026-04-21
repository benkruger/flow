//! Integration tests for `src/hooks/stop_continue.rs`. Drives the public
//! surface (`capture_session_id`, `check_continue`, `check_qa_pending`,
//! `set_blocked_idle`, `set_tab_color`, `check_discussion_mode`,
//! `check_first_stop`, `format_block_output`,
//! `format_conditional_continue_reason`, `DISCUSSION_BLOCK_REASON`) and
//! covers `run()` via subprocess tests.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use flow_rs::commands::clear_blocked::clear_blocked;
use flow_rs::hooks::stop_continue::{
    capture_session_id, check_continue, check_discussion_mode, check_first_stop, check_qa_pending,
    format_block_output, format_conditional_continue_reason, set_blocked_idle, set_tab_color,
    DISCUSSION_BLOCK_REASON,
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

// --- check_qa_pending ---

#[test]
fn test_check_qa_pending_reads_breadcrumb() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let qa_path = state_dir.join("qa-pending.json");
    fs::write(&qa_path, r#"{"_continue_context": "finish QA tests"}"#).unwrap();

    let (should_block, context) = check_qa_pending(dir.path());
    assert!(should_block);
    assert_eq!(context.unwrap(), "finish QA tests");
}

#[test]
fn test_check_qa_pending_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let (should_block, context) = check_qa_pending(dir.path());
    assert!(!should_block);
    assert!(context.is_none());
}

#[test]
fn test_check_qa_pending_empty_context() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let qa_path = state_dir.join("qa-pending.json");
    fs::write(&qa_path, r#"{"_continue_context": ""}"#).unwrap();

    let (should_block, context) = check_qa_pending(dir.path());
    assert!(!should_block);
    assert!(context.is_none());
}

#[test]
fn test_check_qa_pending_missing_context_key() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let qa_path = state_dir.join("qa-pending.json");
    fs::write(&qa_path, r#"{"other": "value"}"#).unwrap();

    let (should_block, _) = check_qa_pending(dir.path());
    assert!(!should_block);
}

#[test]
fn test_check_qa_pending_corrupt_json() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let qa_path = state_dir.join("qa-pending.json");
    fs::write(&qa_path, "{bad json").unwrap();

    let (should_block, _) = check_qa_pending(dir.path());
    assert!(!should_block);
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

#[test]
fn test_check_qa_pending_unreadable_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let qa_path = state_dir.join("qa-pending.json");
    fs::write(&qa_path, r#"{"_continue_context": "x"}"#).unwrap();
    fs::set_permissions(&qa_path, fs::Permissions::from_mode(0o000)).unwrap();
    let _guard = PermissionGuard {
        path: qa_path.clone(),
        restore_mode: 0o644,
    };
    let (should_block, context) = check_qa_pending(dir.path());
    assert!(!should_block);
    assert!(context.is_none());
}

// --- set_tab_color ---

#[test]
fn test_set_tab_color_with_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("test.json");
    fs::write(&state_path, r#"{"repo": "owner/repo"}"#).unwrap();

    set_tab_color(dir.path(), "test", &state_path);
}

#[test]
fn test_set_tab_color_without_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("test.json");

    set_tab_color(dir.path(), "test", &state_path);
}

#[test]
fn test_set_tab_color_corrupt_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("test.json");
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
    fs::create_dir_all(&state_dir).unwrap();
    // Other feature's state file with a repo field.
    fs::write(
        state_dir.join("other-branch.json"),
        r#"{"repo": "owner/repo", "branch": "other-branch"}"#,
    )
    .unwrap();

    // Requested state path does NOT exist — triggers the else arm
    // with find_state_files fallback, which locates other-branch.json.
    let state_path = state_dir.join("test.json");
    set_tab_color(dir.path(), "test", &state_path);
}

#[test]
fn test_set_tab_color_unreadable_state_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("test.json");
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
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("branch-x.json");
    fs::write(&state_path, "{bad json").unwrap();
    // Pre-create `branch-x.log` as a directory — OpenOptions::open
    // cannot open a directory as a writable file.
    fs::create_dir(state_dir.join("branch-x.log")).unwrap();

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
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("test-branch.json");
    let initial = json!({
        "_continue_pending": "commit",
        "_continue_context": "Next step"
    });
    fs::write(&state_path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_continue(&json!({}), &state_path);
    assert!(result.should_block);

    let log_path = state_dir.join("test-branch.log");
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
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("test-branch.json");
    let initial = json!({
        "_continue_pending": "commit",
        "_continue_context": "stale",
        "session_id": "old-session"
    });
    fs::write(&state_path, serde_json::to_string(&initial).unwrap()).unwrap();

    let result = check_continue(&json!({"session_id": "new-session"}), &state_path);
    assert!(!result.should_block);

    let log_path = state_dir.join("test-branch.log");
    assert!(log_path.exists());
    let log_content = fs::read_to_string(&log_path).unwrap();
    assert!(log_content.contains("session mismatch"));
    assert!(log_content.contains("cleared pending=commit"));
}

#[test]
fn test_check_continue_no_pending_does_not_write_log_file() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("test-branch.json");
    fs::write(&state_path, r#"{"branch": "test"}"#).unwrap();

    check_continue(&json!({}), &state_path);

    let log_path = state_dir.join("test-branch.log");
    assert!(!log_path.exists());
}

// --- capture_session_id error logging ---

#[test]
fn test_capture_session_id_corrupt_state_logs_error() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("test-branch.json");
    fs::write(&state_path, "{bad json").unwrap();

    capture_session_id(&json!({"session_id": "abc123"}), &state_path);

    let log_path = state_dir.join("test-branch.log");
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

fn run_hook(cwd: &Path, stdin_input: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["hook", "stop-continue"])
        .current_dir(cwd)
        .env_remove("FLOW_CI_RUNNING")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn flow-rs");
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
    fs::create_dir_all(&state_dir).unwrap();
    let state = json!({
        "branch": "main",
        "_continue_pending": "commit",
        "_continue_context": "Next step"
    });
    fs::write(
        state_dir.join("main.json"),
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

// State file already instructed + no pending + QA breadcrumb → run()
// falls through to check_qa_pending and blocks with flow-complete.
#[test]
fn run_subprocess_qa_pending_fallback_blocks() {
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
    fs::create_dir_all(&state_dir).unwrap();
    // State that allows check_first_stop to fall through (already
    // instructed, no pending) and check_continue to also fall through
    // (no pending).
    let state = json!({
        "branch": "main",
        "_stop_instructed": true,
    });
    fs::write(
        state_dir.join("main.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();
    // QA breadcrumb with context that should trigger the fallback block.
    fs::write(
        state_dir.join("qa-pending.json"),
        r#"{"_continue_context": "finish QA run"}"#,
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
    assert!(json["reason"].as_str().unwrap().contains("finish QA run"));
}
