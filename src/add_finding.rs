//! `bin/flow add-finding` — record a triage finding in FLOW state.
//!
//! Tests live at `tests/add_finding.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` in
//! this file.

use std::path::Path;

use clap::Parser;
use serde_json::{json, Value};

use crate::flow_paths::FlowPaths;
use crate::git::resolve_branch;
use crate::lock::mutate_state;
use crate::phase_config::phase_names;
use crate::utils::now;

/// Valid outcome values for findings.
const VALID_OUTCOMES: &[&str] = &[
    "fixed",
    "filed",
    "dismissed",
    "rule_written",
    "rule_clarified",
];

#[derive(Parser, Debug)]
#[command(name = "add-finding", about = "Record a triage finding in FLOW state")]
pub struct Args {
    /// Finding description
    #[arg(long)]
    pub finding: String,

    /// Reason for the triage outcome
    #[arg(long)]
    pub reason: String,

    /// Triage outcome (fixed, filed, dismissed, rule_written, rule_clarified)
    #[arg(long)]
    pub outcome: String,

    /// Phase that produced the finding
    #[arg(long)]
    pub phase: String,

    /// Issue URL (when outcome is filed)
    #[arg(long)]
    pub issue_url: Option<String>,

    /// Rule path (when outcome is rule_written or rule_clarified)
    #[arg(long)]
    pub path: Option<String>,

    /// Override branch for state file lookup
    #[arg(long)]
    pub branch: Option<String>,
}

/// Outcomes the Code Review phase accepts. The gate enforces this as a
/// positive allowlist so any outcome beyond the two-outcome triage model
/// (Real → fixed, False positive → dismissed) is rejected — including
/// new outcomes that might be added to `VALID_OUTCOMES` in the future.
const CODE_REVIEW_ALLOWED_OUTCOMES: &[&str] = &["fixed", "dismissed"];

/// Returns a rejection message when the (outcome, phase) tuple violates
/// the Code Review filing ban. Inputs are normalized (trimmed, NULs
/// stripped, ASCII-lowercased) so whitespace or case drift in CLI args
/// cannot bypass the gate.
///
/// During Code Review, only outcomes in `CODE_REVIEW_ALLOWED_OUTCOMES`
/// pass. Any other outcome (including `"filed"`, and any outcome added
/// to `VALID_OUTCOMES` later that semantically means "defer") is
/// rejected. Other phases pass unchanged.
///
/// See `.claude/rules/code-review-scope.md` — Code Review triage has
/// two outcomes (Real / False positive); there is no filing path.
fn code_review_filing_gate(outcome: &str, phase: &str) -> Option<String> {
    let phase_norm = normalize_gate_input(phase);
    if phase_norm != "flow-code-review" {
        return None;
    }
    let outcome_norm = normalize_gate_input(outcome);
    if CODE_REVIEW_ALLOWED_OUTCOMES.contains(&outcome_norm.as_str()) {
        return None;
    }
    Some(format!(
        "Outcome '{}' is not valid for phase 'flow-code-review'. \
         Code Review triage has two outcomes: 'fixed' (real findings, \
         fix in Step 4) and 'dismissed' (false positives). All real \
         findings are fixed during Code Review — there is no filing \
         path.",
        outcome
    ))
}

/// Strip NULs and surrounding whitespace, then lowercase. Used by the
/// gate so that whitespace/case/NUL variants of "filed" or
/// "flow-code-review" cannot bypass the check.
fn normalize_gate_input(s: &str) -> String {
    s.replace('\0', "").trim().to_ascii_lowercase()
}

/// Fallible implementation with injected root/cwd — returns
/// `Ok(finding_count)` on success, `Err("no_state")` when no state file
/// exists, or `Err(message)` on failure. Tests pass tempdir paths;
/// production wraps via [`run_impl`]. Branch resolution is delegated
/// to a closure so tests can drive the `None` arm without mocking git.
pub fn run_impl_with_root(args: &Args, root: &Path, cwd: &Path) -> Result<usize, String> {
    run_impl_with_root_resolver(args, root, cwd, &resolve_branch)
}

/// Seam-injected variant of [`run_impl_with_root`] — accepts a
/// custom branch resolver closure. Production uses
/// [`resolve_branch`]; tests pass a closure returning `None` to
/// exercise the "Could not determine current branch" arm without
/// mocking git or mutating process env vars.
pub fn run_impl_with_root_resolver(
    args: &Args,
    root: &Path,
    cwd: &Path,
    resolver: &dyn Fn(Option<&str>, &Path) -> Option<String>,
) -> Result<usize, String> {
    if !VALID_OUTCOMES.contains(&args.outcome.as_str()) {
        return Err(format!(
            "Invalid outcome '{}'. Valid: {}",
            args.outcome,
            VALID_OUTCOMES.join(", ")
        ));
    }

    if let Some(msg) = code_review_filing_gate(&args.outcome, &args.phase) {
        return Err(msg);
    }

    // Drift guard: state mutations must happen from inside the
    // subdirectory the flow was started in. Without this, a user who
    // cds out of an `api/`-scoped flow into `ios/` could record
    // findings against the wrong subtree. See
    // [`crate::cwd_scope::enforce`].
    crate::cwd_scope::enforce(cwd, root)?;

    let branch = match resolver(args.branch.as_deref(), root) {
        Some(b) => b,
        None => return Err("Could not determine current branch".to_string()),
    };
    // Branch reaches us either from `current_branch()` (raw git output)
    // or from `--branch` CLI override (raw user input). Both are
    // external inputs per `.claude/rules/external-input-validation.md`,
    // so use the fallible constructor to reject slash-containing or
    // empty branches as a structured error rather than a panic.
    let state_path = match FlowPaths::try_new(root, &branch) {
        Some(p) => p.state_file(),
        None => return Err(format!("Invalid branch '{}'", branch)),
    };

    if !state_path.exists() {
        return Err("no_state".to_string());
    }

    let names = phase_names();
    let phase_name = match names.get(&args.phase) {
        Some(n) => n.clone(),
        None => args.phase.clone(),
    };
    let timestamp = now();

    let state = mutate_state(&state_path, &mut |state| {
        if !(state.is_object() || state.is_null()) {
            return;
        }
        if state.get("findings").is_none() || !state["findings"].is_array() {
            state["findings"] = json!([]);
        }
        // The block above guarantees state["findings"] is an array, so
        // as_array_mut returns Some unconditionally — no defensive
        // if-let needed.
        let arr = state["findings"]
            .as_array_mut()
            .expect("findings is always an array here");
        let mut entry = json!({
            "finding": args.finding,
            "reason": args.reason,
            "outcome": args.outcome,
            "phase": args.phase,
            "phase_name": phase_name,
            "timestamp": timestamp,
        });
        if let Some(ref url) = args.issue_url {
            entry["issue_url"] = json!(url);
        }
        if let Some(ref path) = args.path {
            entry["path"] = json!(path);
        }
        arr.push(entry);
    });
    let state = match state {
        Ok(s) => s,
        Err(e) => return Err(format!("Failed to add finding: {}", e)),
    };

    Ok(match state["findings"].as_array() {
        Some(a) => a.len(),
        None => 0,
    })
}

/// Main-arm dispatcher: pair the run_impl result with an exit code.
/// Returns `(value, 0)` on success or no-state, `(error_value, 1)` on
/// any other error. The no-state case carries `"status": "no_state"`
/// per the existing CLI contract.
pub fn run_impl_main(args: Args, root: &Path, cwd: &Path) -> (Value, i32) {
    match run_impl_with_root(&args, root, cwd) {
        Ok(count) => (json!({"status": "ok", "finding_count": count}), 0),
        Err(msg) if msg == "no_state" => (json!({"status": "no_state"}), 0),
        Err(msg) => (json!({"status": "error", "message": msg}), 1),
    }
}

/// Testable variant of [`run`] that accepts cwd as a Result so unit
/// tests can drive the `unwrap_or(PathBuf::from("."))` fallback when
/// `current_dir()` fails (deleted-cwd / chroot environments).
pub fn run_impl_main_with_cwd_result(
    args: Args,
    root: &Path,
    cwd_result: std::io::Result<std::path::PathBuf>,
) -> (Value, i32) {
    let cwd = cwd_result.unwrap_or(std::path::PathBuf::from("."));
    run_impl_main(args, root, &cwd)
}
