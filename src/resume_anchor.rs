//! Read-side resolver for the session-keyed phase-anchor marker
//! (`bin/flow resume-anchor`).
//!
//! On a `--continue-step` resume, a phase skill needs `worktree_cwd` to
//! re-anchor its shell, but the value is emitted only by a fresh
//! `phase-enter` and the resume path's branch detection is itself
//! cwd-dependent. `src/phase_anchor.rs` breaks that cycle by writing a
//! session-keyed marker; this module reads it back. The skill calls
//! `bin/flow resume-anchor` before any cwd-dependent step and `cd`s
//! into the recovered `worktree_cwd` on success.
//!
//! Three outcomes, all exit code 0 (callers branch on the JSON `status`
//! field, not the shell exit code, per the
//! `.claude/rules/rust-patterns.md` business-error convention):
//!
//! - `{"status":"ok","worktree_cwd":"<abs path>"}` — marker found,
//!   parsed, and the recovered path is safe to `cd` into.
//! - `{"status":"no_marker"}` — no session id resolves, or the marker
//!   file does not exist. The skill falls back to today's cwd-based
//!   branch detection (graceful degradation).
//! - `{"status":"error","message":"..."}` — the marker exists but is
//!   corrupt: oversized, unparseable, missing `worktree_cwd`, or
//!   carrying an unsafe recovered path. Fail-closed: never hand a
//!   skill a `cd` target derived from a corrupt marker.
//!
//! Session-id resolution and marker-path construction reuse
//! `src/phase_anchor.rs` so the read side resolves the exact path the
//! write side produced. The marker read is byte-capped per
//! `.claude/rules/external-input-path-construction.md`, and the
//! recovered `worktree_cwd` passes a positive path validator before it
//! is returned.
//!
//! Tests live at `tests/resume_anchor.rs` per
//! `.claude/rules/test-placement.md`.

use std::fs::File;
use std::io::Read;
use std::path::{Component, Path};

use serde_json::{json, Value};

use crate::phase_anchor::{marker_path, resolve_session_id};

/// Byte cap for the marker read per
/// `.claude/rules/external-input-path-construction.md` "Enforce a
/// documented size cap on every external read". The marker holds four
/// short JSON string fields; 64 KB bounds a corrupted, hand-edited, or
/// adversarially-grown marker to a value the resolver can process
/// without unbounded heap allocation, matching the cap pattern in
/// `src/hooks/capture_session.rs`.
pub const MARKER_BYTE_CAP: u64 = 64 * 1024;

/// Validate a recovered `worktree_cwd` before it is returned for the
/// skill to `cd` into. Requires an absolute, NUL-free path with no
/// `..` component. A relative or traversal-bearing value would make
/// the skill's `cd "<worktree_cwd>"` resolve somewhere unintended, so
/// the resolver fails closed on it. An empty string is rejected
/// implicitly — `Path::new("")` is not absolute.
fn is_safe_recovered_path(path: &str) -> bool {
    if path.contains('\0') {
        return false;
    }
    let p = Path::new(path);
    p.is_absolute() && !p.components().any(|c| matches!(c, Component::ParentDir))
}

/// Outcome of reading the marker file at a resolved path.
enum MarkerRead {
    /// The marker file does not exist — nothing to recover.
    NotFound,
    /// The marker exists but is corrupt; the string is the reason.
    Error(String),
    /// A valid, safe recovered `worktree_cwd`.
    Ok(String),
}

/// Read and validate the marker at `path`. Byte-capped; fail-closed on
/// every corruption class (oversized, unparseable, missing field,
/// unsafe path). `NotFound` is distinguished from other IO errors so
/// the caller can map a missing marker to `no_marker` rather than
/// `error`.
fn read_marker(path: &Path) -> MarkerRead {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return MarkerRead::NotFound,
        Err(e) => return MarkerRead::Error(format!("marker open failed: {}", e)),
    };
    // Read one byte past the cap so an oversized file is detectable
    // rather than silently truncated to a parseable prefix.
    let mut content = String::new();
    if let Err(e) = file.take(MARKER_BYTE_CAP + 1).read_to_string(&mut content) {
        return MarkerRead::Error(format!("marker read failed: {}", e));
    }
    if content.len() as u64 > MARKER_BYTE_CAP {
        return MarkerRead::Error("marker exceeds byte cap".to_string());
    }
    let parsed: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => return MarkerRead::Error(format!("marker JSON parse failed: {}", e)),
    };
    let worktree_cwd = match parsed.get("worktree_cwd").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return MarkerRead::Error("marker missing worktree_cwd".to_string()),
    };
    if !is_safe_recovered_path(worktree_cwd) {
        return MarkerRead::Error(format!("unsafe worktree_cwd in marker: {:?}", worktree_cwd));
    }
    MarkerRead::Ok(worktree_cwd.to_string())
}

/// Testable core. Resolves the session id (env value → SessionStart
/// capture file via `phase_anchor::resolve_session_id`), builds the
/// marker path, and reads it. Returns the three-outcome JSON. Never
/// panics — every failure is a JSON outcome.
pub fn run_impl(home: &Path, env_value: Option<&str>) -> Value {
    let sid = match resolve_session_id(home, env_value) {
        Some(s) => s,
        None => return json!({"status": "no_marker"}),
    };
    let path = match marker_path(home, &sid) {
        Some(p) => p,
        None => return json!({"status": "no_marker"}),
    };
    match read_marker(&path) {
        MarkerRead::NotFound => json!({"status": "no_marker"}),
        MarkerRead::Error(msg) => json!({"status": "error", "message": msg}),
        MarkerRead::Ok(worktree_cwd) => json!({"status": "ok", "worktree_cwd": worktree_cwd}),
    }
}

/// Main-arm dispatcher. Business outcomes always exit 0 — callers parse
/// the JSON `status` field, not the shell exit code.
pub fn run_impl_main(home: &Path, env_value: Option<&str>) -> (Value, i32) {
    (run_impl(home, env_value), 0)
}
