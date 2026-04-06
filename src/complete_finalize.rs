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
use serde_json::{json, Value};

use crate::cleanup;
use crate::complete_post_merge;
use crate::git::project_root;

#[derive(Parser, Debug)]
#[command(name = "complete-finalize", about = "FLOW Complete phase post-merge + cleanup")]
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

/// Core complete-finalize logic. Runs post-merge then cleanup,
/// continuing cleanup even if post-merge fails.
///
/// Returns Ok(json) with merged results from both operations,
/// Err(string) only for catastrophic failures that prevent any output.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let root = project_root();

    // --- Post-merge (best-effort) ---
    let pm_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        complete_post_merge::post_merge(args.pr, &args.state_file, &args.branch)
    }));

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

    // --- Cleanup ---
    // cleanup() does not need pr_number (that's for abort only — pass None)
    let cleanup_steps = cleanup::cleanup(&root, &args.branch, &args.worktree, None, args.pull);

    // Convert IndexMap<String, String> to Value for JSON output
    let cleanup_json: serde_json::Map<String, Value> = cleanup_steps
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();

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
    if let Some(ref pm) = post_merge_data {
        if let Some(failures) = pm.get("failures") {
            if failures.is_object() && !failures.as_object().unwrap().is_empty() {
                result["post_merge_failures"] = failures.clone();
            }
        }
    }

    Ok(result)
}

/// CLI entry point. Always exits 0 (best-effort — matches post-merge behavior).
pub fn run(args: Args) {
    match run_impl(&args) {
        Ok(result) => {
            println!("{}", result);
        }
        Err(e) => {
            println!("{}", json!({"status": "error", "message": e}));
            std::process::exit(1);
        }
    }
}
