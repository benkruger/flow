//! Cascade-close issues whose blockers are all closed, and close the
//! issue's milestone when its open-issue count reaches zero.
//!
//! Usage:
//!   bin/flow auto-close-parent --repo <owner/repo> --issue-number N
//!
//! On a just-closed issue X:
//!   - Walks the blocked-by dependency graph:
//!     `GET repos/{repo}/issues/{X}/dependencies/blocking` lists each
//!     issue Y that X blocks. For every Y, fetch
//!     `GET repos/{repo}/issues/{Y}/dependencies/blocked_by`; if every
//!     blocker of Y is `state == "closed"`, close Y and recurse with
//!     Y as the newly-closed seed. A visited set short-circuits cycles
//!     and a defensive depth bound caps the walk.
//!   - Also closes the issue's milestone when `open_issues == 0`.
//!
//! Best-effort throughout — any subprocess failure continues silently.
//! Tests live at `tests/auto_close_parent.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` in
//! this file.
//!
//! Output (JSON to stdout):
//!   `{"status": "ok", "closed_issues": [i64], "milestone_closed": bool}`

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use clap::Parser;
use serde_json::{json, Value};

use crate::complete_preflight::LOCAL_TIMEOUT;
use crate::utils::run_cmd;

#[derive(Parser, Debug)]
#[command(
    name = "auto-close-parent",
    about = "Cascade-close blocked-by issues and close milestone when empty"
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

/// Defensive recursion bound for `cascade_close_unblocked`. A
/// hand-edited or hostile dependency graph could form a chain longer
/// than any realistic engineering tree; this constant halts the walk
/// regardless. The visited set already terminates cycles in O(N)
/// steps — this bound is the secondary guard for non-cyclic but
/// excessively-deep DAGs.
pub const MAX_CASCADE_DEPTH: usize = 50;

/// Run a gh command, returning stdout on success or an error string on failure.
pub fn run_api(args: &[&str], cwd: &Path) -> Result<String, String> {
    match run_cmd(args, cwd, "api", Some(Duration::from_secs(LOCAL_TIMEOUT))) {
        Ok((stdout, _stderr)) => Ok(stdout),
        Err(e) => Err(e.message),
    }
}

/// Parse milestone.number from a JSON issue response.
///
/// Returns `None` when the JSON is malformed, the `milestone` field
/// is absent / null / not an object, or `number` is not an integer.
pub fn parse_milestone_number(json_str: &str) -> Option<i64> {
    let data: serde_json::Value = serde_json::from_str(json_str).ok()?;
    data.get("milestone")
        .and_then(|m| m.as_object())
        .and_then(|obj| obj.get("number"))
        .and_then(|n| n.as_i64())
}

/// Fetch milestone.number for an issue via one API call.
///
/// Tests pass a mock `runner` so they never spawn `gh`; production
/// callers pass `&run_api`.
pub fn fetch_milestone_number(
    repo: &str,
    issue_number: i64,
    cwd: &Path,
    runner: &GhApiRunner,
) -> Option<i64> {
    let url = format!("repos/{}/issues/{}", repo, issue_number);
    let stdout = runner(&["gh", "api", &url], cwd).ok()?;
    parse_milestone_number(&stdout)
}

/// Check if a milestone should be closed based on its JSON response.
///
/// Returns true if `open_issues == 0`. Missing or non-numeric
/// `open_issues` defaults to 1 (treated as open).
pub fn should_close_milestone(json_str: &str) -> bool {
    let data: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let open_issues = data
        .get("open_issues")
        .and_then(|v| v.as_i64())
        .unwrap_or(1);

    open_issues == 0
}

/// Check if all milestone issues are closed; close milestone if so.
///
/// If `milestone_number` is provided, uses it directly (skips the
/// lookup). Returns true if the milestone was closed, false
/// otherwise. Best-effort: any failure returns false. Tests pass a
/// mock runner; production passes `&run_api`.
pub fn check_milestone_closed(
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

/// Walk the blocked-by dependency graph from `start_issue` and close
/// every reachable issue whose remaining blockers are all closed.
///
/// Returns the list of newly-closed issue numbers in the order they
/// were closed. The starting issue itself is NOT in the result
/// because the caller closed it before invoking the cascade.
///
/// The walk uses a visited set so cycles terminate in O(N) steps.
/// A defensive depth bound (`MAX_CASCADE_DEPTH`) caps the recursion
/// in case the graph is hand-edited into an excessively deep chain.
/// Best-effort: every API or close failure is swallowed and the
/// cascade continues with the next candidate.
pub fn cascade_close_unblocked(
    repo: &str,
    start_issue: i64,
    cwd: &Path,
    runner: &GhApiRunner,
) -> Vec<i64> {
    let mut visited: HashSet<i64> = HashSet::new();
    let mut closed: Vec<i64> = Vec::new();
    visited.insert(start_issue);
    cascade_recurse(repo, start_issue, cwd, runner, &mut visited, &mut closed, 0);
    closed
}

/// Recursive helper for `cascade_close_unblocked`. Documented invariants:
///
/// - `depth` is the number of recursive frames between this call and
///   the original `cascade_close_unblocked` entry. A frame at depth
///   `MAX_CASCADE_DEPTH` halts before any side effect.
/// - `visited` is written ONLY after a candidate Y has been
///   successfully closed. Convergent (diamond) graphs where Y has
///   multiple paths from `start_issue` must remain eligible for
///   re-evaluation until one of Y's blockers has actually closed —
///   otherwise the first branch that finds Y blocked would mark Y
///   permanently visited and Y would stay open after every blocker
///   closes.
/// - `closed` is append-only and reflects close order; a failed
///   `gh issue close` call leaves Y absent from `visited`, so the
///   next branch that reaches Y will re-check its blocked-by list
///   against the updated graph state.
fn cascade_recurse(
    repo: &str,
    issue: i64,
    cwd: &Path,
    runner: &GhApiRunner,
    visited: &mut HashSet<i64>,
    closed: &mut Vec<i64>,
    depth: usize,
) {
    if depth >= MAX_CASCADE_DEPTH {
        return;
    }

    let blocking_url = format!("repos/{}/issues/{}/dependencies/blocking", repo, issue);
    let blocking_stdout = match runner(&["gh", "api", &blocking_url], cwd) {
        Ok(s) => s,
        Err(_) => return,
    };

    let blocking: Vec<serde_json::Value> = match serde_json::from_str(&blocking_stdout) {
        Ok(v) => v,
        Err(_) => return,
    };

    for y in blocking {
        let y_num = match y.get("number").and_then(|n| n.as_i64()) {
            Some(n) => n,
            None => continue,
        };
        if visited.contains(&y_num) {
            continue;
        }

        let blocked_by_url = format!("repos/{}/issues/{}/dependencies/blocked_by", repo, y_num);
        let blocked_by_stdout = match runner(&["gh", "api", &blocked_by_url], cwd) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let blockers: Vec<serde_json::Value> = match serde_json::from_str(&blocked_by_stdout) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let all_closed = !blockers.is_empty()
            && blockers
                .iter()
                .all(|b| b.get("state").and_then(|s| s.as_str()) == Some("closed"));

        if !all_closed {
            continue;
        }

        let close_result = runner(
            &["gh", "issue", "close", &y_num.to_string(), "--repo", repo],
            cwd,
        );
        if close_result.is_err() {
            continue;
        }

        closed.push(y_num);
        visited.insert(y_num);
        cascade_recurse(repo, y_num, cwd, runner, visited, closed, depth + 1);
    }
}

/// Main-arm dispatcher with injected cwd and runner. Always returns
/// `(Value, 0)` — auto-close is best-effort by design and the cascade
/// outcome surfaces as a list of closed issue numbers in the success
/// payload, never as an error exit. Tests pass a mock runner;
/// production passes `&run_api`.
pub fn run_impl_main(args: Args, cwd: &Path, runner: &GhApiRunner) -> (Value, i32) {
    let closed_issues = cascade_close_unblocked(&args.repo, args.issue_number, cwd, runner);
    let milestone_number = fetch_milestone_number(&args.repo, args.issue_number, cwd, runner);
    let milestone_closed =
        check_milestone_closed(&args.repo, args.issue_number, milestone_number, cwd, runner);

    (
        json!({
            "status": "ok",
            "closed_issues": closed_issues,
            "milestone_closed": milestone_closed,
        }),
        0,
    )
}

/// Best-effort safe-default payload when we can't determine cwd —
/// auto-close-parent never fails the caller, so we return ok with
/// an empty closed_issues list and milestone_closed false.
pub fn safe_default_ok() -> (Value, i32) {
    (
        json!({"status": "ok", "closed_issues": [], "milestone_closed": false}),
        0,
    )
}

/// Seam-injected wrapper that dispatches between `run_impl_main` and
/// `safe_default_ok` based on a caller-supplied cwd provider.
/// Production binds `cwd_fn = std::env::current_dir`; tests pass a
/// closure returning `Err` to exercise the safe-default branch
/// without needing to unlink the subprocess cwd via `pre_exec`.
pub fn run_with_current_dir_from<F>(args: Args, cwd_fn: F, runner: &GhApiRunner) -> (Value, i32)
where
    F: FnOnce() -> std::io::Result<std::path::PathBuf>,
{
    match cwd_fn() {
        Ok(cwd) => run_impl_main(args, &cwd, runner),
        Err(_) => safe_default_ok(),
    }
}
