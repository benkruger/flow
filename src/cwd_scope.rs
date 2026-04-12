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

    let worktree_root = match worktree_root_for(cwd) {
        Some(r) => r,
        None => return Ok(()),
    };

    let expected = if relative_cwd.is_empty() {
        worktree_root.clone()
    } else {
        worktree_root.join(relative_cwd)
    };

    let cwd_canon = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
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

/// Compute the git worktree root for `cwd` via `git rev-parse --show-toplevel`.
///
/// Returns `None` for non-git directories or when git fails. The result
/// is the worktree's root directory (e.g. `.worktrees/<branch>`), not
/// the main repo root.
fn worktree_root_for(cwd: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .ok()?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn init_git_repo(dir: &Path, branch: &str) {
        let run = |args: &[&str]| {
            let output = Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .expect("git command failed");
            assert!(output.status.success(), "git {:?} failed", args);
        };
        run(&["init", "--initial-branch", branch]);
        run(&["config", "user.email", "test@test.com"]);
        run(&["config", "user.name", "Test"]);
        run(&["config", "commit.gpgsign", "false"]);
        run(&["commit", "--allow-empty", "-m", "init"]);
    }

    fn write_state(root: &Path, branch: &str, relative_cwd: &str) {
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let state = serde_json::json!({
            "branch": branch,
            "relative_cwd": relative_cwd,
        });
        fs::write(
            state_dir.join(format!("{}.json", branch)),
            state.to_string(),
        )
        .unwrap();
    }

    #[test]
    fn enforce_no_state_file_returns_ok() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path(), "main");
        // No state file
        let result = enforce(dir.path(), dir.path());
        assert!(result.is_ok(), "expected ok, got: {:?}", result);
    }

    #[test]
    fn enforce_non_git_dir_returns_ok() {
        let dir = tempfile::tempdir().unwrap();
        // No git init
        let result = enforce(dir.path(), dir.path());
        assert!(result.is_ok(), "expected ok, got: {:?}", result);
    }

    #[test]
    fn enforce_empty_relative_cwd_at_worktree_root_returns_ok() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path(), "feature-x");
        write_state(dir.path(), "feature-x", "");
        // cwd is the worktree root, relative_cwd is empty → ok
        let result = enforce(dir.path(), dir.path());
        assert!(result.is_ok(), "expected ok, got: {:?}", result);
    }

    #[test]
    fn enforce_empty_relative_cwd_in_subdir_returns_ok() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path(), "feature-x");
        write_state(dir.path(), "feature-x", "");
        let subdir = dir.path().join("api");
        fs::create_dir(&subdir).unwrap();
        // cwd is api/, relative_cwd is empty → ok because api/ is a
        // descendant of the worktree root (prefix match).
        let result = enforce(&subdir, dir.path());
        assert!(result.is_ok(), "expected ok, got: {:?}", result);
    }

    #[test]
    fn enforce_relative_cwd_descendant_returns_ok() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path(), "feature-x");
        write_state(dir.path(), "feature-x", "api");
        let nested = dir.path().join("api").join("src");
        fs::create_dir_all(&nested).unwrap();
        // cwd is api/src/, relative_cwd is "api" → ok (descendant)
        let result = enforce(&nested, dir.path());
        assert!(result.is_ok(), "expected ok, got: {:?}", result);
    }

    #[test]
    fn enforce_relative_cwd_matches_subdir_returns_ok() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path(), "feature-x");
        write_state(dir.path(), "feature-x", "api");
        let subdir = dir.path().join("api");
        fs::create_dir(&subdir).unwrap();
        // cwd is api/, relative_cwd is "api" → ok
        let result = enforce(&subdir, dir.path());
        assert!(result.is_ok(), "expected ok, got: {:?}", result);
    }

    #[test]
    fn enforce_relative_cwd_mismatch_errors() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path(), "feature-x");
        write_state(dir.path(), "feature-x", "api");
        let ios = dir.path().join("ios");
        fs::create_dir(&ios).unwrap();
        // cwd is ios/, relative_cwd is "api" → error naming "api"
        let result = enforce(&ios, dir.path());
        assert!(result.is_err(), "expected error, got: {:?}", result);
        let msg = result.unwrap_err();
        assert!(
            msg.contains("api"),
            "error should name expected directory: {}",
            msg
        );
    }

    #[test]
    fn enforce_nested_relative_cwd_returns_ok() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path(), "feature-x");
        write_state(dir.path(), "feature-x", "packages/api");
        let nested = dir.path().join("packages").join("api");
        fs::create_dir_all(&nested).unwrap();
        let result = enforce(&nested, dir.path());
        assert!(result.is_ok(), "expected ok, got: {:?}", result);
    }

    #[test]
    fn enforce_relative_cwd_at_worktree_root_errors() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path(), "feature-x");
        write_state(dir.path(), "feature-x", "api");
        // cwd is the worktree root but relative_cwd says "api" → error
        let result = enforce(dir.path(), dir.path());
        assert!(result.is_err(), "expected error, got: {:?}", result);
    }

    #[test]
    fn enforce_corrupt_state_file_returns_ok() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path(), "feature-x");
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(state_dir.join("feature-x.json"), "not json").unwrap();
        // Corrupt state file → no enforcement (don't block)
        let result = enforce(dir.path(), dir.path());
        assert!(result.is_ok(), "expected ok, got: {:?}", result);
    }

    #[test]
    fn enforce_missing_relative_cwd_field_treats_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path(), "feature-x");
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        // Pre-existing state file from before relative_cwd was added
        fs::write(
            state_dir.join("feature-x.json"),
            r#"{"branch": "feature-x"}"#,
        )
        .unwrap();
        // No relative_cwd field → treat as empty → cwd at root is ok
        let result = enforce(dir.path(), dir.path());
        assert!(result.is_ok(), "expected ok, got: {:?}", result);
    }
}
