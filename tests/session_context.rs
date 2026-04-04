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

fn switch_branch(dir: &std::path::Path, branch: &str) {
    Command::new("git")
        .args(["checkout", "-b", branch])
        .current_dir(dir)
        .output()
        .unwrap();
}

fn detach_head(dir: &std::path::Path) {
    Command::new("git")
        .args(["checkout", "--detach"])
        .current_dir(dir)
        .output()
        .unwrap();
}

fn make_state(overrides: Value) -> Value {
    let mut state = json!({
        "schema_version": 1,
        "branch": "test-feature",
        "repo": "test/repo",
        "pr_number": 1,
        "pr_url": "https://github.com/test/repo/pull/1",
        "started_at": "2026-01-15T10:00:00-08:00",
        "current_phase": "flow-start",
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
                "status": "in_progress",
                "started_at": "2026-01-15T10:00:00-08:00",
                "completed_at": null,
                "session_started_at": null,
                "cumulative_seconds": 0,
                "visit_count": 1
            },
            "flow-plan": {
                "name": "Plan",
                "status": "pending",
                "started_at": null,
                "completed_at": null,
                "session_started_at": null,
                "cumulative_seconds": 0,
                "visit_count": 0
            },
            "flow-code": {
                "name": "Code",
                "status": "pending",
                "started_at": null,
                "completed_at": null,
                "session_started_at": null,
                "cumulative_seconds": 0,
                "visit_count": 0
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
    });
    if let (Some(base), Some(over)) = (state.as_object_mut(), overrides.as_object()) {
        for (k, v) in over {
            base.insert(k.clone(), v.clone());
        }
    }
    state
}

fn write_state(state_dir: &std::path::Path, name: &str, state: &Value) {
    fs::write(
        state_dir.join(format!("{}.json", name)),
        serde_json::to_string_pretty(state).unwrap(),
    )
    .unwrap();
}

fn run_session_context(dir: &std::path::Path) -> std::process::Output {
    flow_rs()
        .arg("session-context")
        .current_dir(dir)
        .output()
        .unwrap()
}

fn parse_stdout(output: &std::process::Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "Failed to parse JSON: {}\nstdout: {}\nstderr: {}",
            e,
            stdout,
            String::from_utf8_lossy(&output.stderr)
        )
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

// --- Single feature ---

#[test]
fn single_feature_returns_valid_json() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({
        "current_phase": "flow-plan",
        "branch": "invoice-pdf-export"
    }));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("in_progress");
    write_state(&state_dir, "invoice-pdf-export", &state);

    switch_branch(dir.path(), "invoice-pdf-export");
    let result = run_session_context(dir.path());
    assert_eq!(result.status.code(), Some(0));

    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("flow-session-context"), "Should contain flow-session-context tag");
    assert!(ctx.contains("Invoice Pdf Export"), "Should contain feature name");
    assert!(ctx.contains("flow:flow-continue"), "Should mention flow:flow-continue");
}
