use std::process;

use clap::Parser;
use serde_json::json;

use crate::git::{is_foreign_branch, project_root, resolve_branch};
use crate::lock::mutate_state;
use crate::output::{json_error, json_ok};
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

pub fn run(args: Args) {
    let root = project_root();
    let (branch, candidates) = resolve_branch(args.branch.as_deref(), &root);

    if branch.is_none() {
        // add-issue silently returns no_state on ambiguity (unlike append-note)
        if !candidates.is_empty() {
            println!(r#"{{"status":"no_state"}}"#);
            process::exit(0);
        } else {
            json_error("Could not determine current branch", &[]);
            process::exit(1);
        }
    }

    let branch = branch.unwrap();

    // Guard: reject singleton-fallback resolution for interactive commands
    if is_foreign_branch(&branch, args.branch.as_deref()) {
        println!(r#"{{"status":"no_state"}}"#);
        process::exit(0);
    }

    let state_path = root.join(".flow-states").join(format!("{}.json", branch));

    if !state_path.exists() {
        println!(r#"{{"status":"no_state"}}"#);
        process::exit(0);
    }

    let names = phase_names();
    let phase_name = names
        .get(&args.phase)
        .cloned()
        .unwrap_or_else(|| args.phase.clone());
    let timestamp = now();

    match mutate_state(&state_path, |state| {
        if state.get("issues_filed").is_none() || !state["issues_filed"].is_array() {
            state["issues_filed"] = json!([]);
        }
        if let Some(arr) = state["issues_filed"].as_array_mut() {
            arr.push(json!({
                "label": args.label,
                "title": args.title,
                "url": args.url,
                "phase": args.phase,
                "phase_name": phase_name,
                "timestamp": timestamp,
            }));
        }
    }) {
        Ok(state) => {
            let count = state["issues_filed"]
                .as_array()
                .map(|a| a.len())
                .unwrap_or(0);
            json_ok(&[("issue_count", json!(count))]);
        }
        Err(e) => {
            json_error(&format!("Failed to add issue: {}", e), &[]);
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
            if let Some(arr) = s["issues_filed"].as_array_mut() {
                arr.push(json!({
                    "label": "Rule",
                    "title": "Add rule: use git -C",
                    "url": "https://github.com/test/test/issues/1",
                    "phase": phase,
                    "phase_name": phase_name,
                    "timestamp": now(),
                }));
            }
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
            if let Some(arr) = s["issues_filed"].as_array_mut() {
                arr.push(json!({"label": "Rule", "title": "new"}));
            }
        })
        .unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let on_disk: Value = serde_json::from_str(&content).unwrap();
        let issues = on_disk["issues_filed"].as_array().unwrap();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0]["title"], "existing");
        assert_eq!(issues[1]["title"], "new");
    }

    #[test]
    fn add_issue_creates_array_if_missing() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let path = state_dir.join("test.json");
        fs::write(&path, r#"{"current_phase": "flow-code"}"#).unwrap();

        mutate_state(&path, |s| {
            if s.get("issues_filed").is_none() || !s["issues_filed"].is_array() {
                s["issues_filed"] = json!([]);
            }
            if let Some(arr) = s["issues_filed"].as_array_mut() {
                arr.push(json!({"label": "Flaky Test", "title": "test"}));
            }
        })
        .unwrap();

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
            if let Some(arr) = s["issues_filed"].as_array_mut() {
                arr.push(json!({"label": "Rule", "title": "persisted"}));
            }
        })
        .unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let on_disk: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(on_disk["issues_filed"][0]["title"], "persisted");
    }

    #[test]
    fn corrupt_state_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let path = state_dir.join("test.json");
        fs::write(&path, "{corrupt").unwrap();

        let result = mutate_state(&path, |_| {});
        assert!(result.is_err());
    }
}
