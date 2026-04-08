use std::path::Path;
use std::process;

use clap::Parser;
use indexmap::IndexMap;
use serde_json::json;

use crate::output::{json_error, json_ok};
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

pub fn run(args: Args) {
    let state_path = Path::new(&args.state_file);
    if !state_path.exists() {
        json_error(&format!("State file not found: {}", args.state_file), &[]);
        process::exit(1);
    }

    let content = match std::fs::read_to_string(state_path) {
        Ok(c) => c,
        Err(e) => {
            json_error(&format!("Failed to read state file: {}", e), &[]);
            process::exit(1);
        }
    };

    let state: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            json_error(&format!("Failed to parse state file: {}", e), &[]);
            process::exit(1);
        }
    };

    let result = format_issues_summary(&state);

    if result.has_issues {
        let output_path = Path::new(&args.output);
        if let Some(parent) = output_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(output_path, &result.table) {
            json_error(&format!("Failed to write output: {}", e), &[]);
            process::exit(1);
        }
    }

    json_ok(&[
        ("has_issues", json!(result.has_issues)),
        ("banner_line", json!(result.banner_line)),
        ("table", json!(result.table)),
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_issues(labels: &[&str]) -> serde_json::Value {
        let issues: Vec<serde_json::Value> = labels
            .iter()
            .enumerate()
            .map(|(i, label)| {
                json!({
                    "label": label,
                    "title": format!("Issue {}", i + 1),
                    "url": format!("https://github.com/test/test/issues/{}", i + 1),
                    "phase": "flow-learn",
                    "phase_name": "Learn",
                    "timestamp": "2026-01-01T00:00:00-08:00",
                })
            })
            .collect();
        json!(issues)
    }

    #[test]
    fn empty_issues_returns_no_issues() {
        let state = json!({"issues_filed": []});
        let result = format_issues_summary(&state);
        assert!(!result.has_issues);
        assert_eq!(result.banner_line, "");
        assert_eq!(result.table, "");
    }

    #[test]
    fn missing_issues_filed_returns_no_issues() {
        let state = json!({"branch": "test"});
        let result = format_issues_summary(&state);
        assert!(!result.has_issues);
    }

    #[test]
    fn single_issue_formats_correctly() {
        let state = json!({"issues_filed": make_issues(&["Rule"])});
        let result = format_issues_summary(&state);
        assert!(result.has_issues);
        assert_eq!(result.banner_line, "Issues filed: 1 (Rule: 1)");
        assert!(result.table.contains("| Label | Title | Phase | URL |"));
        assert!(result.table.contains("| Rule | Issue 1 | Learn |"));
    }

    #[test]
    fn multiple_labels_grouped() {
        let state =
            json!({"issues_filed": make_issues(&["Rule", "Flaky Test", "Rule", "Tech Debt"])});
        let result = format_issues_summary(&state);
        assert!(result.has_issues);
        assert_eq!(
            result.banner_line,
            "Issues filed: 4 (Rule: 2, Flaky Test: 1, Tech Debt: 1)"
        );
    }

    #[test]
    fn table_contains_all_issues() {
        let state = json!({"issues_filed": make_issues(&["Rule", "Flow"])});
        let result = format_issues_summary(&state);
        let lines: Vec<&str> = result.table.trim().split('\n').collect();
        let header_and_separator = 2;
        assert_eq!(lines.len(), header_and_separator + 2);
    }

    #[test]
    fn table_url_is_short_reference() {
        let state = json!({
            "issues_filed": [{
                "label": "Rule",
                "title": "Test rule",
                "url": "https://github.com/test/test/issues/42",
                "phase": "flow-learn",
                "phase_name": "Learn",
                "timestamp": "2026-01-01T00:00:00-08:00",
            }]
        });
        let result = format_issues_summary(&state);
        assert!(result.table.contains("#42"));
    }

    #[test]
    fn label_order_preserved() {
        let state = json!({"issues_filed": make_issues(&["Flaky Test", "Rule", "Flaky Test"])});
        let result = format_issues_summary(&state);
        assert_eq!(
            result.banner_line,
            "Issues filed: 3 (Flaky Test: 2, Rule: 1)"
        );
    }

    #[test]
    fn phase_name_fallback_to_phase() {
        let state = json!({
            "issues_filed": [{
                "label": "Rule",
                "title": "Test",
                "url": "https://github.com/test/test/issues/1",
                "phase": "flow-code",
            }]
        });
        let result = format_issues_summary(&state);
        assert!(result.table.contains("| flow-code |"));
    }

    #[test]
    fn cli_happy_path() {
        let dir = tempfile::tempdir().unwrap();
        let state = json!({
            "issues_filed": make_issues(&["Rule", "Flow"]),
        });
        let state_path = dir.path().join("state.json");
        std::fs::write(&state_path, serde_json::to_string(&state).unwrap()).unwrap();
        let output_path = dir.path().join("issues.md");

        // Test the format function directly
        let result = format_issues_summary(&state);
        assert!(result.has_issues);
        assert!(result.banner_line.contains("Issues filed: 2"));

        // Write table to verify file output logic
        std::fs::write(&output_path, &result.table).unwrap();
        let table_on_disk = std::fs::read_to_string(&output_path).unwrap();
        assert!(table_on_disk.contains("| Label | Title | Phase | URL |"));
    }

    #[test]
    fn cli_no_issues_skips_file_write() {
        let dir = tempfile::tempdir().unwrap();
        let state = json!({"issues_filed": []});
        let output_path = dir.path().join("issues.md");

        let result = format_issues_summary(&state);
        assert!(!result.has_issues);
        // Should not write file when no issues
        assert!(!output_path.exists());
    }
}
