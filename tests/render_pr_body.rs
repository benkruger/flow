//! Tests for `src/render_pr_body.rs`.
//!
//! Covers `render_body` and `format_timings_table` directly, plus
//! subprocess tests for `bin/flow render-pr-body` that exercise the
//! `run_impl_main` dispatch including gh subprocess failure paths.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_gh_stub, create_git_repo_with_remote, parse_output};
use flow_rs::format_pr_timings::format_timings_table;
use flow_rs::phase_config::PHASE_ORDER;
use flow_rs::render_pr_body::render_body;
use serde_json::{json, Value};

// --- Fixtures ---

fn make_test_state() -> Value {
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
            "log": ".flow-states/test-feature/log",
            "state": ".flow-states/test-feature/state.json"
        },
        "session_tty": null,
        "session_id": null,
        "transcript_path": null,
        "notes": [],
        "prompt": "test feature description",
        "phases": {
            "flow-start": {"name": "Start", "status": "in_progress", "started_at": "2026-01-01T00:00:00Z", "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 1},
            "flow-plan": {"name": "Plan", "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0},
            "flow-code": {"name": "Code", "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0},
            "flow-code-review": {"name": "Code Review", "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0},
            "flow-learn": {"name": "Learn", "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0},
            "flow-complete": {"name": "Complete", "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0}
        }
    })
}

fn minimal_complete_state(feature: &str) -> Value {
    json!({
        "schema_version": 1,
        "branch": "test-branch",
        "feature": feature,
        "prompt": feature,
        "pr_number": 42,
        "pr_url": "https://github.com/o/r/pull/42",
        "phases": {
            "flow-start":        {"status": "complete", "cumulative_seconds": 10, "visit_count": 1},
            "flow-plan":         {"status": "complete", "cumulative_seconds": 20, "visit_count": 1},
            "flow-code":         {"status": "complete", "cumulative_seconds": 30, "visit_count": 1},
            "flow-code-review":  {"status": "complete", "cumulative_seconds": 40, "visit_count": 1},
            "flow-learn":        {"status": "complete", "cumulative_seconds": 50, "visit_count": 1},
            "flow-complete":     {"status": "pending"}
        },
        "findings": [],
        "issues_filed": [],
        "notes": [],
    })
}

fn write_state(repo: &Path, name: &str, state: &Value) -> std::path::PathBuf {
    let branch_dir = repo.join(".flow-states").join(name);
    fs::create_dir_all(&branch_dir).unwrap();
    let path = branch_dir.join("state.json");
    fs::write(&path, serde_json::to_string_pretty(state).unwrap()).unwrap();
    path
}

fn run_render(repo: &Path, args: &[&str], stub_dir: &Path) -> Output {
    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("render-pr-body")
        .args(args)
        .current_dir(repo)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
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

#[test]
fn timings_table_float_cumulative_seconds() {
    let mut state = make_test_state();
    state["phases"]["flow-start"]["cumulative_seconds"] = json!(120.0);

    let table = format_timings_table(&state, true);
    assert!(table.contains("| Start | 2m |"));
    assert!(table.contains("| **Total** | **2m** |"));
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
    fs::write(&plan_file, "# My Plan\n\nDo the thing.").unwrap();
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
    fs::write(&plan_file, "# Plan content").unwrap();
    let dag_file = dir.path().join("dag.md");
    fs::write(&dag_file, "# DAG content").unwrap();
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
    fs::write(&dag_file, r#"<dag goal="test"><node id="1"/></dag>"#).unwrap();
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
    fs::write(
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
    fs::write(&plan_file, "Plan content").unwrap();
    let dag_file = dir.path().join("dag.md");
    fs::write(&dag_file, "DAG content").unwrap();
    let branch_dir = dir.path().join(".flow-states").join("test-feature");
    fs::create_dir_all(&branch_dir).unwrap();
    let log_file = branch_dir.join("log");
    fs::write(&log_file, "2026-01-01 [Phase 1] Step 1 — done").unwrap();

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
    let branch_dir = dir.path().join(".flow-states").join("test-feature");
    fs::create_dir_all(&branch_dir).unwrap();
    let plan_file = branch_dir.join("plan.md");
    fs::write(&plan_file, "# Plan from files block").unwrap();
    state["files"]["plan"] = json!(".flow-states/test-feature/plan.md");

    let body = render_body(&state, dir.path()).unwrap();

    assert!(body.contains("## Plan"));
    assert!(body.contains("Plan from files block"));
}

#[test]
fn dag_from_files_block() {
    let mut state = make_test_state();
    let dir = tempfile::tempdir().unwrap();
    let branch_dir = dir.path().join(".flow-states").join("test-feature");
    fs::create_dir_all(&branch_dir).unwrap();
    let dag_file = branch_dir.join("dag.md");
    fs::write(&dag_file, "# DAG from files block").unwrap();
    state["files"]["dag"] = json!(".flow-states/test-feature/dag.md");

    let body = render_body(&state, dir.path()).unwrap();

    assert!(body.contains("## DAG Analysis"));
    assert!(body.contains("DAG from files block"));
}

#[test]
fn artifacts_table_from_files_block() {
    let mut state = make_test_state();
    state["files"]["plan"] = json!(".flow-states/test-feature/plan.md");
    state["files"]["dag"] = json!(".flow-states/test-feature/dag.md");

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    assert!(body.contains("| File | Path |"));
    assert!(body.contains(".flow-states/test-feature/plan.md"));
    assert!(body.contains(".flow-states/test-feature/dag.md"));
    assert!(body.contains(".flow-states/test-feature/log"));
    assert!(body.contains(".flow-states/test-feature/state.json"));
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
    fs::write(&plan_file, "Plan content").unwrap();
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
    fs::write(&plan_file, "Plan").unwrap();
    let dag_file = dir.path().join("dag.md");
    fs::write(&dag_file, "DAG").unwrap();
    let branch_log_dir = dir.path().join(".flow-states").join("test-feature");
    fs::create_dir_all(&branch_log_dir).unwrap();
    fs::write(branch_log_dir.join("log"), "log entry").unwrap();
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

/// Covers the "transcript_path is Some but empty string" branch in
/// the files-block path of build_artifacts (line skipping the empty
/// transcript row).
#[test]
fn artifacts_files_block_empty_transcript_skipped() {
    let mut state = make_test_state();
    state["files"]["plan"] = json!(".flow-states/test-feature/plan.md");
    state["transcript_path"] = json!("");

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();
    assert!(!body.contains("| Transcript |"));
}

/// Covers "dag_file is empty string" in legacy path.
#[test]
fn artifacts_legacy_empty_dag_file_skipped() {
    let mut state = make_test_state();
    state.as_object_mut().unwrap().remove("files");
    state["plan_file"] = json!("/abs/plan.md");
    state["dag_file"] = json!("");

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();
    assert!(body.contains("**Plan file**"));
    assert!(!body.contains("**DAG file**"));
}

/// Covers "transcript_path is empty string" in legacy path.
#[test]
fn artifacts_legacy_empty_transcript_skipped() {
    let mut state = make_test_state();
    state.as_object_mut().unwrap().remove("files");
    state["plan_file"] = json!("/abs/plan.md");
    state["transcript_path"] = json!("");

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();
    assert!(!body.contains("**Session log**"));
}

/// Covers the `.map_err(|e| e.to_string())` closure on the plan-file
/// read — pointing plan_file at a directory makes `pp.exists()`
/// return true but `read_to_string(pp)` return Err (EISDIR).
#[test]
fn plan_file_as_directory_propagates_error() {
    let mut state = make_test_state();
    let dir = tempfile::tempdir().unwrap();
    let plan_as_dir = dir.path().join("plan-dir");
    fs::create_dir(&plan_as_dir).unwrap();
    state["plan_file"] = json!(plan_as_dir.to_string_lossy().to_string());

    let result = render_body(&state, dir.path());
    assert!(result.is_err());
}

/// Same as above but for the DAG file read.
#[test]
fn dag_file_as_directory_propagates_error() {
    let mut state = make_test_state();
    let dir = tempfile::tempdir().unwrap();
    let dag_as_dir = dir.path().join("dag-dir");
    fs::create_dir(&dag_as_dir).unwrap();
    state["dag_file"] = json!(dag_as_dir.to_string_lossy().to_string());

    let result = render_body(&state, dir.path());
    assert!(result.is_err());
}

/// Same for the session-log file read (absolute path, file slot).
#[test]
fn session_log_as_directory_propagates_error() {
    let mut state = make_test_state();
    let dir = tempfile::tempdir().unwrap();
    let log_as_dir = dir.path().join("log-dir");
    fs::create_dir(&log_as_dir).unwrap();
    state["files"]["log"] = json!(log_as_dir.to_string_lossy().to_string());

    let result = render_body(&state, dir.path());
    assert!(result.is_err());
}

// --- CLI subprocess tests for run_impl_main ---

#[test]
fn render_pr_body_dry_run_returns_sections() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = minimal_complete_state("Test feature");
    let state_path = write_state(&repo, "test-branch", &state);
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_render(
        &repo,
        &[
            "--pr",
            "42",
            "--state-file",
            state_path.to_str().unwrap(),
            "--dry-run",
        ],
        &stub_dir,
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    let sections = data["sections"].as_array().unwrap();
    assert!(!sections.is_empty(), "Expected section headers, got empty");
}

#[test]
fn render_pr_body_missing_state_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");
    let missing = dir.path().join("no-such.json");

    let output = run_render(
        &repo,
        &[
            "--pr",
            "42",
            "--state-file",
            missing.to_str().unwrap(),
            "--dry-run",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("State file not found"));
}

#[test]
fn render_pr_body_malformed_state_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let path = state_dir.join("bad.json");
    fs::write(&path, "not json").unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_render(
        &repo,
        &[
            "--pr",
            "42",
            "--state-file",
            path.to_str().unwrap(),
            "--dry-run",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
}

#[test]
fn render_pr_body_render_error_on_missing_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let mut state = minimal_complete_state("My feature");
    state.as_object_mut().unwrap().remove("prompt");
    let state_path = write_state(&repo, "test-branch", &state);
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_render(
        &repo,
        &[
            "--pr",
            "42",
            "--state-file",
            state_path.to_str().unwrap(),
            "--dry-run",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("missing 'prompt'"));
}

#[test]
fn render_pr_body_non_dry_run_calls_gh_edit() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = minimal_complete_state("Live render");
    let state_path = write_state(&repo, "test-branch", &state);
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_render(
        &repo,
        &["--pr", "42", "--state-file", state_path.to_str().unwrap()],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
}

/// Covers the `read_to_string` Err path in run_impl_main: the path
/// exists but can't be read. A directory at the state-file path
/// passes `.exists()` and then fails `read_to_string`.
#[test]
fn render_pr_body_read_error_reports_io_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    // Create a DIRECTORY at the state-file path. exists() returns
    // true so run_impl_main proceeds to read_to_string, which then
    // fails with an I/O error (EISDIR on Linux, similar on macOS).
    let state_path = state_dir.join("test-branch.json");
    fs::create_dir(&state_path).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_render(
        &repo,
        &[
            "--pr",
            "42",
            "--state-file",
            state_path.to_str().unwrap(),
            "--dry-run",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
}

/// Exercises the else branch of `if let Some(ref sf) = args.state_file`
/// inside run_impl_main — no `--state-file` CLI flag, so state path is
/// auto-derived from `FLOW_SIMULATE_BRANCH` + `project_root()`.
#[test]
fn render_pr_body_auto_detects_state_file_when_no_flag() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = minimal_complete_state("Auto-detect feature");
    write_state(&repo, "auto-feature", &state);
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("render-pr-body")
        .args(["--pr", "42", "--dry-run"])
        .current_dir(&repo)
        .env("PATH", &path_env)
        .env("FLOW_SIMULATE_BRANCH", "auto-feature")
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
}

/// Covers `resolve_path` empty-string short-circuit (returns None
/// instead of treating the empty string as a path). Driven via
/// `render_body` with an empty `plan_file` value.
#[test]
fn resolve_path_empty_string_treated_as_none() {
    let mut state = make_test_state();
    state.as_object_mut().unwrap().remove("files");
    state["plan_file"] = json!("");

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();
    // Empty plan_file shouldn't produce a Plan section.
    assert!(!body.contains("## Plan\n\n<details>"));
}

/// Covers the `files block present but every path empty/null` branch
/// in `build_artifacts`, which short-circuits to `return vec![]`.
#[test]
fn artifacts_files_block_all_empty_returns_empty() {
    let mut state = make_test_state();
    state["files"] = json!({
        "plan": "",
        "dag": "",
        "log": "",
        "state": ""
    });
    // Also null out transcript so the block stays empty.
    state["transcript_path"] = json!(null);

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    // An empty files block + no legacy plan/dag keys produces a bare
    // "## Artifacts" section with no body.
    assert!(body.contains("## Artifacts\n\n## Phase"));
    assert!(!body.contains("| File | Path |"));
}

#[test]
fn render_pr_body_gh_edit_failure_reports_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = minimal_complete_state("Failing edit");
    let state_path = write_state(&repo, "test-branch", &state);
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho 'edit denied' >&2\nexit 1\n");

    let output = run_render(
        &repo,
        &["--pr", "42", "--state-file", state_path.to_str().unwrap()],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("edit denied"));
}
