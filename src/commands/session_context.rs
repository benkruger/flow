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
/// All other functionality (timing reset, transient field consumption,
/// feature context injection, orchestrate detection) was removed in
/// PR #938 to eliminate cross-flow state corruption when sessions
/// open on main.
pub fn run() {
    let root = project_root();
    let detected = detect_repo(Some(&root));
    write_tab_colors(detected.as_deref(), &root);
}
