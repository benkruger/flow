//! Integration tests for start-finalize subcommand.
//!
//! start-finalize consolidates: phase-transition complete + notify-slack +
//! set-timestamp + add-notification into a single command.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

use std::os::unix::fs::PermissionsExt;

use common::{flow_states_dir, parse_output};

// --- Test helpers ---

/// Create a minimal git repo (no remote needed for finalize).
fn create_git_repo(parent: &Path) -> PathBuf {
    let repo = parent.join("repo");
    fs::create_dir_all(&repo).unwrap();

    Command::new("git")
        .args(["-c", "init.defaultBranch=main", "init"])
        .current_dir(&repo)
        .output()
        .unwrap();

    for (key, val) in [
        ("user.email", "test@test.com"),
        ("user.name", "Test"),
        ("commit.gpgsign", "false"),
    ] {
        Command::new("git")
            .args(["config", key, val])
            .current_dir(&repo)
            .output()
            .unwrap();
    }

    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&repo)
        .output()
        .unwrap();

    repo
}

/// Create a state file with flow-start in_progress (ready for completion).
fn create_state_file(repo: &Path, branch: &str, skills_continue: &str) {
    let state_dir = flow_states_dir(repo);
    fs::create_dir_all(&state_dir).unwrap();
    let state = json!({
        "schema_version": 1,
        "branch": branch,
        "repo": "test/repo",
        "pr_number": 42,
        "pr_url": "https://github.com/test/repo/pull/42",
        "started_at": "2026-01-01T00:00:00-08:00",
        "current_phase": "flow-start",
        "files": {
            "plan": null,
            "dag": null,
            "log": format!(".flow-states/{}.log", branch),
            "state": format!(".flow-states/{}.json", branch)
        },
        "session_tty": null,
        "session_id": null,
        "transcript_path": null,
        "notes": [],
        "prompt": "test feature",
        "phases": {
            "flow-start": {
                "name": "Start",
                "status": "in_progress",
                "started_at": "2026-01-01T00:00:00-08:00",
                "completed_at": null,
                "session_started_at": "2026-01-01T00:00:00-08:00",
                "cumulative_seconds": 0,
                "visit_count": 1
            },
            "flow-plan": {
                "name": "Plan",
                "status": "pending",
                "started_at": null,
                "completed_at": null,
                "session_started_at": null,
                "cumulative_seconds": 0,
                "visit_count": 0
            },
            "flow-code": {
                "name": "Code",
                "status": "pending",
                "started_at": null,
                "completed_at": null,
                "session_started_at": null,
                "cumulative_seconds": 0,
                "visit_count": 0
            },
            "flow-code-review": {
                "name": "Code Review",
                "status": "pending",
                "started_at": null,
                "completed_at": null,
                "session_started_at": null,
                "cumulative_seconds": 0,
                "visit_count": 0
            },
            "flow-learn": {
                "name": "Learn",
                "status": "pending",
                "started_at": null,
                "completed_at": null,
                "session_started_at": null,
                "cumulative_seconds": 0,
                "visit_count": 0
            },
            "flow-complete": {
                "name": "Complete",
                "status": "pending",
                "started_at": null,
                "completed_at": null,
                "session_started_at": null,
                "cumulative_seconds": 0,
                "visit_count": 0
            }
        },
        "phase_transitions": [],
        "skills": {
            "flow-start": {
                "continue": skills_continue
            },
            "flow-plan": {
                "continue": skills_continue,
                "dag": "auto"
            }
        },
        "start_step": 4,
        "start_steps_total": 5
    });
    fs::write(
        state_dir.join(format!("{}.json", branch)),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();
}

/// Run flow-rs start-finalize.
fn run_start_finalize(repo: &Path, branch: &str, extra_args: &[&str]) -> Output {
    let mut args = vec!["start-finalize", "--branch", branch];
    args.extend_from_slice(extra_args);

    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(&args)
        .current_dir(repo)
        .env_remove("FLOW_SIMULATE_BRANCH")
        .env_remove("SLACK_BOT_TOKEN")
        .env_remove("SLACK_CHANNEL")
        .output()
        .unwrap()
}

// --- Tests ---

#[test]
fn test_happy_path_no_slack() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo(dir.path());
    create_state_file(&repo, "finalize-branch", "auto");

    let output = run_start_finalize(&repo, "finalize-branch", &[]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert!(
        data["formatted_time"].is_string(),
        "Must include formatted_time"
    );
    assert!(
        data["continue_action"].is_string(),
        "Must include continue_action"
    );

    // State should be updated
    let state_path = flow_states_dir(&repo).join("finalize-branch.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["phases"]["flow-start"]["status"], "complete");
    assert_eq!(state["current_phase"], "flow-plan");
}

#[test]
fn test_continue_action_auto() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo(dir.path());
    create_state_file(&repo, "auto-branch", "auto");

    let output = run_start_finalize(&repo, "auto-branch", &[]);
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(
        data["continue_action"], "invoke",
        "Auto mode should return continue_action=invoke"
    );
}

#[test]
fn test_continue_action_manual() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo(dir.path());
    create_state_file(&repo, "manual-branch", "manual");

    let output = run_start_finalize(&repo, "manual-branch", &[]);
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(
        data["continue_action"], "ask",
        "Manual mode should return continue_action=ask"
    );
}

#[test]
fn test_slack_skipped_without_config() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo(dir.path());
    create_state_file(&repo, "slack-branch", "auto");

    let output = run_start_finalize(
        &repo,
        "slack-branch",
        &["--pr-url", "https://github.com/test/repo/pull/42"],
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    // Slack should be skipped (no SLACK_BOT_TOKEN env var)
    assert!(
        data.get("slack").is_none() || data["slack"]["status"] == "skipped",
        "Slack should be skipped without config"
    );
}

#[test]
fn test_finalize_missing_state_file() {
    // Exercises lines 48-52: state file does not exist → status=error.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo(dir.path());
    // Do NOT create a state file

    let output = run_start_finalize(&repo, "nonexistent-branch", &[]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .contains("No state file"),
        "error should mention missing state file, got: {}",
        data["message"]
    );
}

#[test]
fn test_finalize_corrupt_state_returns_error() {
    // Exercises lines 83-90: mutate_state fails on corrupt JSON → status=error.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo(dir.path());
    let state_dir = flow_states_dir(&repo);
    fs::create_dir_all(&state_dir).unwrap();
    // Write corrupt content that exists but is not valid JSON
    fs::write(state_dir.join("corrupt-branch.json"), "not json{{{").unwrap();

    let output = run_start_finalize(&repo, "corrupt-branch", &[]);
    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .contains("State mutation failed"),
        "error should mention mutation failure, got: {}",
        data["message"]
    );
}

// Note: lines 103-107 (phase_complete error guard) are defensive dead
// code — phase_complete() in phase_transition.rs always returns
// status="ok". The guard protects against a future change to
// phase_complete that introduces an error return. No test can trigger
// this path without modifying phase_complete itself.

#[test]
fn test_slack_success_stores_thread_ts() {
    // Exercises lines 132-160: Slack success path stores thread_ts
    // and appends to notifications[]. Uses a fake curl stub that
    // returns a valid Slack response.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo(dir.path());
    create_state_file(&repo, "slack-ok-branch", "auto");

    // Create a curl stub that returns a valid Slack response
    let stub_dir = repo.join(".stub-bin");
    fs::create_dir_all(&stub_dir).unwrap();
    let curl_stub = stub_dir.join("curl");
    fs::write(
        &curl_stub,
        "#!/bin/bash\necho '{\"ok\": true, \"ts\": \"1234567890.123456\"}'",
    )
    .unwrap();
    fs::set_permissions(&curl_stub, fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "start-finalize",
            "--branch",
            "slack-ok-branch",
            "--pr-url",
            "https://github.com/test/repo/pull/42",
        ])
        .current_dir(&repo)
        .env_remove("FLOW_SIMULATE_BRANCH")
        .env("CLAUDE_PLUGIN_CONFIG_slack_bot_token", "xoxb-fake-token")
        .env("CLAUDE_PLUGIN_CONFIG_slack_channel", "C12345")
        .env(
            "PATH",
            format!(
                "{}:{}",
                stub_dir.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    // The response should include the slack field since it wasn't skipped
    assert!(
        data.get("slack").is_some(),
        "Response should include slack field when Slack call succeeds"
    );
    assert_eq!(data["slack"]["status"], "ok");

    // Check state file for thread_ts and notifications
    let state_path = flow_states_dir(&repo).join("slack-ok-branch.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(
        state["slack_thread_ts"], "1234567890.123456",
        "thread_ts should be stored in state"
    );
    let notifications = state["notifications"].as_array();
    assert!(
        notifications.is_some() && !notifications.unwrap().is_empty(),
        "notifications[] should be populated"
    );
    assert_eq!(notifications.unwrap()[0]["phase"], "flow-start");
    assert_eq!(notifications.unwrap()[0]["ts"], "1234567890.123456");
}
