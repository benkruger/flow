//! PreToolUse hook that validates file tool calls in FLOW worktrees.
//!
//! Two enforcement layers:
//! 1. **Worktree path redirection** — blocks file tool calls that target the
//!    main repo when the working directory is inside a FLOW worktree, directing
//!    the caller to use the worktree copy instead.
//! 2. **Shared config protection** — blocks Edit/Write calls on shared
//!    configuration files (`.gitignore`, `Cargo.toml`, `.github/`, etc.) during
//!    active FLOW phases, directing the caller to confirm with the user first.
//!    Read/Glob/Grep are allowed so codebase exploration is not impacted.
//!
//! Fires on Edit, Write, Read, Glob, and Grep tool calls.
//!
//! Exit 0 — allow (path is fine or not in a worktree)
//! Exit 2 — block (path targets main repo, or shared config Edit/Write)

use std::path::Path;

use serde_json::Value;

use super::read_hook_input;
use crate::flow_paths::FlowStatesDir;

const WORKTREE_MARKER: &str = ".worktrees/";

/// Filenames that are shared configuration affecting all engineers.
///
/// Matches the canonical list from `.claude/rules/permissions.md`
/// "Shared Config Files" section. `.claude/settings.json` is excluded
/// because `validate-claude-paths` already covers it.
const SHARED_CONFIG_FILENAMES: &[&str] = &[
    ".gitignore",
    ".gitattributes",
    "Makefile",
    "Rakefile",
    "justfile",
    "package.json",
    "requirements.txt",
    "go.mod",
    "Cargo.toml",
];

/// Check if a file path targets a shared configuration file.
///
/// Returns `true` when the filename matches one of the nine canonical
/// shared-config filenames, or when the path passes through a `.github/`
/// directory (workflows, issue templates, CODEOWNERS).
pub fn is_shared_config(file_path: &str) -> bool {
    if file_path.is_empty() {
        return false;
    }

    let path = Path::new(file_path);
    let components: Vec<&str> = path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();

    // Check filename against the exact-match list
    if let Some(filename) = components.last() {
        if SHARED_CONFIG_FILENAMES.contains(filename) {
            return true;
        }
    }

    // Check for .github/ directory with descendants
    for (i, comp) in components.iter().enumerate() {
        if *comp == ".github" && i + 1 < components.len() {
            return true;
        }
    }

    false
}

/// Check if an Edit/Write on a shared config file should be blocked.
///
/// Returns `(allowed, message)`. Only blocks when all of:
/// - `tool_name` is "Edit" or "Write" (reads are fine)
/// - CWD is inside a `.worktrees/` directory
/// - `file_path` is inside the worktree (not targeting main repo or external paths)
/// - the file matches the shared-config list
pub fn validate_shared_config(file_path: &str, cwd: &str, tool_name: &str) -> (bool, String) {
    if file_path.is_empty() {
        return (true, String::new());
    }

    if tool_name != "Edit" && tool_name != "Write" {
        return (true, String::new());
    }

    if !cwd.contains(WORKTREE_MARKER) {
        return (true, String::new());
    }

    // Only block paths inside the worktree cwd
    let cwd_prefix = format!("{}/", cwd);
    if !file_path.starts_with(&cwd_prefix) && file_path != cwd {
        return (true, String::new());
    }

    if !is_shared_config(file_path) {
        return (true, String::new());
    }

    let filename = Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(file_path);

    (
        false,
        format!(
            "BLOCKED: {} is a shared configuration file that affects every engineer \
             in the repository. Modifying it during a FLOW phase requires explicit \
             user permission. Use AskUserQuestion to confirm with the user before \
             proceeding. See .claude/rules/permissions.md \"Shared Config Files\" section.",
            filename
        ),
    )
}

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
    let flow_states_dir = FlowStatesDir::new(Path::new(project_root));
    let flow_states_prefix = format!("{}/", flow_states_dir.path().to_string_lossy());
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

    let tool_name = hook_input
        .get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let (sc_allowed, sc_message) = validate_shared_config(&file_path, &cwd, tool_name);
    if !sc_allowed {
        eprintln!("{}", sc_message);
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

    // --- is_shared_config tests ---

    #[test]
    fn test_shared_config_gitignore() {
        assert!(is_shared_config("/project/.worktrees/feat/.gitignore"));
    }

    #[test]
    fn test_shared_config_gitattributes() {
        assert!(is_shared_config("/project/.worktrees/feat/.gitattributes"));
    }

    #[test]
    fn test_shared_config_makefile() {
        assert!(is_shared_config("/project/.worktrees/feat/Makefile"));
    }

    #[test]
    fn test_shared_config_rakefile() {
        assert!(is_shared_config("/project/.worktrees/feat/Rakefile"));
    }

    #[test]
    fn test_shared_config_justfile() {
        assert!(is_shared_config("/project/.worktrees/feat/justfile"));
    }

    #[test]
    fn test_shared_config_package_json() {
        assert!(is_shared_config("/project/.worktrees/feat/package.json"));
    }

    #[test]
    fn test_shared_config_requirements_txt() {
        assert!(is_shared_config(
            "/project/.worktrees/feat/requirements.txt"
        ));
    }

    #[test]
    fn test_shared_config_go_mod() {
        assert!(is_shared_config("/project/.worktrees/feat/go.mod"));
    }

    #[test]
    fn test_shared_config_cargo_toml() {
        assert!(is_shared_config("/project/.worktrees/feat/Cargo.toml"));
    }

    #[test]
    fn test_shared_config_github_directory() {
        assert!(is_shared_config(
            "/project/.worktrees/feat/.github/workflows/ci.yml"
        ));
    }

    #[test]
    fn test_shared_config_github_codeowners() {
        assert!(is_shared_config(
            "/project/.worktrees/feat/.github/CODEOWNERS"
        ));
    }

    #[test]
    fn test_shared_config_not_regular_file() {
        assert!(!is_shared_config("/project/.worktrees/feat/src/lib.rs"));
    }

    #[test]
    fn test_shared_config_not_readme() {
        assert!(!is_shared_config("/project/.worktrees/feat/README.md"));
    }

    #[test]
    fn test_shared_config_empty_path() {
        assert!(!is_shared_config(""));
    }

    #[test]
    fn test_shared_config_case_sensitive_makefile() {
        // lowercase `makefile` is NOT in the canonical list
        assert!(!is_shared_config("/project/.worktrees/feat/makefile"));
    }

    #[test]
    fn test_shared_config_github_directory_itself() {
        // `.github` alone (no child) is a directory, not a file target
        assert!(!is_shared_config("/project/.worktrees/feat/.github"));
    }

    // --- validate_shared_config tests ---

    #[test]
    fn test_shared_config_edit_gitignore_blocked() {
        let cwd = "/project/.worktrees/feat";
        let file_path = "/project/.worktrees/feat/.gitignore";
        let (allowed, msg) = validate_shared_config(file_path, cwd, "Edit");
        assert!(!allowed);
        assert!(msg.contains("shared configuration"));
        assert!(msg.contains("permissions.md"));
    }

    #[test]
    fn test_shared_config_write_cargo_toml_blocked() {
        let cwd = "/project/.worktrees/feat";
        let file_path = "/project/.worktrees/feat/Cargo.toml";
        let (allowed, msg) = validate_shared_config(file_path, cwd, "Write");
        assert!(!allowed);
        assert!(msg.contains("shared configuration"));
    }

    #[test]
    fn test_shared_config_read_gitignore_allowed() {
        let cwd = "/project/.worktrees/feat";
        let file_path = "/project/.worktrees/feat/.gitignore";
        let (allowed, msg) = validate_shared_config(file_path, cwd, "Read");
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_shared_config_grep_github_allowed() {
        let cwd = "/project/.worktrees/feat";
        let file_path = "/project/.worktrees/feat/.github/workflows/ci.yml";
        let (allowed, msg) = validate_shared_config(file_path, cwd, "Grep");
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_shared_config_edit_outside_worktree_allowed() {
        let cwd = "/project";
        let file_path = "/project/.gitignore";
        let (allowed, msg) = validate_shared_config(file_path, cwd, "Edit");
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_shared_config_edit_regular_file_allowed() {
        let cwd = "/project/.worktrees/feat";
        let file_path = "/project/.worktrees/feat/src/lib.rs";
        let (allowed, msg) = validate_shared_config(file_path, cwd, "Edit");
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_shared_config_empty_path_allowed() {
        let cwd = "/project/.worktrees/feat";
        let (allowed, msg) = validate_shared_config("", cwd, "Edit");
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_shared_config_edit_main_repo_shared_allowed() {
        // Path targets main repo's .gitignore from inside worktree.
        // The existing validate() already blocks main-repo paths;
        // validate_shared_config only fires for worktree-internal paths.
        let cwd = "/project/.worktrees/feat";
        let file_path = "/project/.gitignore";
        let (allowed, msg) = validate_shared_config(file_path, cwd, "Edit");
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_shared_config_edit_github_workflow_blocked() {
        let cwd = "/project/.worktrees/feat";
        let file_path = "/project/.worktrees/feat/.github/workflows/ci.yml";
        let (allowed, msg) = validate_shared_config(file_path, cwd, "Edit");
        assert!(!allowed);
        assert!(msg.contains("shared configuration"));
    }

    #[test]
    fn test_shared_config_empty_tool_name_allowed() {
        // Missing tool_name is treated as non-Edit/Write
        let cwd = "/project/.worktrees/feat";
        let file_path = "/project/.worktrees/feat/.gitignore";
        let (allowed, msg) = validate_shared_config(file_path, cwd, "");
        assert!(allowed);
        assert!(msg.is_empty());
    }
}
