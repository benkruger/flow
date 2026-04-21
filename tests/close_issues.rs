//! Integration tests for `bin/flow close-issues` (`src/close_issues.rs`).
//!
//! `close-issues` reads the FLOW state file, extracts `#N` issue
//! references from the prompt, and closes each via `gh issue close`.
//! Tests install a mock `gh` on PATH so subprocess paths are exercised
//! without network access.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};

use common::{create_gh_stub, create_git_repo_with_remote, parse_output};
use flow_rs::close_issues::{
    close_issues_with_runner, close_issues_with_runner_and_timeout, run_impl_main, Args,
};
use flow_rs::utils::extract_issue_numbers;
use serde_json::json;

fn run_close_issues(repo: &Path, state_file: &Path, stub_dir: &Path) -> Output {
    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("close-issues")
        .arg("--state-file")
        .arg(state_file)
        .current_dir(repo)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

#[test]
fn closes_all_issues_from_prompt_with_repo() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_file = dir.path().join("state.json");
    fs::write(
        &state_file,
        json!({
            "prompt": "fix #42 and #99",
            "repo": "owner/name",
        })
        .to_string(),
    )
    .unwrap();
    // gh always exits 0, so both issues "close" successfully.
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_close_issues(&repo, &state_file, &stub_dir);

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    let closed = data["closed"].as_array().unwrap();
    assert_eq!(closed.len(), 2);
    // URLs are included because repo is set.
    let numbers: Vec<i64> = closed
        .iter()
        .map(|v| v["number"].as_i64().unwrap())
        .collect();
    assert!(numbers.contains(&42));
    assert!(numbers.contains(&99));
    for entry in closed {
        let url = entry["url"].as_str().unwrap();
        assert!(url.starts_with("https://github.com/owner/name/issues/"));
    }
    assert!(data["failed"].as_array().unwrap().is_empty());
}

#[test]
fn closed_entries_omit_url_when_repo_absent() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_file = dir.path().join("state.json");
    fs::write(&state_file, json!({"prompt": "resolve #7"}).to_string()).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_close_issues(&repo, &state_file, &stub_dir);

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    let closed = data["closed"].as_array().unwrap();
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0]["number"], 7);
    assert!(
        closed[0].get("url").is_none(),
        "Expected no url key without repo, got: {}",
        closed[0]
    );
}

#[test]
fn partitions_success_and_failure() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_file = dir.path().join("state.json");
    fs::write(
        &state_file,
        json!({
            "prompt": "done #1 and #2",
            "repo": "owner/name",
        })
        .to_string(),
    )
    .unwrap();
    // gh succeeds for issue 1, fails for issue 2.
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         for arg in \"$@\"; do\n\
           if [ \"$arg\" = \"2\" ]; then\n\
             echo 'could not close' >&2\n\
             exit 1\n\
           fi\n\
         done\n\
         exit 0\n",
    );

    let output = run_close_issues(&repo, &state_file, &stub_dir);

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    let closed = data["closed"].as_array().unwrap();
    let failed = data["failed"].as_array().unwrap();
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0]["number"], 1);
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0]["number"], 2);
    assert!(
        failed[0]["error"]
            .as_str()
            .unwrap_or("")
            .contains("could not close"),
        "Expected error in failed entry, got: {}",
        failed[0]
    );
}

#[test]
fn empty_prompt_produces_empty_lists() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_file = dir.path().join("state.json");
    fs::write(
        &state_file,
        json!({"prompt": "nothing to close here"}).to_string(),
    )
    .unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_close_issues(&repo, &state_file, &stub_dir);

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert!(data["closed"].as_array().unwrap().is_empty());
    assert!(data["failed"].as_array().unwrap().is_empty());
}

#[test]
fn missing_prompt_key_treated_as_empty() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_file = dir.path().join("state.json");
    fs::write(&state_file, json!({"branch": "test"}).to_string()).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_close_issues(&repo, &state_file, &stub_dir);

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert!(data["closed"].as_array().unwrap().is_empty());
    assert!(data["failed"].as_array().unwrap().is_empty());
}

#[test]
fn missing_state_file_exits_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let missing = dir.path().join("nonexistent.json");
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_close_issues(&repo, &missing, &stub_dir);

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Could not read state file"));
}

#[test]
fn malformed_state_file_exits_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_file = dir.path().join("state.json");
    fs::write(&state_file, "this is not json").unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_close_issues(&repo, &state_file, &stub_dir);

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Could not read state file"));
}

#[test]
fn gh_spawn_failure_records_as_failed() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_file = dir.path().join("state.json");
    fs::write(
        &state_file,
        json!({"prompt": "fix #5", "repo": "owner/name"}).to_string(),
    )
    .unwrap();
    // No gh stub; empty PATH makes `gh` spawn fail, which exercises
    // the spawn-error branch in close_single_issue.
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("close-issues")
        .arg("--state-file")
        .arg(&state_file)
        .current_dir(&repo)
        .env("PATH", "")
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    let failed = data["failed"].as_array().unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0]["number"], 5);
    assert!(
        failed[0]["error"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains("spawn"),
        "Expected spawn error, got: {}",
        failed[0]["error"]
    );
}

// --- Library-level unit tests (migrated from src/close_issues.rs) ---

// --- CLI integration: run() reads state file ---

#[test]
fn extract_issue_numbers_empty_prompt_returns_empty() {
    let issue_numbers = extract_issue_numbers("");
    assert!(issue_numbers.is_empty());
}

#[test]
fn close_issues_with_runner_empty_list_is_noop() {
    let factory = |_args: &[&str]| {
        Command::new("sh")
            .args(["-c", "exit 0"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    };
    let (closed, failed) = close_issues_with_runner(&[], None, &factory);
    assert!(closed.is_empty());
    assert!(failed.is_empty());
}

// --- close_issues_with_runner ---

#[test]
fn close_issues_with_runner_all_succeed() {
    let factory = |_args: &[&str]| {
        Command::new("sh")
            .args(["-c", "exit 0"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    };
    let (closed, failed) = close_issues_with_runner(&[1, 2], Some("owner/repo"), &factory);
    assert_eq!(closed.len(), 2);
    assert!(failed.is_empty());
    assert_eq!(closed[0]["number"], 1);
    assert!(closed[0]["url"]
        .as_str()
        .unwrap()
        .contains("owner/repo/issues/1"));
}

#[test]
fn close_issues_with_runner_partial_failure() {
    let factory = |args: &[&str]| {
        // First arg after "issue close" is the number; "1" succeeds, "2" fails.
        let num = args[2];
        let cmd = if num == "1" {
            "exit 0"
        } else {
            "echo nope 1>&2; exit 1"
        };
        Command::new("sh")
            .args(["-c", cmd])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    };
    let (closed, failed) = close_issues_with_runner(&[1, 2], None, &factory);
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0]["number"], 1);
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0]["number"], 2);
    assert!(failed[0]["error"].as_str().unwrap().contains("nope"));
}

// --- run_impl_main ---

#[test]
fn close_issues_run_impl_main_no_state_returns_error_tuple() {
    let args = Args {
        state_file: "/nonexistent/state.json".to_string(),
    };
    let (value, code) = run_impl_main(args);
    assert_eq!(value["status"], "error");
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Could not read state file"));
}

#[test]
fn close_issues_run_impl_main_corrupt_state_returns_error_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = dir.path().join("state.json");
    fs::write(&state_file, "{not json").unwrap();
    let args = Args {
        state_file: state_file.to_string_lossy().to_string(),
    };
    let (value, code) = run_impl_main(args);
    assert_eq!(value["status"], "error");
    assert_eq!(code, 1);
}

#[test]
fn close_issues_run_impl_main_no_prompt_returns_empty_lists() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = dir.path().join("state.json");
    fs::write(&state_file, r#"{"branch":"test"}"#).unwrap();
    let args = Args {
        state_file: state_file.to_string_lossy().to_string(),
    };
    let (value, code) = run_impl_main(args);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["closed"].as_array().unwrap().len(), 0);
    assert_eq!(value["failed"].as_array().unwrap().len(), 0);
}

/// Exercises the `close_single_issue` polling timeout:
/// fires when elapsed >= timeout. The `_with_timeout` seam exposes
/// the threshold so tests pass `0`; the first poll trips immediately
/// even though the child is still running.
#[test]
fn close_issues_with_runner_and_timeout_zero_marks_issue_failed() {
    let factory = |_args: &[&str]| {
        Command::new("sh")
            .args(["-c", "sleep 60"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    };
    let (closed, failed) = close_issues_with_runner_and_timeout(&[42], None, 0, &factory);
    assert!(closed.is_empty());
    assert_eq!(failed.len(), 1);
    assert!(
        failed[0]["error"].as_str().unwrap().contains("timeout"),
        "got: {:?}",
        failed[0]
    );
}

#[test]
fn close_issues_with_runner_spawn_failure_returns_failed_entry() {
    // Drives the spawn-failure branch: factory returns Err → close_single_issue
    // returns Err("Failed to spawn: ...") → entry lands in `failed`.
    let factory = |_args: &[&str]| -> std::io::Result<Child> {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such binary",
        ))
    };
    let (closed, failed) = close_issues_with_runner(&[42], None, &factory);
    assert!(closed.is_empty());
    assert_eq!(failed.len(), 1);
    assert!(failed[0]["error"]
        .as_str()
        .unwrap()
        .contains("Failed to spawn"));
}

// Direct `run_impl_main_with_runner` test removed — the seam is now
// private. Dispatch behavior is exercised via subprocess tests that
// spawn `bin/flow close-issues` with a `gh` stub on PATH.
