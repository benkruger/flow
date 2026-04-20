use std::env;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::flow_paths::FlowPaths;

/// Find the main git repository root.
///
/// Uses `git worktree list --porcelain` to find the root, which works
/// correctly whether run from the project root or from inside a worktree.
/// Falls back to `.` if git fails, is not installed, or the current
/// directory is not inside a git repository.
pub fn project_root() -> PathBuf {
    project_root_from_output(
        Command::new("git")
            .args(["worktree", "list", "--porcelain"])
            .output(),
    )
}

/// Pure helper for [`project_root`]: interpret the raw result of
/// running `git worktree list --porcelain`. Tests can construct each
/// variant (spawn error, non-zero exit, success with varied stdout)
/// directly.
pub fn project_root_from_output(output: io::Result<Output>) -> PathBuf {
    match output {
        Ok(o) if o.status.success() => {
            project_root_with_stdout(&String::from_utf8_lossy(&o.stdout))
        }
        _ => PathBuf::from("."),
    }
}

/// Pure parser: take `git worktree list --porcelain` stdout and return
/// the first `worktree <path>` line as a PathBuf, or `PathBuf::from(".")`
/// when no such line is present. Empty input behaves the same as "no
/// worktree line".
pub fn project_root_with_stdout(stdout: &str) -> PathBuf {
    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            return PathBuf::from(path.trim());
        }
    }
    PathBuf::from(".")
}

/// Get the current git branch name.
///
/// Returns None if not on a branch (e.g. detached HEAD) or if git fails.
///
/// If FLOW_SIMULATE_BRANCH is set (and non-empty) in the environment,
/// returns that value instead of querying git. Used by `bin/flow ci
/// --simulate-branch`.
pub fn current_branch() -> Option<String> {
    current_branch_from_output(
        env::var("FLOW_SIMULATE_BRANCH").ok(),
        Command::new("git")
            .args(["branch", "--show-current"])
            .output(),
    )
}

/// Get the current git branch name from a specific working directory.
///
/// Like [`current_branch`] but runs `git branch --show-current` with
/// `.current_dir(cwd)` so tests can point at a fixture repo without
/// mutating the test process cwd. Returns None for detached HEAD,
/// non-git directories, or git failures.
///
/// Unlike [`current_branch`], this helper does NOT consult the
/// FLOW_SIMULATE_BRANCH env var. Callers that need simulate-branch
/// semantics must layer it on top.
pub fn current_branch_in(cwd: &Path) -> Option<String> {
    current_branch_from_output(
        None,
        Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(cwd)
            .output(),
    )
}

/// Pure helper for [`current_branch`] and [`current_branch_in`].
/// `simulated` is the `FLOW_SIMULATE_BRANCH` env var value (empty string
/// falls through); `output` is the raw `io::Result<Output>` from
/// `git branch --show-current`. Tests can construct every variant
/// directly — simulated hit, empty simulated, None simulated, git
/// spawn-failed, git non-zero, git success with populated or empty
/// stdout.
pub fn current_branch_from_output(
    simulated: Option<String>,
    output: io::Result<Output>,
) -> Option<String> {
    if let Some(ref s) = simulated {
        if !s.is_empty() {
            return Some(s.clone());
        }
    }
    let out = output.ok()?;
    if !out.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if branch.is_empty() {
        None
    } else {
        Some(branch)
    }
}

/// Resolve which branch's state file to use.
///
/// Resolution order:
/// 1. If override provided, return it immediately
/// 2. If current_branch() matches a state file, return it
/// 3. Return current_branch() anyway (callers check state file existence)
///
/// Never scans `.flow-states/` for candidates — each caller targets only
/// its own branch.
pub fn resolve_branch(override_branch: Option<&str>, root: &Path) -> Option<String> {
    resolve_branch_impl(override_branch, root, current_branch())
}

/// Cwd-scoped variant of [`resolve_branch`] that uses [`current_branch_in`]
/// instead of [`current_branch`].
///
/// This is the correct choice for CLI subcommands that resolve a branch
/// from an explicit working directory (e.g., the `ci` subcommand running
/// in a worktree) where the branch must be read from the given cwd, not
/// the process's cwd.
pub fn resolve_branch_in(override_branch: Option<&str>, cwd: &Path, root: &Path) -> Option<String> {
    resolve_branch_impl(override_branch, root, current_branch_in(cwd))
}

/// Pure resolution for [`resolve_branch`] and [`resolve_branch_in`].
/// `branch` is the current-branch value (already resolved by whichever
/// reader the caller used); `override_branch` wins when present. Exposed
/// publicly so tests can drive every branch without spawning git.
pub fn resolve_branch_impl(
    override_branch: Option<&str>,
    root: &Path,
    branch: Option<String>,
) -> Option<String> {
    if let Some(b) = override_branch {
        return Some(b.to_string());
    }

    // Exact match — current branch has a state file. `try_new` filters
    // out slash-containing branches (`feature/foo`, `dependabot/*`)
    // which git permits but FLOW's flat state-file layout cannot
    // address; those branches skip the exact-match check and fall
    // through to the "return it anyway" path below.
    if let Some(ref b) = branch {
        if let Some(paths) = FlowPaths::try_new(root, b) {
            if paths.state_file().exists() {
                return Some(b.clone());
            }
        }
    }

    // No state file for current branch — return it anyway
    // (callers check state file existence separately)
    branch
}
