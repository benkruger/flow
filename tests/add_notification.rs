//! Integration tests for `bin/flow add-notification` and its library surface.
//!
//! Migrated from inline `#[cfg(test)]` per
//! `.claude/rules/test-placement.md`.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_git_repo_with_remote, parse_output};
use flow_rs::add_notification::{run_impl_main, truncate_preview, Args};
use flow_rs::lock::mutate_state;
use flow_rs::phase_config::phase_names;
use flow_rs::utils::now;
use serde_json::{json, Value};

fn write_state(repo: &Path, branch: &str, state: &Value) -> std::path::PathBuf {
    let branch_dir = repo.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    let path = branch_dir.join("state.json");
    fs::write(&path, serde_json::to_string_pretty(state).unwrap()).unwrap();
    path
}

fn run_add_notification(repo: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("add-notification")
        .args(args)
        .current_dir(repo)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

#[test]
fn add_notification_records_entry() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({"branch": "b", "slack_notifications": []});
    let state_path = write_state(&repo, "b", &state);

    let output = run_add_notification(
        &repo,
        &[
            "--phase",
            "flow-code",
            "--ts",
            "1234.5678",
            "--thread-ts",
            "1234.0000",
            "--message",
            "Phase 3 complete",
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
    assert_eq!(data["notification_count"], 1);

    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    let notes = on_disk["slack_notifications"].as_array().unwrap();
    assert_eq!(notes[0]["phase"], "flow-code");
    assert_eq!(notes[0]["phase_name"], "Code");
    assert_eq!(notes[0]["ts"], "1234.5678");
    assert_eq!(notes[0]["thread_ts"], "1234.0000");
    assert_eq!(notes[0]["message_preview"], "Phase 3 complete");
}

#[test]
fn add_notification_truncates_long_message() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({"branch": "t", "slack_notifications": []});
    let state_path = write_state(&repo, "t", &state);

    let long_message = "x".repeat(150);
    let output = run_add_notification(
        &repo,
        &[
            "--phase",
            "flow-code",
            "--ts",
            "1",
            "--thread-ts",
            "0",
            "--message",
            &long_message,
            "--branch",
            "t",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    let preview = on_disk["slack_notifications"][0]["message_preview"]
        .as_str()
        .unwrap();
    // MAX_PREVIEW_LENGTH = 100 chars + "..." = 103 chars
    assert!(preview.ends_with("..."));
    assert_eq!(preview.chars().count(), 103);
}

#[test]
fn add_notification_no_state_file_returns_no_state() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());

    let output = run_add_notification(
        &repo,
        &[
            "--phase",
            "flow-code",
            "--ts",
            "1",
            "--thread-ts",
            "0",
            "--message",
            "m",
            "--branch",
            "missing",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "no_state");
}

#[test]
fn add_notification_creates_array_if_missing() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({"branch": "c"});
    let state_path = write_state(&repo, "c", &state);

    let output = run_add_notification(
        &repo,
        &[
            "--phase",
            "flow-code",
            "--ts",
            "1",
            "--thread-ts",
            "0",
            "--message",
            "first",
            "--branch",
            "c",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(on_disk["slack_notifications"].as_array().unwrap().len(), 1);
}

#[test]
fn add_notification_appends_multiple() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({"branch": "m", "slack_notifications": []});
    let state_path = write_state(&repo, "m", &state);

    for i in 1..=3 {
        let msg = format!("msg {}", i);
        let output = run_add_notification(
            &repo,
            &[
                "--phase",
                "flow-code",
                "--ts",
                &format!("{}.0", i),
                "--thread-ts",
                "0",
                "--message",
                &msg,
                "--branch",
                "m",
            ],
        );
        let data = parse_output(&output);
        assert_eq!(data["notification_count"], i);
    }

    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(on_disk["slack_notifications"].as_array().unwrap().len(), 3);
}

#[test]
fn add_notification_no_branch_no_git_returns_branch_resolution_error() {
    // Subprocess cwd is a non-git tempdir AND no --branch override.
    // resolve_branch falls back to current_branch() which returns None
    // for non-git dirs → run_impl_main surfaces the branch error.
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("add-notification")
        .args([
            "--phase",
            "flow-code",
            "--ts",
            "1",
            "--thread-ts",
            "0",
            "--message",
            "m",
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
fn add_notification_corrupt_state_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let branch_dir = repo.join(".flow-states").join("bad");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), "{corrupt").unwrap();

    let output = run_add_notification(
        &repo,
        &[
            "--phase",
            "flow-code",
            "--ts",
            "1",
            "--thread-ts",
            "0",
            "--message",
            "m",
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
            .contains("Failed to add notification"),
        "Error should mention the operation that failed"
    );
}

// --- Library-level tests (migrated from inline `#[cfg(test)]`) ---

fn make_state_lib(branch: &str) -> Value {
    json!({
        "schema_version": 1,
        "branch": branch,
        "current_phase": "flow-code",
        "slack_notifications": []
    })
}

fn write_state_lib(dir: &Path, branch: &str, state: &Value) -> std::path::PathBuf {
    let branch_dir = dir.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    let path = branch_dir.join("state.json");
    fs::write(&path, serde_json::to_string_pretty(state).unwrap()).unwrap();
    path
}

fn make_args(branch: Option<&str>) -> Args {
    Args {
        phase: "flow-code".to_string(),
        ts: "5555555555.555555".to_string(),
        thread_ts: "1111111111.111111".to_string(),
        message: "test message".to_string(),
        branch: branch.map(|s| s.to_string()),
    }
}

#[test]
fn add_notification_to_empty_array_lib() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_state_lib("test-feature");
    let path = write_state_lib(dir.path(), "test-feature", &state);

    let result = mutate_state(&path, &mut |s| {
        let names = phase_names();
        let phase = "flow-code";
        let phase_name = names.get(phase).cloned().unwrap_or_default();
        s["slack_notifications"]
            .as_array_mut()
            .expect("array in fixture")
            .push(json!({
                "phase": phase,
                "phase_name": phase_name,
                "ts": "5555555555.555555",
                "thread_ts": "1111111111.111111",
                "message_preview": "short msg",
                "timestamp": now(),
            }));
    })
    .unwrap();

    let notifs = result["slack_notifications"].as_array().unwrap();
    assert_eq!(notifs.len(), 1);
    assert_eq!(notifs[0]["phase"], "flow-code");
    assert_eq!(notifs[0]["phase_name"], "Code");
    assert!(notifs[0]["timestamp"].as_str().unwrap().contains("T"));
}

#[test]
fn add_notification_preserves_existing_lib() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_state_lib("test-feature");
    state["slack_notifications"] = json!([{"phase": "flow-start", "message_preview": "existing"}]);
    let path = write_state_lib(dir.path(), "test-feature", &state);

    mutate_state(&path, &mut |s| {
        s["slack_notifications"]
            .as_array_mut()
            .expect("array in fixture")
            .push(json!({"phase": "flow-code", "message_preview": "new"}));
    })
    .unwrap();

    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let notifs = on_disk["slack_notifications"].as_array().unwrap();
    assert_eq!(notifs.len(), 2);
    assert_eq!(notifs[0]["message_preview"], "existing");
    assert_eq!(notifs[1]["message_preview"], "new");
}

#[test]
fn truncate_preview_short_message_lib() {
    assert_eq!(truncate_preview("hello"), "hello");
}

#[test]
fn truncate_preview_exactly_100_chars_lib() {
    let msg = "a".repeat(100);
    assert_eq!(truncate_preview(&msg), msg);
}

#[test]
fn truncate_preview_over_100_chars_lib() {
    let msg = "a".repeat(150);
    let result = truncate_preview(&msg);
    assert_eq!(result.len(), 103);
    assert!(result.ends_with("..."));
    assert_eq!(&result[..100], &msg[..100]);
}

#[test]
fn truncate_preview_101_chars_lib() {
    let msg = "a".repeat(101);
    let result = truncate_preview(&msg);
    assert_eq!(result.len(), 103);
    assert!(result.ends_with("..."));
}

#[test]
fn add_notification_creates_array_if_missing_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("test-feature");
    fs::create_dir_all(&branch_dir).unwrap();
    let path = branch_dir.join("state.json");
    fs::write(&path, r#"{"current_phase": "flow-code"}"#).unwrap();

    let args = Args {
        phase: "flow-code".to_string(),
        ts: "1234.5678".to_string(),
        thread_ts: "1234.0000".to_string(),
        message: "test".to_string(),
        branch: Some("test-feature".to_string()),
    };

    let (value, code) = run_impl_main(args, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["notification_count"], 1);
}

#[test]
fn add_notification_array_root_state_noop_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("test-feature");
    fs::create_dir_all(&branch_dir).unwrap();
    let path = branch_dir.join("state.json");
    fs::write(&path, "[1, 2, 3]").unwrap();

    let args = Args {
        phase: "flow-code".to_string(),
        ts: "1234.5678".to_string(),
        thread_ts: "1234.0000".to_string(),
        message: "should not appear".to_string(),
        branch: Some("test-feature".to_string()),
    };

    let (value, code) = run_impl_main(args, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["notification_count"], 0);
}

#[test]
fn add_notification_run_impl_main_no_state_returns_no_state_tuple_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let args = make_args(Some("missing-branch"));
    let (value, code) = run_impl_main(args, &root);
    assert_eq!(value["status"], "no_state");
    assert_eq!(code, 0);
}

#[test]
fn add_notification_run_impl_main_success_returns_count_tuple_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("present-branch");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(
        branch_dir.join("state.json"),
        r#"{"current_phase":"flow-code","slack_notifications":[]}"#,
    )
    .unwrap();
    let args = make_args(Some("present-branch"));
    let (value, code) = run_impl_main(args, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["notification_count"], 1);
}

#[test]
fn add_notification_run_impl_main_mutate_state_failure_returns_error_tuple_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("present-branch");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), "{not json").unwrap();
    let args = make_args(Some("present-branch"));
    let (value, code) = run_impl_main(args, &root);
    assert_eq!(value["status"], "error");
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Failed to add notification"));
}

#[test]
fn add_notification_run_impl_main_unknown_phase_falls_back_to_phase_string_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("unknown-phase");
    fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    fs::write(
        &state_path,
        r#"{"current_phase":"flow-code","slack_notifications":[]}"#,
    )
    .unwrap();
    let mut args = make_args(Some("unknown-phase"));
    args.phase = "custom-unknown-phase".to_string();
    let (value, code) = run_impl_main(args, &root);
    assert_eq!(value["status"], "ok");
    assert_eq!(code, 0);
    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(
        on_disk["slack_notifications"][0]["phase_name"],
        "custom-unknown-phase"
    );
}

#[test]
fn add_notification_run_impl_main_wrong_type_resets_to_array_lib() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("wrong-type");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(
        branch_dir.join("state.json"),
        r#"{"current_phase":"flow-code","slack_notifications":"not-an-array"}"#,
    )
    .unwrap();
    let args = make_args(Some("wrong-type"));
    let (value, code) = run_impl_main(args, &root);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["notification_count"], 1);
    assert_eq!(code, 0);
}

#[test]
fn add_notification_run_impl_main_slash_branch_returns_structured_error_no_panic() {
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
