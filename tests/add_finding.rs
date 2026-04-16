//! Integration tests for `bin/flow add-finding`.
//!
//! The command records a triage finding in the current branch's state
//! file. Tests run flow-rs in a temp git repo with an explicit
//! --branch to bypass git branch detection.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_git_repo_with_remote, parse_output};
use serde_json::{json, Value};

fn write_state(repo: &Path, branch: &str, state: &Value) -> std::path::PathBuf {
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let path = state_dir.join(format!("{}.json", branch));
    fs::write(&path, serde_json::to_string_pretty(state).unwrap()).unwrap();
    path
}

fn run_add_finding(repo: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("add-finding")
        .args(args)
        .current_dir(repo)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

#[test]
fn add_finding_records_dismissed_during_code_review() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({
        "schema_version": 1,
        "branch": "test-feature",
        "current_phase": "flow-code-review",
        "findings": []
    });
    let state_path = write_state(&repo, "test-feature", &state);

    let output = run_add_finding(
        &repo,
        &[
            "--finding",
            "Dead import in parser.rs",
            "--reason",
            "Used only in macro expansion",
            "--outcome",
            "dismissed",
            "--phase",
            "flow-code-review",
            "--branch",
            "test-feature",
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["finding_count"], 1);

    // Verify state file contents.
    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    let findings = on_disk["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["outcome"], "dismissed");
    assert_eq!(findings[0]["phase"], "flow-code-review");
    assert_eq!(findings[0]["finding"], "Dead import in parser.rs");
}

#[test]
fn add_finding_invalid_outcome_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({
        "schema_version": 1,
        "branch": "test-feature",
        "current_phase": "flow-code-review",
        "findings": []
    });
    write_state(&repo, "test-feature", &state);

    let output = run_add_finding(
        &repo,
        &[
            "--finding",
            "x",
            "--reason",
            "y",
            "--outcome",
            "bogus",
            "--phase",
            "flow-code-review",
            "--branch",
            "test-feature",
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Invalid outcome"));
}

#[test]
fn add_finding_code_review_rejects_filed() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({
        "branch": "x",
        "current_phase": "flow-code-review",
        "findings": []
    });
    write_state(&repo, "x", &state);

    let output = run_add_finding(
        &repo,
        &[
            "--finding",
            "needs follow up",
            "--reason",
            "not in scope",
            "--outcome",
            "filed",
            "--phase",
            "flow-code-review",
            "--branch",
            "x",
            "--issue-url",
            "https://github.com/o/r/issues/9",
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    // Gate should name the rule it enforces.
    let msg = data["message"].as_str().unwrap_or("");
    assert!(msg.to_lowercase().contains("code review") || msg.contains("code-review"));
}

#[test]
fn add_finding_allows_filed_outside_code_review() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({
        "branch": "y",
        "current_phase": "flow-learn",
        "findings": []
    });
    write_state(&repo, "y", &state);

    let output = run_add_finding(
        &repo,
        &[
            "--finding",
            "process gap",
            "--reason",
            "no rule yet",
            "--outcome",
            "filed",
            "--phase",
            "flow-learn",
            "--branch",
            "y",
            "--issue-url",
            "https://github.com/o/r/issues/11",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["finding_count"], 1);
}

#[test]
fn add_finding_no_state_file_reports_no_state() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // No state file written — command should return "no_state".
    let output = run_add_finding(
        &repo,
        &[
            "--finding",
            "x",
            "--reason",
            "y",
            "--outcome",
            "fixed",
            "--phase",
            "flow-code",
            "--branch",
            "missing",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "no_state");
}

#[test]
fn add_finding_with_issue_url_records_field() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({
        "branch": "z",
        "current_phase": "flow-learn",
        "findings": []
    });
    let state_path = write_state(&repo, "z", &state);

    let output = run_add_finding(
        &repo,
        &[
            "--finding",
            "learn gap",
            "--reason",
            "no rule",
            "--outcome",
            "filed",
            "--phase",
            "flow-learn",
            "--branch",
            "z",
            "--issue-url",
            "https://github.com/o/r/issues/1",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    let f = &on_disk["findings"][0];
    assert_eq!(f["issue_url"], "https://github.com/o/r/issues/1");
}

#[test]
fn add_finding_with_path_records_rule_path() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({
        "branch": "w",
        "current_phase": "flow-learn",
        "findings": []
    });
    let state_path = write_state(&repo, "w", &state);

    let output = run_add_finding(
        &repo,
        &[
            "--finding",
            "rule written",
            "--reason",
            "captures the pattern",
            "--outcome",
            "rule_written",
            "--phase",
            "flow-learn",
            "--branch",
            "w",
            "--path",
            ".claude/rules/new-rule.md",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    let f = &on_disk["findings"][0];
    assert_eq!(f["path"], ".claude/rules/new-rule.md");
}

#[test]
fn add_finding_multiple_invocations_append() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({
        "branch": "m",
        "current_phase": "flow-code",
        "findings": []
    });
    let state_path = write_state(&repo, "m", &state);

    for i in 1..=3 {
        let finding = format!("finding #{}", i);
        let output = run_add_finding(
            &repo,
            &[
                "--finding",
                &finding,
                "--reason",
                "r",
                "--outcome",
                "fixed",
                "--phase",
                "flow-code",
                "--branch",
                "m",
            ],
        );
        assert_eq!(output.status.code(), Some(0));
    }

    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(on_disk["findings"].as_array().unwrap().len(), 3);
}

#[test]
fn add_finding_array_root_state_returns_ok_zero() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("test-feature.json"), "[1, 2, 3]").unwrap();

    let output = run_add_finding(
        &repo,
        &[
            "--finding",
            "x",
            "--reason",
            "y",
            "--outcome",
            "fixed",
            "--phase",
            "flow-code",
            "--branch",
            "test-feature",
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "Array-root state should not crash; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["finding_count"], 0);
}
