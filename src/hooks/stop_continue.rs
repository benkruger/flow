//! Stop hook that forces continuation when `_continue_pending` is set.
//!
//! When a phase skill sets `_continue_pending=<skill_name>` in the state
//! file before invoking a child skill, this hook fires when the model
//! tries to end its turn. If the flag is non-empty, the hook clears it
//! and blocks the stop, forcing Claude to continue generating and follow
//! the parent skill's remaining instructions.
//!
//! Fail-open with error reporting: any error allows the stop (exit 0,
//! no block output), but writes a diagnostic to stderr and attempts to
//! log to `.flow-states/<branch>.log` for post-mortem visibility.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::commands::clear_blocked::clear_blocked;
use crate::commands::set_blocked::set_blocked;
use crate::git::{project_root, resolve_branch};
use crate::github::detect_repo;
use crate::lock::mutate_state;
use crate::phase_config::find_state_files;
use crate::utils::{now, write_tab_sequences};

/// Result of `check_continue`.
pub struct ContinueResult {
    pub should_block: bool,
    pub skill: Option<String>,
    pub context: Option<String>,
}

/// Write a diagnostic to stderr and (best-effort) append to the flow log.
fn log_diag(root: Option<&Path>, branch: Option<&str>, message: &str) {
    eprintln!("[FLOW stop-continue] {}", message);
    if let (Some(root), Some(branch)) = (root, branch) {
        let log_path = root.join(".flow-states").join(format!("{}.log", branch));
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&log_path) {
            let _ = writeln!(f, "{} [stop-continue] {}", now(), message);
        }
    }
}

/// Derive `(root, branch)` from a state file path of the form
/// `<root>/.flow-states/<branch>.json`, so diagnostic logging can
/// locate `<root>/.flow-states/<branch>.log` without callers having
/// to pass both pieces separately.
///
/// Returns `(None, None)` when the path shape does not match
/// (e.g., test fixtures that place the state file outside a
/// `.flow-states/` directory). Callers should pass the resulting
/// options to `log_diag` directly — when either is None, the file
/// write is skipped and only stderr is used.
fn derive_root_branch(state_path: &Path) -> (Option<&Path>, Option<&str>) {
    let branch = state_path.file_stem().and_then(|s| s.to_str());
    let root = state_path.parent().and_then(|p| {
        if p.file_name().and_then(|n| n.to_str()) == Some(".flow-states") {
            p.parent()
        } else {
            None
        }
    });
    (root, branch)
}

/// Update `session_id` and `transcript_path` in the active state file.
///
/// Fail-open with diagnostic: on any `mutate_state` error (corrupt
/// JSON, locked file, I/O failure) the error is logged via
/// `log_diag` to stderr and the branch log for post-mortem
/// visibility, matching the Python `capture_session_id` contract.
pub fn capture_session_id(hook_input: &Value, state_path: &Path) {
    let session_id = match hook_input.get("session_id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return,
    };

    if !state_path.exists() {
        return;
    }

    let transcript_path = hook_input
        .get("transcript_path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if let Err(e) = mutate_state(state_path, |state| {
        // Guard: state must be an object (or Null, which auto-converts)
        // for string-key mutations. Fail-open on other shapes.
        if !(state.is_object() || state.is_null()) {
            return;
        }
        if state.get("session_id").and_then(|v| v.as_str()) == Some(session_id.as_str()) {
            return;
        }
        state["session_id"] = Value::String(session_id.clone());
        if let Some(tp) = &transcript_path {
            state["transcript_path"] = Value::String(tp.clone());
        }
    }) {
        let (root, branch) = derive_root_branch(state_path);
        log_diag(root, branch, &format!("capture_session_id error: {}", e));
    }
}

/// Check if `_continue_pending` flag is set in the active state file.
///
/// If should_block is true, both `_continue_pending` and `_continue_context`
/// have been cleared in the state file.
///
/// Session isolation: if the state file's session_id differs from the
/// hook input's session_id, the flag is stale (set by a previous session).
/// Clear it and allow stop.
pub fn check_continue(hook_input: &Value, state_path: &Path) -> ContinueResult {
    if !state_path.exists() {
        return ContinueResult {
            should_block: false,
            skill: None,
            context: None,
        };
    }

    // Filter out empty session_id strings from the hook input: the
    // Python original used `hook_input.get("session_id")` which is
    // falsy on both missing keys AND empty strings, so the downstream
    // `if state_sid and hook_sid` check skipped the mismatch logic in
    // both cases. Preserve that backward-compat behavior.
    let hook_sid = hook_input
        .get("session_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    // Use RefCell-like pattern with local mutable state
    let mut should_block = false;
    let mut skill: Option<String> = None;
    let mut context: Option<String> = None;
    let mut decision: Option<String> = None;

    let _ = mutate_state(state_path, |state| {
        // Guard: state must be an object (or Null, which auto-converts)
        // for string-key mutations to succeed without panicking.
        if !(state.is_object() || state.is_null()) {
            return;
        }
        let pending = state
            .get("_continue_pending")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if pending.is_empty() {
            return;
        }

        let state_sid = state
            .get("session_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        if let (Some(ssid), Some(hsid)) = (state_sid.as_ref(), hook_sid.as_ref()) {
            if ssid != hsid {
                state["_continue_pending"] = Value::String(String::new());
                state["_continue_context"] = Value::String(String::new());
                // Note: _stop_instructed is NOT cleared here. Clearing it
                // would cause check_discussion_mode to re-fire in the same
                // hook invocation (a non-user-initiated Stop). phase_enter()
                // clears it when the new session enters its first phase.
                decision = Some(format!(
                    "session mismatch (state={} hook={}), cleared pending={}",
                    ssid, hsid, pending
                ));
                return;
            }
        }

        let ctx = state
            .get("_continue_context")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        state["_continue_pending"] = Value::String(String::new());
        state["_continue_context"] = Value::String(String::new());
        // Clear discussion-mode flag so the next user interruption
        // re-triggers the flow-note instruction.
        if let Some(obj) = state.as_object_mut() {
            obj.remove("_stop_instructed");
        }
        should_block = true;
        skill = Some(pending.clone());
        context = ctx;
        decision = Some(format!("blocking: pending={}", pending));
    });

    if let Some(msg) = decision {
        let (root, branch) = derive_root_branch(state_path);
        log_diag(root, branch, &msg);
    }

    ContinueResult {
        should_block,
        skill,
        context,
    }
}

/// Set `_blocked` flag when the session is going idle.
///
/// Delegates to `commands::set_blocked::set_blocked` which writes
/// `_blocked = now()`. Same effect as the Python `set_blocked_idle`.
pub fn set_blocked_idle(state_path: &Path) {
    set_blocked(state_path);
}

/// Check for a QA continuation breadcrumb at
/// `.flow-states/qa-pending.json`.
///
/// Returns (should_block, context). Does NOT delete the file — the QA
/// skill handles cleanup. Fail-open: any error returns (false, None).
pub fn check_qa_pending(root: &Path) -> (bool, Option<String>) {
    let qa_path = root.join(".flow-states").join("qa-pending.json");
    if !qa_path.exists() {
        return (false, None);
    }
    let content = match std::fs::read_to_string(&qa_path) {
        Ok(c) => c,
        Err(_) => return (false, None),
    };
    let data: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return (false, None),
    };
    let context = data
        .get("_continue_context")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    match context {
        Some(ctx) => (true, Some(ctx)),
        None => (false, None),
    }
}

/// Write the repo color to the terminal tab via /dev/tty.
///
/// Wraps `write_tab_sequences` with root/branch-aware fallback logic:
/// if the branch state file exists use its contents, otherwise scan for
/// any active feature state, otherwise call with just the detected repo.
pub fn set_tab_color(root: &Path, branch: &str, state_path: &Path) {
    let result = if state_path.exists() {
        match std::fs::read_to_string(state_path) {
            Ok(content) => match serde_json::from_str::<Value>(&content) {
                Ok(state) => {
                    let repo = state.get("repo").and_then(|v| v.as_str());
                    write_tab_sequences(repo, Some(root))
                }
                Err(_) => write_tab_sequences(detect_repo(Some(root)).as_deref(), Some(root)),
            },
            Err(_) => write_tab_sequences(detect_repo(Some(root)).as_deref(), Some(root)),
        }
    } else {
        // No state file — find any active feature first, fall back to detect_repo
        let results = find_state_files(root, branch);
        if let Some((_, state, _)) = results.first() {
            let repo = state.get("repo").and_then(|v| v.as_str());
            write_tab_sequences(repo, Some(root))
        } else {
            write_tab_sequences(detect_repo(Some(root)).as_deref(), Some(root))
        }
    };
    if let Err(e) = result {
        log_diag(
            Some(root),
            Some(branch),
            &format!("set_tab_color error: {}", e),
        );
    }
}

/// Block reason for discussion mode — instructs the model to invoke
/// flow:flow-note before continuing and to wait for the user to finish.
pub const DISCUSSION_BLOCK_REASON: &str = "\
The user interrupted the session. Before continuing any work:

1. Invoke /flow:flow-note to capture any correction or learning the user expressed.
2. After the note is captured, respond to the user's message directly.
3. Do NOT resume the previous skill or task until the user explicitly says to continue.

Wait for the user — they are not done talking.";

/// Check if this is the first user interruption during an active flow.
///
/// On the first Stop event where `_stop_instructed` is not already set
/// (bool `true`), sets the flag and returns a blocking `ContinueResult`
/// with `DISCUSSION_BLOCK_REASON` as context. On subsequent stops
/// (flag already `true`), allows the stop through.
///
/// Non-bool values for `_stop_instructed` (e.g. string `"true"`) are
/// treated as not-set — the hook re-blocks and corrects the flag to
/// bool `true` (self-healing).
///
/// Returns a non-blocking result when the state file does not exist
/// (no active flow).
pub fn check_discussion_mode(state_path: &Path) -> ContinueResult {
    if !state_path.exists() {
        return ContinueResult {
            should_block: false,
            skill: None,
            context: None,
        };
    }

    let mut should_block = false;

    let _ = mutate_state(state_path, |state| {
        if !(state.is_object() || state.is_null()) {
            return;
        }
        let already_instructed = state
            .get("_stop_instructed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if already_instructed {
            return;
        }
        state["_stop_instructed"] = json!(true);
        should_block = true;
    });

    if should_block {
        ContinueResult {
            should_block: true,
            skill: Some("discussion".to_string()),
            context: Some(DISCUSSION_BLOCK_REASON.to_string()),
        }
    } else {
        ContinueResult {
            should_block: false,
            skill: None,
            context: None,
        }
    }
}

/// Format the Stop-hook block output JSON.
///
/// Returns `{"decision": "block", "reason": "..."}` where `reason`
/// embeds the skill name and, when context is non-empty, the
/// parent phase's next-step instructions. Matches the Python
/// `stop-continue.py` main() output exactly so Claude Code's
/// stop-hook protocol stays backward compatible.
pub fn format_block_output(skill: &str, context: Option<&str>) -> Value {
    let reason = match context {
        Some(ctx) if !ctx.is_empty() => format!(
            "Continue parent phase — child skill '{}' has returned.\n\nNext steps:\n{}",
            skill, ctx
        ),
        _ => format!(
            "Continue parent phase — child skill '{}' has returned. Resume the parent skill instructions.",
            skill
        ),
    };
    json!({"decision": "block", "reason": reason})
}

/// Run the stop-continue hook (entry point).
///
/// Uses `resolve_branch` for `--branch` override support. Calls
/// `current_branch()` internally — does not scan `.flow-states/`.
pub fn run() {
    let mut stdin_buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut stdin_buf);

    let hook_input: Value = serde_json::from_str(&stdin_buf).unwrap_or_else(|_| json!({}));

    let root: PathBuf = project_root();
    let branch = resolve_branch(None, &root);
    let branch = match branch {
        Some(b) => b,
        None => return,
    };
    let state_path = root.join(".flow-states").join(format!("{}.json", branch));

    let mut result = check_continue(&hook_input, &state_path);

    capture_session_id(&hook_input, &state_path);

    // Fallback: check for QA continuation breadcrumb when no branch
    // state file blocked the stop.
    if !result.should_block {
        let (qa_block, qa_context) = check_qa_pending(&root);
        if qa_block {
            result.should_block = true;
            result.skill = Some("flow-complete".to_string());
            result.context = qa_context;
        }
    }

    // Discussion mode: on the first user interruption during an active
    // flow, block the stop and instruct the model to capture corrections
    // via flow-note before continuing.
    if !result.should_block {
        let disc = check_discussion_mode(&state_path);
        if disc.should_block {
            result = disc;
        }
    }

    // Blocked flag: CLEAR when session is continuing (blocking),
    // SET when session is going idle (not blocking).
    if result.should_block {
        clear_blocked(&state_path);
    } else {
        set_blocked_idle(&state_path);
    }

    set_tab_color(&root, &branch, &state_path);

    if result.should_block {
        let skill_name = result.skill.as_deref().unwrap_or("");
        // Discussion mode uses DISCUSSION_BLOCK_REASON directly as the
        // reason — not the "child skill returned" framing from
        // format_block_output, which is designed for _continue_pending.
        let output = if skill_name == "discussion" {
            json!({"decision": "block", "reason": result.context.as_deref().unwrap_or(DISCUSSION_BLOCK_REASON)})
        } else {
            format_block_output(skill_name, result.context.as_deref())
        };
        println!("{}", serde_json::to_string(&output).unwrap());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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

        // Flags cleared
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
        assert!(state["_blocked"].as_str().unwrap().len() > 0);
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

    // --- set_tab_color ---
    // Note: write_tab_sequences writes to /dev/tty, which may or may not be
    // writable in the test environment. We test that set_tab_color does not
    // panic in various state-file scenarios.

    #[test]
    fn test_set_tab_color_with_state_file() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let state_path = state_dir.join("test.json");
        fs::write(&state_path, r#"{"repo": "owner/repo"}"#).unwrap();

        // Should not panic — /dev/tty write may fail silently on CI
        set_tab_color(dir.path(), "test", &state_path);
    }

    #[test]
    fn test_set_tab_color_without_state_file() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let state_path = state_dir.join("test.json");

        // No state file exists and no other active features — should fall
        // back to detect_repo without panicking
        set_tab_color(dir.path(), "test", &state_path);
    }

    #[test]
    fn test_set_tab_color_corrupt_state_file() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let state_path = state_dir.join("test.json");
        fs::write(&state_path, "{bad json").unwrap();

        // Should not panic — falls through to detect_repo fallback
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
        // Empty string should trigger the "Resume the parent skill" variant,
        // not the "Next steps" variant with a blank body.
        assert!(reason.ends_with("Resume the parent skill instructions."));
        assert!(!reason.contains("Next steps:"));
    }

    #[test]
    fn test_format_block_output_empty_skill_name() {
        // Defensive: an empty skill name still produces a well-formed
        // reason string rather than panicking or producing invalid JSON.
        let out = format_block_output("", None);
        assert_eq!(out["decision"], "block");
        assert!(out["reason"].as_str().unwrap().contains("child skill ''"));
    }

    // --- derive_root_branch ---

    #[test]
    fn test_derive_root_branch_canonical_layout() {
        // Given state at <root>/.flow-states/<branch>.json, the
        // helper recovers both root and branch so diagnostic logs
        // land at <root>/.flow-states/<branch>.log.
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let state_path = state_dir.join("my-feature.json");
        fs::write(&state_path, r#"{}"#).unwrap();

        let (root, branch) = derive_root_branch(&state_path);
        assert_eq!(root, Some(dir.path()));
        assert_eq!(branch, Some("my-feature"));
    }

    #[test]
    fn test_derive_root_branch_returns_none_when_not_in_flow_states() {
        // When the state file is not inside a `.flow-states/`
        // directory (common in unit-test fixtures that use a flat
        // tempdir), the helper returns None for root so log_diag
        // skips the file write instead of polluting a parent dir.
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        fs::write(&state_path, r#"{}"#).unwrap();

        let (root, _) = derive_root_branch(&state_path);
        assert_eq!(root, None);
    }

    // --- check_continue log file writes ---
    // Closes the coverage gap flagged by the reviewer agent: the
    // `check_continue` log_diag calls were previously passing
    // (None, None) and silently dropping the log file write. These
    // tests use the canonical `.flow-states/<branch>.json` layout
    // so `derive_root_branch` can recover the log path.

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
        // When there is no decision to make (pending empty), no log
        // entry should be written — only decisions are logged.
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let state_path = state_dir.join("test-branch.json");
        fs::write(&state_path, r#"{"branch": "test"}"#).unwrap();

        check_continue(&json!({}), &state_path);

        let log_path = state_dir.join("test-branch.log");
        assert!(
            !log_path.exists(),
            "no log entry expected when no decision is made"
        );
    }

    // --- capture_session_id error logging ---
    // Closes the coverage gap flagged by the reviewer agent: the
    // Python `capture_session_id` logged on mutate_state errors but
    // the Rust port was silently dropping them.

    #[test]
    fn test_capture_session_id_corrupt_state_logs_error() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let state_path = state_dir.join("test-branch.json");
        // Corrupt JSON triggers mutate_state's Json error path.
        fs::write(&state_path, "{bad json").unwrap();

        capture_session_id(&json!({"session_id": "abc123"}), &state_path);

        // Log file should exist with the error diagnostic.
        let log_path = state_dir.join("test-branch.log");
        assert!(log_path.exists(), "corrupt-state errors must be logged");
        let log_content = fs::read_to_string(&log_path).unwrap();
        assert!(log_content.contains("capture_session_id error"));
    }

    // --- Adversarial findings: array state and empty hook session_id ---

    #[test]
    fn test_check_continue_array_state_file_does_not_crash() {
        // An array-shaped state file must not panic when the Stop
        // hook fires. The `is_object() || is_null()` guard in the
        // transform closure catches it before any `state["key"] = v`
        // would panic in serde_json's IndexMut.
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
        // When the hook sends session_id="", it must be treated as
        // "no session_id" (Python's falsy semantics), not as a
        // session mismatch. A valid _continue_pending flag should
        // still fire.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let initial = json!({
            "session_id": "existing-session",
            "_continue_pending": "flow-commit",
            "_continue_context": "Next: run tests"
        });
        fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

        let result = check_continue(&json!({"session_id": ""}), &path);
        assert!(
            result.should_block,
            "empty hook session_id must not trigger session mismatch"
        );
        assert_eq!(result.skill.unwrap(), "flow-commit");
        assert_eq!(result.context.unwrap(), "Next: run tests");
    }

    #[test]
    fn test_check_continue_empty_state_session_id_still_blocks() {
        // Symmetric: state session_id="" should also be treated as
        // absent, not as a different session from the hook's sid.
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
        assert!(
            reason.contains("flow:flow-note"),
            "block reason must mention flow:flow-note"
        );
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
        // String "true" is not a bool — as_bool() returns None,
        // so the hook treats it as not-set and re-blocks, setting
        // the flag to the correct bool true.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let initial = json!({"branch": "test", "_stop_instructed": "true"});
        fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

        let result = check_discussion_mode(&path);
        assert!(
            result.should_block,
            "non-bool flag must be treated as not-set"
        );

        let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            state["_stop_instructed"],
            json!(true),
            "flag must be corrected to bool"
        );
    }

    #[test]
    fn test_discussion_mode_clears_blocked() {
        // When discussion mode blocks, the run() control flow hits
        // clear_blocked (not set_blocked_idle). Verify by simulating
        // the sequence: set _blocked, then run check_discussion_mode
        // followed by clear_blocked — _blocked must be absent.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let initial = json!({"branch": "test", "_blocked": "2024-01-01T00:00:00"});
        fs::write(&path, serde_json::to_string(&initial).unwrap()).unwrap();

        let disc = check_discussion_mode(&path);
        assert!(disc.should_block);

        // Simulate run()'s blocked flag branch
        clear_blocked(&path);

        let state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(
            state.get("_blocked").is_none(),
            "_blocked must be cleared when discussion mode blocks"
        );
    }

    #[test]
    fn test_discussion_mode_cleared_on_continue_pending() {
        // When check_continue consumes _continue_pending, it must also
        // clear _stop_instructed so the next user interruption re-triggers
        // discussion mode.
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
        assert!(
            state.get("_stop_instructed").is_none(),
            "_stop_instructed must be cleared when _continue_pending is consumed"
        );
    }
}
