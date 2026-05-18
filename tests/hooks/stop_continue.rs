//! Integration tests for `src/hooks/stop_continue.rs`. Drives the public
//! surface (`capture_session_id`, `check_continue`, `set_blocked_idle`,
//! `set_tab_color`, `check_autonomous_stop`,
//! `check_in_progress_utility_skill`, `format_block_output`) and covers
//! `run()` via subprocess tests.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use flow_rs::hooks::stop_continue::{
    capture_session_id, check_autonomous_stop, check_continue, check_in_progress_utility_skill,
    format_block_output, set_blocked_idle, set_tab_color, RULE_1_STOP_REFUSED_MESSAGE,
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
fn run_predicate_order_is_utility_then_continue_then_autonomous_stop() {
    // Ordering invariant: run() composes three predicates in the
    // order utility_skill → check_continue → check_autonomous_stop.
    // The utility-skill predicate fires FIRST so its verbatim
    // encouraging block wins for the decompose-return shape.
    // check_continue handles multi-child-skill chains. The unified
    // check_autonomous_stop runs last and emits Rule 1 (encouraging)
    // or Rule 2 (halt pending) per the autonomous-mode contract.
    let src_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("hooks")
        .join("stop_continue.rs");
    let content = fs::read_to_string(&src_path).expect("read stop_continue.rs");
    let tail = content
        .split_once("pub fn run() {")
        .map(|(_, t)| t)
        .expect("stop_continue.rs must define `pub fn run()`");
    let body = tail.split_once("\n}\n").map(|(b, _)| b).unwrap_or(tail);
    let idx_utility = body
        .find("check_in_progress_utility_skill(")
        .expect("run() must call check_in_progress_utility_skill");
    let idx_continue = body
        .find("check_continue(")
        .expect("run() must call check_continue");
    let idx_auto = body
        .find("check_autonomous_stop(")
        .expect("run() must call check_autonomous_stop");
    assert!(
        idx_utility < idx_continue,
        "check_in_progress_utility_skill must run BEFORE check_continue"
    );
    assert!(
        idx_continue < idx_auto,
        "check_continue must run BEFORE check_autonomous_stop"
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

#[test]
fn subprocess_check_in_progress_utility_skill_refuses_stop_after_hook_feedback_turn() {
    // Regression guard for the autonomous-flow-halt class of bugs
    // (#1507 transcript shape): a Stop-hook refusal during a multi-
    // step utility skill injects a `type:"user"` turn carrying string
    // content AND `isMeta:true`. The
    // `most_recent_skill_since_user` walker must treat that injected
    // turn as synthetic — skipping it and continuing backward to the
    // real user turn. When the walker correctly skips, it returns
    // `Some("decompose:decompose")` and
    // `check_in_progress_utility_skill` refuses the Stop event with
    // the verbatim encouraging message. A future regression that
    // removes the `is_real_user_turn` filter — or weakens the
    // `isMeta` discriminator to miss non-bool truthy values — would
    // re-introduce the failure mode this test guards against.
    //
    // The fixture mirrors the canonical failing transcript in
    // topological order:
    //
    //   1. real user turn: "sounds good. proceed" (NO isMeta)
    //   2. assistant turn fires Skill(decompose:decompose)
    //   3. synthetic user turn: array content (tool_result)
    //   4. assistant turn: text-only synthesis prose
    //   5. hook-injected user turn: string content + isMeta:true
    //   6. assistant turn: text-only end-of-turn
    //
    // Combined with a `flow:flow-plan` utility marker for the same
    // session_id, the predicate must refuse the Stop with the
    // verbatim encouraging message. Without the fix the walker
    // stops at line 5 and the predicate fails open.
    //
    // Test environment per `.claude/rules/subprocess-test-hygiene.md`:
    // `run_hook_with_home` already neutralizes `FLOW_CI_RUNNING` and
    // sets `HOME` to the fixture's tempdir; the Stop hook is a
    // pure-Rust path that reads only filesystem state, so no
    // additional credential neutralizers are required.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    init_main_repo(&root);
    let session_id = "halt12345";

    // Write the utility marker for flow:flow-plan with the
    // matching session_id. flow:flow-plan is in
    // MULTI_STEP_UTILITY_SKILLS so the Stop hook predicate honors
    // the marker.
    let marker_dir = root.join(".claude").join("flow");
    fs::create_dir_all(&marker_dir).unwrap();
    let payload = json!({
        "skill": "flow:flow-plan",
        "session_id": session_id,
        "started_at": "2026-05-12T17:00:00-07:00",
    });
    fs::write(
        marker_dir.join(format!("utility-in-progress-{}.json", session_id)),
        serde_json::to_string(&payload).unwrap(),
    )
    .unwrap();

    // Build the 6-line failing transcript verbatim. Lines 3 and 5
    // are the synthetic user turns that previously masked the
    // assistant Skill call on line 2.
    let jsonl = concat!(
        // 1. Real user prose — no isMeta.
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"sounds good. proceed\"}}\n",
        // 2. Assistant fires Skill(decompose:decompose).
        "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[",
        "{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"decompose:decompose\"}}",
        "]}}\n",
        // 3. Synthetic tool_result-wrapped user turn (array content).
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[",
        "{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_d1\",\"content\":\"decompose output\"}",
        "]}}\n",
        // 4. Assistant text-only synthesis.
        "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[",
        "{\"type\":\"text\",\"text\":\"synthesis prose\"}",
        "]}}\n",
        // 5. Hook-injected user turn: string content + isMeta:true.
        "{\"type\":\"user\",\"isMeta\":true,\"message\":{\"role\":\"user\",\"content\":\"Stop hook feedback:\\nStop Refused: Continue, you can do it.\"}}\n",
        // 6. Assistant text-only end-of-turn.
        "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[",
        "{\"type\":\"text\",\"text\":\"acknowledging\"}",
        "]}}\n",
    );
    let transcript = crate::common::transcript_fixture(&root, "p", jsonl);

    let stdin_input = format!(
        r#"{{"session_id": "{}", "transcript_path": "{}"}}"#,
        session_id,
        transcript.display(),
    );
    let (code, stdout, stderr) = run_hook_with_home(&root, &stdin_input, &root);
    assert_eq!(
        code, 0,
        "Stop hook exits 0 even when blocking: stderr={}",
        stderr,
    );
    let parsed: Value = serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "Stop hook must emit JSON when blocking — parse failed ({}): stdout={}",
            e, stdout,
        )
    });
    assert_eq!(
        parsed["decision"], "block",
        "must refuse turn-end after hook-feedback synthetic turn: stdout={}",
        stdout,
    );
    assert_eq!(
        parsed["reason"].as_str().unwrap_or(""),
        "Stop Refused: Continue, you can do it. Don't give up, you got this! No excuses!",
        "block reason must be the verbatim encouraging message",
    );
}

// --- check_autonomous_stop ---
//
// `check_autonomous_stop` is the unified autonomous-mode Stop gate.
// Three rules govern Stop events during in-progress autonomous phases:
//
// 1. **Conversation pass-through.** A real user message since the
//    most recent Skill action sets `_halt_pending=true` and ALLOWS
//    the Stop so the model can answer the user.
// 2. **Rule 2 (halt pending, no new user message).** Refuses with
//    `RULE_2_HALT_PENDING_MESSAGE` naming `/flow:flow-continue` and
//    `/flow:flow-abort` as the only exits.
// 3. **Rule 1 (no halt, no new user message, autonomous phase).**
//    Refuses with `RULE_1_STOP_REFUSED_MESSAGE` — the autonomous
//    flow must keep going.

/// Build a minimal state JSON for the autonomous-stop predicate.
fn autonomous_stop_state(
    current_phase: &str,
    phase_status: &str,
    skill_config: Value,
    halt_pending: Value,
) -> Value {
    json!({
        "branch": "test",
        "current_phase": current_phase,
        "phases": { current_phase: { "status": phase_status } },
        "skills": { current_phase: skill_config },
        "_halt_pending": halt_pending,
    })
}

/// Build a transcript path under `<home>/.claude/projects/` so the
/// `is_safe_transcript_path` validator accepts it.
fn auto_stop_transcript(home: &Path, name: &str) -> PathBuf {
    let projects = home.join(".claude").join("projects");
    fs::create_dir_all(&projects).unwrap();
    projects.join(name)
}

/// Write a transcript JSONL with an assistant Skill action followed
/// (optionally) by a real user prose message. Drives the
/// `most_recent_user_message_since_skill_action` walker that
/// `check_autonomous_stop` consults.
fn write_auto_stop_transcript(path: &Path, include_skill: bool, user_message: Option<&str>) {
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

fn read_halt_pending(state_path: &Path) -> bool {
    let state: Value =
        serde_json::from_str(&fs::read_to_string(state_path).unwrap()).unwrap_or(Value::Null);
    state
        .get("_halt_pending")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

#[test]
fn check_autonomous_stop_not_auto_allows_stop() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = auto_stop_transcript(dir.path(), "t.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&autonomous_stop_state(
            "flow-code",
            "in_progress",
            json!("manual"),
            json!(false),
        ))
        .unwrap(),
    )
    .unwrap();
    write_auto_stop_transcript(&transcript, true, None);
    let result = check_autonomous_stop(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    assert!(!result.should_block);
}

#[test]
fn check_autonomous_stop_no_state_file_allows_stop() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("missing.json");
    let transcript = auto_stop_transcript(dir.path(), "t.jsonl");
    write_auto_stop_transcript(&transcript, true, None);
    let result = check_autonomous_stop(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    assert!(!result.should_block);
}

#[test]
fn check_autonomous_stop_missing_current_phase_no_block() {
    // State file exists as an empty object — `current_phase` is
    // absent so the `unwrap_or("")` produces an empty string and
    // the `current_phase.is_empty()` guard early-returns. The
    // predicate falls through to allow-stop.
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    fs::write(&state_path, "{}").unwrap();
    let result = check_autonomous_stop(&state_path, None, dir.path());
    assert!(!result.should_block);
    assert!(!read_halt_pending(&state_path));
}

#[test]
fn check_autonomous_stop_skills_entry_missing_treats_as_not_auto() {
    // State has `current_phase` and `phases.<phase>.status` set
    // but no `skills.<phase>` entry — the `None => false,` arm of
    // the is_auto match fires. is_auto=false makes the
    // phase-not-auto branch clear stale halt and allow stop.
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let state = json!({
        "current_phase": "flow-code",
        "phases": {"flow-code": {"status": "in_progress"}}
    });
    fs::write(&state_path, serde_json::to_string(&state).unwrap()).unwrap();
    let result = check_autonomous_stop(&state_path, None, dir.path());
    assert!(!result.should_block);
    assert!(!read_halt_pending(&state_path));
}

#[test]
fn check_autonomous_stop_wrong_root_type_no_block() {
    // State file root is a JSON array rather than an object.
    // `mutate_state`'s closure must short-circuit via the
    // `!(state.is_object() || state.is_null())` guard so
    // downstream `IndexMut` operations cannot panic per
    // `.claude/rules/rust-patterns.md` "State Mutation Object
    // Guards". The predicate falls through to allow-stop.
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    fs::write(&state_path, "[]").unwrap();
    let result = check_autonomous_stop(&state_path, None, dir.path());
    assert!(!result.should_block);
    assert!(!read_halt_pending(&state_path));
}

#[test]
fn check_autonomous_stop_user_message_sets_halt_and_allows_stop() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = auto_stop_transcript(dir.path(), "t.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&autonomous_stop_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(false),
        ))
        .unwrap(),
    )
    .unwrap();
    write_auto_stop_transcript(&transcript, true, Some("wait, what about X?"));
    let result = check_autonomous_stop(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    assert!(!result.should_block, "user-message path must ALLOW stop");
    assert!(
        read_halt_pending(&state_path),
        "user-message path must SET _halt_pending"
    );
}

#[test]
fn check_autonomous_stop_halt_set_no_user_message_refuses_with_rule2_message() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = auto_stop_transcript(dir.path(), "t.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&autonomous_stop_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(true),
        ))
        .unwrap(),
    )
    .unwrap();
    write_auto_stop_transcript(&transcript, true, None);
    let result = check_autonomous_stop(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    assert!(result.should_block);
    let context = result.context.expect("rule 2 message");
    assert!(
        context.contains("/flow:flow-continue"),
        "rule 2 names /flow:flow-continue: {}",
        context
    );
    assert!(
        context.contains("/flow:flow-abort"),
        "rule 2 names /flow:flow-abort: {}",
        context
    );
}

#[test]
fn check_autonomous_stop_no_halt_no_user_message_refuses_with_rule1_encouraging_message() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = auto_stop_transcript(dir.path(), "t.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&autonomous_stop_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(false),
        ))
        .unwrap(),
    )
    .unwrap();
    write_auto_stop_transcript(&transcript, true, None);
    let result = check_autonomous_stop(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    assert!(result.should_block);
    let context = result.context.expect("rule 1 message");
    assert_eq!(
        context, "Stop Refused: Continue, you can do it. Don't give up, you got this! No excuses!",
        "rule 1 must be the verbatim encouraging message"
    );
}

#[test]
fn check_autonomous_stop_rule_1_fires_after_flow_continue() {
    // End-to-end regression at the consumer layer:
    // assistant Skill, then prose pause, then user-typed
    // `/flow:flow-continue`. The walker's watermark must clear
    // the prose candidate so `check_autonomous_stop` sees no
    // new user message and fires Rule 1 (encouraging refusal)
    // instead of the conversation pass-through (which would
    // re-arm `_halt_pending` permanently).
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = auto_stop_transcript(dir.path(), "t.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&autonomous_stop_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(false),
        ))
        .unwrap(),
    )
    .unwrap();
    let jsonl = concat!(
        "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-code\"}}]}}\n",
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"wait, why X?\"}}\n",
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-continue</command-name>\"}}\n",
    );
    fs::write(&transcript, jsonl).unwrap();
    let result = check_autonomous_stop(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    assert!(
        result.should_block,
        "Rule 1 must refuse the Stop after /flow:flow-continue clears preceding prose"
    );
    assert_eq!(
        result.context,
        Some(RULE_1_STOP_REFUSED_MESSAGE.to_string()),
        "Rule 1 must emit the encouraging message, not Rule 2 or a pass-through"
    );
    assert!(
        !read_halt_pending(&state_path),
        "Rule 1 path must NOT set _halt_pending (the user already resumed)"
    );
}

#[test]
fn check_autonomous_stop_synthetic_user_turn_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = auto_stop_transcript(dir.path(), "t.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&autonomous_stop_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(false),
        ))
        .unwrap(),
    )
    .unwrap();
    // Tool-result wrapper (array content) is synthetic. The walker
    // must skip it and treat the transcript as having no user message.
    let body = format!(
        "{}\n{}\n",
        serde_json::to_string(&json!({
            "type": "assistant",
            "message": {"content": [{"type": "tool_use", "name": "Skill", "input": {"skill": "flow:flow-code"}}]}
        }))
        .unwrap(),
        serde_json::to_string(&json!({
            "type": "user",
            "message": {"content": [{"type": "tool_result", "tool_use_id": "x", "content": "ok"}]}
        }))
        .unwrap()
    );
    fs::write(&transcript, body).unwrap();
    let result = check_autonomous_stop(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    // No real user message → Rule 1 fires (no halt set).
    assert!(result.should_block);
    assert!(!read_halt_pending(&state_path));
}

#[test]
fn check_autonomous_stop_meta_user_turn_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = auto_stop_transcript(dir.path(), "t.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&autonomous_stop_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(false),
        ))
        .unwrap(),
    )
    .unwrap();
    // `isMeta:true` hook-injected feedback is synthetic.
    let body = format!(
        "{}\n{}\n",
        serde_json::to_string(&json!({
            "type": "assistant",
            "message": {"content": [{"type": "tool_use", "name": "Skill", "input": {"skill": "flow:flow-code"}}]}
        }))
        .unwrap(),
        serde_json::to_string(&json!({
            "type": "user",
            "isMeta": true,
            "message": {"content": "Stop hook feedback: ..."}
        }))
        .unwrap()
    );
    fs::write(&transcript, body).unwrap();
    let result = check_autonomous_stop(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    // Synthetic turn is skipped → Rule 1 fires.
    assert!(result.should_block);
    assert!(!read_halt_pending(&state_path));
}

#[test]
fn check_autonomous_stop_phase_status_normalized() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = auto_stop_transcript(dir.path(), "t.jsonl");
    // Uppercase + leading/trailing whitespace + NUL — all must
    // normalize to "in_progress" and the gate must still block.
    fs::write(
        &state_path,
        serde_json::to_string(&autonomous_stop_state(
            "flow-code",
            " In_Progress\u{0000}",
            json!("AUTO "),
            json!(false),
        ))
        .unwrap(),
    )
    .unwrap();
    write_auto_stop_transcript(&transcript, true, None);
    let result = check_autonomous_stop(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    assert!(result.should_block, "normalization must apply");
}

#[test]
fn check_autonomous_stop_skills_simple_and_detailed_shapes() {
    let dir = tempfile::tempdir().unwrap();
    // Simple "auto" string shape.
    let simple_state = dir.path().join("simple.json");
    let simple_transcript = auto_stop_transcript(dir.path(), "simple.jsonl");
    fs::write(
        &simple_state,
        serde_json::to_string(&autonomous_stop_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(false),
        ))
        .unwrap(),
    )
    .unwrap();
    write_auto_stop_transcript(&simple_transcript, true, None);
    let simple = check_autonomous_stop(
        &simple_state,
        Some(simple_transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(simple.should_block, "simple shape must block");

    // Detailed {"continue": "auto"} shape.
    let detailed_state = dir.path().join("detailed.json");
    let detailed_transcript = auto_stop_transcript(dir.path(), "detailed.jsonl");
    fs::write(
        &detailed_state,
        serde_json::to_string(&autonomous_stop_state(
            "flow-code",
            "in_progress",
            json!({"continue": "auto", "commit": "auto"}),
            json!(false),
        ))
        .unwrap(),
    )
    .unwrap();
    write_auto_stop_transcript(&detailed_transcript, true, None);
    let detailed = check_autonomous_stop(
        &detailed_state,
        Some(detailed_transcript.to_str().unwrap()),
        dir.path(),
    );
    assert!(detailed.should_block, "detailed shape must block");
}

#[test]
fn check_autonomous_stop_clears_stale_halt_when_phase_not_auto() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = auto_stop_transcript(dir.path(), "t.jsonl");
    // Stale `_halt_pending=true` from a prior phase. Current phase
    // is `manual` so the predicate must clear the residue and allow
    // the stop.
    fs::write(
        &state_path,
        serde_json::to_string(&autonomous_stop_state(
            "flow-code",
            "in_progress",
            json!("manual"),
            json!(true),
        ))
        .unwrap(),
    )
    .unwrap();
    write_auto_stop_transcript(&transcript, true, None);
    let result = check_autonomous_stop(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    assert!(!result.should_block);
    assert!(
        !read_halt_pending(&state_path),
        "stale halt must be cleared"
    );
}

// --- is_truthy normalization of _halt_pending ---
//
// `_halt_pending` is read by three surfaces in the autonomous-flow
// halt model: `check_autonomous_stop` (Stop event), `validate-skill`
// (Skill tool), and `validate-pretool` (Bash flow-advancing
// commands). All three must agree on what "halt is set" means so the
// user sees consistent guidance. Reading via the shared `is_truthy`
// predicate lets the three surfaces tolerate the JSON shape variation
// state files can carry (hand-edited "true" string, external-tool
// integer 1, normalized "TRUE", whitespace-padded "  true  ") per
// `.claude/rules/security-gates.md` "Normalize Before Comparing" and
// `.claude/rules/rust-patterns.md` "Hook Input Boolean Field
// Tolerance". A regression that swaps the read back to raw
// `.as_bool()` would re-introduce divergence between these surfaces:
// `validate-skill` would block Skill calls while
// `check_autonomous_stop` would emit Rule 1 (encouraging) instead of
// Rule 2 (halt-pending) — the user gets contradictory guidance from
// two hooks that share the same field.

#[test]
fn check_autonomous_stop_treats_string_true_halt_as_set_rule2() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = auto_stop_transcript(dir.path(), "t.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&autonomous_stop_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!("true"),
        ))
        .unwrap(),
    )
    .unwrap();
    write_auto_stop_transcript(&transcript, true, None);
    let result = check_autonomous_stop(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    assert!(result.should_block);
    let context = result.context.expect("rule 2 message");
    assert!(
        context.contains("/flow:flow-continue"),
        "string \"true\" halt must trigger Rule 2: {}",
        context
    );
}

#[test]
fn check_autonomous_stop_treats_nonzero_float_halt_as_set_rule2() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = auto_stop_transcript(dir.path(), "t.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&autonomous_stop_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!(0.5),
        ))
        .unwrap(),
    )
    .unwrap();
    write_auto_stop_transcript(&transcript, true, None);
    let result = check_autonomous_stop(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    assert!(result.should_block);
    let context = result.context.expect("rule 2 message");
    assert!(
        context.contains("/flow:flow-continue"),
        "non-zero float halt must trigger Rule 2: {}",
        context
    );
}

#[test]
fn check_autonomous_stop_treats_string_one_halt_as_set_rule2() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = auto_stop_transcript(dir.path(), "t.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&autonomous_stop_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!("1"),
        ))
        .unwrap(),
    )
    .unwrap();
    write_auto_stop_transcript(&transcript, true, None);
    let result = check_autonomous_stop(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    assert!(result.should_block);
    let context = result.context.expect("rule 2 message");
    assert!(
        context.contains("/flow:flow-continue"),
        "string \"1\" halt must trigger Rule 2: {}",
        context
    );
}

#[test]
fn check_autonomous_stop_treats_uppercase_true_halt_as_set_rule2() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = auto_stop_transcript(dir.path(), "t.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&autonomous_stop_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!("TRUE"),
        ))
        .unwrap(),
    )
    .unwrap();
    write_auto_stop_transcript(&transcript, true, None);
    let result = check_autonomous_stop(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    assert!(result.should_block);
    let context = result.context.expect("rule 2 message");
    assert!(
        context.contains("/flow:flow-continue"),
        "uppercase \"TRUE\" halt must trigger Rule 2: {}",
        context
    );
}

#[test]
fn check_autonomous_stop_treats_whitespace_padded_true_halt_as_set_rule2() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = auto_stop_transcript(dir.path(), "t.jsonl");
    fs::write(
        &state_path,
        serde_json::to_string(&autonomous_stop_state(
            "flow-code",
            "in_progress",
            json!("auto"),
            json!("  true  "),
        ))
        .unwrap(),
    )
    .unwrap();
    write_auto_stop_transcript(&transcript, true, None);
    let result = check_autonomous_stop(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    assert!(result.should_block);
    let context = result.context.expect("rule 2 message");
    assert!(
        context.contains("/flow:flow-continue"),
        "whitespace-padded \"  true  \" halt must trigger Rule 2: {}",
        context
    );
}

#[test]
fn check_autonomous_stop_clears_stale_truthy_string_halt_on_phase_complete() {
    // The stale-halt-clear branch reads `_halt_pending` via the same
    // `is_truthy` normalization so a truthy non-bool residue (e.g.,
    // `"true"`, `1`, `"TRUE"`) is recognized AND cleared when the
    // phase is no longer in_progress + auto. Without the shared
    // predicate, the clear path missed non-bool truthy shapes and
    // left the PreToolUse halt gates stuck.
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let transcript = auto_stop_transcript(dir.path(), "t.jsonl");
    let state = json!({
        "branch": "test",
        "current_phase": "flow-code",
        "phases": { "flow-code": { "status": "complete" } },
        "skills": { "flow-code": "auto" },
        "_halt_pending": "true",
    });
    fs::write(&state_path, serde_json::to_string(&state).unwrap()).unwrap();
    write_auto_stop_transcript(&transcript, true, None);
    let _ = check_autonomous_stop(&state_path, Some(transcript.to_str().unwrap()), dir.path());
    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    let halt_after = on_disk.get("_halt_pending").cloned().unwrap_or(Value::Null);
    // Re-check whether the cleared value is still truthy under
    // is_truthy semantics. A clean state writes `false`; tolerant
    // readers must see "not set" afterward.
    let still_truthy = match &halt_after {
        Value::Bool(true) => true,
        Value::String(s) => {
            let t = s.trim().to_ascii_lowercase();
            t == "true" || t == "1"
        }
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        _ => false,
    };
    assert!(
        !still_truthy,
        "stale truthy non-bool _halt_pending must be cleared when phase is not in_progress+auto: {}",
        halt_after
    );
}
