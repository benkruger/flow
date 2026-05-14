//! PreToolUse hook for the Skill tool — Layer 1 of the user-only
//! skill enforcement chain AND the halt gate for autonomous flows.
//!
//! Two gates fire in order on every Skill tool call:
//!
//! 1. **User-only-skill gate.** Blocks model invocations of any
//!    skill in `USER_ONLY_SKILLS` (`flow:flow-abort`,
//!    `flow:flow-reset`, `flow:flow-release`, `flow:flow-prime`,
//!    `flow:flow-continue`) unless the most recent user-role turn
//!    in the persisted transcript STARTS with one of the two
//!    emission shapes Claude Code uses for user-typed slash
//!    commands: the two-line
//!    `<command-message><skill></command-message>\n<command-name>/<skill></command-name>`
//!    (Claude Code 2.1.140+) or the legacy
//!    `<command-name>/<skill></command-name>`. The walker accepts
//!    either via `starts_with` disjunction so the user-typed slash
//!    command is recognized across emission-shape revisions.
//! 2. **Halt gate.** When the state file has `_halt_pending=true`
//!    (set by `stop_continue::check_autonomous_stop` after the user
//!    typed a message during an autonomous flow), every Skill call
//!    is blocked except the user-only-skill exits. The block
//!    message names `/flow:flow-continue` (resume) and
//!    `/flow:flow-abort` (give up) — the only sanctioned exits from
//!    the halt window. The user-only check runs FIRST so an exit
//!    invocation passes through cleanly when the user typed it.
//!
//! Exit semantics:
//! - Exit 0, no stdout / stderr — allow (skill not user-only and
//!   halt not set, OR user-only with matching user invocation, OR
//!   stdin missing / malformed)
//! - Exit 2, stderr message — block (user-only without matching
//!   invocation, OR halt set on a non-user-only skill)
//!
//! Companion to `validate_ask_user`'s Layer 2 carve-out: when the
//! same Skill tool call would fire an `AskUserQuestion` for user
//! confirmation, the carve-out allows the prompt to fire even
//! during in-progress autonomous phases — resolving the
//! autonomous-deadlock the `--auto` bypass on `/flow:flow-abort`
//! and `/flow:flow-release` previously worked around.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::read_hook_input;
use crate::flow_paths::FlowPaths;
use crate::hooks::stop_continue::STATE_FILE_BYTE_CAP;
use crate::hooks::transcript_walker::{
    is_truthy, last_user_message_invokes_skill, normalize_gate_input, USER_ONLY_SKILLS,
};
use crate::hooks::{detect_branch_from_path, resolve_main_root};
use crate::session_metrics::home_dir_or_empty;

/// Read the state file at `state_path` and return `true` when
/// `_halt_pending` is truthy. Truthy = boolean `true`, the strings
/// `"true"` / `"1"` (case-insensitive), or any non-zero number per
/// `.claude/rules/rust-patterns.md` "Hook Input Boolean Field
/// Tolerance" — the JSON root may not contractually be bool when
/// state files are hand-edited or written by older versions.
///
/// Reads are bounded at `STATE_FILE_BYTE_CAP` per
/// `.claude/rules/external-input-path-construction.md` so a
/// corrupted or hostile state file cannot OOM the hook. Every
/// error class (missing file, oversized file, non-JSON content,
/// non-object root, missing field) returns `false`. Fail-open is
/// the correct posture: the halt gate's purpose is to refuse
/// model-initiated work during the halt window; a missing or
/// corrupt state file means no flow is halted.
fn read_halt_pending(state_path: &Path) -> bool {
    let f = match File::open(state_path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    // Ignore mid-read errors: a partial read produces a (possibly
    // empty) buffer that fails JSON parsing, which the chain below
    // collapses into `false`. Coalescing read-error and parse-error
    // arms into one short-circuit keeps the function reachable from
    // a single test surface (non-JSON content) per
    // `.claude/rules/testability-means-simplicity.md`.
    let mut buf = String::new();
    let _ = BufReader::new(f.take(STATE_FILE_BYTE_CAP)).read_to_string(&mut buf);
    serde_json::from_str::<Value>(&buf)
        .ok()
        .map(|v| is_truthy(v.get("_halt_pending")))
        .unwrap_or(false)
}

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
pub fn validate(
    tool_input: &Value,
    transcript_path: Option<&Path>,
    state_path: Option<&Path>,
    home: &Path,
) -> (bool, String) {
    let skill_raw = tool_input
        .get("skill")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let skill_norm = normalize_gate_input(skill_raw);
    let user_typed_skill = transcript_path
        .map(|p| last_user_message_invokes_skill(p, &skill_norm, home))
        .unwrap_or(false);
    // Layer 1 — user-only-skill gate. When the skill is user-only,
    // require the user to have typed the matching slash command in
    // the most recent user turn. The user-only allow path runs
    // BEFORE the halt gate so a user-typed `/flow:flow-continue`
    // exits the halt window cleanly.
    let is_user_only = USER_ONLY_SKILLS.contains(&skill_norm.as_str());
    if is_user_only && user_typed_skill {
        return (true, String::new());
    }
    if is_user_only {
        return (
            false,
            format!(
                "BLOCKED: `{}` is a user-only skill. The model cannot invoke it. \
                 Ask the user to type `/{}` directly. This skill performs a \
                 destructive or initiating action that requires explicit user \
                 intent — see .claude/rules/user-only-skills.md.",
                skill_norm, skill_norm,
            ),
        );
    }
    // Layer 2 — halt gate. A non-user-only skill (any skill the
    // model can invoke under normal autonomous-flow rules) is
    // blocked while `_halt_pending=true` in the state file. The
    // user must invoke `/flow:flow-continue` (clears halt and
    // resumes) or `/flow:flow-abort` (closes the flow) — those
    // user-only skills pass the gate above when typed.
    let halt_set = state_path.map(read_halt_pending).unwrap_or(false);
    if halt_set {
        return (
            false,
            "BLOCKED: this flow is halted. The autonomous flow paused after a user \
             message and stays paused until the user explicitly resumes or aborts. \
             The model cannot invoke `Skill` while halted. Two exits are \
             available — only the user can take them: type `/flow:flow-continue` \
             to resume, or `/flow:flow-abort` to close the flow. \
             See .claude/rules/autonomous-phase-discipline.md."
                .to_string(),
        );
    }
    (true, String::new())
}

/// Pure decision core. Accepts the parsed stdin payload, `cwd` and
/// `home` as injected dependencies so unit tests drive every branch
/// with a `TempDir` fixture. `cwd` is optional so the wrapper can
/// pass `std::env::current_dir().ok()` without an untestable
/// fallback closure — an unresolvable cwd means the halt gate
/// cannot resolve the state file and silently passes (the
/// user-only-skill gate still runs). Mirrors the `run_impl_main`
/// pattern documented in `.claude/rules/rust-patterns.md`
/// "Main-arm dispatch."
///
/// Return contract:
/// - `(0, None)` → allow silently (exit 0, no stderr)
/// - `(2, Some(message))` → block (stderr the message, exit 2)
pub fn run_impl_main(
    hook_input: Option<Value>,
    cwd: Option<&Path>,
    home: &Path,
) -> (i32, Option<String>) {
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
    // Resolve the state file via project_root + branch when cwd is
    // available. Unresolvable cwd → no state path → halt gate sees
    // false and passes (the user-only-skill gate continues to run).
    let state_path: Option<PathBuf> = cwd.and_then(|c| {
        let (_, project_root) = crate::hooks::find_settings_and_root_from(c);
        let branch = detect_branch_from_path(c)?;
        let main_root = project_root.map(|r| resolve_main_root(&r))?;
        let paths = FlowPaths::try_new(&main_root, &branch)?;
        Some(paths.state_file())
    });
    let (allowed, msg) = validate(
        &tool_input,
        transcript_path.as_deref(),
        state_path.as_deref(),
        home,
    );
    if !allowed {
        return (2, Some(msg));
    }
    (0, None)
}

/// Run the validate-skill hook (entry point from CLI). Reads stdin,
/// resolves `$HOME` via `home_dir_or_empty()` and the current
/// working directory via `std::env::current_dir()`, calls
/// `run_impl_main`, writes any block message to stderr, and exits
/// with the returned code.
pub fn run() {
    let input = read_hook_input();
    let home = home_dir_or_empty();
    let cwd = std::env::current_dir().ok();
    let (code, message) = run_impl_main(input, cwd.as_deref(), &home);
    if let Some(m) = message {
        eprintln!("{}", m);
    }
    std::process::exit(code);
}
