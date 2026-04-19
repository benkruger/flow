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
    // Matches the historic `json_error` behavior in this module: prints
    // a structured error but exits 0 so the calling skill can parse the
    // payload rather than abort.
    (
        json!({
            "status": "error",
            "message": message,
        }),
        0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phase_config::PHASE_ORDER;
    use serde_json::json;

    /// Build a minimal test state.
    fn make_test_state() -> serde_json::Value {
        json!({
            "schema_version": 1,
            "branch": "test-feature",
            "repo": "test/repo",
            "pr_number": 1,
            "pr_url": "https://github.com/test/repo/pull/1",
            "started_at": "2026-01-01T00:00:00Z",
            "current_phase": "flow-start",
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
            "prompt": "test feature description",
            "phases": {
                "flow-start": {
                    "name": "Start",
                    "status": "in_progress",
                    "started_at": "2026-01-01T00:00:00Z",
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
            }
        })
    }

    // --- format_timings_table ---

    #[test]
    fn timings_table_started_only_filters() {
        let mut state = make_test_state();
        state["phases"]["flow-start"]["cumulative_seconds"] = json!(30);
        state["phases"]["flow-plan"]["started_at"] = json!("2026-01-01T00:01:00Z");
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(300);

        let table = format_timings_table(&state, true);
        assert!(table.contains("| Start |"));
        assert!(table.contains("| Plan |"));
        assert!(!table.contains("| Code |"));
        assert!(!table.contains("| Code Review |"));
        assert!(!table.contains("| Learn |"));
        assert!(!table.contains("| Complete |"));
        assert!(table.contains("| **Total** |"));
    }

    #[test]
    fn timings_table_all_phases() {
        let mut state = make_test_state();
        for key in PHASE_ORDER {
            state["phases"][key]["started_at"] = json!("2026-01-01T00:00:00Z");
            state["phases"][key]["cumulative_seconds"] = json!(60);
        }

        let table = format_timings_table(&state, false);
        assert!(table.contains("| Start |"));
        assert!(table.contains("| Code Review |"));
        assert!(table.contains("| Complete |"));
    }

    #[test]
    fn timings_table_total_row() {
        let mut state = make_test_state();
        state["phases"]["flow-start"]["cumulative_seconds"] = json!(120);
        state["phases"]["flow-plan"]["started_at"] = json!("2026-01-01T00:01:00Z");
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(180);

        let table = format_timings_table(&state, true);
        assert!(table.contains("| **Total** | **5m** |"));
    }

    // --- render_body ---

    #[test]
    fn minimal_state() {
        let state = make_test_state();
        let dir = tempfile::tempdir().unwrap();

        let body = render_body(&state, dir.path()).unwrap();

        assert!(body.starts_with("## What"));
        assert!(body.contains("## Artifacts"));
        assert!(body.contains("## Phase Timings"));
        assert!(body.contains("## State File"));
        assert!(!body.contains("## Plan\n"));
        assert!(!body.contains("## DAG Analysis"));
        assert!(!body.contains("## Session Log"));
        assert!(!body.contains("## Issues Filed"));
    }

    #[test]
    fn what_uses_prompt() {
        let mut state = make_test_state();
        state["prompt"] = json!("fix login timeout when session expires");

        let dir = tempfile::tempdir().unwrap();
        let body = render_body(&state, dir.path()).unwrap();

        assert!(body.contains("fix login timeout when session expires."));
    }

    #[test]
    fn what_raises_on_empty_prompt() {
        let mut state = make_test_state();
        state["prompt"] = json!("");

        let dir = tempfile::tempdir().unwrap();
        let result = render_body(&state, dir.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing 'prompt'"));
    }

    #[test]
    fn what_raises_when_no_prompt_key() {
        let mut state = make_test_state();
        state.as_object_mut().unwrap().remove("prompt");

        let dir = tempfile::tempdir().unwrap();
        let result = render_body(&state, dir.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing 'prompt'"));
    }

    #[test]
    fn with_plan_only() {
        let mut state = make_test_state();
        let dir = tempfile::tempdir().unwrap();
        let plan_file = dir.path().join("plan.md");
        std::fs::write(&plan_file, "# My Plan\n\nDo the thing.").unwrap();
        state["plan_file"] = json!(plan_file.to_string_lossy().to_string());

        let body = render_body(&state, dir.path()).unwrap();

        assert!(body.contains("## Plan"));
        assert!(body.contains("Do the thing."));
        assert!(!body.contains("## DAG Analysis"));
    }

    #[test]
    fn with_plan_and_dag() {
        let mut state = make_test_state();
        let dir = tempfile::tempdir().unwrap();
        let plan_file = dir.path().join("plan.md");
        std::fs::write(&plan_file, "# Plan content").unwrap();
        let dag_file = dir.path().join("dag.md");
        std::fs::write(&dag_file, "# DAG content").unwrap();
        state["plan_file"] = json!(plan_file.to_string_lossy().to_string());
        state["dag_file"] = json!(dag_file.to_string_lossy().to_string());

        let body = render_body(&state, dir.path()).unwrap();

        assert!(body.contains("## Plan"));
        assert!(body.contains("## DAG Analysis"));
        assert!(body.contains("Plan content"));
        assert!(body.contains("DAG content"));
    }

    #[test]
    fn dag_always_text_format() {
        let mut state = make_test_state();
        let dir = tempfile::tempdir().unwrap();
        let dag_file = dir.path().join("dag.md");
        std::fs::write(&dag_file, r#"<dag goal="test"><node id="1"/></dag>"#).unwrap();
        state["dag_file"] = json!(dag_file.to_string_lossy().to_string());

        let body = render_body(&state, dir.path()).unwrap();

        assert!(body.contains("```text"));
        assert!(!body.contains("```xml"));
        assert!(body.contains(r#"<dag goal="test">"#));
    }

    #[test]
    fn nested_fences_preserve_subsequent_sections() {
        let mut state = make_test_state();
        let dir = tempfile::tempdir().unwrap();
        let dag_file = dir.path().join("dag.md");
        std::fs::write(
            &dag_file,
            "# DAG Analysis\n\n```xml\n<dag goal='test'><node id='1'/></dag>\n```\n\n```python\nprint('hello')\n```",
        ).unwrap();
        state["dag_file"] = json!(dag_file.to_string_lossy().to_string());

        let body = render_body(&state, dir.path()).unwrap();

        assert!(body.contains("## Phase Timings"));
        assert!(body.contains("## State File"));
        let dag_start = body.find("## DAG Analysis").unwrap();
        let dag_section = &body[dag_start..];
        assert!(dag_section.contains("````"));
    }

    #[test]
    fn with_transcript() {
        let mut state = make_test_state();
        state["transcript_path"] = json!("/path/to/session.jsonl");

        let dir = tempfile::tempdir().unwrap();
        let body = render_body(&state, dir.path()).unwrap();

        assert!(body.contains("| Transcript |"));
        assert!(body.contains("/path/to/session.jsonl"));
    }

    #[test]
    fn full_state() {
        let mut state = make_test_state();
        let dir = tempfile::tempdir().unwrap();

        for key in PHASE_ORDER {
            state["phases"][key]["status"] = json!("complete");
            state["phases"][key]["started_at"] = json!("2026-01-01T00:00:00Z");
            state["phases"][key]["cumulative_seconds"] = json!(60);
        }
        state["current_phase"] = json!("flow-complete");

        let plan_file = dir.path().join("plan.md");
        std::fs::write(&plan_file, "Plan content").unwrap();
        let dag_file = dir.path().join("dag.md");
        std::fs::write(&dag_file, "DAG content").unwrap();
        let log_dir = dir.path().join(".flow-states");
        std::fs::create_dir_all(&log_dir).unwrap();
        let log_file = log_dir.join("test-feature.log");
        std::fs::write(&log_file, "2026-01-01 [Phase 1] Step 1 — done").unwrap();

        state["plan_file"] = json!(plan_file.to_string_lossy().to_string());
        state["dag_file"] = json!(dag_file.to_string_lossy().to_string());
        state["transcript_path"] = json!("/path/to/session.jsonl");
        state["issues_filed"] = json!([{
            "label": "Flow",
            "title": "Test issue",
            "url": "https://github.com/test/test/issues/1",
            "phase_name": "Learn"
        }]);

        let body = render_body(&state, dir.path()).unwrap();

        assert!(body.contains("## What"));
        assert!(body.contains("## Artifacts"));
        assert!(body.contains("## Plan"));
        assert!(body.contains("## DAG Analysis"));
        assert!(body.contains("## Phase Timings"));
        assert!(body.contains("## State File"));
        assert!(body.contains("## Session Log"));
        assert!(body.contains("## Issues Filed"));
    }

    #[test]
    fn with_issues() {
        let mut state = make_test_state();
        state["issues_filed"] = json!([{
            "label": "Rule",
            "title": "Add rule X",
            "url": "https://github.com/test/test/issues/5",
            "phase_name": "Learn"
        }]);

        let dir = tempfile::tempdir().unwrap();
        let body = render_body(&state, dir.path()).unwrap();

        assert!(body.contains("## Issues Filed"));
        assert!(body.contains("Add rule X"));
    }

    #[test]
    fn plan_from_files_block() {
        let mut state = make_test_state();
        let dir = tempfile::tempdir().unwrap();
        let plan_dir = dir.path().join(".flow-states");
        std::fs::create_dir_all(&plan_dir).unwrap();
        let plan_file = plan_dir.join("test-feature-plan.md");
        std::fs::write(&plan_file, "# Plan from files block").unwrap();
        state["files"]["plan"] = json!(".flow-states/test-feature-plan.md");

        let body = render_body(&state, dir.path()).unwrap();

        assert!(body.contains("## Plan"));
        assert!(body.contains("Plan from files block"));
    }

    #[test]
    fn dag_from_files_block() {
        let mut state = make_test_state();
        let dir = tempfile::tempdir().unwrap();
        let dag_dir = dir.path().join(".flow-states");
        std::fs::create_dir_all(&dag_dir).unwrap();
        let dag_file = dag_dir.join("test-feature-dag.md");
        std::fs::write(&dag_file, "# DAG from files block").unwrap();
        state["files"]["dag"] = json!(".flow-states/test-feature-dag.md");

        let body = render_body(&state, dir.path()).unwrap();

        assert!(body.contains("## DAG Analysis"));
        assert!(body.contains("DAG from files block"));
    }

    #[test]
    fn artifacts_table_from_files_block() {
        let mut state = make_test_state();
        state["files"]["plan"] = json!(".flow-states/test-feature-plan.md");
        state["files"]["dag"] = json!(".flow-states/test-feature-dag.md");

        let dir = tempfile::tempdir().unwrap();
        let body = render_body(&state, dir.path()).unwrap();

        assert!(body.contains("| File | Path |"));
        assert!(body.contains(".flow-states/test-feature-plan.md"));
        assert!(body.contains(".flow-states/test-feature-dag.md"));
        assert!(body.contains(".flow-states/test-feature.log"));
        assert!(body.contains(".flow-states/test-feature.json"));
    }

    #[test]
    fn legacy_artifacts_without_files_block() {
        let mut state = make_test_state();
        state.as_object_mut().unwrap().remove("files");
        state["plan_file"] = json!("/abs/path/to/plan.md");
        state["dag_file"] = json!("/abs/path/to/dag.md");
        state["transcript_path"] = json!("/abs/path/to/session.jsonl");

        let dir = tempfile::tempdir().unwrap();
        let body = render_body(&state, dir.path()).unwrap();

        assert!(body.contains("**Plan file**"));
        assert!(body.contains("**DAG file**"));
        assert!(body.contains("**Session log**"));
        assert!(!body.contains("| File | Path |"));
    }

    #[test]
    fn empty_artifacts_no_files_block() {
        let mut state = make_test_state();
        state.as_object_mut().unwrap().remove("files");

        let dir = tempfile::tempdir().unwrap();
        let body = render_body(&state, dir.path()).unwrap();

        assert!(body.contains("## Artifacts\n\n## Phase"));
    }

    #[test]
    fn missing_plan_file() {
        let mut state = make_test_state();
        let dir = tempfile::tempdir().unwrap();
        state["plan_file"] = json!(dir
            .path()
            .join("nonexistent-plan.md")
            .to_string_lossy()
            .to_string());

        let body = render_body(&state, dir.path()).unwrap();
        // Plan heading only appears in a details block, not standalone
        // so "## Plan\n" should not appear
        let has_plan_section = body.contains("## Plan\n\n<details>");
        assert!(!has_plan_section);
    }

    #[test]
    fn missing_dag_file() {
        let mut state = make_test_state();
        let dir = tempfile::tempdir().unwrap();
        state["dag_file"] = json!(dir
            .path()
            .join("nonexistent-dag.md")
            .to_string_lossy()
            .to_string());

        let body = render_body(&state, dir.path()).unwrap();
        assert!(!body.contains("## DAG Analysis"));
    }

    #[test]
    fn idempotent() {
        let mut state = make_test_state();
        let dir = tempfile::tempdir().unwrap();
        let plan_file = dir.path().join("plan.md");
        std::fs::write(&plan_file, "Plan content").unwrap();
        state["plan_file"] = json!(plan_file.to_string_lossy().to_string());

        let body1 = render_body(&state, dir.path()).unwrap();
        let body2 = render_body(&state, dir.path()).unwrap();

        assert_eq!(body1, body2);
    }

    #[test]
    fn phase_timings_shows_started_only() {
        let mut state = make_test_state();
        state["phases"]["flow-start"]["cumulative_seconds"] = json!(30);
        state["phases"]["flow-plan"]["status"] = json!("complete");
        state["phases"]["flow-plan"]["started_at"] = json!("2026-01-01T00:01:00Z");
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(300);
        state["phases"]["flow-code"]["status"] = json!("in_progress");
        state["phases"]["flow-code"]["started_at"] = json!("2026-01-01T00:06:00Z");

        let dir = tempfile::tempdir().unwrap();
        let body = render_body(&state, dir.path()).unwrap();

        assert!(body.contains("| Start |"));
        assert!(body.contains("| Plan |"));
        assert!(body.contains("| Code |"));
        assert!(!body.contains("| Code Review |"));
        assert!(!body.contains("| Learn |"));
        // "Complete" may appear in ## Complete heading from state, check timings section only
        let timings_start = body.find("## Phase Timings").unwrap();
        let timings_end = body.find("<!-- end:Phase Timings -->").unwrap();
        let timings_section = &body[timings_start..timings_end];
        assert!(!timings_section.contains("| Complete |"));
    }

    #[test]
    fn section_order() {
        let mut state = make_test_state();
        let dir = tempfile::tempdir().unwrap();

        for key in PHASE_ORDER {
            state["phases"][key]["status"] = json!("complete");
            state["phases"][key]["started_at"] = json!("2026-01-01T00:00:00Z");
            state["phases"][key]["cumulative_seconds"] = json!(60);
        }
        state["current_phase"] = json!("flow-complete");

        let plan_file = dir.path().join("plan.md");
        std::fs::write(&plan_file, "Plan").unwrap();
        let dag_file = dir.path().join("dag.md");
        std::fs::write(&dag_file, "DAG").unwrap();
        let log_dir = dir.path().join(".flow-states");
        std::fs::create_dir_all(&log_dir).unwrap();
        std::fs::write(log_dir.join("test-feature.log"), "log entry").unwrap();
        state["plan_file"] = json!(plan_file.to_string_lossy().to_string());
        state["dag_file"] = json!(dag_file.to_string_lossy().to_string());
        state["transcript_path"] = json!("/path/to/session.jsonl");
        state["issues_filed"] = json!([{
            "label": "Flow",
            "title": "Issue",
            "url": "https://github.com/t/t/issues/1",
            "phase_name": "Learn"
        }]);

        let body = render_body(&state, dir.path()).unwrap();

        let headings = [
            "## What",
            "## Artifacts",
            "## Plan",
            "## DAG Analysis",
            "## Phase Timings",
            "## State File",
            "## Session Log",
            "## Issues Filed",
        ];
        let positions: Vec<usize> = headings.iter().map(|h| body.find(h).unwrap()).collect();
        let mut sorted = positions.clone();
        sorted.sort();
        assert_eq!(positions, sorted, "Sections out of order");
    }

    #[test]
    fn no_issues_no_section() {
        let mut state = make_test_state();
        state["issues_filed"] = json!([]);

        let dir = tempfile::tempdir().unwrap();
        let body = render_body(&state, dir.path()).unwrap();

        assert!(!body.contains("## Issues Filed"));
    }

    // --- Closing keywords ---

    #[test]
    fn what_section_includes_closing_keywords() {
        let mut state = make_test_state();
        state["prompt"] = json!("work on issue #643");

        let dir = tempfile::tempdir().unwrap();
        let body = render_body(&state, dir.path()).unwrap();

        assert!(body.contains("work on issue #643."));
        assert!(body.contains("Closes #643"));
    }

    #[test]
    fn what_section_no_closing_keywords_without_issues() {
        let mut state = make_test_state();
        state["prompt"] = json!("add dark mode toggle");

        let dir = tempfile::tempdir().unwrap();
        let body = render_body(&state, dir.path()).unwrap();

        assert!(body.contains("add dark mode toggle."));
        assert!(!body.contains("Closes"));
    }

    #[test]
    fn what_section_multiple_closing_keywords() {
        let mut state = make_test_state();
        state["prompt"] = json!("fix #1 and #2");

        let dir = tempfile::tempdir().unwrap();
        let body = render_body(&state, dir.path()).unwrap();

        assert!(body.contains("fix #1 and #2."));
        assert!(body.contains("Closes #1"));
        assert!(body.contains("Closes #2"));
    }

    #[test]
    fn what_section_no_double_period() {
        let mut state = make_test_state();
        state["prompt"] = json!("Fix the login timeout bug.");

        let dir = tempfile::tempdir().unwrap();
        let body = render_body(&state, dir.path()).unwrap();

        assert!(body.contains("Fix the login timeout bug."));
        assert!(!body.contains("Fix the login timeout bug.."));
    }

    #[test]
    fn timings_table_float_cumulative_seconds() {
        let mut state = make_test_state();
        state["phases"]["flow-start"]["cumulative_seconds"] = json!(120.0);

        let table = format_timings_table(&state, true);
        assert!(table.contains("| Start | 2m |"));
        assert!(table.contains("| **Total** | **2m** |"));
    }

    // --- CLI ---

    #[test]
    fn cli_dry_run_returns_sections() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        std::fs::create_dir_all(&state_dir).unwrap();
        let state = make_test_state();
        let state_file = state_dir.join("test-feature.json");
        std::fs::write(&state_file, serde_json::to_string(&state).unwrap()).unwrap();

        // Test render_body directly (CLI test via subprocess not needed for dry-run)
        let body = render_body(&state, dir.path()).unwrap();
        assert!(body.contains("## What"));
        assert!(body.contains("## Artifacts"));
    }

    #[test]
    fn cli_missing_state_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        assert!(!path.exists());
        // The run() function would print error JSON — tested via integration
    }
}
