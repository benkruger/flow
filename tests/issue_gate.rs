//! Integration tests for the Code Review filing gate in
//! `flow-rs issue`. Unit tests in `src/issue.rs` cover the pure
//! helper. These tests exercise the full binary path:
//! `issue::run` → `project_root` → `resolve_branch` →
//! `fs::read_to_string` → `should_reject_for_code_review` →
//! `process::exit(1)`. A refactor that accidentally skips the
//! gate in `run()` (e.g. moving the state-file read after repo
//! resolution or gating on the wrong field) would be caught
//! here, not by the unit tests.

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::flow_states_dir;
use serde_json::{json, Value};

fn flow_rs() -> Command {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
}

fn init_git(dir: &Path, branch: &str) {
    let _ = Command::new("git").args(["init"]).current_dir(dir).output();
    let _ = Command::new("git")
        .args(["checkout", "-b", branch])
        .current_dir(dir)
        .output();
    let _ = Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(dir)
        .output();
    let _ = Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir)
        .output();
}

fn write_state(dir: &Path, branch: &str, state: &Value) {
    let branch_dir = flow_states_dir(dir).join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(
        branch_dir.join("state.json"),
        serde_json::to_string_pretty(state).unwrap(),
    )
    .unwrap();
}

fn write_raw_state(dir: &Path, branch: &str, bytes: &[u8]) {
    let branch_dir = flow_states_dir(dir).join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), bytes).unwrap();
}

fn run_issue(dir: &Path, args: &[&str]) -> (i32, String) {
    let mut cmd = flow_rs();
    cmd.arg("issue");
    for arg in args {
        cmd.arg(arg);
    }
    cmd.current_dir(dir);
    // Avoid FLOW_CI_RUNNING inheritance tripping the recursion guard
    // when this test runs inside `bin/flow ci`.
    cmd.env_remove("FLOW_CI_RUNNING");
    let output = cmd.output().unwrap();
    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (code, format!("{}{}", stdout, stderr))
}

#[test]
fn issue_binary_rejects_during_code_review() {
    let dir = tempfile::tempdir().unwrap();
    init_git(dir.path(), "test-feature");
    write_state(
        dir.path(),
        "test-feature",
        &json!({
            "schema_version": 1,
            "branch": "test-feature",
            "current_phase": "flow-code-review"
        }),
    );

    let (code, combined) = run_issue(
        dir.path(),
        &["--title", "Should be blocked", "--repo", "fake/repo"],
    );
    assert_ne!(code, 0, "gate must fail the process during Code Review");
    assert!(
        combined.contains("disabled during Code Review"),
        "expected rejection message; got: {}",
        combined
    );
}

#[test]
fn issue_binary_allows_override_during_code_review() {
    let dir = tempfile::tempdir().unwrap();
    init_git(dir.path(), "test-feature");
    write_state(
        dir.path(),
        "test-feature",
        &json!({
            "schema_version": 1,
            "branch": "test-feature",
            "current_phase": "flow-code-review"
        }),
    );

    let (_code, combined) = run_issue(
        dir.path(),
        &[
            "--title",
            "Override",
            "--repo",
            "fake/repo",
            "--override-code-review-ban",
        ],
    );
    // With the override set the gate is bypassed, so the command
    // proceeds into `gh issue create` — which fails or succeeds
    // depending on whether `gh` is installed. Either way the
    // rejection message must NOT appear; that is the bypass
    // under test.
    assert!(
        !combined.contains("disabled during Code Review"),
        "override must bypass the gate; got: {}",
        combined
    );
}

#[test]
fn issue_binary_allows_during_other_phases() {
    for phase in &["flow-code", "flow-learn", "flow-start", "flow-plan"] {
        let dir = tempfile::tempdir().unwrap();
        init_git(dir.path(), "test-feature");
        write_state(
            dir.path(),
            "test-feature",
            &json!({
                "schema_version": 1,
                "branch": "test-feature",
                "current_phase": phase
            }),
        );

        let (_code, combined) =
            run_issue(dir.path(), &["--title", "Allowed", "--repo", "fake/repo"]);
        assert!(
            !combined.contains("disabled during Code Review"),
            "phase {} must not hit the Code Review gate; got: {}",
            phase,
            combined
        );
    }
}

#[test]
fn issue_binary_allows_when_no_state_file() {
    let dir = tempfile::tempdir().unwrap();
    init_git(dir.path(), "test-feature");
    // No state file written — command runs outside an active flow.

    let (_code, combined) = run_issue(
        dir.path(),
        &["--title", "Outside flow", "--repo", "fake/repo"],
    );
    assert!(
        !combined.contains("disabled during Code Review"),
        "out-of-flow invocation must not hit the gate; got: {}",
        combined
    );
}

#[test]
fn issue_binary_fails_closed_on_malformed_state() {
    let dir = tempfile::tempdir().unwrap();
    init_git(dir.path(), "test-feature");
    write_raw_state(dir.path(), "test-feature", b"not json");

    let (code, combined) = run_issue(dir.path(), &["--title", "Malformed", "--repo", "fake/repo"]);
    assert_ne!(code, 0, "malformed state must fail CLOSED");
    assert!(
        combined.contains("cannot determine the current FLOW phase"),
        "expected fail-closed message; got: {}",
        combined
    );
}

#[test]
fn issue_binary_blocks_whitespace_padded_phase() {
    let dir = tempfile::tempdir().unwrap();
    init_git(dir.path(), "test-feature");
    write_state(
        dir.path(),
        "test-feature",
        &json!({
            "schema_version": 1,
            "branch": "test-feature",
            "current_phase": " flow-code-review "
        }),
    );

    let (code, combined) = run_issue(dir.path(), &["--title", "Padded", "--repo", "fake/repo"]);
    assert_ne!(
        code, 0,
        "padded current_phase must not bypass the gate; got: {}",
        combined
    );
    assert!(
        combined.contains("disabled during Code Review"),
        "expected Code Review rejection; got: {}",
        combined
    );
}
