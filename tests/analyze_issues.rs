//! Integration tests for `bin/flow analyze-issues`.
//!
//! Uses `--issues-json` to bypass the gh subprocess — the flag exists
//! specifically for testing.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_gh_stub, create_git_repo_with_remote};
use serde_json::{json, Value};

/// Parse the full stdout as JSON (analyze-issues pretty-prints, so
/// last-line parsing doesn't work for it).
fn parse_full_stdout(output: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("Failed to parse stdout as JSON: {}\nstdout: {}", e, stdout))
}

fn run_analyze(repo: &Path, args: &[&str], stub_dir: &Path) -> Output {
    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("analyze-issues")
        .args(args)
        .current_dir(repo)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

/// Build a fake gh issue list response.
fn fake_issue(number: i64, title: &str, labels: Vec<&str>) -> serde_json::Value {
    let label_objs: Vec<serde_json::Value> =
        labels.iter().map(|name| json!({"name": name})).collect();
    json!({
        "number": number,
        "title": title,
        "body": "Some issue body",
        "url": format!("https://github.com/o/r/issues/{}", number),
        "createdAt": "2026-04-01T00:00:00Z",
        "labels": label_objs,
        "milestone": null,
    })
}

#[test]
fn analyze_issues_reads_json_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let issues = vec![
        fake_issue(1, "First", vec!["Rule"]),
        fake_issue(2, "Second", vec!["Tech Debt"]),
    ];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
    // gh is still called for blockers but stub returns empty.
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho '{\"data\":{}}'\nexit 0\n");

    let output = run_analyze(
        &repo,
        &["--issues-json", issues_path.to_str().unwrap()],
        &stub_dir,
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_full_stdout(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["total"], 2);
}

#[test]
fn analyze_issues_partitions_in_progress() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let issues = vec![
        fake_issue(1, "In progress", vec!["Flow In-Progress"]),
        fake_issue(2, "Available", vec!["Rule"]),
    ];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho '{\"data\":{}}'\nexit 0\n");

    let output = run_analyze(
        &repo,
        &["--issues-json", issues_path.to_str().unwrap()],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_full_stdout(&output);
    let in_progress = data["in_progress"].as_array().unwrap();
    assert_eq!(in_progress.len(), 1);
    assert_eq!(in_progress[0]["number"], 1);
}

#[test]
fn analyze_issues_nonexistent_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let missing = dir.path().join("no-such.json");
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_analyze(
        &repo,
        &["--issues-json", missing.to_str().unwrap()],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_full_stdout(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Could not read issues file"));
}

#[test]
fn analyze_issues_empty_list() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let issues: Vec<serde_json::Value> = vec![];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho '{\"data\":{}}'\nexit 0\n");

    let output = run_analyze(
        &repo,
        &["--issues-json", issues_path.to_str().unwrap()],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_full_stdout(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["total"], 0);
    assert!(data["issues"].as_array().unwrap().is_empty());
}

#[test]
fn analyze_issues_ready_filter() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let issues = vec![
        fake_issue(1, "Ready", vec!["Rule"]),
        fake_issue(2, "Also ready", vec!["Tech Debt"]),
    ];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho '{\"data\":{}}'\nexit 0\n");

    let output = run_analyze(
        &repo,
        &["--issues-json", issues_path.to_str().unwrap(), "--ready"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_full_stdout(&output);
    assert_eq!(data["status"], "ok");
}

#[test]
fn analyze_issues_decomposed_filter() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let issues = vec![fake_issue(1, "Any", vec!["Rule"])];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho '{\"data\":{}}'\nexit 0\n");

    let output = run_analyze(
        &repo,
        &[
            "--issues-json",
            issues_path.to_str().unwrap(),
            "--decomposed",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_full_stdout(&output);
    assert_eq!(data["status"], "ok");
}

#[test]
fn analyze_issues_blocked_filter() {
    // Drive the "blocked" filter closure inside filter_issues via the
    // run() CLI path (covers the closure body in the production binary).
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let issues = vec![fake_issue(1, "Decomposed", vec!["Decomposed"])];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho '{\"data\":{}}'\nexit 0\n");

    let output = run_analyze(
        &repo,
        &["--issues-json", issues_path.to_str().unwrap(), "--blocked"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_full_stdout(&output);
    assert_eq!(data["status"], "ok");
}

#[test]
fn analyze_issues_quick_start_filter() {
    // Drive the "quick-start" filter closure inside filter_issues via run().
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let issues = vec![fake_issue(1, "Decomposed", vec!["Decomposed"])];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho '{\"data\":{}}'\nexit 0\n");

    let output = run_analyze(
        &repo,
        &[
            "--issues-json",
            issues_path.to_str().unwrap(),
            "--quick-start",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_full_stdout(&output);
    assert_eq!(data["status"], "ok");
}

#[test]
fn analyze_issues_invalid_json_content_errors() {
    // File exists but contains invalid JSON → run() prints an error and
    // exits 1 via the "Invalid JSON" branch of the from_str match arm.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, "this is not json").unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_analyze(
        &repo,
        &["--issues-json", issues_path.to_str().unwrap()],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_full_stdout(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Invalid JSON"));
}

#[test]
fn analyze_issues_gh_failure_errors() {
    // No --issues-json: run() invokes `gh issue list`. Stub exits non-zero
    // → run() prints an error and exits 1 via the gh_result_to_stdout Err
    // branch.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho 'gh failed' >&2\nexit 1\n");

    let output = run_analyze(&repo, &[], &stub_dir);

    assert_eq!(output.status.code(), Some(1));
    let data = parse_full_stdout(&output);
    assert_eq!(data["status"], "error");
}

#[test]
fn analyze_issues_label_and_milestone_args_forwarded_to_gh() {
    // --label and --milestone args are pushed into the gh command. With a
    // stub that returns a valid issue list, the run() succeeds and exit 0.
    // Drives the `for l in &args.label` loop and the `if let Some(ref m)`
    // milestone branch.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho '[]'\nexit 0\n");

    let output = run_analyze(
        &repo,
        &[
            "--label",
            "Rule",
            "--label",
            "Tech Debt",
            "--milestone",
            "v1",
        ],
        &stub_dir,
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_full_stdout(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["total"], 0);
}
