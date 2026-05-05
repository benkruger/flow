//! Complete phase "Done" banner formatter.
//!
//! Tests live in `tests/format_complete_summary.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]`
//! block in this file.

use std::path::Path;

use clap::Parser;
use serde_json::{json, Value};

use crate::phase_config::{self, PHASE_ORDER};
use crate::state::PhaseState;
use crate::utils::{derive_feature, format_time, read_version, short_issue_ref};
use crate::window_deltas::phase_delta;
use indexmap::IndexMap;

use crate::state::ModelTokens;

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

/// Format an integer token count as a compact string: `1.2K`, `3.4M`,
/// or the raw integer when below 1000. Stable formatting so test
/// assertions can pin specific values.
fn format_tokens(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Render the Token Cost section from `state.phases.<phase>.window_at_*`
/// snapshots via `window_deltas::phase_delta`. Returns an empty Vec
/// when no phase has populated snapshots — the renderer skips the
/// section entirely rather than rendering a header with no rows.
fn token_cost_section(state: &Value) -> Vec<String> {
    let names = phase_config::phase_names();

    let mut phase_rows: Vec<(String, i64, f64, bool)> = Vec::new();
    let mut total_tokens: i64 = 0;
    let mut total_cost: f64 = 0.0;
    let mut combined_by_model: IndexMap<String, ModelTokens> = IndexMap::new();
    let mut reset_observed_anywhere = false;

    for &key in PHASE_ORDER {
        let Some(phase_v) = state.get("phases").and_then(|p| p.get(key)) else {
            continue;
        };
        let Ok(phase_state) = serde_json::from_value::<PhaseState>(phase_v.clone()) else {
            continue;
        };
        let Some(report) = phase_delta(&phase_state) else {
            continue;
        };
        let tokens = report
            .input_tokens_delta
            .saturating_add(report.output_tokens_delta)
            .saturating_add(report.cache_creation_tokens_delta)
            .saturating_add(report.cache_read_tokens_delta);
        // Skip phases that contributed nothing — the section is for
        // surface-able token activity, not a per-phase placeholder.
        if tokens == 0 && report.cost_delta_usd.abs() < f64::EPSILON {
            continue;
        }
        let name = names
            .get(key)
            .cloned()
            .expect("phase_config::phase_names is keyed by PHASE_ORDER");
        phase_rows.push((
            name,
            tokens,
            report.cost_delta_usd,
            report.window_reset_observed,
        ));
        total_tokens = total_tokens.saturating_add(tokens);
        total_cost += report.cost_delta_usd;
        if report.window_reset_observed {
            reset_observed_anywhere = true;
        }
        for (model, mt) in &report.by_model_delta {
            let entry = combined_by_model.entry(model.clone()).or_default();
            entry.input = entry.input.saturating_add(mt.input);
            entry.output = entry.output.saturating_add(mt.output);
            entry.cache_create = entry.cache_create.saturating_add(mt.cache_create);
            entry.cache_read = entry.cache_read.saturating_add(mt.cache_read);
        }
    }

    if phase_rows.is_empty() {
        return Vec::new();
    }

    let mut lines = Vec::new();
    lines.push("  Token Cost".to_string());
    lines.push(format!("  {}", "─".repeat(28)));
    for (name, tokens, cost, reset) in &phase_rows {
        let marker = if *reset { " ↻" } else { "" };
        lines.push(format!(
            "  {:<16} {:>8}  ${:.3}{}",
            format!("{}:", name),
            format_tokens(*tokens),
            cost,
            marker
        ));
    }
    lines.push(format!("  {}", "─".repeat(28)));
    lines.push(format!(
        "  {:<16} {:>8}  ${:.3}",
        "Total:",
        format_tokens(total_tokens),
        total_cost
    ));
    if combined_by_model.len() >= 2 {
        lines.push(String::new());
        lines.push("  By Model".to_string());
        for (model, mt) in &combined_by_model {
            let total_model = mt
                .input
                .saturating_add(mt.output)
                .saturating_add(mt.cache_create)
                .saturating_add(mt.cache_read);
            lines.push(format!(
                "    {:<24} {:>8}",
                model,
                format_tokens(total_model)
            ));
        }
    }
    if reset_observed_anywhere {
        lines.push(String::new());
        lines.push("  ↻ rate-limit window reset observed mid-flow".to_string());
    }
    lines.push(String::new());
    lines
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

    // Token Cost section (between Findings and Artifacts) — empty
    // when no phase carries window snapshot data per
    // `docs/reference/flow-state-schema.md` "Window Snapshot".
    let token_lines = token_cost_section(state);
    lines.extend(token_lines);

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
