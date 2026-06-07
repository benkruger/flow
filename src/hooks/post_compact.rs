//! PostCompact hook: capture compaction context into the state file.
//!
//! Tests live at tests/hooks/post_compact.rs per
//! .claude/rules/test-placement.md — no inline #[cfg(test)] in this file.

use std::path::Path;

use serde_json::Value;

use crate::git::project_root;
use crate::lock::mutate_state;
use crate::utils::tolerant_i64;

/// Capture compaction data into the state file.
///
/// Writes compact_summary (if non-empty), compact_cwd (if present),
/// and increments compact_count. Requires compact_summary key in
/// hook_input to confirm this is a real PostCompact event.
pub fn capture_compact_data(hook_input: &Value, state_path: &Path) {
    if hook_input.get("compact_summary").is_none() {
        return;
    }

    let _ = mutate_state(state_path, &mut |state| {
        // Guard: state must be an object (or Null, which serde_json's
        // IndexMut auto-converts to an empty object) for string-key
        // mutations to succeed. Arrays, bools, numbers, and top-level
        // strings would panic on `state["key"] = v`. Fail-open on
        // any non-writable shape.
        if !(state.is_object() || state.is_null()) {
            return;
        }
        if let Some(summary) = hook_input.get("compact_summary").and_then(|v| v.as_str()) {
            if !summary.is_empty() {
                state["compact_summary"] = Value::String(summary.to_string());
            }
        }
        if let Some(cwd) = hook_input.get("cwd").and_then(|v| v.as_str()) {
            state["compact_cwd"] = Value::String(cwd.to_string());
        }
        // Accept compact_count stored as int, float, or string — state
        // files may carry any of these shapes from external edits or
        // legacy writers. All three resolve to the same canonical i64
        // increment instead of silently resetting to 1.
        let count = state.get("compact_count").map(tolerant_i64).unwrap_or(0);
        state["compact_count"] = Value::Number(count.saturating_add(1).into());
    });
}

/// Run the post-compact hook (entry point).
///
/// Resolves the stdin payload and the active flow's state file via the
/// shared `read_hook_input_and_state` helper, then captures compaction
/// data. `--branch` override support and the no-active-flow fail-open
/// posture live in the helper; this entry point owns only the
/// `project_root()` call and the `capture_compact_data` dispatch.
pub fn run() {
    let root = project_root();
    if let Some((hook_input, state_path)) = crate::hooks::read_hook_input_and_state(&root) {
        capture_compact_data(&hook_input, &state_path);
    }
}
