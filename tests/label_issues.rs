//! Integration tests for `bin/flow label-issues`.
//!
//! The command reads the state file, extracts `#N` issue references,
//! and adds or removes the "Flow In-Progress" label via `gh issue edit`.
//! Tests install a mock `gh` on PATH so subprocess paths are exercised
//! without network access.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_gh_stub, create_git_repo_with_remote, parse_output};
use flow_rs::label_issues::{label_issues, run_impl_main, Args, LabelResult, LABEL};
use serde_json::json;

fn run_cmd(repo: &Path, args: &[&str], stub_dir: &Path) -> Output {
    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("label-issues")
        .args(args)
        .current_dir(repo)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

#[test]
fn add_label_to_all_issues_from_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_file = dir.path().join("state.json");
    fs::write(
        &state_file,
        json!({"prompt": "work on #42 and #99"}).to_string(),
    )
    .unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_cmd(
        &repo,
        &["--state-file", state_file.to_str().unwrap(), "--add"],
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
    let labeled: Vec<i64> = data["labeled"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    assert!(labeled.contains(&42));
    assert!(labeled.contains(&99));
    assert!(data["failed"].as_array().unwrap().is_empty());
}

#[test]
fn remove_label_from_all_issues_from_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_file = dir.path().join("state.json");
    fs::write(&state_file, json!({"prompt": "closing #7"}).to_string()).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_cmd(
        &repo,
        &["--state-file", state_file.to_str().unwrap(), "--remove"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    let labeled: Vec<i64> = data["labeled"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    assert_eq!(labeled, vec![7]);
}

#[test]
fn partitions_success_and_failure() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_file = dir.path().join("state.json");
    fs::write(&state_file, json!({"prompt": "fix #1 and #2"}).to_string()).unwrap();
    // gh succeeds for 1, fails for 2.
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         for arg in \"$@\"; do\n\
           if [ \"$arg\" = \"2\" ]; then\n\
             exit 1\n\
           fi\n\
         done\n\
         exit 0\n",
    );

    let output = run_cmd(
        &repo,
        &["--state-file", state_file.to_str().unwrap(), "--add"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    let labeled: Vec<i64> = data["labeled"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    let failed: Vec<i64> = data["failed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    assert_eq!(labeled, vec![1]);
    assert_eq!(failed, vec![2]);
}

#[test]
fn missing_prompt_key_produces_empty_lists() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_file = dir.path().join("state.json");
    fs::write(&state_file, json!({"branch": "test"}).to_string()).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_cmd(
        &repo,
        &["--state-file", state_file.to_str().unwrap(), "--add"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert!(data["labeled"].as_array().unwrap().is_empty());
    assert!(data["failed"].as_array().unwrap().is_empty());
}

#[test]
fn missing_state_file_exits_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let missing = dir.path().join("nope.json");
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_cmd(
        &repo,
        &["--state-file", missing.to_str().unwrap(), "--add"],
        &stub_dir,
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
fn malformed_state_file_exits_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_file = dir.path().join("state.json");
    fs::write(&state_file, "not json").unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_cmd(
        &repo,
        &["--state-file", state_file.to_str().unwrap(), "--add"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Failed to parse state file"));
}

#[test]
fn gh_spawn_failure_records_as_failed() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_file = dir.path().join("state.json");
    fs::write(&state_file, json!({"prompt": "close #5"}).to_string()).unwrap();
    // No gh stub; empty PATH makes spawn fail.
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("label-issues")
        .args(["--state-file", state_file.to_str().unwrap(), "--add"])
        .current_dir(&repo)
        .env("PATH", "")
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    let failed: Vec<i64> = data["failed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    assert_eq!(failed, vec![5]);
}

#[test]
fn requires_add_or_remove_flag() {
    // clap's ArgGroup rejects invocation without --add or --remove.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_file = dir.path().join("state.json");
    fs::write(&state_file, json!({}).to_string()).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_cmd(
        &repo,
        &["--state-file", state_file.to_str().unwrap()],
        &stub_dir,
    );

    assert_ne!(
        output.status.code(),
        Some(0),
        "Expected non-zero exit without --add or --remove"
    );
}

// --- library-level tests ---

#[test]
fn empty_issue_list_returns_empty_result() {
    // Empty list short-circuits the for loop — no gh spawn occurs.
    let result = label_issues(&[], "add");
    assert_eq!(
        result,
        LabelResult {
            labeled: vec![],
            failed: vec![],
        }
    );
}

#[test]
fn label_constant_is_flow_in_progress() {
    assert_eq!(LABEL, "Flow In-Progress");
}

// --- run_impl_main ---

#[test]
fn label_issues_run_impl_main_missing_state_returns_error_tuple() {
    let args = Args {
        state_file: "/nonexistent/state.json".to_string(),
        add: true,
        remove: false,
    };
    let (value, code) = run_impl_main(args);
    assert_eq!(value["status"], "error");
    assert_eq!(code, 1);
    assert_eq!(value["step"], "read_state");
}

#[test]
fn label_issues_run_impl_main_corrupt_state_returns_error_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = dir.path().join("state.json");
    std::fs::write(&state_file, "{not json").unwrap();
    let args = Args {
        state_file: state_file.to_string_lossy().to_string(),
        add: true,
        remove: false,
    };
    let (value, code) = run_impl_main(args);
    assert_eq!(value["status"], "error");
    assert_eq!(code, 1);
    assert_eq!(value["step"], "parse_state");
}

#[test]
fn label_issues_run_impl_main_no_prompt_returns_ok_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = dir.path().join("state.json");
    std::fs::write(&state_file, r#"{"branch":"test"}"#).unwrap();
    let args = Args {
        state_file: state_file.to_string_lossy().to_string(),
        add: true,
        remove: false,
    };
    let (value, code) = run_impl_main(args);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["labeled"].as_array().unwrap().len(), 0);
    assert_eq!(value["failed"].as_array().unwrap().len(), 0);
}

/// Covers the read_to_string Err arm — state-file path resolves to a
/// directory, so `exists()` passes but `read_to_string` fails with
/// EISDIR.
#[test]
fn label_issues_run_impl_main_state_is_dir_returns_read_error() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = dir.path().join("state.json");
    std::fs::create_dir(&state_file).unwrap();
    let args = Args {
        state_file: state_file.to_string_lossy().to_string(),
        add: true,
        remove: false,
    };
    let (value, code) = run_impl_main(args);
    assert_eq!(value["status"], "error");
    assert_eq!(code, 1);
    assert_eq!(value["step"], "read_state");
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Failed to read state file"));
}
