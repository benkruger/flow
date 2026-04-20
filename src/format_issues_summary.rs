use std::path::Path;

use clap::Parser;
use indexmap::IndexMap;
use serde_json::{json, Value};

use crate::utils::short_issue_ref;

#[derive(Debug, PartialEq)]
pub struct SummaryResult {
    pub has_issues: bool,
    pub banner_line: String,
    pub table: String,
}

/// Build issues summary from state dict.
///
/// Returns SummaryResult with has_issues, banner_line, and table.
pub fn format_issues_summary(state: &serde_json::Value) -> SummaryResult {
    let issues = match state.get("issues_filed").and_then(|v| v.as_array()) {
        Some(arr) if !arr.is_empty() => arr,
        _ => {
            return SummaryResult {
                has_issues: false,
                banner_line: String::new(),
                table: String::new(),
            };
        }
    };

    // Build label counts in encounter order using IndexMap
    let mut label_counts: IndexMap<String, usize> = IndexMap::new();
    for issue in issues {
        let label = issue.get("label").and_then(|v| v.as_str()).unwrap_or("");
        *label_counts.entry(label.to_string()).or_insert(0) += 1;
    }

    let total = issues.len();
    let parts: Vec<String> = label_counts
        .iter()
        .map(|(label, count)| format!("{}: {}", label, count))
        .collect();
    let banner_line = format!("Issues filed: {} ({})", total, parts.join(", "));

    // Build markdown table
    let mut lines = vec![
        "| Label | Title | Phase | URL |".to_string(),
        "|-------|-------|-------|-----|".to_string(),
    ];
    for issue in issues {
        let label = issue.get("label").and_then(|v| v.as_str()).unwrap_or("");
        let title = issue.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let phase = issue
            .get("phase_name")
            .and_then(|v| v.as_str())
            .or_else(|| issue.get("phase").and_then(|v| v.as_str()))
            .unwrap_or("");
        let url = issue.get("url").and_then(|v| v.as_str()).unwrap_or("");
        let short_url = short_issue_ref(url);
        lines.push(format!(
            "| {} | {} | {} | {} |",
            label, title, phase, short_url
        ));
    }

    let table = lines.join("\n");

    SummaryResult {
        has_issues: true,
        banner_line,
        table,
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "format-issues-summary",
    about = "Format issues summary for Complete phase"
)]
pub struct Args {
    /// Path to state JSON file
    #[arg(long)]
    pub state_file: String,

    /// Path to write markdown table
    #[arg(long)]
    pub output: String,
}

/// Fallible CLI logic — returns the SummaryResult on success or an
/// error message. `run_impl_main` wraps this into the `(Value, i32)`
/// contract that `dispatch::dispatch_json` consumes; unit tests call
/// `run_impl` directly to assert on typed results.
pub fn run_impl(args: &Args) -> Result<SummaryResult, String> {
    let state_path = Path::new(&args.state_file);
    if !state_path.exists() {
        return Err(format!("State file not found: {}", args.state_file));
    }

    let content = std::fs::read_to_string(state_path)
        .map_err(|e| format!("Failed to read state file: {}", e))?;

    let state: Value =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse state file: {}", e))?;

    let result = format_issues_summary(&state);

    if result.has_issues {
        let output_path = Path::new(&args.output);
        if let Some(parent) = output_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(output_path, &result.table)
            .map_err(|e| format!("Failed to write output: {}", e))?;
    }

    Ok(result)
}

/// Main-arm entry point: wraps `run_impl` into the `(Value, i32)`
/// contract consumed by `dispatch::dispatch_json`.
pub fn run_impl_main(args: &Args) -> (Value, i32) {
    match run_impl(args) {
        Ok(result) => (
            json!({
                "status": "ok",
                "has_issues": result.has_issues,
                "banner_line": result.banner_line,
                "table": result.table,
            }),
            0,
        ),
        Err(msg) => (
            json!({
                "status": "error",
                "message": msg,
            }),
            1,
        ),
    }
}
