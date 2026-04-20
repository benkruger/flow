//! `bin/flow complete-finalize` — consolidated post-merge + cleanup.
//!
//! Combines complete-post-merge and cleanup into a single process,
//! eliminating the `cd <project_root>` step between them. Both
//! post_merge_inner() and cleanup() use explicit paths, so they
//! compose naturally without changing the shell working directory.
//!
//! Usage: bin/flow complete-finalize --pr <N> --state-file <path>
//!        --branch <name> --worktree <path> [--pull]
//!
//! Output (JSON to stdout):
//!   {"status": "ok", "formatted_time": "...", "summary": "...",
//!    "issues_links": "...", "banner_line": "...", "cleanup": {...}}

use clap::Parser;
use indexmap::IndexMap;
use serde_json::{json, Value};

use crate::cleanup;
use crate::commands::log::append_log;
use crate::complete_post_merge;
use crate::flow_paths::FlowPaths;
use crate::git::project_root;

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

/// Testable inner function with injectable post-merge and cleanup.
///
/// `post_merge_fn` returns the post-merge JSON result.
/// `cleanup_fn` returns the cleanup steps map.
/// Both are called in sequence; cleanup runs even if post-merge panics.
pub fn finalize_inner(
    post_merge_fn: &dyn Fn() -> Value,
    cleanup_fn: &dyn Fn() -> IndexMap<String, String>,
) -> Value {
    // --- Post-merge (best-effort) ---
    let pm_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(post_merge_fn));

    let (post_merge_data, post_merge_error) = match pm_result {
        Ok(data) => (Some(data), None),
        Err(_) => (None, Some("post-merge panicked".to_string())),
    };

    // Extract fields from post-merge result
    let formatted_time = post_merge_data
        .as_ref()
        .and_then(|d| d.get("formatted_time"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let cumulative_seconds = post_merge_data
        .as_ref()
        .and_then(|d| d.get("cumulative_seconds"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let summary = post_merge_data
        .as_ref()
        .and_then(|d| d.get("summary"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let issues_links = post_merge_data
        .as_ref()
        .and_then(|d| d.get("issues_links"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let banner_line = post_merge_data
        .as_ref()
        .and_then(|d| d.get("banner_line"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // --- Cleanup (best-effort — catch panics like post-merge) ---
    let cleanup_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(cleanup_fn));

    let (cleanup_json, cleanup_error) = match cleanup_result {
        Ok(steps) => {
            let map: serde_json::Map<String, Value> = steps
                .into_iter()
                .map(|(k, v)| (k, Value::String(v)))
                .collect();
            (map, None)
        }
        Err(_) => (serde_json::Map::new(), Some("cleanup panicked".to_string())),
    };

    // Build result
    let mut result = json!({
        "status": "ok",
        "formatted_time": formatted_time,
        "cumulative_seconds": cumulative_seconds,
        "summary": summary,
        "issues_links": issues_links,
        "banner_line": banner_line,
        "cleanup": cleanup_json,
    });

    if let Some(err) = post_merge_error {
        result["post_merge_error"] = json!(err);
    }
    if let Some(err) = cleanup_error {
        result["cleanup_error"] = json!(err);
    }
    if let Some(ref pm) = post_merge_data {
        if let Some(failures) = pm.get("failures") {
            if failures.is_object() && !failures.as_object().unwrap().is_empty() {
                result["post_merge_failures"] = failures.clone();
            }
        }
    }

    result
}

/// Testable core with injectable post-merge, cleanup, and an explicit
/// `root`. Unit tests exercise the orchestration (log-closure
/// branches, `has_failures` effective-status selection) without real
/// subprocess side effects by passing mock closures.
///
/// Returns the merged JSON result from both operations. Always
/// returns a `Value` (never errors) because `finalize_inner` catches
/// panics from both closures and reports them as fields on the result.
pub fn run_impl_with_deps(
    args: &Args,
    root: &std::path::Path,
    post_merge_fn: &dyn Fn() -> Value,
    cleanup_fn: &dyn Fn() -> IndexMap<String, String>,
) -> Value {
    // Best-effort logging — `try_new` tolerates slash-containing
    // branches per `.claude/rules/external-input-validation.md`
    // because `args.branch` comes from the `--branch` CLI arg.
    // The `.flow-states/` existence check avoids creating the
    // directory in test fixtures that deliberately omit it.
    let log = |msg: &str| {
        if let Some(paths) = FlowPaths::try_new(root, &args.branch) {
            if paths.flow_states_dir().is_dir() {
                let _ = append_log(root, &args.branch, msg);
            }
        }
    };

    log("[Phase 6] complete-finalize — starting");

    let result = finalize_inner(post_merge_fn, cleanup_fn);

    let has_failures = result.get("post_merge_error").is_some()
        || result
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
        "[Phase 6] complete-finalize — done (\"{}\")",
        effective_status
    ));

    result
}

/// Core complete-finalize logic. Wraps `run_impl_with_deps` with
/// production `project_root()`, `complete_post_merge::post_merge`,
/// and `cleanup::cleanup` closures.
pub fn run_impl(args: &Args) -> Value {
    let root = project_root();
    run_impl_with_deps(
        args,
        &root,
        &|| complete_post_merge::post_merge(args.pr, &args.state_file, &args.branch),
        &|| cleanup::cleanup(&root, &args.branch, &args.worktree, None, args.pull),
    )
}
