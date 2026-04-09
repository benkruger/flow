//! Integration tests for `bin/flow lint`.

mod common;

use std::process::Command;

/// `bin/flow lint --help` succeeds and mentions "lint".
#[test]
fn lint_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["lint", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("lint"),
        "help should mention lint: {}",
        stdout
    );
}

/// `bin/flow lint` in a repo without framework files returns an error.
#[test]
fn lint_no_framework_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["lint"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("error"), "should report error: {}", stdout);
}
