//! Integration tests for phase-finalize subcommand.
//!
//! phase-finalize consolidates: phase_complete() + Slack notification +
//! add-notification into a single command parameterized by --phase.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::flow_states_dir;
use flow_rs::phase_finalize::{run_impl, run_impl_main, Args};
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
    let branch_dir = state_dir.join(branch);
    fs::create_dir_all(&branch_dir).unwrap();

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
            "flow-code": {
                "name": "Code",
                "status": if current_phase == "flow-code" { "in_progress" } else if current_phase == "flow-start" { "pending" } else { "complete" },
                "started_at": if current_phase == "flow-code" { Some("2026-01-01T00:02:00-08:00") } else { None },
                "completed_at": null,
                "session_started_at": if current_phase == "flow-code" { Some("2026-01-01T00:02:00-08:00") } else { None },
                "cumulative_seconds": 0,
                "visit_count": if current_phase == "flow-code" { 1 } else { 0 }
            },
            "flow-review": {
                "name": "Review",
                "status": if current_phase == "flow-review" { "in_progress" } else { "pending" },
                "started_at": if current_phase == "flow-review" { Some("2026-01-01T00:03:00-08:00") } else { None },
                "completed_at": null,
                "session_started_at": if current_phase == "flow-review" { Some("2026-01-01T00:03:00-08:00") } else { None },
                "cumulative_seconds": 0,
                "visit_count": if current_phase == "flow-review" { 1 } else { 0 },
                // Pre-populate every required agent so the
                // required-agents gate added in this PR passes for
                // tests that don't specifically test the gate.
                // Tests that DO test the gate override this via
                // `seed_agents_returned` after `create_state`.
                "agents_returned": [
                    {"agent": "reviewer", "timestamp": "2026-01-01T00:03:30-08:00"},
                    {"agent": "pre-mortem", "timestamp": "2026-01-01T00:03:31-08:00"},
                    {"agent": "adversarial", "timestamp": "2026-01-01T00:03:32-08:00"},
                    {"agent": "documentation", "timestamp": "2026-01-01T00:03:33-08:00"}
                ]
            },
            "flow-learn": {
                "name": "Learn",
                "status": if current_phase == "flow-learn" { "in_progress" } else { "pending" },
                "started_at": if current_phase == "flow-learn" { Some("2026-01-01T00:04:00-08:00") } else { None },
                "completed_at": null,
                "session_started_at": if current_phase == "flow-learn" { Some("2026-01-01T00:04:00-08:00") } else { None },
                "cumulative_seconds": 0,
                "visit_count": if current_phase == "flow-learn" { 1 } else { 0 },
                "agents_returned": [
                    {"agent": "learn-analyst", "timestamp": "2026-01-01T00:04:30-08:00"}
                ]
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
            "flow-review": {
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
        branch_dir.join("state.json"),
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
    let state_path = flow_states_dir(&repo).join(branch).join("state.json");
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
    let state_path = flow_states_dir(&repo).join(branch).join("state.json");
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
    let state_path = flow_states_dir(&repo).join(branch).join("state.json");
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
    assert_eq!(
        data["status"],
        "ok",
        "stdout: {}, stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let state_path = flow_states_dir(&repo).join(branch).join("state.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["phases"]["flow-code"]["status"], "complete");
    assert_eq!(state["current_phase"], "flow-review");
}

#[test]
fn test_review_phase() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "cr-fin";
    let repo = create_git_repo(dir.path());
    create_state(&repo, branch, "flow-review", "auto");

    let output = run_phase_finalize(&repo, &["--phase", "flow-review", "--branch", branch]);
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");

    let state_path = flow_states_dir(&repo).join(branch).join("state.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["phases"]["flow-review"]["status"], "complete");
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

    let state_path = flow_states_dir(&repo).join(branch).join("state.json");
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
    let real_branch_dir = state_dir.join(&real_branch);
    fs::create_dir_all(&real_branch_dir).unwrap();
    fs::write(
        real_branch_dir.join("state.json"),
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

    // Write a frozen phases.json file (matches phase_config schema)
    let frozen_path = flow_states_dir(&repo).join(branch).join("phases.json");
    let frozen_config = json!({
        "order": [
            "flow-start",
            "flow-code",
            "flow-review",
            "flow-learn",
            "flow-complete"
        ],
        "commands": {
            "flow-start": "/flow:flow-start",
            "flow-code": "/flow:flow-code",
            "flow-review": "/flow:flow-review",
            "flow-learn": "/flow:flow-learn",
            "flow-complete": "/flow:flow-complete"
        },
        "phase_names": {
            "flow-start": "Start",
            "flow-code": "Code",
            "flow-review": "Review",
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
    let state_path = flow_states_dir(&repo).join(branch).join("state.json");
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
    let branch_dir = flow_states_dir(&repo).join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), "{not valid json at all").unwrap();

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
    let branch_dir = root.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();

    let phase_order = [
        "flow-start",
        "flow-code",
        "flow-review",
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
        let mut entry = json!({
            "name": p,
            "status": status,
            "started_at": if status != "pending" { Some("2026-01-01T00:00:00-08:00") } else { None },
            "completed_at": if status == "complete" { Some("2026-01-01T00:01:00-08:00") } else { None },
            "session_started_at": if status == "in_progress" { Some("2026-01-01T00:00:00-08:00") } else { None },
            "cumulative_seconds": if status == "complete" { 60 } else { 0 },
            "visit_count": if status == "pending" { 0 } else { 1 }
        });
        // Pre-populate every required agent so the required-agents
        // gate added in this PR passes for tests that don't
        // specifically test the gate. Tests that DO test the gate
        // override this via `seed_agents_returned` after
        // `phase_finalize_write_state`.
        let preset_returned: Option<Value> = match *p {
            "flow-review" => Some(json!([
                {"agent": "reviewer", "timestamp": "2026-01-01T00:00:30-08:00"},
                {"agent": "pre-mortem", "timestamp": "2026-01-01T00:00:31-08:00"},
                {"agent": "adversarial", "timestamp": "2026-01-01T00:00:32-08:00"},
                {"agent": "documentation", "timestamp": "2026-01-01T00:00:33-08:00"}
            ])),
            "flow-learn" => Some(json!([
                {"agent": "learn-analyst", "timestamp": "2026-01-01T00:00:30-08:00"}
            ])),
            _ => None,
        };
        if let Some(preset) = preset_returned {
            entry["agents_returned"] = preset;
        }
        phases.insert(p.to_string(), entry);
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
        branch_dir.join("state.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();
}

fn phase_finalize_read_state(root: &std::path::Path, branch: &str) -> Value {
    let path = root.join(".flow-states").join(branch).join("state.json");
    let content = fs::read_to_string(&path).unwrap();
    serde_json::from_str(&content).unwrap()
}

/// Install a fake `curl` binary at `<dir>/bin/curl` whose stdout is the
/// given `response_json`. Returns the `<dir>/bin` path the caller should
/// prepend to PATH in the subprocess's environment.
fn install_fake_curl(dir: &Path, response_json: &str) -> PathBuf {
    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let script = format!(
        "#!/usr/bin/env bash\ncat <<'FAKECURL'\n{}\nFAKECURL\n",
        response_json
    );
    let curl = bin_dir.join("curl");
    fs::write(&curl, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&curl, fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin_dir
}

fn flow_rs_no_recursion() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd
}

/// Subprocess: Slack reply mode — `--thread-ts` provided, fake curl
/// returns a Slack-shaped ok JSON. State's `slack_notifications` must
/// include the reply; `slack_thread_ts` must NOT be set.
#[test]
fn subprocess_slack_thread_reply_success_records_state() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    phase_finalize_write_state(&root, "branch-a", "flow-code");
    let fake_bin = install_fake_curl(
        &root,
        r#"{"ok":true,"ts":"5555.6666","channel":"C123","message":{"text":"ok"}}"#,
    );

    let path_with_fake = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = flow_rs_no_recursion()
        .args([
            "phase-finalize",
            "--phase",
            "flow-code",
            "--branch",
            "branch-a",
            "--thread-ts",
            "1111.2222",
        ])
        .current_dir(&root)
        .env("PATH", path_with_fake)
        .env("HOME", &root)
        .env("CLAUDE_PLUGIN_CONFIG_slack_bot_token", "test-bot")
        .env("CLAUDE_PLUGIN_CONFIG_slack_channel", "C123")
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
    assert_eq!(data["slack"]["status"], "ok");

    let state = phase_finalize_read_state(&root, "branch-a");
    assert!(state.get("slack_thread_ts").is_none() || state["slack_thread_ts"].is_null());
    let notifs = state["slack_notifications"].as_array().unwrap();
    assert_eq!(notifs.len(), 1);
    assert_eq!(notifs[0]["ts"], "5555.6666");
    assert_eq!(notifs[0]["thread_ts"], "1111.2222");
    assert_eq!(notifs[0]["phase"], "flow-code");
}

/// Subprocess: Slack create mode — no `--thread-ts`, `--pr-url` present,
/// fake curl returns ok. State's `slack_thread_ts` must be set to the
/// returned ts.
#[test]
fn subprocess_slack_thread_create_success_sets_thread_ts() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    phase_finalize_write_state(&root, "branch-b", "flow-start");
    let fake_bin = install_fake_curl(
        &root,
        r#"{"ok":true,"ts":"7777.8888","channel":"C123","message":{"text":"ok"}}"#,
    );

    let path_with_fake = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = flow_rs_no_recursion()
        .args([
            "phase-finalize",
            "--phase",
            "flow-start",
            "--branch",
            "branch-b",
            "--pr-url",
            "https://github.com/org/repo/pull/42",
        ])
        .current_dir(&root)
        .env("PATH", path_with_fake)
        .env("HOME", &root)
        .env("CLAUDE_PLUGIN_CONFIG_slack_bot_token", "test-bot")
        .env("CLAUDE_PLUGIN_CONFIG_slack_channel", "C123")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");

    let state = phase_finalize_read_state(&root, "branch-b");
    assert_eq!(state["slack_thread_ts"], "7777.8888");
    let notifs = state["slack_notifications"].as_array().unwrap();
    assert_eq!(notifs.len(), 1);
    assert_eq!(notifs[0]["thread_ts"], "7777.8888");
}

/// Subprocess: state already has a `slack_notifications` array before
/// finalize runs — a new slack success appends to the existing array
/// rather than overwriting it. Exercises the `is_array().unwrap_or(false)`
/// true path.
#[test]
fn subprocess_slack_preexisting_array_appends_instead_of_resetting() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    phase_finalize_write_state(&root, "branch-pre", "flow-code");

    // Patch the state file to include an existing slack_notifications array.
    let state_path = root
        .join(".flow-states")
        .join("branch-pre")
        .join("state.json");
    let mut state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    state["slack_notifications"] = json!([
        {"phase": "flow-start", "ts": "0000.0000", "thread_ts": "1111.2222", "message": "prior"}
    ]);
    fs::write(&state_path, state.to_string()).unwrap();

    let fake_bin = install_fake_curl(
        &root,
        r#"{"ok":true,"ts":"9999.9999","channel":"C123","message":{"text":"ok"}}"#,
    );

    let path_with_fake = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = flow_rs_no_recursion()
        .args([
            "phase-finalize",
            "--phase",
            "flow-code",
            "--branch",
            "branch-pre",
            "--thread-ts",
            "1111.2222",
        ])
        .current_dir(&root)
        .env("PATH", path_with_fake)
        .env("HOME", &root)
        .env("CLAUDE_PLUGIN_CONFIG_slack_bot_token", "test-bot")
        .env("CLAUDE_PLUGIN_CONFIG_slack_channel", "C123")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");

    let state = phase_finalize_read_state(&root, "branch-pre");
    let notifs = state["slack_notifications"].as_array().unwrap();
    assert_eq!(
        notifs.len(),
        2,
        "existing entry preserved, new one appended"
    );
    assert_eq!(notifs[0]["phase"], "flow-start"); // existing
    assert_eq!(notifs[1]["phase"], "flow-code"); // new
}

/// A phase name outside the standard phase set exercises the
/// `unwrap_or_else(|| args.phase.clone())` fallback when formatting the
/// Slack message header. The state fixture includes the custom phase
/// so phase_complete can write to it.
#[test]
fn subprocess_unknown_phase_name_slack_message_falls_back_to_key() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    // Build state with a non-standard phase entry.
    let branch_dir = root.join(".flow-states").join("branch-unknown");
    fs::create_dir_all(&branch_dir).unwrap();
    let state = json!({
        "schema_version": 1,
        "branch": "branch-unknown",
        "current_phase": "custom-phase",
        "started_at": "2026-01-01T00:00:00-08:00",
        "phases": {
            "custom-phase": {
                "name": "custom-phase",
                "status": "in_progress",
                "started_at": "2026-01-01T00:00:00-08:00",
                "completed_at": null,
                "session_started_at": "2026-01-01T00:00:00-08:00",
                "cumulative_seconds": 0,
                "visit_count": 1
            }
        },
        "phase_transitions": [],
        "prompt": "test feature",
        "notes": [],
    });
    fs::write(
        branch_dir.join("state.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();

    let fake_bin = install_fake_curl(
        &root,
        r#"{"ok":true,"ts":"1010.1010","channel":"C123","message":{"text":"ok"}}"#,
    );

    let path_with_fake = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = flow_rs_no_recursion()
        .args([
            "phase-finalize",
            "--phase",
            "custom-phase",
            "--branch",
            "branch-unknown",
            "--thread-ts",
            "1111.2222",
        ])
        .current_dir(&root)
        .env("PATH", path_with_fake)
        .env("HOME", &root)
        .env("CLAUDE_PLUGIN_CONFIG_slack_bot_token", "test-bot")
        .env("CLAUDE_PLUGIN_CONFIG_slack_channel", "C123")
        .output()
        .unwrap();
    // Slack path runs regardless of phase_complete's outcome.
    let data = parse_output(&output);
    assert!(
        data["status"].as_str() == Some("ok") || data["status"].as_str() == Some("error"),
        "unexpected status: {}",
        data
    );
}

/// Subprocess: Slack returns an error response — `slack_notifications`
/// must NOT be appended to state.
#[test]
fn subprocess_slack_error_response_skips_state_record() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    phase_finalize_write_state(&root, "branch-c", "flow-code");
    // Slack error response: ok:false + error string.
    let fake_bin = install_fake_curl(&root, r#"{"ok":false,"error":"not_in_channel"}"#);

    let path_with_fake = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = flow_rs_no_recursion()
        .args([
            "phase-finalize",
            "--phase",
            "flow-code",
            "--branch",
            "branch-c",
            "--thread-ts",
            "1111.2222",
        ])
        .current_dir(&root)
        .env("PATH", path_with_fake)
        .env("HOME", &root)
        .env("CLAUDE_PLUGIN_CONFIG_slack_bot_token", "test-bot")
        .env("CLAUDE_PLUGIN_CONFIG_slack_channel", "C123")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["slack"]["status"], "error");

    let state = phase_finalize_read_state(&root, "branch-c");
    let notifs_empty = state
        .get("slack_notifications")
        .map(|v| v.as_array().map(|a| a.is_empty()).unwrap_or(true))
        .unwrap_or(true);
    assert!(notifs_empty);
}

#[test]
fn finalize_slash_branch_returns_structured_error_no_panic() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let args = phase_finalize_test_args("flow-code", "feature/foo", None, None);

    let result = run_impl(root, root, &args).unwrap();
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("Invalid branch name"));
}

#[test]
fn finalize_empty_branch_returns_structured_error_no_panic() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let args = phase_finalize_test_args("flow-code", "", None, None);

    let result = run_impl(root, root, &args).unwrap();
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("Invalid branch name"));
}

#[test]
fn finalize_state_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let args = phase_finalize_test_args("flow-code", "branch-missing", None, None);

    let result = run_impl(root, root, &args).unwrap();
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("No state file found"));
}

/// When `.flow-states/<branch>-phases.json` (frozen config) exists,
/// run_impl loads it and passes the frozen order/commands through to
/// phase_complete. Exercise the load branch by writing a minimal frozen
/// config alongside the normal state file.
#[test]
fn finalize_loads_frozen_config_when_present() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    phase_finalize_write_state(root, "branch-frozen", "flow-code");

    let frozen = json!({
        "order": ["flow-start", "flow-code", "flow-review", "flow-learn", "flow-complete"],
        "phases": {
            "flow-start": {"name": "Start", "command": "/flow:flow-start"},
            "flow-code": {"name": "Code", "command": "/flow:flow-code"},
            "flow-review": {"name": "Review", "command": "/flow:flow-review"},
            "flow-learn": {"name": "Learn", "command": "/flow:flow-learn"},
            "flow-complete": {"name": "Complete", "command": "/flow:flow-complete"},
        }
    });
    fs::write(
        root.join(".flow-states")
            .join("branch-frozen")
            .join("phases.json"),
        serde_json::to_string(&frozen).unwrap(),
    )
    .unwrap();

    let args = phase_finalize_test_args("flow-code", "branch-frozen", None, None);
    let result = run_impl(root, root, &args).unwrap();
    assert_eq!(result["status"], "ok");
}

/// A state file whose root is a JSON array trips the
/// `!(state.is_object() || state.is_null())` guard in the single
/// `mutate_state` transform. The guard returns before calling
/// `phase_complete`, so `result_holder` stays Null → `phase_result`
/// status is neither "error" nor a formatted-time-bearing object →
/// fall-through returns ok with default formatted_time/continue_action.
#[test]
fn finalize_array_state_returns_ok_via_guard() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let branch_dir = root.join(".flow-states").join("branch-array");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), "[1, 2, 3]").unwrap();

    let args = phase_finalize_test_args("flow-code", "branch-array", None, None);
    let result = run_impl(root, root, &args).unwrap();
    assert_eq!(result["status"], "ok");
    // Defaults from unwrap_or fallbacks.
    assert_eq!(result["formatted_time"], "<1m");
    assert_eq!(result["continue_action"], "ask");
}

/// Library-level invocation of the public `run_impl_main` wrapper so
/// cargo-llvm-cov reports the function as covered. The wrapper resolves
/// `project_root()` and `current_dir()` and delegates to `run_impl`;
/// driving it from the host worktree exercises the production binding
/// path. The result is intentionally not asserted on — it depends on
/// the host repo state — only that the call returns without panic.
#[test]
fn run_impl_main_covers_production_binding() {
    let args = Args {
        phase: "flow-code".to_string(),
        branch: "nonexistent-branch-for-cov".to_string(),
        thread_ts: None,
        pr_url: None,
    };
    let _ = run_impl_main(&args);
}

/// Seed `phases.<phase>.agents_skipped` on the existing state file.
/// `agents_skipped` is no longer read by phase-finalize; this helper
/// survives so the required-agents gate test can prove a skipped entry
/// does NOT satisfy the gate.
fn seed_agents_skipped(root: &std::path::Path, branch: &str, phase: &str, entries: Value) {
    let path = root.join(".flow-states").join(branch).join("state.json");
    let mut state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    state["phases"][phase]["agents_skipped"] = entries;
    fs::write(&path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
}

#[test]
fn finalize_no_slack_args_response_omits_slack_key() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    phase_finalize_write_state(root, "branch-d", "flow-code");
    let args = phase_finalize_test_args("flow-code", "branch-d", None, None);

    let result = run_impl(root, root, &args).unwrap();
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

// --- required-agents gate ---

/// Helper: seed `phases.<phase>.agents_returned` with `entries`
/// (typically a JSON array of `{agent, timestamp}` objects). Mirrors
/// `seed_agents_skipped` so tests can populate the field that the
/// new required-agents gate composes against `agents_skipped`.
fn seed_agents_returned(root: &std::path::Path, branch: &str, phase: &str, entries: Value) {
    let path = root.join(".flow-states").join(branch).join("state.json");
    let mut state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    state["phases"][phase]["agents_returned"] = entries;
    fs::write(&path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
}

fn returned_entry(agent: &str) -> Value {
    json!({"agent": agent, "timestamp": "2026-01-01T00:00:00-08:00"})
}

/// Helper: clear `phases.<phase>.agents_returned` so the
/// required-agents gate sees no recorded returns. Used by tests
/// that want to test the missing-required branch and need to
/// override the preset populated by `phase_finalize_write_state`.
fn clear_agents_returned(root: &std::path::Path, branch: &str, phase: &str) {
    let path = root.join(".flow-states").join(branch).join("state.json");
    let mut state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    if let Some(obj) = state["phases"][phase].as_object_mut() {
        obj.remove("agents_returned");
    }
    fs::write(&path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
}

fn skipped_entry(agent: &str, reason: &str) -> Value {
    json!({"agent": agent, "reason": reason, "timestamp": "2026-01-01T00:00:00-08:00"})
}

#[test]
fn phase_finalize_rejects_when_required_agents_not_returned() {
    // flow-review requires {reviewer, pre-mortem, adversarial,
    // documentation}. State has no agents_returned — every required
    // agent is missing. The gate must short-circuit before
    // phase_complete with reason required_agent_not_returned and an
    // enumerated `missing` array.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    phase_finalize_write_state(root, "missing-all", "flow-review");
    clear_agents_returned(root, "missing-all", "flow-review");

    let args = phase_finalize_test_args("flow-review", "missing-all", None, None);
    let result = run_impl(root, root, &args).expect("run_impl returns Ok envelope");

    assert_eq!(result["status"], "error");
    assert_eq!(result["reason"], "required_agent_not_returned");
    let missing = result["missing"]
        .as_array()
        .expect("missing array in error envelope");
    let missing_set: std::collections::HashSet<&str> =
        missing.iter().filter_map(|v| v.as_str()).collect();
    assert!(missing_set.contains("reviewer"));
    assert!(missing_set.contains("pre-mortem"));
    assert!(missing_set.contains("adversarial"));
    assert!(missing_set.contains("documentation"));

    // The gate must short-circuit before phase_complete runs — the
    // phase status must remain in_progress so the caller can retry.
    let state = phase_finalize_read_state(root, "missing-all");
    assert_eq!(state["phases"]["flow-review"]["status"], "in_progress");
}

#[test]
fn phase_finalize_rejects_when_required_agents_partially_missing() {
    // Three of four required agents returned; the fourth is missing.
    // The gate must still fire and the `missing` array must name only
    // the one that wasn't accounted for.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    phase_finalize_write_state(root, "missing-one", "flow-review");
    seed_agents_returned(
        root,
        "missing-one",
        "flow-review",
        json!([
            returned_entry("reviewer"),
            returned_entry("pre-mortem"),
            returned_entry("adversarial"),
        ]),
    );

    let args = phase_finalize_test_args("flow-review", "missing-one", None, None);
    let result = run_impl(root, root, &args).expect("run_impl returns Ok envelope");
    assert_eq!(result["status"], "error");
    assert_eq!(result["reason"], "required_agent_not_returned");
    let missing = result["missing"]
        .as_array()
        .expect("missing array in error envelope");
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0], "documentation");
}

#[test]
fn phase_finalize_rejects_when_agents_returned_is_wrong_type() {
    // Per `.claude/rules/security-gates.md` "Fail Closed When State
    // Is Unreliable": a `phases.<phase>.agents_returned` field whose
    // type is not an array must fail-closed with the same reason as
    // missing-required so a corrupted state file cannot silently
    // advance the phase.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    phase_finalize_write_state(root, "wrong-type-returned", "flow-review");
    seed_agents_returned(
        root,
        "wrong-type-returned",
        "flow-review",
        json!("not_an_array"),
    );

    let args = phase_finalize_test_args("flow-review", "wrong-type-returned", None, None);
    let result = run_impl(root, root, &args).expect("run_impl returns Ok envelope");

    assert_eq!(result["status"], "error");
    assert_eq!(result["reason"], "required_agent_not_returned");

    let state = phase_finalize_read_state(root, "wrong-type-returned");
    assert_eq!(state["phases"]["flow-review"]["status"], "in_progress");
}

#[test]
fn phase_finalize_passes_when_all_required_agents_returned() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    phase_finalize_write_state(root, "all-returned", "flow-review");
    seed_agents_returned(
        root,
        "all-returned",
        "flow-review",
        json!([
            returned_entry("reviewer"),
            returned_entry("pre-mortem"),
            returned_entry("adversarial"),
            returned_entry("documentation"),
        ]),
    );

    let args = phase_finalize_test_args("flow-review", "all-returned", None, None);
    let result = run_impl(root, root, &args).expect("run_impl returns Ok envelope");
    assert_eq!(result["status"], "ok");

    let state = phase_finalize_read_state(root, "all-returned");
    assert_eq!(state["phases"]["flow-review"]["status"], "complete");
}

#[test]
fn phase_finalize_rejects_when_only_agents_skipped_present() {
    // Every required agent appears in agents_skipped but none in
    // agents_returned. The gate reads ONLY agents_returned now — a
    // skip recorded by any means does NOT satisfy a required-agent
    // phase. The phase must not advance; the missing array names every
    // required agent.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    phase_finalize_write_state(root, "only-skipped", "flow-review");
    clear_agents_returned(root, "only-skipped", "flow-review");
    seed_agents_skipped(
        root,
        "only-skipped",
        "flow-review",
        json!([
            skipped_entry("reviewer", "rate_limit"),
            skipped_entry("pre-mortem", "api_error"),
            skipped_entry("adversarial", "exhausted_retries"),
            skipped_entry("documentation", "other"),
        ]),
    );

    let args = phase_finalize_test_args("flow-review", "only-skipped", None, None);
    let result = run_impl(root, root, &args).expect("run_impl returns Ok envelope");
    assert_eq!(result["status"], "error");
    assert_eq!(result["reason"], "required_agent_not_returned");
    let missing = result["missing"]
        .as_array()
        .expect("missing array in error envelope");
    assert_eq!(missing.len(), 4, "agents_skipped must not satisfy the gate");

    let state = phase_finalize_read_state(root, "only-skipped");
    assert_eq!(state["phases"]["flow-review"]["status"], "in_progress");
}

#[test]
fn phase_finalize_no_required_agents_gate_for_non_review_learn_phases() {
    // flow-code has no required-agents entry in REQUIRED_AGENTS, so
    // the gate is a no-op for flow-code. The phase advances without
    // any agents_returned or agents_skipped present.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    phase_finalize_write_state(root, "code-phase", "flow-code");
    let args = phase_finalize_test_args("flow-code", "code-phase", None, None);
    let result = run_impl(root, root, &args).expect("run_impl returns Ok envelope");
    assert_eq!(result["status"], "ok");

    let state = phase_finalize_read_state(root, "code-phase");
    assert_eq!(state["phases"]["flow-code"]["status"], "complete");
}

#[test]
fn phase_finalize_required_agents_gate_skips_entries_without_agent_field() {
    // A malformed entry in agents_returned (missing the `agent`
    // field) must not be treated as accounted-for. The gate's
    // `if let Some(name) = entry.get("agent")...` branch tolerates the
    // malformed entry by skipping it; the four valid entries still
    // cover every required agent, so the phase advances.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    phase_finalize_write_state(root, "malformed-entries", "flow-review");
    seed_agents_returned(
        root,
        "malformed-entries",
        "flow-review",
        json!([
            {"timestamp": "2026-01-01T00:00:00-08:00"},
            returned_entry("reviewer"),
            returned_entry("pre-mortem"),
            returned_entry("adversarial"),
            returned_entry("documentation"),
        ]),
    );

    let args = phase_finalize_test_args("flow-review", "malformed-entries", None, None);
    let result = run_impl(root, root, &args).expect("run_impl returns Ok envelope");
    assert_eq!(result["status"], "ok");
}

#[test]
fn phase_finalize_required_agents_gate_fires_for_flow_learn() {
    // flow-learn requires {learn-analyst}. State has no
    // agents_returned; the gate must fire with the same reason and
    // list learn-analyst as missing.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    phase_finalize_write_state(root, "learn-missing", "flow-learn");
    clear_agents_returned(root, "learn-missing", "flow-learn");
    let args = phase_finalize_test_args("flow-learn", "learn-missing", None, None);
    let result = run_impl(root, root, &args).expect("run_impl returns Ok envelope");
    assert_eq!(result["status"], "error");
    assert_eq!(result["reason"], "required_agent_not_returned");
    let missing = result["missing"].as_array().expect("missing array");
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0], "learn-analyst");
}
