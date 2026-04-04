//! PreToolUse hook that blocks file tool calls targeting the main repo
//! when the working directory is inside a FLOW worktree.
//!
//! Fires on Edit, Write, Read, Glob, and Grep tool calls.
//!
//! Exit 0 — allow (path is fine or not in a worktree)
//! Exit 2 — block (path targets main repo instead of worktree)

use serde_json::Value;

use super::read_hook_input;

const WORKTREE_MARKER: &str = ".worktrees/";

/// Extract the file path from tool input.
///
/// Edit/Write/Read use `file_path`. Glob/Grep use `path`.
pub fn get_file_path(tool_input: &Value) -> String {
    if let Some(fp) = tool_input.get("file_path").and_then(|v| v.as_str()) {
        return fp.to_string();
    }
    if let Some(p) = tool_input.get("path").and_then(|v| v.as_str()) {
        return p.to_string();
    }
    String::new()
}

/// Validate that `file_path` targets the worktree, not the main repo.
///
/// Returns `(allowed, message)`.
pub fn validate(file_path: &str, cwd: &str) -> (bool, String) {
    if file_path.is_empty() {
        return (true, String::new());
    }

    let marker_pos = match cwd.find(WORKTREE_MARKER) {
        Some(pos) => pos,
        None => return (true, String::new()), // not in a worktree
    };

    let project_root = cwd[..marker_pos].trim_end_matches('/');

    // Paths outside the project are always fine (~/.claude, /tmp, etc.)
    let prefix = format!("{}/", project_root);
    if !file_path.starts_with(&prefix) {
        return (true, String::new());
    }

    // Paths inside the worktree are fine
    let cwd_prefix = format!("{}/", cwd);
    if file_path.starts_with(&cwd_prefix) || file_path == cwd {
        return (true, String::new());
    }

    // .flow-states/ is the shared state directory at the main repo — always fine
    let flow_states_prefix = format!("{}/.flow-states/", project_root);
    if file_path.starts_with(&flow_states_prefix) {
        return (true, String::new());
    }

    // Block: path targets main repo from inside a worktree
    let relative = &file_path[project_root.len() + 1..];
    let corrected = format!("{}/{}", cwd, relative);

    (
        false,
        format!(
            "BLOCKED: You are in worktree {}. Use {} instead of {}",
            cwd, corrected, file_path
        ),
    )
}

/// Run the validate-worktree-paths hook (entry point from CLI).
pub fn run() {
    let hook_input = match read_hook_input() {
        Some(input) => input,
        None => std::process::exit(0),
    };

    let tool_input = hook_input
        .get("tool_input")
        .cloned()
        .unwrap_or(Value::Object(Default::default()));

    let file_path = get_file_path(&tool_input);
    if file_path.is_empty() {
        std::process::exit(0);
    }

    let cwd = match std::env::current_dir() {
        Ok(p) => p.to_string_lossy().to_string(),
        Err(_) => std::process::exit(0),
    };

    let (allowed, message) = validate(&file_path, &cwd);
    if !allowed {
        eprintln!("{}", message);
        std::process::exit(2);
    }

    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- validate tests ---

    #[test]
    fn test_allows_when_not_in_worktree() {
        let (allowed, msg) = validate("/Users/ben/code/flow/lib/foo.py", "/Users/ben/code/flow");
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_allows_file_inside_worktree() {
        let (allowed, msg) = validate(
            "/Users/ben/code/flow/.worktrees/my-feature/lib/foo.py",
            "/Users/ben/code/flow/.worktrees/my-feature",
        );
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_blocks_main_repo_path_from_worktree() {
        let cwd = "/Users/ben/code/flow/.worktrees/my-feature";
        let (allowed, msg) = validate("/Users/ben/code/flow/lib/foo.py", cwd);
        assert!(!allowed);
        assert!(msg.contains("BLOCKED"));
        assert!(msg.contains(cwd));
    }

    #[test]
    fn test_allows_flow_states_path() {
        let (allowed, msg) = validate(
            "/Users/ben/code/flow/.flow-states/my-feature.json",
            "/Users/ben/code/flow/.worktrees/my-feature",
        );
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_allows_home_directory_paths() {
        let (allowed, msg) = validate(
            "/Users/ben/.claude/plans/some-plan.md",
            "/Users/ben/code/flow/.worktrees/my-feature",
        );
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_allows_plugin_cache_paths() {
        let (allowed, msg) = validate(
            "/Users/ben/.claude/plugins/cache/flow/0.28.5/skills/flow-code/SKILL.md",
            "/Users/ben/code/flow/.worktrees/my-feature",
        );
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_error_message_includes_corrected_path() {
        let cwd = "/Users/ben/code/flow/.worktrees/my-feature";
        let file_path = "/Users/ben/code/flow/skills/flow-prime/SKILL.md";
        let (allowed, msg) = validate(file_path, cwd);
        assert!(!allowed);
        let corrected = "/Users/ben/code/flow/.worktrees/my-feature/skills/flow-prime/SKILL.md";
        assert!(msg.contains(corrected));
        assert!(msg.contains(file_path));
    }

    #[test]
    fn test_allows_empty_file_path() {
        let (allowed, _) = validate("", "/Users/ben/code/flow/.worktrees/my-feature");
        assert!(allowed);
    }

    #[test]
    fn test_allows_worktree_root_path_exactly() {
        let cwd = "/Users/ben/code/flow/.worktrees/my-feature";
        let (allowed, _) = validate(cwd, cwd);
        assert!(allowed);
    }

    // --- get_file_path tests ---

    #[test]
    fn test_get_file_path_prefers_file_path() {
        let tool_input = json!({"file_path": "/some/path.py", "path": "/other/path"});
        assert_eq!(get_file_path(&tool_input), "/some/path.py");
    }

    #[test]
    fn test_get_file_path_falls_back_to_path() {
        let tool_input = json!({"path": "/some/dir"});
        assert_eq!(get_file_path(&tool_input), "/some/dir");
    }

    #[test]
    fn test_get_file_path_returns_empty_for_neither() {
        let tool_input = json!({"command": "something"});
        assert_eq!(get_file_path(&tool_input), "");
    }
}
