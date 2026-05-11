//! Per-session "utility skill in progress" marker file.
//!
//! Multi-step utility skills (currently just `flow:flow-create-issue`)
//! invoke the Skill tool mid-skill to delegate to a child skill. The
//! Skill tool's return is a structural surface where the model often
//! treats the handoff as a natural stopping point and returns control
//! to the user — breaking the unattended-flow contract that
//! flow-create-issue promises to its consumers.
//!
//! `write_marker` (called immediately after the skill's Announce
//! banner) and `clear_marker` (called immediately before the COMPLETE
//! banner and on every error-exit path) keep a JSON marker on disk at
//! `<home>/.claude/flow/utility-in-progress-<session_id>.json` for the
//! skill's full lifecycle. The Stop hook reads the marker for the
//! current Claude Code session_id and refuses turn-end with
//! `{"decision":"block"}` if a marker is present and names a known
//! multi-step utility skill.
//!
//! The marker is per-session (not per-flow): it lives under the
//! user's HOME, not `.flow-states/`, because flow-create-issue runs
//! outside any active FLOW phase. Concurrent Claude Code sessions
//! each get their own marker file because the filename includes
//! `session_id`, so cleaning up after a crashed session is a no-op
//! for other live sessions.
//!
//! Tests live at `tests/commands/utility_marker.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` here.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::session_metrics::is_safe_session_id;
use crate::utils::now;

/// The set of multi-step utility skills the Stop hook protects from
/// mid-skill turn-end. Currently only `flow:flow-create-issue` because
/// that is the only skill that delegates to another Skill tool
/// invocation mid-pipeline. Add to this list when a future utility
/// skill grows the same shape.
pub const MULTI_STEP_UTILITY_SKILLS: &[&str] = &["flow:flow-create-issue"];

/// Subdirectory under HOME where markers live. A future expansion to
/// other FLOW machine-global state can share this directory.
pub const UTILITY_MARKER_SUBDIR: &str = ".claude/flow";

/// Filename prefix for the marker file. The full filename is
/// `<MARKER_FILENAME_PREFIX><session_id>.json`.
pub const MARKER_FILENAME_PREFIX: &str = "utility-in-progress-";

/// Maximum length for a skill name — bounds the JSON payload size
/// and keeps validation cheap.
const SKILL_NAME_MAX_LEN: usize = 64;

/// Validate a `skill` argument. Accepts ASCII alphanumeric plus the
/// punctuation that appears in canonical FLOW skill names
/// (`flow:flow-<name>` — `:`, `-`, `_`). Rejects empty, anything
/// over `SKILL_NAME_MAX_LEN`, and any character outside the allow
/// set so a corrupted state-file or hostile CLI argument cannot
/// inject a path-traversal segment, NUL byte, slash, or backslash
/// into the JSON payload.
pub fn is_safe_skill_name(s: &str) -> bool {
    if s.is_empty() || s.len() > SKILL_NAME_MAX_LEN {
        return false;
    }
    if s == "." || s == ".." {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':')
}

/// Construct the canonical marker path for a given home directory
/// and session_id, returning `None` when validation fails. Validates
/// session_id via `is_safe_session_id` AND home via
/// `is_safe_home` (rejects empty / non-absolute paths) per
/// `.claude/rules/external-input-path-construction.md` rules 1, 2,
/// and 5. Without the home guard, an unset HOME (env var missing
/// or set to empty) silently resolves the marker path against the
/// process cwd — write and read paths diverge, the predicate
/// spuriously blocks (or silently misses) depending on cwd state.
pub fn marker_path(home: &Path, session_id: &str) -> Option<PathBuf> {
    if !is_safe_home(home) {
        return None;
    }
    if !is_safe_session_id(session_id) {
        return None;
    }
    Some(
        home.join(UTILITY_MARKER_SUBDIR)
            .join(format!("{}{}.json", MARKER_FILENAME_PREFIX, session_id)),
    )
}

/// Validate that `home` is a usable absolute path. Rejects empty
/// (env var unset → home_dir_or_empty returned ""), non-absolute
/// (relative env var, "/" fallback notwithstanding clap defaults),
/// and paths containing a NUL byte (corrupted env). Mirrors the
/// home-validation pattern in `crate::session_metrics::read_rate_limits`.
fn is_safe_home(home: &Path) -> bool {
    !home.as_os_str().is_empty() && home.is_absolute() && !home.to_string_lossy().contains('\0')
}

/// Write the marker file for the given skill and session_id. Creates
/// the parent directory if missing. Validates `skill`, `session_id`,
/// and `home` before constructing any filesystem path. The marker
/// JSON contains `skill`, `session_id`, and `started_at` (Pacific-
/// time ISO 8601).
///
/// Symlink-safe per `.claude/rules/rust-patterns.md` "Symlink-Safe
/// Existence Checks Before Writes": before `fs::write`, removes any
/// pre-existing symlink at the marker path so the write cannot
/// follow a hostile symlink and overwrite a target outside
/// `<home>/.claude/flow/`. Regular files are overwritten in place
/// as before — only symlinks are unlinked first.
pub fn write_marker(home: &Path, skill: &str, session_id: &str) -> Result<PathBuf, String> {
    if !is_safe_skill_name(skill) {
        return Err(format!("invalid skill name: {:?}", skill));
    }
    let path = marker_path(home, session_id)
        .ok_or_else(|| format!("invalid session_id or home: session_id={:?}", session_id))?;
    let parent = path
        .parent()
        .expect("marker_path always carries a parent (<home>/.claude/flow)");
    fs::create_dir_all(parent).map_err(|e| format!("create dir failed: {}", e))?;
    // Detect a pre-existing symlink at the marker path via
    // `symlink_metadata` (which does NOT follow symlinks). Remove
    // the symlink before `fs::write` so the write creates a fresh
    // regular file rather than following the symlink to its
    // arbitrary target.
    if let Ok(meta) = fs::symlink_metadata(&path) {
        if meta.file_type().is_symlink() {
            let _ = fs::remove_file(&path);
        }
    }
    let payload = json!({
        "skill": skill,
        "session_id": session_id,
        "started_at": now(),
    });
    // serialization is structurally infallible for a json!() literal whose
    // values are validated strings — no nested types that could fail
    let serialized = serde_json::to_string_pretty(&payload)
        .expect("utility-marker JSON has only string values; serialize never fails");
    fs::write(&path, serialized).map_err(|e| format!("write failed: {}", e))?;
    Ok(path)
}

/// Remove the marker file for the given skill and session_id. Returns
/// `Ok(true)` when the file existed and was removed, `Ok(false)` when
/// it was already absent (idempotent). Validation runs first so a
/// corrupted state-file value cannot escape the canonical directory
/// even when the call is a clear.
pub fn clear_marker(home: &Path, skill: &str, session_id: &str) -> Result<bool, String> {
    if !is_safe_skill_name(skill) {
        return Err(format!("invalid skill name: {:?}", skill));
    }
    let path = marker_path(home, session_id)
        .ok_or_else(|| format!("invalid session_id: {:?}", session_id))?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("remove failed: {}", e)),
    }
}

/// Pure helper for resolving the active session_id from the three
/// sources skills can reach: explicit `--session-id` arg →
/// `CLAUDE_CODE_SESSION_ID` env var (validated via
/// `is_safe_session_id`) → SessionStart capture file. The env-var
/// path is the primary fallback on Claude Code 2.1.132+ because it
/// shares the per-subprocess value the Stop hook receives in its
/// stdin payload, so set-time and clear-time resolve to the same id
/// regardless of concurrent Claude Code activity. The capture file
/// remains the backstop for older Claude Code installs.
///
/// Accepts the env-var value as a parameter so the env-reading
/// boundary stays in `main.rs` and the same precedence chain is
/// reachable from both wrappers without coupling to process state.
/// Production callers are `run_set_main` and `run_clear_main` in
/// the same module; integration tests drive the precedence chain
/// through those wrappers with a controlled `env_value` argument
/// per `.claude/rules/testing-gotchas.md` "Rust Parallel Test Env
/// Var Races" (which forbids `std::env::set_var` in tests).
///
/// Precedence:
/// 1. `explicit` — non-empty wins over every other source.
/// 2. `env_value` — non-empty AND `is_safe_session_id`-valid; an
///    invalid env value falls through to the capture file rather
///    than reaching `marker_path` per
///    `.claude/rules/external-input-path-construction.md`.
/// 3. SessionStart capture file at
///    `<home>/.claude/flow-current-session.json`.
///
/// Returns `None` when every source is empty or invalid.
fn resolve_session_id_from(
    home: &Path,
    explicit: Option<&str>,
    env_value: Option<&str>,
) -> Option<String> {
    if let Some(s) = explicit {
        if !s.is_empty() {
            return Some(s.to_string());
        }
    }
    if let Some(env_sid) = env_value {
        if !env_sid.is_empty() && is_safe_session_id(env_sid) {
            return Some(env_sid.to_string());
        }
    }
    crate::hooks::capture_session::read_captured_session(home).map(|(sid, _)| sid)
}

/// Print the captured session_id (or empty string if unavailable) and
/// exit 0.
///
/// Production skills should prefer `$CLAUDE_CODE_SESSION_ID` from the
/// Bash subprocess environment; `set-utility-in-progress` and
/// `clear-utility-in-progress` resolve the active session_id at the
/// CLI boundary in `main.rs` and forward it through `run_set_main` /
/// `run_clear_main`. The recommended resolution path is the pure
/// helper `resolve_session_id_from` defined above.
///
/// This subcommand persists as a backward-compat surface for Claude
/// Code installs without the per-subprocess env var (Claude Code
/// before 2.1.132) and as an explicit override path for tests and
/// scripted callers that need to read the SessionStart capture file
/// directly.
///
/// Empty stdout (no `\n`) means no captured session_id is available;
/// the skill should treat this as a non-fatal "marker disabled"
/// outcome and continue without writing a marker — the same
/// posture as `set-utility-in-progress` returning a structured
/// error envelope when no session_id is resolvable.
pub fn run_current_session_id_main(home: &Path) -> (String, i32) {
    match crate::hooks::capture_session::read_captured_session(home) {
        Some((sid, _)) => (sid, 0),
        None => (String::new(), 0),
    }
}

/// CLI entry for `bin/flow set-utility-in-progress`. Accepts the
/// resolved HOME directory and the `CLAUDE_CODE_SESSION_ID` env
/// value as parameters so tests can drive the real production path
/// with a `TempDir` fixture and an explicit `env_value=None` without
/// touching the process environment. When `session_id` is `None`,
/// resolution falls through to the env value, then to the
/// SessionStart capture file so older Claude Code installs without
/// the per-subprocess env var still get a marker keyed by the
/// active session.
pub fn run_set_main(
    home: &Path,
    skill: &str,
    session_id: Option<&str>,
    env_value: Option<&str>,
) -> (Value, i32) {
    let resolved = match resolve_session_id_from(home, session_id, env_value) {
        Some(s) => s,
        None => {
            return (
                json!({
                    "status": "error",
                    "message": "no session_id available: pass --session-id or run inside an active Claude Code session with a populated capture file",
                }),
                0,
            );
        }
    };
    match write_marker(home, skill, &resolved) {
        Ok(path) => (json!({"status": "ok", "path": path.to_string_lossy()}), 0),
        Err(message) => (json!({"status": "error", "message": message}), 0),
    }
}

/// CLI entry for `bin/flow clear-utility-in-progress`. Same shape as
/// `run_set_main` — returns JSON to stdout and exit code 0 for
/// business outcomes per the project convention. Same precedence
/// chain (explicit → env → capture file) routed through
/// `resolve_session_id_from`.
pub fn run_clear_main(
    home: &Path,
    skill: &str,
    session_id: Option<&str>,
    env_value: Option<&str>,
) -> (Value, i32) {
    let resolved = match resolve_session_id_from(home, session_id, env_value) {
        Some(s) => s,
        None => {
            return (
                json!({
                    "status": "error",
                    "message": "no session_id available: pass --session-id or run inside an active Claude Code session with a populated capture file",
                }),
                0,
            );
        }
    };
    match clear_marker(home, skill, &resolved) {
        Ok(removed) => (json!({"status": "ok", "removed": removed}), 0),
        Err(message) => (json!({"status": "error", "message": message}), 0),
    }
}
