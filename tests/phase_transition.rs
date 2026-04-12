mod common;

use std::fs;
use std::process::Command;

use common::flow_states_dir;

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

    let state_dir = flow_states_dir(dir.path());
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("test-feature.json"), "{bad json").unwrap();

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
    let dest = flow_states_dir(dir.path()).join("test-feature-phases.json");
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
    let state_path = flow_states_dir(dir.path()).join("test-feature.json");
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
    let state_path = flow_states_dir(dir.path()).join("my-feature.json");
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
    let state_path = flow_states_dir(dir.path()).join("my-feature.json");
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
