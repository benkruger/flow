//! Per-flow snapshot orchestrator. Reads `session_id` and
//! `transcript_path` from the in-memory state JSON, validates them
//! against the path-construction discipline in
//! `.claude/rules/external-input-path-construction.md`, and produces
//! a [`WindowSnapshot`] via [`session_metrics::capture`]. Cost is
//! token-derived downstream in `window_deltas` from the snapshot's
//! `by_model` rollup, not captured here.
//!
//! Producers (`phase_enter`, `phase_finalize`, `phase_transition`,
//! `set_timestamp`, `start_init`, `complete_finalize`,
//! `complete_fast`) call this from inside `mutate_state` closures
//! (the state JSON is already in memory) and write the returned
//! snapshot into the appropriate state field. `home` is supplied
//! by the producer (typically `$HOME`) so this helper takes no
//! process-env dependency.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::session_metrics::{self, is_safe_session_id, is_safe_transcript_path};
use crate::state::WindowSnapshot;
use crate::utils::now;

/// Produce a `WindowSnapshot` for the current flow's state JSON.
///
/// `session_id` and `transcript_path` are both state-derived
/// strings. A corrupted or hand-edited
/// `.flow-states/<branch>/state.json` can populate either field
/// with attacker-controlled values, so this helper validates
/// before constructing filesystem paths. `session_id` must look
/// like a UUID-shaped token (no path separators, no traversal
/// segments). `transcript_path` is rejected when it is not
/// absolute or escapes the user's `~/.claude/projects/` directory
/// — the only place flow's session transcripts live in production.
///
/// Self-heal: when state's `transcript_path` is null (the
/// SessionStart hook's strict validator rejected the path because
/// the file did not yet exist), this derives the canonical
/// transcript location from `<home>/.claude/projects/<encoded>/
/// <session_id>.jsonl` using Claude Code's directory-encoding
/// convention (every character that is not ASCII alphanumeric
/// or `_` or `-` becomes `-`). The derived path runs through the
/// same `is_safe_transcript_path` validator so a hostile entry
/// under `~/.claude/projects/` cannot redirect the read.
pub fn capture_for_active_state(home: &Path, state: &Value, project_root: &Path) -> WindowSnapshot {
    let session_id = state
        .get("session_id")
        .and_then(|v| v.as_str())
        .filter(|s| is_safe_session_id(s))
        .map(|s| s.to_string());
    let transcript_path = state
        .get("transcript_path")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .filter(|p| is_safe_transcript_path(p, home))
        .or_else(|| {
            session_id
                .as_ref()
                .map(|sid| derive_transcript_path(home, project_root, sid))
                .filter(|p| is_safe_transcript_path(p, home))
        });
    session_metrics::capture(home, transcript_path.as_deref(), session_id.as_deref(), now)
}

/// Derive the canonical transcript path Claude Code writes to:
/// `<home>/.claude/projects/<encoded-project-root>/<session_id>.jsonl`.
/// The encoding rule (confirmed by inspecting existing
/// `~/.claude/projects/` entries against their source project
/// roots): every character that is not ASCII alphanumeric and not
/// `_` and not `-` becomes `-`. Examples:
///
/// - `/Users/ben/code/flow` → `-Users-ben-code-flow`
/// - `/Users/ben/.claude` → `-Users-ben--claude` (the leading `/` and the `.` each become `-`)
/// - `/Users/ben/My Project` → `-Users-ben-My-Project` (the space becomes `-`)
/// - `/Users/ben/code-cc-api` → `-Users-ben-code-cc-api` (the `-` characters are preserved)
///
/// The result is run through `is_safe_transcript_path` by the
/// caller, so this helper does no validation itself — it only
/// builds the candidate `PathBuf`.
///
/// Consumed by:
///
/// - `capture_for_active_state` above (the original consumer) — the
///   self-heal branch when state's `transcript_path` is null.
/// - `crate::record_agent_return::run_impl_main` — same self-heal
///   branch, applied before calling
///   `verify_agent_returned_in_phase` to ensure the verifier reads
///   the canonical transcript path even when the state file's
///   `transcript_path` is missing or null.
pub fn derive_transcript_path(home: &Path, project_root: &Path, session_id: &str) -> PathBuf {
    let encoded: String = project_root
        .to_string_lossy()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    home.join(".claude")
        .join("projects")
        .join(encoded)
        .join(format!("{}.jsonl", session_id))
}
