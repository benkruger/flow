//! Verify QA assertions after a completed flow.
//!
//! Usage: bin/flow qa-verify --framework <name> --repo <owner/repo>
//!
//! Checks post-Complete outcomes: cleanup (no leftover state files or
//! worktrees) and at least one merged PR.
//!
//! Always exits 0 — the Python original has no error exit path.
//! The consumer (flow-qa skill) parses JSON, so compact output is fine
//! (Python used indent=2 but it's cosmetic).

use std::path::{Path, PathBuf};
use std::process::{self, Command};

use clap::Parser;
use serde_json::{json, Value};

#[derive(Parser, Debug)]
#[command(name = "qa-verify", about = "Verify QA assertions after a completed flow")]
pub struct Args {
    /// Framework name (reserved for future use)
    #[arg(long)]
    pub framework: Option<String>,

    /// GitHub repo (owner/name)
    #[arg(long)]
    pub repo: String,

    /// Project root path
    #[arg(long, default_value = ".")]
    pub project_root: String,
}

/// Find all .flow-states/*.json files, excluding non-state files
/// (orchestrate* and *-phases.json).
pub fn find_state_files(project_root: &Path) -> Vec<PathBuf> {
    let state_dir = project_root.join(".flow-states");
    if !state_dir.is_dir() {
        return Vec::new();
    }

    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&state_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                // Filter dot-prefixed entries to match Python's glob("*.json")
                // behavior where * does not match leading dots.
                if name.ends_with(".json")
                    && !name.starts_with('.')
                    && !name.starts_with("orchestrate")
                    && !name.ends_with("-phases.json")
                {
                    results.push(path);
                }
            }
        }
    }
    results
}

/// Verify post-Complete outcomes with an injectable command runner.
///
/// The runner takes a slice of command args and returns Some(stdout) on
/// success, None on failure.
pub fn verify_impl(
    _framework: Option<&str>,
    repo: &str,
    project_root: &Path,
    runner: &dyn Fn(&[&str]) -> Option<String>,
) -> Value {
    let mut checks: Vec<Value> = Vec::new();

    // State files should be cleaned up after Complete
    let state_files = find_state_files(project_root);
    checks.push(json!({
        "name": "State files cleaned up",
        "passed": state_files.is_empty(),
        "detail": if state_files.is_empty() {
            "No leftover state files".to_string()
        } else {
            format!("Found {} leftover state file(s)", state_files.len())
        }
    }));

    // Worktrees should be cleaned up after Complete
    let worktrees_dir = project_root.join(".worktrees");
    let worktree_count = if worktrees_dir.is_dir() {
        std::fs::read_dir(&worktrees_dir)
            .map(|entries| entries.count())
            .unwrap_or(0)
    } else {
        0
    };
    checks.push(json!({
        "name": "Worktrees cleaned up",
        "passed": worktree_count == 0,
        "detail": if worktree_count == 0 {
            "No leftover worktrees".to_string()
        } else {
            format!("Found {} leftover worktree(s)", worktree_count)
        }
    }));

    // At least one PR should be merged
    let pr_args = [
        "gh", "pr", "list", "--repo", repo,
        "--state", "merged", "--limit", "1", "--json", "number",
    ];
    match runner(&pr_args) {
        Some(stdout) => {
            let pr_list: Vec<Value> = serde_json::from_str(&stdout).unwrap_or_default();
            let has_merged = !pr_list.is_empty();
            let detail = if has_merged {
                format!(
                    "PR #{} merged",
                    pr_list[0]["number"].as_i64().unwrap_or(0)
                )
            } else {
                "No merged PRs found".to_string()
            };
            checks.push(json!({
                "name": "PR merged",
                "passed": has_merged,
                "detail": detail
            }));
        }
        None => {
            checks.push(json!({
                "name": "PR merged",
                "passed": false,
                "detail": "Could not fetch merged PRs"
            }));
        }
    }

    json!({
        "status": "ok",
        "checks": checks
    })
}

/// CLI entry point.
///
/// Returns Ok(Value) always — qa-verify has no error exit path.
/// Returns Err(String) only for infrastructure failures.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let project_root = Path::new(&args.project_root);

    let runner = |cmd_args: &[&str]| -> Option<String> {
        let output = Command::new(cmd_args[0])
            .args(&cmd_args[1..])
            .output()
            .ok()?;
        if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            None
        }
    };

    Ok(verify_impl(
        args.framework.as_deref(),
        &args.repo,
        project_root,
        &runner,
    ))
}

pub fn run(args: Args) {
    match run_impl(&args) {
        Ok(result) => {
            println!("{}", result);
        }
        Err(e) => {
            println!("{}", json!({"status": "error", "message": e}));
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn mock_ok_pr() -> Option<String> {
        Some(serde_json::to_string(&json!([{"number": 1}])).unwrap())
    }

    fn mock_empty_list() -> Option<String> {
        Some("[]".to_string())
    }

    #[test]
    fn test_verify_all_pass() {
        let dir = tempfile::tempdir().unwrap();

        let result = verify_impl(Some("python"), "owner/repo", dir.path(), &|_| mock_ok_pr());

        assert_eq!(result["status"], "ok");
        let checks = result["checks"].as_array().unwrap();
        assert!(checks.iter().all(|c| c["passed"] == true));
    }

    #[test]
    fn test_verify_leftover_state_file() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir(&state_dir).unwrap();
        fs::write(state_dir.join("leftover.json"), r#"{"branch":"leftover"}"#).unwrap();

        let result = verify_impl(Some("python"), "owner/repo", dir.path(), &|_| mock_ok_pr());

        let checks = result["checks"].as_array().unwrap();
        let state_check: Vec<&Value> = checks
            .iter()
            .filter(|c| {
                c["name"]
                    .as_str()
                    .unwrap()
                    .to_lowercase()
                    .contains("state")
            })
            .collect();
        assert!(!state_check.is_empty());
        assert_eq!(state_check[0]["passed"], false);
    }

    #[test]
    fn test_verify_leftover_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let wt_dir = dir.path().join(".worktrees").join("some-feature");
        fs::create_dir_all(&wt_dir).unwrap();

        let result = verify_impl(Some("python"), "owner/repo", dir.path(), &|_| mock_ok_pr());

        let checks = result["checks"].as_array().unwrap();
        let wt_check: Vec<&Value> = checks
            .iter()
            .filter(|c| {
                c["name"]
                    .as_str()
                    .unwrap()
                    .to_lowercase()
                    .contains("worktree")
            })
            .collect();
        assert!(!wt_check.is_empty());
        assert_eq!(wt_check[0]["passed"], false);
    }

    #[test]
    fn test_verify_no_merged_pr() {
        let dir = tempfile::tempdir().unwrap();

        let result =
            verify_impl(Some("python"), "owner/repo", dir.path(), &|_| mock_empty_list());

        let checks = result["checks"].as_array().unwrap();
        let pr_check: Vec<&Value> = checks
            .iter()
            .filter(|c| c["name"].as_str().unwrap().contains("PR"))
            .collect();
        assert!(!pr_check.is_empty());
        assert_eq!(pr_check[0]["passed"], false);
    }

    #[test]
    fn test_verify_pr_fetch_failure() {
        let dir = tempfile::tempdir().unwrap();

        let result = verify_impl(Some("python"), "owner/repo", dir.path(), &|_| None);

        let checks = result["checks"].as_array().unwrap();
        let pr_check: Vec<&Value> = checks
            .iter()
            .filter(|c| c["name"].as_str().unwrap().contains("PR"))
            .collect();
        assert!(!pr_check.is_empty());
        assert_eq!(pr_check[0]["passed"], false);
    }

    #[test]
    fn test_verify_no_flow_states_dir() {
        let dir = tempfile::tempdir().unwrap();
        // No .flow-states dir created

        let result = verify_impl(Some("python"), "owner/repo", dir.path(), &|_| mock_ok_pr());

        let checks = result["checks"].as_array().unwrap();
        let state_check: Vec<&Value> = checks
            .iter()
            .filter(|c| {
                c["name"]
                    .as_str()
                    .unwrap()
                    .to_lowercase()
                    .contains("state")
            })
            .collect();
        assert!(!state_check.is_empty());
        assert_eq!(state_check[0]["passed"], true);
    }

    #[test]
    fn test_verify_excludes_orchestrate_files() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir(&state_dir).unwrap();
        fs::write(state_dir.join("orchestrate-queue.json"), "{}").unwrap();

        let result = verify_impl(Some("python"), "owner/repo", dir.path(), &|_| mock_ok_pr());

        let checks = result["checks"].as_array().unwrap();
        let state_check: Vec<&Value> = checks
            .iter()
            .filter(|c| {
                c["name"]
                    .as_str()
                    .unwrap()
                    .to_lowercase()
                    .contains("state")
            })
            .collect();
        assert_eq!(state_check[0]["passed"], true);
    }

    #[test]
    fn test_verify_excludes_phases_files() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir(&state_dir).unwrap();
        fs::write(state_dir.join("feature-phases.json"), "{}").unwrap();

        let result = verify_impl(Some("python"), "owner/repo", dir.path(), &|_| mock_ok_pr());

        let checks = result["checks"].as_array().unwrap();
        let state_check: Vec<&Value> = checks
            .iter()
            .filter(|c| {
                c["name"]
                    .as_str()
                    .unwrap()
                    .to_lowercase()
                    .contains("state")
            })
            .collect();
        assert_eq!(state_check[0]["passed"], true);
    }

    #[test]
    fn test_verify_excludes_dot_prefixed_json() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir(&state_dir).unwrap();
        fs::write(state_dir.join(".hidden-state.json"), "{}").unwrap();

        let result = verify_impl(Some("python"), "owner/repo", dir.path(), &|_| mock_ok_pr());

        let checks = result["checks"].as_array().unwrap();
        let state_check: Vec<&Value> = checks
            .iter()
            .filter(|c| {
                c["name"]
                    .as_str()
                    .unwrap()
                    .to_lowercase()
                    .contains("state")
            })
            .collect();
        assert_eq!(state_check[0]["passed"], true);
    }

    // Tombstone tests — removed features that must not return

    #[test]
    fn test_qa_verify_no_decomposed_issue_check() {
        // Tombstone: removed in PR #729. Must not return.
        let dir = tempfile::tempdir().unwrap();

        let result = verify_impl(Some("python"), "owner/repo", dir.path(), &|_| mock_ok_pr());

        let check_names: Vec<&str> = result["checks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["name"].as_str().unwrap())
            .collect();
        assert!(!check_names.contains(&"Decomposed issue created"));
    }

    #[test]
    fn test_qa_verify_no_body_files_check() {
        // Tombstone: removed in PR #729. Must not return.
        let dir = tempfile::tempdir().unwrap();

        let result = verify_impl(Some("python"), "owner/repo", dir.path(), &|_| mock_ok_pr());

        let check_names: Vec<&str> = result["checks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["name"].as_str().unwrap())
            .collect();
        assert!(!check_names.contains(&"No leftover body files"));
    }
}
