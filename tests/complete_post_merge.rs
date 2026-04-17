//! Subprocess integration tests for `bin/flow complete-post-merge`.
//!
//! Covers the CLI entry (`run`) and the `post_merge` production
//! wrapper that calls `project_root()`. The inline tests in
//! `src/complete_post_merge.rs::tests` cover `post_merge_inner`'s
//! branches via mock runners; these subprocess tests prove the
//! wrapper dispatches end-to-end and honors the best-effort
//! always-exit-0 contract.

use std::fs;
use std::os::unix::fs::PermissionsExt;
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
