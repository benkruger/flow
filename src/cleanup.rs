//! Cleanup orchestrator for FLOW features.
//!
//! Two entry shapes:
//!
//! - **Per-branch (`--branch`)** — used by `/flow:flow-complete` (Phase 6)
//!   and `/flow:flow-abort`. Closes the PR, removes the worktree, deletes
//!   branches, removes the branch directory, and sweeps the start-lock
//!   queue entry for the named branch.
//! - **All-flows (`--all`)** — used by `/flow:flow-reset`. Walks every
//!   subdirectory of `.flow-states/` that contains a `state.json` and
//!   runs the per-branch cleanup against each flow. Then runs three
//!   machine-level tail steps: remove `orchestrate.json`, remove
//!   `.flow-states/main/`, and sweep any residual `start-queue/` entries
//!   left behind by interrupted starts. `--dry-run` returns an inventory
//!   of what would be removed without modifying disk.
//!
//! Per-branch usage:
//!   bin/flow cleanup <project_root> --branch <name> --worktree <path> [--pr <number>] [--pull]
//!
//! All-flows usage:
//!   bin/flow cleanup <project_root> --all [--dry-run]
//!
//! Per-branch output (JSON to stdout):
//!   {"status": "ok", "steps": {"pr_close": ..., "worktree": ..., "remote_branch": ...,
//!                              "local_branch": ..., "branch_dir": ..., "queue_entry": ...,
//!                              "git_pull": ...}}
//!
//! All-flows output (JSON to stdout):
//!   {"status": "ok", "dry_run": <bool>, "flows": [...], "orchestrate_json": ...,
//!    "main_dir": ..., "queue_sweep": ...}
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
use serde_json::{json, Map, Value};

use crate::commands::log::append_log;
use crate::commands::start_lock::QUEUE_DIRNAME;
use crate::flow_paths::{FlowPaths, FlowStatesDir};

#[derive(Parser, Debug)]
#[command(name = "cleanup", about = "FLOW cleanup orchestrator")]
pub struct Args {
    /// Path to project root
    pub project_root: String,

    /// Branch name (required unless --all)
    #[arg(long)]
    pub branch: Option<String>,

    /// Worktree path relative to project_root (required unless --all)
    #[arg(long)]
    pub worktree: Option<String>,

    /// PR number to close (per-branch mode only)
    #[arg(long = "pr")]
    pub pr: Option<i64>,

    /// Run git pull origin <base_branch> after per-branch cleanup
    #[arg(long)]
    pub pull: bool,

    /// Reset every flow on this machine. Walks `.flow-states/` for
    /// every subdirectory containing a `state.json` and runs the
    /// per-branch cleanup against it, then removes `orchestrate.json`,
    /// `.flow-states/main/`, and any residual `start-queue/` entries.
    /// Mutually exclusive with `--branch`.
    #[arg(long)]
    pub all: bool,

    /// With `--all`: print the inventory of what would be removed
    /// without modifying disk.
    #[arg(long = "dry-run")]
    pub dry_run: bool,
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

    // Remove worktree (the subsequent `git worktree remove --force`
    // also disposes of any worktree-internal scratch like `tmp/`, so a
    // separate per-tmp step is unnecessary).
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
            steps.insert(
                "queue_entry".to_string(),
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

    // Every per-branch artifact (`state.json`, `log`, `plan.md`,
    // `dag.md`, `phases.json`, `ci-passed`, `timings.md`,
    // `closed-issues.json`, `issues.md`, `rule-content.md`,
    // `commit-msg.txt`, `commit-msg-content.txt`, `start-prompt`)
    // lives under `branch_dir()`, so one `remove_dir_all` covers the
    // full set and naturally handles future per-branch additions
    // without code changes. Code Review's adversarial probe lives
    // inside the worktree's test tree (declared per-project via
    // `bin/test --adversarial-path`) and is disposed of by
    // `git worktree remove` later in this same cleanup pass — no
    // per-suffix glob is required here.
    steps.insert(
        "branch_dir".to_string(),
        try_remove_branch_dir(&paths.branch_dir()),
    );

    // Remove the start-lock queue entry for this branch, if present.
    // `start_init` writes `.flow-states/<QUEUE_DIRNAME>/<branch>` while
    // holding the start lock and `start_finalize` releases it on the
    // happy path; this step is defense-in-depth for the abort path and
    // any unusual case where Complete runs without a clean Start. The
    // queue_dir itself is left in place — `start_lock::queue_path`
    // recreates it on demand for subsequent flows.
    let queue_entry_path = paths.flow_states_dir().join(QUEUE_DIRNAME).join(branch);
    let queue_result = match fs::remove_file(&queue_entry_path) {
        Ok(()) => "removed".to_string(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "skipped".to_string(),
        Err(e) => format!("failed: {}", e),
    };
    steps.insert("queue_entry".to_string(), queue_result);

    // Pull latest origin/<base_branch> (after worktree removal —
    // ordering matters). `base_branch` flows in from the caller's
    // state-file read (defaulting to "main" for legacy state files).
    if pull {
        let (ok, output) = run_cmd(&["git", "pull", "origin", base_branch], project_root);
        steps.insert("git_pull".to_string(), label_result(ok, "pulled", &output));
    }

    steps
}

/// Reset every flow on this machine. Walks `.flow-states/` for branch
/// subdirectories that contain `state.json`, runs the per-branch
/// `cleanup()` against each (closing PRs, removing worktrees, deleting
/// branches, removing branch dirs, sweeping the matching queue entry),
/// then handles three machine-level tail steps:
///
/// - `orchestrate.json` removal — the machine-level orchestration
///   queue singleton.
/// - `.flow-states/main/` removal — the base-branch CI sentinel
///   directory written by `start-gate`.
/// - residual `start-queue/` entry sweep — entries left after
///   per-flow cleanups, e.g. orphans from interrupted starts.
///
/// `dry_run = true` returns an inventory of what would be removed
/// without modifying disk. The directory shells (`.flow-states/`,
/// `.flow-states/start-queue/`) are intentionally left in place even
/// in live mode — `start_lock::queue_path` and other downstream code
/// recreate them on demand. Subdirectories without `state.json`
/// (`main/`, `start-queue/`, transient cleanup remnants) are skipped
/// from the per-flow walk; the named tail steps cover the load-bearing
/// ones.
///
/// On a malformed `state.json`, the flow's entry in `flows[]` carries
/// an `"error"` field describing the parse failure and the per-flow
/// cleanup is skipped — the surrounding loop continues to other
/// flows so one corrupt state file cannot block a reset.
pub fn cleanup_all(project_root: &Path, dry_run: bool) -> Value {
    let states_dir = FlowStatesDir::new(project_root).path().to_path_buf();
    let mut flows: Vec<Value> = Vec::new();

    if states_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&states_dir) {
            let mut subdirs: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
                .collect();
            subdirs.sort_by_key(|e| e.file_name());

            for entry in subdirs {
                let name = entry.file_name();
                let branch_name = name.to_string_lossy().into_owned();
                let state_path = entry.path().join("state.json");
                if !state_path.is_file() {
                    // Subdir without state.json (e.g. `main/`,
                    // `start-queue/`, transient cleanup leftover).
                    // Not a flow — handled by tail steps or ignored.
                    continue;
                }

                let parsed: Result<Value, String> = match fs::read_to_string(&state_path) {
                    Ok(content) => serde_json::from_str::<Value>(&content)
                        .map_err(|e| format!("parse error: {}", e)),
                    Err(e) => Err(format!("read error: {}", e)),
                };

                let mut flow_obj: Map<String, Value> = Map::new();
                flow_obj.insert("branch".to_string(), Value::String(branch_name.clone()));

                match parsed {
                    Err(error) => {
                        flow_obj.insert("error".to_string(), Value::String(error));
                    }
                    Ok(state) => {
                        let worktree_rel = state
                            .get("worktree")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let pr_number = state.get("pr_number").and_then(|v| v.as_i64());
                        let base_branch = state
                            .get("base_branch")
                            .and_then(|v| v.as_str())
                            .unwrap_or("main")
                            .to_string();

                        flow_obj
                            .insert("worktree".to_string(), Value::String(worktree_rel.clone()));
                        flow_obj.insert(
                            "pr_number".to_string(),
                            match pr_number {
                                Some(n) => Value::from(n),
                                None => Value::Null,
                            },
                        );

                        if !dry_run {
                            let steps = cleanup(
                                project_root,
                                &branch_name,
                                &worktree_rel,
                                pr_number,
                                false, // never pull during --all
                                &base_branch,
                            );
                            let steps_map: Map<String, Value> = steps
                                .into_iter()
                                .map(|(k, v)| (k, Value::String(v)))
                                .collect();
                            flow_obj.insert("steps".to_string(), Value::Object(steps_map));
                        }
                    }
                }

                flows.push(Value::Object(flow_obj));
            }
        }
    }

    // Tail step: orchestrate.json removal.
    let orchestrate_path = states_dir.join("orchestrate.json");
    let orchestrate_json = if dry_run {
        if orchestrate_path.is_file() {
            "would_remove".to_string()
        } else {
            "skipped".to_string()
        }
    } else {
        match fs::remove_file(&orchestrate_path) {
            Ok(()) => "deleted".to_string(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => "skipped".to_string(),
            Err(e) => format!("failed: {}", e),
        }
    };

    // Tail step: `.flow-states/main/` directory removal.
    let main_path = states_dir.join("main");
    let main_dir = if dry_run {
        if main_path.is_dir() {
            "would_remove".to_string()
        } else {
            "skipped".to_string()
        }
    } else if main_path.is_dir() {
        match fs::remove_dir_all(&main_path) {
            Ok(()) => "removed".to_string(),
            Err(e) => format!("failed: {}", e),
        }
    } else {
        "skipped".to_string()
    };

    // Tail step: residual start-queue/ entry sweep. The queue_dir
    // itself is left in place — `start_lock::queue_path` recreates
    // it on demand for subsequent flow-starts.
    let queue_dir = states_dir.join(QUEUE_DIRNAME);
    let queue_sweep = match fs::read_dir(&queue_dir) {
        Ok(entries) => {
            let files: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
                .collect();
            if files.is_empty() {
                "skipped".to_string()
            } else if dry_run {
                format!("would_sweep {} entries", files.len())
            } else {
                let mut count = 0usize;
                let mut first_err: Option<String> = None;
                for f in &files {
                    match fs::remove_file(f.path()) {
                        Ok(()) => count += 1,
                        Err(e) => {
                            if first_err.is_none() {
                                first_err = Some(format!("{}", e));
                            }
                        }
                    }
                }
                if count > 0 {
                    format!("swept {} entries", count)
                } else {
                    // count==0 with a non-empty `files` list means every
                    // remove_file failed, so first_err is guaranteed
                    // Some — the loop sets it on the first error.
                    format!(
                        "failed: {}",
                        first_err.expect("count==0 with non-empty files implies first_err is set")
                    )
                }
            }
        }
        Err(_) => "skipped".to_string(),
    };

    json!({
        "status": "ok",
        "dry_run": dry_run,
        "flows": flows,
        "orchestrate_json": orchestrate_json,
        "main_dir": main_dir,
        "queue_sweep": queue_sweep,
    })
}

/// Main-arm dispatch: validate args.project_root and run cleanup.
/// Returns (JSON value, exit code).
///
/// Two modes:
/// - `--all`: invoke [`cleanup_all`] over every flow on disk.
/// - `--branch <name> --worktree <path>`: invoke [`cleanup`] for the
///   single flow.
///
/// Per-branch `base_branch` is resolved from the per-branch state file
/// via `git::read_base_branch` and falls back to git's integration
/// branch (`origin/HEAD`) when the state file is missing, malformed,
/// or omits the field — both the abort path (state file may be
/// partially initialized) and pre-`base_branch`-field state files are
/// covered by the same fallback. `--all` resolves `base_branch`
/// per-flow inside [`cleanup_all`].
pub fn run_impl_main(args: &Args) -> (Value, i32) {
    let root = Path::new(&args.project_root);
    if !root.is_dir() {
        let msg = format!("Project root not found: {}", args.project_root);
        let err_str = crate::output::json_error_string(&msg, &[]);
        return (serde_json::from_str(&err_str).unwrap(), 1);
    }

    if args.all {
        return (cleanup_all(root, args.dry_run), 0);
    }

    // Per-branch mode: --branch and --worktree are required.
    let branch = match args.branch.as_deref() {
        Some(b) => b,
        None => {
            let err_str = crate::output::json_error_string(
                "Either --branch (with --worktree) or --all is required",
                &[],
            );
            return (serde_json::from_str(&err_str).unwrap(), 1);
        }
    };
    let worktree = match args.worktree.as_deref() {
        Some(w) => w,
        None => {
            let err_str = crate::output::json_error_string(
                "--worktree is required when --branch is set",
                &[],
            );
            return (serde_json::from_str(&err_str).unwrap(), 1);
        }
    };

    let base_branch = FlowPaths::try_new(root, branch)
        .and_then(|paths| crate::git::read_base_branch(&paths.state_file()).ok())
        .unwrap_or_else(|| crate::git::default_branch_in(root));

    let steps = cleanup(root, branch, worktree, args.pr, args.pull, &base_branch);
    let steps_map: IndexMap<String, Value> = steps
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();
    let steps_value = serde_json::to_value(steps_map).unwrap();
    let ok_str = crate::output::json_ok_string(&[("steps", steps_value)]);
    (serde_json::from_str(&ok_str).unwrap(), 0)
}
