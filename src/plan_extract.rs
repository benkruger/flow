//! Plan extraction command — accelerates the Plan phase for pre-decomposed issues.
//!
//! Consolidates the Plan phase ceremony (gate check, phase enter, issue fetch,
//! DAG/plan file creation, state mutations, logging, PR render, phase complete)
//! into a single process. For decomposed issues with an Implementation Plan
//! section, this eliminates ~12-19 model round trips.
//!
//! Three response paths:
//! - `extracted`: decomposed issue with Implementation Plan — phase completed in one call
//! - `standard`: not decomposed or no plan section — model takes over for decompose/explore/write
//! - `resumed`: plan already exists in state — phase completed in one call
//!
//! Error responses use `Ok(json!({"status": "error", ...}))` so the caller
//! receives structured JSON on stdout. Infrastructure errors (file I/O, lock
//! failures) return `Err(String)` for `run()` to wrap in `json_error`.

use std::path::PathBuf;

use clap::Parser;
use serde_json::{json, Value};

use std::process::Command;

use crate::commands::log::append_log;
use crate::git::{project_root, resolve_branch};
use crate::lock::mutate_state;
use crate::output::json_error;
use crate::phase_config::load_phase_config;
use crate::phase_transition::{phase_complete, phase_enter};
use crate::render_pr_body::render_body;
use crate::update_pr_body::gh_set_body;
use crate::utils::extract_issue_numbers;

/// Extract and fast-track pre-decomposed plans, or prepare state for model-driven planning.
#[derive(Parser, Debug)]
#[command(name = "plan-extract")]
pub struct Args {
    /// Override branch for state file lookup
    #[arg(long)]
    pub branch: Option<String>,

    /// PR number (read from state file if omitted)
    #[arg(long)]
    pub pr: Option<i64>,
}

pub fn run(args: Args) {
    match run_impl(&args) {
        Ok(result) => {
            println!("{}", result);
        }
        Err(e) => {
            json_error(&e, &[]);
            std::process::exit(1);
        }
    }
}

/// Resolve the project root and state file path for the given branch.
///
/// Returns (root, branch_name, state_path). Returns Err if branch cannot
/// be resolved or state file does not exist.
fn resolve_state(args: &Args) -> Result<(PathBuf, String, PathBuf), Value> {
    let root = project_root();
    let branch = match resolve_branch(args.branch.as_deref(), &root) {
        Some(b) => b,
        None => {
            return Err(
                json!({"status": "error", "message": "Could not determine current branch"}),
            );
        }
    };

    let state_path = root.join(".flow-states").join(format!("{}.json", branch));
    if !state_path.exists() {
        return Err(json!({
            "status": "error",
            "message": format!("No state file found: {}", state_path.display())
        }));
    }

    Ok((root, branch, state_path))
}

/// Check that flow-start is complete. Returns error JSON if not.
fn gate_check(state: &Value) -> Result<(), Value> {
    let start_status = state
        .get("phases")
        .and_then(|p| p.get("flow-start"))
        .and_then(|s| s.get("status"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if start_status != "complete" {
        return Err(json!({
            "status": "error",
            "message": "Phase 1: Start must be complete. Run /flow:flow-start first."
        }));
    }
    Ok(())
}

/// Load frozen phase config if available, for phase_complete.
fn load_frozen_config(
    root: &PathBuf,
    branch: &str,
) -> (
    Option<Vec<String>>,
    Option<indexmap::IndexMap<String, String>>,
) {
    let frozen_path = root
        .join(".flow-states")
        .join(format!("{}-phases.json", branch));
    let frozen_config = if frozen_path.exists() {
        load_phase_config(&frozen_path).ok()
    } else {
        None
    };
    let frozen_order = frozen_config.as_ref().map(|c| c.order.clone());
    let frozen_commands = frozen_config.as_ref().map(|c| c.commands.clone());
    (frozen_order, frozen_commands)
}

/// Fetch a GitHub issue via `gh issue view` and return the parsed JSON.
///
/// Returns None if gh is not available or the command fails. Relies on
/// gh's internal HTTP timeout for network-level protection.
fn fetch_issue(issue_number: i64) -> Option<Value> {
    let output = Command::new("gh")
        .args([
            "issue",
            "view",
            &issue_number.to_string(),
            "--json",
            "number,title,body,labels",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    serde_json::from_slice(&output.stdout).ok()
}

/// Check if an issue has the "decomposed" label (case-insensitive).
fn is_decomposed(issue: &Value) -> bool {
    issue
        .get("labels")
        .and_then(|l| l.as_array())
        .map(|labels| {
            labels.iter().any(|label| {
                label
                    .get("name")
                    .and_then(|n| n.as_str())
                    .map(|n| n.eq_ignore_ascii_case("decomposed"))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// Read the DAG mode from the state file's skills config.
///
/// Returns "auto" if the key is missing or not a string.
fn read_dag_mode(state: &Value) -> String {
    state
        .get("skills")
        .and_then(|s| s.get("flow-plan"))
        .and_then(|p| p.get("dag"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("auto")
        .to_string()
}

/// Extract the `## Implementation Plan` section from an issue body.
///
/// Returns the content between `## Implementation Plan` and the next `##`-level
/// heading (or end of string). Returns None if the section is not found.
pub fn extract_implementation_plan(body: &str) -> Option<String> {
    let marker = "## Implementation Plan";
    let start_idx = body.find(marker)?;
    let after_marker = start_idx + marker.len();

    // Find the next ## heading after the marker
    let content_after = &body[after_marker..];
    let end_offset = content_after
        .find("\n## ")
        .map(|pos| after_marker + pos)
        .unwrap_or(body.len());

    let section = body[after_marker..end_offset].trim().to_string();
    if section.is_empty() {
        return None;
    }
    Some(section)
}

/// Promote markdown headings by one level: `###` → `##`, `####` → `###`.
///
/// Tracks fenced code block boundaries to skip promotions inside code blocks.
pub fn promote_headings(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut in_code_block = false;

    for line in content.lines() {
        let trimmed = line.trim_start();
        // Track fenced code blocks
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            result.push_str(line);
            result.push('\n');
            continue;
        }

        if in_code_block {
            result.push_str(line);
            result.push('\n');
            continue;
        }

        // Promote headings: remove one leading # if line starts with ## or more
        if trimmed.starts_with("####") {
            // #### → ###
            if let Some(pos) = line.find("####") {
                result.push_str(&line[..pos]);
                result.push_str(&line[pos + 1..]);
            }
        } else if trimmed.starts_with("###") {
            // ### → ##
            if let Some(pos) = line.find("###") {
                result.push_str(&line[..pos]);
                result.push_str(&line[pos + 1..]);
            }
        } else {
            result.push_str(line);
        }
        result.push('\n');
    }

    // Remove trailing newline added by the loop
    if result.ends_with('\n') && !content.ends_with('\n') {
        result.pop();
    }
    result
}

/// Count `#### Task N:` headings in the content (pre-promotion format).
pub fn count_tasks(content: &str) -> usize {
    let mut count = 0;
    let mut in_code_block = false;

    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if !in_code_block && trimmed.starts_with("#### Task ") {
            count += 1;
        }
    }
    count
}

/// Run phase_complete via mutate_state and return the result JSON.
fn complete_plan_phase(
    state_path: &PathBuf,
    root: &PathBuf,
    branch: &str,
) -> Result<Value, String> {
    let (frozen_order, frozen_commands) = load_frozen_config(root, branch);
    let result_holder = std::cell::RefCell::new(Value::Null);

    mutate_state(state_path, |state| {
        let result = phase_complete(
            state,
            "flow-plan",
            None,
            frozen_order.as_deref(),
            frozen_commands.as_ref(),
        );
        *result_holder.borrow_mut() = result;
    })
    .map_err(|e| format!("Failed to complete phase: {}", e))?;

    Ok(result_holder.into_inner())
}

/// Fallible entry point for plan extraction.
///
/// Returns structured JSON as `Ok(Value)` for all business responses
/// (including status-error responses like gate failures). Returns
/// `Err(String)` only for infrastructure failures (file I/O, lock errors).
pub fn run_impl(args: &Args) -> Result<Value, String> {
    // --- Resolve state file ---
    let (root, branch, state_path) = match resolve_state(args) {
        Ok(v) => v,
        Err(err_json) => return Ok(err_json),
    };

    // --- Read state for gate and resume checks ---
    let state_content = std::fs::read_to_string(&state_path)
        .map_err(|e| format!("Could not read state file: {}", e))?;
    let state: Value = serde_json::from_str(&state_content)
        .map_err(|e| format!("Invalid JSON in state file: {}", e))?;

    // --- Gate: flow-start must be complete ---
    if let Err(err_json) = gate_check(&state) {
        return Ok(err_json);
    }

    // --- Resume check ---
    let plan_path = state
        .get("files")
        .and_then(|f| f.get("plan"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    // Resume: plan already exists → enter phase, complete immediately, return "resumed"
    if let Some(plan_rel) = plan_path {
        let plan_abs = root.join(plan_rel);

        // Enter the phase (idempotent if already in_progress)
        mutate_state(&state_path, |state| {
            if !(state.is_object() || state.is_null()) {
                return;
            }
            phase_enter(state, "flow-plan", None);
        })
        .map_err(|e| format!("Failed to enter phase: {}", e))?;

        // Complete the phase
        let complete_result = complete_plan_phase(&state_path, &root, &branch)?;

        let plan_content = std::fs::read_to_string(&plan_abs)
            .map_err(|e| format!("Could not read plan file: {}", e))?;

        let formatted_time = complete_result
            .get("formatted_time")
            .and_then(|v| v.as_str())
            .unwrap_or("< 1m")
            .to_string();
        let continue_action = complete_result
            .get("continue_action")
            .and_then(|v| v.as_str())
            .unwrap_or("ask")
            .to_string();

        return Ok(json!({
            "status": "ok",
            "path": "resumed",
            "plan_content": plan_content,
            "plan_file": plan_rel,
            "formatted_time": formatted_time,
            "continue_action": continue_action,
        }));
    }

    // DAG-exists resume is not fast-pathed — re-fetch from GitHub.
    // The skill's Resume Check handles the DAG-only case after plan-extract returns.

    // --- Phase enter ---
    mutate_state(&state_path, |state| {
        if !(state.is_object() || state.is_null()) {
            return;
        }
        phase_enter(state, "flow-plan", None);
        // Set step tracking for TUI
        state["plan_steps_total"] = json!(4);
        state["plan_step"] = json!(1);
    })
    .map_err(|e| format!("Failed to enter phase: {}", e))?;

    // --- Issue fetch + decomposed detection ---
    let prompt = state.get("prompt").and_then(|v| v.as_str()).unwrap_or("");

    let issue_numbers = extract_issue_numbers(prompt);
    let dag_mode = read_dag_mode(&state);

    // No issue references → standard path
    if issue_numbers.is_empty() {
        return Ok(json!({
            "status": "ok",
            "path": "standard",
            "issue_body": null,
            "issue_number": null,
            "dag_mode": dag_mode,
        }));
    }

    // Fetch all referenced issues, looking for a decomposed one
    let mut first_issue_body: Option<String> = None;
    let mut first_issue_number: Option<i64> = None;
    let mut decomposed_data: Option<Value> = None;

    for &num in &issue_numbers {
        let data = match fetch_issue(num) {
            Some(d) => d,
            None => continue,
        };

        let body = data
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Keep the first successfully fetched issue as context
        if first_issue_body.is_none() {
            first_issue_body = Some(body.clone());
            first_issue_number = Some(num);
        }

        if is_decomposed(&data) {
            decomposed_data = Some(data);
            first_issue_body = Some(body);
            first_issue_number = Some(num);
            break;
        }
    }

    let issue_body = first_issue_body.unwrap_or_default();
    let issue_number = first_issue_number.unwrap_or(issue_numbers[0]);

    // No decomposed issue found → standard path with first issue body
    let issue_data = match decomposed_data {
        Some(data) => data,
        None => {
            return Ok(json!({
                "status": "ok",
                "path": "standard",
                "issue_body": if issue_body.is_empty() { Value::Null } else { json!(issue_body) },
                "issue_number": issue_number,
                "dag_mode": dag_mode,
            }));
        }
    };

    // --- Decomposed issue: write DAG file ---
    let feature_desc = issue_data
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("feature");

    let dag_content = format!(
        "# Pre-Decomposed Analysis: {}\n\n{}",
        feature_desc, issue_body
    );
    let dag_rel = format!(".flow-states/{}-dag.md", branch);
    let dag_abs = root.join(&dag_rel);
    std::fs::write(&dag_abs, &dag_content)
        .map_err(|e| format!("Failed to write DAG file: {}", e))?;

    // Update files.dag in state
    mutate_state(&state_path, |state| {
        if !(state.is_object() || state.is_null()) {
            return;
        }
        // Nested guard: ensure files is an object before chained assignment
        if !matches!(state.get("files"), Some(v) if v.is_object()) {
            state["files"] = json!({});
        }
        state["files"]["dag"] = json!(&dag_rel);
        state["plan_step"] = json!(2);
    })
    .map_err(|e| format!("Failed to update state: {}", e))?;

    // --- Extract Implementation Plan section ---
    let plan_section = match extract_implementation_plan(&issue_body) {
        Some(s) => s,
        None => {
            // No Implementation Plan section — return standard path
            // so the model handles it as an older-format decomposed issue
            return Ok(json!({
                "status": "ok",
                "path": "standard",
                "issue_body": issue_body,
                "issue_number": issue_number,
                "dag_mode": dag_mode,
            }));
        }
    };

    // Count tasks before promotion (#### Task N: format)
    let task_count = count_tasks(&plan_section);

    // Promote headings: ### → ##, #### → ###
    let promoted = promote_headings(&plan_section);

    // Write plan file
    let plan_rel = format!(".flow-states/{}-plan.md", branch);
    let plan_abs = root.join(&plan_rel);
    std::fs::write(&plan_abs, &promoted)
        .map_err(|e| format!("Failed to write plan file: {}", e))?;

    // Update state: files.plan, code_tasks_total, plan_step
    mutate_state(&state_path, |state| {
        if !(state.is_object() || state.is_null()) {
            return;
        }
        // Nested guard: ensure files is an object before chained assignment
        if !matches!(state.get("files"), Some(v) if v.is_object()) {
            state["files"] = json!({});
        }
        state["files"]["plan"] = json!(&plan_rel);
        state["code_tasks_total"] = json!(task_count);
        state["plan_step"] = json!(3);
    })
    .map_err(|e| format!("Failed to update state: {}", e))?;

    // --- Logging ---
    let _ = append_log(
        &root,
        &branch,
        "[Phase 2] plan-extract — gate check passed (exit 0)",
    );
    let _ = append_log(
        &root,
        &branch,
        &format!(
            "[Phase 2] plan-extract — issue #{} fetched, decomposed label detected (exit 0)",
            issue_number
        ),
    );
    let _ = append_log(
        &root,
        &branch,
        &format!(
            "[Phase 2] plan-extract — DAG file written: {} (exit 0)",
            dag_rel
        ),
    );
    let _ = append_log(
        &root,
        &branch,
        &format!(
            "[Phase 2] plan-extract — plan extracted, {} tasks, written: {} (exit 0)",
            task_count, plan_rel
        ),
    );

    // --- Update plan_step to 4 before PR render ---
    mutate_state(&state_path, |state| {
        if !(state.is_object() || state.is_null()) {
            return;
        }
        state["plan_step"] = json!(4);
    })
    .map_err(|e| format!("Failed to update state: {}", e))?;

    // --- PR body render ---
    // Re-read the state file since we've mutated it multiple times
    let updated_state_content = std::fs::read_to_string(&state_path)
        .map_err(|e| format!("Could not re-read state file: {}", e))?;
    let updated_state: Value = serde_json::from_str(&updated_state_content)
        .map_err(|e| format!("Invalid JSON in state file: {}", e))?;

    let pr = args
        .pr
        .or_else(|| updated_state.get("pr_number").and_then(|v| v.as_i64()));

    if let Ok(body) = render_body(&updated_state, &root) {
        if let Some(pr_number) = pr {
            let _ = gh_set_body(pr_number, &body);
        }
    }

    let _ = append_log(
        &root,
        &branch,
        "[Phase 2] plan-extract — PR body rendered (exit 0)",
    );

    // --- Phase complete ---
    let complete_result = complete_plan_phase(&state_path, &root, &branch)?;

    let formatted_time = complete_result
        .get("formatted_time")
        .and_then(|v| v.as_str())
        .unwrap_or("< 1m")
        .to_string();
    let continue_action = complete_result
        .get("continue_action")
        .and_then(|v| v.as_str())
        .unwrap_or("ask")
        .to_string();

    let _ = append_log(
        &root,
        &branch,
        &format!(
            "[Phase 2] plan-extract — phase complete ({}) (exit 0)",
            formatted_time
        ),
    );

    Ok(json!({
        "status": "ok",
        "path": "extracted",
        "plan_content": promoted,
        "plan_file": plan_rel,
        "dag_file": dag_rel,
        "task_count": task_count,
        "formatted_time": formatted_time,
        "continue_action": continue_action,
    }))
}
