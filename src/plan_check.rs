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

use crate::duplicate_test_coverage::{self, TestCorpus};
use crate::external_input_audit;
use crate::flow_paths::FlowPaths;
use crate::git::{project_root, resolve_branch};
use crate::output::json_error;
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

    if scope_violations.is_empty() && audit_violations.is_empty() && dup_violations.is_empty() {
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

    let total = scope_violations.len() + audit_violations.len() + dup_violations.len();
    let message = build_violation_message(
        scope_violations.len(),
        audit_violations.len(),
        dup_violations.len(),
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
pub(crate) fn duplicate_violation_to_tagged_json(v: &duplicate_test_coverage::Violation) -> Value {
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

/// Build a human-readable summary message that names each scanner's
/// count when non-zero. The message must tell the author which rule
/// file to consult for each violation class.
///
/// Shared with `src/plan_extract.rs::violations_response` — both
/// callsites MUST produce the same message shape so the skill's
/// repair loop renders consistent output regardless of which path
/// triggered the failure. `pub(crate)` so `plan_extract.rs` can
/// call it directly.
pub(crate) fn build_violation_message(
    scope_count: usize,
    audit_count: usize,
    dup_count: usize,
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
fn resolve_plan_file_override(root: &Path, path: &str) -> PathBuf {
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
fn resolve_plan_file_from_state(
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

#[cfg(test)]
mod tests {
    use super::*;

    // --- resolve_plan_file_override ---

    #[test]
    fn absolute_override_passes_through() {
        let root = Path::new("/tmp/fake-root");
        let resolved = resolve_plan_file_override(root, "/tmp/other/plan.md");
        assert_eq!(resolved, PathBuf::from("/tmp/other/plan.md"));
    }

    #[test]
    fn relative_override_joins_project_root() {
        let root = Path::new("/tmp/fake-root");
        let resolved = resolve_plan_file_override(root, "custom/plan.md");
        assert_eq!(resolved, PathBuf::from("/tmp/fake-root/custom/plan.md"));
    }

    // --- violation_to_tagged_json ---

    #[test]
    fn violation_serializes_all_fields_with_rule_tag() {
        let path = PathBuf::from("/tmp/plan.md");
        let json = violation_to_tagged_json(
            &path,
            42,
            "every subcommand",
            "Add guard to every subcommand.",
            "scope-enumeration",
        );
        assert_eq!(json["file"], "/tmp/plan.md");
        assert_eq!(json["line"], 42);
        assert_eq!(json["phrase"], "every subcommand");
        assert_eq!(json["context"], "Add guard to every subcommand.");
        assert_eq!(json["rule"], "scope-enumeration");
    }

    #[test]
    fn violation_to_tagged_json_carries_audit_rule_label() {
        let path = PathBuf::from("/tmp/plan.md");
        let json = violation_to_tagged_json(
            &path,
            10,
            "panic on empty",
            "tighten to panic on empty",
            "external-input-audit",
        );
        assert_eq!(json["rule"], "external-input-audit");
    }

    // --- build_violation_message ---

    #[test]
    fn message_names_only_scope_when_audit_count_is_zero() {
        let m = build_violation_message(2, 0, 0, 2);
        assert!(m.contains("2 universal-coverage"));
        assert!(m.contains("scope-enumeration.md"));
        assert!(!m.contains("panic/assert"));
        assert!(!m.contains("duplicate-test-coverage"));
    }

    #[test]
    fn message_names_only_audit_when_scope_count_is_zero() {
        let m = build_violation_message(0, 3, 0, 3);
        assert!(m.contains("3 panic/assert"));
        assert!(m.contains("external-input-audit-gate.md"));
        assert!(!m.contains("universal-coverage"));
    }

    #[test]
    fn message_names_only_duplicate_when_others_are_zero() {
        let m = build_violation_message(0, 0, 2, 2);
        assert!(m.contains("2 duplicate-test-coverage"));
        assert!(m.contains("duplicate-test-coverage.md"));
        assert!(!m.contains("universal-coverage"));
        assert!(!m.contains("panic/assert"));
    }

    #[test]
    fn message_names_all_three_rules_when_all_have_violations() {
        let m = build_violation_message(2, 1, 3, 6);
        assert!(m.contains("2 universal-coverage"));
        assert!(m.contains("1 panic/assert"));
        assert!(m.contains("3 duplicate-test-coverage"));
        assert!(m.contains("scope-enumeration.md"));
        assert!(m.contains("external-input-audit-gate.md"));
        assert!(m.contains("duplicate-test-coverage.md"));
        assert!(m.contains("6 plan-check violation"));
    }

    // --- run_impl three-scanner aggregation ---

    /// All three scanners run inside `run_impl` with `--plan-file`
    /// override, and the response aggregates violations with the
    /// correct `rule` tag per violation. Note the duplicate-test-
    /// coverage scanner indexes the running repo's own test corpus;
    /// this fixture plan deliberately names no existing test to keep
    /// only scope + audit violations in scope. The duplicate-scanner
    /// integration test below uses a different fixture that does
    /// name an existing test.
    #[test]
    fn run_impl_aggregates_violations_from_both_scanners() {
        let tmp = std::env::temp_dir().join(format!("plan-check-dual-{}.md", std::process::id()));
        let plan_content = "## Approach\n\n\
            Add the drift guard to every state mutator.\n\n\
            tighten FlowPaths::new to panic on empty branches.\n";
        std::fs::write(&tmp, plan_content).expect("write fixture plan");

        let args = Args {
            branch: None,
            plan_file: Some(tmp.to_string_lossy().to_string()),
        };
        let result = run_impl(&args).expect("run_impl returns business response");
        let _ = std::fs::remove_file(&tmp);

        assert_eq!(result["status"], "error");
        let violations = result["violations"]
            .as_array()
            .expect("violations is an array");
        let rules: Vec<String> = violations
            .iter()
            .map(|v| v["rule"].as_str().unwrap_or("").to_string())
            .collect();
        assert!(
            rules.iter().any(|r| r == "scope-enumeration"),
            "expected at least one scope-enumeration violation, got rules: {:?}",
            rules
        );
        assert!(
            rules.iter().any(|r| r == "external-input-audit"),
            "expected at least one external-input-audit violation, got rules: {:?}",
            rules
        );
        assert!(
            result["message"]
                .as_str()
                .unwrap_or("")
                .contains("plan-check violation"),
            "message must summarize total: {:?}",
            result["message"]
        );
    }

    /// Hermetic unit test for the duplicate-test-coverage JSON
    /// serialization. Builds a synthetic `Violation` from the
    /// scanner module and serializes it via the shared helper, so
    /// the test does not depend on the live test corpus and will
    /// not break if any specific test in the repo is renamed. The
    /// end-to-end scanner-against-corpus behavior is covered by
    /// the hermetic integration tests in
    /// `src/duplicate_test_coverage.rs` that build a `TestCorpus`
    /// from a `TempDir` fixture.
    #[test]
    fn duplicate_violation_json_shape_has_all_required_fields() {
        let v = duplicate_test_coverage::Violation {
            file: PathBuf::from("/tmp/plan.md"),
            line: 42,
            phrase: "foo_bar_baz_quux".to_string(),
            context: "Plan names `foo_bar_baz_quux` as a new test.".to_string(),
            existing_test: "test_foo_bar_baz_quux".to_string(),
            existing_file: "tests/hooks.rs:1499".to_string(),
        };
        let json = duplicate_violation_to_tagged_json(&v);
        assert_eq!(json["file"], "/tmp/plan.md");
        assert_eq!(json["line"], 42);
        assert_eq!(json["phrase"], "foo_bar_baz_quux");
        assert_eq!(
            json["context"],
            "Plan names `foo_bar_baz_quux` as a new test."
        );
        assert_eq!(json["rule"], "duplicate-test-coverage");
        assert_eq!(json["existing_test"], "test_foo_bar_baz_quux");
        assert_eq!(json["existing_file"], "tests/hooks.rs:1499");
    }

    /// Regression: `run_impl` must NOT panic when the current git
    /// branch contains a `/` (e.g. `feature/foo`, `dependabot/*`).
    /// Pre-mortem caught that the original code used
    /// `FlowPaths::new` on the resolved branch, which panics on
    /// slashes — the same failure mode PR #1054 introduced for
    /// hooks. The fallible `try_new` variant is now used so an
    /// invalid-for-FLOW branch name is treated as "no active flow"
    /// instead of crashing the command.
    #[test]
    fn run_impl_does_not_panic_on_slash_branch() {
        let args = Args {
            branch: Some("feature/foo".to_string()),
            plan_file: None,
        };
        let result = run_impl(&args).expect("run_impl returns business response");
        // The exact message depends on whether a state file exists
        // at the FlowPaths path for "feature/foo" — which it
        // cannot, because try_new rejects the branch before the
        // state path is even constructed. Either way, the process
        // must not panic and the response must be a business
        // "error" status so the skill sees a clean JSON error.
        assert_eq!(result["status"], "error");
    }

    /// Clean plan (no violations) returns `{"status": "ok"}` from
    /// the dual-scanner aggregation path.
    #[test]
    fn run_impl_returns_ok_when_both_scanners_clean() {
        let tmp = std::env::temp_dir().join(format!("plan-check-clean-{}.md", std::process::id()));
        std::fs::write(&tmp, "## Approach\n\nA plain plan with no triggers.\n")
            .expect("write fixture plan");

        let args = Args {
            branch: None,
            plan_file: Some(tmp.to_string_lossy().to_string()),
        };
        let result = run_impl(&args).expect("run_impl returns business response");
        let _ = std::fs::remove_file(&tmp);

        assert_eq!(result["status"], "ok");
    }
}
