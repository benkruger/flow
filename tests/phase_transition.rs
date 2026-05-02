mod common;

use std::fs;
use std::process::Command;

use common::flow_states_dir;
use flow_rs::phase_config;
use flow_rs::phase_config::PHASE_ORDER;
use flow_rs::phase_transition::{phase_complete, phase_enter, run_impl_main};
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
        let session = if status == "in_progress" {
            "\"2026-01-01T00:00:00Z\""
        } else {
            "null"
        };
        phases.push_str(&format!(
            r#""{}":{{"name":"{}","status":"{}","started_at":null,"completed_at":null,"session_started_at":{},"cumulative_seconds":0,"visit_count":{}}}"#,
            p, name, status, session, visit_count
        ));
    }
    phases.push('}');

    format!(
        r#"{{"branch":"test-feature","current_phase":"{}","phases":{},"phase_transitions":[]}}"#,
        current_phase, phases
    )
}

fn setup_state(dir: &std::path::Path, branch: &str, state_json: &str) {
    let branch_dir = flow_states_dir(dir).join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), state_json).unwrap();
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
    Command::new("git")
        .args(["checkout", "-b", branch])
        .current_dir(dir)
        .output()
        .unwrap();
}

fn run(
    dir: &std::path::Path,
    phase: &str,
    action: &str,
    extra_args: &[&str],
) -> (i32, serde_json::Value) {
    let mut args = vec!["phase-transition", "--phase", phase, "--action", action];
    args.extend_from_slice(extra_args);
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(&args)
        .current_dir(dir)
        .env_remove("FLOW_SIMULATE_BRANCH")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let code = output.status.code().unwrap_or(-1);
    let json: serde_json::Value =
        serde_json::from_str(stdout.trim()).unwrap_or(serde_json::json!({"raw": stdout.trim()}));
    (code, json)
}

#[test]
fn enter_and_complete_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    let state = make_state("flow-start", &[("flow-start", "complete")]);
    setup_state(dir.path(), "test-feature", &state);

    let (code, json) = run(dir.path(), "flow-plan", "enter", &[]);
    assert_eq!(code, 0);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["phase"], "flow-plan");
    assert_eq!(json["action"], "enter");

    let (code, json) = run(dir.path(), "flow-plan", "complete", &[]);
    assert_eq!(code, 0);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["action"], "complete");
}

#[test]
fn error_missing_state_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");

    let (code, json) = run(dir.path(), "flow-plan", "enter", &[]);
    assert_eq!(code, 1);
    assert_eq!(json["status"], "error");
    assert!(json["message"].as_str().unwrap().contains("No state file"));
}

#[test]
fn error_invalid_phase() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    let state = make_state("flow-start", &[]);
    setup_state(dir.path(), "test-feature", &state);

    let (code, json) = run(dir.path(), "invalid", "enter", &[]);
    assert_eq!(code, 1);
    assert_eq!(json["status"], "error");
    assert!(json["message"].as_str().unwrap().contains("Invalid phase"));
}

#[test]
fn error_phase_not_in_state() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");

    // State with empty phases
    let state = r#"{"branch":"test-feature","current_phase":"flow-start","phases":{}}"#;
    setup_state(dir.path(), "test-feature", state);

    let (code, json) = run(dir.path(), "flow-plan", "enter", &[]);
    assert_eq!(code, 1);
    assert_eq!(json["status"], "error");
    assert!(json["message"].as_str().unwrap().contains("not found"));
}

#[test]
fn error_corrupt_json() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");

    let branch_dir = flow_states_dir(dir.path()).join("test-feature");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), "{bad json").unwrap();

    let (code, json) = run(dir.path(), "flow-plan", "enter", &[]);
    assert_eq!(code, 1);
    assert_eq!(json["status"], "error");
    assert!(json["message"].as_str().unwrap().contains("Could not read"));
}

#[test]
fn branch_flag_works() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "main");
    let state = make_state("flow-start", &[("flow-start", "complete")]);
    setup_state(dir.path(), "other-feature", &state);

    let (code, json) = run(
        dir.path(),
        "flow-plan",
        "enter",
        &["--branch", "other-feature"],
    );
    assert_eq!(code, 0);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["phase"], "flow-plan");
}

#[test]
fn frozen_phases_file_is_used() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    let state = make_state("flow-start", &[("flow-start", "complete")]);
    setup_state(dir.path(), "test-feature", &state);

    // Copy flow-phases.json as frozen
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let source = std::path::PathBuf::from(manifest_dir).join("flow-phases.json");
    let dest = flow_states_dir(dir.path())
        .join("test-feature")
        .join("phases.json");
    fs::copy(source, dest).unwrap();

    // Enter
    let (code, _) = run(dir.path(), "flow-plan", "enter", &[]);
    assert_eq!(code, 0);

    // Complete — should use frozen config for next phase
    let (code, json) = run(dir.path(), "flow-plan", "complete", &[]);
    assert_eq!(code, 0);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["next_phase"], "flow-code");
}

#[test]
fn falls_back_without_frozen_phases() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    let state = make_state("flow-start", &[("flow-start", "complete")]);
    setup_state(dir.path(), "test-feature", &state);

    // No frozen phases file
    let (code, _) = run(dir.path(), "flow-plan", "enter", &[]);
    assert_eq!(code, 0);

    let (code, json) = run(dir.path(), "flow-plan", "complete", &[]);
    assert_eq!(code, 0);
    assert_eq!(json["next_phase"], "flow-code");
}

#[test]
fn non_code_phase_no_diff_stats() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    let state = make_state(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    setup_state(dir.path(), "test-feature", &state);

    let (code, _) = run(dir.path(), "flow-plan", "complete", &[]);
    assert_eq!(code, 0);

    // Read state file to verify no diff_stats
    let state_path = flow_states_dir(dir.path()).join("test-feature").join("state.json");
    let content = fs::read_to_string(state_path).unwrap();
    let state: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(
        state.get("diff_stats").is_none(),
        "Plan completion should not capture diff_stats"
    );
}

#[test]
fn code_phase_completion_captures_diff_stats() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");

    // Add a file on main first
    fs::write(dir.path().join("old.py"), "old\n").unwrap();
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "add old"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Switch back to main, create feature branch
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["checkout", "-b", "my-feature"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Make changes
    fs::write(dir.path().join("new.py"), "new\n").unwrap();
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "add new"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    setup_state(dir.path(), "my-feature", &state);

    let (code, json) = run(
        dir.path(),
        "flow-code",
        "complete",
        &["--branch", "my-feature"],
    );
    assert_eq!(code, 0);
    assert_eq!(json["status"], "ok");

    // Read state file to verify diff_stats
    let state_path = flow_states_dir(dir.path()).join("my-feature").join("state.json");
    let content = fs::read_to_string(state_path).unwrap();
    let updated: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(
        updated.get("diff_stats").is_some(),
        "Code completion should capture diff_stats"
    );
    assert!(updated["diff_stats"]["files_changed"].as_i64().unwrap() >= 1);
    assert!(updated["diff_stats"]["captured_at"].is_string());
}

#[test]
fn diff_stats_with_merge_commit_in_history() {
    // Feature branch has a merge commit (merged a side branch into it).
    // Verifies capture_diff_stats parses correctly when HEAD history
    // includes non-linear commits.
    let dir = tempfile::tempdir().unwrap();

    // Init repo on main
    Command::new("git")
        .args(["-c", "init.defaultBranch=main", "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "commit.gpgsign", "false"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    fs::write(dir.path().join("base.txt"), "base\n").unwrap();
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Create side branch with a change
    Command::new("git")
        .args(["checkout", "-b", "side-branch"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    fs::write(dir.path().join("side.txt"), "side content\n").unwrap();
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "add side"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Back to main, create feature branch
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["checkout", "-b", "my-feature"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    fs::write(dir.path().join("feature.txt"), "feature content\n").unwrap();
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "add feature"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Merge side-branch into feature branch (creates merge commit)
    Command::new("git")
        .args(["merge", "side-branch", "--no-edit"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Set up state for code phase completion
    let state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    setup_state(dir.path(), "my-feature", &state);

    let (code, json) = run(
        dir.path(),
        "flow-code",
        "complete",
        &["--branch", "my-feature"],
    );
    assert_eq!(code, 0);
    assert_eq!(json["status"], "ok");

    // Verify diff_stats parsed correctly with merge in history
    let state_path = flow_states_dir(dir.path()).join("my-feature").join("state.json");
    let content = fs::read_to_string(state_path).unwrap();
    let updated: serde_json::Value = serde_json::from_str(&content).unwrap();
    let stats = &updated["diff_stats"];
    assert!(stats.get("files_changed").is_some());
    let files = stats["files_changed"].as_i64().unwrap();
    let ins = stats["insertions"].as_i64().unwrap();
    let del = stats["deletions"].as_i64().unwrap();
    assert!(files >= 0, "files_changed should be non-negative");
    assert!(ins >= 0, "insertions should be non-negative");
    assert!(del >= 0, "deletions should be non-negative");
    // Feature branch adds 2 files (feature.txt + side.txt from merge)
    assert!(
        files >= 2,
        "Expected at least 2 files changed (feature + side), got {}",
        files
    );
}

/// Subprocess: `phase-transition --action <invalid>` hits the
/// "Invalid action" branch of `run_impl_main`. Complements
/// `error_invalid_phase` which covers the invalid-phase branch.
#[test]
fn error_invalid_action() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    let state = make_state("flow-start", &[("flow-start", "in_progress")]);
    setup_state(dir.path(), "test-feature", &state);

    let (code, json) = run(dir.path(), "flow-start", "frobnicate", &[]);
    assert_eq!(code, 1);
    assert_eq!(json["status"], "error");
    assert!(json["message"].as_str().unwrap().contains("Invalid action"));
}

/// Subprocess: `phase-transition --branch <slash-branch>` exercises
/// the `FlowPaths::try_new` None branch. Per
/// `.claude/rules/external-input-validation.md`, CLI `--branch`
/// overrides must surface structured errors rather than panic.
#[test]
fn error_slash_branch_returns_structured_error_no_panic() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    let state = make_state("flow-start", &[("flow-start", "in_progress")]);
    setup_state(dir.path(), "test-feature", &state);

    let (code, json) = run(
        dir.path(),
        "flow-start",
        "complete",
        &["--branch", "feature/with-slash"],
    );
    // Slash branches are rejected by `FlowPaths::try_new` in
    // `run_impl_main` with an "Invalid branch name" error. Exit 1, no
    // panic. Guards the regression where the branch validation is
    // replaced with `FlowPaths::new` (panicking constructor).
    assert_eq!(code, 1);
    assert_eq!(json["status"], "error");
    let msg = json["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("Invalid branch name"),
        "expected 'Invalid branch name' error, got: {}",
        msg
    );
}

// ===================================================================
// Unit tests — migrated from inline `#[cfg(test)]` in
// `src/phase_transition.rs` per `.claude/rules/test-placement.md`.
// Drive `phase_enter`, `phase_complete`, `capture_diff_stats`,
// and `run_impl_main` through the public surface.
// ===================================================================

/// Build a minimal in-memory state Value (counterpart to the integration
/// `make_state` that returns a JSON string).
fn make_state_value(current_phase: &str, phase_statuses: &[(&str, &str)]) -> Value {
    let phase_names = phase_config::phase_names();
    let mut phases = serde_json::Map::new();
    for &p in PHASE_ORDER {
        let status = phase_statuses
            .iter()
            .find(|(k, _)| *k == p)
            .map(|(_, v)| *v)
            .unwrap_or("pending");
        let visit_count: i64 = if status == "complete" || status == "in_progress" {
            1
        } else {
            0
        };
        let session = if status == "in_progress" {
            json!("2026-01-01T00:00:00Z")
        } else {
            json!(null)
        };
        phases.insert(
            p.to_string(),
            json!({
                "name": phase_names.get(p).unwrap_or(&String::new()),
                "status": status,
                "started_at": null,
                "completed_at": null,
                "session_started_at": session,
                "cumulative_seconds": 0,
                "visit_count": visit_count,
            }),
        );
    }
    json!({
        "branch": "test-feature",
        "current_phase": current_phase,
        "phases": phases,
        "phase_transitions": [],
    })
}

// ===== phase_enter tests =====

#[test]
fn enter_sets_all_fields() {
    let mut state = make_state_value("flow-start", &[("flow-start", "complete")]);
    let result = phase_enter(&mut state, "flow-plan", None);

    assert_eq!(result["status"], "ok");
    assert_eq!(result["phase"], "flow-plan");
    assert_eq!(result["action"], "enter");
    assert_eq!(result["visit_count"], 1);
    assert_eq!(result["first_visit"], true);

    assert_eq!(state["phases"]["flow-plan"]["status"], "in_progress");
    assert!(state["phases"]["flow-plan"]["started_at"].is_string());
    assert!(state["phases"]["flow-plan"]["session_started_at"].is_string());
    assert_eq!(state["phases"]["flow-plan"]["visit_count"], 1);
    assert_eq!(state["current_phase"], "flow-plan");
}

#[test]
fn enter_first_visit_sets_started_at() {
    let mut state = make_state_value("flow-start", &[("flow-start", "complete")]);
    assert!(state["phases"]["flow-plan"]["started_at"].is_null());

    phase_enter(&mut state, "flow-plan", None);

    assert!(state["phases"]["flow-plan"]["started_at"].is_string());
}

#[test]
fn enter_reentry_preserves_started_at() {
    let mut state = make_state_value(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "complete")],
    );
    state["phases"]["flow-plan"]["started_at"] = json!("2026-01-15T10:00:00Z");
    state["phases"]["flow-plan"]["visit_count"] = json!(2);

    let result = phase_enter(&mut state, "flow-plan", None);

    assert_eq!(result["visit_count"], 3);
    assert_eq!(result["first_visit"], false);
    assert_eq!(
        state["phases"]["flow-plan"]["started_at"],
        "2026-01-15T10:00:00Z"
    );
    assert_eq!(state["phases"]["flow-plan"]["visit_count"], 3);
}

#[test]
fn enter_flow_complete() {
    let mut state = make_state_value(
        "flow-learn",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "complete"),
            ("flow-code-review", "complete"),
            ("flow-learn", "complete"),
        ],
    );
    let result = phase_enter(&mut state, "flow-complete", None);

    assert_eq!(result["status"], "ok");
    assert_eq!(result["phase"], "flow-complete");
    assert_eq!(result["visit_count"], 1);
    assert_eq!(result["first_visit"], true);
    assert_eq!(state["phases"]["flow-complete"]["status"], "in_progress");
    assert!(state["phases"]["flow-complete"]["started_at"].is_string());
    assert_eq!(state["current_phase"], "flow-complete");
}

#[test]
fn enter_non_code_review_does_not_set_code_review_step() {
    let mut state = make_state_value("flow-start", &[("flow-start", "complete")]);
    phase_enter(&mut state, "flow-plan", None);

    assert!(state.get("code_review_step").is_none() || state["code_review_step"].is_null());
}

#[test]
fn enter_clears_auto_continue() {
    let mut state = make_state_value("flow-start", &[("flow-start", "complete")]);
    state["_auto_continue"] = json!("/flow:flow-plan");

    phase_enter(&mut state, "flow-plan", None);

    assert!(state.get("_auto_continue").is_none() || state["_auto_continue"].is_null());
}

#[test]
fn enter_clears_stop_instructed() {
    let mut state = make_state_value("flow-start", &[("flow-start", "complete")]);
    state["_stop_instructed"] = json!(true);

    phase_enter(&mut state, "flow-plan", None);

    assert!(state.get("_stop_instructed").is_none());
}

#[test]
fn enter_clears_continue_pending() {
    let mut state = make_state_value("flow-start", &[("flow-start", "complete")]);
    state["_continue_pending"] = json!("commit");
    state["_continue_context"] = json!("stale instructions");

    phase_enter(&mut state, "flow-plan", None);

    assert!(state.get("_continue_pending").is_none());
    assert!(state.get("_continue_context").is_none());
}

#[test]
fn enter_no_error_when_auto_continue_absent() {
    let mut state = make_state_value("flow-start", &[("flow-start", "complete")]);
    let result = phase_enter(&mut state, "flow-plan", None);

    assert_eq!(result["status"], "ok");
    assert!(state.get("_auto_continue").is_none() || state["_auto_continue"].is_null());
}

#[test]
fn enter_records_phase_transition() {
    let mut state = make_state_value("flow-start", &[("flow-start", "complete")]);
    state["phase_transitions"] = json!([]);

    phase_enter(&mut state, "flow-plan", None);

    let transitions = state["phase_transitions"].as_array().unwrap();
    assert_eq!(transitions.len(), 1);
    assert_eq!(transitions[0]["from"], "flow-start");
    assert_eq!(transitions[0]["to"], "flow-plan");
    assert!(transitions[0]["timestamp"].is_string());
    assert!(transitions[0].get("reason").is_none() || transitions[0]["reason"].is_null());
}

#[test]
fn enter_appends_to_existing_transitions() {
    let mut state = make_state_value(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "complete")],
    );
    state["phase_transitions"] = json!([
        {"from": "flow-start", "to": "flow-plan", "timestamp": "2026-01-01T00:00:00-08:00"}
    ]);

    phase_enter(&mut state, "flow-code", None);

    let transitions = state["phase_transitions"].as_array().unwrap();
    assert_eq!(transitions.len(), 2);
    assert_eq!(transitions[1]["from"], "flow-plan");
    assert_eq!(transitions[1]["to"], "flow-code");
}

#[test]
fn enter_transition_has_no_reason_by_default() {
    let mut state = make_state_value("flow-start", &[("flow-start", "complete")]);
    state["phase_transitions"] = json!([]);

    phase_enter(&mut state, "flow-plan", None);

    let entry = &state["phase_transitions"][0];
    assert!(entry.get("reason").is_none() || entry["reason"].is_null());
}

#[test]
fn enter_transition_with_reason() {
    let mut state = make_state_value(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "complete"),
        ],
    );
    state["phase_transitions"] = json!([]);

    phase_enter(&mut state, "flow-plan", Some("approach was wrong"));

    assert_eq!(
        state["phase_transitions"][0]["reason"],
        "approach was wrong"
    );
}

#[test]
fn enter_creates_transitions_array_if_missing() {
    let mut state = make_state_value("flow-start", &[("flow-start", "complete")]);
    state.as_object_mut().unwrap().remove("phase_transitions");

    phase_enter(&mut state, "flow-plan", None);

    assert!(state["phase_transitions"].is_array());
    assert_eq!(state["phase_transitions"].as_array().unwrap().len(), 1);
}

// ===== phase_complete tests =====

#[test]
fn complete_sets_all_fields() {
    let mut state = make_state_value(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    let result = phase_complete(&mut state, "flow-plan", None, None, None);

    assert_eq!(result["status"], "ok");
    assert_eq!(result["phase"], "flow-plan");
    assert_eq!(result["action"], "complete");
    assert!(result.get("cumulative_seconds").is_some());
    assert!(result.get("formatted_time").is_some());
    assert_eq!(result["next_phase"], "flow-code");

    assert_eq!(state["phases"]["flow-plan"]["status"], "complete");
    assert!(state["phases"]["flow-plan"]["completed_at"].is_string());
    assert!(state["phases"]["flow-plan"]["session_started_at"].is_null());
    assert_eq!(state["current_phase"], "flow-code");
}

#[test]
fn complete_adds_to_existing_cumulative() {
    let mut state = make_state_value(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    state["phases"]["flow-plan"]["cumulative_seconds"] = json!(600);

    let result = phase_complete(&mut state, "flow-plan", None, None, None);

    assert!(result["cumulative_seconds"].as_i64().unwrap() >= 600);
}

#[test]
fn complete_formatted_time_less_than_one_minute() {
    let mut state = make_state_value(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    state["phases"]["flow-plan"]["cumulative_seconds"] = json!(0);
    state["phases"]["flow-plan"]["session_started_at"] = json!(null);

    let result = phase_complete(&mut state, "flow-plan", None, None, None);

    assert_eq!(result["formatted_time"], "<1m");
}

#[test]
fn complete_next_phase_override() {
    let mut state = make_state_value(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );

    let result = phase_complete(
        &mut state,
        "flow-plan",
        Some("flow-code-review"),
        None,
        None,
    );

    assert_eq!(result["next_phase"], "flow-code-review");
    assert_eq!(state["current_phase"], "flow-code-review");
}

#[test]
fn complete_null_session_started_at() {
    let mut state = make_state_value(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    state["phases"]["flow-plan"]["session_started_at"] = json!(null);
    state["phases"]["flow-plan"]["cumulative_seconds"] = json!(100);

    let result = phase_complete(&mut state, "flow-plan", None, None, None);

    assert_eq!(result["cumulative_seconds"], 100);
}

#[test]
fn complete_formatted_time_minutes() {
    let mut state = make_state_value(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    state["phases"]["flow-plan"]["cumulative_seconds"] = json!(300);
    state["phases"]["flow-plan"]["session_started_at"] = json!(null);

    let result = phase_complete(&mut state, "flow-plan", None, None, None);

    assert_eq!(result["formatted_time"], "5m");
}

#[test]
fn complete_formatted_time_hours() {
    let mut state = make_state_value(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    state["phases"]["flow-plan"]["cumulative_seconds"] = json!(3900);
    state["phases"]["flow-plan"]["session_started_at"] = json!(null);

    let result = phase_complete(&mut state, "flow-plan", None, None, None);

    assert_eq!(result["formatted_time"], "1h 5m");
}

#[test]
fn complete_uses_custom_phase_order() {
    let mut state = make_state_value(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    let custom_order: Vec<String> = vec![
        "flow-start".into(),
        "flow-plan".into(),
        "flow-code-review".into(),
    ];

    let result = phase_complete(&mut state, "flow-plan", None, Some(&custom_order), None);

    assert_eq!(result["next_phase"], "flow-code-review");
    assert_eq!(state["current_phase"], "flow-code-review");
}

#[test]
fn complete_terminal_phase_auto_next() {
    let mut state = make_state_value(
        "flow-complete",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "complete"),
            ("flow-code-review", "complete"),
            ("flow-learn", "complete"),
            ("flow-complete", "in_progress"),
        ],
    );

    let result = phase_complete(&mut state, "flow-complete", None, None, None);

    assert_eq!(result["status"], "ok");
    assert_eq!(result["next_phase"], "flow-complete");
    assert_eq!(state["current_phase"], "flow-complete");
}

#[test]
fn complete_flow_complete_with_next_phase() {
    let mut state = make_state_value(
        "flow-complete",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "complete"),
            ("flow-code-review", "complete"),
            ("flow-learn", "complete"),
            ("flow-complete", "in_progress"),
        ],
    );

    let result = phase_complete(
        &mut state,
        "flow-complete",
        Some("flow-complete"),
        None,
        None,
    );

    assert_eq!(result["status"], "ok");
    assert_eq!(result["next_phase"], "flow-complete");
    assert_eq!(state["phases"]["flow-complete"]["status"], "complete");
    assert!(state["phases"]["flow-complete"]["completed_at"].is_string());
}

// ===== Auto-continue tests =====

#[test]
fn complete_sets_auto_continue_when_skills_continue_auto() {
    let mut state = make_state_value("flow-start", &[("flow-start", "in_progress")]);
    state["skills"] = json!({"flow-start": {"continue": "auto"}});

    let result = phase_complete(&mut state, "flow-start", None, None, None);

    assert_eq!(state["_auto_continue"], "/flow:flow-plan");
    assert_eq!(result["next_phase"], "flow-plan");
}

#[test]
fn complete_sets_auto_continue_with_flat_string_config() {
    let mut state = make_state_value("flow-start", &[("flow-start", "in_progress")]);
    state["skills"] = json!({"flow-start": "auto"});

    phase_complete(&mut state, "flow-start", None, None, None);

    assert_eq!(state["_auto_continue"], "/flow:flow-plan");
}

#[test]
fn complete_no_auto_continue_when_manual() {
    let mut state = make_state_value("flow-start", &[("flow-start", "in_progress")]);
    state["skills"] = json!({"flow-start": {"continue": "manual"}});

    phase_complete(&mut state, "flow-start", None, None, None);

    assert!(state.get("_auto_continue").is_none() || state["_auto_continue"].is_null());
}

#[test]
fn complete_no_auto_continue_when_no_skills() {
    let mut state = make_state_value("flow-start", &[("flow-start", "in_progress")]);

    phase_complete(&mut state, "flow-start", None, None, None);

    assert!(state.get("_auto_continue").is_none() || state["_auto_continue"].is_null());
}

#[test]
fn complete_clears_auto_continue_when_switching_to_manual() {
    let mut state = make_state_value(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    state["skills"] = json!({"flow-plan": {"continue": "manual"}});
    state["_auto_continue"] = json!("/flow:flow-plan");

    phase_complete(&mut state, "flow-plan", None, None, None);

    assert!(state.get("_auto_continue").is_none() || state["_auto_continue"].is_null());
}

#[test]
fn complete_no_auto_continue_when_skill_config_unexpected_type() {
    let mut state = make_state_value("flow-start", &[("flow-start", "in_progress")]);
    state["skills"] = json!({"flow-start": 42});

    phase_complete(&mut state, "flow-start", None, None, None);

    assert!(state.get("_auto_continue").is_none() || state["_auto_continue"].is_null());
}

#[test]
fn complete_result_continue_action_invoke_when_auto() {
    let mut state = make_state_value("flow-start", &[("flow-start", "in_progress")]);
    state["skills"] = json!({"flow-start": {"continue": "auto"}});

    let result = phase_complete(&mut state, "flow-start", None, None, None);

    assert_eq!(result["continue_action"], "invoke");
    assert_eq!(result["continue_target"], "/flow:flow-plan");
}

#[test]
fn complete_result_continue_action_ask_when_manual() {
    let mut state = make_state_value("flow-start", &[("flow-start", "in_progress")]);
    state["skills"] = json!({"flow-start": {"continue": "manual"}});

    let result = phase_complete(&mut state, "flow-start", None, None, None);

    assert_eq!(result["continue_action"], "ask");
    assert!(result.get("continue_target").is_none());
}

#[test]
fn complete_result_continue_action_ask_when_absent() {
    let mut state = make_state_value("flow-start", &[("flow-start", "in_progress")]);

    let result = phase_complete(&mut state, "flow-start", None, None, None);

    assert_eq!(result["continue_action"], "ask");
    assert!(result.get("continue_target").is_none());
}

#[test]
fn complete_result_continue_action_invoke_with_flat_string() {
    let mut state = make_state_value("flow-start", &[("flow-start", "in_progress")]);
    state["skills"] = json!({"flow-start": "auto"});

    let result = phase_complete(&mut state, "flow-start", None, None, None);

    assert_eq!(result["continue_action"], "invoke");
    assert_eq!(result["continue_target"], "/flow:flow-plan");
}

#[test]
fn complete_result_continue_action_ask_with_unexpected_type() {
    let mut state = make_state_value("flow-start", &[("flow-start", "in_progress")]);
    state["skills"] = json!({"flow-start": 42});

    let result = phase_complete(&mut state, "flow-start", None, None, None);

    assert_eq!(result["continue_action"], "ask");
    assert!(result.get("continue_target").is_none());
}

#[test]
fn complete_result_continue_action_ask_when_auto_but_no_command() {
    let mut state = make_state_value("flow-start", &[("flow-start", "in_progress")]);
    state["skills"] = json!({"flow-start": {"continue": "auto"}});

    let mut cmds = IndexMap::new();
    cmds.insert("flow-start".to_string(), "/flow:flow-start".to_string());

    let result = phase_complete(&mut state, "flow-start", None, None, Some(&cmds));

    assert_eq!(result["continue_action"], "ask");
    assert!(result.get("continue_target").is_none());
    assert!(state.get("_auto_continue").is_none() || state["_auto_continue"].is_null());
}

#[test]
fn complete_future_session_started_clamps_to_zero() {
    let mut state = make_state_value(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    state["phases"]["flow-plan"]["session_started_at"] = json!("2099-12-31T23:59:59Z");
    state["phases"]["flow-plan"]["cumulative_seconds"] = json!(50);

    let result = phase_complete(&mut state, "flow-plan", None, None, None);

    assert_eq!(result["cumulative_seconds"], 50);
}

// ===== counter type tolerance tests =====

#[test]
fn enter_visit_count_string_tolerance() {
    let mut state = make_state_value(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "complete")],
    );
    state["phases"]["flow-plan"]["visit_count"] = json!("3");

    let result = phase_enter(&mut state, "flow-plan", None);

    assert_eq!(result["visit_count"], 4);
    assert_eq!(state["phases"]["flow-plan"]["visit_count"], 4);
}

#[test]
fn enter_visit_count_float_tolerance() {
    let mut state = make_state_value(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "complete")],
    );
    state["phases"]["flow-plan"]["visit_count"] = json!(3.0);

    let result = phase_enter(&mut state, "flow-plan", None);

    assert_eq!(result["visit_count"], 4);
    assert_eq!(state["phases"]["flow-plan"]["visit_count"], 4);
}

#[test]
fn complete_cumulative_seconds_string_tolerance() {
    let mut state = make_state_value(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    state["phases"]["flow-plan"]["cumulative_seconds"] = json!("120");
    state["phases"]["flow-plan"]["session_started_at"] = json!("2099-12-31T23:59:59Z");

    let result = phase_complete(&mut state, "flow-plan", None, None, None);

    assert_eq!(result["cumulative_seconds"], 120);
}

#[test]
fn complete_cumulative_seconds_float_tolerance() {
    let mut state = make_state_value(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    state["phases"]["flow-plan"]["cumulative_seconds"] = json!(120.0);
    state["phases"]["flow-plan"]["session_started_at"] = json!("2099-12-31T23:59:59Z");

    let result = phase_complete(&mut state, "flow-plan", None, None, None);

    assert_eq!(result["cumulative_seconds"], 120);
}

// ===== phase_enter schema robustness tests =====

#[test]
fn enter_phases_key_absent() {
    let mut state = json!({
        "branch": "test-feature",
        "current_phase": "flow-start",
    });
    let result = phase_enter(&mut state, "flow-plan", None);
    assert_eq!(result["status"], "ok");
    assert_eq!(state["phases"]["flow-plan"]["status"], "in_progress");
}

#[test]
fn enter_phases_key_null() {
    let mut state = json!({
        "branch": "test-feature",
        "current_phase": "flow-start",
        "phases": null,
    });
    let result = phase_enter(&mut state, "flow-plan", None);
    assert_eq!(result["status"], "ok");
    assert_eq!(state["phases"]["flow-plan"]["status"], "in_progress");
}

#[test]
fn enter_phases_wrong_type_string() {
    let mut state = json!({
        "branch": "test-feature",
        "current_phase": "flow-start",
        "phases": "corrupted",
    });
    let result = phase_enter(&mut state, "flow-plan", None);
    assert_eq!(result["status"], "ok");
}

#[test]
fn enter_phases_wrong_type_array() {
    let mut state = json!({
        "branch": "test-feature",
        "current_phase": "flow-start",
        "phases": [1, 2, 3],
    });
    let result = phase_enter(&mut state, "flow-plan", None);
    assert_eq!(result["status"], "ok");
}

// ===== capture_diff_stats =====
// `capture_diff_stats` is exercised via subprocess tests that spawn
// `bin/flow phase-transition --action complete` against fixture repos
// (see `code_phase_completion_captures_diff_stats`,
// `diff_stats_with_merge_commit_in_history`,
// `diff_stats_no_main_branch_returns_zeros`,
// `diff_stats_no_diff_returns_zeros`,
// `diff_stats_deletion_only`). `parse_diff_summary` is a private
// helper reached transitively through the same path.

#[test]
fn diff_stats_no_main_branch_returns_zeros() {
    // Repo with a feature branch but no `main` ref. `git diff --stat
    // main...HEAD` exits non-zero, hitting the `_` arm of
    // `capture_diff_stats`'s match (covers both Err and Ok-non-success
    // since they fall through the same `_` handler).
    let dir = tempfile::tempdir().unwrap();
    // Init directly on the feature branch — no main ever exists.
    Command::new("git")
        .args(["-c", "init.defaultBranch=my-feature", "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "commit.gpgsign", "false"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    setup_state(dir.path(), "my-feature", &state);

    let (code, json) = run(
        dir.path(),
        "flow-code",
        "complete",
        &["--branch", "my-feature"],
    );
    assert_eq!(code, 0);
    assert_eq!(json["status"], "ok");

    let state_path = flow_states_dir(dir.path()).join("my-feature").join("state.json");
    let content = fs::read_to_string(state_path).unwrap();
    let updated: serde_json::Value = serde_json::from_str(&content).unwrap();
    let stats = &updated["diff_stats"];
    assert_eq!(stats["files_changed"], 0);
    assert_eq!(stats["insertions"], 0);
    assert_eq!(stats["deletions"], 0);
    assert!(stats.get("captured_at").is_some());
}

#[test]
fn diff_stats_no_diff_returns_zeros() {
    // Feature branch identical to main (no commits on top). `git diff
    // --stat main...HEAD` prints empty stdout. Covers the
    // `stdout.trim().lines().last().unwrap_or("")` None path and the
    // three `extract(...)` None branches in `parse_diff_summary`
    // (summary is "" so no part contains any keyword).
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "my-feature");

    let state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    setup_state(dir.path(), "my-feature", &state);

    let (code, json) = run(
        dir.path(),
        "flow-code",
        "complete",
        &["--branch", "my-feature"],
    );
    assert_eq!(code, 0);
    assert_eq!(json["status"], "ok");

    let state_path = flow_states_dir(dir.path()).join("my-feature").join("state.json");
    let content = fs::read_to_string(state_path).unwrap();
    let updated: serde_json::Value = serde_json::from_str(&content).unwrap();
    let stats = &updated["diff_stats"];
    assert_eq!(stats["files_changed"], 0);
    assert_eq!(stats["insertions"], 0);
    assert_eq!(stats["deletions"], 0);
}

#[test]
fn diff_stats_deletion_only() {
    // Feature branch that DELETES a file present on main. `git diff
    // --stat main...HEAD` reports the deletion, exercising the
    // `extract("deletion")` Some path in `parse_diff_summary`.
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "temp");
    // Add a file on main
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    fs::write(dir.path().join("doomed.py"), "line1\nline2\nline3\n").unwrap();
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "add doomed"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    // Create feature branch from main, delete the file
    Command::new("git")
        .args(["checkout", "-b", "my-feature"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    fs::remove_file(dir.path().join("doomed.py")).unwrap();
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "delete doomed"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    setup_state(dir.path(), "my-feature", &state);

    let (code, json) = run(
        dir.path(),
        "flow-code",
        "complete",
        &["--branch", "my-feature"],
    );
    assert_eq!(code, 0);
    assert_eq!(json["status"], "ok");

    let state_path = flow_states_dir(dir.path()).join("my-feature").join("state.json");
    let content = fs::read_to_string(state_path).unwrap();
    let updated: serde_json::Value = serde_json::from_str(&content).unwrap();
    let stats = &updated["diff_stats"];
    assert!(stats["files_changed"].as_i64().unwrap() >= 1);
    assert!(stats["deletions"].as_i64().unwrap() >= 3);
}

#[test]
fn run_impl_main_complete_with_next_phase_and_reason_exercises_closures() {
    // Covers the `next_phase.map(|s| s.to_string())` and
    // `reason.map(|s| s.to_string())` closures by passing Some values.
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_state_value(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    state["phases"]["flow-plan"]["session_started_at"] = json!("2026-01-01T00:00:00Z");
    write_state(dir.path(), "test", state);
    let (out, code) = run_impl_main(
        "flow-plan",
        "complete",
        Some("flow-code-review"),
        Some("test"),
        Some("approach pivot"),
        dir.path(),
        dir.path(),
    );
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert_eq!(out["next_phase"], "flow-code-review");
}

/// In a non-git cwd with no `--branch` override, `resolve_branch`
/// returns None and phase-transition surfaces a structured error.
/// Exercised via subprocess so the test process's own git cwd does
/// not interfere with branch resolution.
#[test]
fn cli_no_branch_in_non_git_cwd_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().canonicalize().unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .env_remove("FLOW_CI_RUNNING")
        .env("FLOW_SIMULATE_BRANCH", "")
        .env("GIT_CEILING_DIRECTORIES", &dir)
        .args([
            "phase-transition",
            "--phase",
            "flow-plan",
            "--action",
            "enter",
        ])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .expect("stdout");
    let parsed: Value = serde_json::from_str(line.trim()).expect("json");
    assert_eq!(parsed["status"], "error");
    assert!(parsed["message"]
        .as_str()
        .unwrap()
        .contains("Could not determine current branch"));
}

// ===== run_impl_main =====

fn write_state(root: &std::path::Path, branch: &str, state: Value) {
    let branch_dir = root.join(".flow-states").join(branch);
    std::fs::create_dir_all(&branch_dir).unwrap();
    let path = branch_dir.join("state.json");
    std::fs::write(&path, state.to_string()).unwrap();
}

#[test]
fn run_impl_main_invalid_phase_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let (out, code) = run_impl_main(
        "nonexistent",
        "enter",
        None,
        Some("test"),
        None,
        dir.path(),
        dir.path(),
    );
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(out["message"].as_str().unwrap().contains("Invalid phase"));
}

#[test]
fn run_impl_main_invalid_action_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let (out, code) = run_impl_main(
        "flow-plan",
        "bogus",
        None,
        Some("test"),
        None,
        dir.path(),
        dir.path(),
    );
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(out["message"].as_str().unwrap().contains("Invalid action"));
}

fn init_git_repo_at(dir: &std::path::Path, branch: &str) {
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git command failed");
        assert!(output.status.success(), "git {:?} failed", args);
    };
    run(&["init", "--initial-branch", branch]);
    run(&["config", "user.email", "test@test.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "commit.gpgsign", "false"]);
    run(&["commit", "--allow-empty", "-m", "init"]);
}

#[test]
fn run_impl_main_cwd_drift_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo_at(dir.path(), "feature-x");
    let mut state = make_state_value("flow-plan", &[("flow-start", "complete")]);
    state["relative_cwd"] = json!("api");
    write_state(dir.path(), "feature-x", state);

    let (out, code) = run_impl_main(
        "flow-plan",
        "enter",
        None,
        None,
        None,
        dir.path(),
        dir.path(),
    );
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    let msg = out["message"].as_str().unwrap();
    assert!(
        msg.to_lowercase().contains("api") || msg.to_lowercase().contains("expected"),
        "expected cwd-drift error naming api or expected, got: {}",
        msg
    );
}

#[test]
fn run_impl_main_slash_branch_returns_error_no_panic() {
    let dir = tempfile::tempdir().unwrap();
    let (out, code) = run_impl_main(
        "flow-plan",
        "enter",
        None,
        Some("feature/foo"),
        None,
        dir.path(),
        dir.path(),
    );
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    let msg = out["message"].as_str().unwrap();
    assert!(
        msg.contains("Invalid branch") || msg.contains("feature/foo"),
        "expected invalid-branch error, got: {}",
        msg
    );
}

#[test]
fn run_impl_main_empty_branch_returns_error_no_panic() {
    let dir = tempfile::tempdir().unwrap();
    let (out, code) = run_impl_main(
        "flow-plan",
        "enter",
        None,
        Some(""),
        None,
        dir.path(),
        dir.path(),
    );
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(out["message"].as_str().unwrap().contains("Invalid branch"));
}

#[test]
fn run_impl_main_no_state_file_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let (out, code) = run_impl_main(
        "flow-plan",
        "enter",
        None,
        Some("test"),
        None,
        dir.path(),
        dir.path(),
    );
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(out["message"]
        .as_str()
        .unwrap()
        .contains("No state file found"));
}

#[test]
fn run_impl_main_unparseable_state_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".flow-states").join("test")).unwrap();
    std::fs::write(
        dir.path()
            .join(".flow-states")
            .join("test")
            .join("state.json"),
        "not-json",
    )
    .unwrap();
    let (out, code) = run_impl_main(
        "flow-plan",
        "enter",
        None,
        Some("test"),
        None,
        dir.path(),
        dir.path(),
    );
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(out["message"]
        .as_str()
        .unwrap()
        .contains("Could not read state file"));
}

#[test]
fn run_impl_main_missing_phase_key_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".flow-states").join("test")).unwrap();
    let bare = json!({"branch": "test", "current_phase": "flow-start", "phases": {}});
    std::fs::write(
        dir.path()
            .join(".flow-states")
            .join("test")
            .join("state.json"),
        bare.to_string(),
    )
    .unwrap();
    let (out, code) = run_impl_main(
        "flow-plan",
        "enter",
        None,
        Some("test"),
        None,
        dir.path(),
        dir.path(),
    );
    assert_eq!(code, 1);
    assert!(out["message"]
        .as_str()
        .unwrap()
        .contains("Phase flow-plan not found"));
}

#[test]
fn run_impl_main_enter_success_returns_zero() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_state_value("flow-start", &[("flow-start", "complete")]);
    write_state(dir.path(), "test", state);
    let (out, code) = run_impl_main(
        "flow-plan",
        "enter",
        None,
        Some("test"),
        None,
        dir.path(),
        dir.path(),
    );
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert_eq!(out["phase"], "flow-plan");
    assert_eq!(out["action"], "enter");
    let log_content = std::fs::read_to_string(
        dir.path().join(".flow-states").join("test").join("log"),
    )
    .expect("log file must exist after append_log");
    assert!(log_content.contains("phase-transition --action enter --phase flow-plan"));
    assert!(log_content.contains("\"ok\""));
}

#[test]
fn run_impl_main_complete_success_returns_zero() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = make_state_value(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    state["phases"]["flow-plan"]["started_at"] = json!("2026-01-01T00:00:00Z");
    state["phases"]["flow-plan"]["session_started_at"] = json!("2026-01-01T00:00:00Z");
    write_state(dir.path(), "test", state);
    let (out, code) = run_impl_main(
        "flow-plan",
        "complete",
        None,
        Some("test"),
        None,
        dir.path(),
        dir.path(),
    );
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert_eq!(out["action"], "complete");
}

#[test]
fn complete_flow_code_captures_diff_stats() {
    let mut state = make_state_value(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    let result = phase_complete(&mut state, "flow-code", None, None, None);

    assert_eq!(result["status"], "ok");
    assert_eq!(result["next_phase"], "flow-code-review");
    assert!(state.get("diff_stats").is_some());
    assert!(state["diff_stats"].get("files_changed").is_some());
    assert!(state["diff_stats"].get("captured_at").is_some());
}

#[test]
fn complete_phases_key_absent() {
    // Covers the `if let Some(phases)` None branch of phase_complete's
    // phases-type guard — state has no "phases" key at all.
    let mut state = json!({
        "branch": "test-feature",
        "current_phase": "flow-plan",
        "phase_transitions": [],
    });
    let result = phase_complete(&mut state, "flow-plan", None, None, None);
    assert_eq!(result["status"], "ok");
}

#[test]
fn run_impl_main_state_file_unreadable_returns_error() {
    // Covers the Err(_) branch of read_to_string at line 341 — state
    // path exists but is a directory so read fails.
    let dir = tempfile::tempdir().unwrap();
    let state_dir_path = dir
        .path()
        .join(".flow-states")
        .join("test")
        .join("state.json");
    std::fs::create_dir_all(&state_dir_path).unwrap();

    let (out, code) = run_impl_main(
        "flow-plan",
        "enter",
        None,
        Some("test"),
        None,
        dir.path(),
        dir.path(),
    );
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(out["message"]
        .as_str()
        .unwrap()
        .contains("Could not read state file"));
}

#[test]
fn complete_phases_wrong_type_string_resets() {
    let mut state = json!({
        "branch": "test-feature",
        "current_phase": "flow-plan",
        "phases": "corrupted",
        "phase_transitions": [],
    });
    let result = phase_complete(&mut state, "flow-plan", None, None, None);
    assert_eq!(result["status"], "ok");
}

#[test]
fn complete_phases_wrong_type_array_resets() {
    let mut state = json!({
        "branch": "test-feature",
        "current_phase": "flow-plan",
        "phases": [1, 2, 3],
        "phase_transitions": [],
    });
    let result = phase_complete(&mut state, "flow-plan", None, None, None);
    assert_eq!(result["status"], "ok");
}

#[test]
fn run_impl_main_mutate_state_failure_returns_error() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    init_git_repo_at(dir.path(), "test");
    let state = make_state_value("flow-start", &[("flow-start", "complete")]);
    write_state(dir.path(), "test", state);

    let state_file = dir
        .path()
        .join(".flow-states")
        .join("test")
        .join("state.json");
    std::fs::set_permissions(&state_file, std::fs::Permissions::from_mode(0o444)).unwrap();

    let (out, code) = run_impl_main(
        "flow-plan",
        "enter",
        None,
        Some("test"),
        None,
        dir.path(),
        dir.path(),
    );
    let _ = std::fs::set_permissions(&state_file, std::fs::Permissions::from_mode(0o644));
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(out["message"]
        .as_str()
        .unwrap()
        .contains("State mutation failed"));
    let log_path = dir.path().join(".flow-states").join("test.log");
    if log_path.exists() {
        let log = std::fs::read_to_string(&log_path).unwrap();
        assert!(log.contains("\"error\""));
    }
}
