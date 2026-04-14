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

/// `bin/flow test` honors the FLOW_CI_RUNNING recursion guard.
#[test]
fn test_recursion_guard_when_env_preset() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let bin_dir = repo.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let marker = repo.join("should-not-exist");
    let bin_test = bin_dir.join("test");
    std::fs::write(
        &bin_test,
        format!("#!/usr/bin/env bash\ntouch {}\nexit 0\n", marker.display()),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin_test, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["test"])
        .current_dir(&repo)
        .env("FLOW_CI_RUNNING", "1")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"skipped\":true"), "stdout: {}", stdout);
    assert!(
        stdout.contains("recursion guard"),
        "should mention recursion guard: {}",
        stdout
    );
    assert!(
        !marker.exists(),
        "bin/test must not run when recursion guard fires"
    );
}

/// When `./bin/test` exists but is not executable, `Command::status()`
/// returns Err and the subcommand reports "failed to run ./bin/test".
#[test]
fn test_reports_io_error_when_bin_test_not_executable() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let bin_dir = repo.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let bin_test = bin_dir.join("test");
    std::fs::write(&bin_test, "#!/usr/bin/env bash\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin_test, std::fs::Permissions::from_mode(0o644)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["test"])
        .current_dir(&repo)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("failed to run ./bin/test"),
        "should report IO error: {}",
        stdout
    );
}

/// Reports a cwd drift error when cwd is outside the flow's expected
/// subdirectory.
#[test]
fn test_reports_cwd_drift_error() {
    use std::fs;
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let branch_out = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&repo)
        .output()
        .unwrap();
    let branch = String::from_utf8_lossy(&branch_out.stdout)
        .trim()
        .to_string();
    fs::write(
        state_dir.join(format!("{}.json", branch)),
        serde_json::json!({"branch": branch, "relative_cwd": "api"}).to_string(),
    )
    .unwrap();
    let ios = repo.join("ios");
    fs::create_dir(&ios).unwrap();
    let bin_dir = repo.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let bin_test = bin_dir.join("test");
    fs::write(&bin_test, "#!/usr/bin/env bash\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&bin_test, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["test"])
        .current_dir(&ios)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("cwd drift"),
        "should report cwd drift: {}",
        stdout
    );
}

/// Covers the `unwrap_or_else(|_| PathBuf::from("."))` fallback in
/// `run()` when `std::env::current_dir()` fails (deleted cwd).
#[test]
fn test_handles_deleted_cwd() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    let outer = tempfile::tempdir().unwrap();
    let ghost = outer.path().join("ghost");
    fs::create_dir(&ghost).unwrap();

    let wrapper = outer.path().join("wrapper.sh");
    let flow_rs = env!("CARGO_BIN_EXE_flow-rs");
    fs::write(
        &wrapper,
        format!(
            "#!/usr/bin/env bash\n\
             set -e\n\
             cd \"{ghost}\"\n\
             rmdir \"{ghost}\"\n\
             exec \"{flow}\" test\n",
            ghost = ghost.display(),
            flow = flow_rs,
        ),
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(&wrapper)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":\"error\""),
        "expected error JSON from deleted cwd, got: {}\nstderr: {}",
        stdout,
        String::from_utf8_lossy(&output.stderr)
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
