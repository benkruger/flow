//! Integration tests for start-finalize subcommand.
//!
//! The `run_impl_with_deps` pub seam was removed; tests now drive
//! `run_impl_main(&args, &root)` at the library level for non-Slack
//! branches and use subprocess + curl-stub fixtures to drive each
//! Slack response variant.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use flow_rs::start_finalize::{run_impl_main, Args};
use serde_json::{json, Value};

use common::{flow_states_dir, parse_output};

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

fn create_state_file(repo: &Path, branch: &str, skills_continue: &str) {
    let branch_dir = flow_states_dir(repo).join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
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
            "log": format!(".flow-states/{}/log", branch),
            "state": format!(".flow-states/{}/state.json", branch)
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
            "flow-start": {"continue": skills_continue},
            "flow-plan": {"continue": skills_continue, "dag": "auto"}
        },
        "notifications": [],
        "start_step": 4,
        "start_steps_total": 5
    });
    fs::write(
        branch_dir.join("state.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();
}

fn write_curl_stub(stub_dir: &Path, response: &str) {
    fs::create_dir_all(stub_dir).unwrap();
    let curl_stub = stub_dir.join("curl");
    fs::write(
        &curl_stub,
        format!("#!/bin/bash\nprintf '%s' '{}'\n", response),
    )
    .unwrap();
    fs::set_permissions(&curl_stub, fs::Permissions::from_mode(0o755)).unwrap();
}

fn run_start_finalize_subprocess(
    repo: &Path,
    branch: &str,
    extra_args: &[&str],
    stub_dir: Option<&Path>,
    bot_token: Option<&str>,
) -> Output {
    let mut args = vec!["start-finalize", "--branch", branch];
    args.extend_from_slice(extra_args);

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.args(&args)
        .current_dir(repo)
        .env_remove("FLOW_SIMULATE_BRANCH")
        .env_remove("SLACK_BOT_TOKEN")
        .env_remove("SLACK_CHANNEL");
    if let Some(token) = bot_token {
        cmd.env("CLAUDE_PLUGIN_CONFIG_slack_bot_token", token)
            .env("CLAUDE_PLUGIN_CONFIG_slack_channel", "C12345");
    }
    if let Some(dir) = stub_dir {
        cmd.env(
            "PATH",
            format!(
                "{}:{}",
                dir.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
    }
    cmd.output().unwrap()
}

fn seed_state_library(branch: &str, skills_continue: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let branch_dir = root.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    let state = json!({
        "schema_version": 1,
        "branch": branch,
        "current_phase": "flow-start",
        "phases": {
            "flow-start": {
                "name": "Start",
                "status": "in_progress",
                "session_started_at": "2026-01-01T00:00:00-08:00",
                "cumulative_seconds": 0,
                "visit_count": 1,
            },
            "flow-plan": {"name": "Plan", "status": "pending", "cumulative_seconds": 0, "visit_count": 0},
            "flow-code": {"name": "Code", "status": "pending", "cumulative_seconds": 0, "visit_count": 0},
            "flow-code-review": {"name": "Code Review", "status": "pending", "cumulative_seconds": 0, "visit_count": 0},
            "flow-learn": {"name": "Learn", "status": "pending", "cumulative_seconds": 0, "visit_count": 0},
            "flow-complete": {"name": "Complete", "status": "pending", "cumulative_seconds": 0, "visit_count": 0},
        },
        "skills": {
            "flow-start": {"continue": skills_continue},
            "flow-plan": {"continue": skills_continue, "dag": "auto"},
        },
        "phase_transitions": [],
        "notifications": [],
    });
    fs::write(
        branch_dir.join("state.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();
    (dir, root)
}

// --- Subprocess tests (CLI entry) ---

#[test]
fn test_happy_path_no_slack() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo(dir.path());
    create_state_file(&repo, "finalize-branch", "auto");

    let output = run_start_finalize_subprocess(&repo, "finalize-branch", &[], None, None);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert!(data["formatted_time"].is_string());
    assert!(data["continue_action"].is_string());

    let state_path = flow_states_dir(&repo)
        .join("finalize-branch")
        .join("state.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["phases"]["flow-start"]["status"], "complete");
    assert_eq!(state["current_phase"], "flow-plan");
}

#[test]
fn test_continue_action_auto() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo(dir.path());
    create_state_file(&repo, "auto-branch", "auto");

    let output = run_start_finalize_subprocess(&repo, "auto-branch", &[], None, None);
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["continue_action"], "invoke");
}

#[test]
fn test_continue_action_manual() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo(dir.path());
    create_state_file(&repo, "manual-branch", "manual");

    let output = run_start_finalize_subprocess(&repo, "manual-branch", &[], None, None);
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["continue_action"], "ask");
}

#[test]
fn test_slack_skipped_without_config() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo(dir.path());
    create_state_file(&repo, "slack-branch", "auto");

    let output = run_start_finalize_subprocess(
        &repo,
        "slack-branch",
        &["--pr-url", "https://github.com/test/repo/pull/42"],
        None,
        None,
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert!(
        data.get("slack").is_none() || data["slack"]["status"] == "skipped",
        "Slack should be skipped without config"
    );
}

#[test]
fn test_slack_success_stores_thread_ts() {
    // curl stub returns {"ok":true,"ts":"..."} → slack.status=ok →
    // state stores thread_ts and notification.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo(dir.path());
    create_state_file(&repo, "slack-ok-branch", "auto");
    let stub_dir = repo.join(".stub-bin");
    write_curl_stub(&stub_dir, r#"{"ok": true, "ts": "1234567890.123456"}"#);

    let output = run_start_finalize_subprocess(
        &repo,
        "slack-ok-branch",
        &["--pr-url", "https://github.com/test/repo/pull/42"],
        Some(&stub_dir),
        Some("xoxb-fake-token"),
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert!(data.get("slack").is_some());
    assert_eq!(data["slack"]["status"], "ok");

    let state_path = flow_states_dir(&repo)
        .join("slack-ok-branch")
        .join("state.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["slack_thread_ts"], "1234567890.123456");
    let notifications = state["notifications"].as_array().unwrap();
    assert!(!notifications.is_empty());
    assert_eq!(notifications[0]["phase"], "flow-start");
    assert_eq!(notifications[0]["ts"], "1234567890.123456");
}

#[test]
fn test_slack_ok_without_ts_falls_back_to_empty() {
    // curl stub returns {"ok":true} (no ts) → slack.status=ok →
    // thread_ts stored as empty string.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo(dir.path());
    create_state_file(&repo, "no-ts-branch", "auto");
    let stub_dir = repo.join(".stub-bin");
    write_curl_stub(&stub_dir, r#"{"ok": true}"#);

    let output = run_start_finalize_subprocess(
        &repo,
        "no-ts-branch",
        &["--pr-url", "https://github.com/test/repo/pull/42"],
        Some(&stub_dir),
        Some("xoxb-fake-token"),
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["slack"]["status"], "ok");

    let state_path = flow_states_dir(&repo)
        .join("no-ts-branch")
        .join("state.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["slack_thread_ts"], "");
}

#[test]
fn test_slack_error_continues_best_effort() {
    // curl stub returns {"ok":false,"error":"..."} → slack.status=error
    // → state unchanged, response records slack error.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo(dir.path());
    create_state_file(&repo, "slack-err-branch", "auto");
    let stub_dir = repo.join(".stub-bin");
    write_curl_stub(&stub_dir, r#"{"ok": false, "error": "invalid_auth"}"#);

    let output = run_start_finalize_subprocess(
        &repo,
        "slack-err-branch",
        &["--pr-url", "https://github.com/test/repo/pull/42"],
        Some(&stub_dir),
        Some("xoxb-fake-token"),
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["slack"]["status"], "error");

    let state_path = flow_states_dir(&repo)
        .join("slack-err-branch")
        .join("state.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert!(state.get("slack_thread_ts").is_none());
}

#[test]
fn test_slack_success_heals_wrong_notifications_type() {
    // State file has notifications as string (wrong type). After
    // successful Slack, the mutate closure heals it to array and
    // pushes the notification entry.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo(dir.path());
    create_state_file(&repo, "heal-branch", "auto");
    let state_path = flow_states_dir(&repo)
        .join("heal-branch")
        .join("state.json");
    let mut state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    state["notifications"] = json!("not-an-array");
    fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();

    let stub_dir = repo.join(".stub-bin");
    write_curl_stub(&stub_dir, r#"{"ok": true, "ts": "9.9"}"#);

    let output = run_start_finalize_subprocess(
        &repo,
        "heal-branch",
        &["--pr-url", "https://github.com/test/repo/pull/42"],
        Some(&stub_dir),
        Some("xoxb-fake-token"),
    );

    assert_eq!(output.status.code(), Some(0));
    let healed: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    let arr = healed["notifications"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["ts"], "9.9");
}

// --- Library-level tests (run_impl_main with TempDir fixtures) ---

#[test]
fn test_finalize_missing_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let args = Args {
        branch: "nonexistent-branch".to_string(),
        pr_url: None,
        auto: false,
    };

    let (value, code) = run_impl_main(&args, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert!(value["message"]
        .as_str()
        .unwrap_or("")
        .contains("No state file"));
}

#[test]
fn test_finalize_corrupt_state_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let branch_dir = root.join(".flow-states").join("corrupt-branch");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), "not json{{{").unwrap();

    let args = Args {
        branch: "corrupt-branch".to_string(),
        pr_url: None,
        auto: false,
    };

    let (value, code) = run_impl_main(&args, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert!(value["message"]
        .as_str()
        .unwrap_or("")
        .contains("State mutation failed"));
}

#[test]
fn test_finalize_no_pr_url_skips_slack() {
    let (_dir, root) = seed_state_library("no-url-branch", "auto");
    let args = Args {
        branch: "no-url-branch".to_string(),
        pr_url: None,
        auto: false,
    };

    let (value, code) = run_impl_main(&args, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert!(value.get("slack").is_none());

    let state_path = root.join(".flow-states/no-url-branch/state.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert!(state.get("slack_thread_ts").is_none());
}

#[test]
fn test_finalize_happy_wraps_with_exit_zero() {
    let (_dir, root) = seed_state_library("happy-branch", "auto");
    let args = Args {
        branch: "happy-branch".to_string(),
        pr_url: None,
        auto: false,
    };

    let (value, code) = run_impl_main(&args, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
}

#[test]
fn test_finalize_with_invalid_frozen_phases_falls_back() {
    let (_dir, root) = seed_state_library("invalid-frozen-branch", "auto");
    let frozen_path = root.join(".flow-states/invalid-frozen-branch/phases.json");
    fs::write(&frozen_path, r#"{"order": []}"#).unwrap();

    let args = Args {
        branch: "invalid-frozen-branch".to_string(),
        pr_url: None,
        auto: false,
    };

    let (value, code) = run_impl_main(&args, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
}

#[test]
fn test_finalize_with_frozen_phases_loads_config() {
    let (_dir, root) = seed_state_library("frozen-branch", "auto");
    let frozen_path = root.join(".flow-states/frozen-branch/phases.json");
    let frozen = json!({
        "order": ["flow-start", "flow-plan", "flow-code", "flow-code-review", "flow-learn", "flow-complete"],
        "phases": {
            "flow-start": {"name": "Start", "command": "/flow:flow-start"},
            "flow-plan": {"name": "Plan", "command": "/flow:flow-plan"},
            "flow-code": {"name": "Code", "command": "/flow:flow-code"},
            "flow-code-review": {"name": "Code Review", "command": "/flow:flow-code-review"},
            "flow-learn": {"name": "Learn", "command": "/flow:flow-learn"},
            "flow-complete": {"name": "Complete", "command": "/flow:flow-complete"}
        }
    });
    fs::write(&frozen_path, serde_json::to_string_pretty(&frozen).unwrap()).unwrap();

    let args = Args {
        branch: "frozen-branch".to_string(),
        pr_url: None,
        auto: false,
    };

    let (value, code) = run_impl_main(&args, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
}

#[test]
fn test_finalize_pr_url_skipped_slack_without_env() {
    // run_impl_main with pr_url=Some but SLACK env vars unset →
    // notify_slack::notify returns skipped → slack field absent.
    let (_dir, root) = seed_state_library("real-notifier-branch", "auto");
    let args = Args {
        branch: "real-notifier-branch".to_string(),
        pr_url: Some("https://github.com/test/repo/pull/42".to_string()),
        auto: false,
    };

    let (value, code) = run_impl_main(&args, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
}
