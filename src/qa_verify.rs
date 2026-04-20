//! Verify QA assertions after a completed flow.
//!
//! Usage: bin/flow qa-verify --repo <owner/repo>
//!
//! Checks post-Complete outcomes: cleanup (no leftover state files or
//! worktrees) and at least one merged PR.
//!
//! Always exits 0 — qa-verify is a pure reporting command that prints
//! its assertions as JSON for the flow-qa skill to parse and decide
//! pass/fail. Output is emitted compactly because the consumer is
//! programmatic and pretty-printing would just bloat the log.
//!
//! Tests live at tests/qa_verify.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::flow_paths::FlowStatesDir;

use clap::Parser;
use serde_json::{json, Value};

#[derive(Parser, Debug)]
#[command(
    name = "qa-verify",
    about = "Verify QA assertions after a completed flow"
)]
pub struct Args {
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
    let state_dir = FlowStatesDir::new(project_root).path().to_path_buf();
    let mut results = Vec::new();
    let entries = match std::fs::read_dir(&state_dir) {
        Ok(e) => e,
        Err(_) => return results,
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        // Skip dot-prefixed entries — `*.json` follows the fnmatch
        // convention where `*` does not match a leading dot, so a
        // stray `.local.json` from another tool does not get treated
        // as a flow state file.
        let name = file_name.to_string_lossy();
        if name.ends_with(".json")
            && !name.starts_with('.')
            && !name.starts_with("orchestrate")
            && !name.ends_with("-phases.json")
        {
            results.push(entry.path());
        }
    }
    results
}

/// Verify post-Complete outcomes with an injectable command runner.
///
/// The runner takes a slice of command args and returns Some(stdout) on
/// success, None on failure.
pub fn verify_impl(
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
    let worktree_count = match std::fs::read_dir(&worktrees_dir) {
        Ok(entries) => entries.count(),
        Err(_) => 0,
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
        "gh", "pr", "list", "--repo", repo, "--state", "merged", "--limit", "1", "--json", "number",
    ];
    match runner(&pr_args) {
        Some(stdout) => {
            let pr_list: Vec<Value> = serde_json::from_str(&stdout).unwrap_or_default();
            let has_merged = !pr_list.is_empty();
            let detail = if has_merged {
                format!("PR #{} merged", pr_list[0]["number"].as_i64().unwrap_or(0))
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

/// Default subprocess runner: spawn `cmd_args[0]` with remaining args,
/// return Some(stdout) on exit 0, None otherwise. Extracted into a
/// `pub fn` so tests can drive it directly without env-var manipulation
/// to satisfy the closure's code paths.
pub fn subprocess_runner(cmd_args: &[&str]) -> Option<String> {
    let output = Command::new(cmd_args[0])
        .args(&cmd_args[1..])
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        None
    }
}

/// CLI entry point.
///
/// Returns Ok(Value) always — qa-verify has no error exit path.
/// Returns Err(String) only for infrastructure failures.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let project_root = Path::new(&args.project_root);
    Ok(verify_impl(&args.repo, project_root, &subprocess_runner))
}
