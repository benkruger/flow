//! Subprocess integration tests for `bin/flow complete-fast`.
//!
//! Covers the CLI entry (`run`) and the `run_impl` 3-line wrapper
//! that calls `project_root()`. The inline tests in
//! `src/complete_fast.rs::tests` cover every branch of
//! `run_impl_inner`, `fast_inner`, and `production_ci_decider_inner`
//! with mock runners; these subprocess tests prove the CLI entry
//! dispatches to `run_impl_inner` via `run_impl` end-to-end.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

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

fn write_state_file(repo: &Path, branch: &str, learn_status: &str) {
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state = common::make_complete_state(branch, learn_status, None);
    let state_path = state_dir.join(format!("{}.json", branch));
    fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
}

fn run_complete_fast(repo: &Path, branch_arg: Option<&str>) -> (i32, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.arg("complete-fast")
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

/// No state file at `.flow-states/<branch>.json` → `read_state`
/// returns `Err("No state file found")` → `run_impl_inner` propagates
/// via its Result return → `run` prints error JSON and exits 1.
/// Exercises the `Err(e)` arm of `run` that converts `run_impl`
/// errors into stdout error messages.
#[test]
fn fast_run_no_state_file_exits_1_with_error_json() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);

    let (code, stdout, _) = run_complete_fast(&repo, Some(BRANCH));

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(
        json["message"]
            .as_str()
            .unwrap_or("")
            .contains("No state file"),
        "expected No state file message; got: {}",
        json["message"]
    );
}

/// `--branch feature/foo` → `read_state` returns structured error
/// for the slash-containing branch (FlowPaths::try_new returns None)
/// → `run_impl_inner` propagates via Result → `run` prints error JSON
/// and exits 1. Proves the slash-branch regression from PR #1054 /
/// PR #1137 is still guarded at the complete-fast CLI entry.
#[test]
fn fast_run_slash_branch_exits_1_structured_error() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);

    let (code, stdout, stderr) = run_complete_fast(&repo, Some("feature/foo"));

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(
        json["message"]
            .as_str()
            .unwrap_or("")
            .contains("not a valid FLOW branch"),
        "expected slash-branch structured error; got: {}",
        json["message"]
    );
    assert!(
        !stderr.contains("panicked at"),
        "slash branch must not panic; stderr={}",
        stderr
    );
}

/// No `--branch` override + no git branch resolvable → resolve_branch
/// returns `None` → `run_impl_inner` returns `Err("Could not
/// determine current branch")` → `run` exits 1 with error JSON.
#[test]
fn fast_run_invalid_branch_resolve_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    // Deliberately no git repo — complete-fast resolves branch via
    // git subprocess and gets None.
    let (code, stdout, _) = run_complete_fast(&parent, None);

    assert_eq!(code, 1);
    // Result may be either "Could not determine current branch"
    // (resolve_branch None) or a git-repo error propagated from
    // deeper; either way the process exits 1 with structured JSON.
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
}

/// Valid fixture with `flow-learn.status == "pending"` → the Learn
/// gate inside `run_impl_inner` returns `Ok(json!({"status":"error",
/// ...}))` → `run` dispatches to `run_impl_inner` via `run_impl`,
/// detects status==error, and exits 1. Proves the `run_impl` 3-line
/// wrapper dispatches to `run_impl_inner` end-to-end.
#[test]
fn fast_run_impl_dispatches_to_run_impl_inner() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    write_state_file(&repo, BRANCH, "pending");

    let (code, stdout, _) = run_complete_fast(&repo, Some(BRANCH));

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(
        json["message"]
            .as_str()
            .unwrap_or("")
            .contains("Phase 5: Learn"),
        "dispatch must reach run_impl_inner's Learn gate; got: {}",
        json["message"]
    );
}
