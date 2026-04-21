//! Integration tests for `src/git.rs`. Drives the public wrappers
//! (`current_branch`, `current_branch_in`, `project_root`,
//! `resolve_branch`, `resolve_branch_in`) through real git fixtures.
//! The pure helpers behind these wrappers are now private; their
//! branches are exercised transitively via the wrappers.

use std::fs;
use std::path::Path;
use std::process::Command;

use flow_rs::git::{
    current_branch, current_branch_in, project_root, resolve_branch, resolve_branch_in,
};

/// Initialize a git repo in the given directory with an initial commit
/// on the named branch.
fn init_git_repo(dir: &Path, initial_branch: &str) {
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git command failed");
        assert!(output.status.success(), "git {:?} failed", args);
    };
    run(&["init", "--initial-branch", initial_branch]);
    run(&["config", "user.email", "test@test.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "commit.gpgsign", "false"]);
    run(&["commit", "--allow-empty", "-m", "init"]);
}

// --- project_root (subprocess) ---

#[test]
fn project_root_in_real_repo_returns_existing_path() {
    let root = project_root();
    assert!(root.exists() || root == Path::new("."));
}

// --- current_branch (subprocess) ---

#[test]
fn current_branch_in_real_repo_returns_without_panic() {
    // Process cwd is the flow repo. current_branch queries git; the
    // exact branch depends on the test harness state.
    let _ = current_branch();
}

// --- current_branch_in ---

#[test]
fn current_branch_in_reads_cwd_repo() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "my-feature");
    let branch = current_branch_in(dir.path());
    assert_eq!(branch, Some("my-feature".to_string()));
}

#[test]
fn current_branch_in_detached_head() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "main");
    let sha = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let sha = String::from_utf8_lossy(&sha.stdout).trim().to_string();
    let output = Command::new("git")
        .args(["checkout", &sha])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let branch = current_branch_in(dir.path());
    assert_eq!(branch, None);
}

#[test]
fn current_branch_in_non_git_dir_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let branch = current_branch_in(dir.path());
    assert_eq!(branch, None);
}

// --- resolve_branch (public wrapper) ---

#[test]
fn resolve_branch_override_wins() {
    let dir = tempfile::tempdir().unwrap();
    let branch = resolve_branch(Some("explicit-branch"), dir.path());
    assert_eq!(branch, Some("explicit-branch".to_string()));
}

// --- resolve_branch_in ---

#[test]
fn resolve_branch_in_override_wins() {
    let repo = tempfile::tempdir().unwrap();
    init_git_repo(repo.path(), "main");
    let root = tempfile::tempdir().unwrap();
    let branch = resolve_branch_in(Some("explicit"), repo.path(), root.path());
    assert_eq!(branch, Some("explicit".to_string()));
}

#[test]
fn resolve_branch_in_reads_branch_from_cwd() {
    let repo = tempfile::tempdir().unwrap();
    init_git_repo(repo.path(), "cwd-branch");
    let root = tempfile::tempdir().unwrap();
    let branch = resolve_branch_in(None, repo.path(), root.path());
    assert_eq!(branch, Some("cwd-branch".to_string()));
}

#[test]
fn resolve_branch_in_matches_state_file() {
    let repo = tempfile::tempdir().unwrap();
    init_git_repo(repo.path(), "matched");
    let root = tempfile::tempdir().unwrap();
    let state_dir = root.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("matched.json"), r#"{"branch": "matched"}"#).unwrap();

    let branch = resolve_branch_in(None, repo.path(), root.path());
    assert_eq!(branch, Some("matched".to_string()));
}
