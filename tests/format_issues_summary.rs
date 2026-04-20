//! Integration tests for `bin/flow format-issues-summary` and the
//! library-level `format_issues_summary` / `run_impl` / `run_impl_main`
//! surfaces. Mirrors `src/format_issues_summary.rs` per
//! `.claude/rules/test-placement.md`.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_git_repo_with_remote, parse_output};
use flow_rs::format_issues_summary::{format_issues_summary, run_impl, run_impl_main, Args};
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

// --- format_issues_summary (library surface) ---

fn make_issues(labels: &[&str]) -> serde_json::Value {
    let issues: Vec<serde_json::Value> = labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            json!({
                "label": label,
                "title": format!("Issue {}", i + 1),
                "url": format!("https://github.com/test/test/issues/{}", i + 1),
                "phase": "flow-learn",
                "phase_name": "Learn",
                "timestamp": "2026-01-01T00:00:00-08:00",
            })
        })
        .collect();
    json!(issues)
}

#[test]
fn empty_issues_returns_no_issues() {
    let state = json!({"issues_filed": []});
    let result = format_issues_summary(&state);
    assert!(!result.has_issues);
    assert_eq!(result.banner_line, "");
    assert_eq!(result.table, "");
}

#[test]
fn missing_issues_filed_returns_no_issues() {
    let state = json!({"branch": "test"});
    let result = format_issues_summary(&state);
    assert!(!result.has_issues);
}

#[test]
fn single_issue_formats_correctly() {
    let state = json!({"issues_filed": make_issues(&["Rule"])});
    let result = format_issues_summary(&state);
    assert!(result.has_issues);
    assert_eq!(result.banner_line, "Issues filed: 1 (Rule: 1)");
    assert!(result.table.contains("| Label | Title | Phase | URL |"));
    assert!(result.table.contains("| Rule | Issue 1 | Learn |"));
}

#[test]
fn multiple_labels_grouped() {
    let state = json!({"issues_filed": make_issues(&["Rule", "Flaky Test", "Rule", "Tech Debt"])});
    let result = format_issues_summary(&state);
    assert!(result.has_issues);
    assert_eq!(
        result.banner_line,
        "Issues filed: 4 (Rule: 2, Flaky Test: 1, Tech Debt: 1)"
    );
}

#[test]
fn table_contains_all_issues() {
    let state = json!({"issues_filed": make_issues(&["Rule", "Flow"])});
    let result = format_issues_summary(&state);
    let lines: Vec<&str> = result.table.trim().split('\n').collect();
    let header_and_separator = 2;
    assert_eq!(lines.len(), header_and_separator + 2);
}

#[test]
fn table_url_is_short_reference() {
    let state = json!({
        "issues_filed": [{
            "label": "Rule",
            "title": "Test rule",
            "url": "https://github.com/test/test/issues/42",
            "phase": "flow-learn",
            "phase_name": "Learn",
            "timestamp": "2026-01-01T00:00:00-08:00",
        }]
    });
    let result = format_issues_summary(&state);
    assert!(result.table.contains("#42"));
}

#[test]
fn label_order_preserved() {
    let state = json!({"issues_filed": make_issues(&["Flaky Test", "Rule", "Flaky Test"])});
    let result = format_issues_summary(&state);
    assert_eq!(
        result.banner_line,
        "Issues filed: 3 (Flaky Test: 2, Rule: 1)"
    );
}

#[test]
fn phase_name_fallback_to_phase() {
    let state = json!({
        "issues_filed": [{
            "label": "Rule",
            "title": "Test",
            "url": "https://github.com/test/test/issues/1",
            "phase": "flow-code",
        }]
    });
    let result = format_issues_summary(&state);
    assert!(result.table.contains("| flow-code |"));
}

// --- run_impl (fallible seam) ---

fn write_state_path(dir: &Path, state: &Value) -> std::path::PathBuf {
    let path = dir.join("state.json");
    fs::write(&path, serde_json::to_string(state).unwrap()).unwrap();
    path
}

#[test]
fn run_impl_happy_path_writes_file_and_returns_result() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({"issues_filed": make_issues(&["Rule"])});
    let state_path = write_state_path(dir.path(), &state);
    let output_path = dir.path().join("issues.md");
    let args = Args {
        state_file: state_path.to_string_lossy().to_string(),
        output: output_path.to_string_lossy().to_string(),
    };
    let result = run_impl(&args).unwrap();
    assert!(result.has_issues);
    assert!(output_path.exists());
}

#[test]
fn run_impl_no_issues_skips_file_write() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({"issues_filed": []});
    let state_path = write_state_path(dir.path(), &state);
    let output_path = dir.path().join("issues.md");
    let args = Args {
        state_file: state_path.to_string_lossy().to_string(),
        output: output_path.to_string_lossy().to_string(),
    };
    let result = run_impl(&args).unwrap();
    assert!(!result.has_issues);
    assert!(!output_path.exists());
}

#[test]
fn run_impl_missing_state_file_returns_err() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        state_file: dir
            .path()
            .join("missing.json")
            .to_string_lossy()
            .to_string(),
        output: dir.path().join("out.md").to_string_lossy().to_string(),
    };
    let result = run_impl(&args);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not found"));
}

#[test]
fn run_impl_malformed_state_returns_err() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.json");
    fs::write(&bad, "{not json").unwrap();
    let args = Args {
        state_file: bad.to_string_lossy().to_string(),
        output: dir.path().join("out.md").to_string_lossy().to_string(),
    };
    let result = run_impl(&args);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Failed to parse"));
}

/// Exercises the read_to_string Err arm — state file path resolves
/// to a directory, so `exists()` passes but read_to_string fails
/// with EISDIR.
#[test]
fn run_impl_state_file_is_directory_returns_read_error() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.json");
    fs::create_dir(&state_path).unwrap();
    let args = Args {
        state_file: state_path.to_string_lossy().to_string(),
        output: dir.path().join("out.md").to_string_lossy().to_string(),
    };
    let err = run_impl(&args).unwrap_err();
    assert!(err.contains("Failed to read state file"), "got: {}", err);
}

/// Exercises fs::write Err arm — output path lives inside an
/// existing regular file (parent `mkdir -p` no-ops, write fails
/// with NotADirectory).
#[test]
fn run_impl_write_error_returns_err() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({"issues_filed": make_issues(&["Rule"])});
    let state_path = write_state_path(dir.path(), &state);
    let parent_as_file = dir.path().join("not-a-dir");
    fs::write(&parent_as_file, "blocker").unwrap();
    let output_path = parent_as_file.join("out.md");
    let args = Args {
        state_file: state_path.to_string_lossy().to_string(),
        output: output_path.to_string_lossy().to_string(),
    };
    let err = run_impl(&args).unwrap_err();
    assert!(err.contains("Failed to write output"), "got: {}", err);
}

/// Exercises the `output_path.parent() == None` branch. An empty
/// output string has no parent, so the `if let Some(parent)` arm
/// is skipped and fs::write("") fails.
#[test]
fn run_impl_empty_output_skips_mkdir_and_err_on_write() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({"issues_filed": make_issues(&["Rule"])});
    let state_path = write_state_path(dir.path(), &state);
    let args = Args {
        state_file: state_path.to_string_lossy().to_string(),
        output: String::new(),
    };
    let err = run_impl(&args).unwrap_err();
    assert!(err.contains("Failed to write output"), "got: {}", err);
}

// --- run_impl_main (main.rs entry point) ---

#[test]
fn run_impl_main_happy_path_ok_with_json_value() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({"issues_filed": make_issues(&["Rule", "Flow"])});
    let state_path = write_state_path(dir.path(), &state);
    let args = Args {
        state_file: state_path.to_string_lossy().to_string(),
        output: dir.path().join("out.md").to_string_lossy().to_string(),
    };
    let (value, code) = run_impl_main(&args);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["has_issues"], true);
    assert!(value["banner_line"]
        .as_str()
        .unwrap()
        .contains("Issues filed: 2"));
}

#[test]
fn run_impl_main_no_issues_skips_file_write_returns_ok() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({"issues_filed": []});
    let state_path = write_state_path(dir.path(), &state);
    let args = Args {
        state_file: state_path.to_string_lossy().to_string(),
        output: dir.path().join("out.md").to_string_lossy().to_string(),
    };
    let (value, code) = run_impl_main(&args);
    assert_eq!(code, 0);
    assert_eq!(value["has_issues"], false);
}

#[test]
fn run_impl_main_missing_state_err_exit_1() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        state_file: dir
            .path()
            .join("missing.json")
            .to_string_lossy()
            .to_string(),
        output: dir.path().join("out.md").to_string_lossy().to_string(),
    };
    let (value, code) = run_impl_main(&args);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    assert!(value["message"].as_str().unwrap().contains("not found"));
}
