//! Subprocess integration tests for `bin/flow complete-post-merge`.
//!
//! Covers the CLI entry (`run`) and the `post_merge` production
//! wrapper that calls `project_root()`. The inline tests in
//! `src/complete_post_merge.rs::tests` cover `post_merge_inner`'s
//! branches via mock runners; these subprocess tests prove the
//! wrapper dispatches end-to-end and honors the best-effort
//! always-exit-0 contract.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

use flow_rs::complete_post_merge::post_merge_inner;
use flow_rs::complete_preflight::CmdResult;
use serde_json::{json, Value};

mod common;

const BRANCH: &str = "test-feature";

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

fn write_state_file(repo: &Path, branch: &str) -> PathBuf {
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state = common::make_complete_state(branch, "complete", None);
    let state_path = state_dir.join(format!("{}.json", branch));
    fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    state_path
}

/// Write a `bin/flow` stub that exits 0 for every subcommand.
/// Post-merge calls several `bin/flow` subcommands (phase-transition,
/// render-pr-body, format-issues-summary, close-issues,
/// format-complete-summary, label-issues); the stub makes them all
/// succeed trivially without touching GitHub or the state file.
fn write_flow_stub(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let script = "#!/bin/sh\nexit 0\n";
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Write a `gh` stub that exits 0 for every subcommand.
fn build_path_stub_dir(parent: &Path) -> PathBuf {
    let stubs = parent.join("stubs");
    fs::create_dir_all(&stubs).unwrap();
    let gh_path = stubs.join("gh");
    fs::write(&gh_path, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&gh_path, fs::Permissions::from_mode(0o755)).unwrap();
    stubs
}

fn run_post_merge(
    cwd: &Path,
    pr: &str,
    state_file: &str,
    branch: &str,
    flow_bin_path: &Path,
    path_stub_dir: &Path,
) -> (i32, String, String) {
    let current_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", path_stub_dir.display(), current_path);
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "complete-post-merge",
            "--pr",
            pr,
            "--state-file",
            state_file,
            "--branch",
            branch,
        ])
        .current_dir(cwd)
        .env("PATH", new_path)
        .env("FLOW_BIN_PATH", flow_bin_path)
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

/// With most subprocesses stubbed to succeed and a minimal state
/// fixture, post-merge runs to completion and exits 0 per its
/// best-effort always-exit-0 contract. Exercises the CLI `run`
/// entry's unconditional exit-0 arm.
#[test]
fn post_merge_run_best_effort_exits_0() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = write_state_file(&repo, BRANCH);
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let path_stub = build_path_stub_dir(&parent);

    let (code, stdout, _) = run_post_merge(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &path_stub,
    );

    assert_eq!(
        code, 0,
        "complete-post-merge is best-effort and always exits 0; stdout={}",
        stdout
    );
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
}

/// The `post_merge` wrapper calls `project_root()` and threads
/// production `bin_flow_path()` + `run_cmd_with_timeout` into
/// `post_merge_inner`. With stubs in place, the resulting JSON
/// contains the expected default fields (status, closed_issues,
/// parents_closed, slack), proving the wrapper's delegation chain
/// reaches `post_merge_inner` end-to-end.
#[test]
fn post_merge_wrapper_resolves_project_root() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = write_state_file(&repo, BRANCH);
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let path_stub = build_path_stub_dir(&parent);

    let (_, stdout, _) = run_post_merge(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &path_stub,
    );

    let json = last_json_line(&stdout);
    // The wrapper's delegation is proved by the presence of the
    // default result fields that only `post_merge_inner` populates.
    assert!(
        json.get("closed_issues").is_some(),
        "post_merge_inner must populate closed_issues; got: {}",
        json
    );
    assert!(
        json.get("parents_closed").is_some(),
        "post_merge_inner must populate parents_closed; got: {}",
        json
    );
    assert!(
        json.get("slack").is_some(),
        "post_merge_inner must populate slack; got: {}",
        json
    );
}

// ===================================================================
// Unit tests for `post_merge_inner` — drives the public seam with a
// mock runner. Migrated from inline `#[cfg(test)]` in
// `src/complete_post_merge.rs` per `.claude/rules/test-placement.md`.
// ===================================================================

const PT_COMPLETE_OK: &str = r#"{"status": "ok", "phase": "flow-complete", "action": "complete", "cumulative_seconds": 45, "formatted_time": "<1m", "next_phase": "flow-complete", "continue_action": "invoke"}"#;
const RENDER_PR_OK: &str = r#"{"status": "ok", "sections": ["What"]}"#;
const ISSUES_SUMMARY_NO_ISSUES: &str =
    r#"{"status": "ok", "has_issues": false, "banner_line": "", "table": ""}"#;
const ISSUES_SUMMARY_WITH_ISSUES: &str = r#"{"status": "ok", "has_issues": true, "banner_line": "Issues filed: 1 (Flaky Test: 1)", "table": "| Label | Title |"}"#;
const CLOSE_ISSUES_EMPTY: &str = r#"{"status": "ok", "closed": [], "failed": []}"#;
const CLOSE_ISSUES_WITH_CLOSED: &str = r#"{"status": "ok", "closed": [{"number": 100, "url": "https://github.com/test/test/issues/100"}], "failed": []}"#;
const SUMMARY_OK: &str =
    r#"{"status": "ok", "summary": "test summary", "total_seconds": 300, "issues_links": ""}"#;
const LABEL_OK: &str = r#"{"status": "ok", "labeled": [100], "failed": []}"#;
const AUTO_CLOSE_OK: &str =
    r#"{"status": "ok", "parent_closed": false, "milestone_closed": false}"#;
const SLACK_OK: &str = r#"{"status": "ok", "ts": "1234567890.123456"}"#;
const ADD_NOTIFICATION_OK: &str = r#"{"status": "ok", "notification_count": 1}"#;

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

fn fail(stderr: &str) -> CmdResult {
    Ok((1, String::new(), stderr.to_string()))
}

fn err(msg: &str) -> CmdResult {
    Err(msg.to_string())
}

/// Setup fixture: create root/.flow-states/ and write state file there.
fn setup_inner(
    dir: &Path,
    branch: &str,
    slack_thread_ts: Option<&str>,
    repo: Option<&str>,
) -> PathBuf {
    let state_dir = dir.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let mut state = json!({
        "schema_version": 1,
        "branch": branch,
        "pr_number": 42,
        "pr_url": "https://github.com/test/test/pull/42",
        "prompt": "work on issue #100",
        "complete_step": 5,
        "phases": {
            "flow-start": {"status": "complete"},
            "flow-plan": {"status": "complete"},
            "flow-code": {"status": "complete"},
            "flow-code-review": {"status": "complete"},
            "flow-learn": {"status": "complete"},
            "flow-complete": {"status": "in_progress"}
        }
    });
    if let Some(ts) = slack_thread_ts {
        state["slack_thread_ts"] = json!(ts);
    }
    if let Some(r) = repo {
        state["repo"] = json!(r);
    } else {
        state["repo"] = json!("test/test");
    }
    let state_path = state_dir.join(format!("{}.json", branch));
    fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    state_path
}

// --- happy paths ---

#[test]
fn happy_path_no_issues() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", None, None);

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_EMPTY),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "ok");
    assert_eq!(result["formatted_time"], "<1m");
    assert_eq!(result["summary"], "test summary");
    assert_eq!(result["cumulative_seconds"], 45);
}

#[test]
fn happy_path_with_closed_issues() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", None, None);

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_WITH_CLOSED),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
        ok(AUTO_CLOSE_OK),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "ok");
    assert_eq!(result["closed_issues"].as_array().unwrap().len(), 1);

    let closed_path = dir
        .path()
        .join(".flow-states")
        .join("test-feature-closed-issues.json");
    assert!(closed_path.exists());
    let content: Value = serde_json::from_str(&fs::read_to_string(&closed_path).unwrap()).unwrap();
    assert_eq!(content[0]["number"], 100);
}

#[test]
fn individual_failure_continues() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", None, None);

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_EMPTY),
        ok(SUMMARY_OK),
        fail("gh error"),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "ok");
    assert!(result["failures"]
        .as_object()
        .unwrap()
        .contains_key("label_issues"));
}

// --- slack ---

#[test]
fn slack_not_configured() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", None, None);

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_EMPTY),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["slack"]["status"], "skipped");
}

#[test]
fn slack_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", Some("1234.5678"), None);

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_EMPTY),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
        ok(SLACK_OK),
        ok(ADD_NOTIFICATION_OK),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["slack"]["status"], "ok");
    assert_eq!(result["slack"]["ts"], "1234567890.123456");
}

#[test]
fn slack_failure_continues() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", Some("1234.5678"), None);

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_EMPTY),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
        ok(r#"{"status": "error", "message": "token expired"}"#),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["slack"]["status"], "error");
    assert_eq!(result["status"], "ok");
}

#[test]
fn slack_invalid_response() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", Some("1234.5678"), None);

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_EMPTY),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
        ok("not json"),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["slack"]["status"], "error");
    assert!(result["slack"]["message"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("invalid"));
}

#[test]
fn slack_thread_ts_empty_string_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", Some(""), None);

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_EMPTY),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["slack"]["status"], "skipped");
}

#[test]
fn slack_transport_error() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", Some("1234.5678"), None);

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_EMPTY),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
        err("Timed out after 60s"),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["slack"]["status"], "error");
    assert_eq!(result["status"], "ok");
}

// --- phase-transition invocation ---

#[test]
fn phase_transition_called_with_next_phase() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", None, None);

    let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::new(RefCell::new(Vec::new()));
    let runner = tracking_runner(
        vec![
            ok(PT_COMPLETE_OK),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok(CLOSE_ISSUES_EMPTY),
            ok(SUMMARY_OK),
            ok(LABEL_OK),
        ],
        calls.clone(),
    );

    post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
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
    assert!(pt_call.contains(&"--next-phase".to_string()));
    assert!(pt_call.contains(&"flow-complete".to_string()));
    assert!(pt_call.contains(&"--branch".to_string()));
    assert!(pt_call.contains(&"test-feature".to_string()));
}

#[test]
fn render_pr_body_called_with_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", None, None);

    let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::new(RefCell::new(Vec::new()));
    let runner = tracking_runner(
        vec![
            ok(PT_COMPLETE_OK),
            ok(RENDER_PR_OK),
            ok(ISSUES_SUMMARY_NO_ISSUES),
            ok(CLOSE_ISSUES_EMPTY),
            ok(SUMMARY_OK),
            ok(LABEL_OK),
        ],
        calls.clone(),
    );

    post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    let render_call = calls
        .borrow()
        .iter()
        .find(|c| c.iter().any(|a| a == "render-pr-body"))
        .cloned()
        .expect("render-pr-body call not found");
    assert!(render_call.contains(&"--state-file".to_string()));
    assert!(render_call.contains(&state_path.to_str().unwrap().to_string()));
}

// --- step counter persistence ---

#[test]
fn step_counters_updated() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", None, None);

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_EMPTY),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
    ]);

    post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    let content = fs::read_to_string(&state_path).unwrap();
    let state: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(state["complete_step"], json!(6));
}

// --- error paths ---

#[test]
fn phase_transition_failure_captured() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", None, None);

    let runner = mock_runner(vec![
        fail(r#"{"status": "error", "message": "bad state"}"#),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_EMPTY),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "ok");
    assert!(result["failures"]
        .as_object()
        .unwrap()
        .contains_key("phase_transition"));
}

#[test]
fn corrupt_state_file_handled() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("test-feature.json");
    fs::write(&state_path, "not valid json{{{").unwrap();

    let runner = mock_runner(vec![
        fail(r#"{"status": "error"}"#),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_EMPTY),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "ok");
}

#[test]
fn issues_summary_with_issues_populates_banner() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", None, None);

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_WITH_ISSUES),
        ok(CLOSE_ISSUES_EMPTY),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["banner_line"], "Issues filed: 1 (Flaky Test: 1)");
}

#[test]
fn closed_issues_file_write_error() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    let state = json!({
        "schema_version": 1,
        "branch": "test-feature",
        "pr_number": 42,
        "repo": "test/test",
        "phases": {}
    });
    fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_WITH_CLOSED),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
        ok(AUTO_CLOSE_OK),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert!(result["failures"]
        .as_object()
        .unwrap()
        .contains_key("closed_issues_file"));
}

#[test]
fn parent_closed_populates_parents_closed() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", None, None);

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_WITH_CLOSED),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
        ok(r#"{"status": "ok", "parent_closed": true, "milestone_closed": false}"#),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    let parents: Vec<i64> = result["parents_closed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    assert_eq!(parents, vec![100]);
}

#[test]
fn milestone_closed_also_populates_parents_closed() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", None, None);

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_WITH_CLOSED),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
        ok(r#"{"status": "ok", "parent_closed": false, "milestone_closed": true}"#),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    let parents: Vec<i64> = result["parents_closed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    assert_eq!(parents, vec![100]);
}

#[test]
fn repo_null_skips_auto_close_parent() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state = json!({
        "schema_version": 1,
        "branch": "test-feature",
        "pr_number": 42,
        "repo": null,
        "phases": {}
    });
    let state_path = state_dir.join("test-feature.json");
    fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_WITH_CLOSED),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "ok");
    assert_eq!(result["parents_closed"], json!([]));
}

#[test]
fn repo_empty_string_skips_auto_close_parent() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", None, Some(""));

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_WITH_CLOSED),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["parents_closed"], json!([]));
}

#[test]
fn timeout_handling_all_calls_fail() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", None, None);

    let runner = mock_runner(vec![
        err("Timed out after 60s"),
        err("Timed out after 60s"),
        err("Timed out after 30s"),
        err("Timed out after 60s"),
        err("Timed out after 30s"),
        err("Timed out after 60s"),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "ok");
    let failures = result["failures"].as_object().unwrap();
    assert!(failures.contains_key("phase_transition"));
    assert!(failures.contains_key("render_pr_body"));
    assert!(failures.contains_key("label_issues"));
}

#[test]
fn render_pr_body_failure_captured() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", None, None);

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        fail("gh API error"),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_EMPTY),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert!(result["failures"]
        .as_object()
        .unwrap()
        .contains_key("render_pr_body"));
}

#[test]
fn missing_state_file_still_produces_result() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join(".flow-states").join("test-feature.json");
    fs::create_dir_all(state_path.parent().unwrap()).unwrap();

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_EMPTY),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "ok");
    assert_eq!(result["slack"]["status"], "skipped");
}

#[test]
fn non_object_state_file_does_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("test-feature.json");
    fs::write(&state_path, "[1, 2, 3]").unwrap();

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_EMPTY),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "ok");
}

#[test]
fn invalid_branch_with_slash_returns_early_with_invalid_branch_failure() {
    // Covers the `FlowPaths::try_new` None branch (invalid_branch path).
    // A slash-containing branch is rejected by FlowPaths because the
    // flat .flow-states/ layout cannot address it.
    let dir = tempfile::tempdir().unwrap();
    let runner = mock_runner(vec![]);

    let result = post_merge_inner(
        42,
        "/nonexistent/state.json",
        "feature/slash",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    let failures = result["failures"].as_object().unwrap();
    assert!(failures.contains_key("invalid_branch"));
    assert!(failures["invalid_branch"]
        .as_str()
        .unwrap()
        .contains("contains '/'"));
    // No further subprocesses should have been called.
}

#[test]
fn state_path_read_error_falls_back_to_empty_state() {
    // Covers the `Err(_) => json!({})` branch of read_to_string:
    // state_path.exists() is true but read fails (e.g. path is a
    // directory). post_merge_inner must tolerate the failure and
    // continue with an empty state value.
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    // Create a DIRECTORY at the state_path so read_to_string fails.
    let state_path = state_dir.join("test-feature.json");
    fs::create_dir_all(&state_path).unwrap();

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_EMPTY),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "ok");
    // Empty state → no repo → no auto-close-parent calls, no slack.
    assert_eq!(result["slack"]["status"], "skipped");
}

#[test]
fn format_complete_summary_non_ok_status_does_not_populate_summary() {
    // Covers the fallthrough when sum_data.status != "ok": summary and
    // issues_links stay at their default empty strings.
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", None, None);

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_EMPTY),
        ok(r#"{"status": "error", "message": "summary failed"}"#),
        ok(LABEL_OK),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["summary"], "");
    assert_eq!(result["issues_links"], "");
}

#[test]
fn auto_close_parent_runner_error_is_silently_ignored() {
    // Covers the fallthrough when runner returns Err for auto-close-
    // parent (closing `}` of `if let Ok(..) = runner(..)`). The error
    // is swallowed per best-effort semantics.
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", None, None);

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_WITH_CLOSED),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
        err("auto-close-parent timeout"),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "ok");
    assert_eq!(result["parents_closed"], json!([]));
}

#[test]
fn slack_status_ok_without_ts_skips_add_notification() {
    // Covers the None branch of `if let Some(ts) = ts_opt`: slack returns
    // status=ok but no "ts" field (or empty). We still record the slack
    // object but skip the add-notification call.
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", Some("1234.5678"), None);

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_EMPTY),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
        ok(r#"{"status": "ok"}"#),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["slack"]["status"], "ok");
}

#[test]
fn phase_transition_parseable_non_ok_falls_back_to_stderr_branch() {
    // Covers `let msg = parse_err.unwrap_or_else(|| stderr.trim().to_string())`
    // at line 181: the closure fires when parse_err is None — i.e.,
    // stdout IS valid JSON but status is not "ok". The fallback closure
    // reads stderr.
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", None, None);

    let runner = mock_runner(vec![
        // phase-transition returns valid JSON but status != "ok" AND
        // subprocess exited 1 so stderr carries text.
        Ok((
            1,
            r#"{"status":"error","message":"phase locked"}"#.to_string(),
            "stderr detail".to_string(),
        )),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_EMPTY),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    let failures = result["failures"].as_object().unwrap();
    assert!(failures.contains_key("phase_transition"));
    // The message came from stderr (not the JSON parse error), because
    // parse_err was None (JSON parsed cleanly) and the fallback fired.
    assert_eq!(failures["phase_transition"], "stderr detail");
}

#[test]
fn close_issues_json_without_closed_array_produces_empty_list() {
    // Covers the fallthrough of `if let Some(closed_arr) = close_data.get("closed")...`
    // — close-issues returns parseable JSON without a "closed" array.
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", None, None);

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(r#"{"status":"ok"}"#),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "ok");
    assert_eq!(result["closed_issues"], json!([]));
}

#[test]
fn closed_issue_without_number_field_skips_auto_close() {
    // Covers the fallthrough of `if let Some(issue_num) = issue.get("number")...`
    // when a closed issue entry has no numeric "number" field.
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", None, None);

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        // closed entry missing "number" — skip auto-close-parent.
        ok(r#"{"status":"ok","closed":[{"url":"https://example.com/issues/x"}],"failed":[]}"#),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
        // No auto-close-parent call — issue had no number.
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "ok");
    assert_eq!(result["parents_closed"], json!([]));
}

#[test]
fn auto_close_parent_parse_failure_is_silently_ignored() {
    // Covers the fallthrough when auto-close-parent's stdout is not
    // parseable JSON — the `if let Some(acp_data) = parsed` arm does
    // not fire and parents_closed stays empty.
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", None, None);

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok(CLOSE_ISSUES_WITH_CLOSED),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
        ok("not valid json{{{"),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "ok");
    assert_eq!(result["parents_closed"], json!([]));
}

#[test]
fn close_issues_parse_failure_continues() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = setup_inner(dir.path(), "test-feature", None, None);

    let runner = mock_runner(vec![
        ok(PT_COMPLETE_OK),
        ok(RENDER_PR_OK),
        ok(ISSUES_SUMMARY_NO_ISSUES),
        ok("not json"),
        ok(SUMMARY_OK),
        ok(LABEL_OK),
    ]);

    let result = post_merge_inner(
        42,
        state_path.to_str().unwrap(),
        "test-feature",
        dir.path(),
        "/fake/bin/flow",
        &runner,
    );

    assert_eq!(result["status"], "ok");
    assert_eq!(result["closed_issues"], json!([]));
}
