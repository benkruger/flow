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
/// Intentionally minimal — tab color writing is the only session-start
/// action. State file mutations and feature context injection belong in
/// phase-scoped commands, not session-wide hooks that run on every branch.
pub fn run() {
    let root = project_root();
    let detected = detect_repo(Some(&root));
    write_tab_colors(detected.as_deref(), &root);
}
