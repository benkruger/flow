//! Integration tests for `bin/flow format`.

mod common;

use std::process::Command;

/// `bin/flow format --help` succeeds and mentions "format".
#[test]
fn format_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["format", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("format"),
        "help should mention format: {}",
        stdout
    );
}

/// `bin/flow format` in a repo without framework files returns an error.
#[test]
fn format_no_framework_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["format"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("error"), "should report error: {}", stdout);
}
