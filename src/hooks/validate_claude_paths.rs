//! PreToolUse hook that blocks Edit/Write on .claude/rules/, .claude/skills/,
//! and CLAUDE.md during active FLOW phases, redirecting to bin/flow write-rule.
//!
//! Fires on Edit and Write tool calls.
//!
//! Exit 0 — allow (path is not protected, or no FLOW phase active)
//! Exit 2 — block (path is protected and FLOW phase is active)

use std::path::Path;

use super::{detect_branch_from_path, is_flow_active, read_hook_input, resolve_main_root};
use crate::flow_paths::FlowStatesDir;

/// Check if a file path targets a protected .claude/ location.
///
/// Protected: .claude/rules/ (any depth), .claude/skills/ (any depth),
/// CLAUDE.md (any level).
/// Not protected: .claude/settings.json, .claude/settings.local.json.
pub fn is_protected_path(file_path: &str) -> bool {
    if file_path.is_empty() {
        return false;
    }

    let path = Path::new(file_path);
    let components: Vec<&str> = path
        .components()
        .map(|c| c.as_os_str().to_str().unwrap_or(""))
        .collect();

    // Check for .claude/rules/ or .claude/skills/ at any depth
    for (i, comp) in components.iter().enumerate() {
        if *comp == ".claude" && i + 1 < components.len() {
            let next = components[i + 1];
            if next == "rules" || next == "skills" {
                return true;
            }
        }
    }

    // Check for CLAUDE.md at any level
    if let Some(filename) = components.last() {
        if *filename == "CLAUDE.md" {
            return true;
        }
    }

    false
}

/// Validate that an Edit/Write on this path is allowed.
///
/// Returns `(allowed, message)`.
pub fn validate(file_path: &str, flow_active: bool) -> (bool, String) {
    if file_path.is_empty() {
        return (true, String::new());
    }

    if !flow_active {
        return (true, String::new());
    }

    if !is_protected_path(file_path) {
        return (true, String::new());
    }

    (
        false,
        "BLOCKED: .claude/ paths are protected during FLOW phases. \
         Use `${CLAUDE_PLUGIN_ROOT}/bin/flow write-rule --path <target> --content-file <temp>` instead. \
         Write the full file content to a temp file in .flow-states/, \
         then run the write-rule command."
            .to_string(),
    )
}

/// Find the project root by walking up from `cwd` for a `.flow-states/`
/// directory. Pure helper — accepts `cwd` as a parameter so unit tests
/// can drive every branch with a `TempDir` fixture. Mirrors the sibling
/// cwd-injection pattern in `src/hooks/mod.rs`
/// (`find_settings_and_root_from`, `detect_branch_from_path`).
fn find_project_root_in(cwd: &Path) -> Option<std::path::PathBuf> {
    let mut current = cwd.to_path_buf();
    loop {
        if FlowStatesDir::new(&current).path().is_dir() {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

/// Pure core of the validate-claude-paths hook.
///
/// Accepts the parsed stdin payload and the resolved cwd as injected
/// dependencies so every branch is reachable from unit tests with a
/// `TempDir` fixture. Follows the `run_impl_main` pattern in
/// `.claude/rules/rust-patterns.md` — `process::exit` and stderr I/O
/// live in the thin `run()` wrapper below.
///
/// Return contract:
/// - `(0, None)` → allow silently (wrapper exits 0, no stderr)
/// - `(2, Some(message))` → block (wrapper prints message to stderr, exits 2)
pub fn run_impl_main(hook_input: Option<serde_json::Value>, cwd: &Path) -> (i32, Option<String>) {
    let hook_input = match hook_input {
        Some(v) => v,
        None => return (0, None),
    };

    let tool_input = hook_input
        .get("tool_input")
        .cloned()
        .unwrap_or(serde_json::Value::Object(Default::default()));

    let file_path = tool_input
        .get("file_path")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if file_path.is_empty() {
        return (0, None);
    }

    let project_root = find_project_root_in(cwd);
    let branch = if project_root.is_some() {
        detect_branch_from_path(cwd)
    } else {
        None
    };
    let flow_active = match (&branch, &project_root) {
        (Some(b), Some(r)) => is_flow_active(b, &resolve_main_root(r)),
        _ => false,
    };

    let (allowed, message) = validate(file_path, flow_active);
    if !allowed {
        return (2, Some(message));
    }

    (0, None)
}

/// Run the validate-claude-paths hook (entry point from CLI).
///
/// Thin wrapper: reads stdin, resolves `std::env::current_dir()`,
/// calls `run_impl_main`, writes any block message to stderr, and
/// exits with the returned code.
pub fn run() {
    let input = read_hook_input();
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/"));
    let (code, message) = run_impl_main(input, &cwd);
    if let Some(m) = message {
        eprintln!("{}", m);
    }
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- is_protected_path tests ---

    #[test]
    fn test_is_protected_path_empty() {
        assert!(!is_protected_path(""));
    }

    #[test]
    fn test_is_protected_path_claude_rules() {
        assert!(is_protected_path("/project/.claude/rules/foo.md"));
    }

    #[test]
    fn test_is_protected_path_claude_md() {
        assert!(is_protected_path("/project/CLAUDE.md"));
    }

    #[test]
    fn test_is_protected_path_claude_skills() {
        assert!(is_protected_path("/project/.claude/skills/foo/SKILL.md"));
    }

    #[test]
    fn test_is_protected_path_settings() {
        assert!(!is_protected_path("/project/.claude/settings.json"));
    }

    #[test]
    fn test_is_protected_path_settings_local() {
        assert!(!is_protected_path("/project/.claude/settings.local.json"));
    }

    #[test]
    fn test_is_protected_path_nested_rules() {
        assert!(is_protected_path("/project/.claude/rules/subdir/deep.md"));
    }

    #[test]
    fn test_is_protected_path_nested_skills() {
        assert!(is_protected_path(
            "/project/.claude/skills/subdir/deep/SKILL.md"
        ));
    }

    #[test]
    fn test_is_protected_path_worktree_rules() {
        assert!(is_protected_path(
            "/project/.worktrees/feat/.claude/rules/foo.md"
        ));
    }

    #[test]
    fn test_is_protected_path_worktree_claude_md() {
        assert!(is_protected_path("/project/.worktrees/feat/CLAUDE.md"));
    }

    #[test]
    fn test_is_protected_path_worktree_skills() {
        assert!(is_protected_path(
            "/project/.worktrees/feat/.claude/skills/foo/SKILL.md"
        ));
    }

    // --- validate tests ---

    #[test]
    fn test_blocks_claude_rules_when_flow_active() {
        let (allowed, msg) = validate("/project/.claude/rules/foo.md", true);
        assert!(!allowed);
        assert!(msg.contains("BLOCKED"));
        assert!(msg.contains("write-rule"));
    }

    #[test]
    fn test_blocks_claude_md_when_flow_active() {
        let (allowed, msg) = validate("/project/CLAUDE.md", true);
        assert!(!allowed);
        assert!(msg.contains("BLOCKED"));
        assert!(msg.contains("write-rule"));
    }

    #[test]
    fn test_allows_claude_rules_when_no_flow() {
        let (allowed, msg) = validate("/project/.claude/rules/foo.md", false);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_allows_claude_md_when_no_flow() {
        let (allowed, msg) = validate("/project/CLAUDE.md", false);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_allows_unrelated_path_when_flow_active() {
        let (allowed, msg) = validate("/project/lib/foo.py", true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_allows_claude_settings_when_flow_active() {
        let (allowed, msg) = validate("/project/.claude/settings.json", true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_allows_flow_states_path() {
        let (allowed, msg) = validate("/project/.flow-states/branch-rule-content.md", true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_allows_empty_path() {
        let (allowed, msg) = validate("", true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_blocks_nested_claude_rules() {
        let (allowed, msg) = validate("/project/.claude/rules/subdir/deep.md", true);
        assert!(!allowed);
        assert!(msg.contains("BLOCKED"));
    }

    #[test]
    fn test_blocks_worktree_claude_rules() {
        let (allowed, msg) = validate("/project/.worktrees/feat/.claude/rules/foo.md", true);
        assert!(!allowed);
        assert!(msg.contains("BLOCKED"));
    }

    #[test]
    fn test_blocks_worktree_claude_md() {
        let (allowed, msg) = validate("/project/.worktrees/feat/CLAUDE.md", true);
        assert!(!allowed);
        assert!(msg.contains("BLOCKED"));
    }

    #[test]
    fn test_blocks_claude_skills_when_flow_active() {
        let (allowed, msg) = validate("/project/.claude/skills/foo/SKILL.md", true);
        assert!(!allowed);
        assert!(msg.contains("BLOCKED"));
        assert!(msg.contains("write-rule"));
    }

    #[test]
    fn test_blocks_nested_claude_skills() {
        let (allowed, msg) = validate("/project/.claude/skills/subdir/deep/SKILL.md", true);
        assert!(!allowed);
        assert!(msg.contains("BLOCKED"));
    }

    #[test]
    fn test_blocks_worktree_claude_skills() {
        let (allowed, msg) = validate("/project/.worktrees/feat/.claude/skills/foo/SKILL.md", true);
        assert!(!allowed);
        assert!(msg.contains("BLOCKED"));
    }

    #[test]
    fn test_allows_claude_skills_when_no_flow() {
        let (allowed, msg) = validate("/project/.claude/skills/foo/SKILL.md", false);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_allows_claude_settings_local() {
        let (allowed, _) = validate("/project/.claude/settings.local.json", true);
        assert!(allowed);
    }

    #[test]
    fn test_error_message_mentions_write_rule() {
        let (_, msg) = validate("/project/.claude/rules/foo.md", true);
        assert!(msg.contains("write-rule"));
        assert!(msg.contains("--path"));
        assert!(msg.contains("--content-file"));
    }

    // --- find_project_root_in ---

    #[test]
    fn find_project_root_in_returns_cwd_when_flow_states_present() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".flow-states")).unwrap();
        assert_eq!(find_project_root_in(&root), Some(root));
    }

    #[test]
    fn find_project_root_in_returns_ancestor_when_flow_states_in_parent() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".flow-states")).unwrap();
        let deep = root.join("sub").join("deep");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(find_project_root_in(&deep), Some(root));
    }

    #[test]
    fn find_project_root_in_returns_none_when_no_flow_states() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        // No .flow-states/ at root or any ancestor created by this test.
        // If a real .flow-states/ exists at a parent of the tempdir on
        // the developer machine, this test would fail — signaling that
        // the isolation assumption is wrong. CI runs in a fresh env
        // without such ancestors.
        assert_eq!(find_project_root_in(&root), None);
    }

    // --- run_impl_main ---

    /// Seed a fixture laid out as `<root>/.flow-states/<branch>.json`
    /// plus `<root>/.worktrees/<branch>/.git` marker so the worktree
    /// cwd `<root>/.worktrees/<branch>` resolves project_root via
    /// `find_project_root_in` and branch via `detect_branch_from_path`.
    fn seed_active_flow_fixture(root: &Path, branch: &str) -> std::path::PathBuf {
        std::fs::create_dir_all(root.join(".flow-states")).unwrap();
        std::fs::write(
            root.join(".flow-states").join(format!("{}.json", branch)),
            "{}",
        )
        .unwrap();
        let worktree = root.join(".worktrees").join(branch);
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(worktree.join(".git"), "gitdir: fake\n").unwrap();
        worktree
    }

    #[test]
    fn run_impl_main_returns_zero_when_hook_input_missing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (code, msg) = run_impl_main(None, &root);
        assert_eq!(code, 0);
        assert!(msg.is_none());
    }

    #[test]
    fn run_impl_main_returns_zero_when_file_path_empty() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let input = serde_json::json!({"tool_input": {}});
        let (code, msg) = run_impl_main(Some(input), &root);
        assert_eq!(code, 0);
        assert!(msg.is_none());
    }

    #[test]
    fn run_impl_main_returns_zero_when_no_project_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        // No .flow-states/ anywhere — find_project_root_in returns None,
        // flow_active is false, so even a protected file_path is allowed.
        let input = serde_json::json!({
            "tool_input": {"file_path": "/anything/.claude/rules/foo.md"}
        });
        let (code, msg) = run_impl_main(Some(input), &root);
        assert_eq!(code, 0);
        assert!(msg.is_none());
    }

    #[test]
    fn run_impl_main_returns_block_when_flow_active_and_protected_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let worktree = seed_active_flow_fixture(&root, "feat");
        let target = worktree.join(".claude/rules/foo.md");
        let input = serde_json::json!({
            "tool_input": {"file_path": target.to_string_lossy()}
        });
        let (code, msg) = run_impl_main(Some(input), &worktree);
        assert_eq!(code, 2);
        let msg = msg.expect("block returns Some(message)");
        assert!(msg.contains("BLOCKED"), "message: {}", msg);
    }

    #[test]
    fn run_impl_main_returns_zero_when_flow_active_and_unprotected_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let worktree = seed_active_flow_fixture(&root, "feat");
        let target = worktree.join("src/lib.rs");
        let input = serde_json::json!({
            "tool_input": {"file_path": target.to_string_lossy()}
        });
        let (code, msg) = run_impl_main(Some(input), &worktree);
        assert_eq!(code, 0);
        assert!(msg.is_none());
    }

    #[test]
    fn run_impl_main_returns_zero_when_branch_none() {
        // project_root resolves (`.flow-states/` present) but cwd is
        // outside any `.worktrees/<branch>/` layout, so
        // `detect_branch_from_path` falls back to `git branch
        // --show-current` in cwd. TempDir has no git repo → None.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".flow-states")).unwrap();
        // cwd is a sub-directory under root, not under .worktrees/.
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let input = serde_json::json!({
            "tool_input": {"file_path": "/anything/.claude/rules/foo.md"}
        });
        let (code, msg) = run_impl_main(Some(input), &sub);
        assert_eq!(code, 0);
        assert!(msg.is_none());
    }
}
