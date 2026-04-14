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

/// `bin/flow build` honors the FLOW_CI_RUNNING recursion guard: when the
/// env var is already set by an outer caller, the subcommand emits a
/// skipped JSON and exits 0 without executing `./bin/build`.
#[test]
fn build_recursion_guard_when_env_preset() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let bin_dir = repo.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let marker = repo.join("should-not-exist");
    let bin_build = bin_dir.join("build");
    // If this script ran, it would create the marker.
    std::fs::write(
        &bin_build,
        format!("#!/usr/bin/env bash\ntouch {}\nexit 0\n", marker.display()),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin_build, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["build"])
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
        "bin/build must not run when recursion guard fires"
    );
}

/// When `./bin/build` exists as a regular file but is not executable,
/// `Command::status()` returns Err and the subcommand reports
/// "failed to run ./bin/build".
#[test]
fn build_reports_io_error_when_bin_build_not_executable() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    let bin_dir = repo.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let bin_build = bin_dir.join("build");
    std::fs::write(&bin_build, "#!/usr/bin/env bash\nexit 0\n").unwrap();
    // No execute bit — is_file() passes, exec fails with Err(e)
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin_build, std::fs::Permissions::from_mode(0o644)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["build"])
        .current_dir(&repo)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("failed to run ./bin/build"),
        "should report IO error: {}",
        stdout
    );
}

/// `run_impl` returns an error JSON when the cwd drift guard rejects the
/// working directory (cwd outside the flow's expected subdirectory).
#[test]
fn build_reports_cwd_drift_error() {
    use std::fs;
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    // Write a state file that scopes the flow to `api/`.
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    // Resolve current branch by reading HEAD.
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
    // Create a sibling dir `ios/` and run from there.
    let ios = repo.join("ios");
    fs::create_dir(&ios).unwrap();
    // bin/build exists in repo so we don't stop on the missing-bin branch.
    let bin_dir = repo.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let bin_build = bin_dir.join("build");
    fs::write(&bin_build, "#!/usr/bin/env bash\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&bin_build, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["build"])
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
/// `run()` when `std::env::current_dir()` fails. Triggered by spawning
/// flow-rs from inside a directory that has been unlinked — Unix keeps
/// the process alive but getcwd returns ENOENT.
#[test]
fn build_handles_deleted_cwd() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    let outer = tempfile::tempdir().unwrap();
    // The transient cwd that we delete.
    let ghost = outer.path().join("ghost");
    fs::create_dir(&ghost).unwrap();

    // A wrapper script that cds into the ghost dir, unlinks it, then
    // execs flow-rs. The exec'd flow-rs inherits the deleted cwd.
    let wrapper = outer.path().join("wrapper.sh");
    let flow_rs = env!("CARGO_BIN_EXE_flow-rs");
    fs::write(
        &wrapper,
        format!(
            "#!/usr/bin/env bash\n\
             set -e\n\
             cd \"{ghost}\"\n\
             rmdir \"{ghost}\"\n\
             exec \"{flow}\" build\n",
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
    // Process must not panic — the fallback PathBuf::from(".") keeps it alive.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":\"error\""),
        "expected error JSON from deleted cwd, got: {}\nstderr: {}",
        stdout,
        String::from_utf8_lossy(&output.stderr)
    );
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
