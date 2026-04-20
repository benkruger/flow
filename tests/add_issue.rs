//! Integration tests for `bin/flow add-issue` and its library surface.
//!
//! Subprocess tests exercise CLI dispatch. Library tests drive
//! `run_impl_main` directly. Migrated from inline `#[cfg(test)]` per
//! `.claude/rules/test-placement.md`.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_git_repo_with_remote, parse_output};
use flow_rs::add_issue::{run_impl_main, Args};
use flow_rs::lock::mutate_state;
use flow_rs::phase_config::phase_names;
use flow_rs::utils::now;
use serde_json::{json, Value};

fn write_state(repo: &Path, branch: &str, state: &Value) -> std::path::PathBuf {
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let path = state_dir.join(format!("{}.json", branch));
    fs::write(&path, serde_json::to_string_pretty(state).unwrap()).unwrap();
    path
}

fn run_add_issue(repo: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("add-issue")
        .args(args)
        .current_dir(repo)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

#[test]
fn add_issue_records_entry_in_state() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({"branch": "b", "current_phase": "flow-learn", "issues_filed": []});
    let state_path = write_state(&repo, "b", &state);

    let output = run_add_issue(
        &repo,
        &[
            "--label",
            "Rule",
            "--title",
            "Test rule",
            "--url",
            "https://github.com/o/r/issues/1",
            "--phase",
            "flow-learn",
            "--branch",
            "b",
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
    assert_eq!(data["issue_count"], 1);

    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    let issues = on_disk["issues_filed"].as_array().unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0]["label"], "Rule");
    assert_eq!(issues[0]["title"], "Test rule");
    assert_eq!(issues[0]["url"], "https://github.com/o/r/issues/1");
    assert_eq!(issues[0]["phase"], "flow-learn");
    assert_eq!(issues[0]["phase_name"], "Learn");
}

#[test]
fn add_issue_no_state_file_returns_no_state() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let output = run_add_issue(
        &repo,
        &[
            "--label",
            "Rule",
            "--title",
            "t",
            "--url",
            "u",
            "--phase",
            "flow-learn",
            "--branch",
            "missing",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "no_state");
}

#[test]
fn add_issue_creates_issues_filed_array_if_missing() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // State has no issues_filed key at all.
    let state = json!({"branch": "c", "current_phase": "flow-learn"});
    let state_path = write_state(&repo, "c", &state);

    let output = run_add_issue(
        &repo,
        &[
            "--label",
            "Flow",
            "--title",
            "Flow process gap",
            "--url",
            "https://github.com/benkruger/flow/issues/10",
            "--phase",
            "flow-learn",
            "--branch",
            "c",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(on_disk["issues_filed"].as_array().unwrap().len(), 1);
}

#[test]
fn add_issue_appends_to_existing_list() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({
        "branch": "d",
        "current_phase": "flow-learn",
        "issues_filed": [{"label": "Prior", "title": "Existing"}]
    });
    let state_path = write_state(&repo, "d", &state);

    let output = run_add_issue(
        &repo,
        &[
            "--label",
            "Rule",
            "--title",
            "New rule",
            "--url",
            "https://x/y/issues/2",
            "--phase",
            "flow-learn",
            "--branch",
            "d",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["issue_count"], 2);

    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    let issues = on_disk["issues_filed"].as_array().unwrap();
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0]["label"], "Prior");
    assert_eq!(issues[1]["label"], "Rule");
}

#[test]
fn add_issue_unknown_phase_falls_back_to_raw_name() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({"branch": "u", "current_phase": "flow-learn", "issues_filed": []});
    let state_path = write_state(&repo, "u", &state);

    let output = run_add_issue(
        &repo,
        &[
            "--label",
            "Rule",
            "--title",
            "t",
            "--url",
            "u",
            "--phase",
            "some-custom-phase",
            "--branch",
            "u",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    let f = &on_disk["issues_filed"][0];
    // Unknown phase: phase_name falls back to the raw phase string.
    assert_eq!(f["phase"], "some-custom-phase");
    assert_eq!(f["phase_name"], "some-custom-phase");
}

#[test]
fn add_issue_no_branch_no_git_returns_branch_resolution_error() {
    // Subprocess cwd is a non-git tempdir AND no --branch override is
    // passed. resolve_branch falls back to current_branch() which returns
    // None for non-git dirs, so run_impl_main surfaces the
    // "Could not determine current branch" error and exits 1.
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("add-issue")
        .args([
            "--label",
            "Rule",
            "--title",
            "t",
            "--url",
            "u",
            "--phase",
            "flow-learn",
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
fn add_issue_corrupt_state_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("bad.json"), "{corrupt").unwrap();

    let output = run_add_issue(
        &repo,
        &[
            "--label",
            "Rule",
            "--title",
            "t",
            "--url",
            "u",
            "--phase",
            "flow-learn",
            "--branch",
            "bad",
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .contains("Failed to add issue"),
        "Error should mention the operation that failed"
    );
}

// --- Library-level tests (migrated from inline `#[cfg(test)]`) ---

fn make_state_lib(branch: &str) -> Value {
    json!({
        "schema_version": 1,
        "branch": branch,
        "current_phase": "flow-learn",
        "issues_filed": []
    })
}

fn write_state_lib(dir: &Path, branch: &str, state: &Value) -> std::path::PathBuf {
    let state_dir = dir.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let path = state_dir.join(format!("{}.json", branch));
    fs::write(&path, serde_json::to_string_pretty(state).unwrap()).unwrap();
    path
}

fn make_args(branch: Option<&str>) -> Args {
    Args {
        label: "Rule".to_string(),
        title: "test-title".to_string(),
        url: "https://github.com/owner/repo/issues/1".to_string(),
        phase: "flow-learn".to_string(),
        branch: branch.map(|s| s.to_string()),
    }
}

#[test]
fn add_issue_to_empty_array_lib() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_state_lib("test-feature");
    let path = write_state_lib(dir.path(), "test-feature", &state);

    let result = mutate_state(&path, |s| {
        let names = phase_names();
        let phase = "flow-learn";
        let phase_name = names.get(phase).cloned().unwrap_or_default();
        s["issues_filed"]
            .as_array_mut()
            .expect("issues_filed is always an array in this fixture")
            .push(json!({
                "label": "Rule",
                "title": "Add rule: use git -C",
                "url": "https://github.com/test/test/issues/1",
                "phase": phase,
                "phase_name": phase_name,
                "timestamp": now(),
            }));
    })
    .unwrap();

    let issues = result["issues_filed"].as_array().unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0]["label"], "Rule");
    assert_eq!(issues[0]["phase_name"], "Learn");
    assert!(issues[0]["timestamp"].as_str().unwrap().contains("T"));
}

#[test]
fn add_issue_preserves_existing_lib() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_state_lib("test-feature");
    state["issues_filed"] = json!([{"label": "Flow", "title": "existing"}]);
    let path = write_state_lib(dir.path(), "test-feature", &state);

    mutate_state(&path, |s| {
        s["issues_filed"]
            .as_array_mut()
            .expect("array in fixture")
            .push(json!({"label": "Rule", "title": "new"}));
    })
    .unwrap();

    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let issues = on_disk["issues_filed"].as_array().unwrap();
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0]["title"], "existing");
    assert_eq!(issues[1]["title"], "new");
}

#[test]
fn add_issue_creates_array_if_missing_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let state_dir = root.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let path = state_dir.join("test-feature.json");
    fs::write(&path, r#"{"current_phase": "flow-learn"}"#).unwrap();

    let args = Args {
        label: "Flaky Test".to_string(),
        title: "test".to_string(),
        url: "https://example.com/1".to_string(),
        phase: "flow-learn".to_string(),
        branch: Some("test-feature".to_string()),
    };

    let (value, code) = run_impl_main(args, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["issue_count"], 1);

    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(on_disk["issues_filed"].as_array().unwrap().len(), 1);
}

#[test]
fn add_issue_array_root_state_noop_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let state_dir = root.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let path = state_dir.join("test-feature.json");
    fs::write(&path, "[1, 2, 3]").unwrap();

    let args = Args {
        label: "Rule".to_string(),
        title: "should not appear".to_string(),
        url: "https://example.com/1".to_string(),
        phase: "flow-learn".to_string(),
        branch: Some("test-feature".to_string()),
    };

    let (value, code) = run_impl_main(args, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["issue_count"], 0);

    let after = fs::read_to_string(&path).unwrap();
    let parsed: Value = serde_json::from_str(&after).unwrap();
    assert!(parsed.is_array());
    assert_eq!(parsed.as_array().unwrap().len(), 3);
}

#[test]
fn add_issue_run_impl_main_no_state_returns_no_state_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let args = make_args(Some("missing-branch"));
    let (value, code) = run_impl_main(args, &root);
    assert_eq!(value["status"], "no_state");
    assert_eq!(code, 0);
}

#[test]
fn add_issue_run_impl_main_success_returns_issue_count_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let state_dir = root.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join("present-branch.json"),
        r#"{"current_phase":"flow-learn","issues_filed":[]}"#,
    )
    .unwrap();
    let args = make_args(Some("present-branch"));
    let (value, code) = run_impl_main(args, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["issue_count"], 1);
}

#[test]
fn add_issue_run_impl_main_mutate_state_failure_returns_error_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let state_dir = root.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("present-branch.json"), "{not json").unwrap();
    let args = make_args(Some("present-branch"));
    let (value, code) = run_impl_main(args, &root);
    assert_eq!(value["status"], "error");
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Failed to add issue"));
}

#[test]
fn add_issue_run_impl_main_unknown_phase_falls_back_to_phase_string() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let state_dir = root.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join("unknown-phase.json"),
        r#"{"current_phase":"flow-learn","issues_filed":[]}"#,
    )
    .unwrap();
    let mut args = make_args(Some("unknown-phase"));
    args.phase = "custom-unknown-phase".to_string();
    let (value, code) = run_impl_main(args, &root);
    assert_eq!(value["status"], "ok");
    assert_eq!(code, 0);
    let on_disk: Value =
        serde_json::from_str(&fs::read_to_string(state_dir.join("unknown-phase.json")).unwrap())
            .unwrap();
    assert_eq!(
        on_disk["issues_filed"][0]["phase_name"],
        "custom-unknown-phase"
    );
}

#[test]
fn add_issue_run_impl_main_wrong_type_resets_to_array() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let state_dir = root.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join("wrong-type.json"),
        r#"{"current_phase":"flow-learn","issues_filed":"not-an-array"}"#,
    )
    .unwrap();
    let args = make_args(Some("wrong-type"));
    let (value, code) = run_impl_main(args, &root);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["issue_count"], 1);
    assert_eq!(code, 0);
}

#[test]
fn add_issue_run_impl_main_slash_branch_returns_structured_error_no_panic() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let args = make_args(Some("feature/foo"));
    let (value, code) = run_impl_main(args, &root);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Invalid branch 'feature/foo'"));
}
