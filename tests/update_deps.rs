//! Integration tests for `src/update_deps.rs` — mirrors the
//! production module per `.claude/rules/test-placement.md`.
//!
//! Covers the compiled binary's `update-deps` dispatch plus the
//! library-level `run_update_deps` / `run_impl` entry points with
//! real git fixtures.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use flow_rs::update_deps::{run_impl, run_update_deps};

fn flow_rs_no_recursion() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd
}

/// `flow-rs update-deps --help` covers the Args clap parser and help path.
#[test]
fn update_deps_help_exits_0() {
    let output = flow_rs_no_recursion()
        .args(["update-deps", "--help"])
        .output()
        .expect("spawn flow-rs update-deps --help");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Usage:"),
        "expected Usage: header in --help output, got: {}",
        stdout
    );
}

/// `flow-rs update-deps` in a tempdir without Cargo.toml does not
/// panic — the module reports a structured result on stdout via its
/// dispatcher.
#[test]
fn update_deps_empty_tempdir_does_not_panic() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");

    let output = flow_rs_no_recursion()
        .arg("update-deps")
        .current_dir(&root)
        .env("GIT_CEILING_DIRECTORIES", &root)
        .env("GH_TOKEN", "invalid")
        .env("HOME", &root)
        .output()
        .expect("spawn flow-rs update-deps");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked at"),
        "update-deps must not panic outside a cargo project, got: {}",
        stderr
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":"),
        "update-deps must emit JSON status on stdout, got: {}",
        stdout
    );
}

// --- library-level tests ---

fn init_git_repo(dir: &Path) {
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git command failed");
        assert!(output.status.success(), "git {:?} failed", args);
    };
    run(&["init", "--initial-branch", "main"]);
    run(&["config", "user.email", "test@test.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "commit.gpgsign", "false"]);
    run(&["commit", "--allow-empty", "-m", "init"]);
}

fn write_deps_script(dir: &Path, body: &str) {
    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let deps = bin_dir.join("dependencies");
    fs::write(&deps, format!("#!/usr/bin/env bash\n{}\n", body)).unwrap();
    fs::set_permissions(&deps, fs::Permissions::from_mode(0o755)).unwrap();
}

fn commit_all(dir: &Path, msg: &str) {
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", msg])
        .current_dir(dir)
        .output()
        .unwrap();
}

#[test]
fn skipped_when_no_bin_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());
    assert!(!dir.path().join("bin").join("dependencies").exists());
    let (out, code) = run_update_deps(dir.path(), 300);
    assert_eq!(code, 0);
    assert_eq!(out["status"], "skipped");
    assert!(out["reason"].as_str().unwrap().contains("not found"));
}

#[test]
fn no_changes_after_run() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());
    write_deps_script(dir.path(), "# no-op");
    commit_all(dir.path(), "add deps");

    let (out, code) = run_update_deps(dir.path(), 300);
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert_eq!(out["changes"], false);
}

#[test]
fn changes_after_run() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());
    write_deps_script(dir.path(), "echo updated > deps.lock");
    commit_all(dir.path(), "add deps");

    let (out, code) = run_update_deps(dir.path(), 300);
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert_eq!(out["changes"], true);
}

#[test]
fn error_when_deps_fails() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());
    write_deps_script(dir.path(), "exit 1");
    commit_all(dir.path(), "add deps");

    let (out, code) = run_update_deps(dir.path(), 300);
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    let msg = out["message"].as_str().unwrap().to_lowercase();
    assert!(msg.contains("failed") || msg.contains("exit"));
}

#[test]
fn timeout_reports_error() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());
    write_deps_script(dir.path(), "sleep 300");
    commit_all(dir.path(), "add deps");

    let start = Instant::now();
    let (out, code) = run_update_deps(dir.path(), 1);
    let elapsed = start.elapsed();

    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(out["message"].as_str().unwrap().contains("timed out"));
    assert!(
        elapsed < Duration::from_secs(10),
        "timeout took too long: {:?}",
        elapsed
    );
}

#[test]
fn non_bash_deps_script() {
    // Python shebang — confirms we don't force bash
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());
    let bin_dir = dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let deps = bin_dir.join("dependencies");
    fs::write(
        &deps,
        "#!/usr/bin/env python3\nfrom pathlib import Path\nPath('py-deps.lock').write_text('v1')\n",
    )
    .unwrap();
    fs::set_permissions(&deps, fs::Permissions::from_mode(0o755)).unwrap();
    commit_all(dir.path(), "add deps");

    let (out, code) = run_update_deps(dir.path(), 300);
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert_eq!(out["changes"], true);
}

#[test]
fn non_executable_deps_reports_error() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());
    let bin_dir = dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let deps = bin_dir.join("dependencies");
    fs::write(&deps, "#!/usr/bin/env bash\necho ok\n").unwrap();
    fs::set_permissions(&deps, fs::Permissions::from_mode(0o644)).unwrap();
    commit_all(dir.path(), "add deps");

    let (out, code) = run_update_deps(dir.path(), 300);
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(out["message"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("executed"));
}

#[test]
fn directory_instead_of_file_reports_skipped() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());
    let deps_dir = dir.path().join("bin").join("dependencies");
    fs::create_dir_all(&deps_dir).unwrap();

    let (out, code) = run_update_deps(dir.path(), 300);
    assert_eq!(code, 0);
    assert_eq!(out["status"], "skipped");
}

#[test]
fn git_status_failure_reports_error() {
    // Non-git directory — deps exists but git status fails
    let dir = tempfile::tempdir().unwrap();
    write_deps_script(dir.path(), "# no-op");
    let (out, code) = run_update_deps(dir.path(), 300);
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(out["message"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("git status"));
}

/// Exercises the `exit_status.code() == None` branch. A
/// self-SIGKILL'd child exits via signal, so `.code()` is `None` and
/// the error message reports `"signal"` rather than an integer exit
/// code.
#[test]
fn deps_self_sigkill_reports_signal_in_message() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());
    // SIGKILL is not trappable — bash dies and the parent sees a
    // signal-terminated child (`exit_status.code() == None`).
    write_deps_script(dir.path(), "kill -KILL $$");
    commit_all(dir.path(), "add deps");

    let (out, code) = run_update_deps(dir.path(), 300);
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    let msg = out["message"].as_str().unwrap();
    assert!(msg.contains("signal"), "expected 'signal' in {}", msg);
}

#[test]
fn deps_stdout_does_not_corrupt_return_value() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());
    write_deps_script(
        dir.path(),
        "echo 'Installing dependencies...' > /dev/null 2>&1",
    );
    commit_all(dir.path(), "add deps");

    let (out, code) = run_update_deps(dir.path(), 300);
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert_eq!(out["changes"], false);
}

// --- run_impl() tests ---

#[test]
fn cli_default_timeout_when_env_absent() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());
    write_deps_script(dir.path(), "# no-op");
    commit_all(dir.path(), "add deps");

    let (out, code) = run_impl(dir.path(), None);
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
}

#[test]
fn cli_env_timeout_override() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());
    write_deps_script(dir.path(), "sleep 300");
    commit_all(dir.path(), "add deps");

    let (out, code) = run_impl(dir.path(), Some("1"));
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(out["message"].as_str().unwrap().contains("timed out"));
}

#[test]
fn cli_invalid_env_timeout_reports_error() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());
    write_deps_script(dir.path(), "# no-op");
    commit_all(dir.path(), "add deps");

    let (out, code) = run_impl(dir.path(), Some("notanumber"));
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(out["message"]
        .as_str()
        .unwrap()
        .contains("FLOW_UPDATE_DEPS_TIMEOUT"));
}
