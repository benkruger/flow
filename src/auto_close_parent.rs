//! Auto-close parent issue and milestone.
//!
//! Usage:
//!   bin/flow auto-close-parent --repo <owner/repo> --issue-number N
//!
//! Checks if the issue has a parent (sub-issue relationship). If so, checks
//! whether all sibling sub-issues are closed. If all closed, closes the parent.
//! Also checks the issue's milestone — if all milestone issues are closed,
//! closes the milestone.
//!
//! Best-effort throughout — any failure continues silently.
//!
//! Output (JSON to stdout):
//!   {"status": "ok", "parent_closed": bool, "milestone_closed": bool}

use std::path::Path;
use std::time::Duration;

use clap::Parser;
use serde_json::{json, Value};

use crate::complete_preflight::LOCAL_TIMEOUT;
use crate::utils::run_cmd;

#[derive(Parser, Debug)]
#[command(
    name = "auto-close-parent",
    about = "Auto-close parent issue and milestone"
)]
pub struct Args {
    /// Repository (owner/name)
    #[arg(long)]
    pub repo: String,

    /// Issue number to check
    #[arg(long = "issue-number")]
    pub issue_number: i64,
}

/// Type alias for the gh-api runner closure used by `_with_runner`
/// seams. Production binds to a closure wrapping `run_cmd`. Tests
/// inject mock closures returning queued or fixed
/// `Result<String, String>` responses per call so the test never
/// spawns a real `gh` subprocess.
pub type GhApiRunner = dyn Fn(&[&str], &Path) -> Result<String, String>;

/// Run a gh command, returning stdout on success or an error string on failure.
pub fn run_api(args: &[&str], cwd: &Path) -> Result<String, String> {
    match run_cmd(args, cwd, "api", Some(Duration::from_secs(LOCAL_TIMEOUT))) {
        Ok((stdout, _stderr)) => Ok(stdout),
        Err(e) => Err(e.message),
    }
}

/// Parse parent_issue.number and milestone.number from a JSON issue response.
///
/// Returns (parent_number_or_None, milestone_number_or_None).
pub fn parse_issue_fields(json_str: &str) -> (Option<i64>, Option<i64>) {
    let data: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return (None, None),
    };

    let parent_number = data
        .get("parent_issue")
        .and_then(|p| p.as_object())
        .and_then(|obj| obj.get("number"))
        .and_then(|n| n.as_i64());

    let milestone_number = data
        .get("milestone")
        .and_then(|m| m.as_object())
        .and_then(|obj| obj.get("number"))
        .and_then(|n| n.as_i64());

    (parent_number, milestone_number)
}

/// Fetch parent_issue.number and milestone.number in one API call.
///
/// Returns (parent_number_or_None, milestone_number_or_None).
/// Best-effort: returns (None, None) on any failure.
pub fn fetch_issue_fields(repo: &str, issue_number: i64, cwd: &Path) -> (Option<i64>, Option<i64>) {
    fetch_issue_fields_with_runner(repo, issue_number, cwd, &run_api)
}

/// Seam-injected variant of [`fetch_issue_fields`]. Tests pass a mock
/// runner returning canned responses or simulated failures so they
/// never spawn `gh`.
pub fn fetch_issue_fields_with_runner(
    repo: &str,
    issue_number: i64,
    cwd: &Path,
    runner: &GhApiRunner,
) -> (Option<i64>, Option<i64>) {
    let url = format!("repos/{}/issues/{}", repo, issue_number);
    let stdout = match runner(&["gh", "api", &url], cwd) {
        Ok(s) => s,
        Err(_) => return (None, None),
    };
    parse_issue_fields(&stdout)
}

/// Check if all sub-issues are closed from a JSON array response.
///
/// Returns true if the list is non-empty and every item has state "closed".
pub fn all_sub_issues_closed(json_str: &str) -> bool {
    let sub_issues: Vec<serde_json::Value> = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return false,
    };

    if sub_issues.is_empty() {
        return false;
    }

    sub_issues
        .iter()
        .all(|si| si.get("state").and_then(|s| s.as_str()) == Some("closed"))
}

/// Check if a milestone should be closed based on its JSON response.
///
/// Returns true if open_issues is 0.
pub fn should_close_milestone(json_str: &str) -> bool {
    let data: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return false,
    };

    // Default to 1 so a missing field is treated as open, never accidentally closing
    let open_issues = data
        .get("open_issues")
        .and_then(|v| v.as_i64())
        .unwrap_or(1);

    open_issues == 0
}

/// Check if all sub-issues of the parent are closed; close parent if so.
///
/// If parent_number is provided, uses it directly (skips the lookup).
/// Returns true if the parent was closed, false otherwise.
/// Best-effort: any failure returns false.
pub fn check_parent_closed(
    repo: &str,
    issue_number: i64,
    parent_number: Option<i64>,
    cwd: &Path,
) -> bool {
    check_parent_closed_with_runner(repo, issue_number, parent_number, cwd, &run_api)
}

/// Seam-injected variant of [`check_parent_closed`].
pub fn check_parent_closed_with_runner(
    repo: &str,
    issue_number: i64,
    parent_number: Option<i64>,
    cwd: &Path,
    runner: &GhApiRunner,
) -> bool {
    let parent = match parent_number {
        Some(n) => n,
        None => {
            // Standalone call — fetch the parent number
            let url = format!("repos/{}/issues/{}", repo, issue_number);
            let stdout = match runner(&["gh", "api", &url, "--jq", ".parent_issue.number"], cwd) {
                Ok(s) => s,
                Err(_) => return false,
            };
            let trimmed = stdout.trim();
            if trimmed.is_empty() || trimmed == "null" {
                return false;
            }
            match trimmed.parse::<i64>() {
                Ok(n) => n,
                Err(_) => return false,
            }
        }
    };

    // Get all sub-issues of the parent
    let url = format!("repos/{}/issues/{}/sub_issues", repo, parent);
    let stdout = match runner(&["gh", "api", &url], cwd) {
        Ok(s) => s,
        Err(_) => return false,
    };

    if !all_sub_issues_closed(&stdout) {
        return false;
    }

    // All closed — close the parent
    runner(
        &["gh", "issue", "close", &parent.to_string(), "--repo", repo],
        cwd,
    )
    .is_ok()
}

/// Check if all milestone issues are closed; close milestone if so.
///
/// If milestone_number is provided, uses it directly (skips the lookup).
/// Returns true if the milestone was closed, false otherwise.
/// Best-effort: any failure returns false.
pub fn check_milestone_closed(
    repo: &str,
    issue_number: i64,
    milestone_number: Option<i64>,
    cwd: &Path,
) -> bool {
    check_milestone_closed_with_runner(repo, issue_number, milestone_number, cwd, &run_api)
}

/// Seam-injected variant of [`check_milestone_closed`].
pub fn check_milestone_closed_with_runner(
    repo: &str,
    issue_number: i64,
    milestone_number: Option<i64>,
    cwd: &Path,
    runner: &GhApiRunner,
) -> bool {
    let milestone = match milestone_number {
        Some(n) => n,
        None => {
            // Standalone call — fetch the milestone number
            let url = format!("repos/{}/issues/{}", repo, issue_number);
            let stdout = match runner(&["gh", "api", &url, "--jq", ".milestone.number"], cwd) {
                Ok(s) => s,
                Err(_) => return false,
            };
            let trimmed = stdout.trim();
            if trimmed.is_empty() || trimmed == "null" {
                return false;
            }
            match trimmed.parse::<i64>() {
                Ok(n) => n,
                Err(_) => return false,
            }
        }
    };

    // Check milestone open_issues count
    let url = format!("repos/{}/milestones/{}", repo, milestone);
    let stdout = match runner(&["gh", "api", &url], cwd) {
        Ok(s) => s,
        Err(_) => return false,
    };

    if !should_close_milestone(&stdout) {
        return false;
    }

    // All closed — close the milestone
    runner(
        &[
            "gh",
            "api",
            &format!("repos/{}/milestones/{}", repo, milestone),
            "--method",
            "PATCH",
            "-f",
            "state=closed",
        ],
        cwd,
    )
    .is_ok()
}

/// Main-arm dispatcher with injected cwd. Always returns
/// `(Value, 0)` — auto-close is best-effort by design and the parent /
/// milestone close decisions surface as boolean fields in the success
/// payload, never as an error exit.
pub fn run_impl_main(args: Args, cwd: &Path) -> (Value, i32) {
    run_impl_main_with_runner(args, cwd, &run_api)
}

/// Seam-injected variant of [`run_impl_main`]. Tests pass a mock
/// runner so they exercise the dispatch shape without spawning real
/// `gh`. Per `.claude/rules/subprocess-test-hygiene.md`, this avoids
/// network calls in unit tests that would otherwise inherit
/// `GH_TOKEN` from the developer's environment.
pub fn run_impl_main_with_runner(args: Args, cwd: &Path, runner: &GhApiRunner) -> (Value, i32) {
    // Fetch both fields in one API call to avoid redundant requests
    let (parent_number, milestone_number) =
        fetch_issue_fields_with_runner(&args.repo, args.issue_number, cwd, runner);

    let parent_closed =
        check_parent_closed_with_runner(&args.repo, args.issue_number, parent_number, cwd, runner);
    let milestone_closed = check_milestone_closed_with_runner(
        &args.repo,
        args.issue_number,
        milestone_number,
        cwd,
        runner,
    );

    (
        json!({
            "status": "ok",
            "parent_closed": parent_closed,
            "milestone_closed": milestone_closed,
        }),
        0,
    )
}

/// Testable entry that accepts an injectable cwd Result so unit tests
/// can drive the cwd-failure branch (returns a best-effort ok payload)
/// without requiring a deleted-cwd test environment.
pub fn run_with_cwd_result(
    args: Args,
    cwd_result: std::io::Result<std::path::PathBuf>,
) -> (Value, i32) {
    let cwd = match cwd_result {
        Ok(c) => c,
        Err(_) => {
            return (
                json!({"status": "ok", "parent_closed": false, "milestone_closed": false}),
                0,
            );
        }
    };
    run_impl_main(args, &cwd)
}

/// CLI entry point for auto-close-parent.
pub fn run(args: Args) -> ! {
    // current_dir() can fail in deleted-cwd environments, certain
    // container runtimes, and chroot jails. The historical contract
    // is to short-circuit with a best-effort safe-default response
    // rather than spawn `gh` against an undefined cwd. This best-
    // effort behavior is documented as a hard contract by the module
    // doc comment ("Best-effort throughout — any failure continues
    // silently") and is required by Phase 6 Complete cleanup, which
    // calls `auto-close-parent` as a non-critical step.
    let (value, code) = run_with_cwd_result(args, std::env::current_dir());
    crate::dispatch::dispatch_json(value, code)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercises the cwd-Err branch of `run_with_cwd_result` — when
    /// `current_dir()` fails, return a best-effort `status: ok` payload
    /// with both close flags false, exit code 0.
    #[test]
    fn run_with_cwd_result_err_returns_safe_default_ok() {
        let args = Args {
            repo: "owner/repo".to_string(),
            issue_number: 42,
        };
        let cwd_err = Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "deleted cwd",
        ));
        let (value, code) = run_with_cwd_result(args, cwd_err);
        assert_eq!(code, 0);
        assert_eq!(value["status"], "ok");
        assert_eq!(value["parent_closed"], false);
        assert_eq!(value["milestone_closed"], false);
    }

    // --- parse_issue_fields() ---

    #[test]
    fn parse_issue_fields_both_present() {
        let json = r#"{"parent_issue": {"number": 10}, "milestone": {"number": 3}}"#;
        let (parent, milestone) = parse_issue_fields(json);
        assert_eq!(parent, Some(10));
        assert_eq!(milestone, Some(3));
    }

    #[test]
    fn parse_issue_fields_absent() {
        let json = "{}";
        let (parent, milestone) = parse_issue_fields(json);
        assert_eq!(parent, None);
        assert_eq!(milestone, None);
    }

    #[test]
    fn parse_issue_fields_invalid_json() {
        let (parent, milestone) = parse_issue_fields("not json");
        assert_eq!(parent, None);
        assert_eq!(milestone, None);
    }

    #[test]
    fn parse_issue_fields_parent_not_dict() {
        let json = r#"{"parent_issue": "not_a_dict", "milestone": {"number": 3}}"#;
        let (parent, milestone) = parse_issue_fields(json);
        assert_eq!(parent, None);
        assert_eq!(milestone, Some(3));
    }

    #[test]
    fn parse_issue_fields_milestone_number_not_int() {
        let json = r#"{"parent_issue": {"number": 10}, "milestone": {"number": "not_int"}}"#;
        let (parent, milestone) = parse_issue_fields(json);
        assert_eq!(parent, Some(10));
        assert_eq!(milestone, None);
    }

    #[test]
    fn parse_issue_fields_null_values() {
        let json = r#"{"parent_issue": null, "milestone": null}"#;
        let (parent, milestone) = parse_issue_fields(json);
        assert_eq!(parent, None);
        assert_eq!(milestone, None);
    }

    // --- all_sub_issues_closed() ---

    #[test]
    fn all_sub_issues_closed_all_closed() {
        let json = r#"[{"number": 5, "state": "closed"}, {"number": 6, "state": "closed"}]"#;
        assert!(all_sub_issues_closed(json));
    }

    #[test]
    fn all_sub_issues_closed_some_open() {
        let json = r#"[{"number": 5, "state": "closed"}, {"number": 6, "state": "open"}]"#;
        assert!(!all_sub_issues_closed(json));
    }

    #[test]
    fn all_sub_issues_closed_empty() {
        assert!(!all_sub_issues_closed("[]"));
    }

    #[test]
    fn all_sub_issues_closed_invalid_json() {
        assert!(!all_sub_issues_closed("not json"));
    }

    #[test]
    fn all_sub_issues_closed_missing_state_field() {
        let json = r#"[{"number": 5}]"#;
        assert!(!all_sub_issues_closed(json));
    }

    // --- should_close_milestone() ---

    #[test]
    fn should_close_milestone_zero_open() {
        let json = r#"{"open_issues": 0, "closed_issues": 5}"#;
        assert!(should_close_milestone(json));
    }

    #[test]
    fn should_close_milestone_has_open() {
        let json = r#"{"open_issues": 2, "closed_issues": 3}"#;
        assert!(!should_close_milestone(json));
    }

    #[test]
    fn should_close_milestone_missing_field() {
        // Missing open_issues defaults to 1 (not closing)
        let json = r#"{"closed_issues": 5}"#;
        assert!(!should_close_milestone(json));
    }

    #[test]
    fn should_close_milestone_invalid_json() {
        assert!(!should_close_milestone("not json"));
    }

    #[test]
    fn should_close_milestone_null_open_issues() {
        // null defaults to 1 via unwrap_or
        let json = r#"{"open_issues": null}"#;
        assert!(!should_close_milestone(json));
    }

    // --- run_impl_main / run_impl_main_with_runner ---

    #[test]
    fn auto_close_parent_run_impl_main_with_runner_all_runner_failures_returns_ok() {
        // Inject a runner that fails every call. Per the best-effort
        // contract, fetch_issue_fields returns (None, None), the
        // standalone fetches in check_parent_closed and
        // check_milestone_closed both fail, and the function returns
        // OK with both close booleans false. Test never spawns gh
        // (subprocess hygiene per .claude/rules/subprocess-test-hygiene.md).
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let args = Args {
            repo: "owner/repo".to_string(),
            issue_number: 999,
        };
        let runner: &GhApiRunner = &|_, _| Err("simulated".to_string());
        let (value, code) = run_impl_main_with_runner(args, &cwd, runner);
        assert_eq!(code, 0);
        assert_eq!(value["status"], "ok");
        assert_eq!(value["parent_closed"], false);
        assert_eq!(value["milestone_closed"], false);
    }

    /// Build a stub `gh` binary on PATH that always exits non-zero so
    /// production wrappers (run_api, fetch_issue_fields, check_*,
    /// run_impl_main) reach their best-effort failure paths without
    /// spawning real gh. Returns the test's stub directory.
    fn install_failing_gh_stub() -> tempfile::TempDir {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let stub_dir = tempfile::tempdir().unwrap();
        let stub = stub_dir.path().join("gh");
        let mut f = std::fs::File::create(&stub).unwrap();
        f.write_all(b"#!/bin/bash\nexit 1\n").unwrap();
        let mut perms = std::fs::metadata(&stub).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&stub, perms).unwrap();
        stub_dir
    }

    /// Run a closure with PATH temporarily set to the stub dir.
    /// SAFETY: tests must serialize this — env vars are process-global.
    /// Wrapped in a mutex to prevent parallel test races on PATH.
    fn with_stub_path<F: FnOnce()>(stub_dir: &Path, f: F) {
        use std::sync::Mutex;
        static PATH_LOCK: Mutex<()> = Mutex::new(());
        let _guard = PATH_LOCK.lock().unwrap();
        let original = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", stub_dir.display(), original);
        // SAFETY: serialized via PATH_LOCK; only inside this test helper.
        unsafe {
            std::env::set_var("PATH", new_path);
        }
        f();
        unsafe {
            std::env::set_var("PATH", original);
        }
    }

    #[test]
    fn check_parent_closed_with_runner_standalone_null_returns_false() {
        // parent_number=None → standalone fetch path. Runner returns the
        // literal "null" string → trimmed=="null" branch fires → false.
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let runner: &GhApiRunner = &|_, _| Ok("null\n".to_string());
        assert!(!check_parent_closed_with_runner(
            "owner/repo",
            5,
            None,
            &cwd,
            runner
        ));
    }

    #[test]
    fn check_parent_closed_with_runner_standalone_empty_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let runner: &GhApiRunner = &|_, _| Ok(String::new());
        assert!(!check_parent_closed_with_runner(
            "owner/repo",
            5,
            None,
            &cwd,
            runner
        ));
    }

    #[test]
    fn check_parent_closed_with_runner_standalone_unparseable_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let runner: &GhApiRunner = &|_, _| Ok("not_an_int".to_string());
        assert!(!check_parent_closed_with_runner(
            "owner/repo",
            5,
            None,
            &cwd,
            runner
        ));
    }

    #[test]
    fn check_parent_closed_with_runner_standalone_succeeds_then_closes() {
        // Runner returns parent number "10" then sub_issues all closed
        // then close gh succeeds → returns true.
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let queue: std::cell::RefCell<std::collections::VecDeque<String>> =
            std::cell::RefCell::new(std::collections::VecDeque::from(vec![
                "10\n".to_string(),
                r#"[{"number":5,"state":"closed"}]"#.to_string(),
                String::new(),
            ]));
        let runner: &GhApiRunner =
            &move |_, _| Ok(queue.borrow_mut().pop_front().unwrap_or_default());
        assert!(check_parent_closed_with_runner(
            "owner/repo",
            5,
            None,
            &cwd,
            runner
        ));
    }

    #[test]
    fn check_milestone_closed_with_runner_standalone_null_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let runner: &GhApiRunner = &|_, _| Ok("null\n".to_string());
        assert!(!check_milestone_closed_with_runner(
            "owner/repo",
            5,
            None,
            &cwd,
            runner
        ));
    }

    #[test]
    fn check_milestone_closed_with_runner_standalone_empty_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let runner: &GhApiRunner = &|_, _| Ok(String::new());
        assert!(!check_milestone_closed_with_runner(
            "owner/repo",
            5,
            None,
            &cwd,
            runner
        ));
    }

    #[test]
    fn check_milestone_closed_with_runner_standalone_unparseable_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let runner: &GhApiRunner = &|_, _| Ok("not_an_int".to_string());
        assert!(!check_milestone_closed_with_runner(
            "owner/repo",
            5,
            None,
            &cwd,
            runner
        ));
    }

    #[test]
    fn check_milestone_closed_with_runner_standalone_succeeds_then_closes() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let queue: std::cell::RefCell<std::collections::VecDeque<String>> =
            std::cell::RefCell::new(std::collections::VecDeque::from(vec![
                "3\n".to_string(),
                r#"{"open_issues":0}"#.to_string(),
                String::new(),
            ]));
        let runner: &GhApiRunner =
            &move |_, _| Ok(queue.borrow_mut().pop_front().unwrap_or_default());
        assert!(check_milestone_closed_with_runner(
            "owner/repo",
            5,
            None,
            &cwd,
            runner
        ));
    }

    #[test]
    fn check_parent_closed_with_runner_close_command_fails_returns_false() {
        // Sub-issues all closed but the close command itself fails → false.
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let queue: std::cell::RefCell<std::collections::VecDeque<Result<String, String>>> =
            std::cell::RefCell::new(std::collections::VecDeque::from(vec![
                Ok(r#"[{"number":5,"state":"closed"}]"#.to_string()),
                Err("close failed".to_string()),
            ]));
        let runner: &GhApiRunner = &move |_, _| {
            queue
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| Err("queue empty".to_string()))
        };
        assert!(!check_parent_closed_with_runner(
            "owner/repo",
            5,
            Some(10),
            &cwd,
            runner
        ));
    }

    #[test]
    fn check_milestone_closed_with_runner_patch_command_fails_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let queue: std::cell::RefCell<std::collections::VecDeque<Result<String, String>>> =
            std::cell::RefCell::new(std::collections::VecDeque::from(vec![
                Ok(r#"{"open_issues":0}"#.to_string()),
                Err("patch failed".to_string()),
            ]));
        let runner: &GhApiRunner = &move |_, _| {
            queue
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| Err("queue empty".to_string()))
        };
        assert!(!check_milestone_closed_with_runner(
            "owner/repo",
            5,
            Some(3),
            &cwd,
            runner
        ));
    }

    #[test]
    fn check_parent_closed_with_runner_sub_issues_open_returns_false() {
        // Sub-issues fetch returns a list with an open issue → false.
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let runner: &GhApiRunner = &|_, _| {
            Ok(r#"[{"number":5,"state":"closed"},{"number":6,"state":"open"}]"#.to_string())
        };
        assert!(!check_parent_closed_with_runner(
            "owner/repo",
            5,
            Some(10),
            &cwd,
            runner
        ));
    }

    /// Exercises production line 186 — sub-issues fetch fails when the
    /// parent number is provided directly. The parent-fetch branch is
    /// skipped, so the runner's first call is the sub_issues lookup.
    #[test]
    fn check_parent_closed_with_runner_sub_issues_fetch_error_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let runner: &GhApiRunner = &|_, _| Err("network error".to_string());
        assert!(!check_parent_closed_with_runner(
            "owner/repo",
            5,
            Some(10),
            &cwd,
            runner
        ));
    }

    /// Exercises production line 247 — milestone fetch fails when the
    /// milestone number is provided directly. Mirrors the parent variant.
    #[test]
    fn check_milestone_closed_with_runner_milestone_fetch_error_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let runner: &GhApiRunner = &|_, _| Err("network error".to_string());
        assert!(!check_milestone_closed_with_runner(
            "owner/repo",
            5,
            Some(3),
            &cwd,
            runner
        ));
    }

    #[test]
    fn check_milestone_closed_with_runner_open_issues_nonzero_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let runner: &GhApiRunner = &|_, _| Ok(r#"{"open_issues":2}"#.to_string());
        assert!(!check_milestone_closed_with_runner(
            "owner/repo",
            5,
            Some(3),
            &cwd,
            runner
        ));
    }

    #[test]
    fn run_api_with_failing_gh_returns_err() {
        let stub_dir = install_failing_gh_stub();
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().canonicalize().unwrap();
        with_stub_path(stub_dir.path(), || {
            let result = run_api(&["gh", "api", "repos/x/y/issues/1"], &cwd);
            assert!(result.is_err(), "gh stub exits 1 → run_api Err");
        });
    }

    #[test]
    fn fetch_issue_fields_production_with_failing_gh_returns_none_none() {
        let stub_dir = install_failing_gh_stub();
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().canonicalize().unwrap();
        with_stub_path(stub_dir.path(), || {
            let (parent, milestone) = fetch_issue_fields("owner/repo", 5, &cwd);
            assert_eq!(parent, None);
            assert_eq!(milestone, None);
        });
    }

    #[test]
    fn check_parent_closed_production_with_failing_gh_returns_false() {
        let stub_dir = install_failing_gh_stub();
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().canonicalize().unwrap();
        with_stub_path(stub_dir.path(), || {
            // parent_number=None → standalone fetch path; gh fails → false
            assert!(!check_parent_closed("owner/repo", 5, None, &cwd));
        });
    }

    #[test]
    fn check_milestone_closed_production_with_failing_gh_returns_false() {
        let stub_dir = install_failing_gh_stub();
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().canonicalize().unwrap();
        with_stub_path(stub_dir.path(), || {
            // milestone_number=None → standalone fetch path; gh fails → false
            assert!(!check_milestone_closed("owner/repo", 5, None, &cwd));
        });
    }

    #[test]
    fn run_impl_main_production_with_failing_gh_returns_ok_both_false() {
        let stub_dir = install_failing_gh_stub();
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().canonicalize().unwrap();
        with_stub_path(stub_dir.path(), || {
            let args = Args {
                repo: "owner/repo".to_string(),
                issue_number: 5,
            };
            let (value, code) = run_impl_main(args, &cwd);
            assert_eq!(code, 0);
            assert_eq!(value["status"], "ok");
            assert_eq!(value["parent_closed"], false);
            assert_eq!(value["milestone_closed"], false);
        });
    }

    #[test]
    fn auto_close_parent_run_impl_main_with_runner_happy_path_closes_both() {
        // Inject responses simulating: fetch_issue_fields returns
        // parent_number=10, milestone_number=3; sub_issues all closed;
        // milestone open_issues=0; close calls succeed.
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let args = Args {
            repo: "owner/repo".to_string(),
            issue_number: 5,
        };
        let queue: std::cell::RefCell<std::collections::VecDeque<String>> =
            std::cell::RefCell::new(std::collections::VecDeque::from(vec![
                // fetch_issue_fields → repos/owner/repo/issues/5
                r#"{"parent_issue":{"number":10},"milestone":{"number":3}}"#.to_string(),
                // check_parent_closed → repos/owner/repo/issues/10/sub_issues
                r#"[{"number":5,"state":"closed"},{"number":6,"state":"closed"}]"#.to_string(),
                // close parent → gh issue close 10 --repo owner/repo
                String::new(),
                // check_milestone_closed → repos/owner/repo/milestones/3
                r#"{"open_issues":0}"#.to_string(),
                // close milestone → PATCH state=closed
                String::new(),
            ]));
        let runner: &GhApiRunner =
            &move |_, _| Ok(queue.borrow_mut().pop_front().unwrap_or_default());
        let (value, code) = run_impl_main_with_runner(args, &cwd, runner);
        assert_eq!(code, 0);
        assert_eq!(value["status"], "ok");
        assert_eq!(value["parent_closed"], true);
        assert_eq!(value["milestone_closed"], true);
    }
}
