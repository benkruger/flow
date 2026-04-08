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
use crate::complete_post_merge;
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

/// Core complete-finalize logic. Runs post-merge then cleanup,
/// continuing cleanup even if post-merge fails.
///
/// Returns Ok(json) with merged results from both operations,
/// Err(string) only for catastrophic failures that prevent any output.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let root = project_root();

    let result = finalize_inner(
        &|| complete_post_merge::post_merge(args.pr, &args.state_file, &args.branch),
        &|| cleanup::cleanup(&root, &args.branch, &args.worktree, None, args.pull),
    );

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

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    fn mock_post_merge_ok() -> Value {
        json!({
            "status": "ok",
            "formatted_time": "2m",
            "cumulative_seconds": 120,
            "summary": "Feature complete",
            "issues_links": "https://github.com/test/test/issues/42",
            "banner_line": "Issues filed: 1",
            "failures": {},
        })
    }

    fn mock_cleanup_ok() -> IndexMap<String, String> {
        let mut steps = IndexMap::new();
        steps.insert("worktree".to_string(), "removed".to_string());
        steps.insert("state_file".to_string(), "deleted".to_string());
        steps.insert("log_file".to_string(), "deleted".to_string());
        steps
    }

    #[test]
    fn test_happy_path() {
        let result = finalize_inner(&mock_post_merge_ok, &mock_cleanup_ok);

        assert_eq!(result["status"], "ok");
        assert_eq!(result["formatted_time"], "2m");
        assert_eq!(result["cumulative_seconds"], 120);
        assert_eq!(result["summary"], "Feature complete");
        assert_eq!(
            result["issues_links"],
            "https://github.com/test/test/issues/42"
        );
        assert_eq!(result["cleanup"]["worktree"], "removed");
        assert_eq!(result["cleanup"]["state_file"], "deleted");
        assert!(result.get("post_merge_error").is_none());
        assert!(result.get("post_merge_failures").is_none());
    }

    #[test]
    fn test_post_merge_failure_still_cleans_up() {
        let panicking_pm = || -> Value {
            panic!("simulated post-merge crash");
        };

        let result = finalize_inner(&panicking_pm, &mock_cleanup_ok);

        // Overall status is still ok — cleanup succeeded
        assert_eq!(result["status"], "ok");
        // Post-merge error captured
        assert_eq!(result["post_merge_error"], "post-merge panicked");
        // Cleanup still ran
        assert_eq!(result["cleanup"]["worktree"], "removed");
        assert_eq!(result["cleanup"]["state_file"], "deleted");
        // Post-merge fields default to empty
        assert_eq!(result["formatted_time"], "");
        assert_eq!(result["cumulative_seconds"], 0);
    }

    #[test]
    fn test_post_merge_with_failures_propagated() {
        let pm_with_failures = || -> Value {
            json!({
                "status": "ok",
                "formatted_time": "<1m",
                "cumulative_seconds": 30,
                "summary": "done",
                "issues_links": "",
                "banner_line": "",
                "failures": {
                    "render_pr_body": "gh API error",
                    "label_issues": "timeout",
                },
            })
        };

        let result = finalize_inner(&pm_with_failures, &mock_cleanup_ok);

        assert_eq!(result["status"], "ok");
        let failures = result["post_merge_failures"].as_object().unwrap();
        assert!(failures.contains_key("render_pr_body"));
        assert!(failures.contains_key("label_issues"));
    }

    #[test]
    fn test_cleanup_results_included() {
        let cleanup_with_pull = || -> IndexMap<String, String> {
            let mut steps = mock_cleanup_ok();
            steps.insert("git_pull".to_string(), "pulled".to_string());
            steps
        };

        let result = finalize_inner(&mock_post_merge_ok, &cleanup_with_pull);

        assert_eq!(result["cleanup"]["git_pull"], "pulled");
        assert_eq!(result["cleanup"]["worktree"], "removed");
    }
}
