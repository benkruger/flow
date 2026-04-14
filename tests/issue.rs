//! Integration tests for `bin/flow issue`.
//!
//! The command wraps `gh issue create` with label-retry logic, body
//! file handling, repo detection fallbacks, and a Code Review filing
//! ban. Tests install a mock `gh` on PATH and state-file fixtures to
//! cover every branch.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_gh_stub, create_git_repo_with_remote, parse_output};
use serde_json::json;

fn run_cmd(repo: &Path, args: &[&str], stub_dir: &Path) -> Output {
    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("issue")
        .args(args)
        .current_dir(repo)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

#[test]
fn issue_create_happy_path_with_repo_flag() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\necho 'https://github.com/owner/name/issues/42'\nexit 0\n",
    );

    let output = run_cmd(
        &repo,
        &["--repo", "owner/name", "--title", "Test issue"],
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
    assert_eq!(data["url"], "https://github.com/owner/name/issues/42");
    assert_eq!(data["number"], 42);
}

#[test]
fn issue_create_with_label_success() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\necho 'https://github.com/o/r/issues/5'\nexit 0\n",
    );

    let output = run_cmd(
        &repo,
        &["--repo", "o/r", "--title", "Labeled", "--label", "bug"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["number"], 5);
}

#[test]
fn issue_create_label_not_found_retries_with_create() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // First issue-create call fails with "label not found".
    // Then `gh label create` succeeds.
    // Second issue-create call (retry with label) succeeds.
    let counter = dir.path().join(".counter");
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\n\
             COUNTER=\"{}\"\n\
             if [ ! -f \"$COUNTER\" ]; then echo 0 > \"$COUNTER\"; fi\n\
             if [ \"$1\" = \"label\" ] && [ \"$2\" = \"create\" ]; then\n\
               exit 0\n\
             fi\n\
             N=$(cat \"$COUNTER\")\n\
             N=$((N + 1))\n\
             echo $N > \"$COUNTER\"\n\
             if [ \"$N\" -eq 1 ]; then\n\
               echo 'could not add label: label not found' >&2\n\
               exit 1\n\
             fi\n\
             echo 'https://github.com/o/r/issues/10'\n\
             exit 0\n",
            counter.display()
        ),
    );

    let output = run_cmd(
        &repo,
        &["--repo", "o/r", "--title", "T", "--label", "new-label"],
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
    assert_eq!(data["number"], 10);
}

#[test]
fn issue_create_label_not_found_and_create_fails_retries_without_label() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // First issue-create fails with "label not found"
    // Then `gh label create` ALSO fails
    // Then second issue-create (without label) succeeds
    let counter = dir.path().join(".counter");
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\n\
             COUNTER=\"{}\"\n\
             if [ ! -f \"$COUNTER\" ]; then echo 0 > \"$COUNTER\"; fi\n\
             if [ \"$1\" = \"label\" ] && [ \"$2\" = \"create\" ]; then\n\
               exit 1\n\
             fi\n\
             N=$(cat \"$COUNTER\")\n\
             N=$((N + 1))\n\
             echo $N > \"$COUNTER\"\n\
             if [ \"$N\" -eq 1 ]; then\n\
               echo 'label not found' >&2\n\
               exit 1\n\
             fi\n\
             echo 'https://github.com/o/r/issues/11'\n\
             exit 0\n",
            counter.display()
        ),
    );

    let output = run_cmd(
        &repo,
        &["--repo", "o/r", "--title", "T", "--label", "untouchable"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["number"], 11);
}

#[test]
fn issue_create_gh_failure_unrelated_to_label_propagates() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho 'some other error' >&2\nexit 1\n");

    let output = run_cmd(&repo, &["--repo", "o/r", "--title", "T"], &stub_dir);

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("some other error"));
}

#[test]
fn issue_create_with_body_file_reads_and_deletes_it() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let body_file = repo.join(".flow-issue-body");
    fs::write(&body_file, "Issue body text").unwrap();

    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\necho 'https://github.com/o/r/issues/7'\nexit 0\n",
    );

    let output = run_cmd(
        &repo,
        &[
            "--repo",
            "o/r",
            "--title",
            "T",
            "--body-file",
            body_file.to_str().unwrap(),
        ],
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
    // body file should be deleted after reading
    assert!(!body_file.exists(), "body file should be consumed");
}

#[test]
fn issue_create_missing_body_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");
    let missing = repo.join(".nonexistent-body");

    let output = run_cmd(
        &repo,
        &[
            "--repo",
            "o/r",
            "--title",
            "T",
            "--body-file",
            missing.to_str().unwrap(),
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Could not read body file"));
}

#[test]
fn issue_create_resolves_repo_from_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_file = dir.path().join("state.json");
    fs::write(
        &state_file,
        json!({"repo": "state-owner/state-name"}).to_string(),
    )
    .unwrap();
    // gh prints the URL; the stub accepts any --repo.
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\necho 'https://github.com/state-owner/state-name/issues/1'\nexit 0\n",
    );

    let output = run_cmd(
        &repo,
        &["--title", "T", "--state-file", state_file.to_str().unwrap()],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert!(data["url"]
        .as_str()
        .unwrap()
        .starts_with("https://github.com/state-owner/state-name/issues/"));
}

#[test]
fn issue_create_no_repo_and_no_detection_exits_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // Helper sets a local `bare.git` remote which detect_repo rejects
    // (requires github.com URL). No --repo → error exit.
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_cmd(&repo, &["--title", "T"], &stub_dir);

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Could not detect repo"));
}
