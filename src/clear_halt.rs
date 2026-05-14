//! `bin/flow clear-halt` — clear `_halt_pending` so an autonomous
//! flow that was paused by `check_autonomous_stop` resumes.
//!
//! Invoked by `skills/flow-continue/SKILL.md` as its only step.
//! `/flow:flow-continue` is in `USER_ONLY_SKILLS`, so the model
//! cannot invoke the skill — only a user typing the slash command
//! reaches this subcommand. The subcommand independently
//! self-gates via
//! `transcript_walker::last_user_message_invokes_skill` so a
//! direct Bash invocation by the model also cannot clear the halt
//! without the user typing `/flow:flow-continue`.
//!
//! Output shape (all paths exit 0; callers parse `status`):
//! - `{"status":"ok"}` — `_halt_pending` cleared.
//! - `{"status":"skipped","reason":"no_state_file"}` — branch has
//!   no active flow (state file absent).
//! - `{"status":"error","reason":"unauthorized","message":...}` —
//!   the persisted transcript's most recent real user turn does
//!   NOT start with either emission shape Claude Code uses for
//!   `/flow:flow-continue`: the two-line shape
//!   `<command-message>flow:flow-continue</command-message>\n<command-name>/flow:flow-continue</command-name>`
//!   (Claude Code 2.1.140+) or the legacy one-line shape
//!   `<command-name>/flow:flow-continue</command-name>`. The walker
//!   accepts either via `starts_with` disjunction.
//! - `{"status":"error","reason":"invalid_branch"}` — branch
//!   fails `FlowPaths::is_valid_branch`.
//! - `{"status":"error","reason":"no_transcript_path"}` — state
//!   file lacks a usable `session_id` or `transcript_path`.
//! - `{"status":"error","reason":"state_write_failed"}` — the
//!   state file could not be read, parsed, locked, or written.
//!   Surfaces every `mutate_state` infrastructure failure
//!   (`MutateError::Io`, `Lock`, `Json`).
//!
//! Tests live at `tests/clear_halt.rs` per
//! `.claude/rules/test-placement.md`.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use clap::Parser;
use serde_json::{json, Value};

use crate::flow_paths::FlowPaths;
use crate::hooks::transcript_walker::last_user_message_invokes_skill;
use crate::lock::mutate_state;
use crate::per_flow_capture::derive_transcript_path;
use crate::session_metrics::{is_safe_session_id, is_safe_transcript_path};

/// Skill name the transcript walker checks for. The user types
/// `/flow:flow-continue` and Claude Code emits the slash-command
/// marker at the start of the user-role turn's `message.content`
/// in one of two shapes — the two-line
/// `<command-message>flow:flow-continue</command-message>\n<command-name>/flow:flow-continue</command-name>`
/// (Claude Code 2.1.140+) or the legacy
/// `<command-name>/flow:flow-continue</command-name>`. The walker
/// accepts either as the `starts_with` anchor.
const CONTINUE_SKILL: &str = "flow:flow-continue";

#[derive(Parser, Debug)]
#[command(
    name = "clear-halt",
    about = "Clear the _halt_pending state field so an autonomous flow resumes"
)]
pub struct Args {
    /// Branch whose state file holds `_halt_pending`. Validated
    /// through `FlowPaths::try_new` per
    /// `.claude/rules/branch-path-safety.md`.
    #[arg(long)]
    pub branch: String,
}

/// Read state's `session_id` and `transcript_path` and resolve a
/// validated transcript PathBuf. Returns `None` when neither field
/// produces a path that passes `is_safe_transcript_path`. The
/// caller maps `None` to a `no_transcript_path` error.
///
/// `state.get(...)` on a non-Object `Value` returns `None` for
/// every variant (Null, Array, Number, String, Bool), so a
/// `Some` return from this helper proves the state root is an
/// Object with at least one usable session field. Downstream
/// `mutate_state` callers rely on this invariant to omit a
/// per-closure object-shape guard.
fn resolve_transcript_path(state: &Value, home: &Path, project_root: &Path) -> Option<PathBuf> {
    let session_id = state
        .get("session_id")
        .and_then(|v| v.as_str())
        .filter(|s| is_safe_session_id(s))
        .map(|s| s.to_string());
    state
        .get("transcript_path")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .filter(|p| is_safe_transcript_path(p, home))
        .or_else(|| {
            session_id
                .as_ref()
                .map(|sid| derive_transcript_path(home, project_root, sid))
                .filter(|p| is_safe_transcript_path(p, home))
        })
}

/// Main-arm dispatcher. Returns `(value, exit_code)` where exit
/// code is always `0` per the FLOW business-error convention;
/// callers parse the JSON `status` field.
///
/// State-shape contract inside the `mutate_state` closure:
/// `resolve_transcript_path` returning `Some` proves the freshly-
/// re-read state is a JSON Object (the only Value variant where
/// `get(...)` yields `Some`). Direct `IndexMut` assignment on the
/// state root is therefore safe without a wrong-root-type guard.
pub fn run_impl_main(args: &Args, root: &Path, home: &Path) -> (Value, i32) {
    let paths = match FlowPaths::try_new(root, &args.branch) {
        Some(p) => p,
        None => {
            return (
                json!({
                    "status": "error",
                    "reason": "invalid_branch",
                    "message": format!("invalid branch name: {:?}", args.branch),
                }),
                0,
            );
        }
    };
    let state_path = paths.state_file();
    if !state_path.exists() {
        return (
            json!({
                "status": "skipped",
                "reason": "no_state_file",
            }),
            0,
        );
    }

    // Closure-captured failure value lets the mutate_state closure
    // signal an unauthorized or no-transcript-path early return to
    // the caller without short-circuiting the write window. The
    // closure leaves the state untouched on failure; mutate_state
    // re-writes the unmutated content, which is a safe no-op.
    let failure: RefCell<Option<Value>> = RefCell::new(None);
    let result = mutate_state(&state_path, &mut |st| {
        let transcript_path = match resolve_transcript_path(st, home, root) {
            Some(p) => p,
            None => {
                *failure.borrow_mut() = Some(json!({
                    "status": "error",
                    "reason": "no_transcript_path",
                    "message": "state has no usable session_id or transcript_path",
                }));
                return;
            }
        };
        if !last_user_message_invokes_skill(&transcript_path, CONTINUE_SKILL, home) {
            *failure.borrow_mut() = Some(json!({
                "status": "error",
                "reason": "unauthorized",
                "message": "transcript shows the most recent user turn did not invoke /flow:flow-continue; expected content to start with either <command-message>flow:flow-continue</command-message>\\n<command-name>/flow:flow-continue</command-name> (Claude Code 2.1.140+) or <command-name>/flow:flow-continue</command-name>",
            }));
            return;
        }
        st["_halt_pending"] = json!(false);
    });
    if let Some(v) = failure.into_inner() {
        return (v, 0);
    }
    match result {
        Ok(_) => (json!({"status": "ok"}), 0),
        Err(e) => (
            json!({
                "status": "error",
                "reason": "state_write_failed",
                "message": format!("failed to clear halt: {}", e),
            }),
            0,
        ),
    }
}
