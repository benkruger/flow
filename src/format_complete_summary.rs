use std::path::Path;

use clap::Parser;
use serde_json::{json, Value};

use crate::phase_config::{self, PHASE_ORDER};
use crate::utils::{derive_feature, format_time, read_version, short_issue_ref};

/// Maximum prompt length before truncation.
const MAX_PROMPT_LENGTH: usize = 80;

/// Result of formatting the complete summary.
#[derive(Debug)]
pub struct SummaryResult {
    pub summary: String,
    pub total_seconds: i64,
    pub issues_links: String,
}

/// Truncate prompt to MAX_PROMPT_LENGTH chars (code points) with ellipsis.
fn truncate_prompt(prompt: &str) -> String {
    if prompt.chars().count() <= MAX_PROMPT_LENGTH {
        return prompt.to_string();
    }
    let truncated: String = prompt.chars().take(MAX_PROMPT_LENGTH).collect();
    format!("{}...", truncated)
}

/// Map a finding outcome to its display marker.
fn outcome_marker(outcome: &str) -> &'static str {
    match outcome {
        "fixed" => "✓",
        "dismissed" => "✗",
        "filed" => "→",
        "rule_written" | "rule_clarified" => "+",
        _ => "?",
    }
}

/// Map a finding outcome to its display label.
fn outcome_label(outcome: &str) -> &'static str {
    match outcome {
        "fixed" => "Fixed",
        "dismissed" => "Dismissed",
        "filed" => "Filed",
        "rule_written" => "Rule written",
        "rule_clarified" => "Rule clarified",
        _ => "Unknown",
    }
}

/// Render a findings section for a single phase.
///
/// Returns lines for the section header and each finding (two lines per finding:
/// marker + description, then indented outcome label + reason). Returns empty vec
/// if no findings match the phase.
fn phase_findings_section(findings: &[Value], phase_key: &str, section_title: &str) -> Vec<String> {
    let matched: Vec<&Value> = findings
        .iter()
        .filter(|f| f.get("phase").and_then(|p| p.as_str()) == Some(phase_key))
        .collect();
    if matched.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    lines.push(format!("  {}", section_title));
    lines.push(format!("  {}", "─".repeat(28)));
    for f in &matched {
        let finding = f.get("finding").and_then(|v| v.as_str()).unwrap_or("");
        let reason = f.get("reason").and_then(|v| v.as_str()).unwrap_or("");
        let outcome = f.get("outcome").and_then(|v| v.as_str()).unwrap_or("");
        let marker = outcome_marker(outcome);
        let label = outcome_label(outcome);
        lines.push(format!("    {} {}", marker, finding));
        lines.push(format!("      {} — {}", label, reason));
    }
    lines.push(String::new());
    lines
}

/// Build the Complete phase Done banner from state dict.
pub fn format_complete_summary(state: &Value, closed_issues: Option<&[Value]>) -> SummaryResult {
    let names = phase_config::phase_names();

    let branch = state
        .get("branch")
        .and_then(|b| b.as_str())
        .unwrap_or("unknown");
    let feature = derive_feature(branch);
    let prompt = state.get("prompt").and_then(|p| p.as_str()).unwrap_or("");
    let pr_url = state
        .get("pr_url")
        .and_then(|u| u.as_str())
        .unwrap_or("N/A");
    let phases = state.get("phases").and_then(|p| p.as_object());
    let issues = state.get("issues_filed").and_then(|i| i.as_array());
    let notes = state.get("notes").and_then(|n| n.as_array());
    let findings = state.get("findings").and_then(|f| f.as_array());
    let version = read_version();

    // Build phase timing rows and total
    let mut total_seconds: i64 = 0;
    let mut timing_lines = Vec::new();

    for &key in PHASE_ORDER {
        let phase = phases.and_then(|p| p.get(key));
        let seconds = phase
            .and_then(|p| p.get("cumulative_seconds"))
            .and_then(|s| s.as_i64())
            .unwrap_or(0);
        total_seconds += seconds;
        let name = names.get(key).map(|s| s.as_str()).unwrap_or(key);
        timing_lines.push(format!(
            "  {:<16} {}",
            format!("{}:", name),
            format_time(seconds)
        ));
    }

    // Build the summary
    let border = "━".repeat(58);
    let mut lines = Vec::new();
    lines.push(border.clone());
    lines.push(format!("  ✓ FLOW v{} — Complete", version));
    lines.push(border.clone());
    lines.push(String::new());
    lines.push(format!("  Feature:  {}", feature));
    lines.push(format!("  What:     {}", truncate_prompt(prompt)));
    lines.push(format!("  PR:       {}", pr_url));

    // Resolved section (closed issues)
    if let Some(closed) = closed_issues {
        if !closed.is_empty() {
            lines.push(String::new());
            lines.push("  Resolved".to_string());
            lines.push(format!("  {}", "─".repeat(28)));
            for resolved in closed {
                let num = resolved.get("number").and_then(|n| n.as_i64()).unwrap_or(0);
                let url = resolved.get("url").and_then(|u| u.as_str()).unwrap_or("");
                if !url.is_empty() {
                    lines.push(format!("    #{} {}", num, url));
                } else {
                    lines.push(format!("    #{}", num));
                }
            }
        }
    }

    lines.push(String::new());
    lines.push("  Timeline".to_string());
    lines.push(format!("  {}", "─".repeat(28)));
    for timing_line in &timing_lines {
        lines.push(timing_line.clone());
    }
    lines.push(format!("  {}", "─".repeat(28)));
    lines.push(format!("  {:<16} {}", "Total:", format_time(total_seconds)));
    lines.push(String::new());

    // Findings sections (between Timeline and Artifacts)
    if let Some(findings_arr) = findings {
        let cr_lines =
            phase_findings_section(findings_arr, "flow-code-review", "Code Review Findings");
        lines.extend(cr_lines);
        let learn_lines = phase_findings_section(findings_arr, "flow-learn", "Learn Findings");
        lines.extend(learn_lines);
    }

    // Artifacts section
    let issues_count = issues.map(|i| i.len()).unwrap_or(0);
    let notes_count = notes.map(|n| n.len()).unwrap_or(0);
    let has_artifacts = issues_count > 0 || notes_count > 0;
    if has_artifacts {
        lines.push("  Artifacts".to_string());
        lines.push(format!("  {}", "─".repeat(28)));
        if issues_count > 0 {
            lines.push(format!("  Issues filed: {}", issues_count));
        }
        if notes_count > 0 {
            lines.push(format!("  Notes captured: {}", notes_count));
        }
        lines.push(String::new());
    }

    lines.push(border);

    // Build issues_links
    let mut issue_link_lines = Vec::new();
    if let Some(issues_arr) = issues {
        for issue in issues_arr {
            let url = issue.get("url").and_then(|u| u.as_str()).unwrap_or("");
            let shorthand = if !url.is_empty() {
                short_issue_ref(url)
            } else {
                String::new()
            };
            let prefix = if shorthand.starts_with('#') {
                format!("{} ", shorthand)
            } else {
                String::new()
            };
            let label = issue.get("label").and_then(|l| l.as_str()).unwrap_or("");
            let title = issue.get("title").and_then(|t| t.as_str()).unwrap_or("");
            let title_part = format!("[{}] {}{}", label, prefix, title);
            if !url.is_empty() {
                issue_link_lines.push(format!("  {} — {}", title_part, url));
            } else {
                issue_link_lines.push(format!("  {}", title_part));
            }
        }
    }

    let summary = lines.join("\n");
    let issues_links = issue_link_lines.join("\n");

    SummaryResult {
        summary,
        total_seconds,
        issues_links,
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "format-complete-summary",
    about = "Format the Complete phase Done banner"
)]
pub struct Args {
    /// Path to state JSON file
    #[arg(long)]
    pub state_file: String,

    /// Path to closed issues JSON file
    #[arg(long)]
    pub closed_issues_file: Option<String>,
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

    let closed_issues: Option<Vec<Value>> = args.closed_issues_file.as_ref().and_then(|path| {
        let closed_path = Path::new(path);
        if !closed_path.exists() {
            return None;
        }
        let closed_content = std::fs::read_to_string(closed_path).ok()?;
        serde_json::from_str(&closed_content).ok()
    });

    Ok(format_complete_summary(&state, closed_issues.as_deref()))
}

/// Main-arm entry point: runs the fallible `run_impl` and wraps the
/// result into the `(Value, i32)` contract that
/// `dispatch::dispatch_json` consumes. Success returns exit 0 with a
/// `status: "ok"` payload; error returns exit 1 with a
/// `status: "error"` payload.
pub fn run_impl_main(args: &Args) -> (Value, i32) {
    match run_impl(args) {
        Ok(result) => (
            json!({
                "status": "ok",
                "summary": result.summary,
                "total_seconds": result.total_seconds,
                "issues_links": result.issues_links,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const PHASE_NAMES_LIST: [&str; 6] =
        ["Start", "Plan", "Code", "Code Review", "Learn", "Complete"];

    fn all_complete_state() -> Value {
        let mut phases = serde_json::Map::new();
        let all_phases = [
            "flow-start",
            "flow-plan",
            "flow-code",
            "flow-code-review",
            "flow-learn",
            "flow-complete",
        ];
        let timings = [20, 300, 2700, 720, 120, 45];
        for (i, &p) in all_phases.iter().enumerate() {
            phases.insert(
                p.to_string(),
                json!({
                    "name": PHASE_NAMES_LIST[i],
                    "status": "complete",
                    "started_at": "2026-01-01T00:00:00-08:00",
                    "completed_at": "2026-01-01T01:00:00-08:00",
                    "session_started_at": null,
                    "cumulative_seconds": timings[i],
                    "visit_count": 1,
                }),
            );
        }
        json!({
            "branch": "test-feature",
            "pr_url": "https://github.com/test/test/pull/1",
            "started_at": "2026-01-01T00:00:00-08:00",
            "current_phase": "flow-complete",
            "prompt": "Add invoice PDF export with watermark support",
            "issues_filed": [],
            "notes": [],
            "phases": phases,
        })
    }

    #[test]
    fn test_basic_summary() {
        let state = all_complete_state();
        let result = format_complete_summary(&state, None);

        assert!(
            result.summary.contains("Test Feature"),
            "Summary:\n{}",
            result.summary
        );
        assert!(
            result
                .summary
                .contains("Add invoice PDF export with watermark support"),
            "Summary:\n{}",
            result.summary
        );
        assert!(
            result
                .summary
                .contains("https://github.com/test/test/pull/1"),
            "Summary:\n{}",
            result.summary
        );
        for name in &PHASE_NAMES_LIST {
            assert!(
                result.summary.contains(&format!("{}:", name)),
                "Missing phase {} in summary:\n{}",
                name,
                result.summary
            );
        }
        assert!(
            result.summary.contains("Total:"),
            "Summary:\n{}",
            result.summary
        );
        assert_eq!(result.total_seconds, 20 + 300 + 2700 + 720 + 120 + 45);
        // issues_links should be present (empty string is fine)
        let _ = &result.issues_links;
    }

    #[test]
    fn test_summary_with_issues() {
        let mut state = all_complete_state();
        state["issues_filed"] = json!([
            {
                "label": "Rule",
                "title": "Test rule",
                "url": "https://github.com/test/test/issues/1",
                "phase": "flow-learn",
                "phase_name": "Learn",
                "timestamp": "2026-01-01T00:00:00-08:00",
            },
            {
                "label": "Tech Debt",
                "title": "Refactor X",
                "url": "https://github.com/test/test/issues/2",
                "phase": "flow-code-review",
                "phase_name": "Code Review",
                "timestamp": "2026-01-01T00:00:00-08:00",
            },
        ]);

        let result = format_complete_summary(&state, None);

        assert!(
            result.summary.contains("Issues filed: 2"),
            "Summary:\n{}",
            result.summary
        );
        // Per-issue details are NOT in the banner
        assert!(!result
            .summary
            .contains("https://github.com/test/test/issues/1"));
        assert!(!result
            .summary
            .contains("https://github.com/test/test/issues/2"));
        // They are in issues_links
        assert!(
            result.issues_links.contains("[Rule] #1 Test rule"),
            "Links:\n{}",
            result.issues_links
        );
        assert!(result
            .issues_links
            .contains("https://github.com/test/test/issues/1"));
        assert!(
            result.issues_links.contains("[Tech Debt] #2 Refactor X"),
            "Links:\n{}",
            result.issues_links
        );
        assert!(result
            .issues_links
            .contains("https://github.com/test/test/issues/2"));
    }

    #[test]
    fn test_summary_with_single_issue() {
        let mut state = all_complete_state();
        state["issues_filed"] = json!([
            {
                "label": "Flow",
                "title": "Fix routing logic",
                "url": "https://github.com/test/test/issues/42",
                "phase": "flow-learn",
                "phase_name": "Learn",
                "timestamp": "2026-01-01T00:00:00-08:00",
            },
        ]);

        let result = format_complete_summary(&state, None);

        assert!(result.summary.contains("Issues filed: 1"));
        assert!(!result
            .summary
            .contains("https://github.com/test/test/issues/42"));
        assert!(result.issues_links.contains("[Flow] #42 Fix routing logic"));
        assert!(result
            .issues_links
            .contains("https://github.com/test/test/issues/42"));
    }

    #[test]
    fn test_summary_with_issues_url_without_number() {
        let mut state = all_complete_state();
        state["issues_filed"] = json!([
            {
                "label": "Rule",
                "title": "Some rule",
                "url": "https://example.com/custom-path",
                "phase": "flow-learn",
                "phase_name": "Learn",
                "timestamp": "2026-01-01T00:00:00-08:00",
            },
        ]);

        let result = format_complete_summary(&state, None);

        assert!(result.summary.contains("Issues filed: 1"));
        assert!(!result.summary.contains("https://example.com/custom-path"));
        assert!(result.issues_links.contains("[Rule] Some rule"));
        assert!(result
            .issues_links
            .contains("https://example.com/custom-path"));
    }

    #[test]
    fn test_summary_with_resolved_issues() {
        let state = all_complete_state();
        let closed = vec![json!({
            "number": 407,
            "url": "https://github.com/test/test/issues/407",
        })];

        let result = format_complete_summary(&state, Some(&closed));

        assert!(
            result.summary.contains("Resolved"),
            "Summary:\n{}",
            result.summary
        );
        assert!(
            result.summary.contains("#407"),
            "Summary:\n{}",
            result.summary
        );
        assert!(result
            .summary
            .contains("https://github.com/test/test/issues/407"));
    }

    #[test]
    fn test_summary_with_multiple_resolved_issues() {
        let state = all_complete_state();
        let closed = vec![
            json!({"number": 83, "url": "https://github.com/test/test/issues/83"}),
            json!({"number": 89, "url": "https://github.com/test/test/issues/89"}),
        ];

        let result = format_complete_summary(&state, Some(&closed));

        assert!(result.summary.contains("#83"));
        assert!(result.summary.contains("#89"));
    }

    #[test]
    fn test_summary_no_resolved_issues() {
        let state = all_complete_state();

        let result_none = format_complete_summary(&state, None);
        let result_empty = format_complete_summary(&state, Some(&[]));

        assert!(!result_none.summary.contains("Resolved"));
        assert!(!result_empty.summary.contains("Resolved"));
    }

    #[test]
    fn test_summary_with_resolved_and_filed() {
        let mut state = all_complete_state();
        state["issues_filed"] = json!([
            {
                "label": "Tech Debt",
                "title": "Refactor X",
                "url": "https://github.com/test/test/issues/50",
                "phase": "flow-code-review",
                "phase_name": "Code Review",
                "timestamp": "2026-01-01T00:00:00-08:00",
            },
        ]);
        let closed = vec![json!({
            "number": 407,
            "url": "https://github.com/test/test/issues/407",
        })];

        let result = format_complete_summary(&state, Some(&closed));

        assert!(result.summary.contains("Resolved"));
        assert!(result.summary.contains("#407"));
        assert!(result.summary.contains("Issues filed: 1"));
        assert!(result.issues_links.contains("[Tech Debt] #50 Refactor X"));
        assert!(result
            .issues_links
            .contains("https://github.com/test/test/issues/50"));
    }

    #[test]
    fn test_summary_resolved_without_url() {
        let state = all_complete_state();
        let closed = vec![json!({"number": 42})];

        let result = format_complete_summary(&state, Some(&closed));

        assert!(result.summary.contains("Resolved"));
        assert!(result.summary.contains("#42"));
    }

    /// Exercises line 207 — `issue_link_lines.push(format!("  {}", title_part))`
    /// — the empty-url branch of the issues_links builder. Issues with no
    /// `url` field render as `[Label] Title` without the trailing
    /// ` — <url>` segment.
    #[test]
    fn test_summary_with_filed_issue_without_url() {
        let mut state = all_complete_state();
        state["issues_filed"] = json!([
            {
                "label": "Rule",
                "title": "URL-less rule",
                "phase": "flow-learn",
                "phase_name": "Learn",
                "timestamp": "2026-01-01T00:00:00-08:00",
            },
        ]);

        let result = format_complete_summary(&state, None);
        assert!(
            result.issues_links.contains("[Rule] URL-less rule"),
            "expected URL-less label/title in issues_links, got: {}",
            result.issues_links
        );
        assert!(
            !result.issues_links.contains(" — "),
            "no URL means no em-dash separator, got: {}",
            result.issues_links
        );
    }

    /// Exercises line 36 (`outcome_marker` catch-all `_ => "?"`) and
    /// line 48 (`outcome_label` catch-all `_ => "Unknown"`) — fires
    /// when a finding's outcome is none of the five known values
    /// (e.g., a future outcome added to VALID_OUTCOMES that this
    /// formatter has not yet learned about).
    #[test]
    fn test_summary_with_unknown_outcome_falls_back_to_question_marker() {
        let mut state = all_complete_state();
        state["findings"] = json!([
            {
                "finding": "future-outcome finding",
                "reason": "uses a not-yet-handled outcome",
                "outcome": "deferred",
                "phase": "flow-code-review",
                "phase_name": "Code Review",
                "timestamp": "2026-01-01T00:00:00-08:00",
            },
        ]);

        let result = format_complete_summary(&state, None);
        assert!(
            result.summary.contains("?"),
            "expected '?' marker for unknown outcome, got: {}",
            result.summary
        );
        assert!(
            result.summary.contains("Unknown"),
            "expected 'Unknown' label for unknown outcome, got: {}",
            result.summary
        );
    }

    #[test]
    fn test_summary_with_notes() {
        let mut state = all_complete_state();
        state["notes"] = json!([
            {
                "phase": "flow-code",
                "phase_name": "Code",
                "timestamp": "2026-01-01T00:00:00-08:00",
                "type": "correction",
                "note": "Test note",
            },
        ]);

        let result = format_complete_summary(&state, None);

        assert!(
            result.summary.contains("Notes captured: 1"),
            "Summary:\n{}",
            result.summary
        );
    }

    #[test]
    fn test_summary_no_issues_no_notes() {
        let mut state = all_complete_state();
        state["issues_filed"] = json!([]);
        state["notes"] = json!([]);

        let result = format_complete_summary(&state, None);

        assert!(!result.summary.contains("Issues filed"));
        assert!(!result.summary.contains("Notes captured"));
        assert_eq!(result.issues_links, "");
    }

    /// Exercises the `issues = None` path (line 96 returns None when
    /// `issues_filed` is absent OR not an array). This drives the
    /// implicit else branch of `if let Some(issues_arr) = issues`.
    #[test]
    fn test_summary_issues_filed_key_absent_renders_empty_links() {
        let mut state = all_complete_state();
        // Remove the issues_filed key entirely so state.get returns None.
        state.as_object_mut().unwrap().remove("issues_filed");
        let result = format_complete_summary(&state, None);
        assert_eq!(result.issues_links, "");
    }

    /// Companion: `issues_filed` present but not an array (e.g. a string)
    /// also yields `issues = None` via the `as_array` filter.
    #[test]
    fn test_summary_issues_filed_wrong_type_renders_empty_links() {
        let mut state = all_complete_state();
        state["issues_filed"] = json!("not-an-array");
        let result = format_complete_summary(&state, None);
        assert_eq!(result.issues_links, "");
    }

    #[test]
    fn test_issues_links_without_url() {
        let mut state = all_complete_state();
        state["issues_filed"] = json!([
            {
                "label": "Tech Debt",
                "title": "Missing test",
                "url": "",
                "phase": "flow-code-review",
                "phase_name": "Code Review",
                "timestamp": "2026-01-01T00:00:00-08:00",
            },
        ]);

        let result = format_complete_summary(&state, None);

        assert!(result.issues_links.contains("[Tech Debt] Missing test"));
        // No URL separator when URL is empty
        assert!(!result.issues_links.contains("—"));
    }

    #[test]
    fn test_summary_truncates_long_prompt() {
        let mut state = all_complete_state();
        let long_prompt = "A".repeat(100);
        state["prompt"] = json!(long_prompt);

        let result = format_complete_summary(&state, None);

        assert!(
            !result.summary.contains(&long_prompt),
            "Summary should not contain full prompt"
        );
        assert!(result.summary.contains("..."));
        let expected = format!("{}...", "A".repeat(80));
        assert!(result.summary.contains(&expected));
    }

    #[test]
    fn test_summary_short_prompt_not_truncated() {
        let mut state = all_complete_state();
        state["prompt"] = json!("Fix login bug");

        let result = format_complete_summary(&state, None);

        assert!(result.summary.contains("Fix login bug"));
        assert!(!result.summary.contains("..."));
    }

    #[test]
    fn test_summary_uses_format_time() {
        let state = all_complete_state();
        // flow-start has 20s → "<1m"
        // flow-code has 2700s → "45m"
        // flow-plan has 300s → "5m"

        let result = format_complete_summary(&state, None);

        assert!(
            result.summary.contains("<1m"),
            "Summary:\n{}",
            result.summary
        );
        assert!(
            result.summary.contains("45m"),
            "Summary:\n{}",
            result.summary
        );
        assert!(
            result.summary.contains("5m"),
            "Summary:\n{}",
            result.summary
        );
    }

    #[test]
    fn test_summary_heavy_borders() {
        let state = all_complete_state();
        let result = format_complete_summary(&state, None);
        assert!(
            result.summary.contains("━━"),
            "Summary:\n{}",
            result.summary
        );
    }

    #[test]
    fn test_summary_check_mark() {
        let state = all_complete_state();
        let result = format_complete_summary(&state, None);
        assert!(result.summary.contains("✓"), "Summary:\n{}", result.summary);
    }

    #[test]
    fn test_summary_version() {
        let state = all_complete_state();
        let result = format_complete_summary(&state, None);
        assert!(
            result.summary.contains("FLOW v"),
            "Summary:\n{}",
            result.summary
        );
    }

    #[test]
    fn test_truncate_prompt_under_limit() {
        assert_eq!(truncate_prompt("short"), "short");
    }

    #[test]
    fn test_truncate_prompt_at_limit() {
        let exactly_80 = "A".repeat(80);
        assert_eq!(truncate_prompt(&exactly_80), exactly_80);
    }

    #[test]
    fn test_truncate_prompt_over_limit() {
        let over = "A".repeat(100);
        let result = truncate_prompt(&over);
        assert_eq!(result.chars().count(), 83); // 80 + "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_prompt_multibyte() {
        // Multi-byte chars: each is 1 code point but multiple bytes
        let prompt: String = "日".repeat(81);
        let result = truncate_prompt(&prompt);
        assert!(result.chars().count() <= 83); // 80 chars + "..."
        assert!(result.ends_with("..."));
    }

    // --- CLI (run_impl) tests ---

    fn write_state_file(dir: &std::path::Path) -> std::path::PathBuf {
        let state = all_complete_state();
        let state_file = dir.join("state.json");
        std::fs::write(&state_file, serde_json::to_string(&state).unwrap()).unwrap();
        state_file
    }

    #[test]
    fn test_cli_happy_path() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = write_state_file(dir.path());
        let args = Args {
            state_file: state_file.to_string_lossy().to_string(),
            closed_issues_file: None,
        };
        let result = run_impl(&args);
        assert!(result.is_ok());
        let summary = result.unwrap();
        assert!(summary.summary.contains("Test Feature"));
        assert!(summary.total_seconds > 0);
        // issues_links is present (empty string is fine)
        let _ = &summary.issues_links;
    }

    #[test]
    fn test_cli_with_closed_issues_file() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = write_state_file(dir.path());
        let closed = vec![json!({
            "number": 407,
            "url": "https://github.com/test/test/issues/407",
        })];
        let closed_file = dir.path().join("closed.json");
        std::fs::write(&closed_file, serde_json::to_string(&closed).unwrap()).unwrap();

        let args = Args {
            state_file: state_file.to_string_lossy().to_string(),
            closed_issues_file: Some(closed_file.to_string_lossy().to_string()),
        };
        let result = run_impl(&args).unwrap();
        assert!(result.summary.contains("Resolved"));
        assert!(result.summary.contains("#407"));
    }

    #[test]
    fn test_cli_missing_closed_issues_file() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = write_state_file(dir.path());
        let args = Args {
            state_file: state_file.to_string_lossy().to_string(),
            closed_issues_file: Some(
                dir.path()
                    .join("nonexistent.json")
                    .to_string_lossy()
                    .to_string(),
            ),
        };
        // Missing closed_issues_file should gracefully omit the Resolved section
        let result = run_impl(&args).unwrap();
        assert!(!result.summary.contains("Resolved"));
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
            closed_issues_file: None,
        };
        let result = run_impl(&args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_cli_corrupt_state_file() {
        let dir = tempfile::tempdir().unwrap();
        let bad_file = dir.path().join("bad.json");
        std::fs::write(&bad_file, "{bad json").unwrap();
        let args = Args {
            state_file: bad_file.to_string_lossy().to_string(),
            closed_issues_file: None,
        };
        let result = run_impl(&args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to parse"));
    }

    // --- Findings display tests ---

    #[test]
    fn test_summary_with_code_review_findings() {
        let mut state = all_complete_state();
        state["findings"] = json!([
            {
                "finding": "Unused variable in handler",
                "reason": "False positive from macro expansion",
                "outcome": "dismissed",
                "phase": "flow-code-review",
                "phase_name": "Code Review",
                "timestamp": "2026-01-01T00:30:00-08:00",
            },
            {
                "finding": "Missing null check in parser",
                "reason": "Could panic on malformed input",
                "outcome": "fixed",
                "phase": "flow-code-review",
                "phase_name": "Code Review",
                "timestamp": "2026-01-01T00:31:00-08:00",
            },
        ]);

        let result = format_complete_summary(&state, None);

        assert!(
            result.summary.contains("Code Review Findings"),
            "Summary:\n{}",
            result.summary
        );
        assert!(
            result.summary.contains("Unused variable in handler"),
            "Summary:\n{}",
            result.summary
        );
        assert!(
            result.summary.contains("Missing null check in parser"),
            "Summary:\n{}",
            result.summary
        );
        assert!(
            result.summary.contains("✗"),
            "Dismissed marker missing:\n{}",
            result.summary
        );
        assert!(
            result.summary.contains("✓"),
            "Fixed marker missing:\n{}",
            result.summary
        );
        assert!(
            result
                .summary
                .contains("False positive from macro expansion"),
            "Summary:\n{}",
            result.summary
        );
    }

    #[test]
    fn test_summary_with_learn_findings() {
        let mut state = all_complete_state();
        state["findings"] = json!([
            {
                "finding": "No rule for error handling",
                "reason": "Gap identified during analysis",
                "outcome": "rule_written",
                "phase": "flow-learn",
                "phase_name": "Learn",
                "path": ".claude/rules/error-handling.md",
                "timestamp": "2026-01-01T00:45:00-08:00",
            },
        ]);

        let result = format_complete_summary(&state, None);

        assert!(
            result.summary.contains("Learn Findings"),
            "Summary:\n{}",
            result.summary
        );
        assert!(
            result.summary.contains("No rule for error handling"),
            "Summary:\n{}",
            result.summary
        );
        assert!(
            result.summary.contains("+"),
            "Rule written marker missing:\n{}",
            result.summary
        );
    }

    #[test]
    fn test_summary_with_both_phase_findings() {
        let mut state = all_complete_state();
        state["findings"] = json!([
            {
                "finding": "Bug in parser",
                "reason": "Fixed inline",
                "outcome": "fixed",
                "phase": "flow-code-review",
                "phase_name": "Code Review",
                "timestamp": "2026-01-01T00:30:00-08:00",
            },
            {
                "finding": "Missing rule",
                "reason": "Created new rule",
                "outcome": "rule_written",
                "phase": "flow-learn",
                "phase_name": "Learn",
                "timestamp": "2026-01-01T00:45:00-08:00",
            },
        ]);

        let result = format_complete_summary(&state, None);

        assert!(
            result.summary.contains("Code Review Findings"),
            "Summary:\n{}",
            result.summary
        );
        assert!(
            result.summary.contains("Learn Findings"),
            "Summary:\n{}",
            result.summary
        );
    }

    #[test]
    fn test_summary_no_findings_hides_sections() {
        let mut state = all_complete_state();
        state["findings"] = json!([]);

        let result_empty = format_complete_summary(&state, None);
        assert!(
            !result_empty.summary.contains("Code Review Findings"),
            "Summary:\n{}",
            result_empty.summary
        );
        assert!(
            !result_empty.summary.contains("Learn Findings"),
            "Summary:\n{}",
            result_empty.summary
        );

        // Also test when findings key is missing entirely
        let state_no_key = all_complete_state();
        let result_missing = format_complete_summary(&state_no_key, None);
        assert!(!result_missing.summary.contains("Code Review Findings"));
        assert!(!result_missing.summary.contains("Learn Findings"));
    }

    #[test]
    fn test_summary_findings_all_outcomes() {
        let mut state = all_complete_state();
        state["findings"] = json!([
            {
                "finding": "f1",
                "reason": "r1",
                "outcome": "fixed",
                "phase": "flow-code-review",
                "phase_name": "Code Review",
                "timestamp": "2026-01-01T00:30:00-08:00",
            },
            {
                "finding": "f2",
                "reason": "r2",
                "outcome": "dismissed",
                "phase": "flow-code-review",
                "phase_name": "Code Review",
                "timestamp": "2026-01-01T00:31:00-08:00",
            },
            {
                "finding": "f3",
                "reason": "r3",
                "outcome": "filed",
                "phase": "flow-code-review",
                "phase_name": "Code Review",
                "issue_url": "https://github.com/test/test/issues/99",
                "timestamp": "2026-01-01T00:32:00-08:00",
            },
            {
                "finding": "f4",
                "reason": "r4",
                "outcome": "rule_written",
                "phase": "flow-learn",
                "phase_name": "Learn",
                "path": ".claude/rules/test.md",
                "timestamp": "2026-01-01T00:33:00-08:00",
            },
            {
                "finding": "f5",
                "reason": "r5",
                "outcome": "rule_clarified",
                "phase": "flow-learn",
                "phase_name": "Learn",
                "path": ".claude/rules/existing.md",
                "timestamp": "2026-01-01T00:34:00-08:00",
            },
        ]);

        let result = format_complete_summary(&state, None);

        // fixed → ✓
        assert!(result.summary.contains("✓"), "Summary:\n{}", result.summary);
        // dismissed → ✗
        assert!(result.summary.contains("✗"), "Summary:\n{}", result.summary);
        // filed → →
        assert!(result.summary.contains("→"), "Summary:\n{}", result.summary);
        // rule_written and rule_clarified → +
        // Count occurrences of "+" in Learn Findings section
        let learn_section_start = result.summary.find("Learn Findings");
        assert!(
            learn_section_start.is_some(),
            "Learn Findings section missing:\n{}",
            result.summary
        );
    }

    #[test]
    fn run_impl_main_happy_path_returns_ok_value() {
        // On success, run_impl_main wraps the SummaryResult into a
        // status:ok JSON payload with exit code 0. This pins the
        // contract main.rs's FormatCompleteSummary arm relies on
        // when it calls dispatch::dispatch_json.
        let dir = tempfile::tempdir().unwrap();
        let state_file = write_state_file(dir.path());
        let args = Args {
            state_file: state_file.to_string_lossy().to_string(),
            closed_issues_file: None,
        };
        let (value, code) = run_impl_main(&args);
        assert_eq!(code, 0);
        assert_eq!(value["status"], "ok");
        assert!(value["summary"].as_str().unwrap().contains("Test Feature"));
        assert!(value["total_seconds"].as_i64().unwrap() > 0);
    }

    #[test]
    fn run_impl_main_missing_state_file_returns_err_exit_1() {
        // On error, run_impl_main returns a status:error JSON
        // payload with exit code 1 — no process::exit inside the
        // module, isolating termination to dispatch::dispatch_json.
        let dir = tempfile::tempdir().unwrap();
        let args = Args {
            state_file: dir
                .path()
                .join("missing.json")
                .to_string_lossy()
                .to_string(),
            closed_issues_file: None,
        };
        let (value, code) = run_impl_main(&args);
        assert_eq!(code, 1);
        assert_eq!(value["status"], "error");
        assert!(value["message"].as_str().unwrap().contains("not found"));
    }

    #[test]
    fn run_impl_closed_content_unreadable_omits_resolved() {
        // The run_impl closed-issues read uses read_to_string(...).ok()?
        // so a closed file that exists but cannot be read silently
        // omits the Resolved section. Pin the graceful-degradation
        // contract by pointing closed_issues_file at a directory —
        // read_to_string returns Err(IsADirectory), the .ok()? drops
        // it, and run_impl still produces a summary.
        let dir = tempfile::tempdir().unwrap();
        let state_file = write_state_file(dir.path());
        let closed_dir = dir.path().join("closed_as_dir");
        std::fs::create_dir_all(&closed_dir).unwrap();
        let args = Args {
            state_file: state_file.to_string_lossy().to_string(),
            closed_issues_file: Some(closed_dir.to_string_lossy().to_string()),
        };
        let result = run_impl(&args).unwrap();
        assert!(!result.summary.contains("Resolved"));
        assert!(result.summary.contains("Test Feature"));
    }

    #[test]
    fn run_impl_closed_content_malformed_omits_resolved() {
        // Malformed JSON in closed_issues_file: from_str(...).ok()?
        // drops the parse error and omits the Resolved section.
        let dir = tempfile::tempdir().unwrap();
        let state_file = write_state_file(dir.path());
        let closed_file = dir.path().join("malformed.json");
        std::fs::write(&closed_file, "{not valid json").unwrap();
        let args = Args {
            state_file: state_file.to_string_lossy().to_string(),
            closed_issues_file: Some(closed_file.to_string_lossy().to_string()),
        };
        let result = run_impl(&args).unwrap();
        assert!(!result.summary.contains("Resolved"));
        assert!(result.summary.contains("Test Feature"));
    }

    #[test]
    fn test_summary_findings_with_existing_artifacts() {
        let mut state = all_complete_state();
        state["findings"] = json!([
            {
                "finding": "Bug found",
                "reason": "Fixed it",
                "outcome": "fixed",
                "phase": "flow-code-review",
                "phase_name": "Code Review",
                "timestamp": "2026-01-01T00:30:00-08:00",
            },
        ]);
        state["issues_filed"] = json!([
            {
                "label": "Tech Debt",
                "title": "Refactor X",
                "url": "https://github.com/test/test/issues/50",
                "phase": "flow-code-review",
                "phase_name": "Code Review",
                "timestamp": "2026-01-01T00:00:00-08:00",
            },
        ]);

        let result = format_complete_summary(&state, None);

        // Both findings and artifacts sections should coexist
        assert!(
            result.summary.contains("Code Review Findings"),
            "Summary:\n{}",
            result.summary
        );
        assert!(
            result.summary.contains("Issues filed: 1"),
            "Summary:\n{}",
            result.summary
        );
    }
}
