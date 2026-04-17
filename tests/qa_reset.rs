//! Integration tests for `src/qa_reset.rs`.
//!
//! Covers the CLI wrapper surface and the production `default_runner`
//! that inline unit tests cannot reach. Inline tests in
//! `src/qa_reset.rs` drive `reset_git`, `close_prs`,
//! `delete_remote_branches`, `load_issue_template`, `reset_issues`,
//! `clean_local`, and `reset_impl` with injected runner closures.
//! This file covers `run()`'s process-exit paths and drives
//! `default_runner` against a real subprocess.

use std::fs;
use std::path::Path;
use std::process::Command;

use flow_rs::qa_reset::{self, CmdResult};

/// Subprocess: `bin/flow qa-reset --repo owner/repo --local-path
/// <nonexistent>` exercises `run()`'s `Ok(result)` arm when the
/// underlying `reset_git` fails — the result carries
/// `status=error` and `run()` calls `process::exit(1)`.
#[test]
fn qa_reset_cli_nonexistent_local_path_exits_nonzero_with_error_json() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let missing = root.join("not-a-repo");

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "qa-reset",
            "--repo",
            "owner/nonexistent",
            "--local-path",
            missing.to_str().unwrap(),
        ])
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("failed to spawn flow-rs");

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit 1 on missing local path, got {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":\"error\""),
        "expected status=error in stdout, got: {}",
        stdout
    );
}

/// Library-level: drives `qa_reset::default_runner` against a real
/// subprocess that succeeds. The production runner captures stdout,
/// stderr, and the exit status into a `CmdResult`; inline tests only
/// cover the mock-runner path, so this test ensures the real runner
/// invariant holds.
#[test]
fn qa_reset_default_runner_captures_stdout_on_success() {
    let result: CmdResult = qa_reset::default_runner(&["echo", "hello"], None);
    assert!(
        result.success,
        "expected success=true for `echo hello`, got stderr: {}",
        result.stderr
    );
    assert!(
        result.stdout.contains("hello"),
        "expected 'hello' in stdout, got: {}",
        result.stdout
    );
}

/// Library-level: drives `qa_reset::default_runner` against a command
/// that does not exist. The runner catches the spawn error and
/// returns `success=false` with the error message in `stderr`.
#[test]
fn qa_reset_default_runner_spawn_failure_returns_error_in_stderr() {
    let result: CmdResult =
        qa_reset::default_runner(&["definitely_not_a_real_command_for_qa_reset_test"], None);
    assert!(
        !result.success,
        "expected success=false for missing command, got stdout: {}",
        result.stdout
    );
    assert!(
        !result.stderr.is_empty(),
        "expected non-empty stderr for missing command, got empty"
    );
}

/// Library-level: drives `qa_reset::default_runner` with a command
/// that exits non-zero. The runner reports `success=false` and
/// preserves stdout/stderr captured from the child.
#[test]
fn qa_reset_default_runner_nonzero_exit_reports_failure() {
    // `false` is a POSIX command that exits 1 with no output.
    let result: CmdResult = qa_reset::default_runner(&["false"], None);
    assert!(
        !result.success,
        "expected success=false for `false`, got stdout: {}, stderr: {}",
        result.stdout, result.stderr
    );
}

/// Library-level: drives `qa_reset::default_runner` with an explicit
/// cwd so the `Some(dir)` branch of the internal cwd-setter fires.
/// Previous tests only hit the `None` branch.
#[test]
fn qa_reset_default_runner_with_cwd_runs_in_target_directory() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let marker_name = "qa_reset_cwd_marker";
    fs::write(root.join(marker_name), "hello").unwrap();

    let result: CmdResult = qa_reset::default_runner(&["ls"], Some(Path::new(&root)));
    assert!(
        result.success,
        "expected success=true for `ls` in tempdir, got stderr: {}",
        result.stderr
    );
    assert!(
        result.stdout.contains(marker_name),
        "expected marker file in ls output, got: {}",
        result.stdout
    );
}
