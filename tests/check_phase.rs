//! Integration tests for `flow_rs check-phase`. Drive through the public
//! entry points only — no private helpers imported per
//! `.claude/rules/test-placement.md`.

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::flow_states_dir;
use flow_rs::check_phase::run_impl_main;
use flow_rs::phase_config::PHASE_ORDER;
use serde_json::{json, Value};

/// Build a state-file JSON matching the default phase order.
fn make_state(current_phase: &str, phase_statuses: &[(&str, &str)]) -> Value {
    let order = [
        "flow-start",
        "flow-plan",
        "flow-code",
        "flow-code-review",
        "flow-learn",
        "flow-complete",
    ];
    let names = [
        ("flow-start", "Start"),
        ("flow-plan", "Plan"),
        ("flow-code", "Code"),
        ("flow-code-review", "Code Review"),
        ("flow-learn", "Learn"),
        ("flow-complete", "Complete"),
    ];
    let name_map: std::collections::HashMap<&str, &str> = names.into_iter().collect();
    let status_map: std::collections::HashMap<&str, &str> =
        phase_statuses.iter().copied().collect();

    let mut phases = serde_json::Map::new();
    for p in order {
        let status = status_map.get(p).copied().unwrap_or("pending");
        let name = name_map.get(p).copied().unwrap_or(p);
        let visit_count = if status == "complete" || status == "in_progress" {
            1
        } else {
            0
        };
        phases.insert(
            p.to_string(),
            json!({
                "name": name,
                "status": status,
                "started_at": null,
                "completed_at": null,
                "session_started_at": null,
                "cumulative_seconds": 0,
                "visit_count": visit_count,
            }),
        );
    }

    json!({
        "branch": "test-feature",
        "current_phase": current_phase,
        "phases": phases,
    })
}

fn write_state(root: &Path, branch: &str, state: Value) {
    let branch_dir = root.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), state.to_string()).unwrap();
}

fn setup_state(dir: &Path, branch: &str, state_json: &Value) {
    let branch_dir = flow_states_dir(dir).join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), state_json.to_string()).unwrap();
}

fn setup_git_repo(dir: &Path, branch: &str) {
    Command::new("git")
        .args(["-c", "init.defaultBranch=main", "init"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "commit.gpgsign", "false"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["checkout", "-b", branch])
        .current_dir(dir)
        .output()
        .unwrap();
}

fn flow_rs() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd
}

// --- CLI integration tests ---

#[test]
fn phase_1_always_exits_0() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");

    let output = flow_rs()
        .env_remove("FLOW_SIMULATE_BRANCH")
        .args(["check-phase", "--required", "flow-start"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn no_state_file_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");

    let output = flow_rs()
        .env_remove("FLOW_SIMULATE_BRANCH")
        .args(["check-phase", "--required", "flow-plan"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("/flow:flow-start"));
}

#[test]
fn previous_phase_pending_blocks_cli() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");

    let state = make_state("flow-plan", &[("flow-start", "pending")]);
    setup_state(dir.path(), "test-feature", &state);

    let output = flow_rs()
        .env_remove("FLOW_SIMULATE_BRANCH")
        .args(["check-phase", "--required", "flow-plan"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("BLOCKED"));
    assert!(stdout.contains("pending"));
}

#[test]
fn previous_phase_complete_allows_cli() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");

    let state = make_state("flow-plan", &[("flow-start", "complete")]);
    setup_state(dir.path(), "test-feature", &state);

    let output = flow_rs()
        .env_remove("FLOW_SIMULATE_BRANCH")
        .args(["check-phase", "--required", "flow-plan"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn branch_flag_uses_specified_state_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "main");

    let state = make_state("flow-plan", &[("flow-start", "complete")]);
    setup_state(dir.path(), "other-feature", &state);

    let output = flow_rs()
        .env_remove("FLOW_SIMULATE_BRANCH")
        .args([
            "check-phase",
            "--required",
            "flow-plan",
            "--branch",
            "other-feature",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn no_state_file_for_current_branch() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "main");

    for name in ["feat-a", "feat-b"] {
        let state = make_state("flow-plan", &[("flow-start", "complete")]);
        setup_state(dir.path(), name, &state);
    }

    let output = flow_rs()
        .env_remove("FLOW_SIMULATE_BRANCH")
        .args(["check-phase", "--required", "flow-plan"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No FLOW feature in progress on branch"),
        "Expected 'No FLOW feature in progress' but got: {}",
        stdout
    );
}

#[test]
fn frozen_phases_file_is_loaded() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");

    let state = make_state("flow-plan", &[("flow-start", "complete")]);
    setup_state(dir.path(), "test-feature", &state);

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let source = std::path::PathBuf::from(manifest_dir).join("flow-phases.json");
    let dest = flow_states_dir(dir.path())
        .join("test-feature")
        .join("phases.json");
    fs::copy(source, dest).unwrap();

    let output = flow_rs()
        .env_remove("FLOW_SIMULATE_BRANCH")
        .args(["check-phase", "--required", "flow-plan"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
}

/// `check-phase` from a non-git cwd with no --branch returns "Could not
/// determine current git branch". Exercises the resolve_branch->None
/// arm; verified via subprocess so the test process's own git cwd does
/// not interfere.
#[test]
fn cli_no_branch_in_non_git_cwd_blocks() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().canonicalize().unwrap();

    let output = flow_rs()
        .env_remove("FLOW_SIMULATE_BRANCH")
        .env("FLOW_SIMULATE_BRANCH", "")
        .args(["check-phase", "--required", "flow-plan"])
        .current_dir(&dir)
        .env("GIT_CEILING_DIRECTORIES", &dir)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Could not determine current git branch"),
        "expected branch-resolution block, got: {}",
        stdout
    );
}

// --- run_impl_main tests (library-level via public entry point) ---

#[test]
fn run_impl_main_first_phase_returns_empty_and_exit_0() {
    let dir = tempfile::tempdir().unwrap();
    let (out, code) = run_impl_main(PHASE_ORDER[0], Some("any"), dir.path());
    assert_eq!(code, 0);
    assert!(out.is_empty());
}

#[test]
fn run_impl_main_no_state_file_returns_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let (out, code) = run_impl_main("flow-plan", Some("test"), dir.path());
    assert_eq!(code, 1);
    assert!(out.contains("BLOCKED"));
    assert!(out.contains("No FLOW feature in progress"));
    assert!(out.contains("test"));
}

#[test]
fn run_impl_main_loads_frozen_phase_config_when_present() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let branch = "test-frozen-load";
    let branch_dir = root.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    let state = make_state(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    fs::write(
        branch_dir.join("state.json"),
        serde_json::to_string(&state).unwrap(),
    )
    .unwrap();

    let frozen = json!({
        "order": ["flow-start", "flow-plan", "flow-code", "flow-code-review", "flow-learn", "flow-complete"],
        "phases": {
            "flow-start": {"name": "Start", "command": "/flow:flow-start"},
            "flow-plan": {"name": "Plan", "command": "/flow:flow-plan"},
            "flow-code": {"name": "Code", "command": "/flow:flow-code"},
            "flow-code-review": {"name": "Code Review", "command": "/flow:flow-code-review"},
            "flow-learn": {"name": "Learn", "command": "/flow:flow-learn"},
            "flow-complete": {"name": "Complete", "command": "/flow:flow-complete"},
        }
    });
    fs::write(
        branch_dir.join("phases.json"),
        serde_json::to_string(&frozen).unwrap(),
    )
    .unwrap();

    let (output, code) = run_impl_main("flow-plan", Some(branch), &root);
    assert_eq!(code, 0);
    assert!(output.is_empty());
}

#[test]
fn run_impl_main_state_file_is_directory_returns_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let branch_dir = root.join(".flow-states").join("test-feature");
    fs::create_dir_all(&branch_dir).unwrap();
    // Make state.json a directory so reading it fails.
    fs::create_dir(branch_dir.join("state.json")).unwrap();

    let (output, code) = run_impl_main("flow-plan", Some("test-feature"), &root);
    assert_eq!(code, 1);
    assert!(output.contains("BLOCKED: Could not read state file"));
}

#[test]
fn run_impl_main_unparseable_state_file_returns_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let branch_dir = dir.path().join(".flow-states").join("test");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), "not-valid-json").unwrap();
    let (out, code) = run_impl_main("flow-plan", Some("test"), dir.path());
    assert_eq!(code, 1);
    assert!(out.contains("BLOCKED"));
    assert!(out.contains("Could not read state file"));
}

#[test]
fn run_impl_main_allowed_returns_zero() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_state("flow-plan", &[("flow-start", "complete")]);
    write_state(dir.path(), "test", state);
    let (out, code) = run_impl_main("flow-plan", Some("test"), dir.path());
    assert_eq!(code, 0);
    assert!(out.is_empty());
}

#[test]
fn run_impl_main_blocked_returns_one_with_blocked_message() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_state("flow-plan", &[("flow-start", "pending")]);
    write_state(dir.path(), "test", state);
    let (out, code) = run_impl_main("flow-plan", Some("test"), dir.path());
    assert_eq!(code, 1);
    assert!(out.contains("BLOCKED"));
}

#[test]
fn run_impl_main_previous_phase_in_progress_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_state("flow-plan", &[("flow-start", "in_progress")]);
    write_state(dir.path(), "test", state);
    let (out, code) = run_impl_main("flow-plan", Some("test"), dir.path());
    assert_eq!(code, 1);
    assert!(out.contains("BLOCKED"));
    assert!(out.contains("in_progress"));
}

#[test]
fn run_impl_main_sequential_chain_phase_4_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_state(
        "flow-code-review",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "complete"),
        ],
    );
    write_state(dir.path(), "test", state);
    let (_, code) = run_impl_main("flow-code-review", Some("test"), dir.path());
    assert_eq!(code, 0);
}

#[test]
fn run_impl_main_reentry_returns_note_and_zero() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_state(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "complete")],
    );
    state["phases"]["flow-plan"]["visit_count"] = json!(2);
    write_state(dir.path(), "test", state);
    let (out, code) = run_impl_main("flow-plan", Some("test"), dir.path());
    assert_eq!(code, 0);
    assert!(out.contains("previously completed"));
    assert!(out.contains("2 visit(s)"));
}

#[test]
fn run_impl_main_reentry_missing_visit_count_reports_zero() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_state(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "complete")],
    );
    state["phases"]["flow-plan"]
        .as_object_mut()
        .unwrap()
        .remove("visit_count");
    write_state(dir.path(), "test", state);
    let (out, code) = run_impl_main("flow-plan", Some("test"), dir.path());
    assert_eq!(code, 0);
    assert!(out.contains("previously completed"));
    assert!(out.contains("0 visit(s)"));
}

#[test]
fn run_impl_main_first_visit_no_previously_completed_message() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_state("flow-plan", &[("flow-start", "complete")]);
    write_state(dir.path(), "test", state);
    let (out, _) = run_impl_main("flow-plan", Some("test"), dir.path());
    assert!(!out.contains("previously completed"));
}

#[test]
fn run_impl_main_phase_5_requires_phase_4_complete() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_state(
        "flow-learn",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "complete"),
            ("flow-code-review", "pending"),
        ],
    );
    write_state(dir.path(), "test", state);
    let (out, code) = run_impl_main("flow-learn", Some("test"), dir.path());
    assert_eq!(code, 1);
    assert!(out.contains("Phase 4"));
}

#[test]
fn run_impl_main_phase_6_requires_phase_5_complete() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_state(
        "flow-complete",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "complete"),
            ("flow-code-review", "complete"),
            ("flow-learn", "pending"),
        ],
    );
    write_state(dir.path(), "test", state);
    let (out, code) = run_impl_main("flow-complete", Some("test"), dir.path());
    assert_eq!(code, 1);
    assert!(out.contains("Phase 5"));
}

#[test]
fn run_impl_main_missing_phases_key_blocks() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".flow-states")).unwrap();
    fs::write(
        dir.path().join(".flow-states").join("test.json"),
        json!({"branch": "test", "current_phase": "flow-plan"}).to_string(),
    )
    .unwrap();
    let (out, code) = run_impl_main("flow-plan", Some("test"), dir.path());
    assert_eq!(code, 1);
    assert!(out.contains("BLOCKED"));
}

#[test]
fn run_impl_main_blocked_message_includes_correct_command() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_state(
        "flow-code-review",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "pending"),
        ],
    );
    write_state(dir.path(), "test", state);
    let (out, code) = run_impl_main("flow-code-review", Some("test"), dir.path());
    assert_eq!(code, 1);
    assert!(out.contains("/flow:flow-code"));
}

#[test]
fn run_impl_main_slash_branch_returns_blocked_no_panic() {
    let dir = tempfile::tempdir().unwrap();
    let (out, code) = run_impl_main("flow-plan", Some("feature/foo"), dir.path());
    assert_eq!(code, 1);
    assert!(out.contains("BLOCKED"));
    assert!(out.contains("feature/foo"));
}

#[test]
fn run_impl_main_empty_branch_returns_blocked_no_panic() {
    let dir = tempfile::tempdir().unwrap();
    let (out, code) = run_impl_main("flow-plan", Some(""), dir.path());
    assert_eq!(code, 1);
    assert!(out.contains("BLOCKED"));
}

#[test]
fn run_impl_main_invalid_phase_returns_json_error() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_state("flow-start", &[("flow-start", "complete")]);
    write_state(dir.path(), "test", state);
    let (out, code) = run_impl_main("nonexistent", Some("test"), dir.path());
    assert_eq!(code, 1);
    let parsed: Value = serde_json::from_str(&out).expect("invalid-phase path emits JSON");
    assert_eq!(parsed["status"], "error");
    assert!(parsed["message"]
        .as_str()
        .unwrap()
        .contains("Invalid phase"));
}

/// Frozen phase config with missing `name` and `command` entries falls
/// back to the phase key itself for name and `/flow:<key>` for command.
#[test]
fn run_impl_main_frozen_config_missing_names_and_commands_falls_back() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let branch = "fx";
    let branch_dir = root.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    let state = make_state("flow-plan", &[("flow-start", "pending")]);
    fs::write(
        branch_dir.join("state.json"),
        serde_json::to_string(&state).unwrap(),
    )
    .unwrap();

    // Frozen config where phase entries omit both `name` AND `command`
    // so check_phase hits both fallback arms in the message formatter.
    let frozen = json!({
        "order": ["flow-start", "flow-plan"],
        "phases": {
            "flow-start": {},
            "flow-plan": {},
        }
    });
    fs::write(
        branch_dir.join("phases.json"),
        serde_json::to_string(&frozen).unwrap(),
    )
    .unwrap();

    let (output, code) = run_impl_main("flow-plan", Some(branch), &root);
    assert_eq!(code, 1);
    assert!(output.contains("BLOCKED"));
    assert!(output.contains("flow-start"));
    assert!(output.contains("/flow:flow-start"));
}

/// A frozen phase config whose first phase differs from the default
/// `PHASE_ORDER[0]` (`flow-start`) must still short-circuit on the
/// frozen first phase. Exercises `check_phase`'s `phase_idx == 0`
/// return arm, which run_impl_main's own PHASE_ORDER[0] short-circuit
/// cannot reach when the frozen order differs.
#[test]
fn run_impl_main_frozen_first_phase_different_from_default() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let branch = "fxfirst";
    let branch_dir = root.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();

    let state = make_state("flow-plan", &[]);
    fs::write(
        branch_dir.join("state.json"),
        serde_json::to_string(&state).unwrap(),
    )
    .unwrap();

    // Frozen config whose first phase is "flow-plan" (not the default
    // "flow-start"). Requesting "flow-plan" reaches check_phase because
    // run_impl_main only short-circuits on PHASE_ORDER[0] (="flow-start").
    // Inside check_phase, phase_idx=0 under the frozen order, so the
    // first-phase-short-circuit arm fires.
    let frozen = json!({
        "order": ["flow-plan", "flow-code"],
        "phases": {
            "flow-plan": {"name": "Plan", "command": "/flow:flow-plan"},
            "flow-code": {"name": "Code", "command": "/flow:flow-code"},
        }
    });
    fs::write(
        branch_dir.join("phases.json"),
        serde_json::to_string(&frozen).unwrap(),
    )
    .unwrap();

    let (output, code) = run_impl_main("flow-plan", Some(branch), &root);
    assert_eq!(code, 0);
    assert!(
        output.is_empty(),
        "expected empty output for first-phase allowed, got: {}",
        output
    );
}
