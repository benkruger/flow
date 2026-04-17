//! Integration tests for `bin/flow cleanup`.
//!
//! Drives the compiled binary against a minimal project fixture so the
//! `run()` entry point and its dispatch into `cleanup::run_impl` are
//! exercised end-to-end. Matches the subprocess-hygiene pattern used in
//! `tests/main_dispatch.rs` — `FLOW_CI_RUNNING` is unset, `GH_TOKEN` is
//! invalidated, and `HOME` is pinned to the test tempdir.

use std::process::Command;

fn flow_rs_no_recursion() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd
}

/// `flow-rs cleanup <nonexistent-root>` passes Clap but fails the
/// existence check in `cleanup::run_impl` — the `run()` wrapper wraps
/// the error via `json_error` and exits 1.
#[test]
fn cleanup_nonexistent_root_exits_1() {
    let output = flow_rs_no_recursion()
        .args([
            "cleanup",
            "/nonexistent/path/does/not/exist",
            "--branch",
            "test-branch",
            "--worktree",
            ".worktrees/test-branch",
        ])
        .output()
        .expect("spawn flow-rs cleanup");
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":\"error\""),
        "expected structured error in stdout, got: {}",
        stdout
    );
}

/// `flow-rs cleanup --help` covers the Args clap parser and help path.
#[test]
fn cleanup_help_exits_0() {
    let output = flow_rs_no_recursion()
        .args(["cleanup", "--help"])
        .output()
        .expect("spawn flow-rs cleanup --help");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Usage:"),
        "expected Usage: header in --help output, got: {}",
        stdout
    );
}

/// `flow-rs cleanup` missing required args fails Clap parsing.
#[test]
fn cleanup_missing_args_exits_nonzero() {
    let output = flow_rs_no_recursion()
        .arg("cleanup")
        .output()
        .expect("spawn flow-rs cleanup");
    assert_ne!(
        output.status.code(),
        Some(0),
        "cleanup with no project root should reject, got: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `flow-rs cleanup` in a valid tempdir without a .flow-states directory
/// is a no-op cleanup path — the command must not panic and should
/// report structured JSON on stdout.
#[test]
fn cleanup_empty_tempdir_does_not_panic() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");

    let output = flow_rs_no_recursion()
        .args([
            "cleanup",
            root.to_str().unwrap(),
            "--branch",
            "no-such-branch",
            "--worktree",
            ".worktrees/no-such-branch",
        ])
        .env("GIT_CEILING_DIRECTORIES", &root)
        .env("GH_TOKEN", "invalid")
        .env("HOME", &root)
        .output()
        .expect("spawn flow-rs cleanup");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked at"),
        "cleanup must not panic on empty tempdir, got: {}",
        stderr
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // The command writes structured JSON; we only care that it parses
    // and produces some status (error or ok — both are non-panicking).
    assert!(
        stdout.contains("\"status\":"),
        "expected JSON status in stdout, got: {}",
        stdout
    );
}
