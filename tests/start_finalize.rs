//! Integration tests for start-finalize subcommand.
//!
//! start-finalize consolidates: phase-transition complete + notify-slack +
//! set-timestamp + add-notification into a single command.

mod common;

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use flow_rs::notify_slack;
use flow_rs::start_finalize::{run_impl_main, run_impl_with_deps, Args};
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

// --- run_impl_with_deps (library-level unit tests) ---

fn seed_state(branch: &str, skills_continue: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let state_dir = root.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
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
        state_dir.join(format!("{}.json", branch)),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();
    (dir, root)
}

fn panicking_notifier(_args: &notify_slack::Args) -> Value {
    panic!("notifier must not be called when pr_url is None");
}

#[test]
fn finalize_no_pr_url_skips_slack() {
    let (_dir, root) = seed_state("no-url-branch", "auto");
    let args = Args {
        branch: "no-url-branch".to_string(),
        pr_url: None,
        auto: false,
    };

    let result = run_impl_with_deps(&args, &root, &panicking_notifier);
    assert_eq!(result["status"], "ok");
    assert!(result.get("slack").is_none());

    let state_path = root.join(".flow-states/no-url-branch.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert!(state.get("slack_thread_ts").is_none());
}

#[test]
fn finalize_notifier_skipped_leaves_state_untouched() {
    let (_dir, root) = seed_state("skipped-branch", "auto");
    let args = Args {
        branch: "skipped-branch".to_string(),
        pr_url: Some("https://github.com/test/repo/pull/42".to_string()),
        auto: false,
    };
    let notifier = |_: &notify_slack::Args| -> Value { json!({"status": "skipped"}) };

    let result = run_impl_with_deps(&args, &root, &notifier);
    assert_eq!(result["status"], "ok");
    assert!(result.get("slack").is_none());

    let state_path = root.join(".flow-states/skipped-branch.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert!(state.get("slack_thread_ts").is_none());
    assert!(state["notifications"].as_array().unwrap().is_empty());
}

#[test]
fn finalize_notifier_ok_writes_thread_ts_and_notification() {
    let (_dir, root) = seed_state("ok-branch", "auto");
    let args = Args {
        branch: "ok-branch".to_string(),
        pr_url: Some("https://github.com/test/repo/pull/42".to_string()),
        auto: false,
    };
    let notifier = |_: &notify_slack::Args| -> Value { json!({"status": "ok", "ts": "1234.5678"}) };

    let result = run_impl_with_deps(&args, &root, &notifier);
    assert_eq!(result["status"], "ok");
    assert_eq!(result["slack"]["status"], "ok");
    assert_eq!(result["slack"]["ts"], "1234.5678");

    let state_path = root.join(".flow-states/ok-branch.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["slack_thread_ts"], "1234.5678");
    let notifications = state["notifications"].as_array().unwrap();
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0]["phase"], "flow-start");
    assert_eq!(notifications[0]["ts"], "1234.5678");
    assert_eq!(notifications[0]["thread_ts"], "1234.5678");
}

#[test]
fn finalize_notifier_error_continues_best_effort() {
    let (_dir, root) = seed_state("err-branch", "auto");
    let args = Args {
        branch: "err-branch".to_string(),
        pr_url: Some("https://github.com/test/repo/pull/42".to_string()),
        auto: false,
    };
    let notifier =
        |_: &notify_slack::Args| -> Value { json!({"status": "error", "message": "curl failed"}) };

    let result = run_impl_with_deps(&args, &root, &notifier);
    assert_eq!(result["status"], "ok");
    assert_eq!(result["slack"]["status"], "error");

    let state_path = root.join(".flow-states/err-branch.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert!(state.get("slack_thread_ts").is_none());
}

#[test]
fn finalize_notifier_ok_with_wrong_notifications_type_heals() {
    let (_dir, root) = seed_state("heal-branch", "auto");
    let state_path = root.join(".flow-states/heal-branch.json");
    let mut state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    state["notifications"] = json!("not-an-array");
    fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();

    let args = Args {
        branch: "heal-branch".to_string(),
        pr_url: Some("https://github.com/test/repo/pull/42".to_string()),
        auto: false,
    };
    let notifier = |_: &notify_slack::Args| -> Value { json!({"status": "ok", "ts": "9.9"}) };

    let result = run_impl_with_deps(&args, &root, &notifier);
    assert_eq!(result["status"], "ok");

    let healed: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    let arr = healed["notifications"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["ts"], "9.9");
}

#[test]
fn finalize_missing_state_returns_error_with_deps() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let args = Args {
        branch: "nope-branch".to_string(),
        pr_url: None,
        auto: false,
    };
    let result = run_impl_with_deps(&args, &root, &panicking_notifier);
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("No state file"));
}

#[test]
fn finalize_corrupt_state_returns_error_with_deps() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let state_dir = root.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("corrupt-branch.json"), "not json{{{").unwrap();

    let args = Args {
        branch: "corrupt-branch".to_string(),
        pr_url: None,
        auto: false,
    };
    let result = run_impl_with_deps(&args, &root, &panicking_notifier);
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("State mutation failed"));
}

/// Covers the `load_phase_config().ok() == None` branch: the frozen
/// file exists but has invalid schema (missing `phases` key). The
/// `.ok()` converts Err to None, and production proceeds with
/// frozen_config=None.
#[test]
fn finalize_with_invalid_frozen_phases_falls_back() {
    let (_dir, root) = seed_state("invalid-frozen-branch", "auto");
    let frozen_path = root.join(".flow-states/invalid-frozen-branch-phases.json");
    fs::write(&frozen_path, "{\"order\": []}").unwrap(); // missing "phases"

    let args = Args {
        branch: "invalid-frozen-branch".to_string(),
        pr_url: None,
        auto: false,
    };
    let result = run_impl_with_deps(&args, &root, &panicking_notifier);
    assert_eq!(result["status"], "ok");
}

/// Covers the `frozen_path.exists() == true` branch: seed a
/// `.flow-states/frozen-phases.json` so `run_impl_with_deps` loads
/// the frozen phase config.
#[test]
fn finalize_with_frozen_phases_loads_config() {
    let (_dir, root) = seed_state("frozen-branch", "auto");
    let frozen_path = root.join(".flow-states/frozen-branch-phases.json");
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
    let result = run_impl_with_deps(&args, &root, &panicking_notifier);
    assert_eq!(result["status"], "ok");
}

/// Covers the `slack_result["ts"].as_str().unwrap_or("")` None
/// branch: notifier returns `{"status":"ok"}` without a `ts` field.
/// The empty-string fallback flows through and `slack_thread_ts`
/// becomes empty rather than panicking.
#[test]
fn finalize_notifier_ok_without_ts_falls_back_to_empty() {
    let (_dir, root) = seed_state("no-ts-branch", "auto");
    let args = Args {
        branch: "no-ts-branch".to_string(),
        pr_url: Some("https://github.com/test/repo/pull/42".to_string()),
        auto: false,
    };
    let notifier = |_: &notify_slack::Args| -> Value { json!({"status": "ok"}) };

    let result = run_impl_with_deps(&args, &root, &notifier);
    assert_eq!(result["status"], "ok");
    assert_eq!(result["slack"]["status"], "ok");

    let state_path = root.join(".flow-states/no-ts-branch.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["slack_thread_ts"], "");
}

#[test]
fn finalize_with_deps_notifier_called_once() {
    let (_dir, root) = seed_state("call-count-branch", "auto");
    let args = Args {
        branch: "call-count-branch".to_string(),
        pr_url: Some("https://github.com/test/repo/pull/42".to_string()),
        auto: false,
    };
    let calls: RefCell<usize> = RefCell::new(0);
    let notifier = |_: &notify_slack::Args| -> Value {
        *calls.borrow_mut() += 1;
        json!({"status": "ok", "ts": "42.0"})
    };

    let _ = run_impl_with_deps(&args, &root, &notifier);
    assert_eq!(*calls.borrow(), 1);
}

// --- run_impl_main ---

#[test]
fn finalize_run_impl_main_err_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let args = Args {
        branch: "main-err-branch".to_string(),
        pr_url: None,
        auto: false,
    };
    let (v, code) = run_impl_main(&args, &root);
    assert_eq!(code, 0);
    assert_eq!(v["status"], "error");
    assert!(v["message"]
        .as_str()
        .unwrap_or("")
        .contains("No state file found"));
}

#[test]
fn finalize_run_impl_main_happy_wraps_with_exit_zero() {
    let (_dir, root) = seed_state("happy-main-branch", "auto");
    let args = Args {
        branch: "happy-main-branch".to_string(),
        pr_url: None,
        auto: false,
    };
    let (v, code) = run_impl_main(&args, &root);
    assert_eq!(code, 0);
    assert_eq!(v["status"], "ok");
}

/// Exercises `run_impl_main` with `pr_url=Some`, covering the real
/// `notify_slack::notify` binding. When `SLACK_WEBHOOK_URL` is unset
/// in the test env, `notify` returns `{"status":"skipped"}` without
/// network I/O — the response's slack field is omitted per the
/// `status != "skipped"` filter.
#[test]
fn finalize_run_impl_main_pr_url_threads_to_real_notifier() {
    let (_dir, root) = seed_state("real-notifier-branch", "auto");
    let args = Args {
        branch: "real-notifier-branch".to_string(),
        pr_url: Some("https://github.com/test/repo/pull/42".to_string()),
        auto: false,
    };
    let (v, code) = run_impl_main(&args, &root);
    assert_eq!(code, 0);
    assert_eq!(v["status"], "ok");
}
