//! PreToolUse hook for the Skill tool — Layer 1 of the user-only
//! skill enforcement chain. Blocks model invocations of the four
//! `USER_ONLY_SKILLS` (`flow:flow-abort`, `flow:flow-reset`,
//! `flow:flow-release`, `flow:flow-prime`) unless the most recent
//! user-role turn in the persisted transcript carries a matching
//! `<command-name>/<skill></command-name>` substring (i.e. the user
//! typed the slash command directly).
//!
//! Exit semantics:
//! - Exit 0, no stdout / stderr — allow (skill not user-only, or
//!   user-only and a matching user invocation found, or stdin
//!   missing / malformed)
//! - Exit 2, stderr message — block (skill is user-only and the
//!   transcript walker found no matching user invocation in the
//!   most recent user turn)
//!
//! Companion to `validate_ask_user`'s Layer 2 carve-out: when the
//! same Skill tool call would fire an `AskUserQuestion` for user
//! confirmation, the carve-out allows the prompt to fire even
//! during in-progress autonomous phases — resolving the
//! autonomous-deadlock the `--auto` bypass on `/flow:flow-abort`
//! and `/flow:flow-release` previously worked around.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::read_hook_input;
use crate::hooks::transcript_walker::{
    last_user_message_invokes_skill, normalize_gate_input, USER_ONLY_SKILLS,
};
use crate::window_snapshot::home_dir_or_empty;

/// Decide whether to allow or block a Skill tool invocation.
///
/// Returns `(allowed, message)`:
/// - `(true, "")` — allow the tool call (silent)
/// - `(false, msg)` — block; caller writes `msg` to stderr and exits 2
///
/// The `skill` field is normalized through `normalize_gate_input`
/// (NUL strip + trim + ASCII lowercase) before the membership check
/// per `.claude/rules/security-gates.md` "Normalize Before
/// Comparing", so case-variant (`flow:Flow-Abort`),
/// whitespace-padded (`"flow:flow-abort "`), and NUL-padded inputs
/// all match the canonical entries in `USER_ONLY_SKILLS`.
///
/// `tool_input` is the parsed JSON payload Claude Code passes for
/// the Skill tool — its `skill` field carries the name being
/// invoked. `transcript_path` is the persisted JSONL session log
/// (when present in the hook stdin); `home` is `$HOME` (passed in
/// rather than read from the env so tests can drive a tempdir
/// fixture without `set_var` env races per
/// `.claude/rules/testing-gotchas.md` "Rust Parallel Test Env Var
/// Races").
pub fn validate(tool_input: &Value, transcript_path: Option<&Path>, home: &Path) -> (bool, String) {
    let skill_raw = tool_input
        .get("skill")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let skill_norm = normalize_gate_input(skill_raw);
    if !USER_ONLY_SKILLS.contains(&skill_norm.as_str()) {
        return (true, String::new());
    }
    if let Some(path) = transcript_path {
        if last_user_message_invokes_skill(path, &skill_norm, home) {
            return (true, String::new());
        }
    }
    (
        false,
        format!(
            "BLOCKED: `{}` is a user-only skill. The model cannot invoke it. \
             Ask the user to type `/{}` directly. This skill performs a \
             destructive or initiating action that requires explicit user \
             intent — see .claude/rules/user-only-skills.md.",
            skill_norm, skill_norm,
        ),
    )
}

/// Pure decision core. Accepts the parsed stdin payload and `home`
/// as injected dependencies so unit tests drive every branch with a
/// `TempDir` fixture. Mirrors the `run_impl_main` pattern documented
/// in `.claude/rules/rust-patterns.md` "Main-arm dispatch."
///
/// Return contract:
/// - `(0, None)` → allow silently (exit 0, no stderr)
/// - `(2, Some(message))` → block (stderr the message, exit 2)
pub fn run_impl_main(hook_input: Option<Value>, home: &Path) -> (i32, Option<String>) {
    let hook_input = match hook_input {
        Some(v) => v,
        None => return (0, None),
    };
    let tool_input = hook_input
        .get("tool_input")
        .cloned()
        .unwrap_or(Value::Object(Default::default()));
    let transcript_path: Option<PathBuf> = hook_input
        .get("transcript_path")
        .and_then(|v| v.as_str())
        .map(PathBuf::from);
    let (allowed, msg) = validate(&tool_input, transcript_path.as_deref(), home);
    if !allowed {
        return (2, Some(msg));
    }
    (0, None)
}

/// Run the validate-skill hook (entry point from CLI). Reads stdin,
/// resolves `$HOME` via `home_dir_or_empty()`, calls
/// `run_impl_main`, writes any block message to stderr, and exits
/// with the returned code.
pub fn run() {
    let input = read_hook_input();
    let home = home_dir_or_empty();
    let (code, message) = run_impl_main(input, &home);
    if let Some(m) = message {
        eprintln!("{}", m);
    }
    std::process::exit(code);
}
