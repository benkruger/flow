//! Integration tests for `bin/flow build`.

mod common;

use std::process::Command;

/// `bin/flow build --help` succeeds and mentions "build".
#[test]
fn build_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["build", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("build"),
        "help should mention build: {}",
        stdout
    );
}

/// `bin/flow build` in a repo without framework files returns an error.
#[test]
fn build_no_framework_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["build"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("error"), "should report error: {}", stdout);
}
