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
    check_continue, check_discussion_mode, check_first_stop, check_prose_pause_at_task_entry,
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

/// Covers the Err arm of `set_tab_color`'s `if let Err(e) = result`
/// (the `log_diag(... "set_tab_color error: {e}" ...)` branch).
/// That arm only fires when `write_tab_sequences`'s
/// `fs::write("/dev/tty", ...)` call returns Err. Whether the write
/// succeeds depends on the test process's controlling terminal:
/// interactive shells give the process a tty → write succeeds → Err
/// arm uncovered; non-tty contexts (Claude Code, most CI agents) →
/// write fails → Err arm covered. That environmental flakiness was
/// producing non-deterministic coverage across `bin/flow ci` runs.
///
/// This test spawns a subprocess of the test binary with `setsid`
/// via `pre_exec` to detach it from the controlling terminal before
/// `execvp`, then runs the set_tab_color call body inside. The
/// subprocess's `fs::write("/dev/tty", ...)` returns `ENXIO` (no
/// controlling terminal), forcing the Err arm every run regardless
/// of how the host was launched.
///
/// The subprocess dispatch is keyed on an env var
/// `FLOW_TEST_SET_TAB_COLOR_ERR_WORKER=1`. When set, the test binary
/// body runs the worker code instead of the nextest harness. Other
/// tests in the binary ignore this path because the env var is
/// unset in their process.
#[cfg(unix)]
#[test]
fn test_set_tab_color_err_arm_via_setsid_subprocess() {
    use std::os::unix::process::CommandExt;

    // Worker branch: if this env var is set, the test binary was
    // spawned by the harness below. Drive set_tab_color with a
    // state path that has "repo" so write_tab_sequences reaches
    // `fs::write("/dev/tty", ...)`, which fails after setsid.
    if std::env::var("FLOW_TEST_SET_TAB_COLOR_ERR_WORKER").is_ok() {
        let dir = std::env::var("FLOW_TEST_WORKER_DIR").expect("worker dir");
        let root = std::path::PathBuf::from(&dir);
        let state_path = root.join(".flow-states").join("test").join("state.json");
        // set_tab_color swallows errors via log_diag — no assertion
        // needed, the profdata's count on the Err arm is the signal.
        set_tab_color(&root, "test", &state_path);
        std::process::exit(0);
    }

    // Harness branch: set up fixture, spawn worker in a new session.
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(state_dir.join("test")).unwrap();
    fs::write(
        state_dir.join("test").join("state.json"),
        r#"{"repo": "owner/repo"}"#,
    )
    .unwrap();

    let test_exe = std::env::current_exe().expect("current_exe");
    let mut cmd = Command::new(&test_exe);
    cmd.arg("--exact")
        .arg("test_set_tab_color_err_arm_via_setsid_subprocess")
        .arg("--nocapture")
        .env("FLOW_TEST_SET_TAB_COLOR_ERR_WORKER", "1")
        .env("FLOW_TEST_WORKER_DIR", dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let status = cmd.status().expect("spawn worker");
    assert!(
        status.success(),
        "worker process did not exit cleanly: {:?}",
        status
    );
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
        "current_phase": "flow-plan",
        "phases": {"flow-plan": {"status": "in_progress"}},
        "skills": {"flow-plan": "auto"}
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
            "flow-plan",
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
