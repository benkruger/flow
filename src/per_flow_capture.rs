//! Per-flow snapshot orchestrator. Reads `session_id` and
//! `transcript_path` from the in-memory state JSON, validates them
//! against the path-construction discipline in
//! `.claude/rules/external-input-path-construction.md`, derives the
//! per-session cost-file path, and bundles
//! [`session_metrics::capture`] with [`session_cost::read_cost_file`]
//! into a final [`WindowSnapshot`].
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

use crate::session_cost;
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
    let mut snap =
        session_metrics::capture(home, transcript_path.as_deref(), session_id.as_deref(), now);
    if let Some(sid) = session_id.as_deref() {
        // `session_id` has already passed `is_safe_session_id` on
        // line 51, and `cost_file_path` uses the same validator, so
        // this branch is unreachable per
        // `.claude/rules/reachable-is-testable.md` "When the test
        // resists the real production path".
        let cost_path = session_cost::cost_file_path(project_root, sid)
            .expect("sid passed is_safe_session_id upstream");
        snap.session_cost_usd = session_cost::read_cost_file(&cost_path);
    }
    snap
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
fn derive_transcript_path(home: &Path, project_root: &Path, session_id: &str) -> PathBuf {
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
