//! Subprocess-level coverage for the `orchestrate-report` and
//! `orchestrate-state` CLI wrappers.
//!
//! Spawning the compiled `flow-rs` binary with real args exercises the
//! `run()` wrapper's `println!` lines, which in-process `run_impl` tests
//! cannot reach. cargo-llvm-cov instruments subprocess calls to the
//! same binary, so the branches land in the coverage report.
//!
//! Follows the `tests/main_dispatch.rs` spawn pattern
//! (`env!("CARGO_BIN_EXE_flow-rs")`) and
//! `.claude/rules/testing-gotchas.md` macOS Subprocess Path
//! Canonicalization (tempdir root canonicalized before descendant path
//! construction).

use std::fs;
use std::process::Command;

use serde_json::json;

const FLOW_RS: &str = env!("CARGO_BIN_EXE_flow-rs");

/// Happy-path spawn of `flow-rs orchestrate-report` — verifies that the
/// `run()` wrapper's `println!(run_impl(&args))` line prints the
/// morning-report JSON with `"status":"ok"` and an exit code of 0.
#[test]
fn orchestrate_report_run_happy_path_prints_ok_status() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();

    let state = json!({
        "started_at": "2026-03-20T22:00:00-07:00",
        "completed_at": "2026-03-21T06:00:00-07:00",
        "queue": [{
            "issue_number": 42,
            "title": "Add PDF export",
            "status": "completed",
            "started_at": "2026-03-20T22:05:00-07:00",
            "completed_at": "2026-03-20T23:00:00-07:00",
            "outcome": "completed",
            "pr_url": "https://github.com/test/test/pull/42",
            "branch": "issue-42",
            "reason": null,
        }],
        "current_index": null,
    });
    let state_path = root.join("orchestrate.json");
    fs::write(&state_path, serde_json::to_string(&state).unwrap()).unwrap();

    let output = Command::new(FLOW_RS)
        .arg("orchestrate-report")
        .arg("--state-file")
        .arg(&state_path)
        .arg("--output-dir")
        .arg(&root)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("failed to spawn flow-rs");

    assert_eq!(
        output.status.code(),
        Some(0),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":\"ok\""),
        "expected stdout to contain \"status\":\"ok\", got: {}",
        stdout
    );
    assert!(
        stdout.contains("\"completed\":1"),
        "expected stdout to contain \"completed\":1, got: {}",
        stdout
    );
    // The summary file should exist as a side effect.
    assert!(root.join("orchestrate-summary.md").exists());
}

/// Happy-path spawn of `flow-rs orchestrate-state --read` — verifies
/// the `run()` wrapper's `println!(Ok(value))` line by dispatching a
/// read against a pre-written valid state file.
#[test]
fn orchestrate_state_run_read_happy_path_prints_ok_status() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();

    let state = json!({
        "started_at": "2026-03-20T22:00:00-07:00",
        "completed_at": null,
        "queue": [{
            "issue_number": 42,
            "title": "Sample issue",
            "status": "pending",
            "started_at": null,
            "completed_at": null,
            "outcome": null,
            "pr_url": null,
            "branch": null,
            "reason": null,
        }],
        "current_index": null,
    });
    let state_path = root.join("orchestrate.json");
    fs::write(&state_path, serde_json::to_string(&state).unwrap()).unwrap();

    let output = Command::new(FLOW_RS)
        .arg("orchestrate-state")
        .arg("--read")
        .arg("--state-file")
        .arg(&state_path)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("failed to spawn flow-rs");

    assert_eq!(
        output.status.code(),
        Some(0),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":\"ok\""),
        "expected stdout to contain \"status\":\"ok\", got: {}",
        stdout
    );
    assert!(
        stdout.contains("\"state\""),
        "expected stdout to contain the state payload, got: {}",
        stdout
    );
}

/// Err-path spawn of `flow-rs orchestrate-state --create` — verifies
/// the `run()` wrapper's `println!(Err(msg))` arm by pointing
/// `--queue-file` at a nonexistent path so `run_impl` returns Err,
/// forcing the wrapper into its error-printing branch.
#[test]
fn orchestrate_state_run_create_missing_queue_file_prints_error_status() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let missing_queue = root.join("nonexistent-queue.json");
    let state_dir = root.join(".flow-states");

    let output = Command::new(FLOW_RS)
        .arg("orchestrate-state")
        .arg("--create")
        .arg("--queue-file")
        .arg(&missing_queue)
        .arg("--state-dir")
        .arg(&state_dir)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("failed to spawn flow-rs");

    // run_impl's Err is caught and printed by run(); exit stays 0.
    assert_eq!(
        output.status.code(),
        Some(0),
        "expected exit 0 (run prints Err and exits 0), stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":\"error\""),
        "expected stdout to contain \"status\":\"error\", got: {}",
        stdout
    );
    assert!(
        stdout.contains("Failed to read queue file"),
        "expected stdout to name the read-failure step, got: {}",
        stdout
    );
}
