//! Integration tests for `src/qa_verify.rs`.
//!
//! Covers the CLI wrapper and the production subprocess runner inside
//! `run_impl` that inline unit tests cannot reach. Inline tests drive
//! `verify_impl` and `find_state_files` with injected closures and
//! tempdir fixtures, but never spawn the real runner or the `run()`
//! entry point.

use std::fs;
use std::process::Command;

use flow_rs::qa_verify;
use flow_rs::qa_verify::verify_impl;
use serde_json::{json, Value};

/// Subprocess: `bin/flow qa-verify --repo owner/nonexistent
/// --project-root <tempdir>` drives `run()` through `run_impl` which
/// builds the real subprocess runner. The runner's `gh pr list` call
/// fails against a nonexistent repo — a legitimate production path —
/// and the verify_impl pushes a "Could not fetch merged PRs" check.
/// `run()` always exits 0 because qa-verify is a pure reporting
/// command (see module doc comment).
#[test]
fn qa_verify_cli_exits_zero_and_reports_check_failures() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "qa-verify",
            "--repo",
            "owner/nonexistent-qa-verify-test",
            "--project-root",
            root.to_str().unwrap(),
        ])
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("failed to spawn flow-rs");

    assert_eq!(
        output.status.code(),
        Some(0),
        "qa-verify always exits 0, got {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":\"ok\""),
        "expected status=ok in stdout, got: {}",
        stdout
    );
    assert!(
        stdout.contains("\"checks\""),
        "expected checks array in stdout, got: {}",
        stdout
    );
}

/// Subprocess: tempdir carries a leftover state file. The check
/// reports `"passed": false` for that assertion.
#[test]
fn qa_verify_cli_reports_leftover_state_file_failure() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let state_dir = root.join(".flow-states");
    fs::create_dir(&state_dir).unwrap();
    fs::write(state_dir.join("leftover.json"), r#"{"branch":"x"}"#).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "qa-verify",
            "--repo",
            "owner/nonexistent-qa-verify-test",
            "--project-root",
            root.to_str().unwrap(),
        ])
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("failed to spawn flow-rs");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("leftover"),
        "expected 'leftover' in stdout, got: {}",
        stdout
    );
}

/// Library-level: drives `run_impl` directly. The real inline runner
/// closure fires, spawns `gh pr list` against a bogus repo, the `gh`
/// command returns non-zero, and the runner closure returns `None`.
/// The check table surfaces that path.
#[test]
fn qa_verify_run_impl_real_runner_none_branch_reports_fetch_failure() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();

    let args = qa_verify::Args {
        repo: "owner/nonexistent-qa-verify-lib-test".to_string(),
        project_root: root.to_string_lossy().to_string(),
    };
    let result = qa_verify::run_impl(&args).expect("run_impl returns Ok");
    assert_eq!(result["status"], "ok");

    let checks = result["checks"].as_array().expect("checks is array");
    let pr_check = checks
        .iter()
        .find(|c| c["name"].as_str().is_some_and(|n| n.contains("PR")))
        .expect("PR check exists");
    // The gh call either failed (None → fetch-failure message) or
    // succeeded with empty list (no merged PRs). Both branches set
    // passed=false, which is what we're verifying as the "real
    // runner was invoked and returned a structured response" path.
    assert_eq!(pr_check["passed"], false);
}

// --- Library-level unit tests (migrated from src/qa_verify.rs) ---

fn mock_ok_pr() -> Option<String> {
    Some(serde_json::to_string(&json!([{"number": 1}])).unwrap())
}

fn mock_empty_list() -> Option<String> {
    Some("[]".to_string())
}

#[test]
fn test_verify_all_pass() {
    let dir = tempfile::tempdir().unwrap();

    let result = verify_impl("owner/repo", dir.path(), &|_| mock_ok_pr());

    assert_eq!(result["status"], "ok");
    let checks = result["checks"].as_array().unwrap();
    assert!(checks.iter().all(|c| c["passed"] == true));
}

#[test]
fn test_verify_leftover_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();
    fs::write(state_dir.join("leftover.json"), r#"{"branch":"leftover"}"#).unwrap();

    let result = verify_impl("owner/repo", dir.path(), &|_| mock_ok_pr());

    let checks = result["checks"].as_array().unwrap();
    let state_check: Vec<&Value> = checks
        .iter()
        .filter(|c| c["name"].as_str().unwrap().to_lowercase().contains("state"))
        .collect();
    assert!(!state_check.is_empty());
    assert_eq!(state_check[0]["passed"], false);
}

#[test]
fn test_verify_leftover_worktree() {
    let dir = tempfile::tempdir().unwrap();
    let wt_dir = dir.path().join(".worktrees").join("some-feature");
    fs::create_dir_all(&wt_dir).unwrap();

    let result = verify_impl("owner/repo", dir.path(), &|_| mock_ok_pr());

    let checks = result["checks"].as_array().unwrap();
    let wt_check: Vec<&Value> = checks
        .iter()
        .filter(|c| {
            c["name"]
                .as_str()
                .unwrap()
                .to_lowercase()
                .contains("worktree")
        })
        .collect();
    assert!(!wt_check.is_empty());
    assert_eq!(wt_check[0]["passed"], false);
}

#[test]
fn test_verify_no_merged_pr() {
    let dir = tempfile::tempdir().unwrap();

    let result = verify_impl("owner/repo", dir.path(), &|_| mock_empty_list());

    let checks = result["checks"].as_array().unwrap();
    let pr_check: Vec<&Value> = checks
        .iter()
        .filter(|c| c["name"].as_str().unwrap().contains("PR"))
        .collect();
    assert!(!pr_check.is_empty());
    assert_eq!(pr_check[0]["passed"], false);
}

#[test]
fn test_verify_pr_fetch_failure() {
    let dir = tempfile::tempdir().unwrap();

    let result = verify_impl("owner/repo", dir.path(), &|_| None);

    let checks = result["checks"].as_array().unwrap();
    let pr_check: Vec<&Value> = checks
        .iter()
        .filter(|c| c["name"].as_str().unwrap().contains("PR"))
        .collect();
    assert!(!pr_check.is_empty());
    assert_eq!(pr_check[0]["passed"], false);
}

#[test]
fn test_verify_no_flow_states_dir() {
    let dir = tempfile::tempdir().unwrap();

    let result = verify_impl("owner/repo", dir.path(), &|_| mock_ok_pr());

    let checks = result["checks"].as_array().unwrap();
    let state_check: Vec<&Value> = checks
        .iter()
        .filter(|c| c["name"].as_str().unwrap().to_lowercase().contains("state"))
        .collect();
    assert!(!state_check.is_empty());
    assert_eq!(state_check[0]["passed"], true);
}

#[test]
fn test_verify_excludes_orchestrate_files() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();
    fs::write(state_dir.join("orchestrate-queue.json"), "{}").unwrap();

    let result = verify_impl("owner/repo", dir.path(), &|_| mock_ok_pr());

    let checks = result["checks"].as_array().unwrap();
    let state_check: Vec<&Value> = checks
        .iter()
        .filter(|c| c["name"].as_str().unwrap().to_lowercase().contains("state"))
        .collect();
    assert_eq!(state_check[0]["passed"], true);
}

#[test]
fn test_verify_excludes_phases_files() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();
    fs::write(state_dir.join("feature-phases.json"), "{}").unwrap();

    let result = verify_impl("owner/repo", dir.path(), &|_| mock_ok_pr());

    let checks = result["checks"].as_array().unwrap();
    let state_check: Vec<&Value> = checks
        .iter()
        .filter(|c| c["name"].as_str().unwrap().to_lowercase().contains("state"))
        .collect();
    assert_eq!(state_check[0]["passed"], true);
}

/// Drives `subprocess_runner` Err path: nonexistent binary returns None.
#[test]
fn subprocess_runner_nonexistent_binary_returns_none() {
    let result = flow_rs::qa_verify::subprocess_runner(&[
        "/nonexistent/binary/path/does-not-exist-deadbeef",
    ]);
    assert!(result.is_none());
}

/// Drives `subprocess_runner` success branch via `/bin/echo` (always
/// present on POSIX systems). Exits 0, returns captured stdout.
#[test]
fn subprocess_runner_success_branch_returns_stdout() {
    let result = flow_rs::qa_verify::subprocess_runner(&["/bin/echo", "hello"]);
    assert_eq!(result.as_deref(), Some("hello\n"));
}

/// Drives `subprocess_runner` non-zero exit: `/usr/bin/false` always
/// exits 1. Returns None regardless of stdout.
#[test]
fn subprocess_runner_nonzero_exit_returns_none() {
    // /usr/bin/false is present on all POSIX test environments.
    let result = flow_rs::qa_verify::subprocess_runner(&["/usr/bin/false"]);
    assert!(result.is_none());
}

#[test]
fn test_verify_excludes_dot_prefixed_json() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();
    fs::write(state_dir.join(".hidden-state.json"), "{}").unwrap();

    let result = verify_impl("owner/repo", dir.path(), &|_| mock_ok_pr());

    let checks = result["checks"].as_array().unwrap();
    let state_check: Vec<&Value> = checks
        .iter()
        .filter(|c| c["name"].as_str().unwrap().to_lowercase().contains("state"))
        .collect();
    assert_eq!(state_check[0]["passed"], true);
}
