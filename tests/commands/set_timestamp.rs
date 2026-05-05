//! Tests for `src/commands/set_timestamp.rs`.
//!
//! Exercises the public helpers (`set_nested`, `apply_updates`,
//! `validate_code_task`, `run_impl_main`) via library calls and the
//! full CLI dispatch via `flow-rs set-timestamp` subprocess spawns.
//! Both layers are load-bearing: library tests guard the helpers in
//! isolation; subprocess tests guard the `main.rs` `SetTimestamp`
//! arm wiring plus the end-to-end dispatch chain.

#[path = "../common/mod.rs"]
mod common;

use std::fs;
use std::process::Command;

use common::flow_states_dir;
use regex::Regex;
use serde_json::{json, Value};

use flow_rs::commands::set_timestamp::{
    apply_updates, is_step_counter_field, run_impl_main, set_nested, validate_code_task,
};

fn iso_pattern() -> Regex {
    Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[Z+-]").unwrap()
}

fn flow_rs() -> Command {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
}

// --- is_step_counter_field ---

/// Each of the five named step counters returns `true` from the
/// helper. Locks the closed enumeration that Task 11 reads when
/// deciding whether to capture a `StepSnapshot` after a
/// `set-timestamp` mutation.
#[test]
fn is_step_counter_field_returns_true_for_each_named_field() {
    assert!(is_step_counter_field("plan_step"));
    assert!(is_step_counter_field("code_task"));
    assert!(is_step_counter_field("code_review_step"));
    assert!(is_step_counter_field("learn_step"));
    assert!(is_step_counter_field("complete_step"));
}

/// Non-step-counter fields return `false`, including dotted /
/// nested paths and other state fields written via set-timestamp.
#[test]
fn is_step_counter_field_returns_false_for_non_step_fields() {
    assert!(!is_step_counter_field(""));
    assert!(!is_step_counter_field("code_task_name"));
    assert!(!is_step_counter_field("files.plan"));
    assert!(!is_step_counter_field("_continue_pending"));
    assert!(!is_step_counter_field("Plan_Step"));
    assert!(!is_step_counter_field("plan_steps_total"));
}

// --- set_nested unit tests ---

#[test]
fn test_set_nested_simple_dict_key() {
    let mut obj = json!({"design": {"status": "pending"}});
    set_nested(&mut obj, &["design", "status"], json!("approved")).unwrap();
    assert_eq!(obj["design"]["status"], "approved");
}

#[test]
fn test_set_nested_nested_path() {
    let mut obj = json!({"a": {"b": {"c": 1}}});
    set_nested(&mut obj, &["a", "b", "c"], json!(99)).unwrap();
    assert_eq!(obj["a"]["b"]["c"], 99);
}

#[test]
fn test_set_nested_list_index() {
    let mut obj = json!({"items": [10, 20, 30]});
    set_nested(&mut obj, &["items", "1"], json!(99)).unwrap();
    assert_eq!(obj["items"][1], 99);
}

#[test]
fn test_set_nested_list_non_numeric_intermediate() {
    let mut obj = json!({"items": [{"a": 1}]});
    let result = set_nested(&mut obj, &["items", "abc", "a"], json!("val"));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Expected numeric index"));
}

#[test]
fn test_set_nested_non_traversable_intermediate() {
    let mut obj = json!({"outer": {"name": "hello"}});
    let result = set_nested(&mut obj, &["outer", "name", "deep", "sub"], json!("val"));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Cannot navigate into"));
}

#[test]
fn test_set_nested_list_final_non_numeric() {
    let mut obj = json!({"items": [1, 2, 3]});
    let result = set_nested(&mut obj, &["items", "abc"], json!("val"));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Expected numeric index"));
}

#[test]
fn test_set_nested_list_final_out_of_range() {
    let mut obj = json!({"items": [1, 2, 3]});
    let result = set_nested(&mut obj, &["items", "99"], json!("val"));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("out of range"));
}

#[test]
fn test_set_nested_non_settable_final() {
    let mut obj = json!({"items": [1, 2]});
    let result = set_nested(&mut obj, &["items", "0", "sub"], json!("val"));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Cannot set key"));
}

#[test]
fn test_set_nested_list_intermediate_out_of_range() {
    let mut obj = json!({"items": [{"a": 1}]});
    let result = set_nested(&mut obj, &["items", "99", "a"], json!("val"));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("out of range"));
}

#[test]
fn test_set_nested_dict_key_not_found() {
    let mut obj = json!({"a": {"b": 1}});
    let result = set_nested(&mut obj, &["a", "missing", "x"], json!("val"));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not found"));
}

#[test]
fn test_set_nested_creates_new_dict_key() {
    let mut obj = json!({"a": {}});
    set_nested(&mut obj, &["a", "new_key"], json!("new_value")).unwrap();
    assert_eq!(obj["a"]["new_key"], "new_value");
}

#[test]
fn test_set_nested_array_in_nested_path() {
    let mut obj = json!({"plan": {"tasks": [
        {"id": 1, "status": "pending", "started_at": null},
        {"id": 2, "status": "pending", "started_at": null}
    ]}});
    set_nested(
        &mut obj,
        &["plan", "tasks", "0", "status"],
        json!("in_progress"),
    )
    .unwrap();
    assert_eq!(obj["plan"]["tasks"][0]["status"], "in_progress");
    assert_eq!(obj["plan"]["tasks"][1]["status"], "pending");
}

// --- apply_updates tests ---

#[test]
fn test_apply_updates_simple_string() {
    let mut state = json!({"design": {"status": "pending"}});
    let updates = apply_updates(&mut state, &["design.status=approved".to_string()]).unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(state["design"]["status"], "approved");
}

#[test]
fn test_apply_updates_now_magic_value() {
    let mut state = json!({"design": {"approved_at": null}});
    let updates = apply_updates(&mut state, &["design.approved_at=NOW".to_string()]).unwrap();
    assert_eq!(updates.len(), 1);
    assert!(iso_pattern().is_match(updates[0].value.as_str().unwrap()));
    assert!(iso_pattern().is_match(state["design"]["approved_at"].as_str().unwrap()));
}

#[test]
fn test_apply_updates_integer_coercion() {
    let mut state = json!({"code_review_step": 0});
    let updates = apply_updates(&mut state, &["code_review_step=1".to_string()]).unwrap();
    assert_eq!(state["code_review_step"], 1);
    assert!(state["code_review_step"].is_i64());
    assert_eq!(updates[0].value, json!(1));
}

#[test]
fn test_apply_updates_negative_integer() {
    let mut state = json!({"offset": 0});
    let updates = apply_updates(&mut state, &["offset=-5".to_string()]).unwrap();
    assert_eq!(state["offset"], -5);
    assert!(state["offset"].is_i64());
    assert_eq!(updates[0].value, json!(-5));
}

#[test]
fn test_apply_updates_non_digit_stays_string() {
    let mut state = json!({"some_field": "old"});
    let updates = apply_updates(&mut state, &["some_field=in_progress".to_string()]).unwrap();
    assert_eq!(state["some_field"], "in_progress");
    assert!(state["some_field"].is_string());
    assert_eq!(updates[0].value, json!("in_progress"));
}

#[test]
fn test_apply_updates_multiple_args() {
    let mut state = json!({"plan": {"tasks": [
        {"id": 1, "status": "pending", "started_at": null}
    ]}});
    let updates = apply_updates(
        &mut state,
        &[
            "plan.tasks.0.status=in_progress".to_string(),
            "plan.tasks.0.started_at=NOW".to_string(),
        ],
    )
    .unwrap();
    assert_eq!(updates.len(), 2);
    assert_eq!(state["plan"]["tasks"][0]["status"], "in_progress");
    assert!(iso_pattern().is_match(state["plan"]["tasks"][0]["started_at"].as_str().unwrap()));
}

#[test]
fn test_apply_updates_invalid_format() {
    let mut state = json!({"a": 1});
    let result = apply_updates(&mut state, &["design.approved_at".to_string()]);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Invalid format"));
}

// --- validate_code_task tests ---

#[test]
fn test_code_task_increment_by_one() {
    let state = json!({"code_task": 0});
    assert!(validate_code_task(&state, 1).is_ok());
    let state = json!({"code_task": 1});
    assert!(validate_code_task(&state, 2).is_ok());
}

#[test]
fn test_code_task_initial_set_to_one() {
    let state = json!({"branch": "test"}); // no code_task key
    assert!(validate_code_task(&state, 1).is_ok());
}

#[test]
fn test_code_task_jump_blocked() {
    let state = json!({"code_task": 0});
    let result = validate_code_task(&state, 5);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("increment by 1"));
}

#[test]
fn test_code_task_skip_blocked() {
    let state = json!({"code_task": 3});
    let result = validate_code_task(&state, 5);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("increment by 1"));
}

#[test]
fn test_code_task_reset_to_zero() {
    let state = json!({"code_task": 3});
    assert!(validate_code_task(&state, 0).is_ok());
}

#[test]
fn test_code_task_non_integer_blocked() {
    let mut state = json!({"code_task": 0});
    let result = apply_updates(&mut state, &["code_task=abc".to_string()]);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("must be an integer"));
}

#[test]
fn test_code_task_cli_increment_blocked() {
    let mut state = json!({"code_task": 0});
    let result = apply_updates(&mut state, &["code_task=5".to_string()]);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("increment by 1"));
}

#[test]
fn test_code_task_error_message_mentions_batch_set() {
    let state = json!({"code_task": 0});
    let result = validate_code_task(&state, 5);
    assert!(result.is_err());
    let msg = result.unwrap_err();
    assert!(
        msg.contains("--set code_task="),
        "Error message should mention batch --set pattern, got: {}",
        msg
    );
}

#[test]
fn test_code_task_batch_increment_in_single_call() {
    let mut state = json!({"code_task": 0});
    let updates = apply_updates(
        &mut state,
        &[
            "code_task=1".to_string(),
            "code_task=2".to_string(),
            "code_task=3".to_string(),
        ],
    )
    .unwrap();
    assert_eq!(updates.len(), 3);
    assert_eq!(state["code_task"], 3);
}

// --- set_nested per-variant error arms ---

#[test]
fn set_nested_empty_path_errors() {
    let mut obj = json!({});
    let result = set_nested(&mut obj, &[], json!("v"));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Empty path"));
}

#[test]
fn set_nested_null_intermediate_errors() {
    let mut obj = json!({"a": null});
    let result = set_nested(&mut obj, &["a", "x", "y"], json!("v"));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("NoneType"));
}

#[test]
fn set_nested_bool_intermediate_errors() {
    let mut obj = json!({"a": true});
    let result = set_nested(&mut obj, &["a", "x", "y"], json!("v"));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("bool"));
}

#[test]
fn set_nested_number_intermediate_errors() {
    let mut obj = json!({"a": 42});
    let result = set_nested(&mut obj, &["a", "x", "y"], json!("v"));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("int"));
}

#[test]
fn set_nested_null_final_errors() {
    let mut obj = json!({"a": null});
    let result = set_nested(&mut obj, &["a", "x"], json!("v"));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("NoneType"));
}

#[test]
fn set_nested_bool_final_errors() {
    let mut obj = json!({"a": true});
    let result = set_nested(&mut obj, &["a", "x"], json!("v"));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("bool"));
}

#[test]
fn set_nested_string_final_errors() {
    let mut obj = json!({"a": "hello"});
    let result = set_nested(&mut obj, &["a", "x"], json!("v"));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("str"));
}

// --- run_impl_main library-level tests ---

/// No state file at the branch-scoped path — the valid branch is
/// provided via override, so resolve_branch succeeds, but the file
/// does not exist.
#[test]
fn run_impl_main_no_state_file_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (value, code) = run_impl_main(
        &["foo=bar".to_string()],
        Some("set-ts-no-state"),
        &root,
        &root,
    );
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("No state file found"));
}

/// Malformed `--set` arg (missing `=`) → apply_updates returns Err
/// inside the mutate_state closure. The closure restores the snapshot
/// before `mutate_state` persists, so the state file is unchanged.
#[test]
fn run_impl_main_invalid_set_arg_returns_error_and_preserves_state() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("set-ts-invalid");
    fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    let original = r#"{"branch":"set-ts-invalid","existing":"value"}"#;
    fs::write(&state_path, original).unwrap();

    let (value, code) = run_impl_main(
        &["no_equals_sign".to_string()],
        Some("set-ts-invalid"),
        &root,
        &root,
    );
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("expected path=value"));

    let after: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(after["existing"], "value");
}

/// Non-JSON failure in mutate_state (I/O error from attempting to
/// open a directory at the state-file path) → the error message is
/// returned as-is, not rewritten with the "Could not read state
/// file:" prefix. Covers the `else { msg }` branch.
#[test]
fn run_impl_main_non_json_mutate_error_returns_raw_message() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("set-ts-io");
    fs::create_dir_all(&branch_dir).unwrap();
    // Create a DIRECTORY where the state file is expected. `exists()`
    // returns true so run_impl_main proceeds to mutate_state, which
    // then fails with an I/O error from OpenOptions::open on a
    // directory — MutateError::Io(...), Display "I/O error: ...".
    // The caller's "Invalid JSON" / "JSON error" substring check
    // misses, so the message passes through as-is.
    fs::create_dir(branch_dir.join("state.json")).unwrap();

    let (value, code) = run_impl_main(&["foo=bar".to_string()], Some("set-ts-io"), &root, &root);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    let msg = value["message"].as_str().unwrap();
    // Confirm the raw error is passed through (not wrapped by the
    // "Could not read state file:" prefix).
    assert!(
        !msg.starts_with("Could not read state file:"),
        "I/O errors should pass through raw, got: {}",
        msg
    );
    assert!(
        msg.contains("I/O error"),
        "expected I/O error prefix, got: {}",
        msg
    );
}

/// Corrupt JSON in the state file → mutate_state returns Err which
/// the caller rewrites to "Could not read state file: <details>".
#[test]
fn run_impl_main_corrupt_state_file_returns_could_not_read_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("set-ts-corrupt");
    fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    fs::write(&state_path, "{not-json").unwrap();

    let (value, code) = run_impl_main(
        &["foo=bar".to_string()],
        Some("set-ts-corrupt"),
        &root,
        &root,
    );
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Could not read state file"));
}

/// Happy path: valid state file + valid update → success with the
/// applied update list.
#[test]
fn run_impl_main_success_returns_updates() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("set-ts-happy");
    fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    fs::write(
        &state_path,
        r#"{"branch":"set-ts-happy","design":{"status":"pending"}}"#,
    )
    .unwrap();

    let (value, code) = run_impl_main(
        &["design.status=approved".to_string()],
        Some("set-ts-happy"),
        &root,
        &root,
    );
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    let updates = value["updates"].as_array().unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0]["path"], "design.status");
    assert_eq!(updates[0]["value"], "approved");

    let after: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(after["design"]["status"], "approved");
}

/// cwd_scope::enforce Err branch: state file records `relative_cwd
/// = "sub"`, invocation from root (not root/sub) drifts and rejects.
#[test]
fn run_impl_main_cwd_drift_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();

    // Minimal git repo with one commit so current_branch_in returns
    // a populated branch name.
    for (args, label) in [
        (vec!["init", "-b", "main"], "init"),
        (vec!["config", "user.email", "t@t.com"], "email"),
        (vec!["config", "user.name", "T"], "name"),
        (vec!["config", "commit.gpgsign", "false"], "gpg"),
        (vec!["commit", "--allow-empty", "-m", "init"], "commit"),
    ] {
        let out = Command::new("git")
            .args(&args)
            .current_dir(&root)
            .output()
            .unwrap_or_else(|_| panic!("git {} failed to spawn", label));
        assert!(out.status.success(), "git {:?} failed: {:?}", args, out);
    }

    let branch_dir = root.join(".flow-states").join("main");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(
        branch_dir.join("state.json"),
        r#"{"branch":"main","relative_cwd":"sub"}"#,
    )
    .unwrap();

    let (value, code) = run_impl_main(&["foo=bar".to_string()], Some("main"), &root, &root);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    assert!(value["message"].as_str().unwrap().contains("cwd drift"));
}

// --- CLI subprocess tests ---
//
// These cover the `SetTimestamp` arm in `main.rs` end-to-end:
// argument parsing → run_impl_main → dispatch_json → exit code.
// They also exercise the `resolve_branch` None branch (a non-git
// cwd with no --branch override), which the library-level tests
// cannot reach because the test process cwd is the flow repo.

fn make_cli_state() -> Value {
    json!({
        "schema_version": 1,
        "branch": "test-feature",
        "current_phase": "flow-code",
        "started_at": "2026-01-01T00:00:00-08:00",
        "files": {
            "plan": null,
            "dag": null,
            "log": ".flow-states/test-feature/log",
            "state": ".flow-states/test-feature/state.json"
        },
        "phases": {
            "flow-start": {"name": "Start", "status": "complete", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0},
            "flow-plan": {"name": "Plan", "status": "complete", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0},
            "flow-code": {"name": "Code", "status": "in_progress", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0}
        }
    })
}

fn setup_cli_state(dir: &std::path::Path, branch: &str, state: &Value) -> std::path::PathBuf {
    let branch_dir = flow_states_dir(dir).join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    let path = branch_dir.join("state.json");
    fs::write(&path, serde_json::to_string_pretty(state).unwrap()).unwrap();
    path
}

fn run_cli(dir: &std::path::Path, args: &[&str]) -> (i32, Value) {
    let mut cmd = flow_rs();
    cmd.arg("set-timestamp");
    for arg in args {
        cmd.arg(arg);
    }
    cmd.env("FLOW_SIMULATE_BRANCH", "test-feature");
    cmd.current_dir(dir);

    // Init a git repo so project_root() returns the tempdir.
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
    let mut state = make_cli_state();
    state["design"] = json!({"status": "pending"});
    setup_cli_state(dir.path(), "test-feature", &state);

    let (code, output) = run_cli(dir.path(), &["--set", "design.status=approved"]);
    assert_eq!(code, 0);
    assert_eq!(output["status"], "ok");
    assert_eq!(output["updates"][0]["value"], "approved");

    let content = fs::read_to_string(
        flow_states_dir(dir.path())
            .join("test-feature")
            .join("state.json"),
    )
    .unwrap();
    let on_disk: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(on_disk["design"]["status"], "approved");
}

#[test]
fn test_cli_now_magic_value() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_cli_state();
    state["design"] = json!({"approved_at": null});
    setup_cli_state(dir.path(), "test-feature", &state);

    let (code, output) = run_cli(dir.path(), &["--set", "design.approved_at=NOW"]);
    assert_eq!(code, 0);
    assert_eq!(output["status"], "ok");
    assert!(iso_pattern().is_match(output["updates"][0]["value"].as_str().unwrap()));
}

#[test]
fn test_cli_multiple_set_args() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_cli_state();
    state["plan"] = json!({"tasks": [{"id": 1, "status": "pending", "started_at": null}]});
    setup_cli_state(dir.path(), "test-feature", &state);

    let (code, output) = run_cli(
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
    let mut state = make_cli_state();
    state["design"] = json!({"status": "pending"});
    setup_cli_state(dir.path(), "other-feature", &state);

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
    let mut state = make_cli_state();
    state["code_review_step"] = json!(0);
    setup_cli_state(dir.path(), "test-feature", &state);

    let (code, output) = run_cli(dir.path(), &["--set", "code_review_step=1"]);
    assert_eq!(code, 0);
    assert_eq!(output["updates"][0]["value"], 1);

    let content = fs::read_to_string(
        flow_states_dir(dir.path())
            .join("test-feature")
            .join("state.json"),
    )
    .unwrap();
    let on_disk: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(on_disk["code_review_step"], 1);
    assert!(on_disk["code_review_step"].is_i64());
}

#[test]
fn test_cli_negative_integer_coercion() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_cli_state();
    state["offset"] = json!(0);
    setup_cli_state(dir.path(), "test-feature", &state);

    let (code, output) = run_cli(dir.path(), &["--set", "offset=-5"]);
    assert_eq!(code, 0);
    assert_eq!(output["updates"][0]["value"], -5);
}

#[test]
fn test_cli_non_digit_values_remain_strings() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_cli_state();
    state["some_field"] = json!("old");
    setup_cli_state(dir.path(), "test-feature", &state);

    let (code, output) = run_cli(dir.path(), &["--set", "some_field=in_progress"]);
    assert_eq!(code, 0);
    assert_eq!(output["updates"][0]["value"], "in_progress");
}

#[test]
fn test_cli_code_task_increment_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_cli_state();
    state["code_task"] = json!(0);
    setup_cli_state(dir.path(), "test-feature", &state);

    let (code, output) = run_cli(dir.path(), &["--set", "code_task=1"]);
    assert_eq!(code, 0);
    assert_eq!(output["updates"][0]["value"], 1);
}

#[test]
fn test_cli_code_task_jump_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_cli_state();
    state["code_task"] = json!(0);
    setup_cli_state(dir.path(), "test-feature", &state);

    let (code, output) = run_cli(dir.path(), &["--set", "code_task=5"]);
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
    let mut state = make_cli_state();
    state["code_task"] = json!(3);
    setup_cli_state(dir.path(), "test-feature", &state);

    let (code, output) = run_cli(dir.path(), &["--set", "code_task=0"]);
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

    // Create .flow-states dir but no state file.
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
    let state = make_cli_state();
    setup_cli_state(dir.path(), "test-feature", &state);

    let (code, output) = run_cli(dir.path(), &["--set", "nonexistent.field=NOW"]);
    assert_eq!(code, 1);
    assert_eq!(output["status"], "error");
    assert!(output["message"].as_str().unwrap().contains("not found"));
}

#[test]
fn test_cli_error_array_out_of_range() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_cli_state();
    state["plan"] = json!({"tasks": [{"id": 1, "status": "pending"}]});
    setup_cli_state(dir.path(), "test-feature", &state);

    let (code, output) = run_cli(dir.path(), &["--set", "plan.tasks.5.status=in_progress"]);
    assert_eq!(code, 1);
    assert_eq!(output["status"], "error");
    assert!(output["message"].as_str().unwrap().contains("out of range"));
}

#[test]
fn test_cli_error_invalid_format() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_cli_state();
    setup_cli_state(dir.path(), "test-feature", &state);

    let (code, output) = run_cli(dir.path(), &["--set", "design.approved_at"]);
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
    let branch_dir = flow_states_dir(dir.path()).join("test-feature");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), "{bad json").unwrap();

    let (code, output) = run_cli(dir.path(), &["--set", "design.approved_at=NOW"]);
    assert_eq!(code, 1);
    assert_eq!(output["status"], "error");
    assert!(output["message"]
        .as_str()
        .unwrap()
        .contains("Could not read"));
}

/// `resolve_branch` None branch: subprocess cwd is a non-git tempdir,
/// no `--branch` override, and no `FLOW_SIMULATE_BRANCH` env var.
/// `current_branch()` inside the subprocess returns None so
/// `resolve_branch_impl` returns None and run_impl_main reports
/// "Could not determine current branch".
#[test]
fn test_cli_no_branch_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();

    let mut cmd = flow_rs();
    cmd.arg("set-timestamp")
        .arg("--set")
        .arg("foo=bar")
        .env_remove("FLOW_SIMULATE_BRANCH")
        .env_remove("FLOW_CI_RUNNING")
        .env("GH_TOKEN", "invalid")
        .env("HOME", &root)
        .current_dir(&root);

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let parsed: Value = serde_json::from_str(&stdout).unwrap_or(json!({"raw": stdout}));
    assert_eq!(output.status.code().unwrap_or(-1), 1);
    assert_eq!(parsed["status"], "error");
    assert!(parsed["message"]
        .as_str()
        .unwrap_or("")
        .contains("Could not determine current branch"));
}

/// `--branch ''` (empty string) — `FlowPaths::try_new` rejects, so
/// the subprocess returns a structured error rather than panicking.
/// Per `.claude/rules/external-input-validation.md` "CLI subcommand
/// entry callsite discipline".
#[test]
fn run_impl_main_with_empty_branch_returns_structured_error_no_panic() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir_all(root.join(".git")).expect("mkdir .git");

    let mut cmd = flow_rs();
    cmd.arg("set-timestamp")
        .arg("--branch")
        .arg("")
        .arg("--set")
        .arg("plan_step=1")
        .env_remove("FLOW_SIMULATE_BRANCH")
        .env_remove("FLOW_CI_RUNNING")
        .env("GH_TOKEN", "invalid")
        .env("HOME", &root)
        .current_dir(&root);

    let output = cmd.output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked at"),
        "must not panic on empty branch. stderr={:?}",
        stderr
    );
}

/// `--branch feature/foo` (slash-containing) — `FlowPaths::try_new`
/// rejects; same structured-error contract as the empty case.
#[test]
fn run_impl_main_with_slash_branch_returns_structured_error_no_panic() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir_all(root.join(".git")).expect("mkdir .git");

    let mut cmd = flow_rs();
    cmd.arg("set-timestamp")
        .arg("--branch")
        .arg("feature/foo")
        .arg("--set")
        .arg("plan_step=1")
        .env_remove("FLOW_SIMULATE_BRANCH")
        .env_remove("FLOW_CI_RUNNING")
        .env("GH_TOKEN", "invalid")
        .env("HOME", &root)
        .current_dir(&root);

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked at"),
        "must not panic on slash branch. stderr={:?}",
        stderr
    );
    let parsed: Value = serde_json::from_str(&stdout).unwrap_or(json!({"raw": stdout}));
    assert_eq!(output.status.code().unwrap_or(-1), 1);
    assert_eq!(parsed["status"], "error");
    assert!(parsed["message"]
        .as_str()
        .unwrap_or("")
        .contains("Invalid branch name"));
}
