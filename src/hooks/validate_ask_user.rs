//! PreToolUse hook for AskUserQuestion — enforces autonomous-phase discipline.
//!
//! Three outcomes, evaluated in order:
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
//! 2. **Auto-answer** (exit 0, JSON on stdout) — when `_auto_continue`
//!    is set and the block did not fire. Answers the AskUserQuestion
//!    with the successor skill command so phase transitions advance
//!    even if the skill's HARD-GATE was ignored.
//! 3. **Allow** (exit 0, no stdout) — otherwise. The tool call passes
//!    through to Claude Code's normal permission system.

use std::path::Path;

use serde_json::{json, Value};

use super::read_hook_input;
use crate::flow_paths::FlowPaths;
use crate::git::{current_branch, project_root};
use crate::lock::mutate_state;
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
    // with Code Review=auto in the Recommended preset) would deadlock: the
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

/// Decision produced by `run_impl_main` — translates to exit code +
/// side effect in `run()`.
#[derive(Debug)]
enum HookAction {
    /// Exit 0, no side effects. Used when the hook cannot resolve a
    /// state file (no stdin input, no branch, slash branch).
    Allow,
    /// Exit 2, stderr message. Autonomous-phase block.
    Block(String),
    /// Exit 0, stdout JSON answer. `_auto_continue` auto-answer.
    AutoAnswer(Value),
    /// Exit 0, call `set_blocked` on the given state path.
    AllowWithMark(std::path::PathBuf),
}

/// Pure decision core for the validate-ask-user hook. Accepts the
/// parsed stdin payload, current git branch, and project root as
/// parameters. Called from `run()` with live inputs; integration
/// tests drive the decision tree by spawning the hook subprocess
/// with controlled state fixtures.
fn run_impl_main(
    hook_input: Option<Value>,
    branch: Option<String>,
    project_root: &Path,
) -> HookAction {
    // No stdin input — nothing to gate on.
    if hook_input.is_none() {
        return HookAction::Allow;
    }

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

    let (allowed, message, hook_response) = validate(Some(&state_path));
    // Block path: `set_blocked` is intentionally not called — the
    // hook refused the tool call at the gate, so there is no
    // "blocked-while-executing" timestamp to record. `_blocked` is
    // only written when an AskUserQuestion was actually delivered.
    if !allowed {
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
    match run_impl_main(hook_input, branch, &root) {
        HookAction::Allow => std::process::exit(0),
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
            std::process::exit(0);
        }
    }
}
