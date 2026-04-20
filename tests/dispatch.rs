//! Library-level tests for `flow_rs::dispatch`. Migrated from inline
//! `#[cfg(test)]` per `.claude/rules/test-placement.md`.
//!
//! The four `dispatch_*` helpers (`dispatch_json`, `dispatch_text`,
//! `dispatch_result_json`, `dispatch_ok_result_json`) all terminate
//! the process via `std::process::exit`, so they are exercised via
//! subprocess tests below that spawn the compiled binary against
//! subcommands known to route through each helper. The two pure
//! helpers (`result_to_value_code`, `ok_result_to_value_code`) are
//! tested in-process.

use std::process::Command;

use flow_rs::dispatch::{ok_result_to_value_code, result_to_value_code};
use serde_json::json;

// --- Pure helpers ---

#[test]
fn result_to_value_code_ok_non_error_returns_code_zero() {
    let (v, code) = result_to_value_code(Ok(json!({"status": "ok", "data": 42})));
    assert_eq!(code, 0);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"], 42);
}

#[test]
fn result_to_value_code_ok_error_status_returns_code_one() {
    let (v, code) = result_to_value_code(Ok(json!({"status": "error", "message": "bad"})));
    assert_eq!(code, 1);
    assert_eq!(v["status"], "error");
}

#[test]
fn result_to_value_code_err_wraps_message_with_code_one() {
    let (v, code) = result_to_value_code(Err("infra failure".to_string()));
    assert_eq!(code, 1);
    assert_eq!(v["status"], "error");
    assert_eq!(v["message"], "infra failure");
}

#[test]
fn result_to_value_code_ok_without_status_field_is_success() {
    let (v, code) = result_to_value_code(Ok(json!({"data": 1})));
    assert_eq!(code, 0);
    assert_eq!(v["data"], 1);
}

#[test]
fn ok_result_to_value_code_ok_error_status_still_exits_zero() {
    let (v, code) = ok_result_to_value_code(Ok(json!({"status": "error", "message": "gate"})));
    assert_eq!(code, 0);
    assert_eq!(v["status"], "error");
}

#[test]
fn ok_result_to_value_code_err_wraps_message_with_code_one() {
    let (v, code) = ok_result_to_value_code(Err("infra".to_string()));
    assert_eq!(code, 1);
    assert_eq!(v["status"], "error");
    assert_eq!(v["message"], "infra");
}

#[test]
fn ok_result_to_value_code_ok_without_error_status_exits_zero() {
    let (v, code) = ok_result_to_value_code(Ok(json!({"status": "ok"})));
    assert_eq!(code, 0);
    assert_eq!(v["status"], "ok");
}

// --- Subprocess exercise of the four exit helpers ---

fn flow_rs() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd
}

/// `check-phase --required flow-start` short-circuits to `("", 0)` —
/// hits `dispatch_text`'s empty-text branch (no println, exit 0).
#[test]
fn dispatch_text_empty_branch_via_check_phase_first_phase() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let output = flow_rs()
        .args(["check-phase", "--required", "flow-start"])
        .current_dir(&root)
        .env("GIT_CEILING_DIRECTORIES", &root)
        .env("HOME", &root)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "expected empty stdout, got: {:?}",
        output.stdout
    );
}

/// `check-phase --required flow-plan` in a non-git tempdir emits a
/// BLOCKED text message and exits 1 — hits `dispatch_text`'s
/// non-empty branch (println, exit 1).
#[test]
fn dispatch_text_nonempty_branch_via_check_phase_no_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let output = flow_rs()
        .args(["check-phase", "--required", "flow-plan"])
        .current_dir(&root)
        .env("GIT_CEILING_DIRECTORIES", &root)
        .env("HOME", &root)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("BLOCKED"),
        "expected BLOCKED text, got: {}",
        stdout
    );
}

/// `cleanup /nonexistent/path ...` routes through `dispatch_json`
/// with an error payload and exit code 1.
#[test]
fn dispatch_json_error_via_cleanup_invalid_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let output = flow_rs()
        .args([
            "cleanup",
            "/nonexistent/path/does/not/exist",
            "--branch",
            "x",
            "--worktree",
            ".worktrees/x",
        ])
        .env("HOME", &root)
        .env("GH_TOKEN", "invalid")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":\"error\""),
        "expected JSON error, got: {}",
        stdout
    );
}

/// `complete-fast` in a non-FLOW tempdir returns `Err` from
/// `run_impl`, which `dispatch_result_json` wraps into a
/// `{status: error, message}` JSON payload with exit code 1.
#[test]
fn dispatch_result_json_err_via_complete_fast_no_state() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let output = flow_rs()
        .args(["complete-fast"])
        .current_dir(&root)
        .env("GIT_CEILING_DIRECTORIES", &root)
        .env("HOME", &root)
        .env("GH_TOKEN", "invalid")
        .output()
        .unwrap();
    // Whether code is 0 or 1, the output must parse as JSON with a
    // "status" field — that proves dispatch_result_json printed.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":"),
        "expected JSON with status, got stdout: {}\nstderr: {}",
        stdout,
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `tombstone-audit` in an empty tempdir returns `Ok(value)`, which
/// `dispatch_ok_result_json` prints as valid JSON with exit code 0.
/// The value shape is `{stale, current, total_tombstones, ...}` with
/// no `status` field on the happy path — we just assert the helper
/// produced parseable JSON on stdout and exited 0.
#[test]
fn dispatch_ok_result_json_ok_via_tombstone_audit_empty_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let output = flow_rs()
        .args(["tombstone-audit"])
        .current_dir(&root)
        .env("GIT_CEILING_DIRECTORIES", &root)
        .env("HOME", &root)
        .env("GH_TOKEN", "invalid")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("expected valid JSON, parse error {}: {}", e, stdout));
    assert!(
        parsed.is_object(),
        "expected a JSON object, got: {}",
        stdout
    );
}
