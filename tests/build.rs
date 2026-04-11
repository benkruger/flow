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

/// `bin/flow build` errors with a "./bin/build not found" message when the
/// repo has no executable `bin/build` script.
#[test]
fn build_errors_when_bin_build_missing() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["build"])
        .current_dir(&repo)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("./bin/build"),
        "should mention ./bin/build: {}",
        stdout
    );
    assert!(
        stdout.contains("not found"),
        "should report not found: {}",
        stdout
    );
}

/// `bin/flow build` execs the repo-local `./bin/build` script.
#[test]
fn build_execs_repo_local_bin_build() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let bin_dir = repo.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let bin_build = bin_dir.join("build");
    std::fs::write(&bin_build, "#!/usr/bin/env bash\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin_build, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["build"])
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

/// `bin/flow build` propagates a nonzero exit code from `./bin/build`.
#[test]
fn build_propagates_failure_exit() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let bin_dir = repo.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let bin_build = bin_dir.join("build");
    std::fs::write(&bin_build, "#!/usr/bin/env bash\nexit 1\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin_build, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["build"])
        .current_dir(&repo)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("error"), "should report error: {}", stdout);
}

/// `bin/flow build` sets `FLOW_CI_RUNNING=1` in the spawned process.
#[test]
fn build_sets_flow_ci_running_env() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let bin_dir = repo.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let bin_build = bin_dir.join("build");
    let marker = repo.join("env-marker");
    std::fs::write(
        &bin_build,
        format!(
            "#!/usr/bin/env bash\nif [ \"${{FLOW_CI_RUNNING:-}}\" = \"1\" ]; then touch {}; fi\nexit 0\n",
            marker.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin_build, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["build"])
        .current_dir(&repo)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(marker.exists(), "FLOW_CI_RUNNING was not set in child");
}
