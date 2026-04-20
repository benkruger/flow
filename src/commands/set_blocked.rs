//! Set the `_blocked` timestamp on the state file.
//!
//! Tests live at tests/set_blocked.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use std::io::Read;
use std::path::Path;

use serde_json::Value;

use crate::flow_paths::FlowPaths;
use crate::git::{current_branch, project_root};
use crate::lock::mutate_state;
use crate::utils::now;

/// Set _blocked flag in the state file. Fail-open: any error exits 0.
pub fn set_blocked(state_path: &Path) {
    if !state_path.exists() {
        return;
    }
    let _ = mutate_state(state_path, |state| {
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
pub fn run() {
    // Read stdin best-effort (hook sends JSON context)
    let mut _stdin = String::new();
    let _ = std::io::stdin().read_to_string(&mut _stdin);

    let branch = match current_branch() {
        Some(b) => b,
        None => return,
    };

    let root = project_root();
    let state_path = FlowPaths::new(&root, &branch).state_file();

    set_blocked(&state_path);
}
