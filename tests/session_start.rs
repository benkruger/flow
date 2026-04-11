//! Tests for hooks/session-start.sh — the SessionStart hook.

mod common;

use std::fs;
use std::process::Command;

fn run_session_start(cwd: &std::path::Path) -> std::process::Output {
    let script = common::hooks_dir().join("session-start.sh");
    Command::new("bash")
        .arg(&script)
        .current_dir(cwd)
        .output()
        .unwrap()
}

fn init_git_repo(dir: &std::path::Path) {
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(dir)
        .output()
        .unwrap();
    for (key, val) in [
        ("user.email", "test@test.com"),
        ("user.name", "Test"),
        ("commit.gpgsign", "false"),
    ] {
        Command::new("git")
            .args(["config", key, val])
            .current_dir(dir)
            .output()
            .unwrap();
    }
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir)
        .output()
        .unwrap();
}

fn switch_branch(dir: &std::path::Path, branch: &str) {
    Command::new("git")
        .args(["checkout", "-b", branch])
        .current_dir(dir)
        .output()
        .unwrap();
}

fn make_state_json(branch: &str, current_phase: &str) -> String {
    serde_json::json!({
        "schema_version": 1,
        "branch": branch,
        "repo": "test/test",
        "pr_number": 1,
        "pr_url": "https://github.com/test/test/pull/1",
        "started_at": "2026-01-01T00:00:00-08:00",
        "current_phase": current_phase,
        "files": {
            "plan": null,
            "dag": null,
            "log": format!(".flow-states/{}.log", branch),
            "state": format!(".flow-states/{}.json", branch)
        },
        "session_id": null,
        "transcript_path": null,
        "notes": [],
        "prompt": "test feature",
        "phases": {
            "flow-start": { "name": "Start", "status": "complete", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 1 },
            "flow-plan": { "name": "Plan", "status": "complete", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 1 },
            "flow-code": { "name": "Code", "status": "in_progress", "started_at": null, "completed_at": null, "session_started_at": "2026-01-01T00:00:00-08:00", "cumulative_seconds": 0, "visit_count": 1 },
            "flow-code-review": { "name": "Code Review", "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0 },
            "flow-learn": { "name": "Learn", "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0 },
            "flow-complete": { "name": "Complete", "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0 }
        },
        "phase_transitions": []
    })
    .to_string()
}

// --- No features ---

/// No .flow-states/ directory and no .flow.json → exits 0, no stdout.
#[test]
fn no_state_directory_exits_0_silent() {
    let dir = tempfile::tempdir().unwrap();
    let output = run_session_start(dir.path());
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "",
        "Should produce no stdout"
    );
}

/// Empty state directory and no .flow.json → exits 0, no stdout.
#[test]
fn empty_state_directory_exits_0_silent() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".flow-states")).unwrap();
    let output = run_session_start(dir.path());
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "",
        "Should produce no stdout"
    );
}

/// .flow.json exists but no state files → exit 0, no stdout.
#[test]
fn flow_json_no_state_files_exits_0() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());
    fs::write(
        dir.path().join(".flow.json"),
        "{\"flow_version\": \"0.38.0\"}",
    )
    .unwrap();
    let output = run_session_start(dir.path());
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "",
        "Should produce no stdout"
    );
}

// --- Tab color / state behavior tests ---

/// Color escape sequences must not appear in stdout (they go to /dev/tty).
#[test]
fn active_flow_color_sequences_not_in_stdout() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join("color-test.json"),
        make_state_json("color-test", "flow-code"),
    )
    .unwrap();
    switch_branch(dir.path(), "color-test");

    let output = run_session_start(dir.path());
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        "",
        "Session-context must produce no JSON output — tab colors only"
    );
    assert!(
        !stdout.contains("\x1b]6;1;bg;"),
        "iTerm2 color escape sequences must not appear in stdout"
    );
}

/// State files exist on matching branch → exit 0, no stdout (tab colors only).
#[test]
fn state_files_present_exits_0_no_json() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join("my-feature.json"),
        make_state_json("my-feature", "flow-code"),
    )
    .unwrap();
    switch_branch(dir.path(), "my-feature");

    let output = run_session_start(dir.path());
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "",
        "Session-context must produce no JSON output — tab colors only"
    );
}

/// On main with active feature state files → exit 0, no stdout.
#[test]
fn on_main_with_state_files_exits_0_no_json() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join("some-feature.json"),
        make_state_json("some-feature", "flow-code"),
    )
    .unwrap();
    // Stay on main — do NOT switch branches
    let output = run_session_start(dir.path());
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "",
        "On main with active flows: must produce no JSON output"
    );
}

/// Session-context must not mutate any state files.
#[test]
fn state_files_not_mutated() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();

    let mut state: serde_json::Value =
        serde_json::from_str(&make_state_json("my-feature", "flow-code")).unwrap();
    state["_last_failure"] = serde_json::json!({
        "type": "test",
        "message": "should survive",
        "timestamp": "2026-01-01T00:00:00-08:00"
    });
    state["compact_summary"] = serde_json::json!("should survive");
    state["_blocked"] = serde_json::json!("2026-01-01T00:00:00-08:00");

    let state_path = state_dir.join("my-feature.json");
    fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();

    switch_branch(dir.path(), "my-feature");
    let original = fs::read_to_string(&state_path).unwrap();
    run_session_start(dir.path());
    let after = fs::read_to_string(&state_path).unwrap();

    assert_eq!(
        original, after,
        "State file must not be mutated by session-context"
    );
}
