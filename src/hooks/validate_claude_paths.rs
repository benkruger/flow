//! PreToolUse hook that blocks Edit/Write on .claude/rules/, .claude/skills/,
//! and CLAUDE.md during active FLOW phases, redirecting to bin/flow write-rule.
//!
//! Fires on Edit and Write tool calls.
//!
//! Exit 0 — allow (path is not protected, or no FLOW phase active)
//! Exit 2 — block (path is protected and FLOW phase is active)

use std::path::Path;

use super::{detect_branch_from_cwd, is_flow_active, read_hook_input, resolve_main_root};

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

/// Find the project root by walking up from CWD for `.flow-states/` directory.
fn find_project_root() -> Option<std::path::PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let mut current = cwd.as_path().to_path_buf();
    loop {
        if current.join(".flow-states").is_dir() {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

/// Run the validate-claude-paths hook (entry point from CLI).
pub fn run() {
    let hook_input = match read_hook_input() {
        Some(input) => input,
        None => std::process::exit(0),
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
        std::process::exit(0);
    }

    let project_root = find_project_root();
    let branch = if project_root.is_some() {
        detect_branch_from_cwd()
    } else {
        None
    };
    let flow_active = match (&branch, &project_root) {
        (Some(b), Some(r)) => is_flow_active(b, &resolve_main_root(r)),
        _ => false,
    };

    let (allowed, message) = validate(file_path, flow_active);
    if !allowed {
        eprintln!("{}", message);
        std::process::exit(2);
    }

    std::process::exit(0);
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
}
