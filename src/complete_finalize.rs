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

    #[test]
    fn test_cleanup_panic_captured_and_reported() {
        let panicking_cleanup = || -> IndexMap<String, String> {
            panic!("simulated cleanup crash");
        };

        let result = finalize_inner(&mock_post_merge_ok, &panicking_cleanup);

        assert_eq!(result["status"], "ok");
        assert_eq!(result["cleanup_error"], "cleanup panicked");
        assert!(result["cleanup"].as_object().unwrap().is_empty());
        // Post-merge data still populated.
        assert_eq!(result["formatted_time"], "2m");
    }

    #[test]
    fn test_both_post_merge_and_cleanup_panic() {
        let panic_pm = || -> Value { panic!("pm boom") };
        let panic_cleanup = || -> IndexMap<String, String> { panic!("cleanup boom") };

        let result = finalize_inner(&panic_pm, &panic_cleanup);

        assert_eq!(result["status"], "ok");
        assert_eq!(result["post_merge_error"], "post-merge panicked");
        assert_eq!(result["cleanup_error"], "cleanup panicked");
    }

    #[test]
    fn test_empty_failures_object_not_included() {
        let pm_empty_failures = || -> Value {
            json!({
                "status": "ok",
                "formatted_time": "1m",
                "cumulative_seconds": 60,
                "summary": "done",
                "issues_links": "",
                "banner_line": "",
                "failures": {},
            })
        };

        let result = finalize_inner(&pm_empty_failures, &mock_cleanup_ok);

        // Empty failures object should NOT be added to result.
        assert!(result.get("post_merge_failures").is_none());
    }

    #[test]
    fn test_missing_post_merge_fields_default_to_empty() {
        let pm_minimal = || -> Value { json!({"status": "ok"}) };

        let result = finalize_inner(&pm_minimal, &mock_cleanup_ok);

        assert_eq!(result["formatted_time"], "");
        assert_eq!(result["cumulative_seconds"], 0);
        assert_eq!(result["summary"], "");
        assert_eq!(result["issues_links"], "");
        assert_eq!(result["banner_line"], "");
    }

    // --- run_impl_with_deps ---

    fn fake_args(branch: &str, state_file: &str, worktree: &str) -> Args {
        Args {
            pr: 42,
            state_file: state_file.to_string(),
            branch: branch.to_string(),
            worktree: worktree.to_string(),
            pull: false,
        }
    }

    /// `run_impl_with_deps` returns a `Value` (not `Result`) and its
    /// `status` field is `"ok"` for clean inputs. Guards the
    /// design-change deletion of the dead `Err` arm: `finalize_inner`
    /// never panics its closures when the mocks return ok, and
    /// `has_failures` evaluates false, so the orchestration produces
    /// the canonical happy-path Value.
    #[test]
    fn run_impl_returns_ok_status_value_for_clean_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join(".flow-states/test.json");
        let args = fake_args(
            "test-feature",
            state_path.to_string_lossy().as_ref(),
            ".worktrees/test-feature",
        );

        let result: Value =
            run_impl_with_deps(&args, dir.path(), &mock_post_merge_ok, &mock_cleanup_ok);

        assert_eq!(result["status"], "ok");
        assert_eq!(result["formatted_time"], "2m");
        assert_eq!(result["cumulative_seconds"], 120);
        assert_eq!(result["summary"], "Feature complete");
        assert!(result.get("post_merge_error").is_none());
        assert!(result.get("post_merge_failures").is_none());
    }

    /// When the injected post-merge closure panics,
    /// `run_impl_with_deps` returns a `Value` whose `status` stays
    /// `"ok"` (cleanup still runs) but `post_merge_error` is
    /// populated. This exercises the `has_failures == true` log-line
    /// path in `run_impl_with_deps` — the effective_status is
    /// "ok with failures" — and proves the signature change
    /// (`Result<Value, String>` → `Value`) preserves the capture of
    /// post-merge panic into a structured field rather than into an
    /// `Err` variant.
    #[test]
    fn run_impl_returns_post_merge_error_in_result_when_post_merge_panics() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join(".flow-states/test.json");
        let args = fake_args(
            "test-feature",
            state_path.to_string_lossy().as_ref(),
            ".worktrees/test-feature",
        );
        let panic_pm = || -> Value { panic!("simulated post-merge crash") };

        let result: Value = run_impl_with_deps(&args, dir.path(), &panic_pm, &mock_cleanup_ok);

        assert_eq!(result["status"], "ok");
        assert_eq!(result["post_merge_error"], "post-merge panicked");
        // Cleanup still ran — asserts the `has_failures` branch did
        // not short-circuit the closure invocation.
        assert_eq!(result["cleanup"]["worktree"], "removed");
    }
}
