//! Structural presence tests for session logging.
//!
//! Each module that performs significant operations must call `append_log`
//! to record entries in `.flow-states/<branch>.log`. These source-content
//! tests assert that the call site exists in each module's source — they
//! verify structural presence, not behavioral log production.

mod common;

use std::fs;
use std::process::Command;

use flow_rs::commands::log::{append_log, run_impl_main};

/// phase_transition.rs must call append_log for phase-transition logging.
#[test]
fn phase_transition_uses_append_log() {
    let src = fs::read_to_string(common::repo_root().join("src/phase_transition.rs")).unwrap();
    assert!(
        src.contains("append_log("),
        "src/phase_transition.rs::run_impl_main must call append_log for phase-transition session logging"
    );
}

/// complete_post_merge.rs must call append_log for post-merge step logging.
#[test]
fn complete_post_merge_uses_append_log() {
    let src = fs::read_to_string(common::repo_root().join("src/complete_post_merge.rs")).unwrap();
    assert!(
        src.contains("append_log("),
        "src/complete_post_merge.rs must call append_log for post-merge session logging"
    );
}

/// cleanup.rs must call append_log for cleanup step logging.
#[test]
fn cleanup_uses_append_log() {
    let src = fs::read_to_string(common::repo_root().join("src/cleanup.rs")).unwrap();
    assert!(
        src.contains("append_log("),
        "src/cleanup.rs must call append_log for cleanup session logging"
    );
}

/// complete_finalize.rs must call append_log for orchestration logging.
#[test]
fn complete_finalize_uses_append_log() {
    let src = fs::read_to_string(common::repo_root().join("src/complete_finalize.rs")).unwrap();
    assert!(
        src.contains("append_log("),
        "src/complete_finalize.rs must call append_log for orchestration session logging"
    );
}

/// finalize_commit.rs must call append_log for commit-cycle logging.
#[test]
fn finalize_commit_uses_append_log() {
    let src = fs::read_to_string(common::repo_root().join("src/finalize_commit.rs")).unwrap();
    assert!(
        src.contains("append_log("),
        "src/finalize_commit.rs must call append_log for commit-cycle session logging"
    );
}

// --- behavioral tests for bin/flow log ---

fn flow_rs() -> Command {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
}

/// `bin/flow log <branch> <message>` appends a line and exits 0 — covers
/// the `run()` success path.
#[test]
fn bin_flow_log_success_exits_zero_and_writes_line() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());

    let output = flow_rs()
        .args(["log", "my-feature", "hello world"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert_eq!(output.status.code().unwrap(), 0);
    assert!(output.stdout.is_empty());

    let log = fs::read_to_string(repo.join(".flow-states/my-feature/log")).unwrap();
    assert!(log.contains("hello world"), "log missing message: {}", log);
}

/// `bin/flow log` exits 1 and prints an error on append_log failure.
/// Triggered by creating a regular FILE at `.flow-states/` so
/// `fs::create_dir_all` fails with "not a directory".
#[test]
fn bin_flow_log_exits_nonzero_on_create_dir_failure() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    // Occupy the flow_states path with a file so create_dir_all errors.
    fs::write(repo.join(".flow-states"), "sentinel").unwrap();

    let output = flow_rs()
        .args(["log", "my-feature", "hi"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code().unwrap(),
        1,
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("flow log:"),
        "should report error prefix: {}",
        stderr
    );
}

/// Covers the `?` on `OpenOptions::open(&log_path)`: when the log path is a
/// directory, opening it as a file fails with EISDIR.
#[test]
fn bin_flow_log_exits_nonzero_when_log_path_is_directory() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    // Create .flow-states/my-feature/log AS A DIRECTORY. The open call
    // then errors with "Is a directory".
    fs::create_dir_all(repo.join(".flow-states/my-feature/log")).unwrap();

    let output = flow_rs()
        .args(["log", "my-feature", "hi"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert_eq!(output.status.code().unwrap(), 1);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("flow log:"), "stderr: {}", stderr);
}

// --- Library-level unit tests (migrated from src/commands/log.rs) ---

#[test]
fn appends_to_existing_log() {
    let dir = tempfile::tempdir().unwrap();
    let branch_dir = dir.path().join(".flow-states").join("my-feature");
    fs::create_dir_all(&branch_dir).unwrap();
    let log_file = branch_dir.join("log");
    fs::write(&log_file, "existing line\n").unwrap();

    append_log(dir.path(), "my-feature", "[Phase 1] Step 5 — test (exit 0)").unwrap();

    let content = fs::read_to_string(&log_file).unwrap();
    assert!(content.starts_with("existing line\n"));
    assert!(content.contains("[Phase 1] Step 5 — test (exit 0)"));
    // Should have exactly 2 lines
    let lines: Vec<&str> = content.trim().lines().collect();
    assert_eq!(lines.len(), 2);
}

#[test]
fn creates_new_log_file() {
    let dir = tempfile::tempdir().unwrap();
    let log_dir = dir.path().join(".flow-states");
    fs::create_dir(&log_dir).unwrap();

    append_log(dir.path(), "feat-branch", "[Phase 1] test message").unwrap();

    let log_file = log_dir.join("feat-branch").join("log");
    assert!(log_file.exists());
    let content = fs::read_to_string(&log_file).unwrap();
    assert!(content.contains("[Phase 1] test message"));
}

#[test]
fn creates_directory_if_missing() {
    let dir = tempfile::tempdir().unwrap();

    append_log(dir.path(), "branch", "message").unwrap();

    assert!(dir.path().join(".flow-states").is_dir());
    assert!(dir
        .path()
        .join(".flow-states")
        .join("branch")
        .join("log")
        .exists());
}

#[test]
fn multiple_appends() {
    let dir = tempfile::tempdir().unwrap();
    let log_dir = dir.path().join(".flow-states");
    fs::create_dir(&log_dir).unwrap();

    append_log(dir.path(), "branch", "first").unwrap();
    append_log(dir.path(), "branch", "second").unwrap();

    let content = fs::read_to_string(log_dir.join("branch").join("log")).unwrap();
    let lines: Vec<&str> = content.trim().lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].ends_with("first"));
    assert!(lines[1].ends_with("second"));
}

#[test]
fn run_impl_main_success_returns_empty_stderr_zero_code() {
    let dir = tempfile::tempdir().unwrap();
    let (msg, code) = run_impl_main(dir.path(), "branch", "message");
    assert_eq!(msg, "");
    assert_eq!(code, 0);
}

#[test]
fn run_impl_main_failure_returns_stderr_one_code() {
    // Place a regular file at .flow-states/ so create_dir_all fails.
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".flow-states"), "I am a file, not a dir").unwrap();
    let (msg, code) = run_impl_main(dir.path(), "branch", "message");
    assert_eq!(code, 1);
    assert!(msg.starts_with("flow log:"), "got: {}", msg);
}

#[test]
fn timestamp_is_included() {
    let dir = tempfile::tempdir().unwrap();

    append_log(dir.path(), "branch", "test").unwrap();

    let content = fs::read_to_string(dir.path().join(".flow-states/branch/log")).unwrap();
    let line = content.trim();
    // Should have format: "YYYY-MM-DDTHH:MM:SS-HH:MM test"
    assert!(line.contains('T'), "Timestamp should contain 'T': {}", line);
    assert!(line.ends_with("test"));
}
