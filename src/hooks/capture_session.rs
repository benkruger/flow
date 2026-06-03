//! SessionStart hook: persist `session_id` and `transcript_path` so
//! flow-start can seed them into the new flow's state file.
//!
//! Claude Code delivers `session_id` and `transcript_path` to hooks via
//! stdin JSON, but does not expose either as an environment variable
//! visible to Bash-tool subprocesses. Without this capture, the
//! `session_id` field of a freshly-created state file stays Null and
//! the per-session cost-file lookup at `<project_root>/.claude/cost/
//! <YYYY-MM>/<session_id>.txt` cannot resolve, leaving the Token Cost
//! section's start anchor empty (issue #1410).
//!
//! The hook fires on every Claude Code SessionStart and overwrites the
//! capture file unconditionally. Multi-session machines accept
//! last-writer-wins: a wrong session_id at flow-start would cause the
//! cost lookup to fail gracefully (no cost file matches), which is no
//! worse than the pre-fix state where session_id was always Null.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::session_metrics::{
    home_dir_or_empty, is_safe_session_id, is_safe_transcript_path_structural,
};

/// Capture-file payload byte cap per
/// `.claude/rules/external-input-path-construction.md` "Enforce a
/// documented size cap on every external read". The capture file
/// holds two short JSON string fields (`session_id` ≤ 256 bytes
/// per `is_safe_session_id`, `transcript_path` typically a few
/// hundred bytes); 64 KB bounds a corrupted, hand-edited, or
/// adversarially-grown file to a value the SessionStart hook can
/// process without unbounded heap allocation, matching the byte-cap
/// pattern in `src/session_metrics.rs::TRANSCRIPT_BYTE_CAP`.
const CAPTURE_FILE_BYTE_CAP: u64 = 64 * 1024;

/// Stdin payload byte cap for the SessionStart hook. Claude Code
/// passes a small JSON object on stdin; 64 KB is generous and
/// bounds a runaway producer or hostile injection at the input
/// boundary so a multi-megabyte stdin cannot reach the validator
/// or be re-serialized into the capture file.
const STDIN_BYTE_CAP: u64 = 64 * 1024;

/// Canonical capture-file path under `<home>/.claude/`. Co-located with
/// `rate-limits.json` and `projects/` so all FLOW HOME-dependent state
/// shares the same directory tree.
pub(crate) fn capture_file_path(home: &Path) -> PathBuf {
    home.join(".claude").join("flow-current-session.json")
}

/// Read and validate the capture file written by [`run`].
///
/// Returns `Some((session_id, transcript_path))` when:
/// 1. `home` is absolute (rejects empty / relative env-var values per
///    `.claude/rules/external-input-path-construction.md`).
/// 2. The capture file exists, fits within [`CAPTURE_FILE_BYTE_CAP`],
///    and parses as JSON.
/// 3. `session_id` matches [`is_safe_session_id`].
/// 4. `transcript_path` is either absent OR matches
///    [`is_safe_transcript_path_structural`] against `home`. The
///    structural variant accepts shape-valid paths whose JSONL file
///    does not yet exist on disk, so flow-start at SessionStart time
///    seeds a real `transcript_path` into the state file instead of
///    leaving it null (issue #1525). Symlink-escape is closed by
///    every read-time consumer (transcript walkers) via
///    `is_safe_transcript_path`'s canonicalize step before any
///    `File::open`.
pub(crate) fn read_captured_session(home: &Path) -> Option<(String, Option<String>)> {
    if home.as_os_str().is_empty() || !home.is_absolute() {
        return None;
    }
    let path = capture_file_path(home);
    let file = fs::File::open(&path).ok()?;
    let mut content = String::new();
    file.take(CAPTURE_FILE_BYTE_CAP)
        .read_to_string(&mut content)
        .ok()?;
    let parsed: Value = serde_json::from_str(&content).ok()?;
    let session_id = parsed
        .get("session_id")
        .and_then(|v| v.as_str())
        .filter(|s| is_safe_session_id(s))
        .map(|s| s.to_string())?;
    let transcript_path = parsed
        .get("transcript_path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|tp| is_safe_transcript_path_structural(Path::new(tp), home));
    Some((session_id, transcript_path))
}

/// SessionStart hook entry point. Reads stdin JSON, validates the
/// payload, writes the capture file. Errors are silently swallowed —
/// the hook must never block the SessionStart event.
pub fn run() {
    let mut buf = String::new();
    let _ = std::io::stdin()
        .take(STDIN_BYTE_CAP)
        .read_to_string(&mut buf);
    let input: Value = serde_json::from_str(&buf).unwrap_or(Value::Null);
    let session_id = match input.get("session_id").and_then(|v| v.as_str()) {
        Some(s) if is_safe_session_id(s) => s.to_string(),
        _ => return,
    };
    let home = home_dir_or_empty();
    if home.as_os_str().is_empty() || !home.is_absolute() {
        return;
    }
    let transcript_path = input
        .get("transcript_path")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .filter(|p| is_safe_transcript_path_structural(p, &home))
        .map(|p| p.to_string_lossy().to_string());
    let payload = json!({
        "session_id": session_id.clone(),
        "transcript_path": transcript_path.clone(),
    });
    let path = capture_file_path(&home);
    // `capture_file_path` always returns `<home>/.claude/<basename>`,
    // so `parent()` is always `Some(<home>/.claude)`. The `.expect`
    // documents the upstream invariant per
    // `.claude/rules/testability-means-simplicity.md` "When the test
    // resists the real production path".
    let parent = path
        .parent()
        .expect("capture_file_path always returns <home>/.claude/<basename>");
    let _ = fs::create_dir_all(parent);
    // Symlink-safe write per `.claude/rules/rust-patterns.md`
    // "Symlink-Safe Existence Checks Before Writes". A pre-existing
    // symlink at `path` would cause `fs::write` to follow the link
    // and overwrite its target — an arbitrary-write primitive
    // exploitable on shared `~/.claude/` directories. Detecting any
    // existing entry via `fs::symlink_metadata` (which does NOT
    // follow symlinks) and removing it first ensures `fs::write`
    // always creates a fresh regular file.
    if let Ok(meta) = fs::symlink_metadata(&path) {
        if meta.file_type().is_symlink() {
            let _ = fs::remove_file(&path);
        }
    }
    let _ = fs::write(&path, payload.to_string());
}
