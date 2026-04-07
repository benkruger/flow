use std::fs;
use std::process::Command;

use serde_json::{json, Value};

fn flow_rs() -> Command {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
}

fn setup_git_repo(dir: &std::path::Path) {
    Command::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir)
        .output()
        .unwrap();
}

fn run_session_context(dir: &std::path::Path) -> std::process::Output {
    flow_rs()
        .arg("session-context")
        .current_dir(dir)
        .env_remove("FLOW_SIMULATE_BRANCH")
        .output()
        .unwrap()
}

fn make_state() -> Value {
    json!({
        "schema_version": 1,
        "branch": "test-feature",
        "repo": "test/repo",
        "pr_number": 1,
        "pr_url": "https://github.com/test/repo/pull/1",
        "started_at": "2026-01-15T10:00:00-08:00",
        "current_phase": "flow-code",
        "framework": "python",
        "files": {
            "plan": null,
            "dag": null,
            "log": ".flow-states/test-feature.log",
            "state": ".flow-states/test-feature.json"
        },
        "session_tty": null,
        "session_id": null,
        "transcript_path": null,
        "notes": [],
        "prompt": "test feature",
        "phases": {
            "flow-start": {
                "name": "Start",
                "status": "complete",
                "started_at": "2026-01-15T10:00:00-08:00",
                "completed_at": "2026-01-15T10:05:00-08:00",
                "session_started_at": null,
                "cumulative_seconds": 300,
                "visit_count": 1
            },
            "flow-plan": {
                "name": "Plan",
                "status": "complete",
                "started_at": "2026-01-15T10:05:00-08:00",
                "completed_at": "2026-01-15T10:10:00-08:00",
                "session_started_at": null,
                "cumulative_seconds": 300,
                "visit_count": 1
            },
            "flow-code": {
                "name": "Code",
                "status": "in_progress",
                "started_at": "2026-01-15T10:10:00-08:00",
                "completed_at": null,
                "session_started_at": "2026-01-15T10:10:00-08:00",
                "cumulative_seconds": 0,
                "visit_count": 1
            },
            "flow-code-review": {
                "name": "Code Review",
                "status": "pending",
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
        "skills": {}
    })
}

// --- No features ---

#[test]
fn no_state_directory_exits_0_silent() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let result = run_session_context(dir.path());
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(result.stdout.len(), 0, "No stdout when no state files");
}

#[test]
fn empty_state_directory_exits_0_silent() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    fs::create_dir(dir.path().join(".flow-states")).unwrap();
    let result = run_session_context(dir.path());
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(result.stdout.len(), 0, "No stdout when state dir is empty");
}

// --- Tab-colors-only behavior ---

#[test]
fn state_files_present_exits_0_no_json() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let state = make_state();
    fs::write(
        state_dir.join("test-feature.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();

    Command::new("git")
        .args(["checkout", "-b", "test-feature"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let result = run_session_context(dir.path());
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        result.stdout.len(),
        0,
        "Session-context must produce no JSON output — tab colors only"
    );
}

#[test]
fn on_main_with_state_files_exits_0_no_json() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let state = make_state();
    fs::write(
        state_dir.join("test-feature.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();

    // Stay on main — do NOT switch branch
    let result = run_session_context(dir.path());
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        result.stdout.len(),
        0,
        "On main with active flows: must produce no JSON output — tab colors only"
    );
}

#[test]
fn state_files_not_mutated() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let state = make_state();
    let original = serde_json::to_string_pretty(&state).unwrap();
    fs::write(state_dir.join("test-feature.json"), &original).unwrap();

    Command::new("git")
        .args(["checkout", "-b", "test-feature"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    run_session_context(dir.path());

    let after = fs::read_to_string(state_dir.join("test-feature.json")).unwrap();
    assert_eq!(original, after, "State file must not be mutated by session-context");
}

#[test]
fn tab_color_sequences_not_in_stdout() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let result = run_session_context(dir.path());
    assert_eq!(result.status.code(), Some(0));

    // iTerm2 color escape sequences must not appear in stdout (they go to /dev/tty)
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        !stdout.contains("\x1b]6;1;bg;"),
        "Color escape sequences must not be in stdout"
    );
}

// --- Tombstone tests ---

#[test]
fn test_session_context_no_reset_interrupted() {
    // Tombstone: removed in PR #938. Must not return.
    // reset_interrupted corrupted cumulative_seconds on every active flow
    // when a session opened on main.
    let source = include_str!("../src/commands/session_context.rs");
    assert!(
        !source.contains("reset_interrupted"),
        "reset_interrupted was removed in PR #938 to eliminate cross-flow state corruption"
    );
}

#[test]
fn test_session_context_no_feature_context_injection() {
    // Tombstone: removed in PR #938. Must not return.
    // Feature context injection listed all branch names and NOTE_INSTRUCTION
    // in sessions on main, causing flow-note to target random flows.
    let source = include_str!("../src/commands/session_context.rs");
    assert!(
        !source.contains("NOTE_INSTRUCTION"),
        "NOTE_INSTRUCTION was removed in PR #938 to eliminate cross-flow context injection"
    );
    assert!(
        !source.contains("build_single_feature_context"),
        "build_single_feature_context was removed in PR #938"
    );
    assert!(
        !source.contains("build_multi_feature_context"),
        "build_multi_feature_context was removed in PR #938"
    );
}

#[test]
fn test_session_context_no_detect_orchestrate() {
    // Tombstone: removed in PR #938. Must not return.
    // Orchestrate detection will be rethought separately.
    let source = include_str!("../src/commands/session_context.rs");
    assert!(
        !source.contains("detect_orchestrate"),
        "detect_orchestrate was removed in PR #938 — orchestrate detection will be rethought separately"
    );
}
