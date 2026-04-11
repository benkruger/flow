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

/// `bin/flow format` errors with a "./bin/format not found" message when the
/// repo has no executable `bin/format` script.
#[test]
fn format_errors_when_bin_format_missing() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["format"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("./bin/format"),
        "should mention ./bin/format: {}",
        stdout
    );
    assert!(
        stdout.contains("not found"),
        "should report not found: {}",
        stdout
    );
}

/// `bin/flow format` execs the repo-local `./bin/format` script.
#[test]
fn format_execs_repo_local_bin_format() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let bin_dir = repo.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let bin_format = bin_dir.join("format");
    std::fs::write(&bin_format, "#!/usr/bin/env bash\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin_format, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["format"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":\"ok\""),
        "expected ok: {}",
        stdout
    );
}

/// `bin/flow format` propagates a nonzero exit code from `./bin/format`.
#[test]
fn format_propagates_failure_exit() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let bin_dir = repo.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let bin_format = bin_dir.join("format");
    std::fs::write(&bin_format, "#!/usr/bin/env bash\nexit 1\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin_format, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["format"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("error"), "should report error: {}", stdout);
}
