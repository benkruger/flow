//! Integration tests for `bin/flow close-issue` (`src/close_issue.rs`).
//!
//! The module shells out to `gh issue close` and, when no `--repo` is
//! provided, to `git remote -v` via `detect_repo`. Tests install a
//! mock `gh` on PATH via `common::create_gh_stub` so the subprocess
//! paths are exercised without network access.

mod common;

use std::path::Path;
use std::process::{Command, Output};

use common::{create_gh_stub, create_git_repo_with_remote, parse_output};

fn run_close_issue(repo: &Path, args: &[&str], stub_dir: &Path) -> Output {
    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("close-issue")
        .args(args)
        .current_dir(repo)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

#[test]
fn close_issue_happy_path_with_repo_flag() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // gh exits 0 on any invocation.
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_close_issue(
        &repo,
        &["--repo", "owner/name", "--number", "42"],
        &stub_dir,
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
}

#[test]
fn close_issue_gh_failure_returns_stderr_message() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // gh exits 1 with a stderr error message.
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho 'issue not found' >&2\nexit 1\n");

    let output = run_close_issue(
        &repo,
        &["--repo", "owner/name", "--number", "999"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .contains("issue not found"),
        "Expected stderr in message, got: {}",
        data["message"]
    );
}

#[test]
fn close_issue_gh_failure_falls_back_to_stdout_when_stderr_empty() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // gh exits 1 with message on stdout only.
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho 'problem on stdout'\nexit 1\n");

    let output = run_close_issue(&repo, &["--repo", "owner/name", "--number", "7"], &stub_dir);

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .contains("problem on stdout"),
        "Expected stdout in message, got: {}",
        data["message"]
    );
}

#[test]
fn close_issue_gh_failure_with_no_output_returns_unknown_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // gh exits 1 with nothing on either stream.
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 1\n");

    let output = run_close_issue(&repo, &["--repo", "owner/name", "--number", "1"], &stub_dir);

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["message"], "Unknown error");
}

#[test]
fn close_issue_detects_repo_when_flag_omitted() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // `detect_repo` requires a github.com-style URL. Override the fake
    // remote that the helper sets up.
    Command::new("git")
        .args([
            "remote",
            "set-url",
            "origin",
            "git@github.com:owner/name.git",
        ])
        .current_dir(&repo)
        .output()
        .unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_close_issue(&repo, &["--number", "42"], &stub_dir);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
}

#[test]
fn close_issue_exits_when_repo_undetectable_and_no_flag() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // The helper's origin is a local `bare.git` path, which does NOT
    // match the github.com pattern `detect_repo` requires. No --repo
    // flag means `detect_repo_or_fail` must error-exit.
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_close_issue(&repo, &["--number", "42"], &stub_dir);

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .contains("Could not detect repo"),
        "Expected repo-detection error, got: {}",
        data["message"]
    );
}

#[test]
fn close_issue_gh_spawn_failure_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // Install no gh stub — running with an empty PATH makes the
    // spawn of `gh` fail, exercising the spawn-error path.
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("close-issue")
        .args(["--repo", "owner/name", "--number", "1"])
        .current_dir(&repo)
        .env("PATH", "")
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains("spawn"),
        "Expected spawn-related error, got: {}",
        data["message"]
    );
}
