//! Integration tests for phase-enter subcommand.
//!
//! phase-enter consolidates: gate check + phase_enter() + step counters +
//! state data return into a single command parameterized by --phase.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::flow_states_dir;
use flow_rs::phase_enter::resolve_mode;
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
    let state_dir = flow_states_dir(repo);
    let branch_dir = state_dir.join(branch);
    fs::create_dir_all(&branch_dir).unwrap();

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
        branch_dir.join("state.json"),
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
    let state_path = flow_states_dir(&repo).join(branch).join("state.json");
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
    let state_path = flow_states_dir(&repo).join(branch).join("state.json");
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
    let state_path = flow_states_dir(&repo).join(branch).join("state.json");
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
        &fs::read_to_string(
            flow_states_dir(&repo)
                .join(branch)
                .join("state.json"),
        )
        .unwrap(),
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
        &fs::read_to_string(
            repo2
                .join(".flow-states")
                .join(branch2)
                .join("state.json"),
        )
        .unwrap(),
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
        &fs::read_to_string(
            flow_states_dir(&repo)
                .join(branch)
                .join("state.json"),
        )
        .unwrap(),
    )
    .unwrap();
    // No step counter fields should be set
    assert!(state.get("code_steps_total").is_none());
    assert!(state.get("code_step").is_none());
}

#[test]
fn test_corrupt_json_state_file_returns_error() {
    // State file exists but contains invalid JSON. run_impl's
    // serde_json::from_str returns Err, propagated via `?` to
    // run() which hits the Err branch (json_error + process::exit).
    let dir = tempfile::tempdir().unwrap();
    let branch = "corrupt-state";
    let repo = create_git_repo(dir.path(), branch);
    let branch_dir = flow_states_dir(&repo).join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), "not valid json").unwrap();

    let output = run_phase_enter(&repo, &["--phase", "flow-code", "--branch", branch]);
    assert_ne!(
        output.status.code(),
        Some(0),
        "should exit non-zero for corrupt state"
    );
    // json_error prints to stdout, not stderr
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Invalid JSON") || stdout.contains("state file"),
        "stdout should mention the parse failure: {}",
        stdout
    );
}

#[test]
fn test_mode_string_config_via_subprocess() {
    // Exercises resolve_mode's `cfg.is_string()` branch through
    // the subprocess path (run → run_impl → resolve_mode).
    let dir = tempfile::tempdir().unwrap();
    let branch = "mode-string";
    let repo = create_git_repo(dir.path(), branch);
    let skills = json!({"flow-code": "auto"});
    create_state(&repo, branch, "flow-plan", "complete", Some(skills));

    let output = run_phase_enter(&repo, &["--phase", "flow-code", "--branch", branch]);
    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["mode"]["commit"], "auto");
    assert_eq!(data["mode"]["continue"], "auto");
}

#[test]
fn test_mutate_state_failure_returns_error() {
    // Make the state file read-only after creation so mutate_state
    // fails when trying to write back. Exercises run_impl's
    // mutate_state Err path.
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let branch = "mutate-fail";
    let repo = create_git_repo(dir.path(), branch);
    create_state(&repo, branch, "flow-plan", "complete", None);

    let state_file = flow_states_dir(&repo).join(branch).join("state.json");
    fs::set_permissions(&state_file, fs::Permissions::from_mode(0o444)).unwrap();

    let output = run_phase_enter(&repo, &["--phase", "flow-code", "--branch", branch]);
    // Restore permissions for cleanup
    let _ = fs::set_permissions(&state_file, fs::Permissions::from_mode(0o644));
    assert_eq!(output.status.code(), Some(0)); // Application error, not process error
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap()
            .contains("State mutation failed"),
        "should report mutation failure: {}",
        data["message"]
    );
}

/// Subprocess: `phase-enter --branch <slash-branch>` exercises the
/// `FlowPaths::try_new` None branch for a slash-containing branch
/// inside `resolve_state`. Returns structured error with exit 0, no
/// panic. Guards the regression where `resolve_state` used
/// `FlowPaths::new` (panicking) and any slash branch like
/// `feature/foo` would crash the CLI. Consumer: every skill and hook
/// that invokes `bin/flow phase-enter` during an active flow —
/// per `.claude/rules/external-input-validation.md`, CLI `--branch`
/// overrides must never panic.
#[test]
fn test_slash_branch_returns_structured_error_no_panic() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let repo = create_git_repo(&root, "main");

    let output = run_phase_enter(
        &repo,
        &["--phase", "flow-code", "--branch", "feature/with-slash"],
    );
    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    let message = data["message"].as_str().unwrap_or("");
    assert!(
        message.contains("Invalid branch name"),
        "expected 'Invalid branch name' error, got: {}",
        message
    );
}

/// Subprocess: `phase-enter` on a phase that is already `complete`
/// hits the gate-failure branch that the inline tests don't exercise
/// at this specific shape (starting complete state, trying to re-enter).
#[test]
fn test_reenter_complete_phase_returns_gate_error() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "reenter-complete";
    let repo = create_git_repo(dir.path(), branch);
    // Create a state where flow-start is complete so we can re-enter
    // flow-code by asserting the gate behavior.
    create_state(&repo, branch, "flow-start", "auto", None);

    // Enter flow-start — the first phase in PHASE_ORDER has no
    // predecessor, so gate_check returns the "no predecessor" error.
    // Guards the regression where gate_check's "no predecessor" branch
    // silently succeeds (would let phase-enter re-initialize the first
    // phase mid-flow and lose state).
    let output = run_phase_enter(&repo, &["--phase", "flow-start", "--branch", branch]);
    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    let message = data["message"].as_str().unwrap_or("");
    assert!(
        message.contains("no predecessor") || message.contains("phase order"),
        "expected 'no predecessor' gate error, got: {}",
        message
    );
}

// Direct `phase_field_prefix` and `gate_check` tests removed — both
// are now private. `phase_field_prefix` is exercised only through
// `run_impl` (the prefix values surface as state-file field names
// written by phase-enter); `gate_check` is exercised through the
// subprocess tests above that spawn `bin/flow phase-enter`.

// --- resolve_mode (migrated from inline) ---

#[test]
fn resolve_mode_object_config() {
    let state = json!({
        "skills": {
            "flow-code": {"commit": "auto", "continue": "manual"}
        }
    });
    let (commit, cont) = resolve_mode(&state, "flow-code");
    assert_eq!(commit, "auto");
    assert_eq!(cont, "manual");
}

#[test]
fn resolve_mode_string_config() {
    let state = json!({"skills": {"flow-code": "auto"}});
    let (commit, cont) = resolve_mode(&state, "flow-code");
    assert_eq!(commit, "auto");
    assert_eq!(cont, "auto");
}

#[test]
fn resolve_mode_empty_string_falls_to_defaults() {
    let state = json!({"skills": {"flow-code": ""}});
    let (commit, cont) = resolve_mode(&state, "flow-code");
    assert_eq!(commit, "manual");
    assert_eq!(cont, "manual");
}

#[test]
fn resolve_mode_no_skills_key() {
    let state = json!({"branch": "test"});
    let (commit, cont) = resolve_mode(&state, "flow-code");
    assert_eq!(commit, "manual");
    assert_eq!(cont, "manual");
}

#[test]
fn resolve_mode_flow_learn_defaults() {
    let state = json!({"skills": {}});
    let (commit, cont) = resolve_mode(&state, "flow-learn");
    assert_eq!(commit, "auto");
    assert_eq!(cont, "auto");
}

#[test]
fn resolve_mode_unexpected_type() {
    let state = json!({"skills": {"flow-code": 42}});
    let (commit, cont) = resolve_mode(&state, "flow-code");
    assert_eq!(commit, "manual");
    assert_eq!(cont, "manual");
}

#[test]
fn resolve_mode_phase_not_in_skills() {
    let state = json!({"skills": {"flow-start": {"continue": "auto"}}});
    let (commit, cont) = resolve_mode(&state, "flow-code");
    assert_eq!(commit, "manual");
    assert_eq!(cont, "manual");
}

#[test]
fn resolve_mode_object_config_partial_keys() {
    let state = json!({"skills": {"flow-code": {"commit": "auto"}}});
    let (commit, cont) = resolve_mode(&state, "flow-code");
    assert_eq!(commit, "auto");
    assert_eq!(cont, "manual");
}

#[test]
fn resolve_mode_flow_learn_object_override() {
    let state = json!({
        "skills": {"flow-learn": {"commit": "manual", "continue": "manual"}}
    });
    let (commit, cont) = resolve_mode(&state, "flow-learn");
    assert_eq!(commit, "manual");
    assert_eq!(cont, "manual");
}

/// Covers the `Err(_) => return Err(...)` branch of `resolve_branch` in
/// `resolve_state`. Spawning `phase-enter` from a non-git directory
/// with no `--branch` flag produces a current-branch failure.
#[test]
fn phase_enter_no_branch_in_non_git_dir_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["phase-enter", "--phase", "flow-code"])
        .current_dir(tmp.path())
        .env_remove("FLOW_SIMULATE_BRANCH")
        .env_remove("FLOW_CI_RUNNING")
        .env("GIT_CEILING_DIRECTORIES", tmp.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last = stdout.trim().lines().last().unwrap_or("");
    let data: Value = serde_json::from_str(last).unwrap_or(json!({}));
    assert_eq!(data["status"], "error");
    let msg = data["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("Could not determine current branch") || msg.contains("Invalid branch"),
        "expected branch-resolution error, got: {}",
        msg
    );
}

/// Covers the `cwd_scope::enforce` Err branch at line 183 of
/// `phase_enter::run_impl`. The state file declares
/// `relative_cwd="api"` but the subprocess runs from the worktree
/// root — cwd is NOT inside `<root>/api`, so `enforce` returns Err
/// and run_impl short-circuits with a "cwd drift" error payload.
#[test]
fn phase_enter_with_relative_cwd_mismatch_returns_cwd_drift_error() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "cwd-drift";
    let repo = create_git_repo(dir.path(), branch);
    // `cwd_scope::enforce` keys off `current_branch_in(cwd)` — switch
    // the repo's active branch to `branch` so the state-file lookup
    // for `<branch>.json` finds our fixture.
    Command::new("git")
        .args(["checkout", branch])
        .current_dir(&repo)
        .output()
        .unwrap();
    let state_dir = flow_states_dir(&repo);
    let branch_dir = state_dir.join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    // Force relative_cwd to "api" so cwd-scope expects `<repo>/api`.
    fs::write(
        branch_dir.join("state.json"),
        serde_json::to_string(&json!({
            "branch": branch,
            "relative_cwd": "api",
            "current_phase": "flow-plan",
            "phases": {
                "flow-plan": {"status": "complete"}
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let output = run_phase_enter(&repo, &["--phase", "flow-code", "--branch", branch]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last = stdout.trim().lines().last().unwrap_or("");
    let data: Value = serde_json::from_str(last).unwrap_or(json!({}));
    assert_eq!(data["status"], "error");
    let msg = data["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("cwd drift"),
        "expected cwd drift error, got: {}",
        msg
    );
}

/// Covers the `map_err(|e| format!("Could not read state file: {}", e))?`
/// closure on line 188 of `run_impl`. The state "file" is actually a
/// directory — `Path::exists()` returns true for directories, so
/// `resolve_state` passes, but `fs::read_to_string` then fails with
/// `EISDIR`. The `?` propagates a `Err(String)` from `run_impl`,
/// which the binary's dispatch converts into exit 1 + stderr message.
#[test]
fn phase_enter_state_path_is_directory_errors() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "state-is-dir";
    let _repo = create_git_repo(dir.path(), branch);
    let repo = &_repo;
    let state_dir = flow_states_dir(repo);
    let branch_dir = state_dir.join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    // Create a DIRECTORY at the state file path. exists() returns
    // true, but read_to_string returns Err(EISDIR).
    fs::create_dir_all(branch_dir.join("state.json")).unwrap();

    let output = run_phase_enter(repo, &["--phase", "flow-code", "--branch", branch]);
    // Infrastructure-level Err is routed through
    // `dispatch_ok_result_json` → `{status:error, message}` on stdout
    // with exit code 1.
    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit 1 from Err(String), stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    let msg = data["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("Could not read state file"),
        "expected 'Could not read state file' in message, got: {}",
        msg
    );
}

/// Covers the `or_else(|| state.get("plan_file")...)` fallback closure
/// on line 221 of `run_impl`. State has no `files.plan` entry but
/// does have a top-level `plan_file` (legacy schema shape). The
/// response's `plan_file` field comes from the fallback.
#[test]
fn phase_enter_legacy_plan_file_fallback_covered() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "legacy-plan";
    let _repo = create_git_repo(dir.path(), branch);
    let repo = &_repo;
    let state_dir = flow_states_dir(repo);
    let branch_dir = state_dir.join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    // Legacy state: no `files.plan`, has top-level `plan_file`.
    let state = json!({
        "branch": branch,
        "repo": "test/repo",
        "pr_number": 42,
        "pr_url": "https://github.com/test/repo/pull/42",
        "feature": "Legacy Feature",
        "slack_thread_ts": "1.2",
        "current_phase": "flow-plan",
        "plan_file": ".flow-states/legacy-plan.md",
        "files": {"plan": null, "log": format!(".flow-states/{}.log", branch)},
        "phases": {
            "flow-start": {"status": "complete"},
            "flow-plan": {"status": "complete"},
        },
        "phase_transitions": [],
    });
    fs::write(
        branch_dir.join("state.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();

    let output = run_phase_enter(repo, &["--phase", "flow-code", "--branch", branch]);
    let data = parse_output(&output);
    assert_eq!(
        data["status"],
        "ok",
        "stderr: {}, stdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        data["plan_file"], ".flow-states/legacy-plan.md",
        "plan_file must come from the legacy top-level field"
    );
}

/// Covers the implicit `None` arm of all five `if let Some(x) = field`
/// blocks that build the response (pr_number, pr_url, feature,
/// slack_thread_ts, plan_file). State has none of the optional fields
/// populated; the response is built with only the required fields.
#[test]
fn phase_enter_response_omits_absent_optional_fields() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "minimal-state";
    let _repo = create_git_repo(dir.path(), branch);
    let repo = &_repo;
    let state_dir = flow_states_dir(repo);
    let branch_dir = state_dir.join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    // Minimal state: no pr_number, no pr_url, no feature, no
    // slack_thread_ts, no files.plan, no top-level plan_file. All
    // five optional response fields are absent.
    let state = json!({
        "branch": branch,
        "current_phase": "flow-plan",
        "phases": {
            "flow-start": {"status": "complete"},
            "flow-plan": {"status": "complete"},
        },
        "phase_transitions": [],
    });
    fs::write(
        branch_dir.join("state.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();

    let output = run_phase_enter(repo, &["--phase", "flow-code", "--branch", branch]);
    let data = parse_output(&output);
    assert_eq!(
        data["status"],
        "ok",
        "stderr: {}, stdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    // None of the optional fields appear in the response.
    assert!(data.get("pr_number").is_none(), "pr_number must be absent");
    assert!(data.get("pr_url").is_none(), "pr_url must be absent");
    assert!(data.get("feature").is_none(), "feature must be absent");
    assert!(
        data.get("slack_thread_ts").is_none(),
        "slack_thread_ts must be absent"
    );
    assert!(data.get("plan_file").is_none(), "plan_file must be absent");
}
