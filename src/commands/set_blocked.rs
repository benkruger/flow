//! Set the `_blocked` timestamp on the state file.
//!
//! Tests live at tests/commands/set_blocked.rs per
//! .claude/rules/test-placement.md — no inline #[cfg(test)] in this file.

use std::path::Path;

use serde_json::Value;

use crate::commands::blocked_common::resolve_blocked_state_path;
use crate::lock::mutate_state;
use crate::utils::now;

/// Set _blocked flag in the state file. Fail-open: any error exits 0.
pub fn set_blocked(state_path: &Path) {
    if !state_path.exists() {
        return;
    }
    let _ = mutate_state(state_path, &mut |state| {
        // Guard: state must be an object (or Null, which auto-converts)
        // for string-key mutations. Arrays and primitives would panic.
        // Fail-open on any non-writable shape.
        if !(state.is_object() || state.is_null()) {
            return;
        }
        state["_blocked"] = Value::String(now());
    });
}

/// Run the set-blocked command (hook entry point).
///
/// Resolves the state-file path via the shared
/// `resolve_blocked_state_path` helper, then sets the `_blocked`
/// flag. The stdin read, branch resolution, and no-active-flow
/// fail-open posture live in the helper.
pub fn run() {
    if let Some(state_path) = resolve_blocked_state_path() {
        set_blocked(&state_path);
    }
}
