//! Integration tests for `bin/flow add-notification`.
//!
//! Records a Slack notification in the branch's state file.

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
fn add_notification_corrupt_state_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("bad.json"), "{corrupt").unwrap();

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
