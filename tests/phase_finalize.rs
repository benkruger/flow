//! Integration tests for phase-finalize subcommand.
//!
//! phase-finalize consolidates: phase_complete() + Slack notification +
//! add-notification into a single command parameterized by --phase.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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
fn create_state(
    repo: &Path,
    branch: &str,
    current_phase: &str,
    skills_continue: &str,
) {
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();

    let state = json!({
        "schema_version": 1,
        "branch": branch,
        "repo": "test/repo",
        "pr_number": 42,
        "pr_url": "https://github.com/test/repo/pull/42",
        "started_at": "2026-01-01T00:00:00-08:00",
        "current_phase": current_phase,
        "framework": "python",
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
            "--phase", "flow-learn",
            "--branch", branch,
            "--thread-ts", "1234567890.123456",
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
    let state_path = repo.join(".flow-states").join(format!("{}.json", branch));
    let state: Value =
        serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
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
            "--phase", "flow-start",
            "--branch", branch,
            "--pr-url", "https://github.com/test/repo/pull/42",
        ],
    );
    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert!(data["formatted_time"].is_string());

    // State should show Start complete
    let state_path = repo.join(".flow-states").join(format!("{}.json", branch));
    let state: Value =
        serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["phases"]["flow-start"]["status"], "complete");
}

#[test]
fn test_no_slack_config() {
    // No thread-ts, no pr-url → Slack entirely skipped
    let dir = tempfile::tempdir().unwrap();
    let branch = "no-slack";
    let repo = create_git_repo(dir.path());
    create_state(&repo, branch, "flow-code", "auto");

    let output = run_phase_finalize(
        &repo,
        &["--phase", "flow-code", "--branch", branch],
    );
    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert!(data.get("slack").is_none(), "No slack key when both thread-ts and pr-url absent");

    // Phase still completes
    let state_path = repo.join(".flow-states").join(format!("{}.json", branch));
    let state: Value =
        serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["phases"]["flow-code"]["status"], "complete");
}

#[test]
fn test_continue_action_auto() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "auto-action";
    let repo = create_git_repo(dir.path());
    create_state(&repo, branch, "flow-code", "auto");

    let output = run_phase_finalize(
        &repo,
        &["--phase", "flow-code", "--branch", branch],
    );
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

    let output = run_phase_finalize(
        &repo,
        &["--phase", "flow-code", "--branch", branch],
    );
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

    let output = run_phase_finalize(
        &repo,
        &["--phase", "flow-code", "--branch", branch],
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");

    let state_path = repo.join(".flow-states").join(format!("{}.json", branch));
    let state: Value =
        serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["phases"]["flow-code"]["status"], "complete");
    assert_eq!(state["current_phase"], "flow-code-review");
}

#[test]
fn test_code_review_phase() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "cr-fin";
    let repo = create_git_repo(dir.path());
    create_state(&repo, branch, "flow-code-review", "auto");

    let output = run_phase_finalize(
        &repo,
        &["--phase", "flow-code-review", "--branch", branch],
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");

    let state_path = repo.join(".flow-states").join(format!("{}.json", branch));
    let state: Value =
        serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["phases"]["flow-code-review"]["status"], "complete");
    assert_eq!(state["current_phase"], "flow-learn");
}
