//! PostCompact hook: capture compaction context into the state file.
//!
//! Tests live at tests/post_compact.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use std::io::Read;
use std::path::Path;

use serde_json::Value;

use crate::flow_paths::FlowPaths;
use crate::git::{project_root, resolve_branch};
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
/// Uses `resolve_branch` for `--branch` override support. Calls
/// `current_branch()` internally — does not scan `.flow-states/`.
pub fn run() {
    let mut stdin_buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut stdin_buf);

    let hook_input: Value = match serde_json::from_str(&stdin_buf) {
        Ok(v) => v,
        Err(_) => return,
    };

    let root = project_root();
    let branch = resolve_branch(None, &root);
    let branch = match branch {
        Some(b) => b,
        None => return,
    };

    // Slash-containing git branches are not valid FLOW branches —
    // treat as "no active flow" and return rather than panicking.
    let state_path = match FlowPaths::try_new(&root, &branch) {
        Some(p) => p.state_file(),
        None => return,
    };

    if !state_path.exists() {
        return;
    }

    capture_compact_data(&hook_input, &state_path);
}
