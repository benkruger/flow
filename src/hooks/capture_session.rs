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

use crate::window_snapshot::{home_dir_or_empty, is_safe_session_id, is_safe_transcript_path};

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
/// 2. The capture file exists and parses as JSON.
/// 3. `session_id` matches [`is_safe_session_id`].
/// 4. `transcript_path` is either absent OR matches
///    [`is_safe_transcript_path`] against `home`.
///
/// Returns `None` on any failure path — fail-open semantics so a
/// malformed or missing capture file leaves the caller's state
/// untouched.
pub(crate) fn read_captured_session(home: &Path) -> Option<(String, Option<String>)> {
    if home.as_os_str().is_empty() || !home.is_absolute() {
        return None;
    }
    let path = capture_file_path(home);
    let content = fs::read_to_string(&path).ok()?;
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
        .filter(|tp| is_safe_transcript_path(Path::new(tp), home));
    Some((session_id, transcript_path))
}

/// SessionStart hook entry point. Reads stdin JSON, validates the
/// payload, writes the capture file. Errors are silently swallowed —
/// the hook must never block the SessionStart event.
pub fn run() {
    let mut buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut buf);
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
        .filter(|p| is_safe_transcript_path(p, &home))
        .map(|p| p.to_string_lossy().to_string());
    let payload = json!({
        "session_id": session_id,
        "transcript_path": transcript_path,
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
    let _ = fs::write(&path, payload.to_string());
}
