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
/// running `git worktree list --porcelain`.
fn project_root_from_output(output: io::Result<Output>) -> PathBuf {
    match output {
        Ok(o) if o.status.success() => {
            project_root_with_stdout(&String::from_utf8_lossy(&o.stdout))
        }
        _ => PathBuf::from("."),
    }
}

/// Pure parser: take `git worktree list --porcelain` stdout and return
/// the first `worktree <path>` line as a PathBuf, or `PathBuf::from(".")`
/// when no such line is present.
fn project_root_with_stdout(stdout: &str) -> PathBuf {
    stdout
        .lines()
        .find_map(|line| {
            line.strip_prefix("worktree ")
                .map(|p| PathBuf::from(p.trim()))
        })
        .unwrap_or_else(|| PathBuf::from("."))
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
/// `git branch --show-current`.
fn current_branch_from_output(
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

/// Detect the integration branch (the branch FLOW pulls from, runs CI on,
/// pushes deps to, and targets with the PR `--base`).
///
/// Reads `git symbolic-ref --short refs/remotes/origin/HEAD` from the
/// given cwd. When the symbolic-ref is set (the normal state after
/// `git clone`), strips the `origin/` prefix and returns the branch
/// name.
///
/// Returns `Err(msg)` when git cannot resolve the integration branch
/// (no `origin` remote, no symbolic-ref configured, non-git directory,
/// git binary unavailable). Git is the single source of truth — callers
/// must propagate the failure rather than guess at a default. The error
/// message names the failure class so downstream error envelopes can
/// surface it to the user.
pub fn default_branch_in(cwd: &Path) -> Result<String, String> {
    default_branch_from_output(
        Command::new("git")
            .args(["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
            .current_dir(cwd)
            .output(),
    )
}

/// Pure helper for [`default_branch_in`]. On success, strips the
/// `origin/` prefix once and rejects an empty result; otherwise
/// returns the branch name. On any non-success path — spawn
/// failure (git binary missing) or non-zero exit (no `origin`
/// remote, symbolic-ref unset) — returns `Err` with a message
/// naming the failure class.
///
/// `strip_prefix` (not `trim_start_matches`) removes the
/// `origin/` prefix exactly once so `origin/origin/x` from a
/// misconfigured remote does not silently collapse to `x`. The
/// empty-after-strip rejection catches the one reachable malformed
/// shape: `git symbolic-ref` printing exactly `origin/` (without a
/// branch suffix) under a hand-crafted `update-ref` misconfiguration.
/// Other malformed shapes (path-traversal segments, leading dashes,
/// control characters) are unreachable because git itself rejects
/// such branch names at creation time. Git output is "Trusted but
/// external" per `.claude/rules/external-input-validation.md` —
/// the producer validates so every consumer can trust the value.
fn default_branch_from_output(output: io::Result<Output>) -> Result<String, String> {
    match output {
        Ok(o) if o.status.success() => {
            let raw = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let stripped = raw.strip_prefix("origin/").unwrap_or(&raw).to_string();
            if stripped.is_empty() {
                return Err(
                    "git symbolic-ref refs/remotes/origin/HEAD returned an empty branch name"
                        .to_string(),
                );
            }
            Ok(stripped)
        }
        Ok(o) => Err(format!(
            "git symbolic-ref refs/remotes/origin/HEAD failed (exit {}): {}",
            o.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(e) => Err(format!(
            "git symbolic-ref refs/remotes/origin/HEAD spawn failed: {}",
            e
        )),
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
/// reader the caller used); `override_branch` wins when present.
fn resolve_branch_impl(
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
