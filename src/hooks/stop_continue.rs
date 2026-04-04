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
use crate::git::{current_branch, project_root};
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

/// Update `session_id` and `transcript_path` in the active state file.
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

    let _ = mutate_state(state_path, |state| {
        if state.get("session_id").and_then(|v| v.as_str()) == Some(session_id.as_str()) {
            return;
        }
        state["session_id"] = Value::String(session_id.clone());
        if let Some(tp) = &transcript_path {
            state["transcript_path"] = Value::String(tp.clone());
        }
    });
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

    let hook_sid = hook_input
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Use RefCell-like pattern with local mutable state
    let mut should_block = false;
    let mut skill: Option<String> = None;
    let mut context: Option<String> = None;
    let mut decision: Option<String> = None;

    let _ = mutate_state(state_path, |state| {
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
            .map(|s| s.to_string());

        if let (Some(ssid), Some(hsid)) = (state_sid.as_ref(), hook_sid.as_ref()) {
            if ssid != hsid {
                state["_continue_pending"] = Value::String(String::new());
                state["_continue_context"] = Value::String(String::new());
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
        should_block = true;
        skill = Some(pending.clone());
        context = ctx;
        decision = Some(format!("blocking: pending={}", pending));
    });

    if let Some(msg) = decision {
        log_diag(None, None, &msg);
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
        log_diag(Some(root), Some(branch), &format!("set_tab_color error: {}", e));
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
pub fn run() {
    let mut stdin_buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut stdin_buf);

    let hook_input: Value = serde_json::from_str(&stdin_buf).unwrap_or_else(|_| json!({}));

    let branch = match current_branch() {
        Some(b) => b,
        None => return,
    };

    let root: PathBuf = project_root();
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
        let output = format_block_output(skill_name, result.context.as_deref());
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
        fs::write(
            &qa_path,
            r#"{"_continue_context": "finish QA tests"}"#,
        )
        .unwrap();

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
}
