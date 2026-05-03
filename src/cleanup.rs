//! Cleanup orchestrator for FLOW features.
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
//!
//! Tests live at tests/cleanup.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use std::fs;
use std::path::Path;
use std::process::Command;

use clap::Parser;
use indexmap::IndexMap;

use crate::commands::log::append_log;
use crate::flow_paths::FlowPaths;

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

/// Run a command in `cwd` via `Command::output()` without a timeout.
/// Returns `(success, trimmed-output)` where output is stderr on
/// failure (or stdout when stderr is empty).
fn run_cmd(args: &[&str], cwd: &Path) -> (bool, String) {
    match Command::new(args[0])
        .args(&args[1..])
        .current_dir(cwd)
        .output()
    {
        Ok(output) => {
            if output.status.success() {
                (
                    true,
                    String::from_utf8_lossy(&output.stdout).trim().to_string(),
                )
            } else {
                let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
                if err.is_empty() {
                    (
                        false,
                        String::from_utf8_lossy(&output.stdout).trim().to_string(),
                    )
                } else {
                    (false, err)
                }
            }
        }
        Err(e) => (false, e.to_string()),
    }
}

fn label_result(ok: bool, ok_label: &str, output: &str) -> String {
    if ok {
        ok_label.to_string()
    } else {
        format!("failed: {}", output)
    }
}

/// Recursively remove `<.flow-states>/<branch>/` and everything inside
/// it. The branch directory holds every per-branch artifact (state
/// file, log, plan, DAG, frozen phases, CI sentinel, timings,
/// closed-issues record, issues summary, scratch rule content, commit
/// message, start prompt, adversarial test files of any extension), so
/// a single recursive remove replaces the previous per-suffix
/// enumeration and the bespoke adversarial-test glob. Idempotent —
/// `NotFound` is treated as success because cleanup may run twice
/// (abort-then-complete in adjacent sessions, or a retry after a
/// partial failure).
fn try_remove_branch_dir(branch_dir: &Path) -> String {
    match fs::remove_dir_all(branch_dir) {
        Ok(()) => "deleted".to_string(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "skipped".to_string(),
        Err(e) => format!("failed: {}", e),
    }
}

/// Perform cleanup steps. Returns an ordered map of step results.
/// Called cross-module from `complete_finalize::run_impl_with_deps` as
/// well as from `run_impl_main` below.
///
/// `base_branch` is the integration branch the optional `--pull`
/// step targets via `git pull origin <base_branch>`; the caller
/// resolves it from the state file (or falls back to `"main"` for
/// legacy state files / the abort path with no state file).
pub fn cleanup(
    project_root: &Path,
    branch: &str,
    worktree: &str,
    pr_number: Option<i64>,
    pull: bool,
    base_branch: &str,
) -> IndexMap<String, String> {
    let mut steps = IndexMap::new();

    // Close PR (abort only)
    if let Some(pr) = pr_number {
        let (ok, output) = run_cmd(&["gh", "pr", "close", &pr.to_string()], project_root);
        steps.insert("pr_close".to_string(), label_result(ok, "closed", &output));
    } else {
        steps.insert("pr_close".to_string(), "skipped".to_string());
    }

    // Remove worktree tmp/ (FLOW repo only — before worktree removal)
    let is_flow_repo = project_root.join("flow-phases.json").exists();
    let wt_tmp = project_root.join(worktree).join("tmp");
    if is_flow_repo && wt_tmp.is_dir() {
        let (ok, output) = match fs::remove_dir_all(&wt_tmp) {
            Ok(()) => (true, String::new()),
            Err(e) => (false, format!("{}", e)),
        };
        steps.insert(
            "worktree_tmp".to_string(),
            label_result(ok, "removed", &output),
        );
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
        steps.insert("worktree".to_string(), label_result(ok, "removed", &output));
    } else {
        steps.insert("worktree".to_string(), "skipped".to_string());
    }

    // Delete remote branch (abort only — GitHub auto-deletes after merge)
    if pr_number.is_some() {
        let (ok, output) = run_cmd(&["git", "push", "origin", "--delete", branch], project_root);
        steps.insert(
            "remote_branch".to_string(),
            label_result(ok, "deleted", &output),
        );
    } else {
        steps.insert("remote_branch".to_string(), "skipped".to_string());
    }

    // Delete local branch
    let (ok, output) = run_cmd(&["git", "branch", "-D", branch], project_root);
    steps.insert(
        "local_branch".to_string(),
        label_result(ok, "deleted", &output),
    );

    // External-input audit: `branch` reaches cleanup directly from
    // complete-finalize's `--branch` CLI arg per
    // `.claude/rules/external-input-validation.md`. Slash-containing
    // or empty branches cannot address `.flow-states/<branch>/` —
    // use `try_new` and skip the branch-dir removal when the branch
    // is invalid. `--pull` still runs because it does not depend on
    // FlowPaths.
    let paths = match FlowPaths::try_new(project_root, branch) {
        Some(p) => p,
        None => {
            steps.insert(
                "branch_dir".to_string(),
                "skipped: invalid branch".to_string(),
            );
            if pull {
                let (ok, output) = run_cmd(&["git", "pull", "origin", base_branch], project_root);
                steps.insert("git_pull".to_string(), label_result(ok, "pulled", &output));
            }
            return steps;
        }
    };

    // Log cleanup progress before the branch directory (and therefore
    // the log file inside it) is removed. Only log if the log file
    // already exists — `append_log` creates the file if missing, which
    // would otherwise cause `try_remove_branch_dir` to remove a freshly
    // created file instead of a missing one and produce surprising
    // results in test fixtures that intentionally omit the log. This
    // entry is written mid-cleanup (before the dir removal), so it
    // cannot report a total step count — the JSON output has the full
    // step results.
    let log_path = paths.log_file();
    if log_path.exists() {
        let _ = append_log(
            project_root,
            branch,
            "[Phase 6] cleanup — in progress (branch directory will be removed next)",
        );
    }

    // Single recursive remove replaces the previous per-suffix delete
    // enumeration and the bespoke adversarial-test glob. Every
    // per-branch artifact (`state.json`, `log`, `plan.md`, `dag.md`,
    // `phases.json`, `ci-passed`, `timings.md`, `closed-issues.json`,
    // `issues.md`, `rule-content.md`, `commit-msg.txt`,
    // `commit-msg-content.txt`, `start-prompt`, `adversarial_test.*`)
    // lives under `branch_dir()`, so a single `remove_dir_all` is
    // sufficient and naturally handles future per-branch additions
    // without code changes.
    steps.insert(
        "branch_dir".to_string(),
        try_remove_branch_dir(&paths.branch_dir()),
    );

    // Pull latest origin/<base_branch> (after worktree removal —
    // ordering matters). `base_branch` flows in from the caller's
    // state-file read (defaulting to "main" for legacy state files).
    if pull {
        let (ok, output) = run_cmd(&["git", "pull", "origin", base_branch], project_root);
        steps.insert("git_pull".to_string(), label_result(ok, "pulled", &output));
    }

    steps
}

/// Main-arm dispatch: validate args.project_root and run cleanup.
/// Returns (JSON value, exit code).
///
/// `base_branch` is resolved from the per-branch state file via
/// `git::read_base_branch` and falls back to git's integration
/// branch (origin/HEAD) when the state file is missing, malformed,
/// or omits the field — both the abort path (state file may be
/// present but partially initialized) and pre-`base_branch`-field
/// state files are covered by the same fallback.
pub fn run_impl_main(args: &Args) -> (serde_json::Value, i32) {
    let root = Path::new(&args.project_root);
    if !root.is_dir() {
        let msg = format!("Project root not found: {}", args.project_root);
        let err_str = crate::output::json_error_string(&msg, &[]);
        return (serde_json::from_str(&err_str).unwrap(), 1);
    }

    let base_branch = FlowPaths::try_new(root, &args.branch)
        .and_then(|paths| crate::git::read_base_branch(&paths.state_file()).ok())
        .unwrap_or_else(|| crate::git::default_branch_in(root));

    let steps = cleanup(
        root,
        &args.branch,
        &args.worktree,
        args.pr,
        args.pull,
        &base_branch,
    );
    let steps_map: indexmap::IndexMap<String, serde_json::Value> = steps
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::String(v)))
        .collect();
    let steps_value = serde_json::to_value(steps_map).unwrap();
    let ok_str = crate::output::json_ok_string(&[("steps", steps_value)]);
    (serde_json::from_str(&ok_str).unwrap(), 0)
}
