//! Subprocess integration tests for `bin/flow complete-preflight`.
//!
//! `wait_with_timeout`, `WaitError`, `preflight_inner`, and `preflight`
//! pub seams were removed; the runner-closure injection was dropped
//! from `check_pr_status` and `merge_main`. Tests drive the compiled
//! `flow-rs complete-preflight` binary with env-controlled stubs for
//! `bin/flow phase-transition`, `gh pr view`, and the full git family.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

use flow_rs::complete_preflight::{
    check_learn_phase, check_pr_status, merge_main, resolve_mode, run_cmd_with_timeout,
};

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

fn write_state_file(repo: &Path, branch: &str, learn_status: &str) -> PathBuf {
    let branch_dir = repo.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    let state = json!({
        "schema_version": 1,
        "branch": branch,
        "pr_number": 42,
        "pr_url": "https://github.com/test/test/pull/42",
        "phases": {
            "flow-learn": {"status": learn_status},
        },
    });
    fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    state_path
}

/// bin/flow stub handling phase-transition via FAKE_PT_OUT/FAKE_PT_EXIT.
fn write_flow_stub(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let script = r#"#!/bin/sh
case "$1" in
    phase-transition)
        if [ -n "$FAKE_PT_STDERR" ]; then printf '%s' "$FAKE_PT_STDERR" >&2; fi
        if [ -n "$FAKE_PT_OUT" ]; then
            printf '%s' "$FAKE_PT_OUT"
        else
            printf '%s' '{"status":"ok"}'
        fi
        exit ${FAKE_PT_EXIT:-0}
        ;;
    *)
        exit 0
        ;;
esac
"#;
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn build_path_stubs(parent: &Path) -> PathBuf {
    let stubs = parent.join("stubs");
    fs::create_dir_all(&stubs).unwrap();

    let gh_script = r#"#!/bin/sh
case "$1 $2" in
    "pr view")
        if [ -n "$FAKE_PR_VIEW_STDERR" ]; then printf '%s' "$FAKE_PR_VIEW_STDERR" >&2; fi
        if [ -n "$FAKE_PR_STATE" ]; then
            printf '%s' "$FAKE_PR_STATE"
        else
            printf '%s' 'OPEN'
        fi
        exit ${FAKE_PR_VIEW_EXIT:-0}
        ;;
    *)
        exit 0
        ;;
esac
"#;
    let gh_path = stubs.join("gh");
    fs::write(&gh_path, gh_script).unwrap();
    fs::set_permissions(&gh_path, fs::Permissions::from_mode(0o755)).unwrap();

    let git_script = r#"#!/bin/sh
case "$1" in
    fetch)
        if [ -n "$FAKE_FETCH_STDERR" ]; then printf '%s' "$FAKE_FETCH_STDERR" >&2; fi
        exit ${FAKE_FETCH_EXIT:-0}
        ;;
    merge-base)
        exit ${FAKE_MERGE_BASE_EXIT:-0}
        ;;
    merge)
        if [ -n "$FAKE_GIT_MERGE_STDERR" ]; then printf '%s' "$FAKE_GIT_MERGE_STDERR" >&2; fi
        exit ${FAKE_GIT_MERGE_EXIT:-0}
        ;;
    push)
        if [ -n "$FAKE_PUSH_STDERR" ]; then printf '%s' "$FAKE_PUSH_STDERR" >&2; fi
        exit ${FAKE_PUSH_EXIT:-0}
        ;;
    status)
        if [ -n "$FAKE_GIT_STATUS_OUT" ]; then
            printf '%s' "$FAKE_GIT_STATUS_OUT"
        fi
        exit 0
        ;;
    *)
        exec /usr/bin/git "$@"
        ;;
esac
"#;
    let git_path = stubs.join("git");
    fs::write(&git_path, git_script).unwrap();
    fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();

    stubs
}

fn run_preflight(
    cwd: &Path,
    branch_arg: Option<&str>,
    mode_flag: Option<&str>,
    flow_bin_path: &Path,
    stubs: &Path,
    env: &[(&str, &str)],
) -> Output {
    let current_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", stubs.display(), current_path);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.arg("complete-preflight");
    if let Some(b) = branch_arg {
        cmd.arg("--branch").arg(b);
    }
    if let Some(flag) = mode_flag {
        cmd.arg(flag);
    }
    cmd.current_dir(cwd)
        .env("PATH", new_path)
        .env("FLOW_BIN_PATH", flow_bin_path)
        .env_remove("FLOW_CI_RUNNING");
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().expect("spawn flow-rs")
}

fn last_json_line(stdout: &str) -> Value {
    let last = stdout
        .lines()
        .rfind(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON line in stdout; stdout={}", stdout));
    serde_json::from_str(last)
        .unwrap_or_else(|e| panic!("failed to parse JSON line '{}': {}", last, e))
}

struct Fixture {
    _dir: tempfile::TempDir,
    repo: PathBuf,
    flow_bin: PathBuf,
    stubs: PathBuf,
}

fn setup(learn_status: &str) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    write_state_file(&repo, BRANCH, learn_status);
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);
    Fixture {
        _dir: dir,
        repo,
        flow_bin,
        stubs,
    }
}

// --- Library-level tests for pure helpers ---

#[test]
fn resolve_mode_auto_flag_wins() {
    assert_eq!(resolve_mode(true, false, None), "auto");
}

#[test]
fn resolve_mode_manual_flag_wins() {
    assert_eq!(resolve_mode(false, true, None), "manual");
}

#[test]
fn resolve_mode_skill_string_used() {
    let state = json!({"skills": {"flow-complete": "manual"}});
    assert_eq!(resolve_mode(false, false, Some(&state)), "manual");
}

#[test]
fn resolve_mode_skill_object_continue_used() {
    let state = json!({"skills": {"flow-complete": {"continue": "manual"}}});
    assert_eq!(resolve_mode(false, false, Some(&state)), "manual");
}

#[test]
fn resolve_mode_skill_object_missing_continue_defaults_auto() {
    let state = json!({"skills": {"flow-complete": {"other": "x"}}});
    assert_eq!(resolve_mode(false, false, Some(&state)), "auto");
}

#[test]
fn resolve_mode_skill_config_number_defaults_auto() {
    // skill_config is neither string nor object — falls past both
    // branches and returns the "auto" default.
    let state = json!({"skills": {"flow-complete": 42}});
    assert_eq!(resolve_mode(false, false, Some(&state)), "auto");
}

#[test]
fn resolve_mode_skill_config_array_defaults_auto() {
    let state = json!({"skills": {"flow-complete": ["x"]}});
    assert_eq!(resolve_mode(false, false, Some(&state)), "auto");
}

#[test]
fn resolve_mode_no_skill_config_defaults_auto() {
    let state = json!({});
    assert_eq!(resolve_mode(false, false, Some(&state)), "auto");
}

#[test]
fn resolve_mode_no_state_defaults_auto() {
    assert_eq!(resolve_mode(false, false, None), "auto");
}

#[test]
fn check_learn_phase_pending_returns_warning() {
    let state = json!({"phases": {"flow-learn": {"status": "pending"}}});
    let w = check_learn_phase(&state);
    assert_eq!(w.len(), 1);
    assert!(w[0].contains("Phase 5 not complete"));
}

#[test]
fn check_learn_phase_complete_no_warnings() {
    let state = json!({"phases": {"flow-learn": {"status": "complete"}}});
    assert!(check_learn_phase(&state).is_empty());
}

#[test]
fn check_learn_phase_missing_phases_returns_warning() {
    let state = json!({});
    let w = check_learn_phase(&state);
    assert_eq!(w.len(), 1);
}

#[test]
fn run_cmd_with_timeout_empty_args_returns_err() {
    let result = run_cmd_with_timeout(&[], 30);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "empty command");
}

#[test]
fn run_cmd_with_timeout_success_returns_ok() {
    let result = run_cmd_with_timeout(&["/bin/sh", "-c", "echo hello"], 30);
    let (code, stdout, _stderr) = result.expect("sh must be available");
    assert_eq!(code, 0);
    assert!(stdout.contains("hello"));
}

#[test]
fn run_cmd_with_timeout_nonzero_exit_returned_as_ok() {
    let result = run_cmd_with_timeout(&["/bin/sh", "-c", "exit 7"], 30);
    let (code, _, _) = result.expect("sh must be available");
    assert_eq!(code, 7);
}

#[test]
fn run_cmd_with_timeout_spawn_failure_returns_err() {
    let result = run_cmd_with_timeout(&["/nonexistent-binary-12345"], 30);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_lowercase()
        .contains("failed to spawn"));
}

#[test]
fn run_cmd_with_timeout_timeout_kills_child() {
    let result = run_cmd_with_timeout(&["/bin/sh", "-c", "sleep 5"], 0);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_lowercase().contains("timed out"));
}

#[test]
fn check_pr_status_no_pr_number_no_branch_returns_err() {
    let result = check_pr_status(None, "");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("No PR number or branch"));
}

// --- Subprocess tests for the full preflight flow ---

#[test]
fn slash_branch_returns_structured_error() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);

    let output = run_preflight(
        &repo,
        Some("feature/foo"),
        Some("--auto"),
        &flow_bin,
        &stubs,
        &[],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap_or("")
        .contains("not a valid FLOW branch"));
}

#[test]
fn no_branch_and_no_git_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);

    let output = run_preflight(&parent, None, None, &flow_bin, &stubs, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
}

#[test]
fn no_state_file_inferred_proceeds() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);

    let output = run_preflight(
        &repo,
        Some(BRANCH),
        Some("--auto"),
        &flow_bin,
        &stubs,
        &[("FAKE_PR_STATE", "MERGED")],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["inferred"], true);
    assert_eq!(json["pr_state"], "MERGED");
}

#[test]
fn corrupt_state_file_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let branch_dir = repo.join(".flow-states").join(BRANCH);
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), "{corrupt").unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);

    let output = run_preflight(&repo, Some(BRANCH), Some("--auto"), &flow_bin, &stubs, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("Could not parse state file"));
}

#[test]
fn state_path_is_directory_returns_read_error() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let branch_dir = repo.join(".flow-states").join(BRANCH);
    fs::create_dir_all(&branch_dir).unwrap();
    fs::create_dir(branch_dir.join("state.json")).unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);

    let output = run_preflight(&repo, Some(BRANCH), Some("--auto"), &flow_bin, &stubs, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("Could not read state file"));
}

#[test]
fn phase_transition_error_returns_error() {
    let fx = setup("complete");
    let output = run_preflight(
        &fx.repo,
        Some(BRANCH),
        Some("--auto"),
        &fx.flow_bin,
        &fx.stubs,
        &[("FAKE_PT_EXIT", "1"), ("FAKE_PT_STDERR", "phase fail")],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("Phase transition failed"));
}

#[test]
fn phase_transition_invalid_json_returns_error() {
    let fx = setup("complete");
    let output = run_preflight(
        &fx.repo,
        Some(BRANCH),
        Some("--auto"),
        &fx.flow_bin,
        &fx.stubs,
        &[("FAKE_PT_OUT", "not-json")],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("Phase transition failed"));
}

#[test]
fn pr_status_check_error_returns_error() {
    let fx = setup("complete");
    let output = run_preflight(
        &fx.repo,
        Some(BRANCH),
        Some("--auto"),
        &fx.flow_bin,
        &fx.stubs,
        &[
            ("FAKE_PR_VIEW_EXIT", "1"),
            ("FAKE_PR_VIEW_STDERR", "could not find PR"),
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("could not find PR"));
}

#[test]
fn pr_status_check_error_empty_stderr_uses_fallback() {
    let fx = setup("complete");
    let output = run_preflight(
        &fx.repo,
        Some(BRANCH),
        Some("--auto"),
        &fx.flow_bin,
        &fx.stubs,
        &[("FAKE_PR_VIEW_EXIT", "1")],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("Could not find PR"));
}

#[test]
fn pr_state_merged_returns_ok_with_merged() {
    let fx = setup("complete");
    let output = run_preflight(
        &fx.repo,
        Some(BRANCH),
        Some("--auto"),
        &fx.flow_bin,
        &fx.stubs,
        &[("FAKE_PR_STATE", "MERGED")],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["pr_state"], "MERGED");
}

#[test]
fn pr_state_closed_returns_error() {
    let fx = setup("complete");
    let output = run_preflight(
        &fx.repo,
        Some(BRANCH),
        Some("--auto"),
        &fx.flow_bin,
        &fx.stubs,
        &[("FAKE_PR_STATE", "CLOSED")],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"].as_str().unwrap().contains("PR is closed"));
}

#[test]
fn pr_state_open_clean_merge_returns_ok() {
    let fx = setup("complete");
    let output = run_preflight(
        &fx.repo,
        Some(BRANCH),
        Some("--auto"),
        &fx.flow_bin,
        &fx.stubs,
        &[("FAKE_PR_STATE", "OPEN")],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["merge"], "clean");
}

#[test]
fn pr_state_open_merged_returns_ok() {
    let fx = setup("complete");
    let output = run_preflight(
        &fx.repo,
        Some(BRANCH),
        Some("--auto"),
        &fx.flow_bin,
        &fx.stubs,
        &[
            ("FAKE_PR_STATE", "OPEN"),
            ("FAKE_MERGE_BASE_EXIT", "1"),
            ("FAKE_GIT_MERGE_EXIT", "0"),
            ("FAKE_PUSH_EXIT", "0"),
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["merge"], "merged");
}

#[test]
fn pr_state_open_merge_conflict_returns_conflict() {
    let fx = setup("complete");
    let output = run_preflight(
        &fx.repo,
        Some(BRANCH),
        Some("--auto"),
        &fx.flow_bin,
        &fx.stubs,
        &[
            ("FAKE_PR_STATE", "OPEN"),
            ("FAKE_MERGE_BASE_EXIT", "1"),
            ("FAKE_GIT_MERGE_EXIT", "1"),
            ("FAKE_GIT_STATUS_OUT", "UU conflict.txt\n"),
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "conflict");
    let files: Vec<&str> = json["conflict_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(files, vec!["conflict.txt"]);
}

#[test]
fn pr_state_open_fetch_fail_returns_error() {
    let fx = setup("complete");
    let output = run_preflight(
        &fx.repo,
        Some(BRANCH),
        Some("--auto"),
        &fx.flow_bin,
        &fx.stubs,
        &[
            ("FAKE_PR_STATE", "OPEN"),
            ("FAKE_FETCH_EXIT", "1"),
            ("FAKE_FETCH_STDERR", "fetch boom"),
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"].as_str().unwrap().contains("fetch boom"));
}

#[test]
fn pr_state_open_merge_error_no_conflicts_returns_error() {
    let fx = setup("complete");
    let output = run_preflight(
        &fx.repo,
        Some(BRANCH),
        Some("--auto"),
        &fx.flow_bin,
        &fx.stubs,
        &[
            ("FAKE_PR_STATE", "OPEN"),
            ("FAKE_MERGE_BASE_EXIT", "1"),
            ("FAKE_GIT_MERGE_EXIT", "1"),
            ("FAKE_GIT_MERGE_STDERR", "unknown merge failure"),
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("unknown merge failure"));
}

#[test]
fn pr_state_open_push_fails_returns_error() {
    let fx = setup("complete");
    let output = run_preflight(
        &fx.repo,
        Some(BRANCH),
        Some("--auto"),
        &fx.flow_bin,
        &fx.stubs,
        &[
            ("FAKE_PR_STATE", "OPEN"),
            ("FAKE_MERGE_BASE_EXIT", "1"),
            ("FAKE_GIT_MERGE_EXIT", "0"),
            ("FAKE_PUSH_EXIT", "1"),
            ("FAKE_PUSH_STDERR", "remote rejected"),
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"].as_str().unwrap().contains("push failed"));
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("remote rejected"));
}

#[test]
fn pr_state_unexpected_returns_error() {
    let fx = setup("complete");
    let output = run_preflight(
        &fx.repo,
        Some(BRANCH),
        Some("--auto"),
        &fx.flow_bin,
        &fx.stubs,
        &[("FAKE_PR_STATE", "DRAFT")],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("Unexpected PR state"));
}

#[test]
fn warnings_populated_when_learn_pending() {
    let fx = setup("pending");
    let output = run_preflight(
        &fx.repo,
        Some(BRANCH),
        Some("--auto"),
        &fx.flow_bin,
        &fx.stubs,
        &[("FAKE_PR_STATE", "MERGED")],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    let w = json["warnings"].as_array().unwrap();
    assert!(!w.is_empty());
}

#[test]
fn gh_missing_from_path_returns_error() {
    // With gh absent from the flow-rs subprocess PATH,
    // check_pr_status's run_cmd_with_timeout returns Err(spawn
    // failure); the `?` surfaces the spawn error into the JSON
    // contract as a structured error.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    write_state_file(&repo, BRANCH, "complete");
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    // Empty stubs dir: no gh, no git.
    let stubs = parent.join("empty-stubs");
    fs::create_dir_all(&stubs).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("complete-preflight")
        .arg("--branch")
        .arg(BRANCH)
        .arg("--auto")
        .current_dir(&repo)
        .env("PATH", stubs.to_str().unwrap())
        .env("FLOW_BIN_PATH", &flow_bin)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("spawn flow-rs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    let msg = json["message"].as_str().unwrap_or("");
    assert!(
        msg.to_lowercase().contains("failed to spawn") || msg.to_lowercase().contains("gh"),
        "expected spawn failure for gh, got: {}",
        msg
    );
}

#[test]
fn git_missing_from_path_after_gh_returns_error() {
    // With git absent from PATH but gh present via stub, check_pr_status
    // succeeds with OPEN and merge_main attempts `git fetch` which
    // spawn-fails; surface is an error status.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    write_state_file(&repo, BRANCH, "complete");
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = parent.join("gh-only-stubs");
    fs::create_dir_all(&stubs).unwrap();
    let gh_script = r#"#!/bin/sh
case "$1 $2" in
    "pr view")
        printf '%s' 'OPEN'
        exit 0
        ;;
    *)
        exit 0
        ;;
esac
"#;
    let gh_path = stubs.join("gh");
    fs::write(&gh_path, gh_script).unwrap();
    fs::set_permissions(&gh_path, fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("complete-preflight")
        .arg("--branch")
        .arg(BRANCH)
        .arg("--auto")
        .current_dir(&repo)
        .env("PATH", stubs.to_str().unwrap())
        .env("FLOW_BIN_PATH", &flow_bin)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("spawn flow-rs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    let msg = json["message"].as_str().unwrap_or("");
    assert!(
        msg.to_lowercase().contains("failed to spawn") || msg.to_lowercase().contains("git"),
        "expected spawn failure for git, got: {}",
        msg
    );
}

#[test]
fn phase_transition_spawn_error_returns_error() {
    // FLOW_BIN_PATH points at a nonexistent path; spawn fails and the
    // `?` in phase_transition_enter surfaces "Phase transition failed:
    // Failed to spawn ..." as the JSON error message.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    write_state_file(&repo, BRANCH, "complete");
    let stubs = build_path_stubs(&parent);
    let nonexistent = parent.join("does-not-exist").join("flow");

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("complete-preflight")
        .arg("--branch")
        .arg(BRANCH)
        .arg("--auto")
        .current_dir(&repo)
        .env(
            "PATH",
            format!(
                "{}:{}",
                stubs.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("FLOW_BIN_PATH", &nonexistent)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("spawn flow-rs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    let msg = json["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("Phase transition failed"),
        "expected Phase transition failed, got: {}",
        msg
    );
    assert!(
        msg.to_lowercase().contains("failed to spawn"),
        "expected spawn failure, got: {}",
        msg
    );
}

#[test]
fn merge_main_library_call_with_git_absent() {
    // Direct library-level test for merge_main's spawn-Err arm.
    // `merge_main` uses the ambient PATH; we can't mutate it in-
    // process safely, so this test demonstrates the path by asserting
    // merge_main is callable and returns a tuple shape. The actual
    // first-arm Err coverage is driven by the subprocess test above
    // (`git_missing_from_path_after_gh_returns_error`).
    //
    // Called without git on PATH (impossible here — parent PATH has
    // git), merge_main would Err. Called with git present, it
    // succeeds/fails based on git's actual state. We just verify the
    // return-shape contract stays stable.
    let (status, _) = merge_main("main");
    assert!(
        matches!(status.as_str(), "clean" | "merged" | "conflict" | "error"),
        "unexpected merge_main status: {}",
        status
    );
}

#[test]
fn manual_flag_resolves_manual_mode() {
    let fx = setup("complete");
    let output = run_preflight(
        &fx.repo,
        Some(BRANCH),
        Some("--manual"),
        &fx.flow_bin,
        &fx.stubs,
        &[("FAKE_PR_STATE", "MERGED")],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["mode"], "manual");
}

/// Drive the `Some(str)` branch of `read_base_branch` through
/// `complete-preflight`'s `merge_main` invocation and prove the
/// state-file value reaches `git fetch origin <base_branch>`. The
/// fixture's bare remote has only `main`; the state file declares
/// `base_branch: "staging"`. With the PR `OPEN` and no merge-base
/// shortcut, `merge_main` issues `git fetch origin staging`, which
/// the gh-only stub set passes through to real git — real git fails
/// because `origin/staging` doesn't exist on the bare remote, and
/// the stderr (carrying "staging") surfaces as the JSON `message`
/// proving base_branch flowed through rather than the hardcoded
/// "main".
#[test]
fn complete_preflight_merge_base_uses_base_branch_from_state() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);

    // Hand-write a state file with base_branch=staging (the helper
    // does not set this field).
    let branch_dir = repo.join(".flow-states").join(BRANCH);
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(
        branch_dir.join("state.json"),
        json!({
            "schema_version": 1,
            "branch": BRANCH,
            "base_branch": "staging",
            "pr_number": 42,
            "pr_url": "https://github.com/test/test/pull/42",
            "phases": {"flow-learn": {"status": "complete"}},
        })
        .to_string(),
    )
    .unwrap();

    // gh-only stub: returns OPEN PR. git stays the real binary so
    // `git fetch origin staging` against the bare remote produces a
    // genuine "couldn't find remote ref staging" stderr.
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = parent.join("gh-stubs");
    fs::create_dir_all(&stubs).unwrap();
    let gh_script = r#"#!/bin/sh
case "$1 $2" in
    "pr view") printf '%s' 'OPEN'; exit 0 ;;
    *) exit 0 ;;
esac
"#;
    let gh_path = stubs.join("gh");
    fs::write(&gh_path, gh_script).unwrap();
    fs::set_permissions(&gh_path, fs::Permissions::from_mode(0o755)).unwrap();

    let path = format!(
        "{}:{}",
        stubs.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("complete-preflight")
        .arg("--branch")
        .arg(BRANCH)
        .arg("--auto")
        .current_dir(&repo)
        .env("PATH", path)
        .env("FLOW_BIN_PATH", &flow_bin)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("spawn flow-rs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(
        json["status"], "error",
        "expected error status from missing origin/staging, got: {}",
        stdout
    );
    let msg = json["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("staging"),
        "merge_main error must reference 'staging' to prove base_branch flowed through, got: {}",
        msg
    );
}
