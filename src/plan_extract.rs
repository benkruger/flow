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

use std::path::{Path, PathBuf};

use clap::Parser;
use serde_json::{json, Value};

use std::process::Command;

use crate::commands::log::append_log;
use crate::duplicate_test_coverage::{scan as dup_scan, TestCorpus, Violation as DupViolation};
use crate::external_input_audit::{scan as audit_scan, Violation as AuditViolation};
use crate::flow_paths::FlowPaths;
use crate::git::{project_root, resolve_branch};
use crate::lock::mutate_state;
use crate::output::json_error;
use crate::phase_config::load_phase_config;
use crate::phase_transition::{phase_complete, phase_enter};
use crate::render_pr_body::render_body;
use crate::scope_enumeration::{scan as scope_scan, Violation};
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

    let state_path = FlowPaths::new(&root, &branch).state_file();
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
    root: &Path,
    branch: &str,
) -> (
    Option<Vec<String>>,
    Option<indexmap::IndexMap<String, String>>,
) {
    let frozen_path = FlowPaths::new(root, branch).frozen_phases();
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

/// Find a markdown heading as a full line match, not a substring.
///
/// Returns the byte index of the heading start, or None if not found.
/// The heading must appear at the start of a line (preceded by `\n` or at
/// position 0) and be followed by optional trailing whitespace then `\n`,
/// `\r`, or end of string. Trailing spaces and tabs after the heading text
/// are tolerated (common in editor artifacts and copy-paste).
fn find_heading(body: &str, heading: &str) -> Option<usize> {
    // Check if body starts with the heading
    if let Some(after) = body.strip_prefix(heading) {
        if is_heading_terminated(after) {
            return Some(0);
        }
    }
    // Search for \n followed by heading
    let search = format!("\n{}", heading);
    let mut start = 0;
    while let Some(pos) = body[start..].find(&search) {
        let abs_pos = start + pos + 1; // +1 to skip the \n
        let after_heading = abs_pos + heading.len();
        let remainder = &body[after_heading..];
        if is_heading_terminated(remainder) {
            return Some(abs_pos);
        }
        start = start + pos + 1;
    }
    None
}

/// Check that the text after a heading marker is a valid line termination:
/// optional trailing whitespace (spaces/tabs) followed by `\n`, `\r`, or EOF.
fn is_heading_terminated(after: &str) -> bool {
    let trimmed = after.trim_start_matches([' ', '\t']);
    trimmed.is_empty() || trimmed.starts_with('\n') || trimmed.starts_with('\r')
}

/// Extract the `## Implementation Plan` section from an issue body.
///
/// Uses full-heading matching: the marker must appear at the start of a line
/// and be followed by a newline or end of string. Returns the content between
/// `## Implementation Plan` and the next `##`-level heading (or end of string).
/// Returns None if the section is not found.
pub fn extract_implementation_plan(body: &str) -> Option<String> {
    let marker = "## Implementation Plan";
    let start_idx = find_heading(body, marker)?;
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

/// Count `### Task N:` or `#### Task N:` headings in the content.
///
/// Used by the resume path to re-derive `code_tasks_total` from a
/// plan file that the user may have edited after a scope-enumeration
/// violation. The extracted path writes tasks with `#### Task ` before
/// `promote_headings`; after promotion the plan file on disk uses
/// `### Task `. Standard-path plans (written by the model) typically
/// also use `### Task`. This counter accepts both so a resume-path
/// recount produces a meaningful total regardless of which path
/// originally wrote the file.
pub fn count_tasks_any_level(content: &str) -> usize {
    let mut count = 0;
    let mut in_code_block = false;

    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if !in_code_block && (trimmed.starts_with("#### Task ") || trimmed.starts_with("### Task "))
        {
            count += 1;
        }
    }
    count
}

/// Build the plan-check violation response that both the extracted
/// path and the resume path return when the promoted plan content
/// fails one or both Plan-phase scanners (scope-enumeration or
/// external-input-audit).
///
/// Mirrors the `bin/flow plan-check` error shape so downstream
/// consumers (the `flow-plan` skill, model prompts) can handle every
/// gate callsite with one parser. Each violation carries a `rule`
/// field naming the scanner that fired so the repair loop can point
/// the user at the right rule file.
fn violations_response(
    scope_violations: &[Violation],
    audit_violations: &[AuditViolation],
    dup_violations: &[DupViolation],
    path_label: &str,
) -> Value {
    let mut violations_json: Vec<Value> = Vec::new();
    for v in scope_violations {
        violations_json.push(json!({
            "file": v.file.display().to_string(),
            "line": v.line,
            "phrase": v.phrase,
            "context": v.context,
            "rule": "scope-enumeration",
        }));
    }
    for v in audit_violations {
        violations_json.push(json!({
            "file": v.file.display().to_string(),
            "line": v.line,
            "phrase": v.phrase,
            "context": v.context,
            "rule": "external-input-audit",
        }));
    }
    for v in dup_violations {
        // Shared helper with `plan_check.rs` so the JSON shape
        // stays in sync across both gate callsites.
        violations_json.push(crate::plan_check::duplicate_violation_to_tagged_json(v));
    }
    let total = scope_violations.len() + audit_violations.len() + dup_violations.len();
    // Reuse the message builder from plan_check so both gate
    // callsites render identical wording. plan_extract adds the
    // path-specific "Edit the plan, then re-run /flow:flow-plan"
    // suffix that plan_check's bare-check variant omits.
    let base = crate::plan_check::build_violation_message(
        scope_violations.len(),
        audit_violations.len(),
        dup_violations.len(),
        total,
    );
    json!({
        "status": "error",
        "path": path_label,
        "violations": violations_json,
        "message": format!("{} Edit the plan, then re-run /flow:flow-plan.", base),
    })
}

/// Run phase_complete via mutate_state and return the result JSON.
fn complete_plan_phase(state_path: &Path, root: &Path, branch: &str) -> Result<Value, String> {
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

    // Resume: plan already exists → read file first, then enter/complete phase
    if let Some(plan_rel) = plan_path {
        let plan_abs = root.join(plan_rel);

        // Read plan file FIRST — fail early before any state mutations
        let plan_content = std::fs::read_to_string(&plan_abs)
            .map_err(|e| format!("Could not read plan file: {}", e))?;

        // Gate resume on all three Plan-phase rules. The plan file
        // may have been written by a prior plan-extract run that
        // left `files.plan` set despite violations (see extracted
        // path below); scanning on resume ensures the phase cannot
        // complete until the user edits the plan to satisfy every
        // scanner. See `.claude/rules/scope-enumeration.md`,
        // `.claude/rules/external-input-audit-gate.md`, and
        // `.claude/rules/duplicate-test-coverage.md` for the rules
        // and their opt-out comment vocabularies.
        let scope_violations = scope_scan(&plan_content, &plan_abs);
        let audit_violations = audit_scan(&plan_content, &plan_abs);
        let test_corpus = TestCorpus::from_repo(&root);
        let dup_violations = dup_scan(&plan_content, &plan_abs, &test_corpus);
        if !scope_violations.is_empty()
            || !audit_violations.is_empty()
            || !dup_violations.is_empty()
        {
            return Ok(violations_response(
                &scope_violations,
                &audit_violations,
                &dup_violations,
                "resumed",
            ));
        }

        // Re-count tasks from the post-edit plan file so that
        // `code_tasks_total` reflects any tasks the user added
        // while fixing scope-enumeration violations. The extracted
        // path sets this field BEFORE the scan runs, so a
        // violation-driven edit can invalidate the initial count.
        let task_count_on_resume = count_tasks_any_level(&plan_content);

        // Enter the phase (safe to call if already in_progress — updates timestamps and visit_count)
        mutate_state(&state_path, |state| {
            if !(state.is_object() || state.is_null()) {
                return;
            }
            phase_enter(state, "flow-plan", None);
            if task_count_on_resume > 0 {
                state["code_tasks_total"] = json!(task_count_on_resume);
            }
        })
        .map_err(|e| format!("Failed to enter phase: {}", e))?;

        // Complete the phase
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
    let dag_abs = FlowPaths::new(&root, &branch).dag_file();
    // Derive the relative path from the absolute path so the value
    // stored in state stays in sync with the on-disk location. If
    // `FlowPaths::dag_file()` ever changes its suffix, the state
    // file's `files.dag` entry follows automatically.
    let dag_rel = dag_abs
        .strip_prefix(&root)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| dag_abs.to_string_lossy().into_owned());
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
    let plan_abs = FlowPaths::new(&root, &branch).plan_file();
    // Derive the relative path from the absolute path so the value
    // stored in state stays in sync with the on-disk location.
    let plan_rel = plan_abs
        .strip_prefix(&root)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| plan_abs.to_string_lossy().into_owned());
    std::fs::write(&plan_abs, &promoted)
        .map_err(|e| format!("Failed to write plan file: {}", e))?;

    // Update state: files.plan, code_tasks_total, plan_step.
    // Set files.plan BEFORE the scope-enumeration check so that a
    // failed check leaves the state in a shape the resume path can
    // pick up on the next invocation (the user edits the plan file
    // in place; re-running plan-extract takes the resume path and
    // re-scans). Without this ordering, a violation would unset the
    // plan path and the next run would re-extract from the issue
    // body, clobbering the user's edits.
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

    // Gate completion on all three Plan-phase rules. Any violation
    // blocks phase completion — the model must edit the plan file
    // to satisfy every scanner and re-run. See
    // `.claude/rules/scope-enumeration.md`,
    // `.claude/rules/external-input-audit-gate.md`, and
    // `.claude/rules/duplicate-test-coverage.md` for the rules and
    // their opt-out comment vocabularies.
    let scope_violations = scope_scan(&promoted, &plan_abs);
    let audit_violations = audit_scan(&promoted, &plan_abs);
    let test_corpus = TestCorpus::from_repo(&root);
    let dup_violations = dup_scan(&promoted, &plan_abs, &test_corpus);
    if !scope_violations.is_empty() || !audit_violations.is_empty() || !dup_violations.is_empty() {
        let _ = append_log(
            &root,
            &branch,
            &format!(
                "[Phase 2] plan-extract — plan-check violations (scope {} / audit {} / dup {}) in {} (exit 0)",
                scope_violations.len(),
                audit_violations.len(),
                dup_violations.len(),
                plan_rel
            ),
        );
        return Ok(violations_response(
            &scope_violations,
            &audit_violations,
            &dup_violations,
            "extracted",
        ));
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // --- violations_response ---

    /// All three scanners' violations land in one tagged
    /// `violations[]` array. The `rule` field tells the repair loop
    /// which scanner fired so the user can be pointed at the right
    /// rule file. Duplicate-test-coverage violations additionally
    /// carry `existing_test` and `existing_file` so the caller can
    /// name both the proposed test and its pre-existing twin.
    #[test]
    fn violations_response_aggregates_all_three_scanners_with_rule_tags() {
        let scope = vec![Violation {
            file: PathBuf::from("/tmp/plan.md"),
            line: 10,
            phrase: "every subcommand".to_string(),
            context: "Add guard to every subcommand.".to_string(),
        }];
        let audit = vec![AuditViolation {
            file: PathBuf::from("/tmp/plan.md"),
            line: 20,
            phrase: "panic on empty".to_string(),
            context: "tighten to panic on empty".to_string(),
        }];
        let dup = vec![DupViolation {
            file: PathBuf::from("/tmp/plan.md"),
            line: 30,
            phrase: "duplicate_name_here".to_string(),
            context: "Plan names `duplicate_name_here` as new.".to_string(),
            existing_test: "test_duplicate_name_here".to_string(),
            existing_file: "tests/hooks.rs:1499".to_string(),
        }];
        let resp = violations_response(&scope, &audit, &dup, "extracted");
        assert_eq!(resp["status"], "error");
        assert_eq!(resp["path"], "extracted");

        let violations = resp["violations"].as_array().expect("array");
        assert_eq!(violations.len(), 3);
        let rules: Vec<String> = violations
            .iter()
            .map(|v| v["rule"].as_str().unwrap_or("").to_string())
            .collect();
        assert!(rules.contains(&"scope-enumeration".to_string()));
        assert!(rules.contains(&"external-input-audit".to_string()));
        assert!(rules.contains(&"duplicate-test-coverage".to_string()));

        // Duplicate entries must carry existing_test and existing_file.
        let dup_entry = violations
            .iter()
            .find(|v| v["rule"].as_str() == Some("duplicate-test-coverage"))
            .expect("dup entry present");
        assert_eq!(
            dup_entry["existing_test"].as_str(),
            Some("test_duplicate_name_here")
        );
        assert_eq!(
            dup_entry["existing_file"].as_str(),
            Some("tests/hooks.rs:1499")
        );

        let msg = resp["message"].as_str().unwrap_or("");
        assert!(msg.contains("3 plan-check violation"));
        assert!(msg.contains("scope-enumeration.md"));
        assert!(msg.contains("external-input-audit-gate.md"));
        assert!(msg.contains("duplicate-test-coverage.md"));
    }

    /// When only the audit scanner finds a violation, the message
    /// names only the audit rule file — not the scope-enumeration
    /// or duplicate-test-coverage rules — so the user is not
    /// misdirected.
    #[test]
    fn violations_response_audit_only_omits_other_rule_messages() {
        let scope: Vec<Violation> = vec![];
        let audit = vec![AuditViolation {
            file: PathBuf::from("/tmp/plan.md"),
            line: 5,
            phrase: "panic on empty".to_string(),
            context: "tighten to panic on empty".to_string(),
        }];
        let dup: Vec<DupViolation> = vec![];
        let resp = violations_response(&scope, &audit, &dup, "resumed");
        let msg = resp["message"].as_str().unwrap_or("");
        assert!(msg.contains("external-input-audit-gate.md"));
        assert!(!msg.contains("scope-enumeration.md"));
        assert!(!msg.contains("duplicate-test-coverage.md"));
        assert_eq!(resp["path"], "resumed");
    }

    // --- load_frozen_config ---

    /// When a frozen phases file exists at the expected path,
    /// `load_frozen_config` returns `(Some(order), Some(commands))`
    /// with values parsed from the file. This exercises lines 117,
    /// 121-122 which are unreached when no frozen file exists.
    #[test]
    fn load_frozen_config_with_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let branch = "test-frozen";
        let flow_states = root.join(".flow-states");
        std::fs::create_dir_all(&flow_states).unwrap();
        let frozen_path = flow_states.join(format!("{}-phases.json", branch));
        let frozen_json = serde_json::json!({
            "order": ["flow-start", "flow-plan"],
            "phases": {
                "flow-start": {"name": "Start", "command": "/flow:flow-start"},
                "flow-plan": {"name": "Plan", "command": "/flow:flow-plan"}
            }
        });
        std::fs::write(&frozen_path, frozen_json.to_string()).unwrap();

        let (order, commands) = load_frozen_config(root, branch);
        assert!(
            order.is_some(),
            "order should be Some when frozen file exists"
        );
        assert!(
            commands.is_some(),
            "commands should be Some when frozen file exists"
        );
        let order = order.unwrap();
        assert_eq!(order.len(), 2);
        assert_eq!(order[0], "flow-start");
    }

    // --- count_tasks_any_level code-block toggle ---

    /// `count_tasks_any_level` must not count task headings inside
    /// fenced code blocks. This exercises lines 327-328 (the
    /// code-block toggle branch).
    #[test]
    fn count_tasks_any_level_skips_code_blocks() {
        let content = "### Task 1: Real task\n\n\
            ```\n\
            ### Task 2: Inside code block\n\
            ```\n\n\
            ### Task 3: Another real task\n";
        assert_eq!(count_tasks_any_level(content), 2);
    }

    /// Duplicate-only response names only the duplicate rule file.
    #[test]
    fn violations_response_duplicate_only_names_only_duplicate_rule() {
        let scope: Vec<Violation> = vec![];
        let audit: Vec<AuditViolation> = vec![];
        let dup = vec![DupViolation {
            file: PathBuf::from("/tmp/plan.md"),
            line: 42,
            phrase: "proposed_dup_name".to_string(),
            context: "Add `proposed_dup_name` as a new test.".to_string(),
            existing_test: "test_proposed_dup_name".to_string(),
            existing_file: "tests/foo.rs:100".to_string(),
        }];
        let resp = violations_response(&scope, &audit, &dup, "extracted");
        let msg = resp["message"].as_str().unwrap_or("");
        assert!(msg.contains("duplicate-test-coverage.md"));
        assert!(!msg.contains("scope-enumeration.md"));
        assert!(!msg.contains("external-input-audit-gate.md"));
    }

    // --- complete_plan_phase error path ---

    /// When `mutate_state` fails (e.g. non-existent state file path),
    /// `complete_plan_phase` returns `Err(String)` via the `.map_err`
    /// closure at line 412.
    #[test]
    fn complete_plan_phase_returns_err_on_missing_state() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let state_path = root.join(".flow-states").join("nonexistent.json");
        let result = complete_plan_phase(&state_path, root, "nonexistent");
        assert!(result.is_err(), "expected Err when state file is missing");
        let err = result.unwrap_err();
        assert!(
            err.contains("Failed to complete phase"),
            "expected map_err message, got: {}",
            err
        );
    }
}
