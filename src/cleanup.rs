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
//! Each step reports one of: "removed"/"deleted"/"closed", "skipped", or "failed: <reason>".

use std::path::Path;
use std::process::Command;

use clap::Parser;
use indexmap::IndexMap;
use serde_json::json;

use crate::output::json_error;

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
    #[arg(long)]
    pub pr: Option<u32>,
    /// Run git pull origin main after cleanup
    #[arg(long)]
    pub pull: bool,
}

/// Run a command, returning (success, output).
pub fn run_cmd(args: &[&str], cwd: &Path) -> (bool, String) {
    match Command::new(args[0])
        .args(&args[1..])
        .current_dir(cwd)
        .output()
    {
        Ok(output) => {
            if output.status.success() {
                (true, String::from_utf8_lossy(&output.stdout).trim().to_string())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                (false, if stderr.is_empty() { stdout } else { stderr })
            }
        }
        Err(e) => (false, e.to_string()),
    }
}

/// Delete a file, returning "deleted", "skipped", or "failed: <reason>".
fn delete_file(path: &Path) -> String {
    if path.exists() {
        match std::fs::remove_file(path) {
            Ok(_) => "deleted".to_string(),
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
    pr_number: Option<u32>,
    pull: bool,
) -> IndexMap<String, String> {
    let mut steps = IndexMap::new();

    // Close PR (abort only)
    if let Some(pr) = pr_number {
        let (ok, output) = run_cmd(
            &["gh", "pr", "close", &pr.to_string()],
            project_root,
        );
        steps.insert(
            "pr_close".to_string(),
            if ok { "closed".to_string() } else { format!("failed: {}", output) },
        );
    } else {
        steps.insert("pr_close".to_string(), "skipped".to_string());
    }

    // Remove worktree tmp/ (FLOW repo only — before worktree removal)
    let is_flow_repo = project_root.join("flow-phases.json").exists();
    let wt_tmp = project_root.join(worktree).join("tmp");
    if is_flow_repo && wt_tmp.is_dir() {
        match std::fs::remove_dir_all(&wt_tmp) {
            Ok(_) => {
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
            if ok { "removed".to_string() } else { format!("failed: {}", output) },
        );
    } else {
        steps.insert("worktree".to_string(), "skipped".to_string());
    }

    // Delete remote branch
    let (ok, output) = run_cmd(
        &["git", "push", "origin", "--delete", branch],
        project_root,
    );
    steps.insert(
        "remote_branch".to_string(),
        if ok { "deleted".to_string() } else { format!("failed: {}", output) },
    );

    // Delete local branch
    let (ok, output) = run_cmd(
        &["git", "branch", "-D", branch],
        project_root,
    );
    steps.insert(
        "local_branch".to_string(),
        if ok { "deleted".to_string() } else { format!("failed: {}", output) },
    );

    // Delete state files
    let state_dir = project_root.join(".flow-states");
    steps.insert(
        "state_file".to_string(),
        delete_file(&state_dir.join(format!("{}.json", branch))),
    );
    steps.insert(
        "plan_file".to_string(),
        delete_file(&state_dir.join(format!("{}-plan.md", branch))),
    );
    steps.insert(
        "dag_file".to_string(),
        delete_file(&state_dir.join(format!("{}-dag.md", branch))),
    );
    steps.insert(
        "log_file".to_string(),
        delete_file(&state_dir.join(format!("{}.log", branch))),
    );
    steps.insert(
        "frozen_phases".to_string(),
        delete_file(&state_dir.join(format!("{}-phases.json", branch))),
    );
    steps.insert(
        "ci_sentinel".to_string(),
        delete_file(&state_dir.join(format!("{}-ci-passed", branch))),
    );
    steps.insert(
        "timings_file".to_string(),
        delete_file(&state_dir.join(format!("{}-timings.md", branch))),
    );
    steps.insert(
        "closed_issues_file".to_string(),
        delete_file(&state_dir.join(format!("{}-closed-issues.json", branch))),
    );
    steps.insert(
        "issues_file".to_string(),
        delete_file(&state_dir.join(format!("{}-issues.md", branch))),
    );

    // Pull latest main (after worktree removal — ordering matters)
    if pull {
        let (ok, output) = run_cmd(
            &["git", "pull", "origin", "main"],
            project_root,
        );
        steps.insert(
            "git_pull".to_string(),
            if ok { "pulled".to_string() } else { format!("failed: {}", output) },
        );
    }

    steps
}

/// CLI entry point for the cleanup subcommand.
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
    println!("{}", json!({"status": "ok", "steps": steps}));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command as StdCommand;

    /// Initialize a bare git repo with an initial commit.
    fn init_git_repo(root: &Path) {
        StdCommand::new("git")
            .args(["init"])
            .current_dir(root)
            .output()
            .unwrap();
        StdCommand::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(root)
            .output()
            .unwrap();
        StdCommand::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(root)
            .output()
            .unwrap();
        StdCommand::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(root)
            .output()
            .unwrap();
    }

    /// Create a worktree and state files for testing cleanup.
    fn setup_feature(root: &Path, branch: &str) -> String {
        let wt_rel = format!(".worktrees/{}", branch);
        StdCommand::new("git")
            .args(["worktree", "add", &wt_rel, "-b", branch])
            .current_dir(root)
            .output()
            .unwrap();

        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(
            state_dir.join(format!("{}.json", branch)),
            json!({"branch": branch}).to_string(),
        )
        .unwrap();
        fs::write(state_dir.join(format!("{}.log", branch)), "test log\n").unwrap();

        wt_rel
    }

    fn git_repo() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        init_git_repo(&root);
        (dir, root)
    }

    // --- run_cmd tests ---

    #[test]
    fn run_cmd_success() {
        let dir = tempfile::tempdir().unwrap();
        let (ok, output) = run_cmd(&["echo", "hello"], dir.path());
        assert!(ok);
        assert_eq!(output, "hello");
    }

    #[test]
    fn run_cmd_failure() {
        let dir = tempfile::tempdir().unwrap();
        let (ok, _output) = run_cmd(&["git", "status"], dir.path());
        // No git repo, so git status will fail or succeed depending on parent
        // Just verify it returns without panic
        let _ = ok;
    }

    #[test]
    fn run_cmd_handles_missing_command() {
        let dir = tempfile::tempdir().unwrap();
        let (ok, output) = run_cmd(&["nonexistent-command-xyz"], dir.path());
        assert!(!ok);
        assert!(!output.is_empty());
    }

    // --- cleanup removes worktree ---

    #[test]
    fn cleanup_removes_worktree() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["worktree"], "removed");
        assert!(!root.join(&wt_rel).exists());
    }

    // --- cleanup deletes state file ---

    #[test]
    fn cleanup_deletes_state_file() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["state_file"], "deleted");
        assert!(!root.join(".flow-states/test-feature.json").exists());
    }

    // --- cleanup deletes log file ---

    #[test]
    fn cleanup_deletes_log_file() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["log_file"], "deleted");
        assert!(!root.join(".flow-states/test-feature.log").exists());
    }

    // --- cleanup deletes plan file ---

    #[test]
    fn cleanup_deletes_plan_file() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let plan = root.join(".flow-states/test-feature-plan.md");
        fs::write(&plan, "# Plan\n").unwrap();
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["plan_file"], "deleted");
        assert!(!plan.exists());
    }

    #[test]
    fn cleanup_skips_missing_plan_file() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["plan_file"], "skipped");
    }

    // --- cleanup deletes DAG file ---

    #[test]
    fn cleanup_deletes_dag_file() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let dag = root.join(".flow-states/test-feature-dag.md");
        fs::write(&dag, "# DAG\n").unwrap();
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["dag_file"], "deleted");
        assert!(!dag.exists());
    }

    #[test]
    fn cleanup_skips_missing_dag_file() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["dag_file"], "skipped");
    }

    // --- cleanup deletes frozen phases file ---

    #[test]
    fn cleanup_deletes_frozen_phases_file() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let frozen = root.join(".flow-states/test-feature-phases.json");
        fs::write(&frozen, r#"{"phases": {}, "order": []}"#).unwrap();
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["frozen_phases"], "deleted");
        assert!(!frozen.exists());
    }

    #[test]
    fn cleanup_skips_missing_frozen_phases() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["frozen_phases"], "skipped");
    }

    // --- cleanup deletes CI sentinel ---

    #[test]
    fn cleanup_deletes_ci_sentinel() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let sentinel = root.join(".flow-states/test-feature-ci-passed");
        fs::write(&sentinel, "snapshot\n").unwrap();
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["ci_sentinel"], "deleted");
        assert!(!sentinel.exists());
    }

    #[test]
    fn cleanup_skips_missing_ci_sentinel() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["ci_sentinel"], "skipped");
    }

    // --- cleanup deletes timings file ---

    #[test]
    fn cleanup_deletes_timings_file() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let timings = root.join(".flow-states/test-feature-timings.md");
        fs::write(&timings, "| Phase | Duration |\n").unwrap();
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["timings_file"], "deleted");
        assert!(!timings.exists());
    }

    #[test]
    fn cleanup_skips_missing_timings_file() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["timings_file"], "skipped");
    }

    // --- cleanup deletes closed issues file ---

    #[test]
    fn cleanup_deletes_closed_issues_file() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let closed = root.join(".flow-states/test-feature-closed-issues.json");
        fs::write(&closed, r#"[{"number": 42, "url": "https://github.com/t/t/issues/42"}]"#).unwrap();
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["closed_issues_file"], "deleted");
        assert!(!closed.exists());
    }

    #[test]
    fn cleanup_skips_missing_closed_issues_file() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["closed_issues_file"], "skipped");
    }

    // --- cleanup deletes issues file ---

    #[test]
    fn cleanup_deletes_issues_file() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let issues = root.join(".flow-states/test-feature-issues.md");
        fs::write(&issues, "| Label | Title |\n").unwrap();
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["issues_file"], "deleted");
        assert!(!issues.exists());
    }

    #[test]
    fn cleanup_skips_missing_issues_file() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["issues_file"], "skipped");
    }

    // --- PR close ---

    #[test]
    fn cleanup_skips_pr_by_default() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["pr_close"], "skipped");
    }

    #[test]
    fn cleanup_pr_close_fails_gracefully() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let steps = cleanup(&root, "test-feature", &wt_rel, Some(999), false);
        // No GitHub remote configured, so gh pr close will fail
        assert!(steps["pr_close"].starts_with("failed:"));
    }

    // --- Full happy path ---

    #[test]
    fn cleanup_full_happy_path() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);

        // All step results
        assert_eq!(steps["pr_close"], "skipped");
        assert_eq!(steps["worktree_tmp"], "skipped");
        assert_eq!(steps["worktree"], "removed");
        assert!(steps["remote_branch"].starts_with("failed:")); // no remote
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
        assert!(!root.join(&wt_rel).exists());
        assert!(!root.join(".flow-states/test-feature.json").exists());
        assert!(!root.join(".flow-states/test-feature.log").exists());
    }

    // --- Key order matches Python ---

    #[test]
    fn cleanup_key_order_matches_python() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);

        let keys: Vec<&String> = steps.keys().collect();
        let expected = vec![
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
        ];
        assert_eq!(keys, expected);
    }

    #[test]
    fn cleanup_key_order_with_pull() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let steps = cleanup(&root, "test-feature", &wt_rel, None, true);

        let keys: Vec<&String> = steps.keys().collect();
        assert_eq!(keys.last().unwrap().as_str(), "git_pull");
        assert_eq!(keys.len(), 15); // 14 standard + git_pull
    }

    // --- Missing resources ---

    #[test]
    fn cleanup_skips_missing_worktree() {
        let (_dir, root) = git_repo();
        setup_feature(&root, "test-feature");
        // Remove worktree before cleanup
        StdCommand::new("git")
            .args(["worktree", "remove", ".worktrees/test-feature", "--force"])
            .current_dir(&root)
            .output()
            .unwrap();
        let steps = cleanup(&root, "test-feature", ".worktrees/test-feature", None, false);
        assert_eq!(steps["worktree"], "skipped");
    }

    #[test]
    fn cleanup_skips_missing_state_file() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        fs::remove_file(root.join(".flow-states/test-feature.json")).unwrap();
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["state_file"], "skipped");
    }

    #[test]
    fn cleanup_skips_missing_log_file() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        fs::remove_file(root.join(".flow-states/test-feature.log")).unwrap();
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["log_file"], "skipped");
    }

    // --- Branch deletion ---

    #[test]
    fn cleanup_always_deletes_local_branch() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        // Remove worktree first so branch can be deleted
        StdCommand::new("git")
            .args(["worktree", "remove", &wt_rel, "--force"])
            .current_dir(&root)
            .output()
            .unwrap();
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["local_branch"], "deleted");
    }

    #[test]
    fn cleanup_always_attempts_remote_branch() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        // No remote configured, so push --delete will fail
        assert!(steps["remote_branch"].starts_with("failed:"));
    }

    // --- tmp/ directory cleanup ---

    #[test]
    fn cleanup_removes_worktree_tmp_in_flow_repo() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        // Mark as FLOW repo
        fs::write(root.join("flow-phases.json"), "{}").unwrap();
        // Create tmp/ inside the worktree
        let wt_tmp = root.join(&wt_rel).join("tmp");
        fs::create_dir_all(&wt_tmp).unwrap();
        fs::write(wt_tmp.join("release-notes-v1.0.md"), "notes").unwrap();
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["worktree_tmp"], "removed");
    }

    #[test]
    fn cleanup_skips_tmp_without_flow_phases() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        // No flow-phases.json — not a FLOW repo
        let wt_tmp = root.join(&wt_rel).join("tmp");
        fs::create_dir_all(&wt_tmp).unwrap();
        fs::write(wt_tmp.join("some-file.txt"), "data").unwrap();
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["worktree_tmp"], "skipped");
    }

    #[test]
    fn cleanup_skips_missing_worktree_tmp() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        fs::write(root.join("flow-phases.json"), "{}").unwrap();
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert_eq!(steps["worktree_tmp"], "skipped");
    }

    // --- --pull flag tests ---

    #[test]
    fn no_pull_flag_no_git_pull_step() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        assert!(!steps.contains_key("git_pull"));
    }

    #[test]
    fn pull_flag_present_adds_step() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let steps = cleanup(&root, "test-feature", &wt_rel, None, true);
        assert!(steps.contains_key("git_pull"));
        // No remote configured, so pull will fail
        assert!(steps["git_pull"].starts_with("failed:"));
    }

    // --- JSON output format ---

    #[test]
    fn cleanup_json_output_format() {
        let (_dir, root) = git_repo();
        let wt_rel = setup_feature(&root, "test-feature");
        let steps = cleanup(&root, "test-feature", &wt_rel, None, false);
        let output = json!({"status": "ok", "steps": steps});
        let parsed: serde_json::Value = serde_json::from_str(&output.to_string()).unwrap();
        assert_eq!(parsed["status"], "ok");
        assert!(parsed["steps"].is_object());
        assert_eq!(parsed["steps"]["pr_close"], "skipped");
    }
}
