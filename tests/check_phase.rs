mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::flow_states_dir;
use flow_rs::check_phase::{check_phase, run_impl_main, run_impl_main_with_resolver};
use flow_rs::phase_config::{self, PhaseConfig, PHASE_ORDER};
use indexmap::IndexMap;
use serde_json::{json, Value};

fn make_state(current_phase: &str, phase_statuses: &[(&str, &str)]) -> String {
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

    let mut phases = String::from("{");
    for (i, &p) in order.iter().enumerate() {
        if i > 0 {
            phases.push(',');
        }
        let status = status_map.get(p).copied().unwrap_or("pending");
        let name = name_map.get(p).unwrap_or(&p);
        let visit_count = if status == "complete" || status == "in_progress" {
            1
        } else {
            0
        };
        phases.push_str(&format!(
            r#""{}":{{"name":"{}","status":"{}","started_at":null,"completed_at":null,"session_started_at":null,"cumulative_seconds":0,"visit_count":{}}}"#,
            p, name, status, visit_count
        ));
    }
    phases.push('}');

    format!(
        r#"{{"branch":"test-feature","current_phase":"{}","phases":{}}}"#,
        current_phase, phases
    )
}

fn setup_state(dir: &std::path::Path, branch: &str, state_json: &str) {
    let state_dir = flow_states_dir(dir);
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join(format!("{}.json", branch)), state_json).unwrap();
}

fn setup_git_repo(dir: &std::path::Path, branch: &str) {
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
    // Create and switch to feature branch
    Command::new("git")
        .args(["checkout", "-b", branch])
        .current_dir(dir)
        .output()
        .unwrap();
}

#[test]
fn phase_1_always_exits_0() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
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

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
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
fn previous_phase_pending_blocks() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");

    let state = make_state("flow-plan", &[("flow-start", "pending")]);
    setup_state(dir.path(), "test-feature", &state);

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
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
fn previous_phase_complete_allows() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");

    let state = make_state("flow-plan", &[("flow-start", "complete")]);
    setup_state(dir.path(), "test-feature", &state);

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
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

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
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

    // Create state files for OTHER branches — resolve_branch targets only
    // the current branch, so check-phase reports no feature on "main".
    for name in ["feat-a", "feat-b"] {
        let state = make_state("flow-plan", &[("flow-start", "complete")]);
        setup_state(dir.path(), name, &state);
    }

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
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

    // Copy flow-phases.json as frozen phases
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let source = std::path::PathBuf::from(manifest_dir).join("flow-phases.json");
    let dest = flow_states_dir(dir.path()).join("test-feature-phases.json");
    fs::copy(source, dest).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .env_remove("FLOW_SIMULATE_BRANCH")
        .args(["check-phase", "--required", "flow-plan"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
}

// --- Library-level tests (migrated from inline `#[cfg(test)]`) ---

fn make_state_lib(current_phase: &str, phase_statuses: &[(&str, &str)]) -> Value {
    let phase_names = phase_config::phase_names();
    let mut phases = serde_json::Map::new();
    for &p in PHASE_ORDER {
        let status = phase_statuses
            .iter()
            .find(|(k, _)| *k == p)
            .map(|(_, v)| *v)
            .unwrap_or("pending");
        let visit_count = if status == "complete" || status == "in_progress" {
            1
        } else {
            0
        };
        phases.insert(
            p.to_string(),
            json!({
                "name": phase_names.get(p).unwrap_or(&String::new()),
                "status": status,
                "started_at": null,
                "completed_at": null,
                "session_started_at": if status == "in_progress" { json!("2026-01-01T00:00:00Z") } else { json!(null) },
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

#[test]
fn previous_phase_pending_blocks_lib() {
    let state = make_state_lib("flow-plan", &[("flow-start", "pending")]);
    let (allowed, output) = check_phase(&state, "flow-plan", None).unwrap();
    assert!(!allowed);
    assert!(output.contains("BLOCKED"));
    assert!(output.contains("pending"));
}

#[test]
fn previous_phase_in_progress_blocks_lib() {
    let state = make_state_lib("flow-plan", &[("flow-start", "in_progress")]);
    let (allowed, output) = check_phase(&state, "flow-plan", None).unwrap();
    assert!(!allowed);
    assert!(output.contains("BLOCKED"));
    assert!(output.contains("in_progress"));
}

#[test]
fn previous_phase_complete_allows_lib() {
    let state = make_state_lib("flow-plan", &[("flow-start", "complete")]);
    let (allowed, _output) = check_phase(&state, "flow-plan", None).unwrap();
    assert!(allowed);
}

#[test]
fn sequential_chain_phase_4_with_1_to_3_complete() {
    let state = make_state_lib(
        "flow-code-review",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "complete"),
        ],
    );
    let (allowed, _) = check_phase(&state, "flow-code-review", None).unwrap();
    assert!(allowed);
}

#[test]
fn re_entering_completed_phase_shows_note() {
    let mut state = make_state_lib(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "complete")],
    );
    state["phases"]["flow-plan"]["visit_count"] = json!(2);
    let (allowed, output) = check_phase(&state, "flow-plan", None).unwrap();
    assert!(allowed);
    assert!(output.contains("previously completed"));
    assert!(output.contains("2 visit(s)"));
}

#[test]
fn re_entering_completed_phase_without_visit_count_reports_zero() {
    let mut state = make_state_lib(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "complete")],
    );
    state["phases"]["flow-plan"]
        .as_object_mut()
        .unwrap()
        .remove("visit_count");
    let (allowed, output) = check_phase(&state, "flow-plan", None).unwrap();
    assert!(allowed);
    assert!(output.contains("previously completed"));
    assert!(output.contains("0 visit(s)"));
}

#[test]
fn first_visit_no_previously_completed_message() {
    let state = make_state_lib("flow-plan", &[("flow-start", "complete")]);
    let (allowed, output) = check_phase(&state, "flow-plan", None).unwrap();
    assert!(allowed);
    assert!(!output.contains("previously completed"));
}

#[test]
fn phase_5_requires_phase_4_complete() {
    let state = make_state_lib(
        "flow-learn",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "complete"),
            ("flow-code-review", "pending"),
        ],
    );
    let (allowed, output) = check_phase(&state, "flow-learn", None).unwrap();
    assert!(!allowed);
    assert!(output.contains("Phase 4"));
}

#[test]
fn phase_6_requires_phase_5_complete() {
    let state = make_state_lib(
        "flow-complete",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "complete"),
            ("flow-code-review", "complete"),
            ("flow-learn", "pending"),
        ],
    );
    let (allowed, output) = check_phase(&state, "flow-complete", None).unwrap();
    assert!(!allowed);
    assert!(output.contains("Phase 5"));
}

#[test]
fn missing_phases_key_blocks() {
    let state = json!({"branch": "test", "current_phase": "flow-plan"});
    let (allowed, output) = check_phase(&state, "flow-plan", None).unwrap();
    assert!(!allowed);
    assert!(output.contains("BLOCKED"));
}

#[test]
fn blocked_message_includes_correct_command() {
    let state = make_state_lib(
        "flow-code-review",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "pending"),
        ],
    );
    let (allowed, output) = check_phase(&state, "flow-code-review", None).unwrap();
    assert!(!allowed);
    assert!(output.contains("/flow:flow-code"));
}

#[test]
fn invalid_phase_name_errors() {
    let state = make_state_lib("flow-start", &[("flow-start", "complete")]);
    let result = check_phase(&state, "nonexistent", None);
    assert!(result.is_err());
}

#[test]
fn check_phase_uses_frozen_config_lib() {
    let config = PhaseConfig {
        order: vec![
            "flow-start".into(),
            "flow-plan".into(),
            "flow-code-review".into(),
        ],
        names: {
            let mut m = IndexMap::new();
            m.insert("flow-start".into(), "Start".into());
            m.insert("flow-plan".into(), "Plan".into());
            m.insert("flow-code-review".into(), "Review".into());
            m
        },
        numbers: {
            let mut m = IndexMap::new();
            m.insert("flow-start".into(), 1);
            m.insert("flow-plan".into(), 2);
            m.insert("flow-code-review".into(), 3);
            m
        },
        commands: {
            let mut m = IndexMap::new();
            m.insert("flow-start".into(), "/t:a".into());
            m.insert("flow-plan".into(), "/t:b".into());
            m.insert("flow-code-review".into(), "/t:c".into());
            m
        },
    };
    let state = make_state_lib(
        "flow-code-review",
        &[("flow-start", "complete"), ("flow-plan", "complete")],
    );
    let (allowed, _) = check_phase(&state, "flow-code-review", Some(&config)).unwrap();
    assert!(allowed);
}

#[test]
fn check_phase_frozen_config_uses_correct_predecessor() {
    let config = PhaseConfig {
        order: vec!["flow-start".into(), "flow-code".into(), "flow-plan".into()],
        names: {
            let mut m = IndexMap::new();
            m.insert("flow-start".into(), "Start".into());
            m.insert("flow-code".into(), "Code".into());
            m.insert("flow-plan".into(), "Plan".into());
            m
        },
        numbers: {
            let mut m = IndexMap::new();
            m.insert("flow-start".into(), 1);
            m.insert("flow-code".into(), 2);
            m.insert("flow-plan".into(), 3);
            m
        },
        commands: {
            let mut m = IndexMap::new();
            m.insert("flow-start".into(), "/t:a".into());
            m.insert("flow-code".into(), "/t:b".into());
            m.insert("flow-plan".into(), "/t:c".into());
            m
        },
    };
    let state = make_state_lib(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-code", "pending")],
    );
    let (allowed, output) = check_phase(&state, "flow-plan", Some(&config)).unwrap();
    assert!(!allowed);
    assert!(output.contains("BLOCKED"));
}

#[test]
fn first_phase_has_no_prerequisites() {
    let state = make_state_lib("flow-start", &[]);
    let (allowed, output) = check_phase(&state, "flow-start", None).unwrap();
    assert!(allowed);
    assert!(output.is_empty());
}

#[test]
fn missing_prev_name_falls_back_to_key() {
    let config = PhaseConfig {
        order: vec!["flow-start".into(), "flow-plan".into()],
        names: IndexMap::new(),
        numbers: IndexMap::new(),
        commands: IndexMap::new(),
    };
    let state = make_state_lib("flow-plan", &[("flow-start", "pending")]);
    let (allowed, output) = check_phase(&state, "flow-plan", Some(&config)).unwrap();
    assert!(!allowed);
    assert!(output.contains("flow-start"));
    assert!(output.contains("/flow:flow-start"));
}

#[test]
fn missing_phase_name_falls_back_to_key() {
    let mut names = IndexMap::new();
    names.insert("flow-start".into(), "Start".into());
    let config = PhaseConfig {
        order: vec!["flow-start".into(), "flow-plan".into()],
        names,
        numbers: IndexMap::new(),
        commands: IndexMap::new(),
    };
    let state = make_state_lib("flow-plan", &[("flow-start", "pending")]);
    let (allowed, output) = check_phase(&state, "flow-plan", Some(&config)).unwrap();
    assert!(!allowed);
    assert!(output.contains("flow-plan"));
}

fn write_state_lib(root: &Path, branch: &str, state: Value) {
    let dir = root.join(".flow-states");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{}.json", branch));
    std::fs::write(&path, state.to_string()).unwrap();
}

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
    let states = root.join(".flow-states");
    std::fs::create_dir_all(&states).unwrap();
    let branch = "test-frozen-load";
    let state = make_state_lib(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    std::fs::write(
        states.join(format!("{}.json", branch)),
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
    std::fs::write(
        states.join(format!("{}-phases.json", branch)),
        serde_json::to_string(&frozen).unwrap(),
    )
    .unwrap();

    let (output, code) = run_impl_main("flow-plan", Some(branch), &root);
    assert_eq!(code, 0);
    assert!(output.is_empty());
}

#[test]
fn run_impl_main_with_resolver_none_returns_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let resolver = |_: Option<&str>, _: &Path| -> Option<String> { None };
    let (output, code) = run_impl_main_with_resolver("flow-plan", None, dir.path(), &resolver);
    assert_eq!(code, 1);
    assert!(output.contains("BLOCKED: Could not determine current git branch"));
}

#[test]
fn run_impl_main_state_file_is_directory_returns_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let states = root.join(".flow-states");
    std::fs::create_dir_all(&states).unwrap();
    std::fs::create_dir(states.join("test-feature.json")).unwrap();

    let (output, code) = run_impl_main("flow-plan", Some("test-feature"), &root);
    assert_eq!(code, 1);
    assert!(output.contains("BLOCKED: Could not read state file"));
}

#[test]
fn run_impl_main_unparseable_state_file_returns_blocked() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".flow-states")).unwrap();
    std::fs::write(
        dir.path().join(".flow-states").join("test.json"),
        "not-valid-json",
    )
    .unwrap();
    let (out, code) = run_impl_main("flow-plan", Some("test"), dir.path());
    assert_eq!(code, 1);
    assert!(out.contains("BLOCKED"));
    assert!(out.contains("Could not read state file"));
}

#[test]
fn run_impl_main_allowed_returns_zero() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_state_lib("flow-plan", &[("flow-start", "complete")]);
    write_state_lib(dir.path(), "test", state);
    let (out, code) = run_impl_main("flow-plan", Some("test"), dir.path());
    assert_eq!(code, 0);
    assert!(out.is_empty());
}

#[test]
fn run_impl_main_blocked_returns_one_with_blocked_message() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_state_lib("flow-plan", &[("flow-start", "pending")]);
    write_state_lib(dir.path(), "test", state);
    let (out, code) = run_impl_main("flow-plan", Some("test"), dir.path());
    assert_eq!(code, 1);
    assert!(out.contains("BLOCKED"));
}

#[test]
fn run_impl_main_reentry_returns_note_and_zero() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_state_lib(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "complete")],
    );
    state["phases"]["flow-plan"]["visit_count"] = json!(2);
    write_state_lib(dir.path(), "test", state);
    let (out, code) = run_impl_main("flow-plan", Some("test"), dir.path());
    assert_eq!(code, 0);
    assert!(out.contains("previously completed"));
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
    let state = make_state_lib("flow-start", &[("flow-start", "complete")]);
    write_state_lib(dir.path(), "test", state);
    let (out, code) = run_impl_main("nonexistent", Some("test"), dir.path());
    assert_eq!(code, 1);
    let parsed: Value = serde_json::from_str(&out).expect("invalid-phase path emits JSON");
    assert_eq!(parsed["status"], "error");
    assert!(parsed["message"]
        .as_str()
        .unwrap()
        .contains("Invalid phase"));
}
