//! Integration tests for `src/complete_merge.rs` — mirrors the
//! production module per `.claude/rules/test-placement.md`. Covers:
//!
//! - Subprocess path through the compiled `flow-rs complete-merge`
//!   binary with `check-freshness` and `gh pr merge` stubs.
//! - Library-level path through `complete_merge_inner` with mock
//!   runners to exercise every internal match arm.
//! - `run_impl_main_with_runner` seam for the exit code → status map.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use flow_rs::complete_merge::{complete_merge_inner, run_impl_main_with_runner, Args};
use flow_rs::complete_preflight::CmdResult;
use serde_json::{json, Value};

mod common;

/// Write the `bin/flow` stub script at `path`. Handles the
/// `check-freshness` subcommand via `$STUB_FRESHNESS_JSON` and
/// exits 0 for any other subcommand. Each test owns its own path
/// so parallel tests do not race.
fn write_bin_flow_stub(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let script = "#!/bin/sh\n\
        case \"$1\" in\n\
          check-freshness) printf '%s\\n' \"$STUB_FRESHNESS_JSON\" ;;\n\
          *) exit 0 ;;\n\
        esac\n";
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Build the `gh` stub at `<stubs_dir>/gh`. Handles `gh pr merge`
/// per `$STUB_GH_MERGE_EXIT`; returns the stubs dir for PATH use.
fn build_path_stub_dir(parent: &Path) -> PathBuf {
    let stubs = parent.join("stubs");
    fs::create_dir_all(&stubs).unwrap();
    let gh_script = "#!/bin/sh\n\
        if [ \"$1 $2\" = \"pr merge\" ]; then\n\
          exit \"${STUB_GH_MERGE_EXIT:-0}\"\n\
        fi\n\
        exit 0\n";
    let gh_path = stubs.join("gh");
    fs::write(&gh_path, gh_script).unwrap();
    fs::set_permissions(&gh_path, fs::Permissions::from_mode(0o755)).unwrap();
    stubs
}

#[allow(clippy::too_many_arguments)]
fn run_complete_merge(
    cwd: &Path,
    pr: &str,
    state_file: &str,
    path_stub_dir: &Path,
    flow_bin_path: &Path,
    freshness_json: &str,
    gh_merge_exit: i32,
) -> (i32, String, String) {
    let current_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", path_stub_dir.display(), current_path);
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["complete-merge", "--pr", pr, "--state-file", state_file])
        .current_dir(cwd)
        .env("PATH", new_path)
        .env("FLOW_BIN_PATH", flow_bin_path)
        .env("STUB_FRESHNESS_JSON", freshness_json)
        .env("STUB_GH_MERGE_EXIT", gh_merge_exit.to_string())
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("spawn flow-rs");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn last_json_line(stdout: &str) -> Value {
    let last = stdout
        .lines()
        .rfind(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON line in stdout; stdout={}", stdout));
    serde_json::from_str(last)
        .unwrap_or_else(|e| panic!("failed to parse JSON line '{}': {}", last, e))
}

/// Stubbed `check-freshness` returns `up_to_date` and stubbed
/// `gh pr merge` exits 0 → `complete_merge_inner` returns
/// `status == "merged"` → `run` exits 0.
#[test]
fn merge_run_merged_exits_0() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_bin_flow_stub(&flow_bin);
    let path_stub = build_path_stub_dir(&parent);
    let state_path = parent.join("state.json");
    fs::write(&state_path, "{\"branch\": \"feat\"}").unwrap();

    let (code, stdout, _) = run_complete_merge(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &path_stub,
        &flow_bin,
        r#"{"status": "up_to_date"}"#,
        0,
    );

    assert_eq!(code, 0, "merged status must exit 0; stdout={}", stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "merged");
    assert_eq!(json["pr_number"], 42);
}

/// Stubbed `check-freshness` returns `max_retries` →
/// `complete_merge_inner` returns `status == "max_retries"` →
/// `run` exits 1 (status != "merged").
#[test]
fn merge_run_non_merged_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_bin_flow_stub(&flow_bin);
    let path_stub = build_path_stub_dir(&parent);
    let state_path = parent.join("state.json");
    fs::write(&state_path, "{\"branch\": \"feat\"}").unwrap();

    let (code, stdout, _) = run_complete_merge(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &path_stub,
        &flow_bin,
        r#"{"status": "max_retries", "retries": 3}"#,
        0,
    );

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "max_retries");
}

/// Missing state file + stubbed `check-freshness` returning an error
/// → `complete_merge_inner` surfaces the freshness error through
/// the result → `run` exits 1. Proves the `complete_merge` wrapper
/// delegates to `complete_merge_inner` end-to-end.
#[test]
fn merge_wrapper_returns_error_on_missing_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_bin_flow_stub(&flow_bin);
    let path_stub = build_path_stub_dir(&parent);
    let missing_state = parent.join("does-not-exist.json");

    let (code, stdout, _) = run_complete_merge(
        &parent,
        "42",
        missing_state.to_string_lossy().as_ref(),
        &path_stub,
        &flow_bin,
        r#"{"status": "error", "message": "state file missing"}"#,
        0,
    );

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(
        json["message"]
            .as_str()
            .unwrap_or("")
            .contains("state file missing"),
        "wrapper must surface freshness error via the result; got: {}",
        json
    );
}

// --- library-level tests (complete_merge_inner with mock runner) ---

fn mock_runner(responses: Vec<CmdResult>) -> impl Fn(&[&str], u64) -> CmdResult {
    let queue = RefCell::new(VecDeque::from(responses));
    move |_args: &[&str], _timeout: u64| -> CmdResult {
        queue
            .borrow_mut()
            .pop_front()
            .expect("mock_runner: no more responses")
    }
}

fn ok(stdout: &str) -> CmdResult {
    Ok((0, stdout.to_string(), String::new()))
}

fn ok_empty() -> CmdResult {
    Ok((0, String::new(), String::new()))
}

fn fail_with_stdout_stderr(stdout: &str, stderr: &str) -> CmdResult {
    Ok((1, stdout.to_string(), stderr.to_string()))
}

fn err(msg: &str) -> CmdResult {
    Err(msg.to_string())
}

fn write_state(path: &Path) {
    let state = json!({
        "schema_version": 1,
        "branch": "test-feature",
        "pr_number": 42,
        "complete_step": 4,
        "phases": {}
    });
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
}

#[test]
fn up_to_date_and_merge_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    write_state(&state_path);

    let runner = mock_runner(vec![ok(r#"{"status": "up_to_date"}"#), ok("merged")]);

    let result = complete_merge_inner(42, state_path.to_str().unwrap(), "/fake/bin/flow", &runner);

    assert_eq!(result["status"], "merged");
    assert_eq!(result["pr_number"], 42);
}

#[test]
fn main_moved_ci_rerun() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    write_state(&state_path);

    let runner = mock_runner(vec![
        ok(r#"{"status": "merged"}"#),
        ok_empty(), // git push
    ]);

    let result = complete_merge_inner(42, state_path.to_str().unwrap(), "/fake/bin/flow", &runner);

    assert_eq!(result["status"], "ci_rerun");
    assert_eq!(result["pushed"], true);
    assert_eq!(result["pr_number"], 42);
}

#[test]
fn merge_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    write_state(&state_path);

    let runner = mock_runner(vec![fail_with_stdout_stderr(
        r#"{"status": "conflict", "files": ["lib/foo.py", "lib/bar.py"]}"#,
        "",
    )]);

    let result = complete_merge_inner(42, state_path.to_str().unwrap(), "/fake/bin/flow", &runner);

    assert_eq!(result["status"], "conflict");
    let files: Vec<String> = result["conflict_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(files, vec!["lib/foo.py", "lib/bar.py"]);
    assert_eq!(result["pr_number"], 42);
}

#[test]
fn max_retries() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    write_state(&state_path);

    let runner = mock_runner(vec![fail_with_stdout_stderr(
        r#"{"status": "max_retries", "retries": 3}"#,
        "",
    )]);

    let result = complete_merge_inner(42, state_path.to_str().unwrap(), "/fake/bin/flow", &runner);

    assert_eq!(result["status"], "max_retries");
    assert_eq!(result["pr_number"], 42);
}

#[test]
fn branch_protection_ci_pending() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    write_state(&state_path);

    let runner = mock_runner(vec![
        ok(r#"{"status": "up_to_date"}"#),
        fail_with_stdout_stderr("", "base branch policy prohibits the merge"),
    ]);

    let result = complete_merge_inner(42, state_path.to_str().unwrap(), "/fake/bin/flow", &runner);

    assert_eq!(result["status"], "ci_pending");
    assert_eq!(result["pr_number"], 42);
}

#[test]
fn merge_fails_other_reason() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    write_state(&state_path);

    let runner = mock_runner(vec![
        ok(r#"{"status": "up_to_date"}"#),
        fail_with_stdout_stderr("", "unknown merge error"),
    ]);

    let result = complete_merge_inner(42, state_path.to_str().unwrap(), "/fake/bin/flow", &runner);

    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("unknown merge error"));
}

#[test]
fn check_freshness_error() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    write_state(&state_path);

    let runner = mock_runner(vec![fail_with_stdout_stderr(
        r#"{"status": "error", "step": "fetch", "message": "network error"}"#,
        "",
    )]);

    let result = complete_merge_inner(42, state_path.to_str().unwrap(), "/fake/bin/flow", &runner);

    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("network error"));
}

#[test]
fn step_counter_set() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    write_state(&state_path);

    let runner = mock_runner(vec![ok(r#"{"status": "up_to_date"}"#), ok("merged")]);

    complete_merge_inner(42, state_path.to_str().unwrap(), "/fake/bin/flow", &runner);

    let state_content = fs::read_to_string(&state_path).unwrap();
    let state: Value = serde_json::from_str(&state_content).unwrap();
    assert_eq!(state["complete_step"], json!(5));
}

#[test]
fn push_failure_after_freshness_merge() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    write_state(&state_path);

    let runner = mock_runner(vec![
        ok(r#"{"status": "merged"}"#),
        fail_with_stdout_stderr("", "remote rejected"),
    ]);

    let result = complete_merge_inner(42, state_path.to_str().unwrap(), "/fake/bin/flow", &runner);

    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("push"));
}

#[test]
fn check_freshness_invalid_json() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    write_state(&state_path);

    let runner = mock_runner(vec![fail_with_stdout_stderr("not json at all", "")]);

    let result = complete_merge_inner(42, state_path.to_str().unwrap(), "/fake/bin/flow", &runner);

    assert_eq!(result["status"], "error");
}

#[test]
fn timeout_handling() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    write_state(&state_path);

    let runner = mock_runner(vec![err("Timed out after 60s")]);

    let result = complete_merge_inner(42, state_path.to_str().unwrap(), "/fake/bin/flow", &runner);

    assert_eq!(result["status"], "error");
}

#[test]
fn unknown_freshness_status() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    write_state(&state_path);

    let runner = mock_runner(vec![ok(r#"{"status": "unexpected_value"}"#)]);

    let result = complete_merge_inner(42, state_path.to_str().unwrap(), "/fake/bin/flow", &runner);

    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("unexpected"));
}

#[test]
fn missing_state_file_skips_step_counter() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("nonexistent.json");

    let runner = mock_runner(vec![ok(r#"{"status": "up_to_date"}"#), ok("merged")]);

    let result = complete_merge_inner(42, state_path.to_str().unwrap(), "/fake/bin/flow", &runner);

    // Still succeeds — step counter is best-effort
    assert_eq!(result["status"], "merged");
}

#[test]
fn object_guard_non_object_state() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    // Write an array instead of an object — mutate_state closure must not panic
    fs::write(&state_path, "[1, 2, 3]").unwrap();

    let runner = mock_runner(vec![ok(r#"{"status": "up_to_date"}"#), ok("merged")]);

    let result = complete_merge_inner(42, state_path.to_str().unwrap(), "/fake/bin/flow", &runner);

    // Should not panic; returns merged
    assert_eq!(result["status"], "merged");
}

#[test]
fn conflict_with_missing_files_defaults_to_empty() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    write_state(&state_path);

    // conflict payload without "files" key
    let runner = mock_runner(vec![fail_with_stdout_stderr(
        r#"{"status": "conflict"}"#,
        "",
    )]);

    let result = complete_merge_inner(42, state_path.to_str().unwrap(), "/fake/bin/flow", &runner);

    assert_eq!(result["status"], "conflict");
    assert_eq!(result["conflict_files"], json!([]));
}

#[test]
fn check_freshness_runner_transport_error() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    write_state(&state_path);

    let runner = mock_runner(vec![err("spawn failed: no such binary")]);

    let result = complete_merge_inner(42, state_path.to_str().unwrap(), "/fake/bin/flow", &runner);

    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("spawn failed"));
}

#[test]
fn squash_merge_transport_error_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    write_state(&state_path);

    let runner = mock_runner(vec![
        ok(r#"{"status": "up_to_date"}"#),
        err("Timed out after 60s"),
    ]);

    let result = complete_merge_inner(42, state_path.to_str().unwrap(), "/fake/bin/flow", &runner);

    assert_eq!(result["status"], "error");
}

#[test]
fn push_transport_error_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    write_state(&state_path);

    let runner = mock_runner(vec![
        ok(r#"{"status": "merged"}"#),
        err("Timed out after 60s"),
    ]);

    let result = complete_merge_inner(42, state_path.to_str().unwrap(), "/fake/bin/flow", &runner);

    assert_eq!(result["status"], "error");
}

// --- run_impl_main_with_runner (exit code → status map) ---

#[test]
fn run_impl_main_with_runner_merged_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    write_state(&state_path);

    let runner = mock_runner(vec![ok(r#"{"status": "up_to_date"}"#), ok("merged")]);

    let args = Args {
        pr: 42,
        state_file: state_path.to_string_lossy().to_string(),
    };
    let (value, code) = run_impl_main_with_runner(&args, "/fake/bin/flow", &runner);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "merged");
}

#[test]
fn run_impl_main_with_runner_non_merged_exits_one() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    write_state(&state_path);

    let runner = mock_runner(vec![fail_with_stdout_stderr(
        r#"{"status": "max_retries"}"#,
        "",
    )]);

    let args = Args {
        pr: 42,
        state_file: state_path.to_string_lossy().to_string(),
    };
    let (value, code) = run_impl_main_with_runner(&args, "/fake/bin/flow", &runner);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "max_retries");
}
