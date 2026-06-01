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
//!
//! After the global capture write, the hook also refreshes the active
//! flow's `session_id` and `transcript_path` in
//! `.flow-states/<branch>/state.json` (keyed by branch-from-cwd). The
//! global capture file seeds NEW flows at flow-start; an already-running
//! flow holds the pointers it captured when it started. When the Claude
//! Code session rotates mid-flow (resume|clear|compact), the rotated
//! transcript no longer matches the flow's stored pointer and
//! `record-agent-return` reports `phase_marker_not_found` for every
//! Review and Learn agent. `refresh_active_flow_session` keeps the
//! pointer current so the verifier reads the live session's transcript.
//!
//! Known limitation: the refresh repoints the flow at the rotated
//! session's transcript wholesale. A resume that lands *mid-phase* —
//! the `phase-enter` marker in the prior session's file and the
//! agents' tool_use/tool_result pairs in the rotated file — is not
//! covered by the refresh alone; `verify_agent_returned_in_phase`
//! anchors on the marker, which now sits in a file the flow no longer
//! points at. Closing that gap needs a session-lineage union scan
//! across the prior and rotated transcripts and is deferred.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::flow_paths::FlowPaths;
use crate::hooks::{detect_branch_from_path, is_flow_active, resolve_hook_cwd, resolve_main_root};
use crate::lock::mutate_state;
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

    // Refresh the active flow's session pointers from the same payload.
    // The global capture file above seeds NEW flows at flow-start, but
    // an already-running flow holds the session_id/transcript_path
    // captured when it started. When the Claude Code session rotates
    // mid-flow (resume|clear|compact), the rotated transcript no longer
    // matches the flow's stored pointer, and `record-agent-return`
    // reports `phase_marker_not_found` for every Review/Learn agent.
    // Refreshing the active flow's state file here keeps the pointer
    // current. The cwd identifies which flow to refresh; a degenerate
    // or absent cwd resolves to no active flow and the refresh no-ops.
    // `resolve_hook_cwd` returns `None` only when the payload carries no
    // `cwd` AND `env::current_dir()` fails — an empty-string fallback
    // resolves no branch in `refresh_active_flow_session`, so the
    // unreachable-in-practice None case collapses into the same no-op.
    let cwd = resolve_hook_cwd(&input).unwrap_or_default();
    refresh_active_flow_session(&cwd, &session_id, transcript_path.as_deref());
}

/// Refresh the active flow's `session_id` AND `transcript_path` to the
/// rotated session's values.
///
/// `cwd` is the SessionStart payload's working directory: during an
/// active flow it is the worktree, from which `detect_branch_from_path`
/// derives the branch and `resolve_main_root` derives the main repo
/// root. Both pointer fields are overwritten together in one
/// `mutate_state` so a half-updated pair cannot leave
/// `resolve_transcript_path` (which prefers `transcript_path`) reading
/// a rotated session_id against a stale transcript.
///
/// `transcript_path` carries the value already validated by `run()`'s
/// structural check (`is_safe_transcript_path_structural`); `None`
/// (absent or invalid in the payload) is written as JSON null so the
/// stale value is not retained.
///
/// Fail-open at every boundary — no detectable branch, no active flow,
/// a wrong-root-type state file — so the hook never blocks or panics on
/// the SessionStart event:
/// - `detect_branch_from_path` returns `None` when `cwd` is not inside a
///   worktree (no flow to refresh).
/// - `is_flow_active` returns `false` for an invalid (slash-bearing)
///   branch or a branch with no `state.json` (no active flow).
/// - the `mutate_state` closure's object guard returns without mutating
///   when the state file is a wrong root type (array, string, number),
///   per `.claude/rules/state-files.md` "Corruption Resilience".
fn refresh_active_flow_session(cwd: &str, session_id: &str, transcript_path: Option<&str>) {
    let cwd_path = Path::new(cwd);
    let branch = match detect_branch_from_path(cwd_path) {
        Some(b) => b,
        None => return,
    };
    let main_root = resolve_main_root(cwd_path);
    if !is_flow_active(&branch, &main_root) {
        return;
    }
    // `is_flow_active` returned true, so the branch passed
    // `FlowPaths::is_valid_branch` AND the state file exists —
    // `try_new` is infallible here. The `.expect` documents that
    // upstream guarantee per `.claude/rules/branch-path-safety.md`
    // (documentation, not a reachable panic).
    let state_path = FlowPaths::try_new(&main_root, &branch)
        .expect("is_flow_active confirmed branch is valid")
        .state_file();
    let _ = mutate_state(&state_path, &mut |state| {
        // Guard: string-key IndexMut panics on array/bool/number/string
        // roots. Fail-open on any non-writable shape (Null is
        // auto-vivified to an object by serde_json's IndexMut).
        if !(state.is_object() || state.is_null()) {
            return;
        }
        state["session_id"] = Value::String(session_id.to_string());
        state["transcript_path"] = match transcript_path {
            Some(tp) => Value::String(tp.to_string()),
            None => Value::Null,
        };
    });
}
