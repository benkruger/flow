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
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::flow_paths::FlowPaths;

/// Enforce that `cwd` is inside (or equal to) the expected subdirectory
/// of the worktree for the current branch's flow.
///
/// Resolution order:
///
/// 1. Resolve the current branch from `cwd`. If detached HEAD or
///    non-git, return Ok(()) (no enforcement).
/// 2. Read the state file at `<project_root>/.flow-states/<branch>.json`.
///    If missing or unparseable, return Ok(()) (no active flow).
/// 3. Read `relative_cwd` from the state file. Default to empty.
/// 4. Compute the worktree root via `git rev-parse --show-toplevel` from
///    `cwd`. If git fails, return Ok(()) (can't determine worktree).
/// 5. Compute expected = `<worktree_root>/<relative_cwd>` (just
///    `<worktree_root>` when empty).
/// 6. Canonicalize both `cwd` and `expected` and check that `cwd` is
///    inside (or equal to) `expected`. If `cwd` is outside, return Err
///    with a message naming the expected directory.
///
/// The check is a prefix match on canonical paths, so descending into
/// subdirectories of `expected` is allowed (e.g. a root-level flow may
/// cd into any worktree directory; an `api`-scoped flow may cd into
/// `api/src/` but not into `ios/`).
///
/// `project_root` is the main repo root (where `.flow-states/` lives).
/// `cwd` is the subcommand's current working directory.
pub fn enforce(cwd: &Path, project_root: &Path) -> Result<(), String> {
    enforce_with_deps(
        cwd,
        project_root,
        &crate::git::current_branch_in,
        &worktree_root_for,
    )
}

/// Seam-injected variant of [`enforce`] that accepts custom resolvers
/// for the branch-from-cwd and worktree-root-from-cwd lookups.
/// Production passes `current_branch_in` and `worktree_root_for`;
/// tests substitute closures to exercise each fail-open branch.
pub fn enforce_with_deps(
    cwd: &Path,
    project_root: &Path,
    branch_resolver: &dyn Fn(&Path) -> Option<String>,
    worktree_root_resolver: &dyn Fn(&Path) -> Option<PathBuf>,
) -> Result<(), String> {
    let branch = match branch_resolver(cwd) {
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

    // current_branch_in(cwd) succeeded, so cwd is a git-managed
    // directory: `git rev-parse --show-toplevel` is expected to succeed
    // too. If it doesn't (transient git failure, network filesystem
    // hiccup), treat it the same as "no active flow" and skip
    // enforcement — consistent with the fail-open posture above.
    let worktree_root = match worktree_root_resolver(cwd) {
        Some(r) => r,
        None => return Ok(()),
    };

    let expected = if relative_cwd.is_empty() {
        worktree_root.clone()
    } else {
        worktree_root.join(relative_cwd)
    };

    // Canonicalize both paths with `unwrap_or` fallback: cwd.canonicalize()
    // always succeeds in practice (current_branch_in succeeded so cwd is
    // a live git directory), while expected.canonicalize() may fail when
    // relative_cwd names a subdirectory that does not yet exist on disk.
    // Both fall-back branches preserve the prefix-check invariant.
    let cwd_canon = match cwd.canonicalize() {
        Ok(p) => p,
        Err(_) => cwd.to_path_buf(),
    };
    let expected_canon = match expected.canonicalize() {
        Ok(p) => p,
        Err(_) => expected.clone(),
    };

    if !cwd_canon.starts_with(&expected_canon) {
        return Err(format!(
            "cwd drift: expected {} (or a subdirectory), current {}. cd to the expected directory before running bin/flow commands.",
            expected_canon.display(),
            cwd_canon.display()
        ));
    }

    Ok(())
}

/// Compute the git worktree root for `cwd` via `git rev-parse --show-toplevel`.
///
/// Returns `None` for non-git directories or when git fails. The result
/// is the worktree's root directory (e.g. `.worktrees/<branch>`), not
/// the main repo root.
pub fn worktree_root_for(cwd: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output();
    parse_worktree_root(output)
}

/// Parse the `git rev-parse --show-toplevel` subprocess output into
/// a worktree root PathBuf. Exposed for tests: each of the three
/// fail paths (spawn error, non-zero exit, empty stdout) is driven
/// by a constructed `std::io::Result<Output>` without needing git
/// subprocess control.
pub fn parse_worktree_root(output: std::io::Result<std::process::Output>) -> Option<PathBuf> {
    let output = output.ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}
