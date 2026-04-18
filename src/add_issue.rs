use std::path::Path;

use clap::Parser;
use serde_json::{json, Value};

use crate::flow_paths::FlowPaths;
use crate::git::{project_root, resolve_branch};
use crate::lock::mutate_state;
use crate::phase_config::phase_names;
use crate::utils::now;

#[derive(Parser, Debug)]
#[command(name = "add-issue", about = "Record a filed issue in FLOW state")]
pub struct Args {
    /// Issue label (e.g. Rule, Flow, Flaky Test)
    #[arg(long)]
    pub label: String,

    /// Issue title
    #[arg(long)]
    pub title: String,

    /// Issue URL
    #[arg(long)]
    pub url: String,

    /// Phase that filed the issue
    #[arg(long)]
    pub phase: String,

    /// Override branch for state file lookup
    #[arg(long)]
    pub branch: Option<String>,
}

/// Main-arm dispatcher with injected root. Returns `(value, exit_code)`:
/// `(ok+issue_count, 0)` on success, `(no_state, 0)` when the state file
/// is missing, `(error+message, 1)` on resolve-branch failure or
/// mutate_state failure. Tests pass tempdir paths and `--branch` args
/// to bypass git resolution.
pub fn run_impl_main(args: Args, root: &Path) -> (Value, i32) {
    let branch = match resolve_branch(args.branch.as_deref(), root) {
        Some(b) => b,
        None => {
            return (
                json!({"status": "error", "message": "Could not determine current branch"}),
                1,
            );
        }
    };
    // Branch reaches us either from `current_branch()` (raw git output)
    // or from `--branch` CLI override (raw user input). Both are
    // external inputs per `.claude/rules/external-input-validation.md`,
    // so use the fallible constructor to reject slash-containing or
    // empty branches as a structured error rather than a panic.
    let state_path = match FlowPaths::try_new(root, &branch) {
        Some(p) => p.state_file(),
        None => {
            return (
                json!({"status": "error", "message": format!("Invalid branch '{}'", branch)}),
                1,
            );
        }
    };

    if !state_path.exists() {
        return (json!({"status": "no_state"}), 0);
    }

    let names = phase_names();
    let phase_name = match names.get(&args.phase) {
        Some(n) => n.clone(),
        None => args.phase.clone(),
    };
    let timestamp = now();

    match mutate_state(&state_path, |state| {
        // Corruption resilience: skip mutation when state root is wrong
        // type (e.g. array from interrupted write) to prevent IndexMut
        // panics. See .claude/rules/rust-patterns.md "State Mutation
        // Object Guards".
        if !(state.is_object() || state.is_null()) {
            return;
        }
        if state.get("issues_filed").is_none() || !state["issues_filed"].is_array() {
            state["issues_filed"] = json!([]);
        }
        // The block above guarantees state["issues_filed"] is an array,
        // so as_array_mut returns Some unconditionally.
        let arr = state["issues_filed"]
            .as_array_mut()
            .expect("issues_filed is always an array here");
        arr.push(json!({
            "label": args.label,
            "title": args.title,
            "url": args.url,
            "phase": args.phase,
            "phase_name": phase_name,
            "timestamp": timestamp,
        }));
    }) {
        Ok(state) => {
            let count = match state["issues_filed"].as_array() {
                Some(a) => a.len(),
                None => 0,
            };
            (json!({"status": "ok", "issue_count": count}), 0)
        }
        Err(e) => (
            json!({"status": "error", "message": format!("Failed to add issue: {}", e)}),
            1,
        ),
    }
}

pub fn run(args: Args) -> ! {
    let root = project_root();
    let (value, code) = run_impl_main(args, &root);
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
            "current_phase": "flow-learn",
            "issues_filed": []
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
    fn add_issue_to_empty_array() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("test-feature");
        let path = write_state(dir.path(), "test-feature", &state);

        let result = mutate_state(&path, |s| {
            let names = phase_names();
            let phase = "flow-learn";
            let phase_name = names.get(phase).cloned().unwrap_or_default();
            s["issues_filed"]
                .as_array_mut()
                .expect("issues_filed is always an array in this fixture")
                .push(json!({
                    "label": "Rule",
                    "title": "Add rule: use git -C",
                    "url": "https://github.com/test/test/issues/1",
                    "phase": phase,
                    "phase_name": phase_name,
                    "timestamp": now(),
                }));
        })
        .unwrap();

        let issues = result["issues_filed"].as_array().unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0]["label"], "Rule");
        assert_eq!(issues[0]["title"], "Add rule: use git -C");
        assert_eq!(issues[0]["url"], "https://github.com/test/test/issues/1");
        assert_eq!(issues[0]["phase"], "flow-learn");
        assert_eq!(issues[0]["phase_name"], "Learn");
        assert!(issues[0]["timestamp"].as_str().unwrap().contains("T"));
    }

    #[test]
    fn add_issue_preserves_existing() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = make_state("test-feature");
        state["issues_filed"] = json!([
            {"label": "Flow", "title": "existing"}
        ]);
        let path = write_state(dir.path(), "test-feature", &state);

        mutate_state(&path, |s| {
            s["issues_filed"]
                .as_array_mut()
                .expect("issues_filed is always an array in this fixture")
                .push(json!({"label": "Rule", "title": "new"}));
        })
        .unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let on_disk: Value = serde_json::from_str(&content).unwrap();
        let issues = on_disk["issues_filed"].as_array().unwrap();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0]["title"], "existing");
        assert_eq!(issues[1]["title"], "new");
    }

    /// Exercises production line 86 (`state["issues_filed"] = json!([])`)
    /// — the auto-create branch fires when the state file lacks the key.
    #[test]
    fn add_issue_creates_array_if_missing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let path = state_dir.join("test-feature.json");
        fs::write(&path, r#"{"current_phase": "flow-learn"}"#).unwrap();

        let args = Args {
            label: "Flaky Test".to_string(),
            title: "test".to_string(),
            url: "https://example.com/1".to_string(),
            phase: "flow-learn".to_string(),
            branch: Some("test-feature".to_string()),
        };

        let (value, code) = run_impl_main(args, &root);
        assert_eq!(code, 0);
        assert_eq!(value["status"], "ok");
        assert_eq!(value["issue_count"], 1);

        let content = fs::read_to_string(&path).unwrap();
        let on_disk: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(on_disk["issues_filed"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn add_issue_persists_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("test-feature");
        let path = write_state(dir.path(), "test-feature", &state);

        mutate_state(&path, |s| {
            s["issues_filed"]
                .as_array_mut()
                .expect("issues_filed is always an array in this fixture")
                .push(json!({"label": "Rule", "title": "persisted"}));
        })
        .unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let on_disk: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(on_disk["issues_filed"][0]["title"], "persisted");
    }

    /// Verify that an array-root state file triggers the production
    /// object guard's early return inside `run_impl_main`'s
    /// mutate_state closure (lines 82-84), leaving the file unchanged.
    #[test]
    fn add_issue_array_root_state_noop() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let path = state_dir.join("test-feature.json");
        fs::write(&path, "[1, 2, 3]").unwrap();

        let args = Args {
            label: "Rule".to_string(),
            title: "should not appear".to_string(),
            url: "https://example.com/1".to_string(),
            phase: "flow-learn".to_string(),
            branch: Some("test-feature".to_string()),
        };

        let (value, code) = run_impl_main(args, &root);
        assert_eq!(code, 0);
        assert_eq!(value["status"], "ok");
        assert_eq!(value["issue_count"], 0);

        let after = fs::read_to_string(&path).unwrap();
        let parsed: Value = serde_json::from_str(&after).unwrap();
        assert!(parsed.is_array(), "Root should still be an array");
        assert_eq!(parsed.as_array().unwrap().len(), 3);
    }

    // Removed: `corrupt_state_file_errors`. The mutate_state corrupt-
    // JSON path is owned by `lock::mutate_state_corrupt_json` and
    // `lock::mutate_state_error_wraps_invalid_json_as_json`; this
    // wrapper test was a duplicate guard per
    // `.claude/rules/tests-guard-real-regressions.md`. The
    // `run_impl_main_mutate_state_failure_returns_error_tuple` test
    // below covers the add-issue-specific error wrapping.

    // --- run_impl_main ---

    fn make_args(branch: Option<&str>) -> Args {
        Args {
            label: "Rule".to_string(),
            title: "test-title".to_string(),
            url: "https://github.com/owner/repo/issues/1".to_string(),
            phase: "flow-learn".to_string(),
            branch: branch.map(|s| s.to_string()),
        }
    }

    #[test]
    fn add_issue_run_impl_main_no_state_returns_no_state_tuple() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let args = make_args(Some("missing-branch"));
        let (value, code) = run_impl_main(args, &root);
        assert_eq!(value["status"], "no_state");
        assert_eq!(code, 0);
    }

    #[test]
    fn add_issue_run_impl_main_success_returns_issue_count_tuple() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(
            state_dir.join("present-branch.json"),
            r#"{"current_phase":"flow-learn","issues_filed":[]}"#,
        )
        .unwrap();
        let args = make_args(Some("present-branch"));
        let (value, code) = run_impl_main(args, &root);
        assert_eq!(code, 0);
        assert_eq!(value["status"], "ok");
        assert_eq!(value["issue_count"], 1);
    }

    #[test]
    fn add_issue_run_impl_main_mutate_state_failure_returns_error_tuple() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        // Write malformed JSON so mutate_state's serde parse fails.
        fs::write(state_dir.join("present-branch.json"), "{not json").unwrap();
        let args = make_args(Some("present-branch"));
        let (value, code) = run_impl_main(args, &root);
        assert_eq!(value["status"], "error");
        assert_eq!(code, 1);
        assert!(value["message"]
            .as_str()
            .unwrap()
            .contains("Failed to add issue"));
    }

    #[test]
    fn add_issue_run_impl_main_array_root_returns_ok_zero_count() {
        // State file root is an array — closure's object guard fires the
        // early return, leaving issues_filed as Value::Null. The
        // as_array() match arm None branch returns count 0.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(state_dir.join("array-root.json"), "[1, 2, 3]").unwrap();
        let args = make_args(Some("array-root"));
        let (value, code) = run_impl_main(args, &root);
        assert_eq!(value["status"], "ok");
        assert_eq!(value["issue_count"], 0);
        assert_eq!(code, 0);
    }

    #[test]
    fn add_issue_run_impl_main_unknown_phase_falls_back_to_phase_string() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(
            state_dir.join("unknown-phase.json"),
            r#"{"current_phase":"flow-learn","issues_filed":[]}"#,
        )
        .unwrap();
        let mut args = make_args(Some("unknown-phase"));
        args.phase = "custom-unknown-phase".to_string();
        let (value, code) = run_impl_main(args, &root);
        assert_eq!(value["status"], "ok");
        assert_eq!(code, 0);
        let on_disk: Value = serde_json::from_str(
            &fs::read_to_string(state_dir.join("unknown-phase.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            on_disk["issues_filed"][0]["phase_name"], "custom-unknown-phase",
            "phase_name should fall back to raw phase string"
        );
    }

    #[test]
    fn add_issue_run_impl_main_findings_wrong_type_resets_to_array() {
        // State file where "issues_filed" is the wrong type (string instead
        // of array) — closure must reset it to an empty array.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(
            state_dir.join("wrong-type.json"),
            r#"{"current_phase":"flow-learn","issues_filed":"not-an-array"}"#,
        )
        .unwrap();
        let args = make_args(Some("wrong-type"));
        let (value, code) = run_impl_main(args, &root);
        assert_eq!(value["status"], "ok");
        assert_eq!(value["issue_count"], 1);
        assert_eq!(code, 0);
    }

    #[test]
    fn add_issue_run_impl_main_slash_branch_returns_structured_error_no_panic() {
        // Regression: --branch feature/foo previously panicked via
        // FlowPaths::new. Per .claude/rules/external-input-validation.md
        // CLI subcommand entry callsite discipline, --branch is external
        // input and must use FlowPaths::try_new with a structured error
        // return.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let args = make_args(Some("feature/foo"));
        let (value, code) = run_impl_main(args, &root);
        assert_eq!(code, 1);
        assert_eq!(value["status"], "error");
        assert!(value["message"]
            .as_str()
            .unwrap()
            .contains("Invalid branch 'feature/foo'"));
    }
}
