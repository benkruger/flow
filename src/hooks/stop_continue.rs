//! Stop hook composed of three predicates that may refuse a turn-end.
//!
//! `run()` evaluates them in order:
//!
//! 1. `check_in_progress_utility_skill` — when a multi-step utility
//!    skill (`flow:flow-plan`) wrote a per-session marker at
//!    `<home>/.claude/flow/utility-in-progress-<id>.json` AND
//!    `decompose:decompose` is the most recent Skill `tool_use` in
//!    the persisted transcript since the most recent real user
//!    turn, refuses turn-end so the model continues from
//!    decompose's return straight to filing.
//! 2. `check_continue` — on subsequent stops, forces continuation
//!    when `_continue_pending=<skill_name>` is set, supporting
//!    multi-child-skill chains.
//! 3. `check_autonomous_stop` — the unified autonomous-mode gate.
//!    Three rules:
//!    - **Rule 1 (no halt, no user message, autonomous phase
//!      in-progress).** Refuse a voluntary turn-end with the
//!      encouraging "Stop Refused" message. The autonomous flow
//!      must keep going.
//!    - **Rule 2 (`_halt_pending=true`, no new user message).**
//!      Refuse with a message naming `/flow:flow-continue` and
//!      `/flow:flow-abort` as the only exits. Persists across
//!      every subsequent Stop until the user types
//!      `/flow:flow-continue` to clear the halt.
//!    - **Conversation pass-through (user message detected since
//!      the last Skill action).** Set `_halt_pending=true` and
//!      ALLOW the Stop so the model can answer the user. The next
//!      Stop without a new user message blocks under Rule 2.
//!
//!    See `.claude/rules/autonomous-phase-discipline.md` "Explicit
//!    User Pause Directives" for the design.
//!
//! Fail-open with error reporting: any error allows the stop (exit 0,
//! no block output), but writes a diagnostic to stderr and attempts to
//! log to `.flow-states/<branch>.log` for post-mortem visibility.

use std::fs;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::commands::clear_blocked::clear_blocked;
use crate::commands::set_blocked::set_blocked;
use crate::flow_paths::FlowPaths;
use crate::git::{project_root, resolve_branch};
use crate::github::detect_repo;
use crate::hooks::transcript_walker::is_truthy;
use crate::lock::mutate_state;
use crate::phase_config::find_state_files;
use crate::utils::{now, tolerant_i64, write_tab_sequences};

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
        // `branch` was extracted via `derive_root_branch` from a
        // state-file path's directory `file_name()` — a single
        // path component that cannot contain `/` by OS-level
        // invariants. The boundary is structurally provable, so
        // `.expect()` documents the guarantee without introducing
        // a reachable panic. Per
        // `.claude/rules/external-input-validation.md` "Hook
        // callsite discipline", the pattern-match default is the
        // safe choice, with the structurally-provable carve-out
        // documented there.
        let log_path = FlowPaths::try_new(root, branch)
            .expect("branch is a path-component file_name — no slashes possible")
            .log_file();
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
    let branch = branch_dir
        .and_then(|d| d.file_name())
        .and_then(|n| n.to_str());
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

/// Write the repo color to the terminal tab via /dev/tty.
///
/// Wraps `write_tab_sequences` with root/branch-aware fallback logic:
/// if the branch state file exists use its contents, otherwise scan for
/// any active feature state, otherwise call with just the detected repo.
///
/// The `Result` from `write_tab_sequences` is discarded: tty write
/// failures are environmental (no controlling terminal, /dev/tty
/// unavailable) and the function is best-effort visual feedback,
/// not a correctness gate. Mirrors `commands::session_context`'s
/// `let _ = write_tab_sequences(...)` pattern.
pub fn set_tab_color(root: &Path, branch: &str, state_path: &Path) {
    let _ = if state_path.exists() {
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
}

/// Block reason for Rule 1 (no halt, no user message, autonomous
/// phase in-progress). Same encouraging string `check_in_progress_utility_skill`
/// uses for the decompose-return shape — the autonomous flow must
/// keep going.
pub const RULE_1_STOP_REFUSED_MESSAGE: &str =
    "Stop Refused: Continue, you can do it. Don't give up, you got this! No excuses!";

/// Threshold for swapping the encouraging Rule 1 message for the
/// pointed message. After this many consecutive Stops in
/// in-progress autonomous flow-code without advancing the
/// `code_task` counter, the refusal text changes to one that names
/// the stalling pattern. N=3 leaves headroom for legitimate
/// multi-turn investigation while catching the loop pattern. See
/// `.claude/rules/autonomous-phase-discipline.md` "Forbidden
/// Stalling Frames".
pub const CONSECUTIVE_UNCHANGED_THRESHOLD: i64 = 3;

/// State field that records the `code_task` value observed at the
/// most recent Stop event in autonomous flow-code. Compared on the
/// next Stop to detect whether the model advanced the plan task.
/// Hook-managed — see `MODEL_DENIED_FIELDS` in
/// `src/commands/set_timestamp.rs` for the CLI write deny.
pub const LAST_OBSERVED_CODE_TASK_FIELD: &str = "_last_observed_code_task";

/// State field that records how many consecutive Stops in
/// autonomous flow-code have observed an unchanged `code_task`.
/// Hook-managed — paired with `LAST_OBSERVED_CODE_TASK_FIELD`.
pub const CONSECUTIVE_UNCHANGED_COUNT_FIELD: &str = "_consecutive_unchanged_count";

/// Pointed Rule 1 refusal text. Fired by `check_autonomous_stop`
/// when the autonomous flow-code phase has produced
/// `CONSECUTIVE_UNCHANGED_THRESHOLD` or more consecutive Stops
/// without advancing the `code_task` counter. Names the stalling
/// pattern explicitly so the model receives a sharper signal than
/// the generic encouraging text. See
/// `.claude/rules/autonomous-phase-discipline.md` "Forbidden
/// Stalling Frames".
pub const RULE_1_STOP_REFUSED_POINTED_MESSAGE: &str = "\
Stop Refused: you have ended several turns in a row without advancing
the plan task counter. This is the autonomous-stalling pattern.

If you are still investigating, NAME the specific blocker that
prevents you from producing an Edit or Write right now — what file,
what unknown, what concrete dependency. Then resolve it on this turn.

If you are not investigating, your next turn MUST produce a tool call
that advances the plan task: a Read or Grep that reads new context,
followed by an Edit or Write that lands code, followed by
`bin/flow set-timestamp --set code_task=<n+1>` when the task lands.

No rule in this project authorizes a halt in autonomous mode. Larger
files are not harder; deliberation text without tool calls is a
turn-end with no work. Keep going.";

/// Block reason for Rule 2 (`_halt_pending=true`, no new user
/// message). Names `/flow:flow-continue` and `/flow:flow-abort` as
/// the only exits so the model holds position until the user picks
/// one. The message persists across every subsequent Stop event
/// until the user types `/flow:flow-continue` to clear the halt.
pub const RULE_2_HALT_PENDING_MESSAGE: &str = "\
A user halt is pending in an autonomous FLOW phase. The autonomous
flow paused because the user typed a message mid-flow.

The only ways out:

1. The user types `/flow:flow-continue` to resume autonomous execution.
2. The user types `/flow:flow-abort` to abandon the flow.

Hold position. Do NOT advance the parent skill, do NOT self-invoke,
do NOT proceed to the next task. Answer the user's most recent
message if it requires a reply, then wait.

See .claude/rules/autonomous-phase-discipline.md \"Explicit User
Pause Directives\" for the design.";

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

/// State file size cap for the direct read in
/// `check_autonomous_stop`. The state file is FLOW-managed and
/// branch-scoped, but a corrupted or hostile state file could grow
/// without bound (account-window snapshots, findings array, log
/// entries) and an unbounded read at every Stop event would scale
/// O(turns × file_size). 4 MB is comfortably above the largest
/// observed legitimate state file and bounds adversarial input.
pub const STATE_FILE_BYTE_CAP: u64 = 4 * 1024 * 1024;

/// Normalize a state-file string before comparing in a gate per
/// `.claude/rules/security-gates.md` "Normalize Before Comparing":
/// strip embedded NULs (defeat-byte-equality from truncated writes),
/// trim whitespace (state-file padding, hand edits), and ASCII-
/// lowercase (case-insensitive intent across `auto`/`Auto`/`AUTO`).
fn normalize_gate_input(s: &str) -> String {
    s.replace('\0', "").trim().to_ascii_lowercase()
}

/// Unified autonomous-mode Stop gate. Three rules govern the Stop
/// event during an in-progress autonomous phase:
///
/// 1. **Conversation pass-through.** When the persisted transcript
///    shows a real user message since the model's most recent Skill
///    action, set `_halt_pending=true` and ALLOW the Stop so the
///    model can answer the user. On the next Stop event without a
///    new user message, Rule 2 fires and blocks.
/// 2. **Rule 2: halt pending, no new user message.** Refuse the
///    Stop with `RULE_2_HALT_PENDING_MESSAGE`. The block persists
///    across every subsequent Stop until the user types
///    `/flow:flow-continue` (which invokes `bin/flow clear-halt` to
///    set `_halt_pending=false`).
/// 3. **Rule 1: no halt, no new user message, autonomous phase.**
///    Refuse the Stop with `RULE_1_STOP_REFUSED_MESSAGE` by default,
///    OR with `RULE_1_STOP_REFUSED_POINTED_MESSAGE` when the model
///    has produced `CONSECUTIVE_UNCHANGED_THRESHOLD` consecutive
///    Stops in autonomous flow-code without advancing the
///    `code_task` counter. The pointed text names the stalling
///    pattern explicitly. The monotonic-+1 validator on `code_task`
///    (`src/commands/set_timestamp.rs::validate_code_task`) plus
///    the `MODEL_DENIED_FIELDS` deny on the counter pair ensure the
///    only way to reset the count is to legitimately advance the
///    plan task. See `.claude/rules/code-task-counter.md` and
///    `.claude/rules/autonomous-phase-discipline.md` "Forbidden
///    Stalling Frames".
///
/// When the phase is not in-progress OR not autonomous, the
/// predicate clears any stale `_halt_pending=true` left over from a
/// prior phase and returns non-blocking so the cascade falls
/// through.
///
/// `_continue_pending` is never touched here. The cascade's multi-
/// child-skill chain (`check_continue`) owns that field.
///
/// Hook-state read timing per `.claude/rules/hook-state-timing.md`:
///
/// - **Field reads.** `current_phase`, `phases.<N>.status`,
///   `skills.<N>`, `_halt_pending` are all written before the Stop
///   event fires. The `current_phase == "<x>"` plus
///   `phases.<x>.status == "in_progress"` pair confines the guard to
///   actively-executing autonomous windows.
/// - **Writer.** This predicate is the sole writer of `_halt_pending`
///   in the Stop path; `bin/flow clear-halt` (invoked by
///   `/flow:flow-continue`) is the only other writer, and
///   `phase_complete()` clears the field on phase advance.
///
/// Fail-open on every error class. Transcript-path validation is
/// performed inside `most_recent_user_message_since_skill_action`
/// via `is_safe_transcript_path`.
pub fn check_autonomous_stop(
    state_path: &Path,
    transcript_path: Option<&str>,
    home: &Path,
) -> ContinueResult {
    let no_block = || ContinueResult {
        should_block: false,
        skill: None,
        context: None,
    };
    if !state_path.exists() {
        return no_block();
    }

    // Read the transcript outside the state-file lock — the transcript
    // file is unrelated and reading it inside mutate_state would hold
    // the state lock during unrelated I/O.
    let user_msg = match transcript_path {
        Some(p) if !p.is_empty() => {
            crate::hooks::transcript_walker::most_recent_user_message_since_skill_action(
                Path::new(p),
                home,
            )
        }
        _ => None,
    };

    let mut should_block = false;
    let mut halt_set = false;
    let mut pointed_text_required = false;
    let _ = mutate_state(state_path, &mut |state| {
        if !(state.is_object() || state.is_null()) {
            return;
        }
        let current_phase = state
            .get("current_phase")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if current_phase.is_empty() {
            return;
        }
        let phase_status = state
            .get("phases")
            .and_then(|p| p.get(&current_phase))
            .and_then(|p| p.get("status"))
            .and_then(|v| v.as_str())
            .map(normalize_gate_input)
            .unwrap_or_default();
        let skill_entry = state.get("skills").and_then(|s| s.get(&current_phase));
        let is_auto = match skill_entry {
            Some(v) if v.as_str().map(normalize_gate_input).as_deref() == Some("auto") => true,
            Some(v) => {
                v.get("continue")
                    .and_then(|c| c.as_str())
                    .map(normalize_gate_input)
                    .as_deref()
                    == Some("auto")
            }
            None => false,
        };
        // Read via the tolerant `is_truthy` predicate so the writer
        // (this predicate) and the readers (validate-skill and
        // validate-pretool halt gates) agree on what "_halt_pending
        // is set" means. A state-derived field could carry truthy
        // shapes other than bool true — string "true", string "1",
        // non-zero number — and using raw `.as_bool()` here while
        // the readers use `is_truthy` produced divergent halt-state
        // classification across the three hook surfaces. Per
        // `.claude/rules/security-gates.md` "Normalize Before
        // Comparing", state-derived input must pass through a
        // single shared normalization predicate.
        let halt_was_set = is_truthy(state.get("_halt_pending"));
        if phase_status != "in_progress" || !is_auto {
            // Phase not auto+in-progress: clear stale halt, allow stop.
            if halt_was_set {
                state["_halt_pending"] = json!(false);
            }
            return;
        }
        match user_msg.as_deref() {
            Some(_) => {
                // Rule 1 (pass-through): user typed since the model's
                // most recent Skill action — set halt and allow Stop so
                // the model can answer. The next Stop without a new
                // user message blocks under Rule 2.
                state["_halt_pending"] = json!(true);
                halt_set = true;
            }
            None => {
                // No user message since the most recent Skill action.
                // Either Rule 2 (halt pending) or Rule 1 (encouraging
                // OR pointed) — all block, message selected below.
                should_block = true;
                halt_set = halt_was_set;
                // Counter-tracking for the autonomous-mode stalling
                // pattern (see `.claude/rules/autonomous-phase-discipline.md`
                // "Forbidden Stalling Frames"). Scope: flow-code only
                // (other autonomous phases have no `code_task`
                // analog) AND only when Rule 1 would fire (no halt
                // set yet). The monotonic-+1 validator on `code_task`
                // (`src/commands/set_timestamp.rs::validate_code_task`)
                // ensures the only way to reset the count is to
                // legitimately advance the task — the model cannot
                // counterfeit the reset because the counter fields
                // themselves are in `MODEL_DENIED_FIELDS`.
                //
                // `current_phase` is normalized before the scope
                // comparison so case-variant or whitespace-padded
                // state-file values (hand edits, interrupted writes,
                // schema drift) still match "flow-code". The sibling
                // `phase_status` and `is_auto` checks above already
                // normalize their inputs per
                // `.claude/rules/security-gates.md` "Normalize Before
                // Comparing"; this comparison joins them.
                if !halt_was_set && normalize_gate_input(&current_phase) == "flow-code" {
                    let code_task = tolerant_i64(state.get("code_task").unwrap_or(&Value::Null));
                    let last_observed_raw = state.get(LAST_OBSERVED_CODE_TASK_FIELD).cloned();
                    let prev_count = tolerant_i64(
                        state
                            .get(CONSECUTIVE_UNCHANGED_COUNT_FIELD)
                            .unwrap_or(&Value::Null),
                    );
                    let new_count = match last_observed_raw.as_ref() {
                        // Initial observation: field absent or null.
                        // Initialize without firing the pointed text
                        // on the first Stop a flow-code window sees.
                        None | Some(Value::Null) => 0,
                        // `code_task` advanced since the last Stop:
                        // reset the count.
                        Some(v) if tolerant_i64(v) != code_task => 0,
                        // `code_task` unchanged: increment with
                        // saturating arithmetic per
                        // `.claude/rules/rust-patterns.md` "Saturating
                        // Arithmetic on Counter Reads".
                        _ => prev_count.saturating_add(1),
                    };
                    state[LAST_OBSERVED_CODE_TASK_FIELD] = json!(code_task);
                    state[CONSECUTIVE_UNCHANGED_COUNT_FIELD] = json!(new_count);
                    pointed_text_required = new_count >= CONSECUTIVE_UNCHANGED_THRESHOLD;
                }
            }
        }
    });

    if !should_block {
        return no_block();
    }
    let context = if halt_set {
        RULE_2_HALT_PENDING_MESSAGE.to_string()
    } else if pointed_text_required {
        RULE_1_STOP_REFUSED_POINTED_MESSAGE.to_string()
    } else {
        RULE_1_STOP_REFUSED_MESSAGE.to_string()
    };
    ContinueResult {
        should_block: true,
        skill: Some("autonomous-stop-refused".to_string()),
        context: Some(context),
    }
}

/// Refuse a voluntary turn-end when a multi-step utility skill's
/// `decompose:decompose` sub-skill has just returned in the current
/// model turn.
///
/// Multi-step utility skills (`flow:flow-plan`) invoke
/// `decompose:decompose` via the Skill tool mid-pipeline. The
/// Skill tool's return is a
/// structural surface where the model treats the handoff as a
/// natural stopping point and returns control to the user — breaking
/// the unattended-flow contract these skills promise. This
/// predicate's job is to catch THAT specific shape: marker present
/// AND `decompose:decompose` is the most recent Skill call since
/// the user typed.
///
/// **Two-signal gate.** The block decision requires BOTH:
///
/// 1. The per-session utility marker file at
///    `<home>/.claude/flow/utility-in-progress-<session_id>.json`
///    exists with matching skill name and session_id (precondition).
/// 2. `crate::hooks::transcript_walker::most_recent_skill_since_user`
///    returns `Some("decompose:decompose")` for the supplied
///    `transcript_path` (the discriminator).
///
/// The transcript walker discriminates "decompose just returned
/// mid-pipeline" (block) from "model just sent a normal
/// conversational reply" (no block). Without the discriminator,
/// every reply during discussion mode would refuse turn-end and the
/// discussion-mode contract these skills offer would not hold.
///
/// **Last-Skill-wins semantics.** When a planning-persona sub-agent
/// (`flow:pm`, `flow:tech-lead`, `flow:cto`) is invoked AFTER
/// `decompose:decompose` in the same window, the walker returns the
/// most recent Skill name and the gate falls through to no_block.
/// The user reacts in the next message and discussion continues.
///
/// Hook-state read timing per `.claude/rules/hook-state-timing.md`:
///
/// - **Field read.** Marker file existence + JSON `skill` and
///   `session_id`. Transcript file content via
///   `most_recent_skill_since_user`.
/// - **Writer.** `bin/flow set-utility-in-progress` writes the
///   marker; Claude Code writes transcript turns as the session
///   advances.
/// - **Clearer.** `bin/flow clear-utility-in-progress` removes the
///   marker at the skill's COMPLETE banner. The transcript is not
///   cleared — older turns remain visible but the walker stops at
///   the most recent real user turn.
/// - **Read window.** Every Stop event during the skill lifecycle.
///   The gate fires on every iteration of a decompose-then-file
///   loop because each iteration produces a new decompose call as
///   the most recent Skill in the transcript.
///
/// Composed into `run()` AFTER `check_continue` and BEFORE
/// `check_autonomous_stop`.
///
/// **Block message.** When the gate fires, the context is the exact
/// encouraging string. The verbatim-context branch in `run()` routes
/// the string into the `decision: "block"` envelope's `reason` field
/// unchanged — no "child skill returned" wrapper, no rule citations,
/// no abort instructions.
///
/// Symlink-safe per `.claude/rules/rust-patterns.md`. Fail-open on
/// every error class: empty/invalid session_id, missing marker,
/// symlinked marker, unparseable marker JSON, marker naming a skill
/// outside the allowlist, missing/invalid transcript path, no Skill
/// call since user, most-recent Skill is not decompose. The Stop
/// hook must never panic — every error path returns no_block.
pub fn check_in_progress_utility_skill(
    session_id: &str,
    transcript_path: Option<&str>,
    home: &Path,
) -> ContinueResult {
    let no_block = || ContinueResult {
        should_block: false,
        skill: None,
        context: None,
    };
    if session_id.is_empty() {
        return no_block();
    }
    let path = match crate::commands::utility_marker::marker_path(home, session_id) {
        Some(p) => p,
        None => return no_block(),
    };
    // Symlink-safe existence check: `symlink_metadata` does NOT
    // follow symlinks. Reject both missing entries and symlinks
    // pointing outside `<home>/.claude/flow/`. A regular file at
    // the marker path passes through and is read below.
    let meta = match fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(_) => return no_block(),
    };
    if meta.file_type().is_symlink() || !meta.file_type().is_file() {
        return no_block();
    }
    // Bound the marker read with the same byte cap as the state file
    // so a corrupted or hostile marker cannot OOM the hook on Stop.
    let marker: Value = match File::open(&path).ok().and_then(|f| {
        let mut buf = String::new();
        let _ = BufReader::new(f.take(STATE_FILE_BYTE_CAP)).read_to_string(&mut buf);
        serde_json::from_str::<Value>(&buf).ok()
    }) {
        Some(v) => v,
        None => return no_block(),
    };
    // Normalize the marker `skill` field per `.claude/rules/security-gates.md`
    // "Normalize Before Comparing": strip NULs, trim whitespace, lowercase
    // ASCII. The marker file is state-derived (hand-editable JSON) so a
    // whitespace-padded, NUL-tainted, or uppercase value must still match
    // the allowlist of canonical lowercase skill names.
    let skill_raw = marker.get("skill").and_then(|v| v.as_str()).unwrap_or("");
    let skill_norm = skill_raw.replace('\0', "").trim().to_ascii_lowercase();
    let marker_session = marker
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if marker_session != session_id {
        return no_block();
    }
    if !crate::commands::utility_marker::MULTI_STEP_UTILITY_SKILLS.contains(&skill_norm.as_str()) {
        return no_block();
    }
    // Marker precondition satisfied. Now check the discriminator:
    // the most recent Skill call since the user typed must be
    // `decompose:decompose`. Without a transcript_path, the walker
    // cannot run and the predicate fails-open — a normal reply must
    // not block.
    let transcript_path = match transcript_path {
        Some(p) if !p.is_empty() => Path::new(p),
        _ => return no_block(),
    };
    let most_recent =
        crate::hooks::transcript_walker::most_recent_skill_since_user(transcript_path, home);
    if most_recent.as_deref() != Some("decompose:decompose") {
        return no_block();
    }
    ContinueResult {
        should_block: true,
        skill: Some("utility-in-progress".to_string()),
        context: Some(
            "Stop Refused: Continue, you can do it. Don't give up, you got this! No excuses!"
                .to_string(),
        ),
    }
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

    // Utility-skill marker guard: when a multi-step utility skill
    // (`flow:flow-plan`) is in progress AND `decompose:decompose`
    // is the most recent Skill
    // since the user turn, refuse turn-end so the model continues
    // past decompose's return. Composed FIRST because its block
    // message is the verbatim encouraging string for that specific
    // failure shape — wins over the generic autonomous-stop
    // refusal for the decompose-return boundary.
    let session_id = hook_input
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let transcript_path = hook_input.get("transcript_path").and_then(|v| v.as_str());
    let home = crate::session_metrics::home_dir_or_empty();
    let mut result = check_in_progress_utility_skill(session_id, transcript_path, &home);

    // Multi-child-skill chains: when a child skill returned and the
    // parent skill set `_continue_pending`, force continuation.
    if !result.should_block {
        result = check_continue(&hook_input, &state_path);
    }

    // Unified autonomous-mode Stop gate. Applies Rule 1 (Stop
    // refused), Rule 2 (halt pending), or conversation pass-through
    // (user typed → set halt, allow Stop) per
    // `.claude/rules/autonomous-phase-discipline.md` "Explicit User
    // Pause Directives".
    if !result.should_block {
        result = check_autonomous_stop(&state_path, transcript_path, &home);
    }

    capture_session_id(&hook_input, &state_path);

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
        // `autonomous-stop-refused` and `utility-in-progress` carry
        // the context string straight into the `decision: "block"`
        // envelope's reason — not the "child skill returned"
        // framing from format_block_output, which is designed for
        // multi-child-skill check_continue continuations.
        let output = if skill_name == "autonomous-stop-refused"
            || skill_name == "utility-in-progress"
        {
            json!({"decision": "block", "reason": result.context.as_deref().unwrap_or(RULE_1_STOP_REFUSED_MESSAGE)})
        } else {
            format_block_output(skill_name, result.context.as_deref())
        };
        println!("{}", serde_json::to_string(&output).unwrap());
    }
}
