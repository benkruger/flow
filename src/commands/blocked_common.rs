//! Shared entry-point boilerplate for the `_blocked` state mutators.
//!
//! `clear-blocked` and `set-blocked` share an identical hook entry
//! sequence — read and discard stdin, resolve the current branch, build
//! `FlowPaths`, and derive the state-file path — differing only in the
//! mutation they apply. This module owns that shared sequence so both
//! entry points reduce to "resolve the path, then mutate."
//!
//! Covered transitively through the `clear-blocked` / `set-blocked`
//! entry points — `tests/commands/clear_blocked.rs` and
//! `tests/commands/set_blocked.rs` drive `clear_blocked::run()` /
//! `set_blocked::run()`, which delegate here — per
//! .claude/rules/test-placement.md public-interface testing. There is
//! no dedicated mirror test file and no inline #[cfg(test)] in this file.

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
///
/// This helper resolves `project_root()` internally rather than taking
/// it as a parameter, unlike the hooks' `read_hook_input_and_state`:
/// the `_blocked` mutators have no `--branch`/root-injection seam, so
/// there is no caller-supplied root to thread through. The two helpers
/// are deliberately separate: `read_hook_input_and_state` uses
/// `resolve_branch` (with `--branch` override) and parses stdin into a
/// `Value`, while this one uses `current_branch` and discards stdin —
/// not a single merged abstraction.
pub fn resolve_blocked_state_path() -> Option<PathBuf> {
    let mut _stdin = String::new();
    let _ = std::io::stdin().read_to_string(&mut _stdin);

    let branch = current_branch()?;
    let root = project_root();
    Some(FlowPaths::try_new(&root, &branch)?.state_file())
}
