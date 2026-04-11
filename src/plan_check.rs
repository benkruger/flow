//! Plan-check: scan the current plan file for universal-coverage
//! prose that lacks a named enumeration of concrete siblings.
//!
//! This command is the Plan-phase gate half of the scope-enumeration
//! rule (`.claude/rules/scope-enumeration.md`). Invoked from
//! `skills/flow-plan/SKILL.md` Step 4 before
//! `phase-transition --action complete`, the gate refuses to let the
//! Plan phase finish if the plan file contains a universal-quantifier
//! claim ("every subcommand", "all runners", "each CLI entry point")
//! that is not paired with a named list of the concrete siblings the
//! claim covers. The pre-decomposed path in `src/plan_extract.rs`
//! invokes `scope_enumeration::scan` directly against the promoted
//! plan content before `complete_plan_phase` to gate the same class
//! of mistake for pre-planned issues.
//!
//! Both callsites share `src/scope_enumeration.rs::scan` so the
//! trigger vocabulary and the enumeration-present heuristic cannot
//! drift between the standard skill path and the extracted path.

use std::path::{Path, PathBuf};

use clap::Parser;
use serde_json::{json, Value};

use crate::git::{project_root, resolve_branch};
use crate::output::json_error;
use crate::scope_enumeration::{scan, Violation};

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
/// Returns structured JSON as `Ok(Value)` for all business responses
/// (clean plan, violations found, gate failures, missing plan file).
/// Returns `Err(String)` only for infrastructure failures (unreadable
/// state file, corrupt JSON) — which `run()` converts to exit 1.
///
/// **Read-only — no `cwd_scope::enforce`.** Plan-check only reads the
/// state file and plan file; it does not mutate state. Per
/// `.claude/rules/rust-patterns.md` Guard Universality Across CLI
/// Entry Points, read-only subcommands are exempt from the drift
/// guard — `format-status` and `tombstone-audit` follow the same
/// pattern. The guard exists to prevent silent state drift when a
/// mutating subcommand runs from the wrong subdirectory; plan-check
/// produces no side effects, so a wrong cwd is harmless.
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

    // Scan and produce the response.
    let violations = scan(&content, &plan_path);
    if violations.is_empty() {
        return Ok(json!({"status": "ok"}));
    }

    let violations_json: Vec<Value> = violations.iter().map(violation_to_json).collect();
    Ok(json!({
        "status": "error",
        "violations": violations_json,
        "message": format!(
            "{} universal-coverage claim(s) in the plan file lack a named enumeration. \
             See .claude/rules/scope-enumeration.md for the rule and options.",
            violations.len()
        ),
    }))
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

    let state_path = root.join(".flow-states").join(format!("{}.json", branch));
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

/// Serialize a `Violation` into the JSON shape consumed by the skill.
///
/// The skill renders the returned list inline so the model can edit
/// the plan file at the cited line; keep every field a plain string
/// or number so downstream formatters do not have to unwrap.
fn violation_to_json(v: &Violation) -> Value {
    json!({
        "file": v.file.display().to_string(),
        "line": v.line,
        "phrase": v.phrase,
        "context": v.context,
    })
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

    // --- violation_to_json ---

    #[test]
    fn violation_serializes_all_fields() {
        let v = Violation {
            file: PathBuf::from("/tmp/plan.md"),
            line: 42,
            phrase: "every subcommand".to_string(),
            context: "Add guard to every subcommand.".to_string(),
        };
        let json = violation_to_json(&v);
        assert_eq!(json["file"], "/tmp/plan.md");
        assert_eq!(json["line"], 42);
        assert_eq!(json["phrase"], "every subcommand");
        assert_eq!(json["context"], "Add guard to every subcommand.");
    }
}
