//! Shared entry-point boilerplate for the `_blocked` state mutators.
//!
//! `clear-blocked` and `set-blocked` share an identical hook entry
//! sequence — read and discard stdin, resolve the current branch, build
//! `FlowPaths`, and derive the state-file path — differing only in the
//! mutation they apply. This module owns that shared sequence so both
//! entry points reduce to "resolve the path, then mutate."
//!
//! Tests live at tests/commands/blocked_common.rs per
//! .claude/rules/test-placement.md — no inline #[cfg(test)] in this file.

use std::io::Read;
use std::path::PathBuf;

use crate::flow_paths::FlowPaths;
use crate::git::{current_branch, project_root};

/// Resolve the active flow's state-file path for the `_blocked` mutators.
///
/// Reads and discards stdin (the hook sends JSON context the `_blocked`
/// mutators do not consume), then resolves the current branch and builds
/// the state-file path. Returns `Some(state_path)` when a branch resolves
/// and passes `FlowPaths::try_new`.
///
/// Fail-open contract: every failure mode returns `None` so the calling
/// hook exits 0 without acting — no resolvable branch (detached HEAD) or a
/// `/`-containing branch that fails `FlowPaths::try_new`. This helper does
/// NOT check whether the state file exists; that guard stays inside
/// `set_blocked`/`clear_blocked` so each mutator owns its own
/// missing-file fail-open.
pub fn resolve_blocked_state_path() -> Option<PathBuf> {
    let mut _stdin = String::new();
    let _ = std::io::stdin().read_to_string(&mut _stdin);

    let branch = current_branch()?;
    let root = project_root();
    Some(FlowPaths::try_new(&root, &branch)?.state_file())
}
