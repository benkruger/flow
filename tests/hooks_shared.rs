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

#[test]
fn test_detect_branch_worktree_loop_exits_without_git_marker_and_git_spawn_fails() {
    // Covers two otherwise-unreached regions of `detect_branch_from_path`:
    //
    //   * The `while` loop exits via its condition (no `.git` marker
    //     found in any ancestor up to `worktrees_dir`) instead of
    //     returning from inside the body.
    //   * The `Err(_) => return None` arm when `Command::new("git")
    //     .current_dir(cwd).output()` fails at spawn because `cwd`
    //     is a path that doesn't exist on disk (ENOENT on chdir).
    //
    // A path containing the `.worktrees/` marker but pointing at a
    // filesystem location that doesn't exist exercises both:
    // the worktree-detection branch runs its walk and finds no
    // `.git` file in any iteration, then the fallback git subprocess
    // spawn fails because the cwd doesn't resolve.
    let cwd = std::path::PathBuf::from("/tmp/flow-rs-does-not-exist-abcxyz-1337/.worktrees/feat");
    let branch = hooks::detect_branch_from_path(&cwd);
    assert_eq!(branch, None);
}

// === is_flow_active ===

#[test]
fn test_is_flow_active_with_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let branch_dir = flow_states_dir(dir.path()).join("my-feature");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), "{}").unwrap();

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
fn test_permission_to_regex_bash_prefix_without_closing_paren() {
    // `Bash(` prefix matches but no `)` suffix — strip_suffix returns
    // None, hitting the second `?` arm inside permission_to_regex.
    assert!(hooks::permission_to_regex("Bash(git status").is_none());
    assert!(hooks::permission_to_regex("Bash(").is_none());
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

// --- Library-level tests (migrated from src/hooks/mod.rs) ---

use flow_rs::flow_paths::FlowPaths;
use flow_rs::hooks::{
    detect_branch_from_path, find_settings_and_root_from, is_flow_active, resolve_main_root,
};

/// Covers the `Err(_) => return (None, None)` arm on line 39 of
/// `find_settings_and_root_from`: `is_file()` returns true but
/// `fs::read_to_string` fails via `chmod 000`.
#[test]
fn find_settings_read_failure_returns_none_none() {
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
/// returns empty stdout (detached HEAD).
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
    let dir = tempfile::tempdir().unwrap();
    assert!(!is_flow_active("feat-branch", dir.path()));
}

#[test]
fn is_flow_active_valid_branch_with_state_file_returns_true() {
    let dir = tempfile::tempdir().unwrap();
    let paths = FlowPaths::new(dir.path(), "feat-branch");
    fs::create_dir_all(paths.state_file().parent().unwrap()).unwrap();
    fs::write(paths.state_file(), "{}").unwrap();
    assert!(is_flow_active("feat-branch", dir.path()));
}

#[test]
fn find_settings_invalid_json_returns_none_none() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir(&claude).unwrap();
    fs::write(claude.join("settings.json"), "{not valid json").unwrap();

    let (val, root) = find_settings_and_root_from(dir.path());
    assert!(val.is_none());
    assert!(root.is_none());
}

#[test]
fn resolve_main_root_strips_worktree_suffix() {
    let worktree = Path::new("/project/.worktrees/feat");
    assert_eq!(
        resolve_main_root(worktree),
        std::path::PathBuf::from("/project")
    );
}

#[test]
fn resolve_main_root_passthrough_without_marker() {
    let plain = Path::new("/project");
    assert_eq!(
        resolve_main_root(plain),
        std::path::PathBuf::from("/project")
    );
}

// Previously `find_settings_and_root_with` / `detect_branch_from_cwd_with`
// had mock-closure tests covering their Err arms. Those generic seams
// were removed because their per-monomorphization Err arms were
// unreachable through any production callsite; validate_pretool now
// calls `env::current_dir().ok()` inline and routes through
// `find_settings_and_root_from` / `detect_branch_from_path` with an
// explicit &Path. The Err arm of the inline env::current_dir() call
// is exercised by the stale-cwd subprocess test in
// tests/adversarial_agent_block.rs::validate_pretool_with_stale_cwd_does_not_panic.
