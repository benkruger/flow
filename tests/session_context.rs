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

// --- Timing reset and transient data ---

#[test]
fn single_feature_resets_session_started_at() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-plan", "branch": "my-feature"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("in_progress");
    state["phases"]["flow-plan"]["session_started_at"] = json!("2026-01-15T10:00:00+00:00");
    state["phases"]["flow-plan"]["cumulative_seconds"] = json!(0);
    write_state(&state_dir, "my-feature", &state);

    switch_branch(dir.path(), "my-feature");
    run_session_context(dir.path());

    let updated: Value = serde_json::from_str(
        &fs::read_to_string(state_dir.join("my-feature.json")).unwrap(),
    )
    .unwrap();
    let restarted = updated["phases"]["flow-plan"]["session_started_at"]
        .as_str()
        .expect("session_started_at should be reset to now(), not null");
    assert_ne!(
        restarted, "2026-01-15T10:00:00+00:00",
        "session_started_at should be updated"
    );
    assert!(
        updated["phases"]["flow-plan"]["cumulative_seconds"]
            .as_i64()
            .unwrap()
            > 0,
        "cumulative_seconds should increase"
    );
}

#[test]
fn reset_interrupted_preserves_existing_cumulative_seconds() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-plan", "branch": "my-feature"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("in_progress");
    state["phases"]["flow-plan"]["session_started_at"] = json!("2026-01-15T10:00:00+00:00");
    state["phases"]["flow-plan"]["cumulative_seconds"] = json!(600);
    write_state(&state_dir, "my-feature", &state);

    switch_branch(dir.path(), "my-feature");
    run_session_context(dir.path());

    let updated: Value = serde_json::from_str(
        &fs::read_to_string(state_dir.join("my-feature.json")).unwrap(),
    )
    .unwrap();
    assert!(
        updated["phases"]["flow-plan"]["cumulative_seconds"]
            .as_i64()
            .unwrap()
            > 600,
        "Should accumulate on top of existing 600"
    );
}

#[test]
fn reset_interrupted_null_session_started_at_no_change() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-plan", "branch": "my-feature"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("in_progress");
    state["phases"]["flow-plan"]["session_started_at"] = Value::Null;
    state["phases"]["flow-plan"]["cumulative_seconds"] = json!(300);
    write_state(&state_dir, "my-feature", &state);

    switch_branch(dir.path(), "my-feature");
    run_session_context(dir.path());

    let updated: Value = serde_json::from_str(
        &fs::read_to_string(state_dir.join("my-feature.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        updated["phases"]["flow-plan"]["cumulative_seconds"]
            .as_i64()
            .unwrap(),
        300,
        "Null session_started_at should not change cumulative_seconds"
    );
}

#[test]
fn reset_interrupted_null_cumulative_seconds() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-plan", "branch": "my-feature"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("in_progress");
    state["phases"]["flow-plan"]["session_started_at"] = json!("2026-01-15T10:00:00+00:00");
    state["phases"]["flow-plan"]["cumulative_seconds"] = Value::Null;
    write_state(&state_dir, "my-feature", &state);

    switch_branch(dir.path(), "my-feature");
    run_session_context(dir.path());

    let updated: Value = serde_json::from_str(
        &fs::read_to_string(state_dir.join("my-feature.json")).unwrap(),
    )
    .unwrap();
    assert!(
        !updated["phases"]["flow-plan"]["cumulative_seconds"].is_null(),
        "cumulative_seconds should not stay null"
    );
    assert!(
        updated["phases"]["flow-plan"]["cumulative_seconds"]
            .as_i64()
            .unwrap()
            > 0,
        "Should accumulate from 0 (null treated as 0)"
    );
}

#[test]
fn reset_interrupted_clears_blocked() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-plan", "branch": "my-feature"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("in_progress");
    state["phases"]["flow-plan"]["session_started_at"] = Value::Null;
    state["phases"]["flow-plan"]["cumulative_seconds"] = json!(300);
    state["_blocked"] = json!("2026-01-15T10:00:00-08:00");
    write_state(&state_dir, "my-feature", &state);

    switch_branch(dir.path(), "my-feature");
    run_session_context(dir.path());

    let updated: Value = serde_json::from_str(
        &fs::read_to_string(state_dir.join("my-feature.json")).unwrap(),
    )
    .unwrap();
    assert!(updated.get("_blocked").is_none(), "_blocked should be cleared");
    assert_eq!(
        updated["phases"]["flow-plan"]["cumulative_seconds"]
            .as_i64()
            .unwrap(),
        300,
    );
}

#[test]
fn last_failure_injected_into_context() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-code", "branch": "my-feature"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("complete");
    state["phases"]["flow-code"]["status"] = json!("in_progress");
    state["_last_failure"] = json!({
        "type": "rate_limit",
        "message": "429 Too Many Requests",
        "timestamp": "2026-03-28T14:23:00-07:00"
    });
    write_state(&state_dir, "my-feature", &state);

    switch_branch(dir.path(), "my-feature");
    let result = run_session_context(dir.path());
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("rate_limit"), "Should mention failure type");
    assert!(ctx.contains("2026-03-28T14:23:00-07:00"), "Should mention timestamp");
}

#[test]
fn last_failure_cleared_from_state_after_injection() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-code", "branch": "my-feature"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("complete");
    state["phases"]["flow-code"]["status"] = json!("in_progress");
    state["_last_failure"] = json!({
        "type": "auth_failure",
        "message": "Invalid API key",
        "timestamp": "2026-03-28T14:23:00-07:00"
    });
    write_state(&state_dir, "my-feature", &state);

    switch_branch(dir.path(), "my-feature");
    run_session_context(dir.path());

    let updated: Value = serde_json::from_str(
        &fs::read_to_string(state_dir.join("my-feature.json")).unwrap(),
    )
    .unwrap();
    assert!(updated.get("_last_failure").is_none(), "_last_failure should be cleared");
}

#[test]
fn compact_summary_injected_into_context() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-code", "branch": "my-feature"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("complete");
    state["phases"]["flow-code"]["status"] = json!("in_progress");
    state["compact_summary"] = json!("User was writing tests for webhook handler.");
    write_state(&state_dir, "my-feature", &state);

    switch_branch(dir.path(), "my-feature");
    let result = run_session_context(dir.path());
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("compact-summary"), "Should contain compact-summary tag");
    assert!(
        ctx.contains("User was writing tests for webhook handler."),
        "Should contain the summary text"
    );
}

#[test]
fn compact_summary_cleared_from_state_after_injection() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-code", "branch": "my-feature"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("complete");
    state["phases"]["flow-code"]["status"] = json!("in_progress");
    state["compact_summary"] = json!("Summary to consume.");
    state["compact_cwd"] = json!("/some/path");
    write_state(&state_dir, "my-feature", &state);

    switch_branch(dir.path(), "my-feature");
    run_session_context(dir.path());

    let updated: Value = serde_json::from_str(
        &fs::read_to_string(state_dir.join("my-feature.json")).unwrap(),
    )
    .unwrap();
    assert!(updated.get("compact_summary").is_none(), "compact_summary should be cleared");
    assert!(updated.get("compact_cwd").is_none(), "compact_cwd should be cleared");
}

#[test]
fn compact_cwd_mismatch_shows_warning() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-code", "branch": "my-feature"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("complete");
    state["phases"]["flow-code"]["status"] = json!("in_progress");
    state["compact_cwd"] = json!("/wrong/directory");
    write_state(&state_dir, "my-feature", &state);

    switch_branch(dir.path(), "my-feature");
    let result = run_session_context(dir.path());
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("/wrong/directory"), "Should mention wrong CWD");
    assert!(ctx.contains(".worktrees/my-feature"), "Should mention worktree");
}

// --- Context building ---

#[test]
fn single_feature_does_not_force_action() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-plan", "branch": "my-feature"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("in_progress");
    write_state(&state_dir, "my-feature", &state);

    switch_branch(dir.path(), "my-feature");
    let result = run_session_context(dir.path());
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(!ctx.contains("FIRST action"), "Should not force FIRST action");
    assert!(
        !ctx.contains("Invoke the flow:flow-continue skill"),
        "Should not command Claude to invoke"
    );
}

#[test]
fn single_feature_includes_note_instruction() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-plan", "branch": "my-feature"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("in_progress");
    write_state(&state_dir, "my-feature", &state);

    switch_branch(dir.path(), "my-feature");
    let result = run_session_context(dir.path());
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("flow:flow-note"), "Should include note instruction");
}

#[test]
fn phase_2_plan_approved_instructs_auto_continue() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-plan", "branch": "my-feature"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("in_progress");
    state["plan_file"] = json!("/Users/test/.claude/plans/test-plan.md");
    write_state(&state_dir, "my-feature", &state);

    switch_branch(dir.path(), "my-feature");
    let result = run_session_context(dir.path());
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("flow:flow-continue"), "Should mention flow-continue");
    assert!(
        !ctx.contains("Do NOT invoke flow:flow-continue"),
        "Should not tell to NOT invoke"
    );
}

#[test]
fn phase_2_no_plan_file_does_not_auto_continue() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-plan", "branch": "my-feature"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("in_progress");
    state["plan_file"] = Value::Null;
    write_state(&state_dir, "my-feature", &state);

    switch_branch(dir.path(), "my-feature");
    let result = run_session_context(dir.path());
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("Do NOT invoke flow:flow-continue"));
}

#[test]
fn phase_2_plan_approved_via_files_block() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-plan", "branch": "my-feature"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("in_progress");
    state["files"]["plan"] = json!(".flow-states/my-feature-plan.md");
    write_state(&state_dir, "my-feature", &state);

    switch_branch(dir.path(), "my-feature");
    let result = run_session_context(dir.path());
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("flow:flow-continue"));
    assert!(!ctx.contains("Do NOT invoke flow:flow-continue"));
}

#[test]
fn never_entered_phase_instructs_auto_continue() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    // current_phase advanced to flow-code by Plan completion, but flow-code is still pending
    let mut state = make_state(json!({"current_phase": "flow-code", "branch": "auto-continue"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("complete");
    // flow-code is pending (not entered yet)
    write_state(&state_dir, "auto-continue", &state);

    switch_branch(dir.path(), "auto-continue");
    let result = run_session_context(dir.path());
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("flow:flow-continue"));
    assert!(!ctx.contains("Do NOT invoke flow:flow-continue"));
}

#[test]
fn phase_1_in_progress_does_not_auto_continue() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-start", "branch": "fresh-start"}));
    state["phases"]["flow-start"]["status"] = json!("in_progress");
    write_state(&state_dir, "fresh-start", &state);

    switch_branch(dir.path(), "fresh-start");
    let result = run_session_context(dir.path());
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("Do NOT invoke"));
}

#[test]
fn code_review_with_step_tracking_shows_progress() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-code-review", "branch": "step-tracking"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("complete");
    state["phases"]["flow-code"]["status"] = json!("complete");
    state["phases"]["flow-code-review"]["status"] = json!("in_progress");
    state["code_review_step"] = json!(2);
    write_state(&state_dir, "step-tracking", &state);

    switch_branch(dir.path(), "step-tracking");
    let result = run_session_context(dir.path());
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("Step 2/4 done"), "Should show step progress");
    assert!(ctx.contains("Security"), "Should name step 3 (Security)");
}

#[test]
fn code_review_bad_step_does_not_crash() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-code-review", "branch": "bad-step"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("complete");
    state["phases"]["flow-code"]["status"] = json!("complete");
    state["phases"]["flow-code-review"]["status"] = json!("in_progress");
    state["code_review_step"] = json!("bad");
    write_state(&state_dir, "bad-step", &state);

    switch_branch(dir.path(), "bad-step");
    let result = run_session_context(dir.path());
    assert_eq!(result.status.code(), Some(0));
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("Bad Step"));
    assert!(!ctx.contains("done"));
}

#[test]
fn code_review_empty_string_step_does_not_crash() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-code-review", "branch": "empty-step"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("complete");
    state["phases"]["flow-code"]["status"] = json!("complete");
    state["phases"]["flow-code-review"]["status"] = json!("in_progress");
    state["code_review_step"] = json!("");
    write_state(&state_dir, "empty-step", &state);

    switch_branch(dir.path(), "empty-step");
    let result = run_session_context(dir.path());
    assert_eq!(result.status.code(), Some(0));
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("Empty Step"));
    assert!(!ctx.contains("done"));
}

#[test]
fn multi_feature_code_review_step_tracking() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut s1 = make_state(json!({"current_phase": "flow-code-review", "branch": "review-feature"}));
    s1["phases"]["flow-start"]["status"] = json!("complete");
    s1["phases"]["flow-plan"]["status"] = json!("complete");
    s1["phases"]["flow-code"]["status"] = json!("complete");
    s1["phases"]["flow-code-review"]["status"] = json!("in_progress");
    s1["code_review_step"] = json!(3);
    write_state(&state_dir, "review-feature", &s1);

    let mut s2 = make_state(json!({"current_phase": "flow-code", "branch": "other-feature"}));
    s2["phases"]["flow-start"]["status"] = json!("complete");
    s2["phases"]["flow-plan"]["status"] = json!("complete");
    s2["phases"]["flow-code"]["status"] = json!("in_progress");
    write_state(&state_dir, "other-feature", &s2);

    detach_head(dir.path());
    let result = run_session_context(dir.path());
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("Step 3/4 done"));
    assert!(ctx.contains("Code Review Plugin"));
}

#[test]
fn single_feature_includes_implementation_guardrail() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-code", "branch": "my-feature"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("complete");
    state["phases"]["flow-code"]["status"] = json!("in_progress");
    write_state(&state_dir, "my-feature", &state);

    switch_branch(dir.path(), "my-feature");
    let result = run_session_context(dir.path());
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("NEVER implement"), "Should include guardrail");
}

#[test]
fn multiple_features_includes_implementation_guardrail() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut s1 = make_state(json!({"current_phase": "flow-plan", "branch": "feature-a"}));
    s1["phases"]["flow-start"]["status"] = json!("complete");
    s1["phases"]["flow-plan"]["status"] = json!("in_progress");
    write_state(&state_dir, "feature-a", &s1);

    let mut s2 = make_state(json!({"current_phase": "flow-code", "branch": "feature-b"}));
    s2["phases"]["flow-start"]["status"] = json!("complete");
    s2["phases"]["flow-plan"]["status"] = json!("complete");
    s2["phases"]["flow-code"]["status"] = json!("in_progress");
    write_state(&state_dir, "feature-b", &s2);

    detach_head(dir.path());
    let result = run_session_context(dir.path());
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("NEVER implement"), "Should include guardrail");
}

#[test]
fn output_has_both_context_fields() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-start", "branch": "some-feature"}));
    state["phases"]["flow-start"]["status"] = json!("in_progress");
    write_state(&state_dir, "some-feature", &state);

    switch_branch(dir.path(), "some-feature");
    let result = run_session_context(dir.path());
    assert_eq!(result.status.code(), Some(0));

    let output = parse_stdout(&result);
    assert!(output.get("additional_context").is_some());
    assert!(output.get("hookSpecificOutput").is_some());
    assert!(output["hookSpecificOutput"].get("additionalContext").is_some());
    assert_eq!(
        output["additional_context"],
        output["hookSpecificOutput"]["additionalContext"],
        "Both fields must contain identical context"
    );
}

#[test]
fn dev_mode_preamble_when_plugin_root_backup_present() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-code", "branch": "dev-mode-test"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("complete");
    state["phases"]["flow-code"]["status"] = json!("in_progress");
    write_state(&state_dir, "dev-mode-test", &state);

    fs::write(
        dir.path().join(".flow.json"),
        r#"{"flow_version": "0.39.0", "plugin_root": "/local/path", "plugin_root_backup": "/cache/path"}"#,
    )
    .unwrap();

    switch_branch(dir.path(), "dev-mode-test");
    let result = run_session_context(dir.path());
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(ctx.contains("[DEV MODE]"), "Should include dev mode preamble");
}

#[test]
fn no_dev_mode_preamble_without_plugin_root_backup() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();

    let mut state = make_state(json!({"current_phase": "flow-code", "branch": "no-dev-mode"}));
    state["phases"]["flow-start"]["status"] = json!("complete");
    state["phases"]["flow-plan"]["status"] = json!("complete");
    state["phases"]["flow-code"]["status"] = json!("in_progress");
    write_state(&state_dir, "no-dev-mode", &state);

    fs::write(
        dir.path().join(".flow.json"),
        r#"{"flow_version": "0.39.0", "plugin_root": "/cache/path"}"#,
    )
    .unwrap();

    switch_branch(dir.path(), "no-dev-mode");
    let result = run_session_context(dir.path());
    let output = parse_stdout(&result);
    let ctx = output["additional_context"].as_str().unwrap();
    assert!(!ctx.contains("[DEV MODE]"), "Should NOT include dev mode preamble");
}
