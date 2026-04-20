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
use std::time::Duration;

use clap::Parser;
use indexmap::IndexMap;

use crate::commands::log::append_log;
use crate::flow_paths::FlowPaths;

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

/// Run a command with `CMD_TIMEOUT`, returning (success, output_string).
pub fn run_cmd(args: &[&str], cwd: &Path) -> (bool, String) {
    run_cmd_with_timeout(args, cwd, CMD_TIMEOUT)
}

/// Run a command with an explicit timeout, returning (success, output_string).
///
/// Spawns a worker thread running `Command::output()` and waits on a
/// channel with `recv_timeout`. On timeout, returns `(false, "timeout")`
/// and orphans the child (the child will exit on its own when its
/// underlying operation finishes). On success, returns
/// `(status.success(), stderr_or_stdout)`.
pub fn run_cmd_with_timeout(args: &[&str], cwd: &Path, timeout: Duration) -> (bool, String) {
    let (tx, rx) = std::sync::mpsc::channel();
    let args_owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    let cwd_owned = cwd.to_path_buf();
    std::thread::spawn(move || {
        let result = Command::new(&args_owned[0])
            .args(&args_owned[1..])
            .current_dir(&cwd_owned)
            .output();
        let _ = tx.send(result);
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(output)) => {
            if output.status.success() {
                return (
                    true,
                    String::from_utf8_lossy(&output.stdout).trim().to_string(),
                );
            }
            let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if error.is_empty() {
                (
                    false,
                    String::from_utf8_lossy(&output.stdout).trim().to_string(),
                )
            } else {
                (false, error)
            }
        }
        Ok(Err(e)) => (false, e.to_string()),
        Err(_) => (false, "timeout".to_string()),
    }
}

/// Format a (success, output) pair as either the success label or
/// `"failed: <output>"`. Used to convert every external command
/// result in `cleanup()` into the same step-string shape.
pub fn label_result(ok: bool, ok_label: &str, output: &str) -> String {
    if ok {
        ok_label.to_string()
    } else {
        format!("failed: {}", output)
    }
}

/// Try to remove a file, returning "deleted", "skipped", or "failed: <reason>".
pub fn try_delete_file(path: &Path) -> String {
    if path.exists() {
        match fs::remove_file(path) {
            Ok(()) => "deleted".to_string(),
            Err(e) => format!("failed: {}", e),
        }
    } else {
        "skipped".to_string()
    }
}

/// Remove every `{branch}-adversarial_test.*` file under `.flow-states/`.
///
/// The Phase 4 adversarial agent writes a single test file whose extension
/// it chooses at runtime from the diff's language (`.rs`, `.py`, `.go`,
/// `.swift`, `.ts`, `.rb`, etc.). Cleanup cannot know the extension ahead
/// of time, so it matches every entry whose file name starts with the
/// literal prefix `{branch}-adversarial_test.`.
///
/// The trailing dot in the prefix is load-bearing: without it, a file named
/// `{branch}-adversarial_test_other.rs` would match. The dot anchors the
/// match on the extension separator so only real adversarial test files
/// are deleted. Other concurrent flows' files (e.g.
/// `other-branch-adversarial_test.rs`) are prefixed with a different branch
/// name and are untouched.
///
/// Directory entries that happen to match the prefix are skipped — only
/// regular files and symlinks are candidates for deletion. `fs::remove_file`
/// on a symlink removes the link itself, not its target.
///
/// The loop continues past individual deletion errors so a single failure
/// (permission denied, directory entry, transient I/O error) does not
/// leave the remaining matching files on disk. The function returns
/// "skipped" if `.flow-states/` is missing or no regular-file entries
/// matched, "deleted" if at least one regular file was successfully
/// removed, or "failed: <reason>" when every matching regular file's
/// deletion failed (reporting the first error encountered).
pub fn try_delete_adversarial_test_files(flow_states: &Path, branch: &str) -> String {
    let entries = match fs::read_dir(flow_states) {
        Ok(iter) => iter,
        Err(_) => return "skipped".to_string(),
    };

    let prefix = format!("{}-adversarial_test.", branch);
    let mut any_matched = false;
    let mut any_deleted = false;
    let mut first_error = String::new();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if !name.starts_with(&prefix) {
            continue;
        }
        // Skip directory entries whose name happens to match the prefix
        // (e.g. a scratch folder created by an engineer or a future
        // feature). `path().is_dir()` follows symlinks so symlinks to
        // files are still eligible; bare symlinks (dangling) are also
        // eligible because `fs::remove_file` unlinks the link itself.
        let path = entry.path();
        if path.is_dir() {
            continue;
        }
        any_matched = true;
        match fs::remove_file(&path) {
            Ok(()) => {
                any_deleted = true;
            }
            Err(e) => {
                if first_error.is_empty() {
                    first_error = format!("{}", e);
                }
            }
        }
    }

    if any_deleted {
        "deleted".to_string()
    } else if any_matched {
        format!("failed: {}", first_error)
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
    // or empty branches cannot address flat `.flow-states/` paths —
    // use `try_new` and skip all path-dependent cleanup steps when
    // the branch is invalid. `--pull` still runs because it does
    // not depend on FlowPaths.
    let paths = match FlowPaths::try_new(project_root, branch) {
        Some(p) => p,
        None => {
            for key in [
                "state_file",
                "plan_file",
                "dag_file",
                "log_file",
                "frozen_phases",
                "ci_sentinel",
                "timings_file",
                "closed_issues_file",
                "issues_file",
                "adversarial_test",
            ] {
                steps.insert(key.to_string(), "skipped: invalid branch".to_string());
            }
            if pull {
                let (ok, output) = run_cmd(&["git", "pull", "origin", "main"], project_root);
                steps.insert("git_pull".to_string(), label_result(ok, "pulled", &output));
            }
            return steps;
        }
    };
    let flow_states = paths.flow_states_dir();
    steps.insert(
        "state_file".to_string(),
        try_delete_file(&paths.state_file()),
    );

    // Delete plan file
    steps.insert("plan_file".to_string(), try_delete_file(&paths.plan_file()));

    // Delete DAG file
    steps.insert("dag_file".to_string(), try_delete_file(&paths.dag_file()));

    // Log cleanup progress before the log file is deleted.
    // Only log if the log file already exists — append_log creates the file
    // if missing, which would cause try_delete_file to return "deleted" instead
    // of "skipped" for test fixtures that intentionally remove the log file.
    // This entry is written mid-cleanup (before file deletions), so it cannot
    // report a total step count — the JSON output has the full step results.
    let log_path = paths.log_file();
    if log_path.exists() {
        let _ = append_log(
            project_root,
            branch,
            "[Phase 6] cleanup — in progress (log file will be deleted next)",
        );
    }

    // Delete log file
    steps.insert("log_file".to_string(), try_delete_file(&paths.log_file()));

    // Delete frozen phases file
    steps.insert(
        "frozen_phases".to_string(),
        try_delete_file(&paths.frozen_phases()),
    );

    // Delete CI sentinel
    steps.insert(
        "ci_sentinel".to_string(),
        try_delete_file(&paths.ci_sentinel()),
    );

    // Delete timings file
    steps.insert(
        "timings_file".to_string(),
        try_delete_file(&paths.timings_file()),
    );

    // Delete closed issues file
    steps.insert(
        "closed_issues_file".to_string(),
        try_delete_file(&paths.closed_issues()),
    );

    // Delete issues file
    steps.insert(
        "issues_file".to_string(),
        try_delete_file(&paths.issues_file()),
    );

    // Delete adversarial test file(s) produced by the Phase 4 adversarial
    // agent. The agent chooses the extension at runtime from the diff's
    // language, so cleanup globs by the branch-scoped prefix instead of a
    // fixed filename. Covers both complete (pr_number=None) and abort
    // (pr_number=Some) paths since they share this function.
    steps.insert(
        "adversarial_test".to_string(),
        try_delete_adversarial_test_files(&flow_states, branch),
    );

    // Pull latest main (after worktree removal — ordering matters)
    if pull {
        let (ok, output) = run_cmd(&["git", "pull", "origin", "main"], project_root);
        steps.insert("git_pull".to_string(), label_result(ok, "pulled", &output));
    }

    steps
}

/// Main-arm dispatch: validate args.project_root and run cleanup.
/// Returns (JSON value, exit code).
pub fn run_impl_main(args: &Args) -> (serde_json::Value, i32) {
    let root = Path::new(&args.project_root);
    if !root.is_dir() {
        let msg = format!("Project root not found: {}", args.project_root);
        let err_str = crate::output::json_error_string(&msg, &[]);
        return (serde_json::from_str(&err_str).unwrap(), 1);
    }

    let steps = cleanup(root, &args.branch, &args.worktree, args.pr, args.pull);
    let steps_map: indexmap::IndexMap<String, serde_json::Value> = steps
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::String(v)))
        .collect();
    let steps_value = serde_json::to_value(steps_map).unwrap();
    let ok_str = crate::output::json_ok_string(&[("steps", steps_value)]);
    (serde_json::from_str(&ok_str).unwrap(), 0)
}
