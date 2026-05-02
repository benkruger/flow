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
use crate::flow_paths::FlowPaths;
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
        let log_path = FlowPaths::new(root, branch).log_file();
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&log_path) {
            let _ = writeln!(f, "{} [stop-continue] {}", now(), message);
        }
    }
}

/// Derive `(root, branch)` from a state file path of the form
/// `<root>/.flow-states/<branch>/state.json`, so diagnostic logging
/// can locate `<root>/.flow-states/<branch>/log` without callers
/// having to pass both pieces separately.
///
/// Returns `(None, None)` when the path shape does not match
/// (e.g., test fixtures that place the state file outside a
/// `.flow-states/<branch>/` directory). Callers should pass the
/// resulting options to `log_diag` directly — when either is None,
/// the file write is skipped and only stderr is used.
fn derive_root_branch(state_path: &Path) -> (Option<&Path>, Option<&str>) {
    let branch_dir = state_path.parent();
    let branch = branch_dir.and_then(|d| d.file_name()).and_then(|n| n.to_str());
    let root = branch_dir.and_then(|d| d.parent()).and_then(|p| {
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
/// visibility. The hook must never block the SessionStart event, so
/// errors are recorded rather than propagated.
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

    if let Err(e) = mutate_state(state_path, &mut |state| {
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

    // Treat both a missing `session_id` key and an empty-string
    // `session_id` as "no session id" so the downstream session-id
    // mismatch branch (which only fires when both `state_sid` and
    // `hook_sid` are `Some`) is skipped in both cases. Without this
    // filter, an empty-string session id would falsely look like a
    // mismatch and clear pending state.
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

    let _ = mutate_state(state_path, &mut |state| {
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
                // would cause check_first_stop to re-enter discussion mode
                // on the next stop (a non-user-initiated Stop). phase_enter()
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
        // re-triggers the flow-note instruction. `state` is guaranteed
        // to be an object at this point — the state["key"] = ...
        // assignments above auto-vivify Value::Null into an object and
        // the outer guard rejected any other shape.
        state
            .as_object_mut()
            .expect("state is an object after key assignments")
            .remove("_stop_instructed");
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
/// `_blocked = now()`. The flag is read by status displays so they
/// can show "session idle since X" until the next phase action
/// clears it.
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
/// **Superseded in `run()` by `check_first_stop()`** which handles both
/// discussion mode and pending continuations in a single function.
/// Not called from the production `run()` path — retained as a
/// standalone building block with its own test suite.
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

    let _ = mutate_state(state_path, &mut |state| {
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

/// Handle the first stop event during an active flow.
///
/// Runs BEFORE `check_continue()` in `run()`. On the first Stop event
/// (when `_stop_instructed` is not already set), this function handles
/// both cases:
///
/// 1. `_continue_pending` is set: consume it, set `_stop_instructed=true`,
///    and block with a conditional message that tells the model to check
///    for user messages before auto-continuing. This prevents pending
///    continuations from trampling user conversations.
///
/// 2. No `_continue_pending`: set `_stop_instructed=true` and block with
///    `DISCUSSION_BLOCK_REASON` (pure discussion mode).
///
/// On subsequent stops (`_stop_instructed` already true), returns
/// non-blocking so `check_continue()` can handle multi-child-skill chains.
///
/// Key difference from `check_continue()`: does NOT remove
/// `_stop_instructed` after consuming pending. `check_continue()` clears
/// the flag because it handles multi-child-skill chains where each
/// successive child completion should re-enable first-stop logic.
/// `check_first_stop()` preserves the flag because it runs once per
/// stop-cycle to establish discussion-mode boundaries — clearing it
/// would allow subsequent stops to incorrectly re-enter discussion mode,
/// duplicating the flow-note instruction on every stop event.
pub fn check_first_stop(hook_input: &Value, state_path: &Path) -> ContinueResult {
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
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let mut should_block = false;
    let mut skill: Option<String> = None;
    let mut context: Option<String> = None;
    let mut decision: Option<String> = None;

    let _ = mutate_state(state_path, &mut |state| {
        if !(state.is_object() || state.is_null()) {
            return;
        }

        // If already instructed, let check_continue handle subsequent stops
        let already_instructed = state
            .get("_stop_instructed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if already_instructed {
            return;
        }

        // First stop — always set _stop_instructed
        state["_stop_instructed"] = json!(true);

        // Check for pending continuation
        let pending = state
            .get("_continue_pending")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if pending.is_empty() {
            // No pending — pure discussion mode
            should_block = true;
            skill = Some("discussion".to_string());
            context = Some(DISCUSSION_BLOCK_REASON.to_string());
            decision = Some("first stop, no pending — discussion mode".to_string());
            return;
        }

        // Session isolation check
        let state_sid = state
            .get("session_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        if let (Some(ssid), Some(hsid)) = (state_sid.as_ref(), hook_sid.as_ref()) {
            if ssid != hsid {
                // Stale pending from previous session — clear and fall through to discussion
                state["_continue_pending"] = Value::String(String::new());
                state["_continue_context"] = Value::String(String::new());
                should_block = true;
                skill = Some("discussion".to_string());
                context = Some(DISCUSSION_BLOCK_REASON.to_string());
                decision = Some(format!(
                    "first stop, session mismatch (state={} hook={}), cleared pending={} — discussion mode",
                    ssid, hsid, pending
                ));
                return;
            }
        }

        // Valid pending — consume and block with conditional message
        let ctx = state
            .get("_continue_context")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        state["_continue_pending"] = Value::String(String::new());
        state["_continue_context"] = Value::String(String::new());
        // NOTE: do NOT remove _stop_instructed here (unlike check_continue)
        // This ensures discussion mode does not re-fire on subsequent stops

        let reason = format_conditional_continue_reason(&pending, ctx.as_deref());
        should_block = true;
        // "discussion-with-pending" distinguishes this path from pure "discussion"
        // in run()'s output formatting — both bypass format_block_output() and use
        // the context directly as the block reason. The distinct name exists for
        // diagnostic logging (log_diag can distinguish the two paths).
        skill = Some("discussion-with-pending".to_string());
        context = Some(reason);
        decision = Some(format!(
            "first stop, conditional continue: pending={}",
            pending
        ));
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

/// Format the Stop-hook block output JSON.
///
/// Returns `{"decision": "block", "reason": "..."}` where `reason`
/// embeds the skill name and, when context is non-empty, the
/// parent phase's next-step instructions. The output format is
/// part of Claude Code's stop-hook protocol contract.
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

/// Format a conditional continue message for the first stop event when
/// `_continue_pending` is set.
///
/// Unlike `format_block_output` which unconditionally says "Continue parent
/// phase", this message instructs the model to check whether the user sent
/// a message during the interrupt. If so, the model should answer the user
/// and invoke flow:flow-note before resuming. If not, the model should
/// continue the parent phase automatically.
///
/// This prevents `_continue_pending` from trampling user conversations on
/// the first interrupt — the user's message gets priority over auto-continue.
pub fn format_conditional_continue_reason(skill: &str, context: Option<&str>) -> String {
    let next_steps = match context {
        Some(ctx) if !ctx.is_empty() => format!("Next steps:\n{}", ctx),
        _ => "Resume the parent skill instructions.".to_string(),
    };
    format!(
        "A child skill '{}' has completed.\n\n\
         Check the conversation context:\n\
         - If the user sent a message since the last skill action, answer their message first. \
         Invoke /flow:flow-note to capture any correction or learning. \
         Do NOT resume the flow until the user explicitly says to continue.\n\
         - If no new user message was sent, continue the parent phase.\n\n\
         {}",
        skill, next_steps
    )
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
    // Slash-containing git branches (`feature/foo`) are not valid FLOW
    // branches — treat them as "no active flow" rather than panicking.
    let state_path = match FlowPaths::try_new(&root, &branch) {
        Some(p) => p.state_file(),
        None => return,
    };

    // First stop handler: on the first Stop event (no _stop_instructed),
    // handles both pending continuations (with conditional user-awareness)
    // and pure discussion mode. Subsequent stops fall through to check_continue.
    let mut result = check_first_stop(&hook_input, &state_path);

    // Multi-child-skill chains: after the first stop set _stop_instructed,
    // subsequent child skill completions need check_continue to fire.
    if !result.should_block {
        result = check_continue(&hook_input, &state_path);
    }

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
        // Discussion mode and discussion-with-pending both use their context
        // directly as the reason — not the "child skill returned" framing
        // from format_block_output, which is designed for multi-child-skill
        // check_continue continuations.
        let output = if skill_name == "discussion" || skill_name == "discussion-with-pending" {
            json!({"decision": "block", "reason": result.context.as_deref().unwrap_or(DISCUSSION_BLOCK_REASON)})
        } else {
            format_block_output(skill_name, result.context.as_deref())
        };
        println!("{}", serde_json::to_string(&output).unwrap());
    }
}
