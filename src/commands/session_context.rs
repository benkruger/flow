use std::path::Path;

use crate::git::project_root;
use crate::github::detect_repo;
use crate::utils::write_tab_sequences;

/// Write tab colors best-effort — errors are silently ignored.
fn write_tab_colors(repo: Option<&str>, root: &Path) {
    let _ = write_tab_sequences(repo, Some(root));
}

/// Session-start hook: detect repo and write terminal tab colors.
///
/// Previously this module contained ~600 lines that scanned all state
/// files, mutated timing fields, consumed transient data, and injected
/// feature context into the session. When a session opened on main
/// (no matching state file), `filter_by_branch()` fell back to ALL
/// state files — corrupting every active flow's timing and transient
/// fields. Removed in PR #938; only tab color writing survives.
pub fn run() {
    let root = project_root();
    let detected = detect_repo(Some(&root));
    write_tab_colors(detected.as_deref(), &root);
}
