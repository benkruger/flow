use std::path::{Path, PathBuf};

use clap::Parser;
use serde_json::json;

use crate::flow_paths::FlowPaths;
use crate::format_issues_summary::format_issues_summary;
use crate::format_pr_timings::format_timings_table;
use crate::git::{current_branch, project_root};
use crate::update_pr_body::{build_details_block, build_plain_section, gh_set_body};
use crate::utils::extract_issue_numbers;

/// Resolve a file path, handling both absolute and relative paths.
///
/// Returns None if path_str is empty or null.
/// Relative paths are resolved against project_dir.
fn resolve_path(path_str: Option<&str>, project_dir: &Path) -> Option<PathBuf> {
    let s = path_str?;
    if s.is_empty() {
        return None;
    }
    let p = Path::new(s);
    if p.is_absolute() {
        Some(p.to_path_buf())
    } else {
        Some(project_dir.join(p))
    }
}

/// Build the ## Artifacts section from state fields.
///
/// Prefers the structured files block (relative paths) when present.
/// Falls back to legacy top-level plan_file/dag_file for old state files.
fn build_artifacts(state: &serde_json::Value) -> Vec<String> {
    if let Some(files) = state.get("files").and_then(|v| v.as_object()) {
        let mut rows = vec!["| File | Path |".to_string(), "|------|------|".to_string()];
        let labels = [
            ("Plan", "plan"),
            ("DAG", "dag"),
            ("Log", "log"),
            ("State", "state"),
        ];
        for (label, key) in &labels {
            if let Some(path) = files.get(*key).and_then(|v| v.as_str()) {
                if !path.is_empty() {
                    rows.push(format!("| {} | `{}` |", label, path));
                }
            }
        }
        if let Some(transcript) = state.get("transcript_path").and_then(|v| v.as_str()) {
            if !transcript.is_empty() {
                rows.push(format!("| Transcript | `{}` |", transcript));
            }
        }
        if rows.len() > 2 {
            return rows;
        }
        return vec![];
    }

    // Legacy: top-level plan_file/dag_file
    let mut items = Vec::new();
    if let Some(plan_file) = state.get("plan_file").and_then(|v| v.as_str()) {
        if !plan_file.is_empty() {
            items.push(format!("- **Plan file**: `{}`", plan_file));
        }
    }
    if let Some(dag_file) = state.get("dag_file").and_then(|v| v.as_str()) {
        if !dag_file.is_empty() {
            items.push(format!("- **DAG file**: `{}`", dag_file));
        }
    }
    if let Some(transcript) = state.get("transcript_path").and_then(|v| v.as_str()) {
        if !transcript.is_empty() {
            items.push(format!("- **Session log**: `{}`", transcript));
        }
    }
    items
}

/// Render the complete PR body from state and artifact files.
///
/// Returns the complete PR body as a string.
pub fn render_body(state: &serde_json::Value, project_dir: &Path) -> Result<String, String> {
    let mut sections = Vec::new();
    let mut section_names = Vec::new();

    // 1. What (always) — requires prompt field from init-state
    let what_text = state
        .get("prompt")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            "State file missing 'prompt' field — init-state should always set this".to_string()
        })?;

    let mut what_section = if what_text.ends_with('.') {
        format!("## What\n\n{}", what_text)
    } else {
        format!("## What\n\n{}.", what_text)
    };
    let issue_numbers = extract_issue_numbers(what_text);
    if !issue_numbers.is_empty() {
        let closing_lines: Vec<String> = issue_numbers
            .iter()
            .map(|n| format!("Closes #{}", n))
            .collect();
        what_section.push_str(&format!("\n\n{}", closing_lines.join("\n")));
    }
    sections.push(what_section);
    section_names.push("What".to_string());

    // 2. Artifacts (always, items conditional)
    let artifact_items = build_artifacts(state);
    if !artifact_items.is_empty() {
        sections.push(format!("## Artifacts\n\n{}", artifact_items.join("\n")));
    } else {
        sections.push("## Artifacts".to_string());
    }
    section_names.push("Artifacts".to_string());

    // Resolve artifact paths from files block with legacy fallback
    let files = state.get("files");
    let plan_path_str = files
        .and_then(|f| f.get("plan"))
        .and_then(|v| v.as_str())
        .or_else(|| state.get("plan_file").and_then(|v| v.as_str()));
    let dag_path_str = files
        .and_then(|f| f.get("dag"))
        .and_then(|v| v.as_str())
        .or_else(|| state.get("dag_file").and_then(|v| v.as_str()));

    let plan_path = resolve_path(plan_path_str, project_dir);
    let dag_path = resolve_path(dag_path_str, project_dir);

    // 3. Plan (conditional)
    if let Some(ref pp) = plan_path {
        if pp.exists() {
            let content = std::fs::read_to_string(pp)
                .map_err(|e| e.to_string())?
                .trim_end_matches('\n')
                .to_string();
            sections.push(build_details_block(
                "Plan",
                "Implementation plan",
                &content,
                "text",
            ));
            section_names.push("Plan".to_string());
        }
    }

    // 4. DAG Analysis (conditional, always text format)
    if let Some(ref dp) = dag_path {
        if dp.exists() {
            let content = std::fs::read_to_string(dp)
                .map_err(|e| e.to_string())?
                .trim_end_matches('\n')
                .to_string();
            sections.push(build_details_block(
                "DAG Analysis",
                "Decompose plugin output",
                &content,
                "text",
            ));
            section_names.push("DAG Analysis".to_string());
        }
    }

    // 5. Phase Timings (always, started phases only)
    let timings_table = format_timings_table(state, true);
    sections.push(build_plain_section("Phase Timings", &timings_table));
    section_names.push("Phase Timings".to_string());

    // 6. State File (always)
    let state_json = serde_json::to_string_pretty(state).unwrap_or_default();
    let branch = state
        .get("branch")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    sections.push(build_details_block(
        "State File",
        &format!(".flow-states/{}.json", branch),
        &state_json,
        "json",
    ));
    section_names.push("State File".to_string());

    // 7. Session Log (conditional)
    let default_log = format!(".flow-states/{}.log", branch);
    let log_path_str = files
        .and_then(|f| f.get("log"))
        .and_then(|v| v.as_str())
        .unwrap_or(&default_log);
    let log_path = resolve_path(Some(log_path_str), project_dir);
    if let Some(ref lp) = log_path {
        if lp.exists() {
            let log_rel = files
                .and_then(|f| f.get("log"))
                .and_then(|v| v.as_str())
                .unwrap_or(&default_log);
            let content = std::fs::read_to_string(lp)
                .map_err(|e| e.to_string())?
                .trim_end_matches('\n')
                .to_string();
            sections.push(build_details_block(
                "Session Log",
                log_rel,
                &content,
                "text",
            ));
            section_names.push("Session Log".to_string());
        }
    }

    // 8. Issues Filed (conditional)
    let issues_result = format_issues_summary(state);
    if issues_result.has_issues {
        sections.push(build_plain_section("Issues Filed", &issues_result.table));
        section_names.push("Issues Filed".to_string());
    }

    Ok(sections.join("\n\n"))
}

#[derive(Parser, Debug)]
#[command(name = "render-pr-body", about = "Render complete PR body from state")]
pub struct Args {
    /// PR number
    #[arg(long)]
    pub pr: i64,

    /// Path to state file (auto-detected if omitted)
    #[arg(long = "state-file")]
    pub state_file: Option<String>,

    /// Generate body and return sections without updating PR
    #[arg(long = "dry-run")]
    pub dry_run: bool,
}

pub fn run_impl_main(args: &Args) -> (serde_json::Value, i32) {
    let state_path = if let Some(ref sf) = args.state_file {
        PathBuf::from(sf)
    } else {
        let root = project_root();
        let branch = current_branch().unwrap_or_default();
        FlowPaths::new(&root, &branch).state_file()
    };

    if !state_path.exists() {
        return json_error_tuple(&format!("State file not found: {}", state_path.display()));
    }

    let content = match std::fs::read_to_string(&state_path) {
        Ok(c) => c,
        Err(e) => return json_error_tuple(&e.to_string()),
    };

    let state: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => return json_error_tuple(&e.to_string()),
    };

    let project_dir = state_path
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(Path::new("."));

    let body = match render_body(&state, project_dir) {
        Ok(b) => b,
        Err(e) => return json_error_tuple(&e),
    };

    if !args.dry_run {
        if let Err(e) = gh_set_body(args.pr, &body) {
            return json_error_tuple(&e);
        }
    }

    let section_names: Vec<&str> = body
        .lines()
        .filter(|line| line.starts_with("## "))
        .map(|line| &line[3..])
        .collect();

    (
        json!({
            "status": "ok",
            "sections": section_names,
        }),
        0,
    )
}

fn json_error_tuple(message: &str) -> (serde_json::Value, i32) {
    // Print a structured error but exit 0 so the calling skill can parse
    // the payload rather than abort.
    (
        json!({
            "status": "error",
            "message": message,
        }),
        0,
    )
}
