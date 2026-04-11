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

/// `bin/flow lint` errors with a "./bin/lint not found" message when the
/// repo has no executable `bin/lint` script.
#[test]
fn lint_errors_when_bin_lint_missing() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["lint"])
        .current_dir(&repo)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("./bin/lint"),
        "should mention ./bin/lint: {}",
        stdout
    );
    assert!(
        stdout.contains("not found"),
        "should report not found: {}",
        stdout
    );
}

/// `bin/flow lint` execs the repo-local `./bin/lint` script.
#[test]
fn lint_execs_repo_local_bin_lint() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let bin_dir = repo.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let bin_lint = bin_dir.join("lint");
    std::fs::write(&bin_lint, "#!/usr/bin/env bash\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin_lint, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["lint"])
        .current_dir(&repo)
        .env_remove("FLOW_CI_RUNNING")
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

/// `bin/flow lint` propagates a nonzero exit code from `./bin/lint`.
#[test]
fn lint_propagates_failure_exit() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let bin_dir = repo.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let bin_lint = bin_dir.join("lint");
    std::fs::write(&bin_lint, "#!/usr/bin/env bash\nexit 1\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin_lint, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["lint"])
        .current_dir(&repo)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("error"), "should report error: {}", stdout);
}
