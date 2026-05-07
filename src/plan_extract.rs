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

use crate::cli_output_contract_scanner::{scan as cli_scan, Violation as CliViolation};
use crate::commands::log::append_log;
use crate::deletion_sweep_scanner::{scan as del_scan, Violation as DelViolation};
use crate::duplicate_test_coverage::{scan as dup_scan, TestCorpus, Violation as DupViolation};
use crate::external_input_audit::{scan as audit_scan, Violation as AuditViolation};
use crate::flow_paths::FlowPaths;
use crate::git::{project_root, resolve_branch};
use crate::lock::mutate_state;
use crate::phase_config::load_phase_config;
use crate::phase_transition::{phase_complete, phase_enter};
use crate::render_pr_body::render_body;
use crate::scope_enumeration::{scan as scope_scan, Violation};
use crate::tombstone_checklist_scanner::{scan as tomb_scan, Violation as TombViolation};
use crate::update_pr_body::gh_set_body;
use crate::utils::extract_issue_numbers;
use crate::verify_references_scanner::{
    scan as verify_scan, DefinitionIndex, Violation as VerifyViolation,
};

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

/// Resolve the project root and state file path for the given branch.
///
/// Returns (root, branch_name, state_path). Returns Err if branch cannot
/// be resolved or state file does not exist.
fn resolve_state(args: &Args, root: PathBuf) -> Result<(PathBuf, String, PathBuf), Value> {
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
    // Spawn failure, non-zero exit, and JSON parse failure all fold
    // to the same `None` result through the Option method chain.
    // `filter`'s predicate runs only on Ok+success, so the non-zero
    // exit branch is the filter drop; the spawn-failure branch is
    // `.ok()` converting Err→None; JSON parse failure is
    // `from_slice(...).ok()` returning None.
    Command::new("gh")
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
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice(&o.stdout).ok())
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
    // Body-starts-with-heading check, collapsed to a single branch via
    // `is_some_and` so the "strip_prefix succeeded but not terminated"
    // fall-through region doesn't require its own test fixture.
    if body
        .strip_prefix(heading)
        .is_some_and(is_heading_terminated)
    {
        return Some(0);
    }
    // Search for \n followed by heading
    let search = format!("\n{}", heading);
    let mut start = 0;
    while let Some(pos) = body[start..].find(&search) {
        let abs_pos = start + pos + 1; // +1 to skip the \n
        let after_heading = abs_pos + heading.len();
        if is_heading_terminated(&body[after_heading..]) {
            return Some(abs_pos);
        }
        start = abs_pos;
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
fn extract_implementation_plan(body: &str) -> Option<String> {
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
fn promote_headings(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut in_code_block = false;

    for line in content.lines() {
        let trimmed = line.trim_start();
        // Track fenced code blocks (backtick and tilde per CommonMark)
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
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

        // Promote headings: remove one leading # if line starts with ## or more.
        // `trimmed.starts_with("####")` ⇒ `line.find("####")` returns Some
        // because `line` contains `trimmed` verbatim (same bytes after
        // `trim_start`). `.expect` does not create an instrumented
        // branch per `.claude/rules/testability-means-simplicity.md`.
        if trimmed.starts_with("####") {
            let pos = line
                .find("####")
                .expect("trimmed.starts_with(####) implies line.find(####) is Some");
            result.push_str(&line[..pos]);
            result.push_str(&line[pos + 1..]);
        } else if trimmed.starts_with("###") {
            let pos = line
                .find("###")
                .expect("trimmed.starts_with(###) implies line.find(###) is Some");
            result.push_str(&line[..pos]);
            result.push_str(&line[pos + 1..]);
        } else {
            result.push_str(line);
        }
        result.push('\n');
    }

    // Remove trailing newline added by the loop. `promote_headings`
    // is only called with trimmed content from
    // `extract_implementation_plan`, so `content` never ends with
    // '\n' — the guard is unreachable by construction and elided
    // per `.claude/rules/testability-means-simplicity.md`. `pop` on
    // an empty string (empty-content edge case) is a safe no-op.
    result.pop();
    result
}

/// Plan-phase Gate 4 — plan-extract truncation gate.
///
/// Detects whether the issue body fed to plan-extract was
/// truncated mid-content. The most reliable signal is an unclosed
/// fenced code block at EOF — when the issue body cuts off inside
/// a fenced block, the issue was truncated. Returns
/// `Some((expected, actual))` when truncation is detected, `None`
/// when the content is intact.
///
/// `expected` and `actual` are the source `#### Task N:` count
/// and the post-promotion `### Task N:` count respectively. The
/// promote_headings transform is deterministic, so a mismatch
/// indicates either an unclosed fenced block at EOF (which
/// suppressed task headings) or some other parse failure.
///
/// The check is conservative: only an unclosed fence at EOF or a
/// task-count mismatch fires the gate. False positives would
/// block legitimate plans, so the gate accepts ambiguous cases.
pub fn detect_truncation(source: &str, promoted: &str) -> Option<(usize, usize)> {
    let expected = count_tasks(source);
    let actual = count_tasks_after_promotion(promoted);
    if has_unclosed_fence(source) || has_unclosed_fence(promoted) {
        return Some((expected, actual));
    }
    if expected != actual {
        return Some((expected, actual));
    }
    None
}

/// Count `### Task N:` headings in the post-promotion content.
fn count_tasks_after_promotion(content: &str) -> usize {
    let mut count = 0;
    let mut in_code_block = false;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code_block = !in_code_block;
            continue;
        }
        if !in_code_block && trimmed.starts_with("### Task ") {
            count += 1;
        }
    }
    count
}

/// Returns `true` when the content has an unclosed fenced code
/// block at EOF (an opening ` ``` ` or `~~~` with no matching
/// close).
fn has_unclosed_fence(content: &str) -> bool {
    let mut open: Option<char> = None;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            open = match open {
                Some('`') => None,
                Some(_) => open,
                None => Some('`'),
            };
        } else if trimmed.starts_with("~~~") {
            open = match open {
                Some('~') => None,
                Some(_) => open,
                None => Some('~'),
            };
        }
    }
    open.is_some()
}

/// Count `#### Task N:` headings in the content (pre-promotion format).
fn count_tasks(content: &str) -> usize {
    let mut count = 0;
    let mut in_code_block = false;

    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
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
fn count_tasks_any_level(content: &str) -> usize {
    let mut count = 0;
    let mut in_code_block = false;

    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
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
#[allow(clippy::too_many_arguments)]
fn violations_response(
    scope_violations: &[Violation],
    audit_violations: &[AuditViolation],
    dup_violations: &[DupViolation],
    cli_violations: &[CliViolation],
    del_violations: &[DelViolation],
    tomb_violations: &[TombViolation],
    verify_violations: &[VerifyViolation],
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
    for v in cli_violations {
        // Shared helper with `plan_check.rs` so both callsites
        // render the cli-output-contracts violation shape identically.
        violations_json.push(crate::plan_check::cli_output_violation_to_tagged_json(v));
    }
    for v in del_violations {
        violations_json.push(crate::plan_check::deletion_sweep_violation_to_tagged_json(
            v,
        ));
    }
    for v in tomb_violations {
        violations_json.push(crate::plan_check::tombstone_checklist_violation_to_tagged_json(v));
    }
    for v in verify_violations {
        violations_json.push(crate::plan_check::verify_references_violation_to_tagged_json(v));
    }
    let total = scope_violations.len()
        + audit_violations.len()
        + dup_violations.len()
        + cli_violations.len()
        + del_violations.len()
        + tomb_violations.len()
        + verify_violations.len();
    // Reuse the message builder from plan_check so both gate
    // callsites render identical wording. plan_extract adds the
    // path-specific "Edit the plan, then re-run /flow:flow-plan"
    // suffix that plan_check's bare-check variant omits.
    let base = crate::plan_check::build_violation_message(
        scope_violations.len(),
        audit_violations.len(),
        dup_violations.len(),
        cli_violations.len(),
        del_violations.len(),
        tomb_violations.len(),
        verify_violations.len(),
        total,
    );
    json!({
        "status": "error",
        "path": path_label,
        "violations": violations_json,
        "message": format!("{} Edit the plan, then re-run /flow:flow-plan.", base),
    })
}

/// Single point of state-mutation error formatting.
///
/// Consolidates every `mutate_state(&state_path, ...).map_err(|e|
/// format!("<label>: {}", e))?` pattern in run_impl_with_root so the
/// error-formatting closure has ONE instantiation instead of one per
/// callsite. The `?` propagation still lives at each callsite (Rust's
/// desugar), but the map_err closure that formats the error message
/// is defined once here and shared across every caller — meaning a
/// single test that drives any callsite into the Err arm covers the
/// formatter for all of them.
fn commit_state(
    state_path: &Path,
    err_label: &str,
    transform: &mut dyn FnMut(&mut Value),
) -> Result<(), String> {
    mutate_state(state_path, transform)
        .map(|_| ())
        .map_err(|e| format!("{}: {}", err_label, e))
}

/// Read + parse the state JSON. Consolidates the read+parse pair used
/// by `run_impl_with_root` at startup and after mutations — the
/// `Could not read`/`Invalid JSON` map_err closures now have one
/// instantiation each shared across every caller.
fn read_state_json(state_path: &Path, read_err_label: &str) -> Result<Value, String> {
    let content =
        std::fs::read_to_string(state_path).map_err(|e| format!("{}: {}", read_err_label, e))?;
    serde_json::from_str(&content).map_err(|e| format!("Invalid JSON in state file: {}", e))
}

/// Fallible entry point for plan extraction.
///
/// Returns structured JSON as `Ok(Value)` for all business responses
/// (including status-error responses like gate failures). Returns
/// `Err(String)` only for infrastructure failures (file I/O, lock errors).
pub fn run_impl(args: &Args) -> Result<Value, String> {
    run_impl_with_root(args, project_root())
}

/// Seam-injected variant of [`run_impl`] accepting the project root
/// as a parameter. Production binds `root = project_root()` via the
/// `run_impl` wrapper above; library tests pass a fixture repo path
/// directly so every private helper (resolve_state, gate_check,
/// load_frozen_config, fetch_issue, is_decomposed, read_dag_mode,
/// find_heading, is_heading_terminated, extract_implementation_plan,
/// promote_headings, count_tasks, count_tasks_any_level,
/// violations_response) reaches 100% per
/// `.claude/rules/no-waivers.md` without requiring the test process
/// to chdir. `run_impl` is the non-test production consumer, so
/// this pub seam satisfies the bright-line test in
/// `.claude/rules/test-placement.md`.
pub fn run_impl_with_root(args: &Args, root: PathBuf) -> Result<Value, String> {
    // --- Resolve state file ---
    let (root, branch, state_path) = match resolve_state(args, root) {
        Ok(v) => v,
        Err(err_json) => return Ok(err_json),
    };

    // --- Read state for gate and resume checks ---
    let state = read_state_json(&state_path, "Could not read state file")?;

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
        let cli_violations = cli_scan(&plan_content, &plan_abs);
        let del_violations = del_scan(&plan_content, &plan_abs);
        let tomb_violations = tomb_scan(&plan_content, &plan_abs);
        let verify_index = DefinitionIndex::from_repo(&root);
        let verify_violations = verify_scan(&plan_content, &plan_abs, &verify_index);
        if !scope_violations.is_empty()
            || !audit_violations.is_empty()
            || !dup_violations.is_empty()
            || !cli_violations.is_empty()
            || !del_violations.is_empty()
            || !tomb_violations.is_empty()
            || !verify_violations.is_empty()
        {
            return Ok(violations_response(
                &scope_violations,
                &audit_violations,
                &dup_violations,
                &cli_violations,
                &del_violations,
                &tomb_violations,
                &verify_violations,
                "resumed",
            ));
        }

        // Re-count tasks from the post-edit plan file so that
        // `code_tasks_total` reflects any tasks the user added
        // while fixing scope-enumeration violations. The extracted
        // path sets this field BEFORE the scan runs, so a
        // violation-driven edit can invalidate the initial count.
        let task_count_on_resume = count_tasks_any_level(&plan_content);

        // Single commit_state that both enters and completes the
        // phase in one atomic state write. Consolidated from two
        // prior calls (phase_enter then complete_plan_phase) so the
        // resume path has exactly one `?` Err arm — reachable via a
        // readonly-state + readable-plan fixture.
        // No outer object-guard here: gate_check earlier rejected
        // non-object state, so `state` is guaranteed to be a JSON
        // object.
        let (frozen_order, frozen_commands) = load_frozen_config(&root, &branch);
        let complete_result_holder = std::cell::RefCell::new(Value::Null);
        commit_state(&state_path, "Failed to complete phase", &mut |state| {
            phase_enter(state, "flow-plan", None);
            if task_count_on_resume > 0 {
                state["code_tasks_total"] = json!(task_count_on_resume);
            }
            *complete_result_holder.borrow_mut() = phase_complete(
                state,
                "flow-plan",
                None,
                frozen_order.as_deref(),
                frozen_commands.as_ref(),
            );
        })?;

        let complete_result = complete_result_holder.into_inner();

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

    // --- Issue fetch + decomposed detection ---
    //
    // Phase entry is DEFERRED until after the path is determined so
    // every execution path has exactly one consolidated commit_state
    // call. This keeps the count of `?` Err arms per path at one,
    // and each arm is reachable via a readonly-state fixture that
    // routes execution through the corresponding branch.
    let prompt = state.get("prompt").and_then(|v| v.as_str()).unwrap_or("");

    let issue_numbers = extract_issue_numbers(prompt);
    let dag_mode = read_dag_mode(&state);

    // No issue references → enter phase + return standard path.
    if issue_numbers.is_empty() {
        commit_state(&state_path, "Failed to enter phase", &mut |state| {
            phase_enter(state, "flow-plan", None);
            state["plan_steps_total"] = json!(4);
            state["plan_step"] = json!(1);
        })?;
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

    // No decomposed issue found → enter phase + return standard
    // path with the first fetched issue body as context.
    let issue_data = match decomposed_data {
        Some(data) => data,
        None => {
            commit_state(&state_path, "Failed to enter phase", &mut |state| {
                phase_enter(state, "flow-plan", None);
                state["plan_steps_total"] = json!(4);
                state["plan_step"] = json!(1);
            })?;
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
    // file's `files.dag` entry follows automatically. `FlowPaths::new`
    // constructs dag_file() as `<root>/.flow-states/<branch>-dag.md`
    // so strip_prefix(&root) always succeeds; `.expect` does not
    // create an instrumented branch per
    // `.claude/rules/testability-means-simplicity.md`.
    let dag_rel = dag_abs
        .strip_prefix(&root)
        .expect("FlowPaths::new constructs dag_file under root")
        .to_string_lossy()
        .into_owned();
    std::fs::write(&dag_abs, &dag_content)
        .map_err(|e| format!("Failed to write DAG file: {}", e))?;

    // --- Extract Implementation Plan section ---
    //
    // Phase entry + files.dag update deferred to each branch's
    // terminal commit_state call so the `?` Err arms converge to
    // exactly one per branch. Nested files-guard remains: state.files
    // may legitimately be a non-object from older state files where
    // the field hadn't been initialized as a map.
    let plan_section = match extract_implementation_plan(&issue_body) {
        Some(s) => s,
        None => {
            // No Implementation Plan section — enter phase, record
            // files.dag, and return standard path so the model
            // handles it as an older-format decomposed issue.
            commit_state(&state_path, "Failed to update state", &mut |state| {
                phase_enter(state, "flow-plan", None);
                state["plan_steps_total"] = json!(4);
                state["plan_step"] = json!(2);
                if !matches!(state.get("files"), Some(v) if v.is_object()) {
                    state["files"] = json!({});
                }
                state["files"]["dag"] = json!(&dag_rel);
            })?;
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

    // Plan-phase Gate 4: detect truncation in the issue body
    // before writing the plan file. An unclosed fence at EOF or
    // a task-count mismatch between source and promoted content
    // signals the issue body was truncated mid-content. Return
    // an error response with `truncated: true` so the skill's
    // Fast Path Done can halt auto-advance.
    if let Some((expected, actual)) = detect_truncation(&plan_section, &promoted) {
        let _ = append_log(
            &root,
            &branch,
            &format!(
                "[Phase 2] plan-extract — truncation detected (expected {} tasks, got {}) in issue body (exit 0)",
                expected, actual
            ),
        );
        return Ok(json!({
            "status": "error",
            "path": "extracted",
            "truncated": true,
            "expected_task_count": expected,
            "actual_task_count": actual,
            "message": format!(
                "Plan-extract truncation detected: expected {} tasks, got {}. The issue body appears to be cut off (likely an unclosed fenced code block at EOF). Edit the issue body to restore the missing content, then re-run /flow:flow-plan.",
                expected, actual
            ),
        }));
    }

    // Write plan file
    let plan_abs = FlowPaths::new(&root, &branch).plan_file();
    // Derive the relative path from the absolute path. `FlowPaths::new`
    // constructs plan_file() as `<root>/.flow-states/<branch>-plan.md`,
    // so strip_prefix(&root) always succeeds — same invariant as the
    // dag_file case above. `.expect` does not create an instrumented
    // branch per `.claude/rules/testability-means-simplicity.md`.
    let plan_rel = plan_abs
        .strip_prefix(&root)
        .expect("FlowPaths::new constructs plan_file under root")
        .to_string_lossy()
        .into_owned();
    std::fs::write(&plan_abs, &promoted)
        .map_err(|e| format!("Failed to write plan file: {}", e))?;

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
    let cli_violations = cli_scan(&promoted, &plan_abs);
    let del_violations = del_scan(&promoted, &plan_abs);
    let tomb_violations = tomb_scan(&promoted, &plan_abs);
    let verify_index = DefinitionIndex::from_repo(&root);
    let verify_violations = verify_scan(&promoted, &plan_abs, &verify_index);
    if !scope_violations.is_empty()
        || !audit_violations.is_empty()
        || !dup_violations.is_empty()
        || !cli_violations.is_empty()
        || !del_violations.is_empty()
        || !tomb_violations.is_empty()
        || !verify_violations.is_empty()
    {
        // Violations: enter phase + record files + return. Single
        // commit_state so the `?` Err arm is reachable via a
        // readonly-state fixture routed through this branch. Setting
        // files.plan BEFORE returning the violation response leaves
        // state in a shape the resume path can pick up on the next
        // invocation — the user edits the plan file in place,
        // re-running plan-extract takes the resume path and re-scans.
        commit_state(&state_path, "Failed to update state", &mut |state| {
            phase_enter(state, "flow-plan", None);
            state["plan_steps_total"] = json!(4);
            state["plan_step"] = json!(3);
            if !matches!(state.get("files"), Some(v) if v.is_object()) {
                state["files"] = json!({});
            }
            state["files"]["dag"] = json!(&dag_rel);
            state["files"]["plan"] = json!(&plan_rel);
            state["code_tasks_total"] = json!(task_count);
        })?;
        let _ = append_log(
            &root,
            &branch,
            &format!(
                "[Phase 2] plan-extract — plan-check violations (scope {} / audit {} / dup {} / cli {} / del {} / tomb {} / verify {}) in {} (exit 0)",
                scope_violations.len(),
                audit_violations.len(),
                dup_violations.len(),
                cli_violations.len(),
                del_violations.len(),
                tomb_violations.len(),
                verify_violations.len(),
                plan_rel
            ),
        );
        return Ok(violations_response(
            &scope_violations,
            &audit_violations,
            &dup_violations,
            &cli_violations,
            &del_violations,
            &tomb_violations,
            &verify_violations,
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

    // Happy path: single atomic commit_state enters the phase,
    // records every file + task count, advances plan_step to 4, and
    // completes the phase. Consolidating these into one closure
    // means the `?` Err arm is reachable via a readonly-state
    // fixture routed through this branch, and the caller
    // (`run_impl_with_root`) captures the mutated state directly
    // from the closure so no post-mutation re-read is needed.
    let (frozen_order, frozen_commands) = load_frozen_config(&root, &branch);
    let complete_result_holder = std::cell::RefCell::new(Value::Null);
    let updated_state_holder = std::cell::RefCell::new(Value::Null);
    commit_state(&state_path, "Failed to complete phase", &mut |state| {
        phase_enter(state, "flow-plan", None);
        state["plan_steps_total"] = json!(4);
        state["plan_step"] = json!(4);
        if !matches!(state.get("files"), Some(v) if v.is_object()) {
            state["files"] = json!({});
        }
        state["files"]["dag"] = json!(&dag_rel);
        state["files"]["plan"] = json!(&plan_rel);
        state["code_tasks_total"] = json!(task_count);
        *complete_result_holder.borrow_mut() = phase_complete(
            state,
            "flow-plan",
            None,
            frozen_order.as_deref(),
            frozen_commands.as_ref(),
        );
        *updated_state_holder.borrow_mut() = state.clone();
    })?;

    let complete_result = complete_result_holder.into_inner();
    let updated_state = updated_state_holder.into_inner();

    // --- PR body render ---
    let pr = args
        .pr
        .or_else(|| updated_state.get("pr_number").and_then(|v| v.as_i64()));

    // render_body cannot fail at this point: prompt is guaranteed non-empty
    // (extract_issue_numbers returned a non-empty set upstream), and the
    // plan/DAG files were just written by this function. The `.expect()`
    // encodes that invariant and avoids an else-branch that no test can
    // reach inside this call path.
    if let Some(pr_number) = pr {
        let body = render_body(&updated_state, &root)
            .expect("render_body succeeds after successful plan write with non-empty prompt");
        let _ = gh_set_body(pr_number, &body);
    }

    let _ = append_log(
        &root,
        &branch,
        "[Phase 2] plan-extract — PR body rendered (exit 0)",
    );

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
