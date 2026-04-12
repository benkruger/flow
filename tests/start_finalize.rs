//! Integration tests for start-finalize subcommand.
//!
//! start-finalize consolidates: phase-transition complete + notify-slack +
//! set-timestamp + add-notification into a single command.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

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
