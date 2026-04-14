//! Integration tests for `bin/flow format-issues-summary`.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_git_repo_with_remote, parse_output};
use serde_json::{json, Value};

fn run_cmd(repo: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("format-issues-summary")
        .args(args)
        .current_dir(repo)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

fn write_state(dir: &Path, state: &Value) -> std::path::PathBuf {
    let path = dir.join("state.json");
    fs::write(&path, serde_json::to_string(state).unwrap()).unwrap();
    path
}

#[test]
fn happy_path_writes_table_and_reports_ok() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({
        "issues_filed": [
            {"label": "Rule", "title": "t1", "url": "https://github.com/o/r/issues/1", "phase_name": "Learn"},
            {"label": "Flow", "title": "t2", "url": "https://github.com/o/r/issues/2", "phase_name": "Learn"}
        ]
    });
    let state_path = write_state(dir.path(), &state);
    let output_path = dir.path().join("issues.md");

    let output = run_cmd(
        &repo,
        &[
            "--state-file",
            state_path.to_str().unwrap(),
            "--output",
            output_path.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["has_issues"], true);
    assert!(data["banner_line"]
        .as_str()
        .unwrap()
        .contains("Issues filed: 2"));
    assert!(output_path.exists());
    let contents = fs::read_to_string(&output_path).unwrap();
    assert!(contents.contains("| Label | Title | Phase | URL |"));
}

#[test]
fn no_issues_does_not_write_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({"issues_filed": []});
    let state_path = write_state(dir.path(), &state);
    let output_path = dir.path().join("no-issues.md");

    let output = run_cmd(
        &repo,
        &[
            "--state-file",
            state_path.to_str().unwrap(),
            "--output",
            output_path.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["has_issues"], false);
    assert!(!output_path.exists());
}

#[test]
fn missing_state_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let missing = dir.path().join("nope.json");
    let output_path = dir.path().join("out.md");

    let output = run_cmd(
        &repo,
        &[
            "--state-file",
            missing.to_str().unwrap(),
            "--output",
            output_path.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("State file not found"));
}

#[test]
fn malformed_state_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_path = dir.path().join("state.json");
    fs::write(&state_path, "not json").unwrap();
    let output_path = dir.path().join("out.md");

    let output = run_cmd(
        &repo,
        &[
            "--state-file",
            state_path.to_str().unwrap(),
            "--output",
            output_path.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Failed to parse"));
}

#[test]
fn creates_parent_directories_for_output() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({
        "issues_filed": [
            {"label": "X", "title": "t", "url": "https://github.com/o/r/issues/1", "phase_name": "Learn"}
        ]
    });
    let state_path = write_state(dir.path(), &state);
    // Deep output path whose parent dirs don't exist.
    let output_path = dir.path().join("nested/deep/issues.md");

    let output = run_cmd(
        &repo,
        &[
            "--state-file",
            state_path.to_str().unwrap(),
            "--output",
            output_path.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(output_path.exists());
}
