//! Cwd drift guard for state-mutating subcommands.
//!
//! When a flow is started from a subdirectory of a mono-repo, the
//! state file captures `relative_cwd` (e.g. `"api"` for a flow started
//! inside `<repo>/api/`). The skill cds the agent into
//! `<worktree>/<relative_cwd>` after worktree creation. Every
//! `bin/flow` subcommand then enforces that cwd against the captured
//! value via [`enforce`] — if the user has cd'd outside the expected
//! subdirectory, the subcommand hard-errors with a message naming the
//! expected directory.
//!
//! Tests live at tests/cwd_scope.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.
//!
//! # Why this matters
//!
//! Without the guard, a user who cds out of `api/` into `ios/` and runs
//! `bin/flow ci` would silently run CI for the wrong subdirectory of the
//! mono-repo. The guard catches the drift before any tool runs and tells
//! the user where they should be.
//!
//! # Backwards compatibility
//!
//! The guard is a no-op when:
//!
//! - `cwd` is not in a git worktree (no branch resolution)
//! - The current branch has no state file (no active FLOW flow)
//! - The state file's `relative_cwd` is empty (root-level flow) AND
//!   `cwd` equals the worktree root
//!
//! Existing flows that pre-date this field default to empty
//! `relative_cwd` and continue to work without modification.

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use crate::flow_paths::FlowPaths;

/// Enforce that `cwd` is inside (or equal to) the expected subdirectory
/// of the worktree for the current branch's flow.
///
/// Resolution order:
///
/// 1. Resolve the current branch from `cwd` via `current_branch_in`. If
///    detached HEAD or non-git, return Ok(()) (no enforcement).
/// 2. Read the state file at `<project_root>/.flow-states/<branch>.json`.
///    If missing, unreadable (e.g. directory at that path), or
///    unparseable, return Ok(()) (no active flow / fail-open).
/// 3. Read `relative_cwd` from the state file. Default to empty.
/// 4. Compute the worktree root via `git rev-parse --show-toplevel`.
///    current_branch_in succeeded so cwd is a live git-managed
///    directory — `--show-toplevel` succeeding is a hard invariant,
///    enforced via `.expect()`. Any failure here signals a race or a
///    broken git install.
/// 5. Compute `expected = <worktree_root>/<relative_cwd>` (just
///    `<worktree_root>` when empty).
/// 6. Canonicalize `cwd` and `expected` and check that `cwd` is inside
///    (or equal to) `expected`. If `cwd` is outside, return Err with
///    a message naming the expected directory.
///
/// The check is a prefix match on canonical paths, so descending into
/// subdirectories of `expected` is allowed (e.g. a root-level flow may
/// cd into any worktree directory; an `api`-scoped flow may cd into
/// `api/src/` but not into `ios/`).
///
/// `project_root` is the main repo root (where `.flow-states/` lives).
/// `cwd` is the subcommand's current working directory.
pub fn enforce(cwd: &Path, project_root: &Path) -> Result<(), String> {
    let branch = match crate::git::current_branch_in(cwd) {
        Some(b) => b,
        None => return Ok(()),
    };

    let state_path = FlowPaths::new(project_root, &branch).state_file();
    if !state_path.exists() {
        return Ok(());
    }

    let content = match fs::read_to_string(&state_path) {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    let state: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };

    let relative_cwd = state
        .get("relative_cwd")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // current_branch_in(cwd) succeeded above, so cwd is a live
    // git-managed directory: `git rev-parse --show-toplevel` must
    // succeed too. Any failure here is a race (cwd removed mid-call)
    // or a broken git install — neither is a production-supported
    // state, so treat as an invariant via `.expect()`.
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .expect("git rev-parse --show-toplevel must succeed in a git-managed cwd");
    let toplevel = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let worktree_root = PathBuf::from(toplevel);

    let expected = if relative_cwd.is_empty() {
        worktree_root.clone()
    } else {
        worktree_root.join(relative_cwd)
    };

    // cwd is the live git-managed directory — canonicalize succeeds.
    // expected may name a subdirectory that does not yet exist on
    // disk (e.g. relative_cwd="api/src" where api/src is not created);
    // fall back to the uncanonicalized expected so the prefix check
    // still reaches a conclusion.
    let cwd_canon = cwd
        .canonicalize()
        .expect("cwd is a live git-managed directory");
    let expected_canon = expected.canonicalize().unwrap_or_else(|_| expected.clone());

    if !cwd_canon.starts_with(&expected_canon) {
        return Err(format!(
            "cwd drift: expected {} (or a subdirectory), current {}. cd to the expected directory before running bin/flow commands.",
            expected_canon.display(),
            cwd_canon.display()
        ));
    }

    Ok(())
}
