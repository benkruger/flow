//! Tests for `bin/flow add-skipped-agent` — appends a skipped-agent
//! entry to `phases.<phase>.agents_skipped` in the state file.
//!
//! Consumed by `phase-finalize` to gate flow-review completion when
//! one or more agents were skipped during the phase. The Review skill
//! invokes this subcommand from its failure-classification logic when
//! an agent's response carries canonical external-failure markers and
//! no structured `**Finding` block.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_git_repo_with_remote, parse_output};
use serde_json::{json, Value};

fn flow_rs_no_recursion() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd
}

fn write_state(repo: &Path, branch: &str, state: &Value) -> std::path::PathBuf {
    let branch_dir = repo.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    let path = branch_dir.join("state.json");
    fs::write(&path, serde_json::to_string_pretty(state).unwrap()).unwrap();
    path
}

fn run_add_skipped_agent(repo: &Path, args: &[&str]) -> Output {
    flow_rs_no_recursion()
        .arg("add-skipped-agent")
        .args(args)
        .current_dir(repo)
        .env("HOME", repo)
        .env("GH_TOKEN", "invalid")
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

// --- Task 10: appends entry ---

#[test]
fn add_skipped_agent_appends_entry_to_phase_agents_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({
        "branch": "b",
        "phases": {"flow-review": {"status": "in_progress"}},
    });
    let state_path = write_state(&repo, "b", &state);

    let output = run_add_skipped_agent(
        &repo,
        &[
            "--branch",
            "b",
            "--agent",
            "reviewer",
            "--reason",
            "rate_limit",
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

    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    let skipped = on_disk["phases"]["flow-review"]["agents_skipped"]
        .as_array()
        .expect("agents_skipped array");
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["agent"], "reviewer");
    assert_eq!(skipped[0]["reason"], "rate_limit");
    assert!(
        skipped[0]["timestamp"].is_string(),
        "timestamp field must be set"
    );
    let ts = skipped[0]["timestamp"].as_str().unwrap();
    // Pacific Time ISO 8601 — contains "T" date/time separator and
    // either a "-08:00"/"−08:00" offset or "PST"/"PDT" — assert the
    // separator only since the offset varies by season.
    assert!(ts.contains('T'), "timestamp should be ISO 8601: {}", ts);
}

#[test]
fn add_skipped_agent_multiple_invocations_append() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({
        "branch": "b",
        "phases": {"flow-review": {"status": "in_progress"}},
    });
    let state_path = write_state(&repo, "b", &state);

    let first = run_add_skipped_agent(
        &repo,
        &[
            "--branch",
            "b",
            "--agent",
            "reviewer",
            "--reason",
            "rate_limit",
        ],
    );
    assert_eq!(first.status.code(), Some(0));

    let second = run_add_skipped_agent(
        &repo,
        &[
            "--branch",
            "b",
            "--agent",
            "pre-mortem",
            "--reason",
            "api_error",
        ],
    );
    assert_eq!(second.status.code(), Some(0));

    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    let skipped = on_disk["phases"]["flow-review"]["agents_skipped"]
        .as_array()
        .expect("agents_skipped array");
    assert_eq!(
        skipped.len(),
        2,
        "second invocation must append, not replace"
    );
    assert_eq!(skipped[0]["agent"], "reviewer");
    assert_eq!(skipped[0]["reason"], "rate_limit");
    assert_eq!(skipped[1]["agent"], "pre-mortem");
    assert_eq!(skipped[1]["reason"], "api_error");
}

#[test]
fn add_skipped_agent_creates_agents_skipped_array_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // State has phases.flow-review but no agents_skipped field —
    // the subcommand must initialize the array on first append.
    let state = json!({
        "branch": "b",
        "phases": {"flow-review": {"status": "in_progress"}},
    });
    let state_path = write_state(&repo, "b", &state);

    let output = run_add_skipped_agent(
        &repo,
        &[
            "--branch",
            "b",
            "--agent",
            "documentation",
            "--reason",
            "other",
        ],
    );
    assert_eq!(output.status.code(), Some(0));

    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    let skipped = on_disk["phases"]["flow-review"]["agents_skipped"]
        .as_array()
        .unwrap();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["agent"], "documentation");
}

// --- Task 11: reason allowlist ---

#[test]
fn add_skipped_agent_rejects_unknown_reason() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({"branch": "b", "phases": {"flow-review": {}}});
    let state_path = write_state(&repo, "b", &state);
    let pre = fs::read_to_string(&state_path).unwrap();

    let output = run_add_skipped_agent(
        &repo,
        &["--branch", "b", "--agent", "reviewer", "--reason", "foo"],
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    let msg = data["message"].as_str().unwrap();
    assert!(
        msg.contains("rate_limit") && msg.contains("api_error") && msg.contains("other"),
        "error message must enumerate the allowlist: {}",
        msg
    );

    let post = fs::read_to_string(&state_path).unwrap();
    assert_eq!(pre, post, "state file must be unchanged on rejection");
}

#[test]
fn add_skipped_agent_rejects_empty_reason() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({"branch": "b", "phases": {"flow-review": {}}});
    write_state(&repo, "b", &state);

    let output = run_add_skipped_agent(
        &repo,
        &["--branch", "b", "--agent", "reviewer", "--reason", ""],
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
}

#[test]
fn add_skipped_agent_rejects_reason_with_space() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({"branch": "b", "phases": {"flow-review": {}}});
    write_state(&repo, "b", &state);

    let output = run_add_skipped_agent(
        &repo,
        &[
            "--branch",
            "b",
            "--agent",
            "reviewer",
            "--reason",
            "rate limit",
        ],
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
}

#[test]
fn add_skipped_agent_normalizes_reason_case_and_whitespace() {
    // Per security-gates.md "Normalize Before Comparing", the gate
    // should accept "  RATE_LIMIT  " (trim + lowercase) as
    // equivalent to "rate_limit" so callers cannot defeat the
    // allowlist by varying case or padding.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({"branch": "b", "phases": {"flow-review": {}}});
    let state_path = write_state(&repo, "b", &state);

    let output = run_add_skipped_agent(
        &repo,
        &[
            "--branch",
            "b",
            "--agent",
            "reviewer",
            "--reason",
            "  RATE_LIMIT  ",
        ],
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(
        on_disk["phases"]["flow-review"]["agents_skipped"][0]["reason"], "rate_limit",
        "normalized reason must be stored, not the raw input"
    );
}

// --- Task 12: branch and state errors ---

#[test]
fn add_skipped_agent_rejects_slash_branch() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());

    let output = run_add_skipped_agent(
        &repo,
        &[
            "--branch",
            "feat/slash",
            "--agent",
            "reviewer",
            "--reason",
            "rate_limit",
        ],
    );

    assert_eq!(output.status.code(), Some(0), "business errors must exit 0");
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
}

#[test]
fn add_skipped_agent_rejects_empty_branch() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());

    let output = run_add_skipped_agent(
        &repo,
        &[
            "--branch",
            "",
            "--agent",
            "reviewer",
            "--reason",
            "rate_limit",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
}

#[test]
fn add_skipped_agent_rejects_dot_branch() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());

    let output = run_add_skipped_agent(
        &repo,
        &[
            "--branch",
            "..",
            "--agent",
            "reviewer",
            "--reason",
            "rate_limit",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
}

#[test]
fn add_skipped_agent_returns_error_when_state_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // No state file written — the subcommand must surface a
    // structured error rather than panicking.

    let output = run_add_skipped_agent(
        &repo,
        &[
            "--branch",
            "no-state",
            "--agent",
            "reviewer",
            "--reason",
            "rate_limit",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
}

// --- corruption resilience ---

#[test]
fn add_skipped_agent_skips_mutation_when_state_root_is_array() {
    // Per .claude/rules/rust-patterns.md "State Mutation Object Guards",
    // an IndexMut on a wrong-root-type Value panics. The guard must
    // skip the mutation and leave the state file unchanged.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let branch_dir = repo.join(".flow-states").join("array-root");
    fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    fs::write(&state_path, "[\"not\", \"an\", \"object\"]").unwrap();
    let pre: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();

    let output = run_add_skipped_agent(
        &repo,
        &[
            "--branch",
            "array-root",
            "--agent",
            "reviewer",
            "--reason",
            "rate_limit",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let post: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(post, pre, "array-root state must be unchanged");
}

#[test]
fn add_skipped_agent_initializes_phases_when_missing() {
    // State file with no phases key — the subcommand must create
    // phases, the named phase, and the agents_skipped array.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({"branch": "b"});
    let state_path = write_state(&repo, "b", &state);

    let output = run_add_skipped_agent(
        &repo,
        &["--branch", "b", "--agent", "reviewer", "--reason", "other"],
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");

    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(
        on_disk["phases"]["flow-review"]["agents_skipped"][0]["agent"],
        "reviewer"
    );
}

#[test]
fn add_skipped_agent_initializes_named_phase_when_missing() {
    // State has phases but no entry for the specified --phase.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = json!({"branch": "b", "phases": {"flow-code": {}}});
    let state_path = write_state(&repo, "b", &state);

    let output = run_add_skipped_agent(
        &repo,
        &[
            "--branch",
            "b",
            "--agent",
            "reviewer",
            "--reason",
            "api_error",
            "--phase",
            "flow-review",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let on_disk: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(
        on_disk["phases"]["flow-review"]["agents_skipped"][0]["agent"],
        "reviewer"
    );
    // The pre-existing flow-code phase must remain untouched.
    assert!(on_disk["phases"]["flow-code"].is_object());
}

#[test]
fn add_skipped_agent_returns_error_when_state_file_is_invalid_json() {
    // mutate_state returns Err on JSON parse failure — exercises the
    // Err arm of the run_impl_main match.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let branch_dir = repo.join(".flow-states").join("bad-json");
    fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    fs::write(&state_path, "this is not valid json at all").unwrap();

    let output = run_add_skipped_agent(
        &repo,
        &[
            "--branch",
            "bad-json",
            "--agent",
            "reviewer",
            "--reason",
            "rate_limit",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap()
        .contains("failed to add skipped-agent"));
}
