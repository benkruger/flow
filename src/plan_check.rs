//! Plan-check: scan the current plan file for Plan-phase rule
//! violations.
//!
//! This command is the Plan-phase gate that aggregates three scanners
//! — `src/scope_enumeration.rs::scan` (universal-coverage prose
//! without a named sibling list), `src/external_input_audit.rs::scan`
//! (panic/assert tightening proposals without a paired callsite
//! audit table), and
//! `src/duplicate_test_coverage.rs::scan` (proposed test names that
//! collide with existing tests in the corpus). Invoked from
//! `skills/flow-plan/SKILL.md` Step 4 before
//! `phase-transition --action complete`, the gate refuses to let the
//! Plan phase finish if any scanner finds violations. Each violation
//! in the JSON response carries a `rule` field naming which scanner
//! fired, so the skill's repair loop can point the author at the
//! right fix. The extracted and resume paths in
//! `src/plan_extract.rs` invoke the same three scanners against the
//! promoted plan content before `complete_plan_phase` to gate the
//! same class of mistakes for pre-planned issues.
//!
//! All four callsites (standard, extracted, resume, and the
//! committed-prose contract test) share the same scanner modules so
//! the trigger vocabulary, opt-out grammars, and rule tagging cannot
//! drift between the three paths.

use std::path::{Path, PathBuf};

use clap::Parser;
use serde_json::{json, Value};

use crate::cli_output_contract_scanner::{self};
use crate::deletion_sweep_scanner::{self};
use crate::duplicate_test_coverage::{self, TestCorpus};
use crate::external_input_audit;
use crate::flow_paths::FlowPaths;
use crate::git::{project_root, resolve_branch};
use crate::scope_enumeration::scan;

/// CLI arguments for the plan-check subcommand.
#[derive(Parser, Debug)]
#[command(name = "plan-check")]
pub struct Args {
    /// Override branch for state file lookup (default: current branch).
    #[arg(long)]
    pub branch: Option<String>,

    /// Override plan file path (default: resolved from `files.plan`
    /// in the state file). Accepts absolute or worktree-relative paths.
    #[arg(long)]
    pub plan_file: Option<String>,
}

/// Fallible entry point for plan-check.
///
/// ## Return value and exit code convention
///
/// This function returns `Ok(Value)` for **all** business responses,
/// including success and all user-actionable errors. The `run()`
/// wrapper prints the returned JSON and exits with code 0. Business
/// responses include:
///
/// - `{"status": "ok"}` — clean plan
/// - `{"status": "error", "violations": [...]}` — violations found
/// - `{"status": "error", "message": "No state file found..."}`
/// - `{"status": "error", "message": "State file has no plan file set..."}`
/// - `{"status": "error", "message": "Plan file not found..."}`
///
/// The SKILL.md caller branches on the `status` field; the shell
/// exit code stays 0 for all of these so bash pipelines do not
/// abort on a recoverable error.
///
/// `Err(String)` is reserved for **infrastructure failures only**:
/// unreadable state file, corrupt JSON, I/O errors other than
/// `NotFound`. Those cases exit with code 1 via `run()` — the
/// `process::exit(1)` is explicit there, not implicit in `run_impl`.
/// This matches the `plan_extract.rs` idiom used across every other
/// `bin/flow` subcommand.
///
/// ## Read-only — no `cwd_scope::enforce`
///
/// Plan-check only reads the state file and plan file; it does not
/// mutate state. Per `.claude/rules/rust-patterns.md` Guard
/// Universality Across CLI Entry Points, read-only subcommands are
/// exempt from the drift guard — `format-status`, `tombstone-audit`,
/// and now `plan-check` follow the same pattern. The guard exists
/// to prevent silent state drift when a mutating subcommand runs
/// from the wrong subdirectory; plan-check produces no side effects,
/// so a wrong cwd is harmless.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let root = project_root();

    // Resolve the plan file path: --plan-file wins over state file.
    let plan_path = match &args.plan_file {
        Some(path) => resolve_plan_file_override(&root, path),
        None => match resolve_plan_file_from_state(&root, args.branch.as_deref())? {
            Ok(path) => path,
            Err(err_json) => return Ok(err_json),
        },
    };

    // Read the plan file contents. A missing file is a business
    // response (the state may point at a stale path); I/O errors
    // other than NotFound are infrastructure failures.
    let content = match std::fs::read_to_string(&plan_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(json!({
                "status": "error",
                "message": format!(
                    "Plan file not found: {}. The state file points at a path that does not exist.",
                    plan_path.display()
                ),
            }));
        }
        Err(e) => {
            return Err(format!(
                "Could not read plan file {}: {}",
                plan_path.display(),
                e
            ))
        }
    };

    // Run all three Plan-phase scanners and aggregate violations into
    // a single response. Each violation carries a `rule` field so the
    // skill's repair loop can render which rule fired and point the
    // user at the right fix. The scope-enumeration scanner fires on
    // universal-coverage prose without a named sibling list. The
    // external-input-audit scanner fires on panic/assert tightening
    // proposals without a paired callsite source-classification
    // table. The duplicate-test-coverage scanner fires on proposed
    // test names that collide with existing tests in the repo's test
    // corpus (indexed from `tests/**/*.rs` and `src/**/*.rs`).
    let scope_violations = scan(&content, &plan_path);
    let audit_violations = external_input_audit::scan(&content, &plan_path);
    let test_corpus = TestCorpus::from_repo(&root);
    let dup_violations = duplicate_test_coverage::scan(&content, &plan_path, &test_corpus);
    let cli_violations = cli_output_contract_scanner::scan(&content, &plan_path);
    let del_violations = deletion_sweep_scanner::scan(&content, &plan_path);

    if scope_violations.is_empty()
        && audit_violations.is_empty()
        && dup_violations.is_empty()
        && cli_violations.is_empty()
        && del_violations.is_empty()
    {
        return Ok(json!({"status": "ok"}));
    }

    let mut violations_json: Vec<Value> = Vec::new();
    for v in &scope_violations {
        violations_json.push(violation_to_tagged_json(
            &v.file,
            v.line,
            &v.phrase,
            &v.context,
            "scope-enumeration",
        ));
    }
    for v in &audit_violations {
        violations_json.push(violation_to_tagged_json(
            &v.file,
            v.line,
            &v.phrase,
            &v.context,
            "external-input-audit",
        ));
    }
    for v in &dup_violations {
        violations_json.push(duplicate_violation_to_tagged_json(v));
    }
    for v in &cli_violations {
        violations_json.push(cli_output_violation_to_tagged_json(v));
    }
    for v in &del_violations {
        violations_json.push(deletion_sweep_violation_to_tagged_json(v));
    }

    let total = scope_violations.len()
        + audit_violations.len()
        + dup_violations.len()
        + cli_violations.len()
        + del_violations.len();
    let message = build_violation_message(
        scope_violations.len(),
        audit_violations.len(),
        dup_violations.len(),
        cli_violations.len(),
        del_violations.len(),
        total,
    );

    Ok(json!({
        "status": "error",
        "violations": violations_json,
        "message": message,
    }))
}

/// Serialize a duplicate-test-coverage violation with its extra
/// `existing_test` and `existing_file` fields so the skill's repair
/// loop can name both the proposed test and its pre-existing twin.
///
/// Shared with `src/plan_extract.rs::violations_response` — both
/// callsites MUST produce the same JSON shape so the skill's repair
/// loop renders consistent output regardless of which path triggered
/// the failure. If a field is added to
/// `duplicate_test_coverage::Violation`, updating this helper
/// automatically updates both callers.
pub fn duplicate_violation_to_tagged_json(v: &duplicate_test_coverage::Violation) -> Value {
    json!({
        "file": v.file.display().to_string(),
        "line": v.line,
        "phrase": v.phrase,
        "context": v.context,
        "rule": "duplicate-test-coverage",
        "existing_test": v.existing_test,
        "existing_file": v.existing_file,
    })
}

/// Serialize a cli-output-contracts violation with its extra
/// `missing_items` field so the skill's repair loop can name which
/// of the four contract items (output_format, exit_codes,
/// error_messages, fallback) the author still needs to add.
///
/// Shared with `src/plan_extract.rs::violations_response` — both
/// callsites MUST produce the same JSON shape so the skill renders
/// consistent output regardless of which path triggered the failure.
pub fn cli_output_violation_to_tagged_json(
    v: &crate::cli_output_contract_scanner::Violation,
) -> Value {
    json!({
        "file": v.file.display().to_string(),
        "line": v.line,
        "phrase": v.phrase,
        "context": v.context,
        "rule": "cli-output-contracts",
        "missing_items": v.missing_items,
    })
}

/// Serialize a deletion-sweep violation with its extra
/// `identifier` field naming the proposed-for-deletion symbol.
///
/// Shared with `src/plan_extract.rs::violations_response`.
pub fn deletion_sweep_violation_to_tagged_json(
    v: &crate::deletion_sweep_scanner::Violation,
) -> Value {
    json!({
        "file": v.file.display().to_string(),
        "line": v.line,
        "phrase": v.phrase,
        "context": v.context,
        "rule": "deletion-sweep",
        "identifier": v.identifier,
    })
}

/// Build a human-readable summary message that names each scanner's
/// count when non-zero. The message must tell the author which rule
/// file to consult for each violation class.
///
/// Shared with `src/plan_extract.rs::violations_response` — both
/// callsites MUST produce the same message shape so the skill's
/// repair loop renders consistent output regardless of which path
/// triggered the failure. `pub(crate)` so `plan_extract.rs` can
/// call it directly.
pub fn build_violation_message(
    scope_count: usize,
    audit_count: usize,
    dup_count: usize,
    cli_count: usize,
    del_count: usize,
    total: usize,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if scope_count > 0 {
        parts.push(format!(
            "{} universal-coverage claim(s) lack a named enumeration (see \
             .claude/rules/scope-enumeration.md)",
            scope_count
        ));
    }
    if audit_count > 0 {
        parts.push(format!(
            "{} panic/assert tightening(s) lack a callsite audit table (see \
             .claude/rules/external-input-audit-gate.md)",
            audit_count
        ));
    }
    if dup_count > 0 {
        parts.push(format!(
            "{} duplicate-test-coverage violation(s): proposed test name(s) \
             collide with existing tests (see \
             .claude/rules/duplicate-test-coverage.md)",
            dup_count
        ));
    }
    if cli_count > 0 {
        parts.push(format!(
            "{} cli-output-contract violation(s): new flag/subcommand proposal(s) \
             lack the four-item contract block (see \
             .claude/rules/cli-output-contracts.md)",
            cli_count
        ));
    }
    if del_count > 0 {
        parts.push(format!(
            "{} deletion-sweep violation(s): delete/rename proposal(s) lack \
             nearby sweep evidence (see \
             .claude/rules/docs-with-behavior.md \"Scope Enumeration (Rename Side)\")",
            del_count
        ));
    }
    format!("{} plan-check violation(s): {}.", total, parts.join("; "))
}

/// Serialize an arbitrary violation shape into tagged JSON. The
/// Violation types from `scope_enumeration` and
/// `external_input_audit` have identical field layouts but are
/// distinct types; this helper takes field-level inputs so it can
/// serialize either source without a trait dance.
fn violation_to_tagged_json(
    file: &Path,
    line: usize,
    phrase: &str,
    context: &str,
    rule: &str,
) -> Value {
    json!({
        "file": file.display().to_string(),
        "line": line,
        "phrase": phrase,
        "context": context,
        "rule": rule,
    })
}

/// Resolve a `--plan-file` override against the project root.
///
/// Absolute paths pass through unchanged; relative paths are joined
/// onto the project root so the command behaves the same regardless
/// of the caller's cwd.
pub fn resolve_plan_file_override(root: &Path, path: &str) -> PathBuf {
    let as_path = Path::new(path);
    if as_path.is_absolute() {
        as_path.to_path_buf()
    } else {
        root.join(as_path)
    }
}

/// Resolve the plan file path from the state file's `files.plan`
/// field (with legacy fallback to top-level `plan_file`).
///
/// Outer `Result` captures infrastructure failures (I/O, JSON parse
/// errors — become `Err` → exit 1). Inner `Result` captures business
/// failures (missing state, missing field — become `Ok(Value)` →
/// exit 0 with `"status":"error"`).
pub fn resolve_plan_file_from_state(
    root: &Path,
    branch_override: Option<&str>,
) -> Result<Result<PathBuf, Value>, String> {
    let branch = match resolve_branch(branch_override, root) {
        Some(b) => b,
        None => {
            return Ok(Err(json!({
                "status": "error",
                "message": "Could not determine current branch. Pass --branch explicitly.",
            })));
        }
    };

    // Use `try_new` instead of `new` because `resolve_branch` can
    // return raw git refs (`feature/foo`, `dependabot/*`) when no
    // state file exists for the current branch — `FlowPaths::new`
    // panics on slashes. This is the exact failure mode PR #1054
    // introduced for hooks, which this PR was designed to prevent
    // at the planning stage. Treat an invalid branch the same as a
    // missing state file: the command is being run outside an
    // active flow, so there is no plan to check.
    let state_path = match FlowPaths::try_new(root, &branch) {
        Some(paths) => paths.state_file(),
        None => {
            return Ok(Err(json!({
                "status": "error",
                "message": format!(
                    "No active FLOW flow on branch '{}' (branch is not a valid FLOW branch name). \
                     Pass --branch or --plan-file explicitly to check a specific plan.",
                    branch
                ),
            })));
        }
    };
    if !state_path.exists() {
        return Ok(Err(json!({
            "status": "error",
            "message": format!("No state file found: {}", state_path.display()),
        })));
    }

    let state_content = std::fs::read_to_string(&state_path)
        .map_err(|e| format!("Could not read state file: {}", e))?;
    let state: Value = serde_json::from_str(&state_content)
        .map_err(|e| format!("Invalid JSON in state file: {}", e))?;

    // Prefer `files.plan`, fall back to the legacy top-level `plan_file`.
    let plan_rel = state
        .get("files")
        .and_then(|f| f.get("plan"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            state
                .get("plan_file")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        });

    match plan_rel {
        Some(rel) => Ok(Ok(root.join(rel))),
        None => Ok(Err(json!({
            "status": "error",
            "message": "State file has no plan file set (files.plan is null). \
                        Run /flow:flow-plan Step 4 to write the plan file first.",
        }))),
    }
}
