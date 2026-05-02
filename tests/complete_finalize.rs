//! Subprocess integration tests for `bin/flow complete-finalize`.
//!
//! post_merge_inner and run_impl_with_deps seams were removed; the
//! module now runs post-merge and cleanup inline. Tests drive the
//! public `run_impl` via the compiled binary with fixtures that
//! control bin/flow stub behavior (so post_merge's failures map is
//! populated on broken subprocesses) and `.flow-states/` layout (so
//! the log-closure existence branch flips).

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

mod common;

const BRANCH: &str = "test-feature";
const SLASH_BRANCH: &str = "feature/foo";

fn make_repo_fixture(parent: &Path) -> PathBuf {
    let repo = common::create_git_repo_with_remote(parent);
    repo.canonicalize().expect("canonicalize repo")
}

fn write_state_file(repo: &Path, branch: &str, create_flow_states_dir: bool) -> PathBuf {
    let branch_dir = repo.join(".flow-states").join(branch);
    let state_path = branch_dir.join("state.json");
    if create_flow_states_dir {
        fs::create_dir_all(&branch_dir).unwrap();
        let state = common::make_complete_state(branch, "complete", None);
        fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    }
    state_path
}

/// bin/flow stub that returns valid JSON for complete-finalize's
/// downstream subcommands (phase-transition, render-pr-body, etc.)
/// so post_merge does not accumulate failures. Used for happy-path
/// subprocess tests.
fn write_success_flow_stub(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let script = r#"#!/bin/sh
case "$1" in
    phase-transition)
        printf '%s' '{"status":"ok","formatted_time":"1m","cumulative_seconds":60}'
        ;;
    render-pr-body|label-issues|add-notification)
        ;;
    format-issues-summary)
        printf '%s' '{"status":"ok","has_issues":false}'
        ;;
    close-issues)
        printf '%s' '{"status":"ok","closed":[],"failed":[]}'
        ;;
    format-complete-summary)
        printf '%s' '{"status":"ok","summary":"done","issues_links":""}'
        ;;
    auto-close-parent)
        printf '%s' '{"status":"ok","parent_closed":false,"milestone_closed":false}'
        ;;
    notify-slack)
        printf '%s' '{"status":"ok","ts":"1234.5678"}'
        ;;
    *)
        ;;
esac
exit 0
"#;
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn path_stub_dir(parent: &Path) -> PathBuf {
    let stubs = parent.join("stubs");
    fs::create_dir_all(&stubs).unwrap();
    let gh = stubs.join("gh");
    fs::write(&gh, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
    stubs
}

#[allow(clippy::too_many_arguments)]
fn run_complete_finalize(
    repo: &Path,
    pr: &str,
    state_file: &str,
    branch: &str,
    worktree: &str,
    pull: bool,
    flow_bin_path: Option<&Path>,
    path_stubs: Option<&Path>,
) -> (i32, String, String) {
    let current_path = std::env::var("PATH").unwrap_or_default();
    let new_path = if let Some(stubs) = path_stubs {
        format!("{}:{}", stubs.display(), current_path)
    } else {
        current_path
    };
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.args([
        "complete-finalize",
        "--pr",
        pr,
        "--state-file",
        state_file,
        "--branch",
        branch,
        "--worktree",
        worktree,
    ])
    .current_dir(repo)
    .env("PATH", new_path)
    .env_remove("FLOW_CI_RUNNING");
    if let Some(p) = flow_bin_path {
        cmd.env("FLOW_BIN_PATH", p);
    }
    if pull {
        cmd.arg("--pull");
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
fn finalize_happy_path_no_failures() {
    // Happy path: bin/flow stub returns valid JSON for every subcommand
    // so post_merge's failures map stays empty → post_merge_failures
    // field absent on the outer result.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = write_state_file(&repo, BRANCH, true);
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, stdout, _) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        false,
        Some(&flow_bin),
        Some(&stubs),
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["formatted_time"], "1m");
    assert_eq!(json["cumulative_seconds"], 60);
    assert_eq!(json["summary"], "done");
    assert!(json.get("post_merge_failures").is_none());
    assert!(json.get("cleanup").is_some());
}

#[test]
fn finalize_with_broken_flow_stubs_populates_post_merge_failures() {
    // No FLOW_BIN_PATH / PATH stubs → every subcommand spawn or call
    // fails → post_merge records entries in its `failures` map →
    // outer result carries `post_merge_failures` with at least one key.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = write_state_file(&repo, BRANCH, true);
    let nonexistent = parent.join("does-not-exist").join("flow");

    let (code, stdout, _) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        false,
        Some(&nonexistent),
        None,
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    let failures = json
        .get("post_merge_failures")
        .and_then(|v| v.as_object())
        .expect("post_merge_failures must be populated when subprocesses fail");
    assert!(
        !failures.is_empty(),
        "failures map should have at least one key; got: {:?}",
        failures
    );
}

#[test]
fn finalize_log_closure_writes_when_flow_states_dir_exists() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = write_state_file(&repo, BRANCH, true);
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, _, _) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        false,
        Some(&flow_bin),
        Some(&stubs),
    );

    assert_eq!(code, 0);
    let log_path = repo.join(".flow-states").join(BRANCH).join("log");
    assert!(
        log_path.exists(),
        "log closure must write to {} when .flow-states/ exists",
        log_path.display()
    );
    let log_content = fs::read_to_string(&log_path).unwrap_or_default();
    assert!(
        log_content.contains("complete-finalize"),
        "log must contain complete-finalize entries; got: {}",
        log_content
    );
}

#[test]
fn finalize_log_closure_skips_when_flow_states_dir_missing() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    // State file outside .flow-states/; directory is NOT created.
    let state_path = repo.join("external-state.json");
    let state = common::make_complete_state(BRANCH, "complete", None);
    fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, _, _) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        false,
        Some(&flow_bin),
        Some(&stubs),
    );

    assert_eq!(code, 0);
    // complete-finalize's log file should NOT exist. complete_post_merge
    // may have created .flow-states/ when writing its own artifacts, but
    // the log FILE is the specific assertion.
    let log_path = repo.join(".flow-states").join(BRANCH).join("log");
    assert!(
        !log_path.exists(),
        "log closure must skip logging when .flow-states/ is missing at entry; found: {}",
        log_path.display()
    );
}

#[test]
fn finalize_slash_branch_does_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = repo.join("external-state.json");
    let state = common::make_complete_state(SLASH_BRANCH, "complete", None);
    fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, _, stderr) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        SLASH_BRANCH,
        ".worktrees/feature-foo",
        false,
        Some(&flow_bin),
        Some(&stubs),
    );

    assert_eq!(code, 0);
    assert!(
        !stderr.contains("panicked at"),
        "slash branch triggered a Rust panic: stderr={}",
        stderr
    );
}

#[test]
fn finalize_pull_flag_threads_to_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = write_state_file(&repo, BRANCH, true);
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, stdout, _) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        true,
        Some(&flow_bin),
        Some(&stubs),
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    let cleanup = json
        .get("cleanup")
        .and_then(|v| v.as_object())
        .expect("cleanup map must be present");
    let _ = cleanup;
}

#[test]
fn finalize_has_failures_ok_status_absent_failures() {
    // post_merge returns no failures → post_merge_failures absent →
    // effective_status == "ok" on the log line. Drive through the
    // success stub.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = write_state_file(&repo, BRANCH, true);
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, stdout, _) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        false,
        Some(&flow_bin),
        Some(&stubs),
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    assert!(json.get("post_merge_failures").is_none());
}

#[test]
fn finalize_result_includes_empty_banner_and_issues_links_on_bare_state() {
    // The state file omits slack thread and has no prompt → various
    // fields in post_merge_data default to "" → outer result mirrors.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join(format!("{}.json", BRANCH));
    // Minimal state with only branch/pr_number.
    fs::write(
        &state_path,
        json!({"branch": BRANCH, "pr_number": 42}).to_string(),
    )
    .unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, stdout, _) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        false,
        Some(&flow_bin),
        Some(&stubs),
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    assert!(json.get("issues_links").is_some());
    assert!(json.get("banner_line").is_some());
}
