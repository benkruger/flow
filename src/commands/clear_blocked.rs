//! Clear the `_blocked` timestamp from the state file.
//!
//! Tests live at tests/commands/clear_blocked.rs per
//! .claude/rules/test-placement.md — no inline #[cfg(test)] in this file.

use std::path::Path;

use crate::commands::blocked_common::resolve_blocked_state_path;
use crate::lock::mutate_state;

/// Clear _blocked flag from the state file. Fail-open: any error exits 0.
pub fn clear_blocked(state_path: &Path) {
    if !state_path.exists() {
        return;
    }
    let _ = mutate_state(state_path, &mut |state| {
        if let Some(obj) = state.as_object_mut() {
            obj.remove("_blocked");
        }
    });
}

/// Run the clear-blocked command (hook entry point).
///
/// Resolves the state-file path via the shared
/// `resolve_blocked_state_path` helper, then clears the `_blocked`
/// flag. The stdin read, branch resolution, and no-active-flow
/// fail-open posture live in the helper.
pub fn run() {
    if let Some(state_path) = resolve_blocked_state_path() {
        clear_blocked(&state_path);
    }
}
