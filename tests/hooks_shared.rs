//! Tests for shared hook utilities (src/hooks/mod.rs).

mod common;

use std::fs;
use std::path::Path;

use serde_json::json;

use common::flow_states_dir;
use flow_rs::hooks;

// === find_settings_and_root_from ===

#[test]
fn test_find_settings_at_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let claude_dir = dir.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    let settings = json!({"permissions": {"allow": [], "deny": []}});
    fs::write(
        claude_dir.join("settings.json"),
        serde_json::to_string(&settings).unwrap(),
    )
    .unwrap();

    let (found, root) = hooks::find_settings_and_root_from(dir.path());
    assert!(found.is_some());
    assert_eq!(root.unwrap(), dir.path());
    assert_eq!(found.unwrap()["permissions"]["allow"], json!([]));
}

#[test]
fn test_find_settings_in_parent() {
    let dir = tempfile::tempdir().unwrap();
    let claude_dir = dir.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    let settings = json!({"permissions": {"allow": ["Bash(git status)"]}});
    fs::write(
        claude_dir.join("settings.json"),
        serde_json::to_string(&settings).unwrap(),
    )
    .unwrap();

    let subdir = dir.path().join("src").join("deep");
    fs::create_dir_all(&subdir).unwrap();

    let (found, root) = hooks::find_settings_and_root_from(&subdir);
    assert!(found.is_some());
    assert_eq!(root.unwrap(), dir.path());
}

#[test]
fn test_find_settings_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let (found, root) = hooks::find_settings_and_root_from(dir.path());
    assert!(found.is_none());
    assert!(root.is_none());
}

#[test]
fn test_find_settings_invalid_json() {
    let dir = tempfile::tempdir().unwrap();
    let claude_dir = dir.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(claude_dir.join("settings.json"), "{bad json").unwrap();

    let (found, root) = hooks::find_settings_and_root_from(dir.path());
    assert!(found.is_none());
    assert!(root.is_none());
}

// === detect_branch_from_path ===

#[test]
fn test_detect_branch_from_worktree_path() {
    let dir = tempfile::tempdir().unwrap();
    let wt = dir.path().join(".worktrees").join("my-feature");
    fs::create_dir_all(&wt).unwrap();
    // Create a .git file (not directory) as worktrees have
    fs::write(wt.join(".git"), "gitdir: /some/path").unwrap();

    let branch = hooks::detect_branch_from_path(&wt);
    assert_eq!(branch, Some("my-feature".to_string()));
}

#[test]
fn test_detect_branch_from_worktree_subdir() {
    let dir = tempfile::tempdir().unwrap();
    let wt = dir.path().join(".worktrees").join("my-feature");
    let subdir = wt.join("src").join("lib");
    fs::create_dir_all(&subdir).unwrap();
    fs::write(wt.join(".git"), "gitdir: /some/path").unwrap();

    let branch = hooks::detect_branch_from_path(&subdir);
    assert_eq!(branch, Some("my-feature".to_string()));
}

#[test]
fn test_detect_branch_not_in_worktree() {
    // Not in a worktree — falls back to git subprocess.
    // Use an empty (non-git) directory so `git branch --show-current` exits
    // non-zero and the helper returns None. Avoid creating a real git repo
    // because CI runners don't have user.name/user.email configured and
    // `git init` populates HEAD with the default branch name.
    let dir = tempfile::tempdir().unwrap();
    let empty_subdir = dir.path().join("not-a-git-repo");
    fs::create_dir_all(&empty_subdir).unwrap();

    let branch = hooks::detect_branch_from_path(&empty_subdir);
    assert!(branch.is_none());
}

// === is_flow_active ===

#[test]
fn test_is_flow_active_with_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = flow_states_dir(dir.path());
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("my-feature.json"), "{}").unwrap();

    assert!(hooks::is_flow_active("my-feature", dir.path()));
}

#[test]
fn test_is_flow_active_no_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = flow_states_dir(dir.path());
    fs::create_dir_all(&state_dir).unwrap();

    assert!(!hooks::is_flow_active("my-feature", dir.path()));
}

#[test]
fn test_is_flow_active_empty_branch() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!hooks::is_flow_active("", dir.path()));
}

#[test]
fn test_is_flow_active_rejects_path_separators() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = flow_states_dir(dir.path());
    fs::create_dir_all(&state_dir).unwrap();

    assert!(!hooks::is_flow_active("../etc/passwd", dir.path()));
    assert!(!hooks::is_flow_active("foo/bar", dir.path()));
    assert!(!hooks::is_flow_active("foo\\bar", dir.path()));
}

// === resolve_main_root ===

#[test]
fn test_resolve_main_root_from_worktree() {
    let root = Path::new("/Users/dev/myproject/.worktrees/my-feature");
    let resolved = hooks::resolve_main_root(root);
    assert_eq!(resolved, Path::new("/Users/dev/myproject/"));
}

#[test]
fn test_resolve_main_root_not_worktree() {
    let root = Path::new("/Users/dev/myproject");
    let resolved = hooks::resolve_main_root(root);
    assert_eq!(resolved, Path::new("/Users/dev/myproject"));
}

// === permission_to_regex ===

#[test]
fn test_permission_to_regex_exact() {
    let re = hooks::permission_to_regex("Bash(git status)").unwrap();
    assert!(re.is_match("git status"));
    assert!(!re.is_match("git status --short"));
    assert!(!re.is_match("xgit status"));
}

#[test]
fn test_permission_to_regex_wildcard() {
    let re = hooks::permission_to_regex("Bash(git diff *)").unwrap();
    assert!(re.is_match("git diff HEAD"));
    assert!(re.is_match("git diff --cached"));
    assert!(!re.is_match("git diff"));
}

#[test]
fn test_permission_to_regex_bin_wildcard() {
    let re = hooks::permission_to_regex("Bash(*bin/*)").unwrap();
    assert!(re.is_match("/usr/bin/ci"));
    assert!(re.is_match("bin/test"));
    assert!(!re.is_match("git status"));
}

#[test]
fn test_permission_to_regex_non_bash() {
    assert!(hooks::permission_to_regex("Read(/tmp/*)").is_none());
    assert!(hooks::permission_to_regex("Edit(foo)").is_none());
    assert!(hooks::permission_to_regex("random string").is_none());
}

#[test]
fn test_permission_to_regex_escapes_special_chars() {
    let re = hooks::permission_to_regex("Bash(bin/ci;*)").unwrap();
    assert!(re.is_match("bin/ci;--verbose"));
    assert!(!re.is_match("bin/ci--verbose"));
}

// === build_permission_regexes ===

#[test]
fn test_build_permission_regexes_allow() {
    let settings = json!({
        "permissions": {
            "allow": ["Bash(git status)", "Bash(git diff *)", "Read(/tmp/*)"],
            "deny": []
        }
    });
    let regexes = hooks::build_permission_regexes(&settings, "allow");
    // Only Bash entries, so Read is filtered out
    assert_eq!(regexes.len(), 2);
    assert!(regexes[0].is_match("git status"));
    assert!(regexes[1].is_match("git diff HEAD"));
}

#[test]
fn test_build_permission_regexes_deny() {
    let settings = json!({
        "permissions": {
            "allow": [],
            "deny": ["Bash(git checkout *)"]
        }
    });
    let regexes = hooks::build_permission_regexes(&settings, "deny");
    assert_eq!(regexes.len(), 1);
    assert!(regexes[0].is_match("git checkout main"));
}

#[test]
fn test_build_permission_regexes_missing_permissions() {
    let settings = json!({});
    let regexes = hooks::build_permission_regexes(&settings, "allow");
    assert!(regexes.is_empty());
}

#[test]
fn test_build_permission_regexes_missing_key() {
    let settings = json!({"permissions": {}});
    let regexes = hooks::build_permission_regexes(&settings, "allow");
    assert!(regexes.is_empty());
}
