//! Shared utilities for PreToolUse hook validators.
//!
//! These hooks fire on every tool call during a session, so they must be fast.
//! All functions avoid subprocess calls where possible, using filesystem-based
//! detection instead.

use regex::Regex;
use serde_json::Value;
use std::env;
use std::path::{Path, PathBuf};

use crate::flow_paths::FlowPaths;

/// Marker directory name for FLOW worktrees.
const WORKTREE_MARKER: &str = ".worktrees/";

/// Commands that have dedicated tool alternatives.
pub const FILE_READ_COMMANDS: &[&str] = &["cat", "head", "tail", "grep", "rg", "find", "ls"];

/// Find `.claude/settings.json` by walking up from CWD.
///
/// Returns `(settings, project_root)` where `project_root` is the directory
/// containing `.claude/`. Returns `(None, None)` if not found or unparseable.
pub fn find_settings_and_root() -> (Option<Value>, Option<PathBuf>) {
    find_settings_and_root_from(&env::current_dir().unwrap_or_default())
}

/// Testable version that takes an explicit starting directory.
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

/// Detect the current branch name from the working directory path.
///
/// In a worktree (`.worktrees/<branch>/`), walks up from the given path to find
/// the worktree root (directory containing a `.git` file), then extracts the
/// branch name as the relative path from `.worktrees/` to that root.
///
/// Falls back to `git branch --show-current` when not in a worktree.
///
/// Returns `None` if not on a branch or if detection fails.
pub fn detect_branch_from_cwd() -> Option<String> {
    detect_branch_from_path(&env::current_dir().ok()?)
}

/// Testable version that takes an explicit path.
pub fn detect_branch_from_path(cwd: &Path) -> Option<String> {
    let cwd_str = cwd.to_string_lossy();
    if let Some(marker_pos) = cwd_str.find(WORKTREE_MARKER) {
        let worktrees_dir_str = &cwd_str[..marker_pos + WORKTREE_MARKER.len()];
        let worktrees_dir = Path::new(worktrees_dir_str.trim_end_matches('/'));

        let mut current = cwd.to_path_buf();
        while current != *worktrees_dir && current.parent() != Some(worktrees_dir.parent()?) {
            if current.join(".git").is_file() {
                let branch = current
                    .strip_prefix(worktrees_dir)
                    .ok()?
                    .to_string_lossy()
                    .to_string();
                return if branch == "." || branch.is_empty() {
                    None
                } else {
                    Some(branch)
                };
            }
            current = current.parent()?.to_path_buf();
        }
    }

    // Fallback to git subprocess (using provided path as CWD)
    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(cwd)
        .output()
        .ok()?;
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
/// Returns `true` when `.flow-states/<branch>.json` exists at the given root.
/// Rejects branch names containing path separators to prevent traversal.
pub fn is_flow_active(branch: &str, root: &Path) -> bool {
    if branch.is_empty() || branch.contains('/') || branch.contains('\\') {
        return false;
    }
    let state_file = FlowPaths::new(root, branch).state_file();
    state_file.is_file()
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
pub fn read_hook_input() -> Option<Value> {
    let mut input = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut input).ok()?;
    serde_json::from_str(&input).ok()
}

pub mod post_compact;
pub mod stop_continue;
pub mod stop_failure;
pub mod validate_ask_user;
pub mod validate_claude_paths;
pub mod validate_pretool;
pub mod validate_worktree_paths;

#[cfg(test)]
mod tests {
    use super::*;

    /// Covers the `Err(_) => return (None, None)` arm on line 39 of
    /// `find_settings_and_root_from`: `is_file()` returns true but
    /// `fs::read_to_string` fails. Triggered by a `.claude/settings.json`
    /// entry that passes `is_file()` (it's a regular file metadata-wise
    /// on macOS when the path is a broken symlink targeting a non-file
    /// — not straightforward). Easiest reliable trigger on Unix is a
    /// directory placed where the file is expected, combined with a
    /// path that tricks `is_file()` — in practice we use a path whose
    /// name matches `.claude/settings.json` but whose parent makes the
    /// read fail with EISDIR.
    ///
    /// A simpler deterministic trigger: create the settings file as
    /// unreadable (`chmod 000`). On macOS, `is_file()` returns true
    /// for metadata, and `read_to_string` fails with EACCES.
    #[test]
    fn find_settings_read_failure_returns_none_none() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join(".claude");
        fs::create_dir(&claude).unwrap();
        let settings = claude.join("settings.json");
        fs::write(&settings, "{}").unwrap();
        // Strip all permission bits so the read fails while
        // `is_file()` still sees a regular file.
        fs::set_permissions(&settings, fs::Permissions::from_mode(0o000)).unwrap();

        let (val, root) = find_settings_and_root_from(dir.path());
        // Restore permissions for tempdir cleanup on drop.
        let _ = fs::set_permissions(&settings, fs::Permissions::from_mode(0o644));
        assert!(val.is_none());
        assert!(root.is_none());
    }

    /// Covers the git fallback None arm when `git branch --show-current`
    /// returns empty stdout (e.g. detached HEAD). Hard to simulate
    /// deterministically without either (a) a stubbed git on PATH or
    /// (b) an actual detached-HEAD fixture. We construct (b) via
    /// `git checkout --detach HEAD` in a fresh repo.
    #[test]
    fn detect_branch_from_path_detached_head_returns_none() {
        use std::process::Command;
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let run = |args: &[&str]| {
            let out = Command::new("git")
                .args(args)
                .current_dir(repo)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {:?}: {:?}", args, out);
        };
        run(&["init", "--initial-branch", "main"]);
        run(&["config", "user.email", "t@t.com"]);
        run(&["config", "user.name", "T"]);
        run(&["config", "commit.gpgsign", "false"]);
        run(&["commit", "--allow-empty", "-m", "init"]);
        // Detach HEAD so `git branch --show-current` returns empty.
        run(&["checkout", "--detach", "HEAD"]);

        assert_eq!(detect_branch_from_path(repo), None);
    }

    // Plan-named coverage tests for issue #1145 (Task 2). `is_flow_active`
    // has three fail-closed rejection arms — empty branch, slash character,
    // and backslash character — plus a final `state_file.is_file()` gate.
    // Each arm gets a named test so a future refactor cannot silently
    // weaken one guard while the others still reject their own malformed
    // input. The backslash variant is the only way to exercise the
    // `branch.contains('\\')` arm; no other test drives it.

    #[test]
    fn is_flow_active_empty_branch_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_flow_active("", dir.path()));
    }

    #[test]
    fn is_flow_active_slash_branch_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_flow_active("feature/foo", dir.path()));
    }

    #[test]
    fn is_flow_active_backslash_branch_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_flow_active("a\\b", dir.path()));
    }

    #[test]
    fn is_flow_active_valid_branch_no_state_file_returns_false() {
        // Branch name passes all rejection guards but no state file exists,
        // so `state_file.is_file()` returns false.
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_flow_active("feat-branch", dir.path()));
    }

    #[test]
    fn is_flow_active_valid_branch_with_state_file_returns_true() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let paths = FlowPaths::new(dir.path(), "feat-branch");
        fs::create_dir_all(paths.state_file().parent().unwrap()).unwrap();
        fs::write(paths.state_file(), "{}").unwrap();
        assert!(is_flow_active("feat-branch", dir.path()));
    }

    /// Covers the `Err(_) => return (None, None)` arm on line 37 of
    /// `find_settings_and_root_from`: `is_file()` succeeds and
    /// `read_to_string` returns Ok, but `serde_json::from_str` fails
    /// because the file is not valid JSON. The sibling test
    /// `find_settings_read_failure_returns_none_none` covers the
    /// outer `Err` arm (EACCES on read); this pins the inner JSON-
    /// parse-failure arm which the subprocess integration tests
    /// never reach because they always write syntactically valid
    /// settings.
    #[test]
    fn find_settings_invalid_json_returns_none_none() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join(".claude");
        fs::create_dir(&claude).unwrap();
        fs::write(claude.join("settings.json"), "{not valid json").unwrap();

        let (val, root) = find_settings_and_root_from(dir.path());
        assert!(val.is_none());
        assert!(root.is_none());
    }

    /// Covers the `resolve_main_root` branch that trims a worktree
    /// suffix off a project_root path — only the no-marker arm was
    /// exercised by the existing suite.
    #[test]
    fn resolve_main_root_strips_worktree_suffix() {
        let worktree = std::path::Path::new("/project/.worktrees/feat");
        assert_eq!(
            resolve_main_root(worktree),
            std::path::PathBuf::from("/project")
        );
    }

    #[test]
    fn resolve_main_root_passthrough_without_marker() {
        let plain = std::path::Path::new("/project");
        assert_eq!(
            resolve_main_root(plain),
            std::path::PathBuf::from("/project")
        );
    }
}
