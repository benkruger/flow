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

/// `bin/flow test` errors with a "./bin/test not found" message when the
/// repo has no executable `bin/test` script.
#[test]
fn test_errors_when_bin_test_missing() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["test"])
        .current_dir(&repo)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("./bin/test"),
        "should mention ./bin/test: {}",
        stdout
    );
    assert!(
        stdout.contains("not found"),
        "should report not found: {}",
        stdout
    );
}

/// `bin/flow test` execs the repo-local `./bin/test` script.
#[test]
fn test_execs_repo_local_bin_test() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let bin_dir = repo.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let bin_test = bin_dir.join("test");
    std::fs::write(&bin_test, "#!/usr/bin/env bash\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin_test, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["test"])
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

/// `bin/flow test --file <path>` forwards the file argument to `./bin/test`.
#[test]
fn test_forwards_file_argument() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let bin_dir = repo.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let bin_test = bin_dir.join("test");
    let captured = repo.join("captured-args");
    std::fs::write(
        &bin_test,
        format!(
            "#!/usr/bin/env bash\necho \"$@\" > {}\nexit 0\n",
            captured.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin_test, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["test", "--file", "tests/foo.rs"])
        .current_dir(&repo)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
    let captured_content = std::fs::read_to_string(&captured).unwrap();
    assert!(
        captured_content.contains("--file"),
        "expected --file forwarded: {}",
        captured_content
    );
    assert!(
        captured_content.contains("tests/foo.rs"),
        "expected file path forwarded: {}",
        captured_content
    );
}

/// `bin/flow test -- <filter>` forwards trailing arguments to `./bin/test`.
#[test]
fn test_forwards_trailing_args() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let bin_dir = repo.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let bin_test = bin_dir.join("test");
    let captured = repo.join("captured-args");
    std::fs::write(
        &bin_test,
        format!(
            "#!/usr/bin/env bash\necho \"$@\" > {}\nexit 0\n",
            captured.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin_test, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["test", "--", "my_filter"])
        .current_dir(&repo)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
    let captured_content = std::fs::read_to_string(&captured).unwrap();
    assert!(
        captured_content.contains("my_filter"),
        "expected filter forwarded: {}",
        captured_content
    );
}

/// `bin/flow test` propagates a nonzero exit code from `./bin/test`.
#[test]
fn test_propagates_failure_exit() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let bin_dir = repo.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let bin_test = bin_dir.join("test");
    std::fs::write(&bin_test, "#!/usr/bin/env bash\nexit 1\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin_test, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["test"])
        .current_dir(&repo)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("error"), "should report error: {}", stdout);
}
