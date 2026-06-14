//! `bin/flow complete-finalize` — consolidated post-merge + cleanup.
//!
//! Combines complete-post-merge and cleanup into a single process,
//! eliminating the `cd <project_root>` step between them. Both
//! `post_merge` and `cleanup` use explicit paths, so they compose
//! naturally without changing the shell working directory.
//!
//! Usage: bin/flow complete-finalize --pr <N> --state-file <path>
//!        --branch <name> --worktree <path> [--pull]
//!
//! Output (JSON to stdout):
//!   {"status": "ok", "formatted_time": "...", "summary": "...",
//!    "issues_links": "...", "banner_line": "...", "cleanup": {...}}
//!
//! Two optional warning fields ride on the otherwise-`ok` result; both
//! report best-effort post-merge work that failed without rolling back
//! the landed merge:
//!   - "post_merge_failures": {...} — present when any best-effort
//!     post-merge subcommand (render-pr-body, close-issues, etc.) failed.
//!   - "base_ci": {"status": "failed", "ci": {...}} — present when the
//!     sentinel-gated integration-branch CI run (after a clean `--pull`)
//!     failed. The CI error envelope is carried under `ci`. SKILL Step 5
//!     reports it to the user; it is a warning, not a rollback.
//!
//! Tests live in `tests/complete_finalize.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` block
//! in this file.

use clap::Parser;
use serde_json::{json, Map, Value};

use std::path::Path;

use crate::cleanup;
use crate::commands::log::append_log;
use crate::complete_post_merge;
use crate::flow_paths::FlowPaths;
use crate::git::{default_branch_in, project_root};

#[derive(Parser, Debug)]
#[command(
    name = "complete-finalize",
    about = "FLOW Complete phase post-merge + cleanup"
)]
pub struct Args {
    /// PR number
    #[arg(long, required = true)]
    pub pr: i64,
    /// Path to state file
    #[arg(long = "state-file", required = true)]
    pub state_file: String,
    /// Branch name
    #[arg(long, required = true)]
    pub branch: String,
    /// Worktree path (relative)
    #[arg(long, required = true)]
    pub worktree: String,
    /// Run git pull origin main after cleanup
    #[arg(long)]
    pub pull: bool,
}

/// Production entry: runs post-merge then cleanup, building the final
/// JSON result. Best-effort logging to `.flow-states/<branch>.log`
/// when the directory exists. The `is_valid_branch` upfront guard
/// rejects empty, slash-containing, path-traversal, and NUL-bearing
/// branch values with a structured `invalid_branch` error envelope
/// before any side effect runs.
///
/// Self-gates against cwd-inside-worktree before any side effect: when
/// the caller's canonicalized cwd equals or sits beneath the
/// canonicalized `--worktree` argument, the worktree removal would
/// strand the caller's shell in a deleted directory. The guard
/// returns a structured `cwd_inside_worktree` error so the failure
/// mode is a clean JSON envelope rather than shell corruption. See
/// `.claude/rules/rust-patterns.md` "Cwd-Inside-Destructive-Path
/// Guard" for the pattern this applies.
pub fn run_impl(args: &Args) -> Value {
    if let (Ok(cwd_canon), Ok(worktree_canon)) = (
        std::env::current_dir().and_then(|p| p.canonicalize()),
        Path::new(&args.worktree).canonicalize(),
    ) {
        if cwd_canon == worktree_canon || cwd_canon.starts_with(&worktree_canon) {
            let root = project_root();
            return json!({
                "status": "error",
                "reason": "cwd_inside_worktree",
                "message": format!(
                    "cd to {} before running complete-finalize",
                    root.display()
                ),
            });
        }
    }

    // Validate the --branch argument before any side effect runs.
    // `cleanup::cleanup` constructs `.flow-states/<branch>/` and
    // `.worktrees/<branch>/` paths from this value; an empty,
    // path-traversal, or NUL-bearing branch would escape the
    // per-branch scope per `.claude/rules/branch-path-safety.md`.
    if !FlowPaths::is_valid_branch(&args.branch) {
        return json!({
            "status": "error",
            "reason": "invalid_branch",
            "message": "--branch is not a valid FLOW branch name (empty, contains '/', '.', '..', or NUL)",
        });
    }

    let root = project_root();
    let branch = &args.branch;

    // Best-effort logging — the upstream `is_valid_branch` check
    // above guarantees `branch` is a valid FLOW branch name, so
    // `FlowPaths::try_new` always returns `Some` here. The
    // `.expect` documents that the boundary check makes the None
    // arm unreachable per `.claude/rules/external-input-validation.md`.
    // The directory guard checks the branch directory rather than
    // the parent `.flow-states/` directory so that the post-cleanup
    // invocation skips when cleanup has just removed the branch
    // directory: `append_log` calls `ensure_branch_dir()`, and a
    // parent-scoped guard would resurrect the directory cleanup
    // just removed.
    let paths = FlowPaths::try_new(&root, branch).expect(
        "is_valid_branch guard above proves branch is non-empty, no '/', no '.'/'..', no NUL",
    );
    let log = |msg: &str| {
        if paths.branch_dir().is_dir() {
            let _ = append_log(&root, branch, msg);
        }
    };

    log("[Phase 4] complete-finalize — starting");

    // Capture the flow-level final account-window snapshot BEFORE
    // post_merge mutates state and cleanup deletes the state file.
    // Lands at state.window_at_complete for consumers
    // (format_complete_summary's Token Cost section) to read when
    // rendering the post-merge summary.
    let home = crate::session_metrics::home_dir_or_empty();
    let state_path = std::path::Path::new(&args.state_file);
    if state_path.exists() {
        let _ = crate::lock::mutate_state(state_path, &mut |state| {
            let snap = crate::per_flow_capture::capture_for_active_state(&home, state, &root);
            crate::session_metrics::write_snapshot_into_state(state, "window_at_complete", &snap);
            // Mirror the snapshot under the phase-scoped key so
            // `format_complete_summary`'s `phase_delta` reads
            // `phases.flow-complete.window_at_complete` for the
            // Complete row. Hand-edited or corrupt state files may
            // carry non-object values at any level, so per-level
            // object guards heal the path before the IndexMut chain
            // per `.claude/rules/rust-patterns.md` "State Mutation
            // Object Guards" — unguarded IndexMut on a non-object
            // primitive (number, string, bool, array) panics.
            if state.is_object() {
                if !state["phases"].is_object() {
                    state["phases"] = serde_json::json!({});
                }
                if !state["phases"]["flow-complete"].is_object() {
                    state["phases"]["flow-complete"] = serde_json::json!({});
                }
                state["phases"]["flow-complete"]["window_at_complete"] =
                    serde_json::to_value(&snap).expect("WindowSnapshot must serialize");
            }
        });
    }

    // Post-merge (best-effort: failures land in its own `failures` map)
    let post_merge_data = complete_post_merge::post_merge(args.pr, &args.state_file, branch);
    let formatted_time = post_merge_data
        .get("formatted_time")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let cumulative_seconds = post_merge_data
        .get("cumulative_seconds")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let summary = post_merge_data
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let issues_links = post_merge_data
        .get("issues_links")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let banner_line = post_merge_data
        .get("banner_line")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Cleanup. Resolve the integration branch from git (single source
    // of truth) so the optional --pull step targets
    // origin/<base_branch>. Fail closed via JSON error envelope when
    // git cannot resolve it.
    let base_branch = match default_branch_in(&root) {
        Ok(b) => b,
        Err(msg) => {
            return json!({
                "status": "error",
                "step": "resolve_base_branch",
                "message": msg,
            });
        }
    };
    let cleanup_steps =
        cleanup::cleanup(&root, branch, &args.worktree, None, args.pull, &base_branch);
    let cleanup_map: Map<String, Value> = cleanup_steps
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();

    // Run sentinel-gated CI on the integration branch when the
    // post-merge pull completed cleanly. The merged tree on local
    // <base_branch> must actually pass CI before the next `start-gate`
    // trusts a sentinel for it — so rather than fabricating the
    // sentinel, run `ci::run_impl` against the base branch. It computes
    // the tree snapshot, skips when a prior sentinel already matches,
    // otherwise runs format/lint/build/test and writes the sentinel
    // ONLY on a real pass. A CI failure is surfaced in the result's
    // `base_ci` field WITHOUT rolling back the already-completed merge
    // or erroring the finalize — the merge landed upstream and cannot
    // be undone from here. The base branch is checked out at the
    // project root after cleanup, so `root` is both the cwd and the
    // sentinel root for the call.
    let mut base_ci: Option<Value> = None;
    if args.pull && cleanup_map.get("git_pull").and_then(|v| v.as_str()) == Some("pulled") {
        // `base_branch` comes from `git::default_branch_in(root)` —
        // unvalidated git subprocess output. A slash-containing
        // origin/HEAD target (e.g. `feature/main`) would reach
        // `sentinel_path` inside `ci::run_impl`, which calls
        // `FlowPaths::try_new(...).expect(...)` and panics on invalid
        // branches; per `.claude/rules/branch-path-safety.md`, callers
        // must gate on `is_valid_branch` before the panicking
        // constructor. The integration-branch CI is best-effort, so an
        // invalid base_branch skips it — the next start-gate run
        // re-establishes the sentinel.
        if FlowPaths::is_valid_branch(&base_branch) {
            let ci_args = crate::ci::Args {
                force: false,
                retry: 0,
                branch: Some(base_branch.clone()),
                simulate_branch: None,
                format: false,
                lint: false,
                build: false,
                test: false,
                audit: false,
                clean: false,
                trailing: Vec::new(),
                reason: Some("verifying integration branch after Complete merge".to_string()),
            };
            let (ci_result, ci_code) = crate::ci::run_impl(&ci_args, &root, &root, false);
            if ci_code != 0 {
                base_ci = Some(json!({ "status": "failed", "ci": ci_result }));
            }
        }
    }

    let mut result = json!({
        "status": "ok",
        "formatted_time": formatted_time,
        "cumulative_seconds": cumulative_seconds,
        "summary": summary,
        "issues_links": issues_links,
        "banner_line": banner_line,
        "cleanup": cleanup_map,
    });

    let failures_map = post_merge_data
        .get("failures")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    if !failures_map.is_empty() {
        result["post_merge_failures"] = Value::Object(failures_map);
    }

    // Surface an integration-branch CI failure as a warning field on
    // the otherwise-ok result. The merge already landed, so a failure
    // here is reported to the user (SKILL Step 5), not a rollback.
    if let Some(bc) = base_ci {
        result["base_ci"] = bc;
    }

    let has_failures = result
        .get("post_merge_failures")
        .and_then(|v| v.as_object())
        .map(|m| !m.is_empty())
        .unwrap_or(false);
    let effective_status = if has_failures {
        "ok with failures"
    } else {
        "ok"
    };
    log(&format!(
        "[Phase 4] complete-finalize — done (\"{}\")",
        effective_status
    ));

    result
}
