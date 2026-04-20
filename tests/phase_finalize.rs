//! Integration tests for phase-finalize subcommand.
//!
//! phase-finalize consolidates: phase_complete() + Slack notification +
//! add-notification into a single command parameterized by --phase.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::flow_states_dir;
use flow_rs::notify_slack;
use flow_rs::phase_finalize::{run_impl_with_deps, Args};
use serde_json::{json, Value};

// --- Test helpers ---

/// Create a minimal git repo.
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

/// Create a state file with a specified phase in_progress.
fn create_state(repo: &Path, branch: &str, current_phase: &str, skills_continue: &str) {
    let state_dir = flow_states_dir(repo);
    fs::create_dir_all(&state_dir).unwrap();

    let state = json!({
        "schema_version": 1,
        "branch": branch,
        "repo": "test/repo",
        "pr_number": 42,
        "pr_url": "https://github.com/test/repo/pull/42",
        "started_at": "2026-01-01T00:00:00-08:00",
        "current_phase": current_phase,
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
                "status": if current_phase == "flow-start" { "in_progress" } else { "complete" },
                "started_at": "2026-01-01T00:00:00-08:00",
                "completed_at": if current_phase == "flow-start" { None } else { Some("2026-01-01T00:01:00-08:00") },
                "session_started_at": "2026-01-01T00:00:00-08:00",
                "cumulative_seconds": 0,
                "visit_count": 1
            },
            "flow-plan": {
                "name": "Plan",
                "status": if current_phase == "flow-plan" { "in_progress" } else if current_phase == "flow-start" { "pending" } else { "complete" },
                "started_at": null,
                "completed_at": null,
                "session_started_at": null,
                "cumulative_seconds": 0,
                "visit_count": 0
            },
            "flow-code": {
                "name": "Code",
                "status": if current_phase == "flow-code" { "in_progress" } else { "pending" },
                "started_at": if current_phase == "flow-code" { Some("2026-01-01T00:02:00-08:00") } else { None },
                "completed_at": null,
                "session_started_at": if current_phase == "flow-code" { Some("2026-01-01T00:02:00-08:00") } else { None },
                "cumulative_seconds": 0,
                "visit_count": if current_phase == "flow-code" { 1 } else { 0 }
            },
            "flow-code-review": {
                "name": "Code Review",
                "status": if current_phase == "flow-code-review" { "in_progress" } else { "pending" },
                "started_at": if current_phase == "flow-code-review" { Some("2026-01-01T00:03:00-08:00") } else { None },
                "completed_at": null,
                "session_started_at": if current_phase == "flow-code-review" { Some("2026-01-01T00:03:00-08:00") } else { None },
                "cumulative_seconds": 0,
                "visit_count": if current_phase == "flow-code-review" { 1 } else { 0 }
            },
            "flow-learn": {
                "name": "Learn",
                "status": if current_phase == "flow-learn" { "in_progress" } else { "pending" },
                "started_at": if current_phase == "flow-learn" { Some("2026-01-01T00:04:00-08:00") } else { None },
                "completed_at": null,
                "session_started_at": if current_phase == "flow-learn" { Some("2026-01-01T00:04:00-08:00") } else { None },
                "cumulative_seconds": 0,
                "visit_count": if current_phase == "flow-learn" { 1 } else { 0 }
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
            "flow-code": {
                "commit": skills_continue,
                "continue": skills_continue
            },
            "flow-code-review": {
                "commit": skills_continue,
                "continue": skills_continue
            },
            "flow-learn": {
                "commit": skills_continue,
                "continue": skills_continue
            }
        },
    });
    fs::write(
        state_dir.join(format!("{}.json", branch)),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();
}

/// Run flow-rs phase-finalize.
fn run_phase_finalize(repo: &Path, extra_args: &[&str]) -> Output {
    let mut args = vec!["phase-finalize"];
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

/// Parse JSON from the last line of stdout.
fn parse_output(output: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last_line = stdout.trim().lines().last().unwrap_or("");
    serde_json::from_str(last_line).unwrap_or_else(|_| json!({"raw": stdout.trim()}))
}

// --- Tests ---

#[test]
fn test_learn_with_slack_reply_skipped() {
    // thread-ts provided but no Slack config → Slack skipped, phase still completes
    let dir = tempfile::tempdir().unwrap();
    let branch = "learn-slack";
    let repo = create_git_repo(dir.path());
    create_state(&repo, branch, "flow-learn", "auto");

    let output = run_phase_finalize(
        &repo,
        &[
            "--phase",
            "flow-learn",
            "--branch",
            branch,
            "--thread-ts",
            "1234567890.123456",
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
    assert!(data["formatted_time"].is_string());
    assert!(data["continue_action"].is_string());

    // State should be updated — phase completed
    let state_path = flow_states_dir(&repo).join(format!("{}.json", branch));
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["phases"]["flow-learn"]["status"], "complete");
}

#[test]
fn test_start_creates_slack_thread_skipped() {
    // No thread-ts, pr-url provided but no Slack config → Slack skipped
    let dir = tempfile::tempdir().unwrap();
    let branch = "start-thread";
    let repo = create_git_repo(dir.path());
    create_state(&repo, branch, "flow-start", "auto");

    let output = run_phase_finalize(
        &repo,
        &[
            "--phase",
            "flow-start",
            "--branch",
            branch,
            "--pr-url",
            "https://github.com/test/repo/pull/42",
        ],
    );
    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert!(data["formatted_time"].is_string());

    // State should show Start complete
    let state_path = flow_states_dir(&repo).join(format!("{}.json", branch));
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["phases"]["flow-start"]["status"], "complete");
}

#[test]
fn test_no_slack_config() {
    // No thread-ts, no pr-url → Slack entirely skipped
    let dir = tempfile::tempdir().unwrap();
    let branch = "no-slack";
    let repo = create_git_repo(dir.path());
    create_state(&repo, branch, "flow-code", "auto");

    let output = run_phase_finalize(&repo, &["--phase", "flow-code", "--branch", branch]);
    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert!(
        data.get("slack").is_none(),
        "No slack key when both thread-ts and pr-url absent"
    );

    // Phase still completes
    let state_path = flow_states_dir(&repo).join(format!("{}.json", branch));
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["phases"]["flow-code"]["status"], "complete");
}

#[test]
fn test_continue_action_auto() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "auto-action";
    let repo = create_git_repo(dir.path());
    create_state(&repo, branch, "flow-code", "auto");

    let output = run_phase_finalize(&repo, &["--phase", "flow-code", "--branch", branch]);
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
    let branch = "manual-action";
    let repo = create_git_repo(dir.path());
    create_state(&repo, branch, "flow-code", "manual");

    let output = run_phase_finalize(&repo, &["--phase", "flow-code", "--branch", branch]);
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(
        data["continue_action"], "ask",
        "Manual mode should return continue_action=ask"
    );
}

#[test]
fn test_code_phase() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "code-fin";
    let repo = create_git_repo(dir.path());
    create_state(&repo, branch, "flow-code", "auto");

    let output = run_phase_finalize(&repo, &["--phase", "flow-code", "--branch", branch]);
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");

    let state_path = flow_states_dir(&repo).join(format!("{}.json", branch));
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["phases"]["flow-code"]["status"], "complete");
    assert_eq!(state["current_phase"], "flow-code-review");
}

#[test]
fn test_code_review_phase() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "cr-fin";
    let repo = create_git_repo(dir.path());
    create_state(&repo, branch, "flow-code-review", "auto");

    let output = run_phase_finalize(&repo, &["--phase", "flow-code-review", "--branch", branch]);
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");

    let state_path = flow_states_dir(&repo).join(format!("{}.json", branch));
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["phases"]["flow-code-review"]["status"], "complete");
    assert_eq!(state["current_phase"], "flow-learn");
}

#[test]
fn test_missing_state_file_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo(dir.path());
    // No state file written.

    let output = run_phase_finalize(
        &repo,
        &["--phase", "flow-code", "--branch", "no-such-branch"],
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("No state file found"));
}

#[test]
fn test_learn_phase_finalize() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "learn-fin";
    let repo = create_git_repo(dir.path());
    create_state(&repo, branch, "flow-learn", "auto");

    let output = run_phase_finalize(&repo, &["--phase", "flow-learn", "--branch", branch]);
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");

    let state_path = flow_states_dir(&repo).join(format!("{}.json", branch));
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["phases"]["flow-learn"]["status"], "complete");
    assert_eq!(state["current_phase"], "flow-complete");
}

#[test]
fn test_cwd_drift_error() {
    // When cwd is outside the flow's relative_cwd, the cwd_scope guard
    // returns an error JSON.
    let dir = tempfile::tempdir().unwrap();
    let branch = "drift-branch";
    let repo = create_git_repo(dir.path());
    // Need a state file — but state must be scoped to "api" and run from "ios".
    let state_dir = flow_states_dir(&repo);
    fs::create_dir_all(&state_dir).unwrap();
    // Get current branch of the fresh repo.
    let branch_out = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&repo)
        .output()
        .unwrap();
    let real_branch = String::from_utf8_lossy(&branch_out.stdout)
        .trim()
        .to_string();
    // Write a state file for the real branch with relative_cwd = "api"
    fs::write(
        state_dir.join(format!("{}.json", real_branch)),
        json!({"branch": real_branch, "relative_cwd": "api"}).to_string(),
    )
    .unwrap();
    let ios = repo.join("ios");
    fs::create_dir(&ios).unwrap();

    // Run phase-finalize from ios/ with --branch targeting a (different) state
    // file. The cwd_scope check runs first against the CURRENT git branch which
    // is real_branch scoped to "api/", so running from ios/ trips the drift guard.
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["phase-finalize", "--phase", "flow-code", "--branch", branch])
        .current_dir(&ios)
        .env_remove("FLOW_SIMULATE_BRANCH")
        .env_remove("SLACK_BOT_TOKEN")
        .env_remove("SLACK_CHANNEL")
        .output()
        .unwrap();
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"].as_str().unwrap_or("").contains("cwd drift"),
        "should report cwd drift: {:?}",
        data
    );
}

#[test]
fn test_frozen_phase_config_used() {
    // When frozen_phases.json exists, phase_complete uses the frozen order
    // and commands. We test that the file is consumed without error.
    let dir = tempfile::tempdir().unwrap();
    let branch = "frozen-branch";
    let repo = create_git_repo(dir.path());
    create_state(&repo, branch, "flow-code", "auto");

    // Write a frozen_phases.json file (matches phase_config schema)
    let frozen_path = flow_states_dir(&repo).join(format!("{}-frozen-phases.json", branch));
    let frozen_config = json!({
        "order": [
            "flow-start",
            "flow-plan",
            "flow-code",
            "flow-code-review",
            "flow-learn",
            "flow-complete"
        ],
        "commands": {
            "flow-start": "/flow:flow-start",
            "flow-plan": "/flow:flow-plan",
            "flow-code": "/flow:flow-code",
            "flow-code-review": "/flow:flow-code-review",
            "flow-learn": "/flow:flow-learn",
            "flow-complete": "/flow:flow-complete"
        },
        "phase_names": {
            "flow-start": "Start",
            "flow-plan": "Plan",
            "flow-code": "Code",
            "flow-code-review": "Code Review",
            "flow-learn": "Learn",
            "flow-complete": "Complete"
        }
    });
    fs::write(
        &frozen_path,
        serde_json::to_string_pretty(&frozen_config).unwrap(),
    )
    .unwrap();

    let output = run_phase_finalize(&repo, &["--phase", "flow-code", "--branch", branch]);
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");

    // Phase still completes with frozen config
    let state_path = flow_states_dir(&repo).join(format!("{}.json", branch));
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["phases"]["flow-code"]["status"], "complete");
}

#[test]
fn test_pr_url_without_thread_ts_attempts_slack() {
    // pr-url triggers Slack attempt path; no Slack config means the inner
    // slack_result is "skipped", which the response branch omits.
    let dir = tempfile::tempdir().unwrap();
    let branch = "pr-only";
    let repo = create_git_repo(dir.path());
    create_state(&repo, branch, "flow-code", "auto");

    let output = run_phase_finalize(
        &repo,
        &[
            "--phase",
            "flow-code",
            "--branch",
            branch,
            "--pr-url",
            "https://github.com/test/repo/pull/99",
        ],
    );
    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    // Skipped slack results are omitted from the response by design.
    assert!(data.get("slack").is_none());
}

/// Subprocess: state file exists but contains malformed JSON.
/// `mutate_state` cannot parse it and `run_impl_with_deps` returns a
/// structured error via the `State mutation failed:` branch rather
/// than panicking.
#[test]
fn test_malformed_state_file_returns_mutation_error() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "malformed-state";
    let repo = create_git_repo(dir.path());
    let state_dir = flow_states_dir(&repo);
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join(format!("{}.json", branch)),
        "{not valid json at all",
    )
    .unwrap();

    let output = run_phase_finalize(&repo, &["--phase", "flow-code", "--branch", branch]);
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    let message = data["message"].as_str().unwrap_or("");
    assert!(
        message.to_lowercase().contains("state mutation")
            || message.to_lowercase().contains("failed"),
        "expected state-mutation error, got: {}",
        message
    );
}

/// Subprocess: passing `--thread-ts` but no Slack credentials means
/// the notifier returns `status=skipped`. The response omits the
/// `slack` key per the omit-when-skipped branch.
#[test]
fn test_thread_ts_without_slack_credentials_omits_slack_key() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "thread-no-creds";
    let repo = create_git_repo(dir.path());
    create_state(&repo, branch, "flow-code", "auto");

    let output = run_phase_finalize(
        &repo,
        &[
            "--phase",
            "flow-code",
            "--branch",
            branch,
            "--thread-ts",
            "1234567890.123456",
        ],
    );
    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert!(
        data.get("slack").is_none(),
        "expected no slack key when status=skipped, got: {:?}",
        data.get("slack")
    );
}

// --- run_impl_with_deps library-level tests (migrated from inline) ---

fn phase_finalize_test_args(
    phase: &str,
    branch: &str,
    thread_ts: Option<&str>,
    pr_url: Option<&str>,
) -> Args {
    Args {
        phase: phase.to_string(),
        branch: branch.to_string(),
        thread_ts: thread_ts.map(|s| s.to_string()),
        pr_url: pr_url.map(|s| s.to_string()),
    }
}

fn phase_finalize_write_state(root: &std::path::Path, branch: &str, current_phase: &str) {
    let state_dir = root.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();

    let phase_order = [
        "flow-start",
        "flow-plan",
        "flow-code",
        "flow-code-review",
        "flow-learn",
        "flow-complete",
    ];
    let cur_idx = phase_order
        .iter()
        .position(|p| *p == current_phase)
        .expect("current_phase must be a known phase");

    let mut phases = serde_json::Map::new();
    for (idx, p) in phase_order.iter().enumerate() {
        let status = match idx.cmp(&cur_idx) {
            std::cmp::Ordering::Less => "complete",
            std::cmp::Ordering::Equal => "in_progress",
            std::cmp::Ordering::Greater => "pending",
        };
        phases.insert(
            p.to_string(),
            json!({
                "name": p,
                "status": status,
                "started_at": if status != "pending" { Some("2026-01-01T00:00:00-08:00") } else { None },
                "completed_at": if status == "complete" { Some("2026-01-01T00:01:00-08:00") } else { None },
                "session_started_at": if status == "in_progress" { Some("2026-01-01T00:00:00-08:00") } else { None },
                "cumulative_seconds": if status == "complete" { 60 } else { 0 },
                "visit_count": if status == "pending" { 0 } else { 1 }
            }),
        );
    }

    let state = json!({
        "schema_version": 1,
        "branch": branch,
        "current_phase": current_phase,
        "started_at": "2026-01-01T00:00:00-08:00",
        "phases": Value::Object(phases),
        "phase_transitions": [],
        "prompt": "test feature",
        "notes": [],
    });

    fs::write(
        state_dir.join(format!("{}.json", branch)),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();
}

fn phase_finalize_read_state(root: &std::path::Path, branch: &str) -> Value {
    let path = root.join(".flow-states").join(format!("{}.json", branch));
    let content = fs::read_to_string(&path).unwrap();
    serde_json::from_str(&content).unwrap()
}

#[test]
fn finalize_with_notifier_slack_thread_reply_success() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    phase_finalize_write_state(root, "branch-a", "flow-code");

    let notifier = |_: &notify_slack::Args| -> Value { json!({"status": "ok", "ts": "5555.6666"}) };
    let args = phase_finalize_test_args("flow-code", "branch-a", Some("1111.2222"), None);

    let result = run_impl_with_deps(root, root, &args, &notifier).unwrap();
    assert_eq!(result["status"], "ok");
    assert_eq!(result["slack"]["status"], "ok");

    let state = phase_finalize_read_state(root, "branch-a");
    assert!(state.get("slack_thread_ts").is_none() || state["slack_thread_ts"].is_null());
    let notifs = state["slack_notifications"].as_array().unwrap();
    assert_eq!(notifs.len(), 1);
    assert_eq!(notifs[0]["ts"], "5555.6666");
    assert_eq!(notifs[0]["thread_ts"], "1111.2222");
    assert_eq!(notifs[0]["phase"], "flow-code");
}

#[test]
fn finalize_with_notifier_slack_thread_create_success() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    phase_finalize_write_state(root, "branch-b", "flow-start");

    let notifier = |_: &notify_slack::Args| -> Value { json!({"status": "ok", "ts": "7777.8888"}) };
    let args = phase_finalize_test_args(
        "flow-start",
        "branch-b",
        None,
        Some("https://github.com/org/repo/pull/42"),
    );

    let result = run_impl_with_deps(root, root, &args, &notifier).unwrap();
    assert_eq!(result["status"], "ok");

    let state = phase_finalize_read_state(root, "branch-b");
    assert_eq!(state["slack_thread_ts"], "7777.8888");
    let notifs = state["slack_notifications"].as_array().unwrap();
    assert_eq!(notifs.len(), 1);
    assert_eq!(notifs[0]["thread_ts"], "7777.8888");
}

#[test]
fn finalize_with_notifier_slack_error_skips_state_record() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    phase_finalize_write_state(root, "branch-c", "flow-code");

    let notifier =
        |_: &notify_slack::Args| -> Value { json!({"status": "error", "message": "boom"}) };
    let args = phase_finalize_test_args("flow-code", "branch-c", Some("1111.2222"), None);

    let result = run_impl_with_deps(root, root, &args, &notifier).unwrap();
    assert_eq!(result["status"], "ok");
    assert_eq!(result["slack"]["status"], "error");

    let state = phase_finalize_read_state(root, "branch-c");
    let notifs_empty = state
        .get("slack_notifications")
        .map(|v| v.as_array().map(|a| a.is_empty()).unwrap_or(true))
        .unwrap_or(true);
    assert!(notifs_empty);
}

#[test]
fn finalize_with_notifier_slash_branch_returns_structured_error_no_panic() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let notifier = |_: &notify_slack::Args| -> Value { json!({"status": "skipped"}) };
    let args = phase_finalize_test_args("flow-code", "feature/foo", None, None);

    let result = run_impl_with_deps(root, root, &args, &notifier).unwrap();
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("Invalid branch name"));
}

#[test]
fn finalize_with_notifier_empty_branch_returns_structured_error_no_panic() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let notifier = |_: &notify_slack::Args| -> Value { json!({"status": "skipped"}) };
    let args = phase_finalize_test_args("flow-code", "", None, None);

    let result = run_impl_with_deps(root, root, &args, &notifier).unwrap();
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("Invalid branch name"));
}

#[test]
fn finalize_with_notifier_state_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let notifier = |_: &notify_slack::Args| -> Value { json!({"status": "skipped"}) };
    let args = phase_finalize_test_args("flow-code", "branch-missing", None, None);

    let result = run_impl_with_deps(root, root, &args, &notifier).unwrap();
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("No state file found"));
}

#[test]
fn finalize_with_notifier_no_slack_args_response_omits_slack_key() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    phase_finalize_write_state(root, "branch-d", "flow-code");

    let notifier = |_: &notify_slack::Args| -> Value {
        panic!("notifier must not be called when neither thread_ts nor pr_url is set");
    };
    let args = phase_finalize_test_args("flow-code", "branch-d", None, None);

    let result = run_impl_with_deps(root, root, &args, &notifier).unwrap();
    assert_eq!(result["status"], "ok");
    assert!(result.get("slack").is_none());

    let state = phase_finalize_read_state(root, "branch-d");
    assert!(state.get("slack_thread_ts").is_none() || state["slack_thread_ts"].is_null());
    let notifs_empty = state
        .get("slack_notifications")
        .map(|v| v.as_array().map(|a| a.is_empty()).unwrap_or(true))
        .unwrap_or(true);
    assert!(notifs_empty);
}
