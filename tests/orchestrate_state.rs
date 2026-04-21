//! Integration tests for `flow_rs::orchestrate_state` — drives the public
//! surface (`create_state`, `start_issue`, `record_outcome`,
//! `complete_orchestration`, `read_state`, `next_issue`, `run_impl`,
//! `run_impl_main`) through constructed inputs and tempdir-backed state
//! files. No inline tests remain in `src/orchestrate_state.rs`.

use std::fs;

use flow_rs::orchestrate_state::{
    complete_orchestration, create_state, next_issue, read_state, record_outcome, run_impl,
    run_impl_main, start_issue, Args,
};
use serde_json::{json, Value};

fn sample_queue() -> Vec<Value> {
    vec![
        json!({"issue_number": 42, "title": "Add PDF export"}),
        json!({"issue_number": 43, "title": "Fix login timeout"}),
        json!({"issue_number": 44, "title": "Refactor auth middleware"}),
    ]
}

fn default_args() -> Args {
    Args {
        create: false,
        start_issue: None,
        record_outcome: None,
        complete: false,
        read: false,
        next: false,
        queue_file: None,
        state_dir: None,
        state_file: None,
        outcome: None,
        pr_url: None,
        branch: None,
        reason: None,
    }
}

// --- create_state ---

#[test]
fn test_create_state() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");

    let result = create_state(&sample_queue(), &state_dir);
    assert_eq!(result["status"], "ok");

    let content = fs::read_to_string(state_dir.join("orchestrate.json")).unwrap();
    let state: Value = serde_json::from_str(&content).unwrap();

    assert!(state["started_at"].as_str().unwrap().contains("T"));
    assert!(state["completed_at"].is_null());
    assert!(state["current_index"].is_null());

    let queue = state["queue"].as_array().unwrap();
    assert_eq!(queue.len(), 3);
    assert_eq!(queue[0]["issue_number"], 42);
    assert_eq!(queue[0]["status"], "pending");
    assert!(queue[0]["started_at"].is_null());
    assert!(queue[0]["completed_at"].is_null());
    assert!(queue[0]["outcome"].is_null());
    assert!(queue[0]["pr_url"].is_null());
    assert!(queue[0]["branch"].is_null());
    assert!(queue[0]["reason"].is_null());
}

#[test]
fn test_create_state_empty_queue() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");

    let result = create_state(&[], &state_dir);
    assert_eq!(result["status"], "ok");

    let content = fs::read_to_string(state_dir.join("orchestrate.json")).unwrap();
    let state: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(state["queue"].as_array().unwrap().len(), 0);
}

#[test]
fn test_create_state_already_exists_in_progress() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();

    let existing = json!({
        "started_at": "2026-03-20T20:00:00-07:00",
        "completed_at": null,
        "queue": [],
        "current_index": 0,
    });
    fs::write(
        state_dir.join("orchestrate.json"),
        serde_json::to_string_pretty(&existing).unwrap(),
    )
    .unwrap();

    let result = create_state(&sample_queue(), &state_dir);
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("already in progress"));
}

#[test]
fn test_create_state_overwrites_completed() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();

    let existing = json!({
        "started_at": "2026-03-20T20:00:00-07:00",
        "completed_at": "2026-03-20T21:00:00-07:00",
        "queue": [],
        "current_index": null,
    });
    fs::write(
        state_dir.join("orchestrate.json"),
        serde_json::to_string_pretty(&existing).unwrap(),
    )
    .unwrap();

    let result = create_state(&sample_queue(), &state_dir);
    assert_eq!(result["status"], "ok");

    let content = fs::read_to_string(state_dir.join("orchestrate.json")).unwrap();
    let state: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(state["queue"].as_array().unwrap().len(), 3);
}

#[test]
fn test_create_state_creates_directory() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    assert!(!state_dir.exists());

    let result = create_state(&sample_queue(), &state_dir);
    assert_eq!(result["status"], "ok");
    assert!(state_dir.join("orchestrate.json").exists());
}

// Replaces the inline `build_queue_item_missing_issue_number_defaults_to_zero`
// test — drives the same defaulting branch via the public `create_state`
// entry, which calls `build_queue_item` internally.
#[test]
fn create_state_missing_issue_number_defaults_to_zero() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");

    let result = create_state(&[json!({"title": "No number"})], &state_dir);
    assert_eq!(result["status"], "ok");

    let content = fs::read_to_string(state_dir.join("orchestrate.json")).unwrap();
    let state: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(state["queue"][0]["issue_number"], 0);
    assert_eq!(state["queue"][0]["title"], "No number");
}

// Replaces the inline `build_queue_item_missing_title_defaults_to_empty`
// test — drives the same defaulting branch via `create_state`.
#[test]
fn create_state_missing_title_defaults_to_empty() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");

    let result = create_state(&[json!({"issue_number": 42})], &state_dir);
    assert_eq!(result["status"], "ok");

    let content = fs::read_to_string(state_dir.join("orchestrate.json")).unwrap();
    let state: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(state["queue"][0]["issue_number"], 42);
    assert_eq!(state["queue"][0]["title"], "");
}

#[test]
fn create_state_existing_corrupt_json_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    // Existing but corrupt — the `if let Ok(existing) = ...` guard in
    // create_state is False, so the "already in progress" check is
    // skipped and the file gets overwritten.
    fs::write(state_dir.join("orchestrate.json"), "{bad json").unwrap();

    let result = create_state(&sample_queue(), &state_dir);
    assert_eq!(result["status"], "ok");

    let content = fs::read_to_string(state_dir.join("orchestrate.json")).unwrap();
    let state: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(state["queue"].as_array().unwrap().len(), 3);
}

#[test]
fn orchestrate_create_state_write_failure_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    // Pre-create the target path as a directory — fs::write EISDIRs.
    fs::create_dir(state_dir.join("orchestrate.json")).unwrap();

    let result = create_state(&sample_queue(), &state_dir);
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("Failed to write state"));
}

#[test]
fn create_state_create_dir_all_failure_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    // The "parent" for state_dir is a regular file — create_dir_all
    // cannot descend into it, returning Err.
    let blocker = dir.path().join("blocker");
    fs::write(&blocker, "not a directory").unwrap();
    let state_dir = blocker.join(".flow-states");

    let result = create_state(&sample_queue(), &state_dir);
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("Failed to create directory"));
}

// --- start_issue ---

#[test]
fn test_start_issue() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    create_state(&sample_queue(), &state_dir);

    let state_path = state_dir.join("orchestrate.json");
    let result = start_issue(&state_path, 0);
    assert_eq!(result["status"], "ok");

    let content = fs::read_to_string(&state_path).unwrap();
    let state: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(state["current_index"], 0);
    assert_eq!(state["queue"][0]["status"], "in_progress");
    assert!(state["queue"][0]["started_at"]
        .as_str()
        .unwrap()
        .contains("T"));
}

#[test]
fn test_start_issue_out_of_range() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    create_state(&sample_queue(), &state_dir);

    let state_path = state_dir.join("orchestrate.json");
    let result = start_issue(&state_path, 10);
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("out of range"));
}

#[test]
fn test_start_issue_missing_state() {
    let dir = tempfile::tempdir().unwrap();
    let result = start_issue(&dir.path().join("missing.json"), 0);
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("not found"));
}

#[test]
fn test_start_issue_non_object_state() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("orchestrate.json");
    fs::write(&state_path, "[1, 2, 3]").unwrap();

    let result = start_issue(&state_path, 0);
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("not a JSON object"));
}

#[test]
fn start_issue_negative_index_returns_out_of_range() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    create_state(&sample_queue(), &state_dir);
    let state_path = state_dir.join("orchestrate.json");

    let result = start_issue(&state_path, -1);
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("out of range"));
}

#[test]
fn start_issue_queue_item_non_object_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("orchestrate.json");
    // Valid object root with a non-object queue entry — bypasses the
    // root-level guard and hits the per-item guard.
    fs::write(
        &state_path,
        serde_json::to_string_pretty(&json!({
            "started_at": "2026-03-20T22:00:00-07:00",
            "completed_at": null,
            "queue": [42],
            "current_index": null,
        }))
        .unwrap(),
    )
    .unwrap();

    let result = start_issue(&state_path, 0);
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("Queue item is not a JSON object"));
}

#[test]
fn start_issue_mutate_state_io_error() {
    let dir = tempfile::tempdir().unwrap();
    // state_path points to a directory — Path::exists() returns true
    // (short-circuits the "not found" early-return), then mutate_state's
    // OpenOptions::open fails with EISDIR.
    let state_path = dir.path().join("orchestrate.json");
    fs::create_dir(&state_path).unwrap();

    let result = start_issue(&state_path, 0);
    assert_eq!(result["status"], "error");
    let message = result["message"].as_str().unwrap().to_string();
    assert!(
        message.contains("I/O error"),
        "expected I/O error, got: {}",
        message
    );
}

// --- record_outcome ---

#[test]
fn test_record_outcome_completed() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    create_state(&sample_queue(), &state_dir);

    let state_path = state_dir.join("orchestrate.json");
    start_issue(&state_path, 0);

    let result = record_outcome(
        &state_path,
        0,
        "completed",
        Some("https://github.com/test/test/pull/100"),
        Some("add-pdf-export"),
        None,
    );
    assert_eq!(result["status"], "ok");

    let content = fs::read_to_string(&state_path).unwrap();
    let state: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(state["queue"][0]["status"], "completed");
    assert_eq!(state["queue"][0]["outcome"], "completed");
    assert!(state["queue"][0]["completed_at"]
        .as_str()
        .unwrap()
        .contains("T"));
    assert_eq!(
        state["queue"][0]["pr_url"],
        "https://github.com/test/test/pull/100"
    );
    assert_eq!(state["queue"][0]["branch"], "add-pdf-export");
}

#[test]
fn test_record_outcome_failed() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    create_state(&sample_queue(), &state_dir);

    let state_path = state_dir.join("orchestrate.json");
    start_issue(&state_path, 1);

    let result = record_outcome(
        &state_path,
        1,
        "failed",
        None,
        None,
        Some("CI failed after 3 attempts"),
    );
    assert_eq!(result["status"], "ok");

    let content = fs::read_to_string(&state_path).unwrap();
    let state: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(state["queue"][1]["status"], "failed");
    assert_eq!(state["queue"][1]["outcome"], "failed");
    assert_eq!(state["queue"][1]["reason"], "CI failed after 3 attempts");
}

#[test]
fn test_record_outcome_out_of_range() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    create_state(&sample_queue(), &state_dir);

    let state_path = state_dir.join("orchestrate.json");
    let result = record_outcome(&state_path, 10, "completed", None, None, None);
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("out of range"));
}

#[test]
fn test_record_outcome_missing_state() {
    let dir = tempfile::tempdir().unwrap();
    let result = record_outcome(
        &dir.path().join("missing.json"),
        0,
        "completed",
        None,
        None,
        None,
    );
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("not found"));
}

#[test]
fn test_record_outcome_non_object_state() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("orchestrate.json");
    fs::write(&state_path, "[1, 2, 3]").unwrap();

    let result = record_outcome(&state_path, 0, "completed", None, None, None);
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("not a JSON object"));
}

#[test]
fn record_outcome_mutate_state_io_error() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("orchestrate.json");
    fs::create_dir(&state_path).unwrap();

    let result = record_outcome(&state_path, 0, "completed", None, None, None);
    assert_eq!(result["status"], "error");
    let msg = result["message"].as_str().unwrap().to_string();
    assert!(msg.contains("I/O error"), "got: {}", msg);
}

#[test]
fn record_outcome_negative_index_returns_out_of_range() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    create_state(&sample_queue(), &state_dir);
    let state_path = state_dir.join("orchestrate.json");

    let result = record_outcome(&state_path, -1, "completed", None, None, None);
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("out of range"));
}

#[test]
fn record_outcome_queue_item_non_object_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("orchestrate.json");
    fs::write(
        &state_path,
        serde_json::to_string_pretty(&json!({
            "started_at": "2026-03-20T22:00:00-07:00",
            "completed_at": null,
            "queue": ["not-an-object"],
            "current_index": null,
        }))
        .unwrap(),
    )
    .unwrap();

    let result = record_outcome(&state_path, 0, "completed", None, None, None);
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("Queue item is not a JSON object"));
}

#[test]
fn record_outcome_empty_pr_url_not_written() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    create_state(&sample_queue(), &state_dir);
    let state_path = state_dir.join("orchestrate.json");

    let result = record_outcome(&state_path, 0, "completed", Some(""), None, None);
    assert_eq!(result["status"], "ok");

    let content = fs::read_to_string(&state_path).unwrap();
    let state: Value = serde_json::from_str(&content).unwrap();
    assert!(state["queue"][0]["pr_url"].is_null());
}

#[test]
fn record_outcome_empty_branch_not_written() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    create_state(&sample_queue(), &state_dir);
    let state_path = state_dir.join("orchestrate.json");

    let result = record_outcome(&state_path, 0, "completed", None, Some(""), None);
    assert_eq!(result["status"], "ok");

    let content = fs::read_to_string(&state_path).unwrap();
    let state: Value = serde_json::from_str(&content).unwrap();
    assert!(state["queue"][0]["branch"].is_null());
}

#[test]
fn record_outcome_empty_reason_not_written() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    create_state(&sample_queue(), &state_dir);
    let state_path = state_dir.join("orchestrate.json");

    let result = record_outcome(&state_path, 0, "failed", None, None, Some(""));
    assert_eq!(result["status"], "ok");

    let content = fs::read_to_string(&state_path).unwrap();
    let state: Value = serde_json::from_str(&content).unwrap();
    assert!(state["queue"][0]["reason"].is_null());
}

// --- complete_orchestration ---

#[test]
fn test_complete() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    create_state(&sample_queue(), &state_dir);

    let state_path = state_dir.join("orchestrate.json");
    let result = complete_orchestration(&state_path);
    assert_eq!(result["status"], "ok");

    let content = fs::read_to_string(&state_path).unwrap();
    let state: Value = serde_json::from_str(&content).unwrap();
    assert!(state["completed_at"].as_str().unwrap().contains("T"));
}

#[test]
fn test_complete_missing_state() {
    let dir = tempfile::tempdir().unwrap();
    let result = complete_orchestration(&dir.path().join("missing.json"));
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("not found"));
}

#[test]
fn test_complete_non_object_state() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("orchestrate.json");
    fs::write(&state_path, "[1, 2, 3]").unwrap();

    let result = complete_orchestration(&state_path);
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("not a JSON object"));
}

#[test]
fn complete_orchestration_mutate_state_io_error() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("orchestrate.json");
    fs::create_dir(&state_path).unwrap();

    let result = complete_orchestration(&state_path);
    assert_eq!(result["status"], "error");
    let msg = result["message"].as_str().unwrap().to_string();
    assert!(msg.contains("I/O error"), "got: {}", msg);
}

// --- read_state ---

#[test]
fn test_read_state() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    create_state(&sample_queue(), &state_dir);

    let state_path = state_dir.join("orchestrate.json");
    let result = read_state(&state_path);
    assert_eq!(result["status"], "ok");
    assert!(result["state"]["started_at"]
        .as_str()
        .unwrap()
        .contains("T"));
    assert_eq!(result["state"]["queue"].as_array().unwrap().len(), 3);
}

#[test]
fn test_read_state_missing() {
    let dir = tempfile::tempdir().unwrap();
    let result = read_state(&dir.path().join("missing.json"));
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("not found"));
}

#[test]
fn read_state_corrupt_json_returns_invalid_json_error() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("orchestrate.json");
    fs::write(&state_path, "{bad json").unwrap();

    let result = read_state(&state_path);
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("Invalid JSON"));
}

#[test]
fn read_state_fs_read_error_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("orchestrate.json");
    fs::create_dir(&state_path).unwrap();

    let result = read_state(&state_path);
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("Failed to read"));
}

// --- next_issue ---

#[test]
fn test_next_issue() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    create_state(&sample_queue(), &state_dir);

    let state_path = state_dir.join("orchestrate.json");
    let result = next_issue(&state_path);
    assert_eq!(result["status"], "ok");
    assert_eq!(result["index"], 0);
    assert_eq!(result["issue_number"], 42);
    assert_eq!(result["title"], "Add PDF export");
}

#[test]
fn test_next_issue_skips_completed() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    create_state(&sample_queue(), &state_dir);

    let state_path = state_dir.join("orchestrate.json");
    start_issue(&state_path, 0);
    record_outcome(&state_path, 0, "completed", None, None, None);

    let result = next_issue(&state_path);
    assert_eq!(result["status"], "ok");
    assert_eq!(result["index"], 1);
    assert_eq!(result["issue_number"], 43);
}

#[test]
fn test_next_issue_all_done() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    create_state(
        &[json!({"issue_number": 42, "title": "One issue"})],
        &state_dir,
    );

    let state_path = state_dir.join("orchestrate.json");
    start_issue(&state_path, 0);
    record_outcome(&state_path, 0, "completed", None, None, None);

    let result = next_issue(&state_path);
    assert_eq!(result["status"], "done");
}

#[test]
fn test_next_issue_missing_state() {
    let dir = tempfile::tempdir().unwrap();
    let result = next_issue(&dir.path().join("missing.json"));
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("not found"));
}

#[test]
fn next_issue_corrupt_json_returns_invalid_json_error() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("orchestrate.json");
    fs::write(&state_path, "{bad json").unwrap();

    let result = next_issue(&state_path);
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("Invalid JSON"));
}

#[test]
fn next_issue_no_queue_key_returns_done() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("orchestrate.json");
    fs::write(&state_path, "{}").unwrap();

    let result = next_issue(&state_path);
    assert_eq!(result["status"], "done");
}

#[test]
fn next_issue_fs_read_error_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("orchestrate.json");
    fs::create_dir(&state_path).unwrap();

    let result = next_issue(&state_path);
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("Failed to read"));
}

// --- run_impl ---

#[test]
fn test_cli_create() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();

    let queue_file = dir.path().join("queue.json");
    fs::write(&queue_file, serde_json::to_string(&sample_queue()).unwrap()).unwrap();

    let args = Args {
        create: true,
        queue_file: Some(queue_file.to_string_lossy().to_string()),
        state_dir: Some(state_dir.to_string_lossy().to_string()),
        ..default_args()
    };

    let result = run_impl(&args).unwrap();
    assert_eq!(result["status"], "ok");
    assert!(state_dir.join("orchestrate.json").exists());
}

#[test]
fn test_cli_create_missing_queue_file() {
    let args = Args {
        create: true,
        ..default_args()
    };

    let result = run_impl(&args).unwrap();
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("--queue-file"));
}

#[test]
fn test_cli_start_issue() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    create_state(&sample_queue(), &state_dir);

    let state_path = state_dir.join("orchestrate.json");
    let args = Args {
        start_issue: Some(0),
        state_file: Some(state_path.to_string_lossy().to_string()),
        ..default_args()
    };

    let result = run_impl(&args).unwrap();
    assert_eq!(result["status"], "ok");
}

#[test]
fn test_cli_start_issue_missing_state_file() {
    let args = Args {
        start_issue: Some(0),
        ..default_args()
    };

    let result = run_impl(&args).unwrap();
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("--state-file"));
}

#[test]
fn test_cli_record_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    create_state(&sample_queue(), &state_dir);

    let state_path = state_dir.join("orchestrate.json");
    start_issue(&state_path, 0);

    let args = Args {
        record_outcome: Some(0),
        state_file: Some(state_path.to_string_lossy().to_string()),
        outcome: Some("completed".to_string()),
        pr_url: Some("https://github.com/test/test/pull/100".to_string()),
        branch: Some("add-pdf-export".to_string()),
        ..default_args()
    };

    let result = run_impl(&args).unwrap();
    assert_eq!(result["status"], "ok");
}

#[test]
fn test_cli_record_outcome_missing_state_file() {
    let args = Args {
        record_outcome: Some(0),
        outcome: Some("completed".to_string()),
        ..default_args()
    };

    let result = run_impl(&args).unwrap();
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("--state-file"));
}

#[test]
fn test_cli_record_outcome_missing_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    create_state(&sample_queue(), &state_dir);

    let state_path = state_dir.join("orchestrate.json");

    let args = Args {
        record_outcome: Some(0),
        state_file: Some(state_path.to_string_lossy().to_string()),
        ..default_args()
    };

    let result = run_impl(&args).unwrap();
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("--outcome"));
}

#[test]
fn test_cli_complete() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    create_state(&sample_queue(), &state_dir);

    let state_path = state_dir.join("orchestrate.json");
    let args = Args {
        complete: true,
        state_file: Some(state_path.to_string_lossy().to_string()),
        ..default_args()
    };

    let result = run_impl(&args).unwrap();
    assert_eq!(result["status"], "ok");
}

#[test]
fn test_cli_complete_missing_state_file() {
    let args = Args {
        complete: true,
        ..default_args()
    };

    let result = run_impl(&args).unwrap();
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("--state-file"));
}

#[test]
fn test_cli_read() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    create_state(&sample_queue(), &state_dir);

    let state_path = state_dir.join("orchestrate.json");
    let args = Args {
        read: true,
        state_file: Some(state_path.to_string_lossy().to_string()),
        ..default_args()
    };

    let result = run_impl(&args).unwrap();
    assert_eq!(result["status"], "ok");
    assert!(result.get("state").is_some());
}

#[test]
fn test_cli_read_missing_state_file() {
    let args = Args {
        read: true,
        ..default_args()
    };

    let result = run_impl(&args).unwrap();
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("--state-file"));
}

#[test]
fn test_cli_next() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    create_state(&sample_queue(), &state_dir);

    let state_path = state_dir.join("orchestrate.json");
    let args = Args {
        next: true,
        state_file: Some(state_path.to_string_lossy().to_string()),
        ..default_args()
    };

    let result = run_impl(&args).unwrap();
    assert_eq!(result["status"], "ok");
    assert_eq!(result["index"], 0);
}

#[test]
fn test_cli_next_missing_state_file() {
    let args = Args {
        next: true,
        ..default_args()
    };

    let result = run_impl(&args).unwrap();
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("--state-file"));
}

#[test]
fn test_cli_read_missing_state() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        read: true,
        state_file: Some(
            dir.path()
                .join("missing.json")
                .to_string_lossy()
                .to_string(),
        ),
        ..default_args()
    };

    let result = run_impl(&args).unwrap();
    assert_eq!(result["status"], "error");
}

#[test]
fn test_cli_exception_handling() {
    let dir = tempfile::tempdir().unwrap();
    let bad_file = dir.path().join("bad.json");
    fs::write(&bad_file, "{corrupt json").unwrap();

    let args = Args {
        start_issue: Some(0),
        state_file: Some(bad_file.to_string_lossy().to_string()),
        ..default_args()
    };

    let result = run_impl(&args).unwrap();
    assert_eq!(result["status"], "error");
}

#[test]
fn run_impl_no_action_returns_no_action_error() {
    let args = default_args();
    let value = run_impl(&args).unwrap();
    assert_eq!(value["status"], "error");
    assert_eq!(value["message"], "No action specified");
}

#[test]
fn run_impl_create_missing_queue_file_returns_err() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        create: true,
        queue_file: Some(
            dir.path()
                .join("nonexistent.json")
                .to_string_lossy()
                .to_string(),
        ),
        state_dir: Some(dir.path().to_string_lossy().to_string()),
        ..default_args()
    };

    let result = run_impl(&args);
    let err = result.unwrap_err();
    assert!(
        err.contains("Failed to read queue file"),
        "expected read-failure err, got: {}",
        err
    );
}

#[test]
fn run_impl_create_malformed_queue_json_returns_err() {
    let dir = tempfile::tempdir().unwrap();
    let queue_file = dir.path().join("queue.json");
    fs::write(&queue_file, "{bad json").unwrap();

    let args = Args {
        create: true,
        queue_file: Some(queue_file.to_string_lossy().to_string()),
        state_dir: Some(dir.path().to_string_lossy().to_string()),
        ..default_args()
    };

    let result = run_impl(&args);
    let err = result.unwrap_err();
    assert!(
        err.contains("Invalid queue JSON"),
        "expected parse-failure err, got: {}",
        err
    );
}

// --- run_impl_main ---

// Exercises the `Ok` branch of `run_impl_main`: a successful `run_impl`
// result is wrapped as `(value, 0)`.
#[test]
fn run_impl_main_ok_branch_returns_value_and_exit_zero() {
    let args = default_args();
    let (value, code) = run_impl_main(&args);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["message"], "No action specified");
}

// Exercises the `Err` branch of `run_impl_main`: a `run_impl` Err(msg)
// is mapped to `json!({"status": "error", "message": msg})` and paired
// with exit code 0 (business errors use JSON-status, never exit).
#[test]
fn run_impl_main_err_branch_wraps_message() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        create: true,
        queue_file: Some(
            dir.path()
                .join("nonexistent.json")
                .to_string_lossy()
                .to_string(),
        ),
        state_dir: Some(dir.path().to_string_lossy().to_string()),
        ..default_args()
    };

    let (value, code) = run_impl_main(&args);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Failed to read queue file"));
}
