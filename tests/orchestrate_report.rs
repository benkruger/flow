//! Integration tests for `src/orchestrate_report.rs`.

use std::fs;
use std::process::Command;

use flow_rs::orchestrate_report::{
    compute_duration_seconds, generate_and_write_report, generate_report, run_impl, Args,
};
use serde_json::{json, Value};

fn completed_item(issue_number: i64, title: &str, pr_url: Option<&str>) -> Value {
    json!({
        "issue_number": issue_number,
        "title": title,
        "status": "completed",
        "started_at": "2026-03-20T22:05:00-07:00",
        "completed_at": "2026-03-20T23:00:00-07:00",
        "outcome": "completed",
        "pr_url": pr_url.unwrap_or(&format!("https://github.com/test/test/pull/{}", issue_number)),
        "branch": format!("issue-{}", issue_number),
        "reason": null,
    })
}

fn failed_item(issue_number: i64, title: &str, reason: &str) -> Value {
    json!({
        "issue_number": issue_number,
        "title": title,
        "status": "failed",
        "started_at": "2026-03-20T22:05:00-07:00",
        "completed_at": "2026-03-20T22:30:00-07:00",
        "outcome": "failed",
        "pr_url": null,
        "branch": null,
        "reason": reason,
    })
}

fn make_report_state(
    queue_items: Vec<Value>,
    started_at: &str,
    completed_at: Option<&str>,
) -> Value {
    json!({
        "started_at": started_at,
        "completed_at": completed_at,
        "queue": queue_items,
        "current_index": null,
    })
}

// --- compute_duration_seconds ---

#[test]
fn test_compute_duration_none_completed_at() {
    assert_eq!(
        compute_duration_seconds("2026-03-20T22:00:00-07:00", None),
        0
    );
}

#[test]
fn test_compute_duration_bad_timestamps() {
    assert_eq!(
        compute_duration_seconds("not-a-timestamp", Some("also-not-a-timestamp")),
        0
    );
}

#[test]
fn test_compute_duration_valid_started_bad_completed_returns_zero() {
    assert_eq!(
        compute_duration_seconds("2026-03-20T22:00:00-07:00", Some("not-a-date")),
        0
    );
}

#[test]
fn test_compute_duration_negative_diff_clamped_to_zero() {
    let secs = compute_duration_seconds(
        "2026-03-21T06:00:00-07:00",
        Some("2026-03-20T22:00:00-07:00"),
    );
    assert_eq!(secs, 0);
}

#[test]
fn test_compute_duration_valid() {
    let secs = compute_duration_seconds(
        "2026-03-20T22:00:00-07:00",
        Some("2026-03-21T06:00:00-07:00"),
    );
    assert_eq!(secs, 28800);
}

// --- generate_report ---

#[test]
fn test_report_all_completed() {
    let state = make_report_state(
        vec![
            completed_item(42, "Add PDF export", None),
            completed_item(43, "Fix login timeout", None),
        ],
        "2026-03-20T22:00:00-07:00",
        Some("2026-03-21T06:00:00-07:00"),
    );

    let result = generate_report(&state);
    assert_eq!(result["completed"], 2);
    assert_eq!(result["failed"], 0);
    assert_eq!(result["total"], 2);
    let summary = result["summary"].as_str().unwrap();
    assert!(summary.contains("#42"));
    assert!(summary.contains("#43"));
    assert!(summary.to_lowercase().contains("completed"));
}

#[test]
fn test_report_mixed_results() {
    let state = make_report_state(
        vec![
            completed_item(42, "Add PDF export", None),
            failed_item(43, "Fix login timeout", "CI failed after 3 attempts"),
        ],
        "2026-03-20T22:00:00-07:00",
        Some("2026-03-21T06:00:00-07:00"),
    );

    let result = generate_report(&state);
    assert_eq!(result["completed"], 1);
    assert_eq!(result["failed"], 1);
    assert_eq!(result["total"], 2);
    let summary = result["summary"].as_str().unwrap();
    assert!(summary.contains("#42"));
    assert!(summary.contains("#43"));
}

#[test]
fn test_report_all_failed() {
    let state = make_report_state(
        vec![
            failed_item(42, "Add PDF export", "CI failed"),
            failed_item(43, "Fix login timeout", "CI failed"),
        ],
        "2026-03-20T22:00:00-07:00",
        Some("2026-03-21T06:00:00-07:00"),
    );

    let result = generate_report(&state);
    assert_eq!(result["completed"], 0);
    assert_eq!(result["failed"], 2);
    assert_eq!(result["total"], 2);
}

#[test]
fn test_report_empty_queue() {
    let state = make_report_state(
        vec![],
        "2026-03-20T22:00:00-07:00",
        Some("2026-03-21T06:00:00-07:00"),
    );

    let result = generate_report(&state);
    assert_eq!(result["completed"], 0);
    assert_eq!(result["failed"], 0);
    assert_eq!(result["total"], 0);
}

#[test]
fn test_report_single_issue() {
    let state = make_report_state(
        vec![completed_item(42, "Add PDF export", None)],
        "2026-03-20T22:00:00-07:00",
        Some("2026-03-21T06:00:00-07:00"),
    );

    let result = generate_report(&state);
    assert_eq!(result["completed"], 1);
    assert_eq!(result["total"], 1);
    let summary = result["summary"].as_str().unwrap();
    assert!(summary.contains("#42"));
    assert!(summary.contains("Add PDF export"));
}

#[test]
fn test_report_includes_timing() {
    let state = make_report_state(
        vec![completed_item(42, "Add PDF export", None)],
        "2026-03-20T22:00:00-07:00",
        Some("2026-03-21T06:00:00-07:00"),
    );

    let result = generate_report(&state);
    let summary = result["summary"].as_str().unwrap();
    assert!(summary.contains("8h"));
}

#[test]
fn test_report_includes_pr_urls() {
    let state = make_report_state(
        vec![completed_item(
            42,
            "Add PDF export",
            Some("https://github.com/test/test/pull/100"),
        )],
        "2026-03-20T22:00:00-07:00",
        Some("2026-03-21T06:00:00-07:00"),
    );

    let result = generate_report(&state);
    let summary = result["summary"].as_str().unwrap();
    assert!(summary.contains("https://github.com/test/test/pull/100"));
}

#[test]
fn test_report_includes_failure_reasons() {
    let state = make_report_state(
        vec![failed_item(
            43,
            "Fix login timeout",
            "CI failed after 3 attempts",
        )],
        "2026-03-20T22:00:00-07:00",
        Some("2026-03-21T06:00:00-07:00"),
    );

    let result = generate_report(&state);
    let summary = result["summary"].as_str().unwrap();
    assert!(summary.contains("CI failed after 3 attempts"));
}

#[test]
fn test_report_none_completed_at() {
    let state = make_report_state(
        vec![completed_item(42, "Add PDF export", None)],
        "2026-03-20T22:00:00-07:00",
        None,
    );

    let result = generate_report(&state);
    let summary = result["summary"].as_str().unwrap();
    assert!(summary.contains("<1m"));
}

#[test]
fn test_report_bad_timestamps() {
    let state = make_report_state(
        vec![completed_item(42, "Add PDF export", None)],
        "not-a-timestamp",
        Some("also-not-a-timestamp"),
    );

    let result = generate_report(&state);
    let summary = result["summary"].as_str().unwrap();
    assert!(summary.contains("<1m"));
}

#[test]
fn test_report_results_table_format() {
    let state = make_report_state(
        vec![
            completed_item(42, "Add PDF export", None),
            failed_item(43, "Fix login", "CI failed"),
        ],
        "2026-03-20T22:00:00-07:00",
        Some("2026-03-21T06:00:00-07:00"),
    );

    let result = generate_report(&state);
    let summary = result["summary"].as_str().unwrap();
    assert!(summary.contains("| #"));
    assert!(summary.contains("Issue"));
    assert!(summary.contains("Outcome"));
}

#[test]
fn test_report_writes_summary_file() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_report_state(
        vec![completed_item(42, "Add PDF export", None)],
        "2026-03-20T22:00:00-07:00",
        Some("2026-03-21T06:00:00-07:00"),
    );
    let state_path = dir.path().join("orchestrate.json");
    fs::write(&state_path, serde_json::to_string(&state).unwrap()).unwrap();

    let result = generate_and_write_report(&state_path, dir.path());
    assert_eq!(result["status"], "ok");

    let summary_path = dir.path().join("orchestrate-summary.md");
    assert!(summary_path.exists());
    let content = fs::read_to_string(summary_path).unwrap();
    assert!(content.contains("#42"));
    assert!(content.contains("Add PDF export"));
}

/// Exercises the `state.get("queue") == None` branch: state without
/// any queue key renders an empty report.
#[test]
fn test_report_missing_queue_key() {
    let state = json!({
        "started_at": "2026-03-20T22:00:00-07:00",
        "completed_at": "2026-03-21T06:00:00-07:00",
    });
    let result = generate_report(&state);
    assert_eq!(result["completed"], 0);
    assert_eq!(result["failed"], 0);
    assert_eq!(result["total"], 0);
}

/// Exercises the `queue` key present but not an array branch.
#[test]
fn test_report_queue_not_array() {
    let state = json!({
        "started_at": "2026-03-20T22:00:00-07:00",
        "completed_at": "2026-03-21T06:00:00-07:00",
        "queue": "not-an-array",
    });
    let result = generate_report(&state);
    assert_eq!(result["total"], 0);
}

// --- missing-field edge branches ---

#[test]
fn report_missing_issue_number_renders_question_mark() {
    let state = make_report_state(
        vec![json!({
            "title": "No issue number",
            "outcome": "completed",
            "pr_url": "https://github.com/x/y/pull/1",
        })],
        "2026-03-20T22:00:00-07:00",
        Some("2026-03-21T06:00:00-07:00"),
    );
    let result = generate_report(&state);
    let summary = result["summary"].as_str().unwrap();
    assert!(summary.contains("| #? "));
}

#[test]
fn report_missing_title_renders_empty_title() {
    let state = make_report_state(
        vec![json!({
            "issue_number": 99,
            "outcome": "completed",
            "pr_url": "https://github.com/x/y/pull/1",
        })],
        "2026-03-20T22:00:00-07:00",
        Some("2026-03-21T06:00:00-07:00"),
    );
    let result = generate_report(&state);
    let summary = result["summary"].as_str().unwrap();
    assert!(summary.contains("| #99  | completed |"));
}

#[test]
fn report_missing_outcome_renders_pending() {
    let state = make_report_state(
        vec![json!({
            "issue_number": 99,
            "title": "Pending item",
        })],
        "2026-03-20T22:00:00-07:00",
        Some("2026-03-21T06:00:00-07:00"),
    );
    let result = generate_report(&state);
    let summary = result["summary"].as_str().unwrap();
    assert!(summary.contains("| pending |"));
    assert_eq!(result["completed"], 0);
    assert_eq!(result["failed"], 0);
}

#[test]
fn report_missing_pr_url_renders_em_dash() {
    let state = make_report_state(
        vec![json!({
            "issue_number": 99,
            "title": "No PR yet",
            "outcome": "completed",
        })],
        "2026-03-20T22:00:00-07:00",
        Some("2026-03-21T06:00:00-07:00"),
    );
    let result = generate_report(&state);
    let summary = result["summary"].as_str().unwrap();
    assert!(summary.contains("| \u{2014} |"));
}

#[test]
fn report_failed_item_missing_issue_number_renders_question_mark() {
    let state = make_report_state(
        vec![json!({
            "title": "No number on failed item",
            "outcome": "failed",
            "reason": "boom",
        })],
        "2026-03-20T22:00:00-07:00",
        Some("2026-03-21T06:00:00-07:00"),
    );
    let result = generate_report(&state);
    let summary = result["summary"].as_str().unwrap();
    // Failed section renders `- #? No number on failed item — boom`
    assert!(summary.contains("- #? No number on failed item"));
}

#[test]
fn report_failed_item_missing_reason_renders_unknown() {
    let state = make_report_state(
        vec![json!({
            "issue_number": 42,
            "title": "Broken item",
            "outcome": "failed",
        })],
        "2026-03-20T22:00:00-07:00",
        Some("2026-03-21T06:00:00-07:00"),
    );
    let result = generate_report(&state);
    let summary = result["summary"].as_str().unwrap();
    assert!(summary.contains("\u{2014} Unknown"));
}

// --- generate_and_write_report I/O error branches ---

#[test]
fn generate_and_write_report_read_failure_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("orchestrate.json");
    fs::create_dir(&state_path).unwrap();
    let result = generate_and_write_report(&state_path, dir.path());
    assert_eq!(result["status"], "error");
    let message = result["message"].as_str().unwrap().to_string();
    assert!(message.contains("Failed to read state file"));
}

#[test]
fn generate_and_write_report_write_failure_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_report_state(
        vec![completed_item(42, "Add PDF export", None)],
        "2026-03-20T22:00:00-07:00",
        Some("2026-03-21T06:00:00-07:00"),
    );
    let state_path = dir.path().join("orchestrate.json");
    fs::write(&state_path, serde_json::to_string(&state).unwrap()).unwrap();

    let summary_path = dir.path().join("orchestrate-summary.md");
    fs::create_dir(&summary_path).unwrap();

    let result = generate_and_write_report(&state_path, dir.path());
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("Failed to write summary"));
}

// --- CLI run_impl tests ---

#[test]
fn test_cli_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_report_state(
        vec![
            completed_item(42, "Add PDF export", None),
            failed_item(43, "Fix login", "CI failed"),
        ],
        "2026-03-20T22:00:00-07:00",
        Some("2026-03-21T06:00:00-07:00"),
    );
    let state_path = dir.path().join("orchestrate.json");
    fs::write(&state_path, serde_json::to_string(&state).unwrap()).unwrap();

    let args = Args {
        state_file: state_path.to_string_lossy().to_string(),
        output_dir: dir.path().to_string_lossy().to_string(),
    };

    let result = run_impl(&args);
    assert_eq!(result["status"], "ok");
    assert_eq!(result["completed"], 1);
    assert_eq!(result["failed"], 1);
    assert!(dir.path().join("orchestrate-summary.md").exists());
}

#[test]
fn test_cli_missing_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        state_file: dir
            .path()
            .join("missing.json")
            .to_string_lossy()
            .to_string(),
        output_dir: dir.path().to_string_lossy().to_string(),
    };

    let result = run_impl(&args);
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("not found"));
}

#[test]
fn test_cli_corrupt_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let bad_file = dir.path().join("orchestrate.json");
    fs::write(&bad_file, "{bad json").unwrap();

    let args = Args {
        state_file: bad_file.to_string_lossy().to_string(),
        output_dir: dir.path().to_string_lossy().to_string(),
    };

    let result = run_impl(&args);
    assert_eq!(result["status"], "error");
}

#[test]
fn args_debug_impl_covered() {
    let args = Args {
        state_file: "x".into(),
        output_dir: "y".into(),
    };
    let s = format!("{:?}", args);
    assert!(s.contains("Args") || s.contains("state_file"));
}

/// Exercises clap-derived Parser implementations via real CLI parsing.
#[test]
fn cli_subprocess_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_report_state(
        vec![completed_item(42, "Add PDF export", None)],
        "2026-03-20T22:00:00-07:00",
        Some("2026-03-21T06:00:00-07:00"),
    );
    let state_path = dir.path().join("orchestrate.json");
    fs::write(&state_path, serde_json::to_string(&state).unwrap()).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "orchestrate-report",
            "--state-file",
            state_path.to_str().unwrap(),
            "--output-dir",
            dir.path().to_str().unwrap(),
        ])
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("spawn flow-rs");
    assert_eq!(output.status.code(), Some(0));
}
