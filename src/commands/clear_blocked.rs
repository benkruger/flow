//! Clear the `_blocked` timestamp from the state file.
//!
//! Tests live at tests/clear_blocked.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use std::io::Read;
use std::path::Path;

use crate::flow_paths::FlowPaths;
use crate::git::{current_branch, project_root};
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

    clear_blocked(&state_path);
}
