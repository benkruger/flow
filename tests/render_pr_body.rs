//! Integration tests for `bin/flow render-pr-body`.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_gh_stub, create_git_repo_with_remote, parse_output};
use serde_json::{json, Value};

fn write_state(repo: &Path, name: &str, state: &Value) -> std::path::PathBuf {
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let path = state_dir.join(format!("{}.json", name));
    fs::write(&path, serde_json::to_string_pretty(state).unwrap()).unwrap();
    path
}

fn run_render(repo: &Path, args: &[&str], stub_dir: &Path) -> Output {
    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("render-pr-body")
        .args(args)
        .current_dir(repo)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

fn minimal_complete_state(feature: &str) -> Value {
    json!({
        "schema_version": 1,
        "branch": "test-branch",
        "feature": feature,
        "prompt": feature,
        "pr_number": 42,
        "pr_url": "https://github.com/o/r/pull/42",
        "phases": {
            "flow-start":        {"status": "complete", "cumulative_seconds": 10, "visit_count": 1},
            "flow-plan":         {"status": "complete", "cumulative_seconds": 20, "visit_count": 1},
            "flow-code":         {"status": "complete", "cumulative_seconds": 30, "visit_count": 1},
            "flow-code-review":  {"status": "complete", "cumulative_seconds": 40, "visit_count": 1},
            "flow-learn":        {"status": "complete", "cumulative_seconds": 50, "visit_count": 1},
            "flow-complete":     {"status": "pending"}
        },
        "findings": [],
        "issues_filed": [],
        "notes": [],
    })
}

#[test]
fn render_pr_body_dry_run_returns_sections() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = minimal_complete_state("Test feature");
    let state_path = write_state(&repo, "test-branch", &state);

    // Even dry-run may call gh internally elsewhere; stub exits 0.
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_render(
        &repo,
        &[
            "--pr",
            "42",
            "--state-file",
            state_path.to_str().unwrap(),
            "--dry-run",
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
    let sections = data["sections"].as_array().unwrap();
    assert!(!sections.is_empty(), "Expected section headers, got empty");
}

#[test]
fn render_pr_body_missing_state_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");
    let missing = dir.path().join("no-such.json");

    let output = run_render(
        &repo,
        &[
            "--pr",
            "42",
            "--state-file",
            missing.to_str().unwrap(),
            "--dry-run",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("State file not found"));
}

#[test]
fn render_pr_body_malformed_state_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let path = state_dir.join("bad.json");
    fs::write(&path, "not json").unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_render(
        &repo,
        &[
            "--pr",
            "42",
            "--state-file",
            path.to_str().unwrap(),
            "--dry-run",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
}

#[test]
fn render_pr_body_non_dry_run_calls_gh_edit() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = minimal_complete_state("Live render");
    let state_path = write_state(&repo, "test-branch", &state);
    // gh succeeds.
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_render(
        &repo,
        &["--pr", "42", "--state-file", state_path.to_str().unwrap()],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
}

#[test]
fn render_pr_body_gh_edit_failure_reports_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = minimal_complete_state("Failing edit");
    let state_path = write_state(&repo, "test-branch", &state);
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho 'edit denied' >&2\nexit 1\n");

    let output = run_render(
        &repo,
        &["--pr", "42", "--state-file", state_path.to_str().unwrap()],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("edit denied"));
}
