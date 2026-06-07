//! Shared utilities for PreToolUse hook validators.
//!
//! These hooks fire on every tool call during a session, so they must be fast.
//! All functions avoid subprocess calls where possible, using filesystem-based
//! detection instead.
//!
//! Tests live at tests/hooks/shared.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use regex::Regex;
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::flow_paths::FlowPaths;

/// Marker directory name for FLOW worktrees.
const WORKTREE_MARKER: &str = ".worktrees/";

/// Find `.claude/settings.json` by walking up from a starting directory.
pub fn find_settings_and_root_from(start: &Path) -> (Option<Value>, Option<PathBuf>) {
    let mut current = start.to_path_buf();
    loop {
        let settings_path = current.join(".claude").join("settings.json");
        if settings_path.is_file() {
            match std::fs::read_to_string(&settings_path) {
                Ok(content) => match serde_json::from_str::<Value>(&content) {
                    Ok(val) => return (Some(val), Some(current)),
                    Err(_) => return (None, None),
                },
                Err(_) => return (None, None),
            }
        }
        if !current.pop() {
            break;
        }
    }
    (None, None)
}

/// Detect the current branch name from an explicit working directory path.
///
/// Worktree-path invariants (cited below via `.expect`):
///   * `worktrees_dir` always contains `.worktrees`, so its `parent()`
///     is always Some — it can never be `/` or an empty path.
///   * `cwd` contains `.worktrees/` textually, so `current` starts
///     as a descendant of `worktrees_dir`. Walking up via
///     `.parent()` reduces the path one level at a time, so
///     `current` is guaranteed to reach `worktrees_dir` — the
///     single loop guard `current != *worktrees_dir` is sufficient.
///     `strip_prefix(worktrees_dir)` always succeeds on the
///     in-body use because the body runs only while `current` is
///     still a strict descendant; `current.parent()` likewise
///     always returns Some.
///
/// Per `.claude/rules/testability-means-simplicity.md`, `.expect`
/// does not create an instrumented branch, so these
/// provably-unreachable error arms are collapsed at the source.
pub fn detect_branch_from_path(cwd: &Path) -> Option<String> {
    let cwd_str = cwd.to_string_lossy();
    if let Some(marker_pos) = cwd_str.find(WORKTREE_MARKER) {
        let worktrees_dir_str = &cwd_str[..marker_pos + WORKTREE_MARKER.len()];
        let worktrees_dir = Path::new(worktrees_dir_str.trim_end_matches('/'));

        let mut current = cwd.to_path_buf();
        while current != *worktrees_dir {
            if current.join(".git").is_file() {
                let branch = current
                    .strip_prefix(worktrees_dir)
                    .expect("current is a descendant of worktrees_dir per loop invariant")
                    .to_string_lossy()
                    .to_string();
                return Some(branch);
            }
            current = current
                .parent()
                .expect("current is strictly deeper than worktrees_dir per loop guard")
                .to_path_buf();
        }
    }

    // Fallback to git subprocess (using provided path as CWD)
    let output = match std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(cwd)
        .output()
    {
        Ok(o) => o,
        Err(_) => return None,
    };
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        None
    } else {
        Some(branch)
    }
}

/// Check if a FLOW feature is active for the given branch.
///
/// Returns `true` when `.flow-states/<branch>/state.json` exists at
/// the given root and the branch passes `FlowPaths::is_valid_branch`.
/// Invalid branch names (empty, `.`, `..`, slash- or NUL-containing,
/// or backslash-containing on Windows shells) return `false` —
/// they cannot identify an active flow under the subdirectory layout.
pub fn is_flow_active(branch: &str, root: &Path) -> bool {
    if branch.contains('\\') {
        return false;
    }
    match FlowPaths::try_new(root, branch) {
        Some(paths) => paths.state_file().is_file(),
        None => false,
    }
}

/// Resolve the main repo root when inside a worktree.
///
/// In a worktree at `<project>/.worktrees/<branch>/`, returns the path
/// before `.worktrees/`. Otherwise returns the input path unchanged.
pub fn resolve_main_root(project_root: &Path) -> PathBuf {
    let root_str = project_root.to_string_lossy();
    if let Some(marker_pos) = root_str.find(WORKTREE_MARKER) {
        PathBuf::from(&root_str[..marker_pos])
    } else {
        project_root.to_path_buf()
    }
}

/// Convert a `Bash(pattern)` permission entry to a compiled regex.
///
/// `Bash(git push)` → `^git push$`
/// `Bash(git push *)` → `^git push .*$`
///
/// Returns `None` for non-`Bash(...)` entries.
pub fn permission_to_regex(perm: &str) -> Option<Regex> {
    let inner = perm.strip_prefix("Bash(")?.strip_suffix(')')?;
    let escaped = regex::escape(inner).replace(r"\*", ".*");
    Regex::new(&format!("^{}$", escaped)).ok()
}

/// Extract `Bash(...)` patterns from settings and compile to regexes.
pub fn build_permission_regexes(settings: &Value, list_key: &str) -> Vec<Regex> {
    let entries = settings
        .get("permissions")
        .and_then(|p| p.get(list_key))
        .and_then(|v| v.as_array());

    match entries {
        Some(arr) => arr
            .iter()
            .filter_map(|e| e.as_str())
            .filter_map(permission_to_regex)
            .collect(),
        None => vec![],
    }
}

/// Read JSON from stdin. Returns None on parse failure (fail-open).
///
/// A stdin read failure falls through to empty-string parsing rather
/// than short-circuiting: `serde_json::from_str("")` returns `Err`
/// which `.ok()?` collapses to `None` — same observable result as the
/// early return used to, without the separate branch.
pub fn read_hook_input() -> Option<Value> {
    let mut input = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
    serde_json::from_str(&input).ok()
}

/// Read the hook's stdin payload and resolve the active flow's state file.
///
/// Composes `read_hook_input()` (stdin read + JSON parse) with branch
/// resolution and `FlowPaths` construction. Returns `Some((input,
/// state_path))` only when the parsed payload, a resolved branch, a
/// valid `FlowPaths`, and an existing state file all succeed.
///
/// Fail-open contract: every failure mode returns `None` so the calling
/// hook exits 0 without acting — unparseable or absent stdin, no
/// resolvable branch (detached HEAD), a `/`-containing branch that
/// fails `FlowPaths::try_new`, or a missing state file. The caller owns
/// the `project_root()` call and passes `root` in; `resolve_branch`
/// preserves `--branch` override support.
pub fn read_hook_input_and_state(root: &Path) -> Option<(Value, PathBuf)> {
    let input = read_hook_input()?;
    let branch = crate::git::resolve_branch(None, root)?;
    let state_path = FlowPaths::try_new(root, &branch)?.state_file();
    if !state_path.exists() {
        return None;
    }
    Some((input, state_path))
}

/// Resolve the working directory a hook should reason about.
///
/// Returns the payload `cwd` field when present and non-empty, else
/// falls back to `std::env::current_dir()`.
///
/// The payload `cwd` is the authoritative source: Claude Code sends
/// the session's (and a sub-agent's) working directory in the hook
/// payload, which during an active flow is the worktree. The hook
/// subprocess's own `std::env::current_dir()` is the directory Claude
/// Code spawned the hook in, which can resolve to the main repo root
/// rather than the worktree — when it does, worktree-derived gates
/// (`compute_worktree_paths` and branch detection) silently see the
/// wrong directory and self-disable. Reading the payload `cwd` first
/// keeps those gates anchored to the worktree; the `env::current_dir()`
/// fallback preserves behavior when no payload `cwd` is supplied.
///
/// An empty `cwd` string is treated as absent (filtered) so a
/// degenerate payload falls back rather than producing an empty path.
pub fn resolve_hook_cwd(hook_input: &Value) -> Option<String> {
    hook_input
        .get("cwd")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        })
}

pub mod agent_prompt_scan;
pub mod agent_run_record;
pub mod capture_session;
pub mod post_compact;
pub mod stop_continue;
pub mod stop_failure;
pub mod transcript_walker;
pub mod validate_ask_user;
pub mod validate_claude_paths;
pub mod validate_pretool;
pub mod validate_skill;
pub mod validate_worktree_paths;
