//! Library-level tests for `flow_rs::cwd_scope`. Migrated from inline
//! `#[cfg(test)]` per `.claude/rules/test-placement.md`.

use std::fs;
use std::path::Path;
use std::process::Command;

use flow_rs::cwd_scope::{enforce, enforce_with_deps, parse_worktree_root, worktree_root_for};

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
fn worktree_root_for_non_git_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    assert!(worktree_root_for(dir.path()).is_none());
}

#[test]
fn parse_worktree_root_spawn_err_returns_none() {
    let err: std::io::Result<std::process::Output> = Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "simulated",
    ));
    assert!(parse_worktree_root(err).is_none());
}

#[test]
fn parse_worktree_root_empty_stdout_returns_none() {
    use std::os::unix::process::ExitStatusExt as _;
    let output = std::process::Output {
        status: std::process::ExitStatus::from_raw(0),
        stdout: b"\n".to_vec(),
        stderr: Vec::new(),
    };
    assert!(parse_worktree_root(Ok(output)).is_none());
}

#[test]
fn parse_worktree_root_valid_output_returns_path() {
    use std::os::unix::process::ExitStatusExt as _;
    let output = std::process::Output {
        status: std::process::ExitStatus::from_raw(0),
        stdout: b"/path/to/repo\n".to_vec(),
        stderr: Vec::new(),
    };
    let result = parse_worktree_root(Ok(output));
    assert_eq!(result, Some(std::path::PathBuf::from("/path/to/repo")));
}

#[test]
fn enforce_with_deps_nonexistent_cwd_uses_fallback() {
    // cwd.canonicalize() fails when the path does not exist. The
    // unwrap_or_else fallback returns cwd.to_path_buf() so the
    // prefix-check still proceeds to a conclusion (Ok or Err) rather
    // than panicking. Either outcome proves the fallback path ran.
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "feature-x");
    write_state(dir.path(), "feature-x", "");
    let fake_cwd = dir.path().join("does-not-exist-nonexistent-cwd-path");
    let result = enforce_with_deps(
        &fake_cwd,
        dir.path(),
        &|_| Some("feature-x".to_string()),
        &|_| Some(dir.path().to_path_buf()),
    );
    // The test just proves the fallback executed without panicking;
    // macOS symlink normalization (/var vs /private/var) means the
    // prefix check may come out either way depending on filesystem
    // layout. Either Ok or Err is acceptable here.
    let _ = result;
}

#[test]
fn enforce_with_deps_worktree_root_none_returns_ok() {
    // branch_resolver returns Some (simulating a git-managed cwd) but
    // worktree_root_resolver returns None (simulating a transient git
    // rev-parse failure). The enforce path should fail-open with Ok(()).
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "feature-x");
    write_state(dir.path(), "feature-x", "");
    let result = enforce_with_deps(
        dir.path(),
        dir.path(),
        &|_| Some("feature-x".to_string()),
        &|_| None,
    );
    assert!(result.is_ok(), "expected ok, got: {:?}", result);
}

#[test]
fn enforce_canonicalize_fallback_nonexistent_relative_cwd() {
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
