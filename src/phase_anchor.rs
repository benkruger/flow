//! Per-session phase-anchor marker file.
//!
//! On a fresh `phase-enter`, the response carries `worktree_cwd` so a
//! skill can `cd "<worktree_cwd>"` to re-anchor its shell. On a
//! `--continue-step` self-invocation, that re-anchor cannot run: the
//! `worktree_cwd` value comes only from `phase-enter`, and the resume
//! path's branch detection is itself cwd-dependent (it matches
//! `git worktree list` against the current directory). When context
//! loss between skill invocations resets cwd to the main-repo root,
//! branch detection resolves the integration branch — not the feature
//! branch — so the path cannot be reconstructed from the branch.
//!
//! This module breaks that circular dependency with a session-keyed
//! marker. `phase-enter` (which runs with the correct cwd and already
//! computes `branch`, `worktree_cwd`, and `relative_cwd` on a fresh
//! invocation) writes `worktree_cwd` to
//! `<home>/.claude/flow/phase-anchor-<session_id>.json`. Because the
//! marker is keyed by session id — not cwd, not branch — the read-side
//! resolver (`bin/flow resume-anchor`, `src/resume_anchor.rs`) recovers
//! `worktree_cwd` even after a same-session cwd reset.
//!
//! The marker is per-session (filename carries `session_id`) so
//! concurrent N×N×N flows never collide. `phase-enter` overwrites it on
//! every entry; Phase 4 Complete cleanup deletes it. A new session that
//! cannot resolve a session id writes no marker — the resolver then
//! falls back to today's cwd-based branch detection (graceful
//! degradation), mirroring the utility-marker skip behavior.
//!
//! Path construction and session-id resolution follow the
//! `src/commands/utility_marker.rs` conventions: `is_safe_home` +
//! `is_safe_session_id` gate the path build per
//! `.claude/rules/external-input-path-construction.md` before any
//! `Path::join`/`format!`. The marker write never blocks or panics
//! phase entry — `write_anchor_if_resolvable` swallows every error.
//!
//! Tests live at `tests/phase_anchor.rs` per
//! `.claude/rules/test-placement.md`.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::session_metrics::is_safe_session_id;
use crate::utils::now;

/// Subdirectory under HOME where the marker lives — shared with the
/// utility-in-progress marker so all FLOW machine-global session state
/// is co-located.
pub const PHASE_ANCHOR_SUBDIR: &str = ".claude/flow";

/// Filename prefix for the phase-anchor marker. The full filename is
/// `<PHASE_ANCHOR_FILENAME_PREFIX><session_id>.json`.
pub const PHASE_ANCHOR_FILENAME_PREFIX: &str = "phase-anchor-";

/// Validate that `home` is a usable absolute path. Rejects empty (env
/// var unset → `home_dir_or_empty` returned ""), non-absolute (relative
/// env var), and paths containing a NUL byte (corrupted env). Mirrors
/// the home-validation pattern in
/// `crate::commands::utility_marker::is_safe_home`.
fn is_safe_home(home: &Path) -> bool {
    !home.as_os_str().is_empty() && home.is_absolute() && !home.to_string_lossy().contains('\0')
}

/// Construct the canonical marker path for a given home directory and
/// session_id, returning `None` when validation fails. Validates `home`
/// via `is_safe_home` and `session_id` via `is_safe_session_id` per
/// `.claude/rules/external-input-path-construction.md` rules 1, 2, and 5
/// before constructing any path. Shared by the writer here and the
/// read-side resolver in `src/resume_anchor.rs` so both sides agree on
/// the exact path.
pub fn marker_path(home: &Path, session_id: &str) -> Option<PathBuf> {
    if !is_safe_home(home) {
        return None;
    }
    if !is_safe_session_id(session_id) {
        return None;
    }
    Some(home.join(PHASE_ANCHOR_SUBDIR).join(format!(
        "{}{}.json",
        PHASE_ANCHOR_FILENAME_PREFIX, session_id
    )))
}

/// Write the phase-anchor marker for the given session_id. Creates the
/// parent directory if missing. Validates `home` and `session_id`
/// before constructing any path. The marker JSON carries `branch`,
/// `worktree_cwd`, `relative_cwd`, and `written_at` (Pacific-time
/// ISO 8601).
///
/// Symlink-safe per `.claude/rules/rust-patterns.md` "Symlink-Safe
/// Existence Checks Before Writes": removes any pre-existing symlink at
/// the marker path before `fs::write` so the write cannot follow a
/// hostile symlink and overwrite an arbitrary target. Regular files are
/// overwritten in place (every `phase-enter` refreshes the marker).
pub fn write_anchor(
    home: &Path,
    session_id: &str,
    branch: &str,
    worktree_cwd: &str,
    relative_cwd: &str,
) -> Result<PathBuf, String> {
    let path = marker_path(home, session_id)
        .ok_or_else(|| format!("invalid session_id or home: session_id={:?}", session_id))?;
    let parent = path
        .parent()
        .expect("marker_path always carries a parent (<home>/.claude/flow)");
    fs::create_dir_all(parent).map_err(|e| format!("create dir failed: {}", e))?;
    if let Ok(meta) = fs::symlink_metadata(&path) {
        if meta.file_type().is_symlink() {
            let _ = fs::remove_file(&path);
        }
    }
    let payload = json!({
        "branch": branch,
        "worktree_cwd": worktree_cwd,
        "relative_cwd": relative_cwd,
        "written_at": now(),
    });
    let serialized = serde_json::to_string_pretty(&payload)
        .expect("phase-anchor JSON has only string values; serialize never fails");
    fs::write(&path, serialized).map_err(|e| format!("write failed: {}", e))?;
    Ok(path)
}

/// Resolve the active session_id for the marker write, returning `None`
/// when no source yields a valid id. Precedence mirrors
/// `crate::commands::utility_marker::resolve_session_id_from`:
///
/// 1. `env_value` — the `CLAUDE_CODE_SESSION_ID` value the caller read
///    at the CLI boundary. Used when non-empty AND `is_safe_session_id`
///    valid; an invalid env value falls through rather than reaching
///    `marker_path`.
/// 2. The SessionStart capture file at
///    `<home>/.claude/flow-current-session.json`.
///
/// The env value is a parameter (not read here) so the env-reading
/// boundary stays in `main.rs`/`phase_enter` and this function is
/// testable without `std::env::set_var` per
/// `.claude/rules/testing-gotchas.md` "Rust Parallel Test Env Var
/// Races".
pub fn resolve_session_id(home: &Path, env_value: Option<&str>) -> Option<String> {
    if let Some(env_sid) = env_value {
        if !env_sid.is_empty() && is_safe_session_id(env_sid) {
            return Some(env_sid.to_string());
        }
    }
    crate::hooks::capture_session::read_captured_session(home).map(|(sid, _)| sid)
}

/// Resolve the session_id and write the marker, swallowing every
/// failure. This is the `phase-enter` entry point: a phase entry must
/// never be blocked or panicked by a marker-write problem. When no
/// session_id resolves, no marker is written (the resolver falls back
/// to cwd-based branch detection).
pub fn write_anchor_if_resolvable(
    home: &Path,
    env_value: Option<&str>,
    branch: &str,
    worktree_cwd: &str,
    relative_cwd: &str,
) {
    if let Some(sid) = resolve_session_id(home, env_value) {
        let _ = write_anchor(home, &sid, branch, worktree_cwd, relative_cwd);
    }
}
