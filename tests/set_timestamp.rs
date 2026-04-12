//! Integration tests for `flow-rs set-timestamp` command.

mod common;

use std::fs;
use std::process::Command;

use common::flow_states_dir;
use regex::Regex;
use serde_json::{json, Value};

fn flow_rs() -> Command {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
}

fn iso_pattern() -> Regex {
    Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[Z+-]").unwrap()
}

fn make_state() -> Value {
    json!({
        "schema_version": 1,
        "branch": "test-feature",
        "current_phase": "flow-code",
        "started_at": "2026-01-01T00:00:00-08:00",
        "files": {
            "plan": null,
            "dag": null,
            "log": ".flow-states/test-feature.log",
            "state": ".flow-states/test-feature.json"
        },
        "phases": {
            "flow-start": {"name": "Start", "status": "complete", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0},
            "flow-plan": {"name": "Plan", "status": "complete", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0},
            "flow-code": {"name": "Code", "status": "in_progress", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0}
        }
    })
}

fn setup_state(dir: &std::path::Path, branch: &str, state: &Value) -> std::path::PathBuf {
    let state_dir = flow_states_dir(dir);
    fs::create_dir_all(&state_dir).unwrap();
    let path = state_dir.join(format!("{}.json", branch));
    fs::write(&path, serde_json::to_string_pretty(state).unwrap()).unwrap();
    path
}

fn run_set_timestamp(dir: &std::path::Path, args: &[&str]) -> (i32, Value) {
    let mut cmd = flow_rs();
    cmd.arg("set-timestamp");
    for arg in args {
        cmd.arg(arg);
    }
    cmd.env("FLOW_SIMULATE_BRANCH", "test-feature");
    cmd.current_dir(dir);

    // Init a git repo so project_root works
    let _ = Command::new("git").args(["init"]).current_dir(dir).output();

    let output = cmd.output().unwrap();
    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let parsed: Value = if stdout.is_empty() {
        json!(null)
    } else {
        serde_json::from_str(&stdout).unwrap_or(json!({"raw": stdout}))
    };
    (exit_code, parsed)
}

#[test]
fn test_cli_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_state();
    state["design"] = json!({"status": "pending"});
    setup_state(dir.path(), "test-feature", &state);

    let (code, output) = run_set_timestamp(dir.path(), &["--set", "design.status=approved"]);
    assert_eq!(code, 0);
    assert_eq!(output["status"], "ok");
    assert_eq!(output["updates"][0]["value"], "approved");

    // Verify file was updated
    let content =
        fs::read_to_string(flow_states_dir(dir.path()).join("test-feature.json")).unwrap();
    let on_disk: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(on_disk["design"]["status"], "approved");
}

#[test]
fn test_cli_now_magic_value() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_state();
    state["design"] = json!({"approved_at": null});
    setup_state(dir.path(), "test-feature", &state);

    let (code, output) = run_set_timestamp(dir.path(), &["--set", "design.approved_at=NOW"]);
    assert_eq!(code, 0);
    assert_eq!(output["status"], "ok");
    assert!(iso_pattern().is_match(output["updates"][0]["value"].as_str().unwrap()));
}

#[test]
fn test_cli_multiple_set_args() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_state();
    state["plan"] = json!({"tasks": [{"id": 1, "status": "pending", "started_at": null}]});
    setup_state(dir.path(), "test-feature", &state);

    let (code, output) = run_set_timestamp(
        dir.path(),
        &[
            "--set",
            "plan.tasks.0.status=in_progress",
            "--set",
            "plan.tasks.0.started_at=NOW",
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(output["updates"].as_array().unwrap().len(), 2);
}

#[test]
fn test_cli_branch_flag() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_state();
    state["design"] = json!({"status": "pending"});
    setup_state(dir.path(), "other-feature", &state);

    let mut cmd = flow_rs();
    cmd.arg("set-timestamp")
        .arg("--set")
        .arg("design.status=approved")
        .arg("--branch")
        .arg("other-feature")
        .current_dir(dir.path());

    let _ = Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output();

    let output = cmd.output().unwrap();
    assert!(output.status.success());
    let parsed: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).unwrap();
    assert_eq!(parsed["status"], "ok");
    assert_eq!(parsed["updates"][0]["value"], "approved");
}

#[test]
fn test_cli_integer_coercion() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_state();
    state["code_review_step"] = json!(0);
    setup_state(dir.path(), "test-feature", &state);

    let (code, output) = run_set_timestamp(dir.path(), &["--set", "code_review_step=1"]);
    assert_eq!(code, 0);
    assert_eq!(output["updates"][0]["value"], 1);

    let content =
        fs::read_to_string(flow_states_dir(dir.path()).join("test-feature.json")).unwrap();
    let on_disk: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(on_disk["code_review_step"], 1);
    assert!(on_disk["code_review_step"].is_i64());
}

#[test]
fn test_cli_negative_integer_coercion() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_state();
    state["offset"] = json!(0);
    setup_state(dir.path(), "test-feature", &state);

    let (code, output) = run_set_timestamp(dir.path(), &["--set", "offset=-5"]);
    assert_eq!(code, 0);
    assert_eq!(output["updates"][0]["value"], -5);
}

#[test]
fn test_cli_non_digit_values_remain_strings() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_state();
    state["some_field"] = json!("old");
    setup_state(dir.path(), "test-feature", &state);

    let (code, output) = run_set_timestamp(dir.path(), &["--set", "some_field=in_progress"]);
    assert_eq!(code, 0);
    assert_eq!(output["updates"][0]["value"], "in_progress");
}

#[test]
fn test_cli_code_task_increment_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_state();
    state["code_task"] = json!(0);
    setup_state(dir.path(), "test-feature", &state);

    let (code, output) = run_set_timestamp(dir.path(), &["--set", "code_task=1"]);
    assert_eq!(code, 0);
    assert_eq!(output["updates"][0]["value"], 1);
}

#[test]
fn test_cli_code_task_jump_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_state();
    state["code_task"] = json!(0);
    setup_state(dir.path(), "test-feature", &state);

    let (code, output) = run_set_timestamp(dir.path(), &["--set", "code_task=5"]);
    assert_eq!(code, 1);
    assert_eq!(output["status"], "error");
    assert!(output["message"]
        .as_str()
        .unwrap()
        .contains("increment by 1"));
}

#[test]
fn test_cli_code_task_reset_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_state();
    state["code_task"] = json!(3);
    setup_state(dir.path(), "test-feature", &state);

    let (code, output) = run_set_timestamp(dir.path(), &["--set", "code_task=0"]);
    assert_eq!(code, 0);
    assert_eq!(output["updates"][0]["value"], 0);
}

#[test]
fn test_cli_error_no_state_file() {
    let dir = tempfile::tempdir().unwrap();

    let _ = Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output();

    // Create .flow-states dir but no state file
    fs::create_dir_all(flow_states_dir(dir.path())).unwrap();

    let mut cmd = flow_rs();
    cmd.arg("set-timestamp")
        .arg("--set")
        .arg("design.approved_at=NOW")
        .env("FLOW_SIMULATE_BRANCH", "test-feature")
        .current_dir(dir.path());

    let output = cmd.output().unwrap();
    assert_eq!(output.status.code().unwrap(), 1);
    let parsed: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).unwrap();
    assert_eq!(parsed["status"], "error");
    assert!(parsed["message"]
        .as_str()
        .unwrap()
        .contains("No state file"));
}

#[test]
fn test_cli_error_invalid_path() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_state();
    setup_state(dir.path(), "test-feature", &state);

    let (code, output) = run_set_timestamp(dir.path(), &["--set", "nonexistent.field=NOW"]);
    assert_eq!(code, 1);
    assert_eq!(output["status"], "error");
    assert!(output["message"].as_str().unwrap().contains("not found"));
}

#[test]
fn test_cli_error_array_out_of_range() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_state();
    state["plan"] = json!({"tasks": [{"id": 1, "status": "pending"}]});
    setup_state(dir.path(), "test-feature", &state);

    let (code, output) =
        run_set_timestamp(dir.path(), &["--set", "plan.tasks.5.status=in_progress"]);
    assert_eq!(code, 1);
    assert_eq!(output["status"], "error");
    assert!(output["message"].as_str().unwrap().contains("out of range"));
}

#[test]
fn test_cli_error_invalid_format() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_state();
    setup_state(dir.path(), "test-feature", &state);

    let (code, output) = run_set_timestamp(dir.path(), &["--set", "design.approved_at"]);
    assert_eq!(code, 1);
    assert_eq!(output["status"], "error");
    assert!(output["message"]
        .as_str()
        .unwrap()
        .contains("Invalid format"));
}

#[test]
fn test_cli_error_corrupt_json() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = flow_states_dir(dir.path());
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("test-feature.json"), "{bad json").unwrap();

    let (code, output) = run_set_timestamp(dir.path(), &["--set", "design.approved_at=NOW"]);
    assert_eq!(code, 1);
    assert_eq!(output["status"], "error");
    assert!(output["message"]
        .as_str()
        .unwrap()
        .contains("Could not read"));
}
