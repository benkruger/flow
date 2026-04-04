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

// --- Branch isolation ---

#[test]
fn processes_only_matching_branch_state() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut s1 = make_state(json!({"current_phase": "flow-code", "branch": "feature-alpha"}));
    s1["phases"]["flow-start"]["status"] = json!("complete");
    s1["phases"]["flow-plan"]["status"] = json!("complete");
    s1["phases"]["flow-code"]["status"] = json!("in_progress");
    write_state(&state_dir, "feature-alpha", &s1);

    let mut s2 = make_state(json!({"current_phase": "flow-plan", "branch": "feature-beta"}));
    s2["phases"]["flow-start"]["status"] = json!("complete");
    s2["phases"]["flow-plan"]["status"] = json!("in_progress");
    write_state(&state_dir, "feature-beta", &s2);

    switch_branch(dir.path(), "feature-alpha");
    let result = run_session_context(dir.path());
    assert_eq!(result.status.code(), Some(0));
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("Feature Alpha"), "Should contain Feature Alpha");
    assert!(!ctx.contains("Feature Beta"), "Should NOT contain Feature Beta");
    assert!(!ctx.contains("Multiple"), "Should NOT be multi-feature");
}

#[test]
fn detached_head_multiple_files_fallback() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut s1 = make_state(json!({"current_phase": "flow-code", "branch": "feature-one"}));
    s1["phases"]["flow-start"]["status"] = json!("complete");
    s1["phases"]["flow-plan"]["status"] = json!("complete");
    s1["phases"]["flow-code"]["status"] = json!("in_progress");
    write_state(&state_dir, "feature-one", &s1);

    let mut s2 = make_state(json!({"current_phase": "flow-plan", "branch": "feature-two"}));
    s2["phases"]["flow-start"]["status"] = json!("complete");
    s2["phases"]["flow-plan"]["status"] = json!("in_progress");
    write_state(&state_dir, "feature-two", &s2);

    detach_head(dir.path());
    let result = run_session_context(dir.path());
    assert_eq!(result.status.code(), Some(0));
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("Multiple FLOW features"), "Should list multiple features");
    assert!(ctx.contains("Feature One"), "Should contain Feature One");
    assert!(ctx.contains("Feature Two"), "Should contain Feature Two");
}

#[test]
fn on_main_sees_feature_state_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-code", "branch": "some-feature"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("complete");
    state["phases"]["flow-code"]["status"] = json!("in_progress");
    write_state(&state_dir, "some-feature", &state);

    // Stay on main — do NOT switch branch
    let result = run_session_context(dir.path());
    assert_eq!(result.status.code(), Some(0));
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("Some Feature"), "Main branch should see all features");
}

// --- Edge cases ---

#[test]
fn phases_json_files_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-code", "branch": "real-feature"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("complete");
    state["phases"]["flow-code"]["status"] = json!("in_progress");
    write_state(&state_dir, "real-feature", &state);

    // Ghost: a -phases.json file
    fs::write(
        state_dir.join("real-feature-phases.json"),
        r#"{"phases": []}"#,
    )
    .unwrap();

    switch_branch(dir.path(), "real-feature");
    let result = run_session_context(dir.path());
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("Real Feature"), "Should show Real Feature");
    assert!(!ctx.contains("Multiple"), "Should NOT be multi-feature");
}

#[test]
fn corrupt_state_file_among_valid_ones() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    // Corrupt file
    fs::write(state_dir.join("corrupt.json"), "{bad json").unwrap();

    // Valid file
    let mut state = make_state(json!({"current_phase": "flow-start", "branch": "valid-branch"}));
    state["phases"]["flow-start"]["status"] = json!("in_progress");
    write_state(&state_dir, "valid-branch", &state);

    switch_branch(dir.path(), "valid-branch");
    let result = run_session_context(dir.path());
    assert_eq!(result.status.code(), Some(0));
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("Valid Branch"), "Should show valid feature despite corrupt file");
}

#[test]
fn all_corrupt_state_files_exits_0_silent() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();
    fs::write(state_dir.join("bad-one.json"), "{broken").unwrap();
    fs::write(state_dir.join("bad-two.json"), "not json at all").unwrap();

    let result = run_session_context(dir.path());
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(result.stdout.len(), 0, "All corrupt → silent exit");
}

#[test]
fn non_json_files_ignored() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();
    fs::write(state_dir.join("notes.txt"), "not a state file").unwrap();
    fs::write(state_dir.join("backup.bak"), "also not a state file").unwrap();

    let result = run_session_context(dir.path());
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(result.stdout.len(), 0, "Non-JSON files → silent exit");
}

// --- Multiple features ---

#[test]
fn multiple_features_mentions_both() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut s1 = make_state(json!({"current_phase": "flow-plan", "branch": "feature-alpha"}));
    s1["phases"]["flow-start"]["status"] = json!("complete");
    s1["phases"]["flow-plan"]["status"] = json!("in_progress");
    write_state(&state_dir, "feature-alpha", &s1);

    let mut s2 = make_state(json!({
        "current_phase": "flow-code-review",
        "branch": "feature-beta"
    }));
    s2["phases"]["flow-start"]["status"] = json!("complete");
    s2["phases"]["flow-plan"]["status"] = json!("complete");
    s2["phases"]["flow-code"]["status"] = json!("complete");
    s2["phases"]["flow-code-review"]["status"] = json!("in_progress");
    write_state(&state_dir, "feature-beta", &s2);

    detach_head(dir.path());
    let result = run_session_context(dir.path());
    assert_eq!(result.status.code(), Some(0));
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("Multiple FLOW features"), "Should say multiple");
    assert!(ctx.contains("Feature Alpha"), "Should list alpha");
    assert!(ctx.contains("Feature Beta"), "Should list beta");
}

// --- Orchestrate detection ---

fn make_orchestrate_state(
    queue: Vec<Value>,
    completed_at: Option<&str>,
    current_index: Option<i64>,
) -> Value {
    json!({
        "started_at": "2026-03-20T22:00:00-07:00",
        "completed_at": completed_at,
        "queue": queue,
        "current_index": current_index,
    })
}

#[test]
fn orchestrate_in_progress_injects_resume() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let orch = make_orchestrate_state(
        vec![
            json!({"issue_number": 42, "title": "Add PDF export", "status": "completed", "outcome": "completed"}),
            json!({"issue_number": 43, "title": "Fix login timeout", "status": "in_progress", "outcome": null}),
            json!({"issue_number": 44, "title": "Refactor auth", "status": "pending", "outcome": null}),
        ],
        None,
        Some(1),
    );
    fs::write(
        state_dir.join("orchestrate.json"),
        serde_json::to_string(&orch).unwrap(),
    )
    .unwrap();

    let result = run_session_context(dir.path());
    assert_eq!(result.status.code(), Some(0));
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.to_lowercase().contains("orchestrat"), "Should mention orchestration");
    assert!(ctx.contains("#43"), "Should mention current issue");
    assert!(ctx.to_lowercase().contains("flow-orchestrate"), "Should mention resume command");
}

#[test]
fn orchestrate_completed_injects_report() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let orch = make_orchestrate_state(
        vec![json!({"issue_number": 42, "title": "Add PDF export", "status": "completed", "outcome": "completed"})],
        Some("2026-03-21T06:00:00-07:00"),
        None,
    );
    fs::write(
        state_dir.join("orchestrate.json"),
        serde_json::to_string(&orch).unwrap(),
    )
    .unwrap();

    let summary_content = "# FLOW Orchestration Report\n\nCompleted: 1, Failed: 0";
    fs::write(state_dir.join("orchestrate-summary.md"), summary_content).unwrap();

    let result = run_session_context(dir.path());
    assert_eq!(result.status.code(), Some(0));
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.to_lowercase().contains("orchestrat"), "Should mention orchestration");
    assert!(ctx.contains("Orchestration Report"), "Should include report content");
}

#[test]
fn orchestrate_completed_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let orch = make_orchestrate_state(vec![], Some("2026-03-21T06:00:00-07:00"), None);
    fs::write(
        state_dir.join("orchestrate.json"),
        serde_json::to_string(&orch).unwrap(),
    )
    .unwrap();
    fs::write(state_dir.join("orchestrate-summary.md"), "# Report").unwrap();
    fs::write(state_dir.join("orchestrate.log"), "log line").unwrap();
    fs::write(
        state_dir.join("orchestrate-queue.json"),
        r#"[{"issue_number": 42}]"#,
    )
    .unwrap();

    run_session_context(dir.path());

    assert!(!state_dir.join("orchestrate.json").exists(), "orchestrate.json should be deleted");
    assert!(!state_dir.join("orchestrate-summary.md").exists(), "summary should be deleted");
    assert!(!state_dir.join("orchestrate.log").exists(), "log should be deleted");
    assert!(!state_dir.join("orchestrate-queue.json").exists(), "queue should be deleted");
}

#[test]
fn orchestrate_all_processed_no_resume() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let orch = make_orchestrate_state(
        vec![
            json!({"issue_number": 42, "title": "Add PDF export", "status": "completed", "outcome": "completed"}),
            json!({"issue_number": 43, "title": "Fix login timeout", "status": "failed", "outcome": "failed"}),
            json!({"issue_number": 44, "title": "Refactor auth", "status": "completed", "outcome": "completed"}),
        ],
        None,
        Some(2),
    );
    fs::write(
        state_dir.join("orchestrate.json"),
        serde_json::to_string(&orch).unwrap(),
    )
    .unwrap();

    let result = run_session_context(dir.path());
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(result.stdout.len(), 0, "All processed → silent exit");
}

#[test]
fn orchestrate_coexists_with_feature() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    // Orchestrate state (in progress)
    let orch = make_orchestrate_state(
        vec![json!({"issue_number": 42, "title": "Add PDF export", "status": "in_progress", "outcome": null})],
        None,
        Some(0),
    );
    fs::write(
        state_dir.join("orchestrate.json"),
        serde_json::to_string(&orch).unwrap(),
    )
    .unwrap();

    // Feature state
    let mut state = make_state(json!({"current_phase": "flow-code", "branch": "some-feature"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("complete");
    state["phases"]["flow-code"]["status"] = json!("in_progress");
    write_state(&state_dir, "some-feature", &state);

    switch_branch(dir.path(), "some-feature");
    let result = run_session_context(dir.path());
    assert_eq!(result.status.code(), Some(0));
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.to_lowercase().contains("orchestrat"), "Should mention orchestration");
    assert!(ctx.contains("Some Feature"), "Should mention feature");
}

#[test]
fn orchestrate_missing_summary() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let orch = make_orchestrate_state(vec![], Some("2026-03-21T06:00:00-07:00"), None);
    fs::write(
        state_dir.join("orchestrate.json"),
        serde_json::to_string(&orch).unwrap(),
    )
    .unwrap();
    // No summary file — should not crash

    let result = run_session_context(dir.path());
    assert_eq!(result.status.code(), Some(0));
    // orchestrate.json should still be cleaned up
    assert!(!state_dir.join("orchestrate.json").exists(), "Should clean up even without summary");
}

#[test]
fn no_orchestrate_file_existing_behavior() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-code", "branch": "normal-feature"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("complete");
    state["phases"]["flow-code"]["status"] = json!("in_progress");
    write_state(&state_dir, "normal-feature", &state);

    switch_branch(dir.path(), "normal-feature");
    let result = run_session_context(dir.path());
    assert_eq!(result.status.code(), Some(0));
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("Normal Feature"), "Should show feature");
    assert!(!ctx.to_lowercase().contains("orchestrat"), "Should NOT mention orchestration");
}
