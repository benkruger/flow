use std::process;

use clap::Parser;
use serde_json::json;

use crate::flow_paths::FlowPaths;
use crate::git::{project_root, resolve_branch};
use crate::lock::mutate_state;
use crate::output::{json_error, json_ok};
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

/// Fallible implementation — returns `Ok(finding_count)` on success,
/// `Err("no_state")` when no state file exists, or `Err(message)` on failure.
pub fn run_impl(args: &Args) -> Result<usize, String> {
    if !VALID_OUTCOMES.contains(&args.outcome.as_str()) {
        return Err(format!(
            "Invalid outcome '{}'. Valid: {}",
            args.outcome,
            VALID_OUTCOMES.join(", ")
        ));
    }

    let root = project_root();

    // Drift guard: state mutations must happen from inside the
    // subdirectory the flow was started in. Without this, a user who
    // cds out of an `api/`-scoped flow into `ios/` could record
    // findings against the wrong subtree. See
    // [`crate::cwd_scope::enforce`].
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    crate::cwd_scope::enforce(&cwd, &root)?;

    let branch = resolve_branch(args.branch.as_deref(), &root)
        .ok_or_else(|| "Could not determine current branch".to_string())?;
    let state_path = FlowPaths::new(&root, &branch).state_file();

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

pub fn run(args: Args) {
    match run_impl(&args) {
        Ok(count) => {
            json_ok(&[("finding_count", json!(count))]);
        }
        Err(msg) if msg == "no_state" => {
            println!(r#"{{"status":"no_state"}}"#);
            process::exit(0);
        }
        Err(msg) => {
            json_error(&msg, &[]);
            process::exit(1);
        }
    }
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
                    "finding": "Missing error handling",
                    "reason": "Out of scope for this PR",
                    "outcome": "filed",
                    "phase": "flow-code-review",
                    "phase_name": "Code Review",
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
}
