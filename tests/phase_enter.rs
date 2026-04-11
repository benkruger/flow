//! Integration tests for phase-enter subcommand.
//!
//! phase-enter consolidates: gate check + phase_enter() + step counters +
//! state data return into a single command parameterized by --phase.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

// --- Test helpers ---

/// Create a minimal git repo with a branch.
fn create_git_repo(parent: &Path, branch: &str) -> PathBuf {
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

    // Create and switch to feature branch
    Command::new("git")
        .args(["branch", branch])
        .current_dir(&repo)
        .output()
        .unwrap();

    repo
}

/// Create a state file with configurable phase statuses.
fn create_state(
    repo: &Path,
    branch: &str,
    prev_phase: &str,
    prev_status: &str,
    skills: Option<Value>,
) {
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();

    let skills_val = skills.unwrap_or(json!({}));

    let state = json!({
        "schema_version": 1,
        "branch": branch,
        "repo": "test/repo",
        "pr_number": 42,
        "pr_url": "https://github.com/test/repo/pull/42",
        "started_at": "2026-01-01T00:00:00-08:00",
        "current_phase": prev_phase,
        "feature": "Test Feature",
        "files": {
            "plan": ".flow-states/test-plan.md",
            "dag": null,
            "log": format!(".flow-states/{}.log", branch),
            "state": format!(".flow-states/{}.json", branch)
        },
        "session_tty": null,
        "session_id": null,
        "transcript_path": null,
        "notes": [],
        "prompt": "test feature",
        "slack_thread_ts": "1234567890.123456",
        "phases": {
            "flow-start": {
                "name": "Start",
                "status": "complete",
                "started_at": "2026-01-01T00:00:00-08:00",
                "completed_at": "2026-01-01T00:01:00-08:00",
                "session_started_at": null,
                "cumulative_seconds": 60,
                "visit_count": 1
            },
            "flow-plan": {
                "name": "Plan",
                "status": if prev_phase == "flow-plan" { prev_status } else { "complete" },
                "started_at": "2026-01-01T00:01:00-08:00",
                "completed_at": if prev_phase != "flow-plan" || prev_status == "complete" {
                    Some("2026-01-01T00:02:00-08:00")
                } else { None },
                "session_started_at": null,
                "cumulative_seconds": 60,
                "visit_count": 1
            },
            "flow-code": {
                "name": "Code",
                "status": if prev_phase == "flow-code" { prev_status } else if prev_phase == "flow-plan" { "pending" } else { "complete" },
                "started_at": null,
                "completed_at": null,
                "session_started_at": null,
                "cumulative_seconds": 0,
                "visit_count": 0
            },
            "flow-code-review": {
                "name": "Code Review",
                "status": if prev_phase == "flow-code-review" { prev_status } else { "pending" },
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
        "skills": skills_val,
    });
    fs::write(
        state_dir.join(format!("{}.json", branch)),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();
}

/// Run flow-rs phase-enter.
fn run_phase_enter(repo: &Path, extra_args: &[&str]) -> Output {
    let mut args = vec!["phase-enter"];
    args.extend_from_slice(extra_args);

    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(&args)
        .current_dir(repo)
        .env_remove("FLOW_SIMULATE_BRANCH")
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
fn test_code_phase_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "code-happy";
    let repo = create_git_repo(dir.path(), branch);
    create_state(&repo, branch, "flow-plan", "complete", None);

    let output = run_phase_enter(&repo, &["--phase", "flow-code", "--branch", branch]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["phase"], "flow-code");
    assert_eq!(data["branch"], branch);
    assert!(data["project_root"].is_string());
    assert_eq!(data["pr_number"], 42);
    assert_eq!(data["pr_url"], "https://github.com/test/repo/pull/42");
    assert_eq!(data["feature"], "Test Feature");
    assert_eq!(data["slack_thread_ts"], "1234567890.123456");
    assert_eq!(data["plan_file"], ".flow-states/test-plan.md");
    assert_eq!(data["mode"]["commit"], "manual");
    assert_eq!(data["mode"]["continue"], "manual");

    // State should be updated — phase entered
    let state_path = repo.join(".flow-states").join(format!("{}.json", branch));
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["phases"]["flow-code"]["status"], "in_progress");
    assert_eq!(state["current_phase"], "flow-code");
    assert_eq!(state["phases"]["flow-code"]["visit_count"], 1);

    // No steps_total set for Code phase (no --steps-total passed)
    assert!(state.get("code_steps_total").is_none());
}

#[test]
fn test_code_review_phase_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "review-happy";
    let repo = create_git_repo(dir.path(), branch);
    create_state(&repo, branch, "flow-code", "complete", None);

    let output = run_phase_enter(
        &repo,
        &[
            "--phase",
            "flow-code-review",
            "--branch",
            branch,
            "--steps-total",
            "4",
        ],
    );
    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["phase"], "flow-code-review");

    // State should have step counters set
    let state_path = repo.join(".flow-states").join(format!("{}.json", branch));
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["phases"]["flow-code-review"]["status"], "in_progress");
    assert_eq!(state["code_review_steps_total"], 4);
    assert_eq!(state["code_review_step"], 0);
}

#[test]
fn test_learn_phase_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "learn-happy";
    let repo = create_git_repo(dir.path(), branch);
    create_state(&repo, branch, "flow-code-review", "complete", None);

    let output = run_phase_enter(
        &repo,
        &[
            "--phase",
            "flow-learn",
            "--branch",
            branch,
            "--steps-total",
            "7",
        ],
    );
    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["phase"], "flow-learn");

    // State should have step counters set
    let state_path = repo.join(".flow-states").join(format!("{}.json", branch));
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["phases"]["flow-learn"]["status"], "in_progress");
    assert_eq!(state["learn_steps_total"], 7);
    assert_eq!(state["learn_step"], 0);
}

#[test]
fn test_gate_failure_previous_phase_not_complete() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "gate-fail";
    let repo = create_git_repo(dir.path(), branch);
    create_state(&repo, branch, "flow-plan", "in_progress", None);

    let output = run_phase_enter(&repo, &["--phase", "flow-code", "--branch", branch]);
    assert_eq!(output.status.code(), Some(0)); // Application error, not process error
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    let msg = data["message"].as_str().unwrap();
    assert!(
        msg.contains("flow-plan"),
        "Error should name the blocking phase: {}",
        msg
    );
    assert!(
        msg.contains("complete"),
        "Error should mention 'complete': {}",
        msg
    );
}

#[test]
fn test_gate_failure_no_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "no-state";
    let repo = create_git_repo(dir.path(), branch);
    // Don't create any state file

    let output = run_phase_enter(&repo, &["--phase", "flow-code", "--branch", branch]);
    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"].as_str().unwrap().contains("No state file"));
}

#[test]
fn test_mode_resolution_from_state() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "mode-state";
    let repo = create_git_repo(dir.path(), branch);
    let skills = json!({
        "flow-code": {
            "commit": "auto",
            "continue": "auto"
        }
    });
    create_state(&repo, branch, "flow-plan", "complete", Some(skills));

    let output = run_phase_enter(&repo, &["--phase", "flow-code", "--branch", branch]);
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["mode"]["commit"], "auto");
    assert_eq!(data["mode"]["continue"], "auto");
}

#[test]
fn test_mode_defaults_code_review() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "mode-cr";
    let repo = create_git_repo(dir.path(), branch);
    // No skills config → defaults
    create_state(&repo, branch, "flow-code", "complete", Some(json!({})));

    let output = run_phase_enter(
        &repo,
        &[
            "--phase",
            "flow-code-review",
            "--branch",
            branch,
            "--steps-total",
            "4",
        ],
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(
        data["mode"]["commit"], "manual",
        "Code Review should default to commit=manual"
    );
    assert_eq!(
        data["mode"]["continue"], "manual",
        "Code Review should default to continue=manual"
    );
}

#[test]
fn test_mode_defaults_learn() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "mode-learn";
    let repo = create_git_repo(dir.path(), branch);
    // No skills config → defaults
    create_state(
        &repo,
        branch,
        "flow-code-review",
        "complete",
        Some(json!({})),
    );

    let output = run_phase_enter(
        &repo,
        &[
            "--phase",
            "flow-learn",
            "--branch",
            branch,
            "--steps-total",
            "7",
        ],
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(
        data["mode"]["commit"], "auto",
        "Learn should default to commit=auto"
    );
    assert_eq!(
        data["mode"]["continue"], "auto",
        "Learn should default to continue=auto"
    );
}

#[test]
fn test_step_counter_field_names() {
    // Verify the field name derivation for all 3 applicable phases
    let dir = tempfile::tempdir().unwrap();

    // Code Review: flow-code-review → code_review_steps_total, code_review_step
    let branch = "counter-cr";
    let repo = create_git_repo(dir.path(), branch);
    create_state(&repo, branch, "flow-code", "complete", None);
    let output = run_phase_enter(
        &repo,
        &[
            "--phase",
            "flow-code-review",
            "--branch",
            branch,
            "--steps-total",
            "4",
        ],
    );
    assert_eq!(parse_output(&output)["status"], "ok");
    let state: Value = serde_json::from_str(
        &fs::read_to_string(repo.join(".flow-states").join(format!("{}.json", branch))).unwrap(),
    )
    .unwrap();
    assert_eq!(state["code_review_steps_total"], 4);
    assert_eq!(state["code_review_step"], 0);
    // Verify the wrong field names are NOT present
    assert!(state.get("flow_code_review_steps_total").is_none());

    // Learn: flow-learn → learn_steps_total, learn_step
    let branch2 = "counter-learn";
    let repo2 = create_git_repo(&dir.path().join("sub"), branch2);
    create_state(&repo2, branch2, "flow-code-review", "complete", None);
    let output2 = run_phase_enter(
        &repo2,
        &[
            "--phase",
            "flow-learn",
            "--branch",
            branch2,
            "--steps-total",
            "7",
        ],
    );
    assert_eq!(parse_output(&output2)["status"], "ok");
    let state2: Value = serde_json::from_str(
        &fs::read_to_string(repo2.join(".flow-states").join(format!("{}.json", branch2))).unwrap(),
    )
    .unwrap();
    assert_eq!(state2["learn_steps_total"], 7);
    assert_eq!(state2["learn_step"], 0);
}

#[test]
fn test_no_steps_total_flag() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "no-steps";
    let repo = create_git_repo(dir.path(), branch);
    create_state(&repo, branch, "flow-plan", "complete", None);

    // Code phase: no --steps-total
    let output = run_phase_enter(&repo, &["--phase", "flow-code", "--branch", branch]);
    assert_eq!(parse_output(&output)["status"], "ok");

    let state: Value = serde_json::from_str(
        &fs::read_to_string(repo.join(".flow-states").join(format!("{}.json", branch))).unwrap(),
    )
    .unwrap();
    // No step counter fields should be set
    assert!(state.get("code_steps_total").is_none());
    assert!(state.get("code_step").is_none());
}
