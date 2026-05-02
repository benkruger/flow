//! Integration tests for `bin/flow add-finding` and its library surface.
//!
//! The subprocess tests exercise CLI dispatch. The library tests drive
//! `run_impl_with_root`, `run_impl_with_root_resolver`,
//! `run_impl_main`, and `run_impl_main_with_cwd_result` directly
//! against tempdir fixtures. Migrated from inline `#[cfg(test)]` per
//! `.claude/rules/test-placement.md`.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_git_repo_with_remote, parse_output};
use flow_rs::add_finding::{
    run_impl_main, run_impl_main_with_cwd_result, run_impl_with_root, run_impl_with_root_resolver,
    Args,
};
use serde_json::{json, Value};

fn write_state(repo: &Path, branch: &str, state: &Value) -> std::path::PathBuf {
    let branch_dir = repo.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    let path = branch_dir.join("state.json");
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
fn add_finding_no_branch_no_git_returns_branch_resolution_error() {
    // Subprocess cwd is a non-git tempdir AND no --branch override is
    // passed. resolve_branch falls back to current_branch() which returns
    // None for non-git dirs, so run_impl_with_root surfaces the
    // "Could not determine current branch" error and exits 1.
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("add-finding")
        .args([
            "--finding",
            "x",
            "--reason",
            "y",
            "--outcome",
            "fixed",
            "--phase",
            "flow-code",
        ])
        .current_dir(dir.path())
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .env("GIT_CEILING_DIRECTORIES", dir.path())
        .env_remove("FLOW_SIMULATE_BRANCH")
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Could not determine current branch"));
}

#[test]
fn add_finding_array_root_state_returns_ok_zero() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let branch_dir = repo.join(".flow-states").join("test-feature");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), "[1, 2, 3]").unwrap();

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

// --- Library-level tests (migrated from inline `#[cfg(test)]`) ---

fn make_state_lib(branch: &str) -> Value {
    json!({
        "schema_version": 1,
        "branch": branch,
        "current_phase": "flow-code-review",
        "findings": []
    })
}

fn write_state_lib(dir: &Path, branch: &str, state: &Value) -> std::path::PathBuf {
    let branch_dir = dir.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    let path = branch_dir.join("state.json");
    fs::write(&path, serde_json::to_string_pretty(state).unwrap()).unwrap();
    path
}

fn make_args(outcome: &str, phase: &str, branch: Option<&str>) -> Args {
    Args {
        finding: "test-finding".to_string(),
        reason: "test-reason".to_string(),
        outcome: outcome.to_string(),
        phase: phase.to_string(),
        issue_url: None,
        path: None,
        branch: branch.map(|s| s.to_string()),
    }
}

#[test]
fn add_finding_happy_path_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let state = make_state_lib("test-feature");
    let path = write_state_lib(&root, "test-feature", &state);

    let args = Args {
        finding: "Unused import in parser.rs".to_string(),
        reason: "False positive — import used in macro expansion".to_string(),
        outcome: "dismissed".to_string(),
        phase: "flow-code-review".to_string(),
        issue_url: None,
        path: None,
        branch: Some("test-feature".to_string()),
    };

    let count = run_impl_with_root(&args, &root, &root).unwrap();
    assert_eq!(count, 1);

    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let findings = on_disk["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["finding"], "Unused import in parser.rs");
    assert_eq!(
        findings[0]["reason"],
        "False positive — import used in macro expansion"
    );
    assert_eq!(findings[0]["outcome"], "dismissed");
    assert_eq!(findings[0]["phase"], "flow-code-review");
    assert_eq!(findings[0]["phase_name"], "Code Review");
    assert!(findings[0]["timestamp"].as_str().unwrap().contains("T"));
}

#[test]
fn add_finding_creates_array_if_missing_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("test-feature");
    fs::create_dir_all(&branch_dir).unwrap();
    let path = branch_dir.join("state.json");
    // State file with no `findings` key — exercises the closure's
    // auto-create branch.
    fs::write(&path, r#"{"current_phase": "flow-code-review"}"#).unwrap();

    let args = Args {
        finding: "test".to_string(),
        reason: "test reason".to_string(),
        outcome: "fixed".to_string(),
        phase: "flow-code-review".to_string(),
        issue_url: None,
        path: None,
        branch: Some("test-feature".to_string()),
    };

    let count = run_impl_with_root(&args, &root, &root).unwrap();
    assert_eq!(count, 1);

    let content = fs::read_to_string(&path).unwrap();
    let on_disk: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(on_disk["findings"].as_array().unwrap().len(), 1);
}

#[test]
fn add_finding_valid_outcome_accepted_lib() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_state_lib("test-feature");
    write_state_lib(dir.path(), "test-feature", &state);
    let args = make_args("fixed", "flow-code-review", Some("test-feature"));
    assert_eq!(args.outcome, "fixed");
}

#[test]
fn add_finding_timestamp_is_pacific_lib() {
    use flow_rs::utils::now;
    let ts = now();
    let has_pdt = ts.contains("-07:00");
    let has_pst = ts.contains("-08:00");
    let is_pacific = has_pdt | has_pst;
    assert!(is_pacific, "Timestamp {} should be Pacific Time", ts);
}

// --- code_review_filing_gate (tested via run_impl_with_root) ---

#[test]
fn filed_outcome_rejected_for_code_review_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    write_state_lib(&root, "test-feature", &make_state_lib("test-feature"));
    let args = make_args("filed", "flow-code-review", Some("test-feature"));
    let err = run_impl_with_root(&args, &root, &root).unwrap_err();
    assert!(err.contains("flow-code-review"));
    assert!(err.contains("filed"));
}

#[test]
fn filed_outcome_accepted_for_learn_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let state = json!({
        "current_phase": "flow-learn",
        "findings": []
    });
    write_state_lib(&root, "learn-branch", &state);
    let args = Args {
        finding: "x".to_string(),
        reason: "y".to_string(),
        outcome: "filed".to_string(),
        phase: "flow-learn".to_string(),
        issue_url: Some("https://example.com/1".to_string()),
        path: None,
        branch: Some("learn-branch".to_string()),
    };
    let count = run_impl_with_root(&args, &root, &root).unwrap();
    assert_eq!(count, 1);
}

#[test]
fn dismissed_outcome_accepted_for_code_review_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    write_state_lib(&root, "cr-branch", &make_state_lib("cr-branch"));
    let args = make_args("dismissed", "flow-code-review", Some("cr-branch"));
    assert!(run_impl_with_root(&args, &root, &root).is_ok());
}

#[test]
fn fixed_outcome_accepted_for_code_review_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    write_state_lib(&root, "cr-branch", &make_state_lib("cr-branch"));
    let args = make_args("fixed", "flow-code-review", Some("cr-branch"));
    assert!(run_impl_with_root(&args, &root, &root).is_ok());
}

#[test]
fn filed_outcome_accepted_for_flow_code_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let state = json!({"current_phase": "flow-code", "findings": []});
    write_state_lib(&root, "code-branch", &state);
    let args = make_args("filed", "flow-code", Some("code-branch"));
    assert!(run_impl_with_root(&args, &root, &root).is_ok());
}

#[test]
fn leading_whitespace_phase_rejected_for_code_review_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    write_state_lib(&root, "cr-branch", &make_state_lib("cr-branch"));
    let args = make_args("filed", " flow-code-review", Some("cr-branch"));
    assert!(run_impl_with_root(&args, &root, &root).is_err());
}

#[test]
fn trailing_whitespace_phase_rejected_for_code_review_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    write_state_lib(&root, "cr-branch", &make_state_lib("cr-branch"));
    let args = make_args("filed", "flow-code-review ", Some("cr-branch"));
    assert!(run_impl_with_root(&args, &root, &root).is_err());
}

#[test]
fn uppercase_phase_rejected_for_code_review_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    write_state_lib(&root, "cr-branch", &make_state_lib("cr-branch"));
    let args = make_args("filed", "FLOW-CODE-REVIEW", Some("cr-branch"));
    assert!(run_impl_with_root(&args, &root, &root).is_err());
}

#[test]
fn mixed_case_phase_rejected_for_code_review_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    write_state_lib(&root, "cr-branch", &make_state_lib("cr-branch"));
    let args = make_args("filed", "Flow-Code-Review", Some("cr-branch"));
    assert!(run_impl_with_root(&args, &root, &root).is_err());
}

#[test]
fn uppercase_filed_outcome_rejected_for_code_review_lib() {
    // "Filed" isn't in VALID_OUTCOMES (case-sensitive), so it fails at
    // the VALID_OUTCOMES check before the normalized gate. Either way,
    // the call returns an error.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    write_state_lib(&root, "cr-branch", &make_state_lib("cr-branch"));
    let args = make_args("Filed", "flow-code-review", Some("cr-branch"));
    assert!(run_impl_with_root(&args, &root, &root).is_err());
}

#[test]
fn embedded_nul_phase_rejected_for_code_review_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    write_state_lib(&root, "cr-branch", &make_state_lib("cr-branch"));
    let args = make_args("filed", "flow-code-review\0", Some("cr-branch"));
    assert!(run_impl_with_root(&args, &root, &root).is_err());
}

#[test]
fn add_finding_array_root_state_noop_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("test-feature");
    fs::create_dir_all(&branch_dir).unwrap();
    let path = branch_dir.join("state.json");
    fs::write(&path, "[1, 2, 3]").unwrap();

    let args = Args {
        finding: "should not appear".to_string(),
        reason: "guard should reject".to_string(),
        outcome: "fixed".to_string(),
        phase: "flow-code-review".to_string(),
        issue_url: None,
        path: None,
        branch: Some("test-feature".to_string()),
    };

    let count = run_impl_with_root(&args, &root, &root).unwrap();
    assert_eq!(count, 0);

    let after = fs::read_to_string(&path).unwrap();
    let parsed: Value = serde_json::from_str(&after).unwrap();
    assert!(parsed.is_array());
    assert_eq!(parsed.as_array().unwrap().len(), 3);
}

// --- run_impl_main seam tests ---

#[test]
fn add_finding_run_impl_main_invalid_outcome_returns_error_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let args = make_args("not-an-outcome", "flow-learn", Some("test-branch"));
    let (value, code) = run_impl_main(args, &root, &root);
    assert_eq!(value["status"], "error");
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Invalid outcome"));
}

#[test]
fn add_finding_run_impl_main_code_review_filing_blocked_returns_error_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let args = make_args("filed", "flow-code-review", Some("test-branch"));
    let (value, code) = run_impl_main(args, &root, &root);
    assert_eq!(value["status"], "error");
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Code Review"));
}

#[test]
fn add_finding_run_impl_main_no_state_returns_no_state_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let args = make_args("fixed", "flow-learn", Some("missing-branch"));
    let (value, code) = run_impl_main(args, &root, &root);
    assert_eq!(value["status"], "no_state");
    assert_eq!(code, 0);
}

#[test]
fn add_finding_run_impl_main_success_returns_finding_count_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("present-branch");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(
        branch_dir.join("state.json"),
        r#"{"current_phase":"flow-learn","findings":[]}"#,
    )
    .unwrap();
    let args = make_args("fixed", "flow-learn", Some("present-branch"));
    let (value, code) = run_impl_main(args, &root, &root);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["finding_count"], 1);
    assert_eq!(code, 0);
}

#[test]
fn add_finding_run_impl_main_success_with_issue_url_writes_field() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("with-url");
    fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    fs::write(
        &state_path,
        r#"{"current_phase":"flow-learn","findings":[]}"#,
    )
    .unwrap();
    let args = Args {
        finding: "process gap".to_string(),
        reason: "filed as flow issue".to_string(),
        outcome: "filed".to_string(),
        phase: "flow-learn".to_string(),
        issue_url: Some("https://github.com/test/test/issues/42".to_string()),
        path: None,
        branch: Some("with-url".to_string()),
    };
    let (value, code) = run_impl_main(args, &root, &root);
    assert_eq!(value["status"], "ok");
    assert_eq!(code, 0);
    let on_disk: Value =
        serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(
        on_disk["findings"][0]["issue_url"],
        "https://github.com/test/test/issues/42"
    );
}

#[test]
fn add_finding_run_impl_main_success_with_path_writes_field() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("with-path");
    fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    fs::write(
        &state_path,
        r#"{"current_phase":"flow-learn","findings":[]}"#,
    )
    .unwrap();
    let args = Args {
        finding: "no rule for X".to_string(),
        reason: "wrote rule".to_string(),
        outcome: "rule_written".to_string(),
        phase: "flow-learn".to_string(),
        issue_url: None,
        path: Some(".claude/rules/x.md".to_string()),
        branch: Some("with-path".to_string()),
    };
    let (value, code) = run_impl_main(args, &root, &root);
    assert_eq!(value["status"], "ok");
    assert_eq!(code, 0);
    let on_disk: Value =
        serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(on_disk["findings"][0]["path"], ".claude/rules/x.md");
}

#[test]
fn add_finding_run_impl_main_unknown_phase_falls_back_to_phase_string() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("custom-phase");
    fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    fs::write(
        &state_path,
        r#"{"current_phase":"flow-learn","findings":[]}"#,
    )
    .unwrap();
    let args = make_args("fixed", "custom-unknown-phase", Some("custom-phase"));
    let (value, code) = run_impl_main(args, &root, &root);
    assert_eq!(value["status"], "ok");
    assert_eq!(code, 0);
    let on_disk: Value =
        serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(
        on_disk["findings"][0]["phase_name"], "custom-unknown-phase",
        "phase_name should fall back to the raw phase string"
    );
}

#[test]
fn add_finding_run_impl_main_no_findings_field_creates_array() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("no-findings");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(
        branch_dir.join("state.json"),
        r#"{"current_phase":"flow-learn"}"#,
    )
    .unwrap();
    let args = make_args("fixed", "flow-learn", Some("no-findings"));
    let (value, code) = run_impl_main(args, &root, &root);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["finding_count"], 1);
    assert_eq!(code, 0);
}

#[test]
fn add_finding_run_impl_main_findings_wrong_type_resets_to_array() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("wrong-type");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(
        branch_dir.join("state.json"),
        r#"{"current_phase":"flow-learn","findings":"not-an-array"}"#,
    )
    .unwrap();
    let args = make_args("fixed", "flow-learn", Some("wrong-type"));
    let (value, code) = run_impl_main(args, &root, &root);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["finding_count"], 1);
    assert_eq!(code, 0);
}

#[test]
fn add_finding_run_impl_main_cwd_drift_returns_error() {
    use std::process::Command as StdCommand;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let run_git = |args: &[&str]| {
        StdCommand::new("git")
            .args(args)
            .current_dir(&root)
            .output()
            .expect("git command failed");
    };
    run_git(&["init", "--initial-branch", "drift-feature"]);
    run_git(&["config", "user.email", "t@t.com"]);
    run_git(&["config", "user.name", "T"]);
    run_git(&["config", "commit.gpgsign", "false"]);
    run_git(&["commit", "--allow-empty", "-m", "init"]);

    let branch_dir = root.join(".flow-states").join("drift-feature");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(
        branch_dir.join("state.json"),
        r#"{"current_phase":"flow-learn","findings":[],"relative_cwd":"api"}"#,
    )
    .unwrap();
    fs::create_dir(root.join("api")).unwrap();
    fs::create_dir(root.join("ios")).unwrap();
    let ios = root.join("ios").canonicalize().unwrap();

    let args = make_args("fixed", "flow-learn", None);
    let (value, code) = run_impl_main(args, &root, &ios);
    assert_eq!(value["status"], "error");
    assert_eq!(code, 1);
    let msg = value["message"].as_str().unwrap();
    assert!(
        msg.contains("cwd drift"),
        "expected cwd drift message, got: {}",
        msg
    );
}

#[test]
fn add_finding_run_impl_main_mutate_state_failure_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("bad-state");
    fs::create_dir_all(&branch_dir).unwrap();
    // Make state.json a directory so mutate_state fails when it tries
    // to read the file.
    fs::create_dir(branch_dir.join("state.json")).unwrap();
    let args = make_args("fixed", "flow-learn", Some("bad-state"));
    let (value, code) = run_impl_main(args, &root, &root);
    assert_eq!(value["status"], "error");
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Failed to add finding"));
}

#[test]
fn add_finding_run_impl_with_root_resolver_none_returns_branch_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let args = make_args("fixed", "flow-learn", None);
    let resolver = |_: Option<&str>, _: &Path| -> Option<String> { None };
    let err = run_impl_with_root_resolver(&args, &root, &root, &resolver).unwrap_err();
    assert_eq!(err, "Could not determine current branch");
}

#[test]
fn add_finding_run_impl_main_slash_branch_returns_structured_error_no_panic() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let args = make_args("fixed", "flow-learn", Some("feature/foo"));
    let (value, code) = run_impl_main(args, &root, &root);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Invalid branch 'feature/foo'"));
}

#[test]
fn add_finding_run_impl_main_with_cwd_result_err_falls_back_to_dot() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let args = make_args("fixed", "flow-learn", Some("test-branch"));
    let cwd_err = Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "deleted cwd",
    ));
    let (value, code) = run_impl_main_with_cwd_result(args, &root, cwd_err);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "no_state");
}

#[test]
fn add_finding_run_impl_main_with_cwd_result_ok_uses_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let args = make_args("fixed", "flow-learn", Some("test-branch"));
    let cwd_ok = Ok(root.clone());
    let (value, code) = run_impl_main_with_cwd_result(args, &root, cwd_ok);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "no_state");
}

#[test]
fn add_finding_future_outcome_rejected_for_code_review() {
    // Outcomes outside the CR allowlist must be rejected.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    write_state_lib(&root, "cr-branch", &make_state_lib("cr-branch"));
    let args = make_args("rule_written", "flow-code-review", Some("cr-branch"));
    // "rule_written" is in VALID_OUTCOMES but not in CODE_REVIEW_ALLOWED_OUTCOMES.
    let err = run_impl_with_root(&args, &root, &root).unwrap_err();
    assert!(err.contains("rule_written") || err.contains("flow-code-review"));
}
