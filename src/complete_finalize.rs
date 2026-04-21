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
//! Tests live in `tests/complete_finalize.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` block
//! in this file.

use clap::Parser;
use serde_json::{json, Map, Value};

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

/// Production entry: runs post-merge then cleanup, building the final
/// JSON result. Best-effort logging to `.flow-states/<branch>.log`
/// when the directory exists. Slash-containing branches no-op the log
/// closure via `FlowPaths::try_new`.
pub fn run_impl(args: &Args) -> Value {
    let root = project_root();
    let branch = &args.branch;

    // Best-effort logging — `try_new` tolerates slash-containing
    // branches per `.claude/rules/external-input-validation.md`.
    let log = |msg: &str| {
        if let Some(paths) = FlowPaths::try_new(&root, branch) {
            if paths.flow_states_dir().is_dir() {
                let _ = append_log(&root, branch, msg);
            }
        }
    };

    log("[Phase 6] complete-finalize — starting");

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

    // Cleanup
    let cleanup_steps = cleanup::cleanup(&root, branch, &args.worktree, None, args.pull);
    let cleanup_map: Map<String, Value> = cleanup_steps
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();

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
        "[Phase 6] complete-finalize — done (\"{}\")",
        effective_status
    ));

    result
}
