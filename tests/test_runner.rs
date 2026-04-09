//! Integration tests for `bin/flow test`.

mod common;

use std::process::Command;

/// `bin/flow test --help` succeeds and mentions "test".
#[test]
fn test_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["test", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("test"),
        "help should mention test: {}",
        stdout
    );
}

/// `bin/flow test` in a repo without framework files returns an error.
#[test]
fn test_no_framework_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["test"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("error"), "should report error: {}", stdout);
}
