//! PreToolUse hook for AskUserQuestion — enforces autonomous-phase discipline.
//!
//! Six outcomes, evaluated in order:
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
//! 2. **User-only skill carve-out** — when `validate` would have
//!    blocked the prompt but the persisted transcript shows the most
//!    recent assistant Skill tool_use targets a skill in
//!    `crate::hooks::transcript_walker::USER_ONLY_SKILLS`
//!    (`flow:flow-abort`, `flow:flow-reset`, `flow-release`,
//!    `flow:flow-prime`, `flow:flow-continue`). Allows the
//!    confirmation prompt to fire so user-only skills'
//!    destructive-operation gates do not deadlock when invoked from
//!    inside an in-progress autonomous phase.
//! 3. **Shared-config carve-out** — when `validate` would have
//!    blocked the prompt but the most recent user-role turn in the
//!    persisted transcript carries a `validate_worktree_paths`
//!    shared-config edit block (a tool_result with `is_error: true`
//!    containing the literal substring "is a shared configuration
//!    file that affects every engineer"). The
//!    `validate_worktree_paths` BLOCKED message itself instructs the
//!    model to call AskUserQuestion to confirm — this carve-out lets
//!    that prompt fire instead of deadlocking. Backed by
//!    `crate::hooks::transcript_walker::recent_edit_blocked_on_shared_config`.
//!    See `.claude/rules/autonomous-phase-discipline.md`
//!    "Shared-Config Carve-Out" subsection.
//! 4. **Auto-answer** (exit 0, JSON on stdout) — when `_auto_continue`
//!    is set and the block did not fire. Answers the AskUserQuestion
//!    with the successor skill command so phase transitions advance
//!    even if the skill's HARD-GATE was ignored.
//! 5. **Allow** (exit 0, stdout: `{"permissionDecision":"defer"}`) —
//!    otherwise. The explicit defer signal tells Claude Code the hook
//!    has no opinion on this tool call, routing it through normal
//!    permission handling without relying on empty-stdout implicit
//!    semantics. Both the no-state-file `Allow` arm and the
//!    state-file-found `AllowWithMark` arm emit the same defer
//!    payload; the `AllowWithMark` arm additionally writes the
//!    `_blocked` timestamp to the state file before exiting.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::read_hook_input;
use crate::flow_paths::FlowPaths;
use crate::git::{current_branch, project_root};
use crate::hooks::transcript_walker::{
    most_recent_skill_in_user_only_set, recent_edit_blocked_on_shared_config,
};
use crate::lock::mutate_state;
use crate::session_metrics::home_dir_or_empty;
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
    // with Review=auto in the Recommended preset) would deadlock: the
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

/// Returns `true` when the most recent assistant tool_use Skill
/// invocation in the transcript at `transcript_path` targets a
/// skill in `crate::hooks::transcript_walker::USER_ONLY_SKILLS`.
/// Returns `false` when `transcript_path` is `None`, when the
/// validator rejects the path (not under `<home>/.claude/projects/`,
/// NUL-byte, relative), or when no Skill tool_use is found before
/// the most recent user turn.
///
/// Companion to `run_impl_main`: when validate would have blocked
/// the AskUserQuestion under autonomous-phase discipline and this
/// helper returns `true`, the block is suppressed so user-only
/// skills can fire their confirmation prompts mid-autonomous-flow.
pub fn user_only_skill_carve_out_applies(transcript_path: Option<&Path>, home: &Path) -> bool {
    match transcript_path {
        Some(p) => most_recent_skill_in_user_only_set(p, home),
        None => false,
    }
}

/// Decision produced by `run_impl_main` — translates to exit code +
/// side effect in `run()`.
#[derive(Debug)]
enum HookAction {
    /// Exit 0, emits `{"permissionDecision":"defer"}` on stdout, no
    /// state-file side effects. Used when the hook cannot resolve a
    /// state file (no stdin input, no branch, slash branch). The
    /// explicit defer signal tells Claude Code the hook has no
    /// opinion on this tool call, routing it through normal
    /// permission handling without relying on empty-stdout implicit
    /// semantics.
    Allow,
    /// Exit 2, stderr message. Autonomous-phase block.
    Block(String),
    /// Exit 0, stdout JSON answer. `_auto_continue` auto-answer.
    AutoAnswer(Value),
    /// Exit 0, emits `{"permissionDecision":"defer"}` on stdout, and
    /// calls `set_blocked` on the given state path so the state file
    /// records that an AskUserQuestion was delivered. Used when the
    /// hook resolved a state file and `validate` returned allow with
    /// no auto-continue (or when a carve-out suppressed an autonomous-
    /// phase block). The defer payload matches the `Allow` arm so
    /// every "no opinion" outcome surfaces the same explicit signal
    /// to Claude Code.
    AllowWithMark(std::path::PathBuf),
}

/// Pure decision core for the validate-ask-user hook. Accepts the
/// parsed stdin payload, current git branch, project root, and
/// `$HOME` as parameters. Called from `run()` with live inputs;
/// integration tests drive the decision tree by spawning the hook
/// subprocess with controlled state fixtures.
fn run_impl_main(
    hook_input: Option<Value>,
    branch: Option<String>,
    project_root: &Path,
    home: &Path,
) -> HookAction {
    // No stdin input — nothing to gate on.
    let input = match hook_input {
        Some(v) => v,
        None => return HookAction::Allow,
    };

    let branch = match branch {
        Some(b) => b,
        None => return HookAction::Allow,
    };

    // Slash-containing git branches are not valid FLOW branches —
    // treat as "no active flow" and exit 0 rather than panicking.
    let state_path = match FlowPaths::try_new(project_root, &branch) {
        Some(p) => p.state_file(),
        None => return HookAction::Allow,
    };

    let transcript_path: Option<PathBuf> = input
        .get("transcript_path")
        .and_then(|v| v.as_str())
        .map(PathBuf::from);

    let (allowed, message, hook_response) = validate(Some(&state_path));
    if !allowed {
        // Carve-out: user-only skills' confirmation prompts fire
        // even during in_progress + auto. Only fires when the
        // transcript walker confirms the most recent assistant
        // Skill tool_use call targets a user-only skill — meaning
        // the user just typed the slash command and the resulting
        // confirmation prompt is part of that user-initiated flow.
        if user_only_skill_carve_out_applies(transcript_path.as_deref(), home) {
            return HookAction::AllowWithMark(state_path);
        }
        // Carve-out: shared-config edits that
        // `validate_worktree_paths` blocked instruct the model to
        // call AskUserQuestion to confirm. Letting the prompt
        // fire — instead of deadlocking against the autonomous
        // block — completes the system-initiated confirmation flow
        // that the prior hook explicitly demanded.
        //
        // Ordering: the user-only-skill carve-out is checked first.
        // If both conditions apply (the user typed a user-only
        // slash command AND a shared-config block is in the
        // transcript), the user-only branch fires first. The
        // ordering is locked by the regression test
        // `both_carve_outs_can_apply_user_only_wins_first` — both
        // branches produce the same `AllowWithMark` outcome so the
        // order is observationally equivalent today, but a future
        // refactor that diverges the branches must preserve the
        // ordering.
        let allow_shared_config = transcript_path
            .as_deref()
            .map(|p| recent_edit_blocked_on_shared_config(p, home))
            .unwrap_or(false);
        if allow_shared_config {
            return HookAction::AllowWithMark(state_path);
        }
        // `set_blocked` is intentionally not called — the hook
        // refused the tool call at the gate, so there is no
        // "blocked-while-executing" timestamp to record. `_blocked`
        // is only written when an AskUserQuestion was actually
        // delivered.
        return HookAction::Block(message);
    }
    if let Some(response) = hook_response {
        return HookAction::AutoAnswer(response);
    }
    HookAction::AllowWithMark(state_path)
}

/// Run the validate-ask-user hook (entry point from CLI). Translates
/// `run_impl_main` decisions into exit codes and stdio side effects.
pub fn run() {
    let hook_input = read_hook_input();
    let branch = current_branch();
    let root = project_root();
    let home = home_dir_or_empty();
    match run_impl_main(hook_input, branch, &root, &home) {
        HookAction::Allow => {
            println!("{}", json!({"permissionDecision": "defer"}));
            std::process::exit(0);
        }
        HookAction::Block(msg) => {
            eprintln!("{}", msg);
            std::process::exit(2);
        }
        HookAction::AutoAnswer(resp) => {
            println!("{}", serde_json::to_string(&resp).unwrap());
            std::process::exit(0);
        }
        HookAction::AllowWithMark(state_path) => {
            set_blocked(&state_path);
            println!("{}", json!({"permissionDecision": "defer"}));
            std::process::exit(0);
        }
    }
}
