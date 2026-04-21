//! Library-level tests for `flow_rs::cwd_scope`. Drives through the
//! public `enforce` entry only — no private helpers are imported per
//! `.claude/rules/test-placement.md`.

use std::fs;
use std::path::Path;
use std::process::Command;

use flow_rs::cwd_scope::enforce;

fn init_git_repo(dir: &Path, branch: &str) {
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git command failed");
        assert!(output.status.success(), "git {:?} failed", args);
    };
    run(&["init", "--initial-branch", branch]);
    run(&["config", "user.email", "test@test.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "commit.gpgsign", "false"]);
    run(&["commit", "--allow-empty", "-m", "init"]);
}

fn write_state(root: &Path, branch: &str, relative_cwd: &str) {
    let state_dir = root.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state = serde_json::json!({
        "branch": branch,
        "relative_cwd": relative_cwd,
    });
    fs::write(
        state_dir.join(format!("{}.json", branch)),
        state.to_string(),
    )
    .unwrap();
}

#[test]
fn enforce_no_state_file_returns_ok() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "main");
    let result = enforce(dir.path(), dir.path());
    assert!(result.is_ok(), "expected ok, got: {:?}", result);
}

#[test]
fn enforce_non_git_dir_returns_ok() {
    let dir = tempfile::tempdir().unwrap();
    let result = enforce(dir.path(), dir.path());
    assert!(result.is_ok(), "expected ok, got: {:?}", result);
}

#[test]
fn enforce_empty_relative_cwd_at_worktree_root_returns_ok() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "feature-x");
    write_state(dir.path(), "feature-x", "");
    let result = enforce(dir.path(), dir.path());
    assert!(result.is_ok(), "expected ok, got: {:?}", result);
}

#[test]
fn enforce_empty_relative_cwd_in_subdir_returns_ok() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "feature-x");
    write_state(dir.path(), "feature-x", "");
    let subdir = dir.path().join("api");
    fs::create_dir(&subdir).unwrap();
    let result = enforce(&subdir, dir.path());
    assert!(result.is_ok(), "expected ok, got: {:?}", result);
}

#[test]
fn enforce_relative_cwd_descendant_returns_ok() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "feature-x");
    write_state(dir.path(), "feature-x", "api");
    let nested = dir.path().join("api").join("src");
    fs::create_dir_all(&nested).unwrap();
    let result = enforce(&nested, dir.path());
    assert!(result.is_ok(), "expected ok, got: {:?}", result);
}

#[test]
fn enforce_relative_cwd_matches_subdir_returns_ok() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "feature-x");
    write_state(dir.path(), "feature-x", "api");
    let subdir = dir.path().join("api");
    fs::create_dir(&subdir).unwrap();
    let result = enforce(&subdir, dir.path());
    assert!(result.is_ok(), "expected ok, got: {:?}", result);
}

#[test]
fn enforce_relative_cwd_mismatch_errors() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "feature-x");
    write_state(dir.path(), "feature-x", "api");
    let ios = dir.path().join("ios");
    fs::create_dir(&ios).unwrap();
    let result = enforce(&ios, dir.path());
    assert!(result.is_err(), "expected error, got: {:?}", result);
    let msg = result.unwrap_err();
    assert!(
        msg.contains("api"),
        "error should name expected directory: {}",
        msg
    );
}

#[test]
fn enforce_nested_relative_cwd_returns_ok() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "feature-x");
    write_state(dir.path(), "feature-x", "packages/api");
    let nested = dir.path().join("packages").join("api");
    fs::create_dir_all(&nested).unwrap();
    let result = enforce(&nested, dir.path());
    assert!(result.is_ok(), "expected ok, got: {:?}", result);
}

#[test]
fn enforce_relative_cwd_at_worktree_root_errors() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "feature-x");
    write_state(dir.path(), "feature-x", "api");
    let result = enforce(dir.path(), dir.path());
    assert!(result.is_err(), "expected error, got: {:?}", result);
}

#[test]
fn enforce_corrupt_state_file_returns_ok() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "feature-x");
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("feature-x.json"), "not json").unwrap();
    let result = enforce(dir.path(), dir.path());
    assert!(result.is_ok(), "expected ok, got: {:?}", result);
}

#[test]
fn enforce_missing_relative_cwd_field_treats_as_empty() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "feature-x");
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join("feature-x.json"),
        r#"{"branch": "feature-x"}"#,
    )
    .unwrap();
    let result = enforce(dir.path(), dir.path());
    assert!(result.is_ok(), "expected ok, got: {:?}", result);
}

#[test]
fn enforce_state_path_is_directory_returns_ok() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "feature-x");
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir(state_dir.join("feature-x.json")).unwrap();
    let result = enforce(dir.path(), dir.path());
    assert!(result.is_ok(), "expected ok, got: {:?}", result);
}

#[test]
fn enforce_canonicalize_fallback_nonexistent_relative_cwd() {
    // When `relative_cwd` names a subdirectory that does not yet exist
    // on disk, expected.canonicalize() fails and the fallback returns
    // the uncanonicalized `expected`. The prefix check against
    // canonicalized cwd still reaches a conclusion — here, Err because
    // cwd (the worktree root) is NOT inside the named subdirectory.
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "feature-x");
    write_state(dir.path(), "feature-x", "nonexistent-subdir");
    let result = enforce(dir.path(), dir.path());
    assert!(result.is_err(), "expected error, got: {:?}", result);
    let msg = result.unwrap_err();
    assert!(
        msg.contains("nonexistent-subdir"),
        "error should name expected directory: {}",
        msg
    );
}
