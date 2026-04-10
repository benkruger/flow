//! Generate morning report from orchestration state.
//!
//! Reads `.flow-states/orchestrate.json`
//! and produces a markdown summary with results table, completed/failed sections,
//! and timing information. Writes `orchestrate-summary.md` to the output directory.

use std::path::Path;

use chrono::DateTime;
use clap::Parser;
use serde_json::{json, Value};

use crate::utils::format_time;

/// Compute duration in seconds between two ISO 8601 timestamps.
///
/// Returns 0 when `completed_at` is None, empty, or either timestamp
/// fails to parse — matching the Python original's graceful fallback.
pub fn compute_duration_seconds(started_at: &str, completed_at: Option<&str>) -> i64 {
    let completed = match completed_at {
        Some(s) if !s.is_empty() => s,
        _ => return 0,
    };

    let start = match DateTime::parse_from_rfc3339(started_at) {
        Ok(dt) => dt,
        Err(_) => return 0,
    };
    let end = match DateTime::parse_from_rfc3339(completed) {
        Ok(dt) => dt,
        Err(_) => return 0,
    };

    let diff = (end - start).num_seconds();
    if diff < 0 {
        0
    } else {
        diff
    }
}

/// Generate a morning report from orchestrate state dict.
///
/// Pure function — takes the parsed state JSON and returns a Value with
/// `summary` (markdown string), `completed` count, `failed` count, and `total`.
pub fn generate_report(state: &Value) -> Value {
    let queue = state.get("queue").and_then(|v| v.as_array());
    let started_at = state
        .get("started_at")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let completed_at = state.get("completed_at").and_then(|v| v.as_str());

    let items = queue.map(|a| a.as_slice()).unwrap_or(&[]);

    let completed_items: Vec<&Value> = items
        .iter()
        .filter(|item| item.get("outcome").and_then(|v| v.as_str()) == Some("completed"))
        .collect();
    let failed_items: Vec<&Value> = items
        .iter()
        .filter(|item| item.get("outcome").and_then(|v| v.as_str()) == Some("failed"))
        .collect();

    let duration_seconds = compute_duration_seconds(started_at, completed_at);
    let duration_str = format_time(duration_seconds);

    let mut lines: Vec<String> = Vec::new();
    lines.push("# FLOW Orchestration Report".to_string());
    lines.push(String::new());
    lines.push(format!("Started: {}", started_at));
    lines.push(format!("Completed: {}", completed_at.unwrap_or("")));
    lines.push(format!("Duration: {}", duration_str));
    lines.push(String::new());

    if !items.is_empty() {
        lines.push("## Results".to_string());
        lines.push(String::new());
        lines.push("| # | Issue | Outcome | PR |".to_string());
        lines.push("|---|-------|---------|-----|".to_string());
        for (i, item) in items.iter().enumerate() {
            let issue_num = item
                .get("issue_number")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "?".to_string());
            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let outcome = item
                .get("outcome")
                .and_then(|v| v.as_str())
                .unwrap_or("pending");
            let pr_url = item.get("pr_url").and_then(|v| v.as_str());
            let pr_display = pr_url.unwrap_or("\u{2014}");
            lines.push(format!(
                "| {} | #{} {} | {} | {} |",
                i + 1,
                issue_num,
                title,
                outcome,
                pr_display
            ));
        }
        lines.push(String::new());
    }

    if !completed_items.is_empty() {
        lines.push(format!("## Completed ({})", completed_items.len()));
        lines.push(String::new());
        for item in &completed_items {
            let issue_num = item
                .get("issue_number")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "?".to_string());
            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let pr_url = item.get("pr_url").and_then(|v| v.as_str()).unwrap_or("");
            lines.push(format!("- #{} {} \u{2014} {}", issue_num, title, pr_url));
        }
        lines.push(String::new());
    }

    if !failed_items.is_empty() {
        lines.push(format!("## Failed ({})", failed_items.len()));
        lines.push(String::new());
        for item in &failed_items {
            let issue_num = item
                .get("issue_number")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "?".to_string());
            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let reason = item
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            lines.push(format!("- #{} {} \u{2014} {}", issue_num, title, reason));
        }
        lines.push(String::new());
    }

    let summary = lines.join("\n");

    json!({
        "summary": summary,
        "completed": completed_items.len(),
        "failed": failed_items.len(),
        "total": items.len(),
    })
}

/// Read state file, generate report, write summary file.
///
/// Returns `{"status": "ok", ...report_fields}` on success.
/// Returns `{"status": "error", "message": "..."}` on failure.
pub fn generate_and_write_report(state_file: &Path, output_dir: &Path) -> Value {
    if !state_file.exists() {
        return json!({
            "status": "error",
            "message": format!("State file not found: {}", state_file.display())
        });
    }

    let content = match std::fs::read_to_string(state_file) {
        Ok(c) => c,
        Err(e) => {
            return json!({
                "status": "error",
                "message": format!("Failed to read state file: {}", e)
            })
        }
    };

    let state: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            return json!({
                "status": "error",
                "message": format!("Invalid JSON: {}", e)
            })
        }
    };

    let report = generate_report(&state);

    let summary = report.get("summary").and_then(|v| v.as_str()).unwrap_or("");
    let output_path = output_dir.join("orchestrate-summary.md");
    if let Err(e) = std::fs::write(&output_path, summary) {
        return json!({
            "status": "error",
            "message": format!("Failed to write summary: {}", e)
        });
    }

    json!({
        "status": "ok",
        "summary": report["summary"],
        "completed": report["completed"],
        "failed": report["failed"],
        "total": report["total"],
    })
}

// --- CLI ---

#[derive(Parser, Debug)]
#[command(
    name = "orchestrate-report",
    about = "Generate orchestration morning report"
)]
pub struct Args {
    /// Path to orchestrate.json
    #[arg(long)]
    pub state_file: String,

    /// Path to output directory
    #[arg(long)]
    pub output_dir: String,
}

/// Testable implementation — returns the JSON Value to print.
///
/// Returns `Ok(value)` for both success and application-error responses
/// (matching Python's always-exit-0 behavior). Returns `Err(msg)` only
/// for infrastructure failures.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    Ok(generate_and_write_report(
        Path::new(&args.state_file),
        Path::new(&args.output_dir),
    ))
}

pub fn run(args: Args) {
    match run_impl(&args) {
        Ok(value) => {
            println!("{}", value);
        }
        Err(msg) => {
            println!("{}", json!({"status": "error", "message": msg}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
    fn test_compute_duration_valid() {
        let secs = compute_duration_seconds(
            "2026-03-20T22:00:00-07:00",
            Some("2026-03-21T06:00:00-07:00"),
        );
        assert_eq!(secs, 28800); // 8 hours
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

        let result = run_impl(&args).unwrap();
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

        let result = run_impl(&args).unwrap();
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

        let result = run_impl(&args).unwrap();
        assert_eq!(result["status"], "error");
    }
}
