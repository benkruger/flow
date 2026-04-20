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
/// fails to parse. The 0 sentinel lets the morning report render as
/// "0s" instead of failing the whole orchestration summary on a
/// malformed timestamp from a partially-written queue entry.
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

    let items: &[Value] = match queue {
        Some(a) => a.as_slice(),
        None => &[],
    };

    // Partition items into completed/failed in a single pass so coverage
    // scopes cleanly to the match arms rather than four closure bodies.
    let mut completed_items: Vec<&Value> = Vec::new();
    let mut failed_items: Vec<&Value> = Vec::new();
    for item in items.iter() {
        match item.get("outcome").and_then(|v| v.as_str()) {
            Some("completed") => completed_items.push(item),
            Some("failed") => failed_items.push(item),
            _ => {}
        }
    }

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
/// The morning-report flow keeps running even when one orchestration
/// queue file is malformed: missing state files and corrupt JSON are
/// surfaced as `{"status":"error", ...}` values, not as failures.
/// There is no infrastructure-error path to distinguish, so the
/// return type is `Value`, not `Result`.
pub fn run_impl(args: &Args) -> Value {
    generate_and_write_report(Path::new(&args.state_file), Path::new(&args.output_dir))
}
