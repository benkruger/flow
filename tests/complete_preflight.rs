//! Integration tests for `src/complete_preflight.rs`. Drives the public
//! surface (`resolve_mode`, `check_learn_phase`, `check_pr_status`,
//! `merge_main`, `preflight_inner`, `wait_with_timeout`,
//! `run_cmd_with_timeout`, `run_impl_main`) through mock runners for
//! unit coverage and the compiled binary for subprocess coverage of the
//! `preflight` production wrapper.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fs;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::rc::Rc;
use std::time::Duration;

use flow_rs::complete_preflight::{
    check_learn_phase, check_pr_status, merge_main, preflight_inner, resolve_mode,
    run_cmd_with_timeout, wait_with_timeout, CmdResult, WaitError,
};
use serde_json::{json, Value};

mod common;

const BRANCH: &str = "test-feature";
const PT_ENTER_OK: &str = r#"{"status": "ok", "phase": "flow-complete", "action": "enter", "visit_count": 1, "first_visit": true}"#;

// --- Helpers (ported from inline tests) ---

fn mock_runner(responses: Vec<CmdResult>) -> impl Fn(&[&str], u64) -> CmdResult {
    let queue = RefCell::new(VecDeque::from(responses));
    move |_args: &[&str], _timeout: u64| -> CmdResult {
        queue
            .borrow_mut()
            .pop_front()
            .expect("mock_runner: no more responses")
    }
}

fn tracking_runner(
    responses: Vec<CmdResult>,
    calls: Rc<RefCell<Vec<Vec<String>>>>,
) -> impl Fn(&[&str], u64) -> CmdResult {
    let queue = RefCell::new(VecDeque::from(responses));
    move |args: &[&str], _timeout: u64| -> CmdResult {
        calls
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        queue
            .borrow_mut()
            .pop_front()
            .expect("tracking_runner: no more responses")
    }
}

fn ok(stdout: &str) -> CmdResult {
    Ok((0, stdout.to_string(), String::new()))
}

fn ok_empty() -> CmdResult {
    Ok((0, String::new(), String::new()))
}

fn fail(stderr: &str) -> CmdResult {
    Ok((1, String::new(), stderr.to_string()))
}

fn err(msg: &str) -> CmdResult {
    Err(msg.to_string())
}

fn setup_state(root: &Path, branch: &str, learn_status: &str, skills: Option<Value>) {
    let state_dir = root.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let mut state = json!({
        "schema_version": 1,
        "branch": branch,
        "repo": "test/test",
        "pr_number": 42,
        "pr_url": "https://github.com/test/test/pull/42",
        "prompt": "work on issue #100",
        "phases": {
            "flow-start": {"status": "complete"},
            "flow-plan": {"status": "complete"},
            "flow-code": {"status": "complete"},
            "flow-code-review": {"status": "complete"},
            "flow-learn": {"status": learn_status},
            "flow-complete": {"status": "pending"}
        }
    });
    if let Some(s) = skills {
        state["skills"] = s;
    }
    fs::write(
        state_dir.join(format!("{}.json", branch)),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();
}

// --- resolve_mode ---

#[test]
fn resolve_mode_auto_flag_wins() {
    let state = json!({"skills": {"flow-complete": "manual"}});
    assert_eq!(resolve_mode(true, false, Some(&state)), "auto");
}

#[test]
fn resolve_mode_manual_flag_wins_over_state() {
    let state = json!({"skills": {"flow-complete": "auto"}});
    assert_eq!(resolve_mode(false, true, Some(&state)), "manual");
}

#[test]
fn resolve_mode_state_string() {
    let state = json!({"skills": {"flow-complete": "manual"}});
    assert_eq!(resolve_mode(false, false, Some(&state)), "manual");
}

#[test]
fn resolve_mode_state_dict_continue() {
    let state = json!({"skills": {"flow-complete": {"continue": "manual", "commit": "auto"}}});
    assert_eq!(resolve_mode(false, false, Some(&state)), "manual");
}

#[test]
fn resolve_mode_state_dict_no_continue_defaults_auto() {
    let state = json!({"skills": {"flow-complete": {"commit": "auto"}}});
    assert_eq!(resolve_mode(false, false, Some(&state)), "auto");
}

#[test]
fn resolve_mode_no_state_defaults_auto() {
    assert_eq!(resolve_mode(false, false, None), "auto");
}

#[test]
fn resolve_mode_state_without_skills_defaults_auto() {
    let state = json!({"branch": "test"});
    assert_eq!(resolve_mode(false, false, Some(&state)), "auto");
}

// --- check_learn_phase ---

#[test]
fn check_learn_phase_complete_no_warning() {
    let state = json!({"phases": {"flow-learn": {"status": "complete"}}});
    assert!(check_learn_phase(&state).is_empty());
}

#[test]
fn check_learn_phase_pending_emits_warning() {
    let state = json!({"phases": {"flow-learn": {"status": "pending"}}});
    let warnings = check_learn_phase(&state);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("Phase 5"));
    assert!(warnings[0].contains("pending"));
}

#[test]
fn check_learn_phase_missing_treated_as_pending() {
    let state = json!({"phases": {}});
    let warnings = check_learn_phase(&state);
    assert_eq!(warnings.len(), 1);
}

// --- check_pr_status ---

#[test]
fn check_pr_status_no_identifier_returns_error() {
    let runner = mock_runner(vec![]);
    let result = check_pr_status(None, "", &runner);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_lowercase().contains("no pr number"));
}

#[test]
fn check_pr_status_uses_pr_number_when_provided() {
    let runner = mock_runner(vec![ok("OPEN")]);
    let result = check_pr_status(Some(42), "some-branch", &runner);
    assert_eq!(result.unwrap(), "OPEN");
}

#[test]
fn check_pr_status_falls_back_to_branch() {
    let runner = mock_runner(vec![ok("MERGED")]);
    let result = check_pr_status(None, "feature-xyz", &runner);
    assert_eq!(result.unwrap(), "MERGED");
}

#[test]
fn check_pr_status_gh_failure_returns_error() {
    let runner = mock_runner(vec![fail("Could not resolve to a Pull Request")]);
    let result = check_pr_status(Some(42), "b", &runner);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Could not resolve"));
}

#[test]
fn check_pr_status_gh_failure_empty_stderr_returns_generic_error() {
    let runner = mock_runner(vec![fail("")]);
    let result = check_pr_status(Some(42), "b", &runner);
    assert_eq!(result.unwrap_err(), "Could not find PR");
}

// --- merge_main ---

#[test]
fn merge_main_already_up_to_date() {
    let runner = mock_runner(vec![ok_empty(), ok_empty()]);
    let (status, data) = merge_main(&runner);
    assert_eq!(status, "clean");
    assert!(data.is_none());
}

#[test]
fn merge_main_new_commits_merged_and_pushed() {
    let runner = mock_runner(vec![ok_empty(), fail(""), ok("Merge made"), ok_empty()]);
    let (status, data) = merge_main(&runner);
    assert_eq!(status, "merged");
    assert!(data.is_none());
}

#[test]
fn merge_main_conflicts_detected() {
    let runner = mock_runner(vec![
        ok_empty(),
        fail(""),
        fail("CONFLICT (content)"),
        ok("UU lib/foo.py\nAA lib/bar.py\n"),
    ]);
    let (status, data) = merge_main(&runner);
    assert_eq!(status, "conflict");
    let files: Vec<String> = data
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(files.contains(&"lib/foo.py".to_string()));
    assert!(files.contains(&"lib/bar.py".to_string()));
}

#[test]
fn merge_main_fetch_error() {
    let runner = mock_runner(vec![fail("Could not resolve host")]);
    let (status, data) = merge_main(&runner);
    assert_eq!(status, "error");
    assert!(data
        .unwrap()
        .as_str()
        .unwrap()
        .contains("Could not resolve"));
}

#[test]
fn merge_main_merge_error_non_conflict() {
    let runner = mock_runner(vec![ok_empty(), fail(""), fail("generic"), ok_empty()]);
    let (status, _data) = merge_main(&runner);
    assert_eq!(status, "error");
}

#[test]
fn merge_main_push_failure_after_merge() {
    let runner = mock_runner(vec![
        ok_empty(),
        fail(""),
        ok("Merge made"),
        fail("remote rejected"),
    ]);
    let (status, data) = merge_main(&runner);
    assert_eq!(status, "error");
    let msg = data.unwrap();
    let msg_str = msg.as_str().unwrap();
    assert!(msg_str.to_lowercase().contains("push failed"));
}

#[test]
fn merge_main_timeout_returns_error() {
    let runner = mock_runner(vec![err("Timed out after 60s")]);
    let (status, _data) = merge_main(&runner);
    assert_eq!(status, "error");
}

// --- preflight_inner: happy paths ---

#[test]
fn preflight_happy_path_open_pr_clean_merge() {
    let dir = tempfile::tempdir().unwrap();
    setup_state(dir.path(), "test-feature", "complete", None);

    let runner = mock_runner(vec![ok(PT_ENTER_OK), ok("OPEN"), ok_empty(), ok_empty()]);

    let result = preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "ok");
    assert_eq!(result["pr_state"], "OPEN");
    assert_eq!(result["merge"], "clean");
    assert_eq!(result["mode"], "auto");
    assert_eq!(result["warnings"].as_array().unwrap().len(), 0);
}

#[test]
fn preflight_pr_already_merged_returns_early() {
    let dir = tempfile::tempdir().unwrap();
    setup_state(dir.path(), "test-feature", "complete", None);

    let runner = mock_runner(vec![ok(PT_ENTER_OK), ok("MERGED")]);

    let result = preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "ok");
    assert_eq!(result["pr_state"], "MERGED");
    assert!(result.get("merge").is_none());
}

#[test]
fn preflight_pr_closed_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    setup_state(dir.path(), "test-feature", "complete", None);

    let runner = mock_runner(vec![ok(PT_ENTER_OK), ok("CLOSED")]);

    let result = preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("closed"));
}

#[test]
fn preflight_merge_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    setup_state(dir.path(), "test-feature", "complete", None);

    let runner = mock_runner(vec![
        ok(PT_ENTER_OK),
        ok("OPEN"),
        ok_empty(),
        fail(""),
        fail("CONFLICT (content)"),
        ok("UU lib/foo.py\nAA lib/bar.py\n"),
    ]);

    let result = preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "conflict");
    let files: Vec<String> = result["conflict_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(files.contains(&"lib/foo.py".to_string()));
    assert!(files.contains(&"lib/bar.py".to_string()));
}

#[test]
fn preflight_no_state_file_infers_from_git() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".flow-states")).unwrap();

    let runner = mock_runner(vec![ok("OPEN"), ok_empty(), ok_empty()]);

    let result = preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "ok");
    assert_eq!(result["inferred"], true);
}

// --- mode flags ---

#[test]
fn preflight_auto_flag_overrides_state() {
    let dir = tempfile::tempdir().unwrap();
    setup_state(
        dir.path(),
        "test-feature",
        "complete",
        Some(json!({"flow-complete": "manual"})),
    );

    let runner = mock_runner(vec![ok(PT_ENTER_OK), ok("OPEN"), ok_empty(), ok_empty()]);

    let result = preflight_inner(
        Some("test-feature"),
        true,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["mode"], "auto");
}

#[test]
fn preflight_manual_flag_overrides_state() {
    let dir = tempfile::tempdir().unwrap();
    setup_state(
        dir.path(),
        "test-feature",
        "complete",
        Some(json!({"flow-complete": "auto"})),
    );

    let runner = mock_runner(vec![ok(PT_ENTER_OK), ok("OPEN"), ok_empty(), ok_empty()]);

    let result = preflight_inner(
        Some("test-feature"),
        false,
        true,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["mode"], "manual");
}

#[test]
fn preflight_mode_from_state_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_state(
        dir.path(),
        "test-feature",
        "complete",
        Some(json!({"flow-complete": "manual"})),
    );

    let runner = mock_runner(vec![ok(PT_ENTER_OK), ok("OPEN"), ok_empty(), ok_empty()]);

    let result = preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["mode"], "manual");
}

// --- learn phase warning ---

#[test]
fn preflight_learn_pending_emits_warning() {
    let dir = tempfile::tempdir().unwrap();
    setup_state(dir.path(), "test-feature", "pending", None);

    let runner = mock_runner(vec![ok(PT_ENTER_OK), ok("OPEN"), ok_empty(), ok_empty()]);

    let result = preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "ok");
    let warnings = result["warnings"].as_array().unwrap();
    assert!(!warnings.is_empty());
    assert!(warnings[0]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("phase 5"));
}

// --- step counter persistence ---

#[test]
fn preflight_sets_complete_step_counters_in_state() {
    let dir = tempfile::tempdir().unwrap();
    setup_state(dir.path(), "test-feature", "complete", None);

    let runner = mock_runner(vec![ok(PT_ENTER_OK), ok("OPEN"), ok_empty(), ok_empty()]);

    preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    let state_content =
        fs::read_to_string(dir.path().join(".flow-states/test-feature.json")).unwrap();
    let state: Value = serde_json::from_str(&state_content).unwrap();
    assert_eq!(state["complete_steps_total"], json!(6));
    assert_eq!(state["complete_step"], json!(1));
}

// --- merged with new commits pushes ---

#[test]
fn preflight_merge_with_new_commits_pushes() {
    let dir = tempfile::tempdir().unwrap();
    setup_state(dir.path(), "test-feature", "complete", None);

    let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::new(RefCell::new(Vec::new()));
    let runner = tracking_runner(
        vec![
            ok(PT_ENTER_OK),
            ok("OPEN"),
            ok_empty(),
            fail(""),
            ok("Merge made"),
            ok_empty(),
        ],
        calls.clone(),
    );

    let result = preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "ok");
    assert_eq!(result["merge"], "merged");
    let push_calls: Vec<_> = calls
        .borrow()
        .iter()
        .filter(|c| c.iter().any(|a| a == "push"))
        .cloned()
        .collect();
    assert!(!push_calls.is_empty());
}

// --- phase transition invocation ---

#[test]
fn preflight_phase_transition_enter_called_with_correct_args() {
    let dir = tempfile::tempdir().unwrap();
    setup_state(dir.path(), "test-feature", "complete", None);

    let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::new(RefCell::new(Vec::new()));
    let runner = tracking_runner(
        vec![ok(PT_ENTER_OK), ok("OPEN"), ok_empty(), ok_empty()],
        calls.clone(),
    );

    preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    let pt_call = calls
        .borrow()
        .iter()
        .find(|c| c.iter().any(|a| a == "phase-transition"))
        .cloned()
        .expect("phase-transition call not found");
    assert!(pt_call.contains(&"--action".to_string()));
    assert!(pt_call.contains(&"enter".to_string()));
    assert!(pt_call.contains(&"--phase".to_string()));
    assert!(pt_call.contains(&"flow-complete".to_string()));
}

// --- error paths ---

#[test]
fn preflight_pr_view_failure_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    setup_state(dir.path(), "test-feature", "complete", None);

    let runner = mock_runner(vec![
        ok(PT_ENTER_OK),
        fail("Could not resolve to a Pull Request"),
    ]);

    let result = preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "error");
}

#[test]
fn preflight_phase_transition_error_returned() {
    let dir = tempfile::tempdir().unwrap();
    setup_state(dir.path(), "test-feature", "complete", None);

    let runner = mock_runner(vec![fail("state file not found")]);

    let result = preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("phase transition"));
}

#[test]
fn preflight_phase_transition_invalid_json() {
    let dir = tempfile::tempdir().unwrap();
    setup_state(dir.path(), "test-feature", "complete", None);

    let runner = mock_runner(vec![ok("not json")]);

    let result = preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "error");
}

#[test]
fn preflight_corrupt_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("test-feature.json"), "not json{{{").unwrap();

    let runner = mock_runner(vec![]);

    let result = preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("parse"));
}

#[test]
fn preflight_fetch_error_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    setup_state(dir.path(), "test-feature", "complete", None);

    let runner = mock_runner(vec![
        ok(PT_ENTER_OK),
        ok("OPEN"),
        fail("Could not resolve host"),
    ]);

    let result = preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "error");
}

#[test]
fn preflight_push_failure_after_merge() {
    let dir = tempfile::tempdir().unwrap();
    setup_state(dir.path(), "test-feature", "complete", None);

    let runner = mock_runner(vec![
        ok(PT_ENTER_OK),
        ok("OPEN"),
        ok_empty(),
        fail(""),
        ok("Merge made"),
        fail("remote rejected"),
    ]);

    let result = preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("push"));
}

#[test]
fn preflight_merge_error_non_conflict() {
    let dir = tempfile::tempdir().unwrap();
    setup_state(dir.path(), "test-feature", "complete", None);

    let runner = mock_runner(vec![
        ok(PT_ENTER_OK),
        ok("OPEN"),
        ok_empty(),
        fail(""),
        fail("merge failed"),
        ok_empty(),
    ]);

    let result = preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "error");
}

#[test]
fn preflight_unexpected_pr_state() {
    let dir = tempfile::tempdir().unwrap();
    setup_state(dir.path(), "test-feature", "complete", None);

    let runner = mock_runner(vec![ok(PT_ENTER_OK), ok("DRAFT")]);

    let result = preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("unexpected"));
}

#[test]
fn preflight_no_branch_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let runner = mock_runner(vec![]);
    let result = preflight_inner(None, false, false, dir.path(), "/fake/bin/flow", &runner);
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("branch"));
}

#[test]
fn preflight_empty_branch_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let runner = mock_runner(vec![]);
    let result = preflight_inner(
        Some(""),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );
    assert_eq!(result["status"], "error");
}

#[test]
fn preflight_slash_branch_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let runner = mock_runner(vec![]);
    let result = preflight_inner(
        Some("feature/foo"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("not a valid FLOW branch"));
}

#[test]
fn preflight_timeout_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    setup_state(dir.path(), "test-feature", "complete", None);

    let runner = mock_runner(vec![err("Timed out after 30s")]);

    let result = preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "error");
}

#[test]
fn preflight_result_includes_worktree_when_state_present() {
    let dir = tempfile::tempdir().unwrap();
    setup_state(dir.path(), "test-feature", "complete", None);

    let runner = mock_runner(vec![ok(PT_ENTER_OK), ok("OPEN"), ok_empty(), ok_empty()]);

    let result = preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert!(result.get("worktree").is_some());
    assert_eq!(result["pr_number"], 42);
    assert!(result["pr_url"]
        .as_str()
        .unwrap()
        .contains("github.com/test/test/pull/42"));
}

#[test]
fn preflight_inferred_result_omits_worktree() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".flow-states")).unwrap();

    let runner = mock_runner(vec![ok("OPEN"), ok_empty(), ok_empty()]);

    let result = preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "ok");
    assert_eq!(result["inferred"], true);
    assert!(result.get("worktree").is_none());
    assert!(result.get("pr_number").is_none());
}

// --- run_cmd_with_timeout kill path ---

#[test]
fn run_cmd_with_timeout_kills_on_expiry() {
    let start = std::time::Instant::now();
    let result = run_cmd_with_timeout(&["sleep", "60"], 1);
    let elapsed = start.elapsed();

    assert!(result.is_err(), "Expected Err for timed-out command");
    let msg = result.unwrap_err();
    assert!(
        msg.contains("Timed out"),
        "Error should mention timeout, got: {}",
        msg
    );
    assert!(
        elapsed.as_secs() < 5,
        "Should complete in <5s after kill, took {:?}",
        elapsed
    );
}

#[test]
fn run_cmd_with_timeout_empty_args_returns_err() {
    let result = run_cmd_with_timeout(&[], 30);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("empty command"));
}

#[test]
fn run_cmd_with_timeout_spawn_failure_returns_err() {
    let result = run_cmd_with_timeout(&["__definitely_not_a_real_binary__xyz"], 30);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Failed to spawn"));
}

#[test]
fn run_cmd_with_timeout_success_returns_output() {
    // `true` exits 0 with empty stdout/stderr — exercises the normal
    // Ok path through wait_with_timeout and stdio drain.
    let result = run_cmd_with_timeout(&["true"], 30);
    let (code, stdout, stderr) = result.expect("true must succeed");
    assert_eq!(code, 0);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
}

// --- wait_with_timeout ---

fn fake_exit_status(code: i32) -> ExitStatus {
    ExitStatus::from_raw(code << 8)
}

#[test]
fn wait_with_timeout_ready_immediately_returns_exit_status() {
    let sleep_calls = Cell::new(0u32);
    let result = wait_with_timeout(
        || Ok(Some(fake_exit_status(0))),
        |_| sleep_calls.set(sleep_calls.get() + 1),
        Duration::from_secs(60),
    );
    let status = result.expect("immediate Ok(Some) must return Ok(status)");
    assert_eq!(status.code(), Some(0));
    assert_eq!(
        sleep_calls.get(),
        0,
        "sleep_fn must not be invoked when try_wait is ready immediately"
    );
}

#[test]
fn wait_with_timeout_polls_then_exits() {
    let poll_count = Cell::new(0u32);
    let sleep_calls = Cell::new(0u32);
    let result = wait_with_timeout(
        || {
            let n = poll_count.get();
            poll_count.set(n + 1);
            if n == 0 {
                Ok(None)
            } else {
                Ok(Some(fake_exit_status(0)))
            }
        },
        |_| sleep_calls.set(sleep_calls.get() + 1),
        Duration::from_secs(60),
    );
    assert!(result.is_ok());
    assert_eq!(poll_count.get(), 2, "try_wait must be polled twice");
    assert_eq!(sleep_calls.get(), 1, "sleep_fn must be invoked once");
}

#[test]
fn wait_with_timeout_expires_returns_timeout_error() {
    let sleep_calls = Cell::new(0u32);
    let result = wait_with_timeout(
        || Ok(None),
        |_| sleep_calls.set(sleep_calls.get() + 1),
        Duration::from_secs(0),
    );
    match result {
        Err(WaitError::Timeout) => {}
        Ok(_) => panic!("expected WaitError::Timeout, got Ok"),
    }
    assert_eq!(
        sleep_calls.get(),
        0,
        "deadline check fires before sleep on timeout=0"
    );
}

// --- Subprocess integration tests for `preflight` wrapper and CLI ---

fn make_repo_fixture(parent: &Path) -> PathBuf {
    let repo = common::create_git_repo_with_remote(parent);
    let repo = repo.canonicalize().expect("canonicalize repo");
    Command::new("git")
        .args(["checkout", "-b", BRANCH])
        .current_dir(&repo)
        .output()
        .unwrap();
    repo
}

fn write_state_file(repo: &Path, branch: &str, learn_status: &str) {
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state = common::make_complete_state(branch, learn_status, None);
    let state_path = state_dir.join(format!("{}.json", branch));
    fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
}

fn run_complete_preflight(repo: &Path, branch_arg: Option<&str>) -> (i32, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.arg("complete-preflight")
        .arg("--auto")
        .current_dir(repo)
        .env_remove("FLOW_CI_RUNNING");
    if let Some(b) = branch_arg {
        cmd.arg("--branch").arg(b);
    }
    let output = cmd.output().expect("spawn flow-rs");
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

#[test]
fn preflight_run_error_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    write_state_file(&repo, BRANCH, "complete");

    let (code, stdout, _) = run_complete_preflight(&repo, Some(BRANCH));

    assert_eq!(
        code, 1,
        "no-gh-auth fixture must surface status=error via exit 1; stdout={}",
        stdout
    );
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
}

#[test]
fn preflight_run_ok_exits_0() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    write_state_file(&repo, BRANCH, "complete");

    // Install fakes for `gh` (returns "MERGED") and `bin/flow`
    // (returns valid phase-transition JSON) so preflight lands on the
    // MERGED-early-return happy path and the wrapper exits 0
    // deterministically — covering the `{ 0 }` arm of
    // `run_impl_main`'s exit-code selection.
    let bin_dir = repo.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let fake_gh = bin_dir.join("gh");
    fs::write(&fake_gh, "#!/usr/bin/env bash\necho MERGED\n").unwrap();
    // `bin_flow_path()` returns `<target>/bin/flow` where target is
    // the project_root parent. For our repo fixture the project_root
    // resolves to the worktree, so a `bin/flow` stub inside the repo
    // suffices. It echoes a valid phase-transition JSON payload.
    let fake_flow = bin_dir.join("flow");
    fs::write(
        &fake_flow,
        r#"#!/usr/bin/env bash
echo '{"status": "ok", "phase": "flow-complete", "action": "enter", "visit_count": 1, "first_visit": true}'
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake_gh, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&fake_flow, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let original_path = std::env::var("PATH").unwrap_or_default();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.arg("complete-preflight")
        .arg("--auto")
        .arg("--branch")
        .arg(BRANCH)
        .current_dir(&repo)
        .env_remove("FLOW_CI_RUNNING")
        .env("FLOW_BIN_PATH", fake_flow.to_str().unwrap())
        .env("PATH", format!("{}:{}", bin_dir.display(), original_path));
    let output = cmd.output().expect("spawn flow-rs");
    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    assert_eq!(
        code, 0,
        "fake-gh MERGED path must exit 0; stdout={}",
        stdout
    );
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["pr_state"], "MERGED");
}

#[test]
fn preflight_wrapper_resolves_current_branch_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    write_state_file(&repo, BRANCH, "complete");

    let (_, stdout, stderr) = run_complete_preflight(&repo, None);

    let json = last_json_line(&stdout);
    let msg = json["message"].as_str().unwrap_or("");
    assert!(
        !msg.contains("Could not determine current branch"),
        "current_branch fallback should have resolved to test-feature; stderr={}",
        stderr
    );
}

// --- Additional coverage for runner Err paths and resolve_mode else-branches ---

#[test]
fn resolve_mode_state_dict_number_value_defaults_auto() {
    // flow-complete is neither a string nor an object — both if-lets
    // fall through and the default-auto tail fires.
    let state = json!({"skills": {"flow-complete": 42}});
    assert_eq!(resolve_mode(false, false, Some(&state)), "auto");
}

#[test]
fn check_pr_status_runner_err_surfaces() {
    let runner = mock_runner(vec![err("network down")]);
    let result = check_pr_status(Some(42), "b", &runner);
    assert_eq!(result.unwrap_err(), "network down");
}

#[test]
fn merge_main_merge_base_runner_err_returns_error() {
    let runner = mock_runner(vec![ok_empty(), err("merge-base timeout")]);
    let (status, data) = merge_main(&runner);
    assert_eq!(status, "error");
    assert_eq!(data.unwrap().as_str().unwrap(), "merge-base timeout");
}

#[test]
fn merge_main_merge_runner_err_returns_error() {
    let runner = mock_runner(vec![
        ok_empty(),
        fail(""), // merge-base not ancestor
        err("merge timeout"),
    ]);
    let (status, data) = merge_main(&runner);
    assert_eq!(status, "error");
    assert_eq!(data.unwrap().as_str().unwrap(), "merge timeout");
}

#[test]
fn merge_main_push_runner_err_returns_error() {
    let runner = mock_runner(vec![
        ok_empty(),
        fail(""),
        ok("Merge made"),
        err("push timeout"),
    ]);
    let (status, data) = merge_main(&runner);
    assert_eq!(status, "error");
    assert!(data
        .unwrap()
        .as_str()
        .unwrap()
        .contains("Merge succeeded but push failed: push timeout"));
}

#[test]
fn merge_main_status_runner_err_returns_error() {
    let runner = mock_runner(vec![
        ok_empty(),
        fail(""),
        fail("merge stderr"),
        err("status timeout"),
    ]);
    let (status, data) = merge_main(&runner);
    assert_eq!(status, "error");
    assert_eq!(data.unwrap().as_str().unwrap(), "merge stderr");
}

// Exercises the `Err(e) =>` arm of fs::read_to_string on the state
// file. The target path is a directory instead of a file, so
// read_to_string returns EISDIR.
#[test]
fn preflight_state_path_is_directory_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("test-feature.json");
    fs::create_dir(&state_path).unwrap();

    let runner = mock_runner(vec![]);
    let result = preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("Could not read state file"));
}

// Exercises the mutate_state closure's wrong-root-type guard.
// A JSON array as the root causes the closure to early-return.
#[test]
fn preflight_state_root_non_object_skips_counter_write() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("test-feature.json");
    // A JSON array — preflight_inner parses it successfully (Value allows
    // array at root), then phase_transition_enter runs, then the
    // step-counter mutate_state sees an array and returns early via the
    // type guard without mutation.
    fs::write(&state_path, "[1, 2, 3]").unwrap();

    let runner = mock_runner(vec![ok(PT_ENTER_OK), ok("OPEN"), ok_empty(), ok_empty()]);
    let result = preflight_inner(
        Some("test-feature"),
        false,
        false,
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );
    // The preflight proceeds (status is ok or conflict or error
    // depending on how downstream code handles the array root). The
    // assertion here is that the array payload was not mutated —
    // mutate_state's guard returned early without panicking.
    let contents = fs::read_to_string(&state_path).unwrap();
    let parsed: Value = serde_json::from_str(&contents).unwrap();
    assert!(
        parsed.is_array(),
        "state root must remain array; guard fired"
    );
    let _ = result; // don't care about downstream shape here
}

// Exercises `run_impl_main`'s `else { 1 }` branch when preflight's
// result has status != "ok". No fixture in this dir → preflight_inner
// returns an inferred result; since there's no gh auth in tests, it
// fails at check_pr_status → status=error → code=1.
#[test]
fn run_impl_main_non_ok_status_returns_exit_1() {
    use flow_rs::complete_preflight::{run_impl_main, Args};
    let dir = tempfile::tempdir().unwrap();
    // Run in a tempdir with no gh/git context. project_root() inside
    // run_impl_main will resolve to the host repo, but with
    // --branch=<nonexistent>, state_file lookup fails and proceeds to
    // gh pr view which fails without auth in test env.
    let args = Args {
        branch: Some("nonexistent-branch-xyz".to_string()),
        auto: true,
        manual: false,
    };
    let (value, code) = run_impl_main(&args);
    // The exact status depends on environment (whether gh is auth'd);
    // the important coverage is the code-selection branch.
    let status = value["status"].as_str().unwrap_or("");
    let _ = dir;
    if status == "ok" {
        assert_eq!(code, 0);
    } else {
        assert_eq!(code, 1);
    }
}

#[test]
fn preflight_wrapper_uses_explicit_branch_override() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    write_state_file(&repo, "different-branch", "complete");

    let (_, stdout, _) = run_complete_preflight(&repo, Some("different-branch"));

    let json = last_json_line(&stdout);
    let msg = json["message"].as_str().unwrap_or("");
    assert!(
        !msg.contains("Could not determine current branch"),
        "explicit --branch must prevail over current_branch(); got: {}",
        msg
    );
    assert!(
        json["status"].is_string(),
        "result must have a status field; got: {}",
        json
    );
}
