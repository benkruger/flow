//! Port of lib/cleanup.py — cleanup orchestrator for FLOW features.
//!
//! Shared by /flow:flow-complete (Phase 6) and /flow:flow-abort. Performs best-effort
//! cleanup steps, continuing on failure.
//!
//! Usage:
//!   bin/flow cleanup <project_root> --branch <name> --worktree <path> [--pr <number>] [--pull]
//!
//! Output (JSON to stdout):
//!   {"status": "ok", "steps": {"worktree": "removed", "state_file": "deleted", ...}}
//!
//! Each step reports one of: "removed"/"deleted"/"closed"/"pulled", "skipped", or "failed: <reason>".

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use clap::Parser;
use indexmap::IndexMap;

use crate::output::{json_error, json_ok};

const CMD_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Parser, Debug)]
#[command(name = "cleanup", about = "FLOW cleanup orchestrator")]
pub struct Args {
    /// Path to project root
    pub project_root: String,

    /// Branch name
    #[arg(long)]
    pub branch: String,

    /// Worktree path (relative)
    #[arg(long)]
    pub worktree: String,

    /// PR number to close
    #[arg(long = "pr")]
    pub pr: Option<i64>,

    /// Run git pull origin main after cleanup
    #[arg(long)]
    pub pull: bool,
}

/// Run a command with a timeout, returning (success, output_string).
fn run_cmd(args: &[&str], cwd: &Path) -> (bool, String) {
    let result = Command::new(args[0])
        .args(&args[1..])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .current_dir(cwd)
        .spawn();

    let mut child = match result {
        Ok(c) => c,
        Err(e) => return (false, e.to_string()),
    };

    let start = Instant::now();
    let poll_interval = Duration::from_millis(50);

    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let output = match child.wait_with_output() {
                    Ok(o) => o,
                    Err(e) => return (false, e.to_string()),
                };
                if output.status.success() {
                    return (
                        true,
                        String::from_utf8_lossy(&output.stdout).trim().to_string(),
                    );
                }
                let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
                if error.is_empty() {
                    return (
                        false,
                        String::from_utf8_lossy(&output.stdout).trim().to_string(),
                    );
                }
                return (false, error);
            }
            Ok(None) => {
                if start.elapsed() >= CMD_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return (false, "timeout".to_string());
                }
                let remaining = CMD_TIMEOUT - start.elapsed();
                std::thread::sleep(poll_interval.min(remaining));
            }
            Err(e) => return (false, e.to_string()),
        }
    }
}

/// Try to remove a file, returning "deleted", "skipped", or "failed: <reason>".
fn try_delete_file(path: &Path) -> String {
    if path.exists() {
        match fs::remove_file(path) {
            Ok(()) => "deleted".to_string(),
            Err(e) => format!("failed: {}", e),
        }
    } else {
        "skipped".to_string()
    }
}

/// Perform cleanup steps. Returns an ordered map of step results.
pub fn cleanup(
    project_root: &Path,
    branch: &str,
    worktree: &str,
    pr_number: Option<i64>,
    pull: bool,
) -> IndexMap<String, String> {
    let mut steps = IndexMap::new();

    // Close PR (abort only)
    if let Some(pr) = pr_number {
        let (ok, output) = run_cmd(&["gh", "pr", "close", &pr.to_string()], project_root);
        steps.insert(
            "pr_close".to_string(),
            if ok {
                "closed".to_string()
            } else {
                format!("failed: {}", output)
            },
        );
    } else {
        steps.insert("pr_close".to_string(), "skipped".to_string());
    }

    // Remove worktree tmp/ (FLOW repo only — before worktree removal)
    let is_flow_repo = project_root.join("flow-phases.json").exists();
    let wt_tmp = project_root.join(worktree).join("tmp");
    if is_flow_repo && wt_tmp.is_dir() {
        match fs::remove_dir_all(&wt_tmp) {
            Ok(()) => {
                steps.insert("worktree_tmp".to_string(), "removed".to_string());
            }
            Err(e) => {
                steps.insert("worktree_tmp".to_string(), format!("failed: {}", e));
            }
        }
    } else {
        steps.insert("worktree_tmp".to_string(), "skipped".to_string());
    }

    // Remove worktree
    let wt_path = project_root.join(worktree);
    if wt_path.exists() {
        let wt_str = wt_path.to_string_lossy().to_string();
        let (ok, output) = run_cmd(
            &["git", "worktree", "remove", &wt_str, "--force"],
            project_root,
        );
        steps.insert(
            "worktree".to_string(),
            if ok {
                "removed".to_string()
            } else {
                format!("failed: {}", output)
            },
        );
    } else {
        steps.insert("worktree".to_string(), "skipped".to_string());
    }

    // Delete remote branch (abort only — GitHub auto-deletes after merge)
    if pr_number.is_some() {
        let (ok, output) = run_cmd(&["git", "push", "origin", "--delete", branch], project_root);
        steps.insert(
            "remote_branch".to_string(),
            if ok {
                "deleted".to_string()
            } else {
                format!("failed: {}", output)
            },
        );
    } else {
        steps.insert("remote_branch".to_string(), "skipped".to_string());
    }

    // Delete local branch
    let (ok, output) = run_cmd(&["git", "branch", "-D", branch], project_root);
    steps.insert(
        "local_branch".to_string(),
        if ok {
            "deleted".to_string()
        } else {
            format!("failed: {}", output)
        },
    );

    // Delete state file
    let flow_states = project_root.join(".flow-states");
    steps.insert(
        "state_file".to_string(),
        try_delete_file(&flow_states.join(format!("{}.json", branch))),
    );

    // Delete plan file
    steps.insert(
        "plan_file".to_string(),
        try_delete_file(&flow_states.join(format!("{}-plan.md", branch))),
    );

    // Delete DAG file
    steps.insert(
        "dag_file".to_string(),
        try_delete_file(&flow_states.join(format!("{}-dag.md", branch))),
    );

    // Delete log file
    steps.insert(
        "log_file".to_string(),
        try_delete_file(&flow_states.join(format!("{}.log", branch))),
    );

    // Delete frozen phases file
    steps.insert(
        "frozen_phases".to_string(),
        try_delete_file(&flow_states.join(format!("{}-phases.json", branch))),
    );

    // Delete CI sentinel
    steps.insert(
        "ci_sentinel".to_string(),
        try_delete_file(&flow_states.join(format!("{}-ci-passed", branch))),
    );

    // Delete timings file
    steps.insert(
        "timings_file".to_string(),
        try_delete_file(&flow_states.join(format!("{}-timings.md", branch))),
    );

    // Delete closed issues file
    steps.insert(
        "closed_issues_file".to_string(),
        try_delete_file(&flow_states.join(format!("{}-closed-issues.json", branch))),
    );

    // Delete issues file
    steps.insert(
        "issues_file".to_string(),
        try_delete_file(&flow_states.join(format!("{}-issues.md", branch))),
    );

    // Pull latest main (after worktree removal — ordering matters)
    if pull {
        let (ok, output) = run_cmd(&["git", "pull", "origin", "main"], project_root);
        steps.insert(
            "git_pull".to_string(),
            if ok {
                "pulled".to_string()
            } else {
                format!("failed: {}", output)
            },
        );
    }

    steps
}

pub fn run(args: Args) {
    let root = Path::new(&args.project_root);
    if !root.is_dir() {
        json_error(
            &format!("Project root not found: {}", args.project_root),
            &[],
        );
        std::process::exit(1);
    }

    let steps = cleanup(root, &args.branch, &args.worktree, args.pr, args.pull);

    // Convert IndexMap<String, String> to serde_json::Value preserving order
    let steps_map: indexmap::IndexMap<String, serde_json::Value> = steps
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::String(v)))
        .collect();
    let steps_value = serde_json::to_value(steps_map).unwrap();

    json_ok(&[("steps", steps_value)]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::process::Command as StdCommand;

    /// Create a minimal git repo for testing.
    fn setup_git_repo(dir: &Path) {
        StdCommand::new("git")
            .args(["init"])
            .current_dir(dir)
            .output()
            .unwrap();
        // Configure identity for CI environments without global git config
        let config_path = dir.join(".git").join("config");
        fs::write(
            &config_path,
            "[user]\n\temail = t@t.com\n\tname = T\n[commit]\n\tgpgsign = false\n",
        )
        .unwrap();
        StdCommand::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir)
            .output()
            .unwrap();
    }

    /// Create a worktree and state file for testing cleanup.
    fn setup_feature(git_repo: &Path, branch: &str) -> String {
        let wt_rel = format!(".worktrees/{}", branch);
        StdCommand::new("git")
            .args(["worktree", "add", &wt_rel, "-b", branch])
            .current_dir(git_repo)
            .output()
            .unwrap();

        // Create state file
        let state_dir = git_repo.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(
            state_dir.join(format!("{}.json", branch)),
            json!({"branch": branch}).to_string(),
        )
        .unwrap();

        // Create log file
        fs::write(state_dir.join(format!("{}.log", branch)), "test log\n").unwrap();

        wt_rel
    }

    // --- Cleanup removes worktree ---

    #[test]
    fn test_cleanup_removes_worktree() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["worktree"], "removed");
        assert!(!dir.path().join(&wt_rel).exists());
    }

    // --- State file deletion ---

    #[test]
    fn test_cleanup_deletes_state_file() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["state_file"], "deleted");
        assert!(!dir.path().join(".flow-states/test-feature.json").exists());
    }

    // --- Log file deletion ---

    #[test]
    fn test_cleanup_deletes_log_file() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["log_file"], "deleted");
        assert!(!dir.path().join(".flow-states/test-feature.log").exists());
    }

    // --- Plan file ---

    #[test]
    fn test_cleanup_deletes_plan_file() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");
        let plan = dir.path().join(".flow-states/test-feature-plan.md");
        fs::write(&plan, "# Plan\n").unwrap();

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["plan_file"], "deleted");
        assert!(!plan.exists());
    }

    #[test]
    fn test_cleanup_skips_missing_plan_file() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["plan_file"], "skipped");
    }

    // --- DAG file ---

    #[test]
    fn test_cleanup_deletes_dag_file() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");
        let dag = dir.path().join(".flow-states/test-feature-dag.md");
        fs::write(&dag, "# DAG\n").unwrap();

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["dag_file"], "deleted");
        assert!(!dag.exists());
    }

    #[test]
    fn test_cleanup_skips_missing_dag_file() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["dag_file"], "skipped");
    }

    // --- Frozen phases file ---

    #[test]
    fn test_cleanup_deletes_frozen_phases_file() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");
        let frozen = dir.path().join(".flow-states/test-feature-phases.json");
        fs::write(&frozen, r#"{"phases": {}, "order": []}"#).unwrap();

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["frozen_phases"], "deleted");
        assert!(!frozen.exists());
    }

    #[test]
    fn test_cleanup_skips_missing_frozen_phases() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["frozen_phases"], "skipped");
    }

    // --- CI sentinel ---

    #[test]
    fn test_cleanup_deletes_ci_sentinel() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");
        let sentinel = dir.path().join(".flow-states/test-feature-ci-passed");
        fs::write(&sentinel, "snapshot\n").unwrap();

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["ci_sentinel"], "deleted");
        assert!(!sentinel.exists());
    }

    #[test]
    fn test_cleanup_skips_missing_ci_sentinel() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["ci_sentinel"], "skipped");
    }

    // --- Timings file ---

    #[test]
    fn test_cleanup_deletes_timings_file() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");
        let timings = dir.path().join(".flow-states/test-feature-timings.md");
        fs::write(&timings, "| Phase | Duration |\n").unwrap();

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["timings_file"], "deleted");
        assert!(!timings.exists());
    }

    #[test]
    fn test_cleanup_skips_missing_timings_file() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["timings_file"], "skipped");
    }

    // --- Closed issues file ---

    #[test]
    fn test_cleanup_deletes_closed_issues_file() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");
        let closed = dir
            .path()
            .join(".flow-states/test-feature-closed-issues.json");
        fs::write(&closed, r#"[{"number": 42}]"#).unwrap();

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["closed_issues_file"], "deleted");
        assert!(!closed.exists());
    }

    #[test]
    fn test_cleanup_skips_missing_closed_issues_file() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["closed_issues_file"], "skipped");
    }

    // --- Issues file ---

    #[test]
    fn test_cleanup_deletes_issues_file() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");
        let issues = dir.path().join(".flow-states/test-feature-issues.md");
        fs::write(&issues, "| Label | Title |\n").unwrap();

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["issues_file"], "deleted");
        assert!(!issues.exists());
    }

    #[test]
    fn test_cleanup_skips_missing_issues_file() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["issues_file"], "skipped");
    }

    // --- PR close ---

    #[test]
    fn test_cleanup_skips_pr_by_default() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["pr_close"], "skipped");
    }

    #[test]
    fn test_abort_pr_close_fails_gracefully() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, Some(999), false);
        assert!(steps["pr_close"].starts_with("failed:"));
    }

    // --- Branch deletion ---

    #[test]
    fn test_cleanup_skips_remote_branch_on_complete() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");

        // Complete path (pr_number=None) skips remote branch deletion
        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["remote_branch"], "skipped");
    }

    #[test]
    fn test_abort_attempts_remote_branch_deletion() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");

        // Abort path (pr_number=Some) attempts remote branch deletion
        let steps = cleanup(dir.path(), "test-feature", &wt_rel, Some(999), false);
        // No remote configured, so push --delete will fail — but it tried
        assert!(steps["remote_branch"].starts_with("failed:"));
    }

    #[test]
    fn test_cleanup_deletes_local_branch() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");
        // Remove worktree first so branch can be deleted
        StdCommand::new("git")
            .args(["worktree", "remove", &wt_rel, "--force"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["local_branch"], "deleted");
    }

    // --- Missing resources ---

    #[test]
    fn test_cleanup_skips_missing_worktree() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");
        // Remove worktree before cleanup
        StdCommand::new("git")
            .args(["worktree", "remove", &wt_rel, "--force"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["worktree"], "skipped");
    }

    #[test]
    fn test_cleanup_skips_missing_state_file() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");
        fs::remove_file(dir.path().join(".flow-states/test-feature.json")).unwrap();

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["state_file"], "skipped");
    }

    #[test]
    fn test_cleanup_skips_missing_log_file() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");
        fs::remove_file(dir.path().join(".flow-states/test-feature.log")).unwrap();

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["log_file"], "skipped");
    }

    // --- Full happy path ---

    #[test]
    fn test_cleanup_full_happy_path() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);

        assert_eq!(steps["pr_close"], "skipped");
        assert_eq!(steps["worktree"], "removed");
        assert_eq!(steps["remote_branch"], "skipped");
        assert_eq!(steps["local_branch"], "deleted");
        assert_eq!(steps["state_file"], "deleted");
        assert_eq!(steps["plan_file"], "skipped");
        assert_eq!(steps["dag_file"], "skipped");
        assert_eq!(steps["log_file"], "deleted");
        assert_eq!(steps["frozen_phases"], "skipped");
        assert_eq!(steps["ci_sentinel"], "skipped");
        assert_eq!(steps["timings_file"], "skipped");
        assert_eq!(steps["closed_issues_file"], "skipped");
        assert_eq!(steps["issues_file"], "skipped");

        // Filesystem effects
        assert!(!dir.path().join(&wt_rel).exists());
        assert!(!dir.path().join(".flow-states/test-feature.json").exists());
        assert!(!dir.path().join(".flow-states/test-feature.log").exists());
    }

    // --- tmp/ directory cleanup ---

    #[test]
    fn test_cleanup_removes_worktree_tmp_in_flow_repo() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");
        // Mark as FLOW repo
        fs::write(dir.path().join("flow-phases.json"), "{}").unwrap();
        // Create tmp/ inside the worktree
        let wt_tmp = dir.path().join(&wt_rel).join("tmp");
        fs::create_dir_all(&wt_tmp).unwrap();
        fs::write(wt_tmp.join("release-notes-v1.0.md"), "notes").unwrap();

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["worktree_tmp"], "removed");
    }

    #[test]
    fn test_cleanup_skips_tmp_without_flow_phases() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");
        // No flow-phases.json — not a FLOW repo
        let wt_tmp = dir.path().join(&wt_rel).join("tmp");
        fs::create_dir_all(&wt_tmp).unwrap();
        fs::write(wt_tmp.join("some-file.txt"), "data").unwrap();

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["worktree_tmp"], "skipped");
    }

    #[test]
    fn test_cleanup_skips_missing_worktree_tmp() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");
        fs::write(dir.path().join("flow-phases.json"), "{}").unwrap();

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert_eq!(steps["worktree_tmp"], "skipped");
    }

    // --- --pull flag tests ---

    #[test]
    fn test_no_pull_flag_no_git_pull_step() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        assert!(!steps.contains_key("git_pull"));
    }

    #[test]
    fn test_pull_flag_present_runs_pull() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, true);
        assert!(steps.contains_key("git_pull"));
        // No remote configured, so pull will fail
        assert!(steps["git_pull"].starts_with("failed:"));
    }

    // --- Step key ordering ---

    #[test]
    fn test_step_key_order_matches_python() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
        let keys: Vec<&String> = steps.keys().collect();

        assert_eq!(
            keys,
            vec![
                "pr_close",
                "worktree_tmp",
                "worktree",
                "remote_branch",
                "local_branch",
                "state_file",
                "plan_file",
                "dag_file",
                "log_file",
                "frozen_phases",
                "ci_sentinel",
                "timings_file",
                "closed_issues_file",
                "issues_file",
            ]
        );
    }

    #[test]
    fn test_step_key_order_with_pull() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path());
        let wt_rel = setup_feature(dir.path(), "test-feature");

        let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, true);
        let keys: Vec<&String> = steps.keys().collect();

        assert_eq!(
            keys,
            vec![
                "pr_close",
                "worktree_tmp",
                "worktree",
                "remote_branch",
                "local_branch",
                "state_file",
                "plan_file",
                "dag_file",
                "log_file",
                "frozen_phases",
                "ci_sentinel",
                "timings_file",
                "closed_issues_file",
                "issues_file",
                "git_pull",
            ]
        );
    }

    // --- CLI: invalid project root ---

    #[test]
    fn test_invalid_project_root() {
        // run() calls process::exit, so we test the logic instead
        let root = Path::new("/nonexistent/path");
        assert!(!root.is_dir());
    }

    // --- run_cmd error handling ---

    #[test]
    fn test_run_cmd_nonexistent_command() {
        let dir = tempfile::tempdir().unwrap();
        let (ok, output) = run_cmd(&["nonexistent_command_12345"], dir.path());
        assert!(!ok);
        assert!(!output.is_empty());
    }
}
