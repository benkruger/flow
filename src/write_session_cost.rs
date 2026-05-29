//! `write-session-cost` — write the active session's token-derived
//! cost to the per-session cost file so month-to-date (MTD) spend
//! reconciles with the token counts rendered beside it.
//!
//! Month-to-date is summed by `session_cost::read_monthly_aggregate`
//! (and the user's statusline) over every per-session file under
//! `<project_root>/.claude/cost/<YYYY-MM>/`. The statusline writes
//! each session's cost from `cost.total_cost_usd`, sampled on a
//! different clock than the token counts. This subcommand overwrites
//! the active session's file with a cost derived from the SAME
//! per-model token capture (`session_metrics::capture` + the
//! `pricing` table) so the MTD total reconciles with the tokens.
//!
//! Invoked from the SessionStart capture hook (the same hook that
//! persists session_id/transcript_path), so it fires across every
//! session a flow spans. A session where no FLOW hook fires writes no
//! token-derived file and the statusline value stands — an accepted
//! coverage boundary. The user statusline is never modified.
//!
//! When the session has no priceable per-model usage (empty
//! transcript, unpriced model family) the subcommand writes nothing
//! rather than clobbering the statusline value with a meaningless
//! zero. The path is computed by `session_cost::cost_file_path` —
//! reused verbatim — and `session_id` is validated by
//! `is_safe_session_id` (via `cost_file_path`) and `transcript_path`
//! by `is_safe_transcript_path_structural` before use, per
//! `.claude/rules/external-input-path-construction.md`.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::pricing::cost_for;
use crate::session_cost::cost_file_path;
use crate::session_metrics::{capture, home_dir_or_empty, is_safe_transcript_path_structural};

/// Stdin payload byte cap. Claude Code passes a small JSON object on
/// stdin (session_id ≤ 256 bytes, transcript_path a few hundred);
/// 64 KB bounds a runaway producer at the input boundary per
/// `.claude/rules/external-input-path-construction.md`.
const STDIN_BYTE_CAP: u64 = 64 * 1024;

/// Compute and write the active session's token-derived cost.
///
/// `stdin` is the hook JSON payload (`session_id`, optional
/// `transcript_path`); `project_root` anchors the cost-file path;
/// `home` anchors `capture`'s rate-limit + sub-session reads.
/// Returns a JSON status envelope and exit code (always 0 — callers
/// parse `status`).
///
/// The capture timestamp is not configurable because this subcommand
/// reads only the snapshot's `by_model` usage to derive cost — the
/// timestamp never reaches the cost file, so no `now_fn` seam is
/// needed.
///
/// Status values: `ok` (file written), `skipped` (no session_id, an
/// unsafe session_id, or no priceable usage — nothing written), or
/// `error` (the write itself failed).
pub fn run_impl_main(stdin: &str, project_root: &Path, home: &Path) -> (Value, i32) {
    let input: Value = serde_json::from_str(stdin).unwrap_or(Value::Null);
    let session_id = match input.get("session_id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return (json!({"status": "skipped", "reason": "no_session_id"}), 0),
    };
    // Validate session_id by constructing the canonical path first —
    // `cost_file_path` returns None for any unsafe id, so an unsafe
    // value fails fast before the capture work.
    let path = match cost_file_path(project_root, &session_id) {
        Some(p) => p,
        None => {
            return (
                json!({"status": "skipped", "reason": "unsafe_session_id"}),
                0,
            )
        }
    };

    let transcript_path = input
        .get("transcript_path")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .filter(|p| is_safe_transcript_path_structural(p, home));

    let snapshot = capture(
        home,
        transcript_path.as_deref(),
        Some(&session_id),
        crate::utils::now,
    );

    // Sum the priced per-model usage. Unpriced models (unknown family)
    // contribute nothing; if nothing prices, leave the statusline
    // value intact rather than writing a meaningless zero.
    let mut total = 0.0_f64;
    let mut any_priced = false;
    for (model, tokens) in &snapshot.by_model {
        if let Some(c) = cost_for(model, tokens) {
            total += c;
            any_priced = true;
        }
    }
    if !any_priced {
        return (json!({"status": "skipped", "reason": "no_priced_usage"}), 0);
    }

    // `cost_file_path` always returns
    // `<project_root>/.claude/cost/<YYYY-MM>/<session_id>`, so
    // `parent()` is always `Some(.../<YYYY-MM>)`. The `.expect`
    // documents the upstream invariant per
    // `.claude/rules/testability-means-simplicity.md` "When the test
    // resists the real production path" (same pattern as
    // `capture_session::run`).
    let parent = path
        .parent()
        .expect("cost_file_path always returns .../<YYYY-MM>/<session_id>");
    let _ = fs::create_dir_all(parent);
    // Symlink-safe write per `.claude/rules/rust-patterns.md`
    // "Symlink-Safe Existence Checks Before Writes": remove any
    // pre-existing symlink so `fs::write` creates a fresh regular
    // file instead of following the link to an arbitrary target.
    if let Ok(meta) = fs::symlink_metadata(&path) {
        if meta.file_type().is_symlink() {
            let _ = fs::remove_file(&path);
        }
    }
    match fs::write(&path, format!("{}\n", total)) {
        Ok(()) => (
            json!({
                "status": "ok",
                "cost_usd": total,
                "path": path.to_string_lossy(),
            }),
            0,
        ),
        Err(e) => (
            json!({
                "status": "error",
                "reason": "write_failed",
                "message": e.to_string(),
            }),
            0,
        ),
    }
}

/// Binary entry point: read the hook stdin payload, resolve
/// `project_root` and `home`, and dispatch the JSON status.
pub fn run() {
    let mut buf = String::new();
    let _ = std::io::stdin()
        .take(STDIN_BYTE_CAP)
        .read_to_string(&mut buf);
    let root = crate::git::project_root();
    let home = home_dir_or_empty();
    let (value, code) = run_impl_main(&buf, &root, &home);
    crate::dispatch::dispatch_json(value, code);
}
