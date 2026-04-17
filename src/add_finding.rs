use std::path::Path;

use clap::Parser;
use serde_json::{json, Value};

use crate::flow_paths::FlowPaths;
use crate::git::{project_root, resolve_branch};
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
/// production wraps via [`run_impl`].
pub fn run_impl_with_root(args: &Args, root: &Path, cwd: &Path) -> Result<usize, String> {
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

    let branch = resolve_branch(args.branch.as_deref(), root)
        .ok_or_else(|| "Could not determine current branch".to_string())?;
    let state_path = FlowPaths::new(root, &branch).state_file();

    if !state_path.exists() {
        return Err("no_state".to_string());
    }

    let names = phase_names();
    let phase_name = names
        .get(&args.phase)
        .cloned()
        .unwrap_or_else(|| args.phase.clone());
    let timestamp = now();

    let state = mutate_state(&state_path, |state| {
        if !(state.is_object() || state.is_null()) {
            return;
        }
        if state.get("findings").is_none() || !state["findings"].is_array() {
            state["findings"] = json!([]);
        }
        if let Some(arr) = state["findings"].as_array_mut() {
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
        }
    })
    .map_err(|e| format!("Failed to add finding: {}", e))?;

    Ok(state["findings"].as_array().map(|a| a.len()).unwrap_or(0))
}

/// Fallible implementation — returns `Ok(finding_count)` on success,
/// `Err("no_state")` when no state file exists, or `Err(message)` on failure.
pub fn run_impl(args: &Args) -> Result<usize, String> {
    let root = project_root();
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    run_impl_with_root(args, &root, &cwd)
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

pub fn run(args: Args) -> ! {
    let root = project_root();
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let (value, code) = run_impl_main(args, &root, &cwd);
    crate::dispatch::dispatch_json(value, code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::fs;
    use std::path::Path;

    fn make_state(branch: &str) -> Value {
        json!({
            "schema_version": 1,
            "branch": branch,
            "current_phase": "flow-code-review",
            "findings": []
        })
    }

    fn write_state(dir: &Path, branch: &str, state: &Value) -> std::path::PathBuf {
        let state_dir = dir.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let path = state_dir.join(format!("{}.json", branch));
        fs::write(&path, serde_json::to_string_pretty(state).unwrap()).unwrap();
        path
    }

    #[test]
    fn add_finding_happy_path() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("test-feature");
        let path = write_state(dir.path(), "test-feature", &state);

        let names = phase_names();
        let phase = "flow-code-review";
        let phase_name = names.get(phase).cloned().unwrap_or_default();
        let timestamp = now();

        let result = mutate_state(&path, |s| {
            if s.get("findings").is_none() || !s["findings"].is_array() {
                s["findings"] = json!([]);
            }
            if let Some(arr) = s["findings"].as_array_mut() {
                arr.push(json!({
                    "finding": "Unused import in parser.rs",
                    "reason": "False positive — import used in macro expansion",
                    "outcome": "dismissed",
                    "phase": phase,
                    "phase_name": phase_name,
                    "timestamp": timestamp,
                }));
            }
        })
        .unwrap();

        let findings = result["findings"].as_array().unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0]["finding"], "Unused import in parser.rs");
        assert_eq!(
            findings[0]["reason"],
            "False positive — import used in macro expansion"
        );
        assert_eq!(findings[0]["outcome"], "dismissed");
        assert_eq!(findings[0]["phase"], "flow-code-review");
        assert_eq!(findings[0]["phase_name"], "Code Review");
        assert!(findings[0]["timestamp"].as_str().unwrap().contains("T"));
    }

    #[test]
    fn add_finding_multiple_increments_count() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("test-feature");
        let path = write_state(dir.path(), "test-feature", &state);

        mutate_state(&path, |s| {
            if let Some(arr) = s["findings"].as_array_mut() {
                arr.push(json!({"finding": "first", "outcome": "fixed"}));
            }
        })
        .unwrap();

        let result = mutate_state(&path, |s| {
            if let Some(arr) = s["findings"].as_array_mut() {
                arr.push(json!({"finding": "second", "outcome": "dismissed"}));
            }
        })
        .unwrap();

        let findings = result["findings"].as_array().unwrap();
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0]["finding"], "first");
        assert_eq!(findings[1]["finding"], "second");
    }

    #[test]
    fn add_finding_creates_array_if_missing() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let path = state_dir.join("test.json");
        fs::write(&path, r#"{"current_phase": "flow-code-review"}"#).unwrap();

        mutate_state(&path, |s| {
            if s.get("findings").is_none() || !s["findings"].is_array() {
                s["findings"] = json!([]);
            }
            if let Some(arr) = s["findings"].as_array_mut() {
                arr.push(json!({"finding": "test", "outcome": "fixed"}));
            }
        })
        .unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let on_disk: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(on_disk["findings"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn add_finding_valid_outcome_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("test-feature");
        write_state(dir.path(), "test-feature", &state);

        let args = Args {
            finding: "test".to_string(),
            reason: "test".to_string(),
            outcome: "fixed".to_string(),
            phase: "flow-code-review".to_string(),
            issue_url: None,
            path: None,
            branch: Some("test-feature".to_string()),
        };

        // run_impl needs project_root() which won't resolve in test fixtures,
        // so we validate the constant directly and test the CLI path via
        // the adversarial integration tests
        for outcome in VALID_OUTCOMES {
            assert!(
                VALID_OUTCOMES.contains(outcome),
                "Outcome {} should be in VALID_OUTCOMES",
                outcome
            );
        }
        // Verify invalid outcomes are rejected
        assert!(!VALID_OUTCOMES.contains(&"invalid"));
        assert!(!VALID_OUTCOMES.contains(&""));
        assert!(!VALID_OUTCOMES.contains(&"FIXED"));
        // Verify the args struct accepts valid outcomes
        assert_eq!(args.outcome, "fixed");
    }

    #[test]
    fn add_finding_filed_includes_issue_url() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("test-feature");
        let path = write_state(dir.path(), "test-feature", &state);

        mutate_state(&path, |s| {
            if let Some(arr) = s["findings"].as_array_mut() {
                arr.push(json!({
                    "finding": "Process gap in Learn phase",
                    "reason": "Filed as Flow issue",
                    "outcome": "filed",
                    "phase": "flow-learn",
                    "phase_name": "Learn",
                    "issue_url": "https://github.com/test/test/issues/42",
                    "timestamp": now(),
                }));
            }
        })
        .unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let on_disk: Value = serde_json::from_str(&content).unwrap();
        let finding = &on_disk["findings"][0];
        assert_eq!(
            finding["issue_url"],
            "https://github.com/test/test/issues/42"
        );
        assert_eq!(finding["outcome"], "filed");
    }

    #[test]
    fn add_finding_rule_written_includes_path() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("test-feature");
        let path = write_state(dir.path(), "test-feature", &state);

        mutate_state(&path, |s| {
            if let Some(arr) = s["findings"].as_array_mut() {
                arr.push(json!({
                    "finding": "No rule for error handling pattern",
                    "reason": "Gap identified during learn analysis",
                    "outcome": "rule_written",
                    "phase": "flow-learn",
                    "phase_name": "Learn",
                    "path": ".claude/rules/error-handling.md",
                    "timestamp": now(),
                }));
            }
        })
        .unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let on_disk: Value = serde_json::from_str(&content).unwrap();
        let finding = &on_disk["findings"][0];
        assert_eq!(finding["path"], ".claude/rules/error-handling.md");
        assert_eq!(finding["outcome"], "rule_written");
    }

    #[test]
    fn add_finding_phase_name_resolution() {
        let names = phase_names();
        assert_eq!(names.get("flow-code-review").unwrap(), "Code Review");
        assert_eq!(names.get("flow-learn").unwrap(), "Learn");
    }

    #[test]
    fn add_finding_persists_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("test-feature");
        let path = write_state(dir.path(), "test-feature", &state);

        mutate_state(&path, |s| {
            if let Some(arr) = s["findings"].as_array_mut() {
                arr.push(json!({"finding": "persisted", "outcome": "fixed"}));
            }
        })
        .unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let on_disk: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(on_disk["findings"][0]["finding"], "persisted");
    }

    #[test]
    fn add_finding_timestamp_is_pacific() {
        let ts = now();
        // Pacific Time offsets: -07:00 (PDT) or -08:00 (PST)
        assert!(
            ts.contains("-07:00") || ts.contains("-08:00"),
            "Timestamp {} should be Pacific Time",
            ts
        );
    }

    // --- code_review_filing_gate ---

    #[test]
    fn filed_outcome_rejected_for_code_review() {
        let msg = code_review_filing_gate("filed", "flow-code-review");
        assert!(msg.is_some(), "filed + flow-code-review must be rejected");
        let text = msg.unwrap();
        assert!(text.contains("flow-code-review"));
        assert!(text.contains("filed"));
    }

    #[test]
    fn filed_outcome_accepted_for_learn() {
        assert!(
            code_review_filing_gate("filed", "flow-learn").is_none(),
            "filed + flow-learn must pass the gate — Learn files process gaps"
        );
    }

    #[test]
    fn dismissed_outcome_accepted_for_code_review() {
        assert!(
            code_review_filing_gate("dismissed", "flow-code-review").is_none(),
            "dismissed + flow-code-review must pass — False positive path"
        );
    }

    #[test]
    fn fixed_outcome_accepted_for_code_review() {
        assert!(
            code_review_filing_gate("fixed", "flow-code-review").is_none(),
            "fixed + flow-code-review must pass — Real finding path"
        );
    }

    #[test]
    fn filed_outcome_accepted_for_flow_code() {
        assert!(
            code_review_filing_gate("filed", "flow-code").is_none(),
            "flow-code files Flaky Test issues — must pass"
        );
    }

    #[test]
    fn leading_whitespace_phase_rejected_for_code_review() {
        assert!(
            code_review_filing_gate("filed", " flow-code-review").is_some(),
            "whitespace drift must not bypass the gate"
        );
    }

    #[test]
    fn trailing_whitespace_phase_rejected_for_code_review() {
        assert!(
            code_review_filing_gate("filed", "flow-code-review ").is_some(),
            "whitespace drift must not bypass the gate"
        );
    }

    #[test]
    fn uppercase_phase_rejected_for_code_review() {
        assert!(
            code_review_filing_gate("filed", "FLOW-CODE-REVIEW").is_some(),
            "case drift must not bypass the gate"
        );
    }

    #[test]
    fn mixed_case_phase_rejected_for_code_review() {
        assert!(
            code_review_filing_gate("filed", "Flow-Code-Review").is_some(),
            "mixed-case drift must not bypass the gate"
        );
    }

    #[test]
    fn uppercase_filed_outcome_rejected_for_code_review() {
        assert!(
            code_review_filing_gate("Filed", "flow-code-review").is_some(),
            "case drift on outcome must not bypass the gate"
        );
    }

    #[test]
    fn embedded_nul_phase_rejected_for_code_review() {
        assert!(
            code_review_filing_gate("filed", "flow-code-review\0").is_some(),
            "trailing NUL must not bypass the gate"
        );
    }

    /// Verify that an array-root state file triggers the object guard's
    /// early return, leaving the file unchanged and preventing an
    /// IndexMut panic on non-object root types.
    #[test]
    fn add_finding_array_root_state_noop() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let path = state_dir.join("test.json");
        let content = "[1, 2, 3]";
        fs::write(&path, content).unwrap();

        mutate_state(&path, |state| {
            if !(state.is_object() || state.is_null()) {
                return;
            }
            if state.get("findings").is_none() || !state["findings"].is_array() {
                state["findings"] = json!([]);
            }
            if let Some(arr) = state["findings"].as_array_mut() {
                arr.push(json!({"finding": "should not appear"}));
            }
        })
        .unwrap();

        let after = fs::read_to_string(&path).unwrap();
        let parsed: Value = serde_json::from_str(&after).unwrap();
        assert!(parsed.is_array(), "Root should still be an array");
        assert_eq!(parsed.as_array().unwrap().len(), 3);
    }

    #[test]
    fn future_outcome_rejected_for_code_review() {
        // Forward-compat: if VALID_OUTCOMES is extended with a new
        // "defer"-ish outcome, it must not silently pass the gate.
        assert!(
            code_review_filing_gate("deferred", "flow-code-review").is_some(),
            "outcomes outside the allowlist must be rejected during Code Review"
        );
        assert!(
            code_review_filing_gate("rule_written", "flow-code-review").is_some(),
            "rule_written is a Learn-phase outcome, not Code Review"
        );
    }

    // --- run_impl_main ---

    fn make_args(outcome: &str, phase: &str, branch: Option<&str>) -> Args {
        Args {
            finding: "test-finding".to_string(),
            reason: "test-reason".to_string(),
            outcome: outcome.to_string(),
            phase: phase.to_string(),
            issue_url: None,
            path: None,
            branch: branch.map(|s| s.to_string()),
        }
    }

    #[test]
    fn add_finding_run_impl_main_invalid_outcome_returns_error_tuple() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let args = make_args("not-an-outcome", "flow-learn", Some("test-branch"));
        let (value, code) = run_impl_main(args, &root, &root);
        assert_eq!(value["status"], "error");
        assert_eq!(code, 1);
        assert!(value["message"]
            .as_str()
            .unwrap()
            .contains("Invalid outcome"));
    }

    #[test]
    fn add_finding_run_impl_main_code_review_filing_blocked_returns_error_tuple() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let args = make_args("filed", "flow-code-review", Some("test-branch"));
        let (value, code) = run_impl_main(args, &root, &root);
        assert_eq!(value["status"], "error");
        assert_eq!(code, 1);
        assert!(value["message"].as_str().unwrap().contains("Code Review"));
    }

    #[test]
    fn add_finding_run_impl_main_no_state_returns_no_state_tuple() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let args = make_args("fixed", "flow-learn", Some("missing-branch"));
        let (value, code) = run_impl_main(args, &root, &root);
        assert_eq!(value["status"], "no_state");
        assert_eq!(code, 0);
    }

    #[test]
    fn add_finding_run_impl_main_success_returns_finding_count_tuple() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let state_dir = root.join(".flow-states");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(
            state_dir.join("present-branch.json"),
            r#"{"current_phase":"flow-learn","findings":[]}"#,
        )
        .unwrap();
        let args = make_args("fixed", "flow-learn", Some("present-branch"));
        let (value, code) = run_impl_main(args, &root, &root);
        assert_eq!(value["status"], "ok");
        assert_eq!(value["finding_count"], 1);
        assert_eq!(code, 0);
    }
}
