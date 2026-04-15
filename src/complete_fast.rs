//! `bin/flow complete-fast` — consolidated Complete phase happy path.
//!
//! Absorbs SOFT-GATE + preflight + CI dirty check + GitHub CI check + merge
//! into a single process. Returns a JSON `path` indicator so the skill can
//! branch on the result instead of making 10 separate tool calls.
//!
//! Usage: bin/flow complete-fast [--branch <name>] [--auto] [--manual]
//!
//! Output (JSON to stdout):
//!   Merged:       {"status": "ok", "path": "merged", ...}
//!   Already:      {"status": "ok", "path": "already_merged", ...}
//!   Confirm:      {"status": "ok", "path": "confirm", ...}
//!   CI stale:     {"status": "ok", "path": "ci_stale", ...}
//!   CI failed:    {"status": "ok", "path": "ci_failed", ...}
//!   CI pending:   {"status": "ok", "path": "ci_pending", ...}
//!   Conflict:     {"status": "ok", "path": "conflict", ...}
//!   Max retries:  {"status": "ok", "path": "max_retries", ...}
//!   Error:        {"status": "error", "message": "..."}

use std::path::{Path, PathBuf};

use clap::Parser;
use serde_json::{json, Value};

use crate::ci;
use crate::complete_preflight::{
    check_learn_phase, check_pr_status, merge_main, resolve_mode, run_cmd_with_timeout, CmdResult,
    COMPLETE_STEPS_TOTAL, NETWORK_TIMEOUT,
};
use crate::flow_paths::FlowPaths;
use crate::git::{project_root, resolve_branch};
use crate::lock::mutate_state;
use crate::phase_transition::phase_enter;
use crate::utils::{bin_flow_path, derive_worktree};

#[derive(Parser, Debug)]
#[command(name = "complete-fast", about = "FLOW Complete phase fast path")]
pub struct Args {
    /// Override branch for state file lookup
    #[arg(long)]
    pub branch: Option<String>,
    /// Force auto mode
    #[arg(long)]
    pub auto: bool,
    /// Force manual mode
    #[arg(long)]
    pub manual: bool,
}

/// Read and parse a state file, returning (state_value, state_path).
fn read_state(root: &Path, branch: &str) -> Result<(Value, PathBuf), String> {
    let state_path = FlowPaths::new(root, branch).state_file();
    if !state_path.exists() {
        return Err(format!(
            "No state file found for branch '{}'. Run /flow:flow-start first.",
            branch
        ));
    }
    let content = std::fs::read_to_string(&state_path)
        .map_err(|e| format!("Could not read state file: {}", e))?;
    let state: Value = serde_json::from_str(&content)
        .map_err(|_| format!("Could not parse state file: {}", state_path.display()))?;
    if !state.is_object() {
        return Err(format!(
            "Corrupt state file (expected JSON object): {}",
            state_path.display()
        ));
    }
    Ok((state, state_path))
}

/// Parse `gh pr checks` tab-separated output into a status string.
/// Returns "pass", "pending", "fail", or "none".
fn parse_gh_checks_output(stdout: &str) -> String {
    let mut has_any = false;
    let mut has_pending = false;
    let mut has_fail = false;

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 2 {
            has_any = true;
            match parts[1] {
                "fail" => has_fail = true,
                "pending" => has_pending = true,
                _ => {} // pass, skipping
            }
        }
    }

    if !has_any {
        "none".to_string()
    } else if has_fail {
        "fail".to_string()
    } else if has_pending {
        "pending".to_string()
    } else {
        "pass".to_string()
    }
}

/// Core complete-fast logic with injectable runner for testability.
///
/// All subprocess calls (gh, git, check-freshness) go through `runner`.
/// CI dirty check uses `ci_skipped` and `ci_failed_output` parameters so tests
/// can control CI behavior without real git repos.
///
/// Returns Ok(json) for all path outcomes (including unhappy paths the
/// skill handles interactively), Err(string) only for infrastructure
/// failures that prevent any path determination.
#[allow(clippy::too_many_arguments)]
pub fn fast_inner(
    branch: &str,
    state: &Value,
    state_path: &Path,
    auto: bool,
    manual: bool,
    bin_flow: &str,
    tree_changed: bool,
    ci_skipped: bool,
    ci_failed_output: Option<&str>,
    gh_ci_status: &str,
    runner: &dyn Fn(&[&str], u64) -> CmdResult,
) -> Value {
    // Resolve mode
    let mode = resolve_mode(auto, manual, Some(state));

    // Collect warnings
    let warnings = check_learn_phase(state);

    // Extract PR info from state
    let pr_number = state.get("pr_number").and_then(|v| v.as_i64());
    let pr_url = state
        .get("pr_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let worktree = derive_worktree(branch);

    // --- CI dirty check (no simulate-branch) ---
    if tree_changed {
        return json!({
            "status": "ok",
            "path": "ci_stale",
            "reason": "main merged into branch — tree changed, CI must re-run",
            "mode": mode,
            "pr_number": pr_number,
            "pr_url": pr_url,
            "branch": branch,
            "worktree": worktree,
            "warnings": warnings,
        });
    }

    if let Some(output) = ci_failed_output {
        return json!({
            "status": "ok",
            "path": "ci_failed",
            "output": output,
            "mode": mode,
            "pr_number": pr_number,
            "pr_url": pr_url,
            "branch": branch,
            "worktree": worktree,
            "warnings": warnings,
        });
    }

    // --- GitHub CI check ---
    match gh_ci_status {
        "pass" | "none" => {} // Continue
        "pending" => {
            return json!({
                "status": "ok",
                "path": "ci_pending",
                "mode": mode,
                "pr_number": pr_number,
                "pr_url": pr_url,
                "branch": branch,
                "worktree": worktree,
                "warnings": warnings,
            });
        }
        "fail" => {
            return json!({
                "status": "ok",
                "path": "ci_failed",
                "output": "GitHub CI checks failed",
                "source": "github",
                "mode": mode,
                "pr_number": pr_number,
                "pr_url": pr_url,
                "branch": branch,
                "worktree": worktree,
                "warnings": warnings,
            });
        }
        _ => {} // Unknown — continue optimistically
    }

    // --- Mode branch: manual returns "confirm", auto proceeds to merge ---
    if mode == "manual" {
        return json!({
            "status": "ok",
            "path": "confirm",
            "mode": mode,
            "pr_number": pr_number,
            "pr_url": pr_url,
            "branch": branch,
            "worktree": worktree,
            "warnings": warnings,
            "ci_skipped": ci_skipped,
        });
    }

    // --- Freshness check + squash merge (auto mode) ---
    let state_file_str = state_path.to_string_lossy().to_string();
    let freshness_result = runner(
        &[bin_flow, "check-freshness", "--state-file", &state_file_str],
        NETWORK_TIMEOUT,
    );

    let (_code, stdout, _stderr) = match freshness_result {
        Err(e) => {
            return json!({
                "status": "error",
                "message": format!("check-freshness failed: {}", e),
                "branch": branch,
            });
        }
        Ok(triple) => triple,
    };

    let freshness: Value = match serde_json::from_str(stdout.trim()) {
        Ok(v) => v,
        Err(_) => {
            return json!({
                "status": "error",
                "message": format!("Invalid JSON from check-freshness: {}", stdout),
                "branch": branch,
            });
        }
    };

    let freshness_status = freshness
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match freshness_status {
        "max_retries" => {
            json!({
                "status": "ok",
                "path": "max_retries",
                "mode": mode,
                "pr_number": pr_number,
                "pr_url": pr_url,
                "branch": branch,
                "worktree": worktree,
                "warnings": warnings,
            })
        }
        "error" => {
            let msg = freshness
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("check-freshness failed");
            json!({
                "status": "error",
                "message": msg,
                "branch": branch,
            })
        }
        "conflict" => {
            let files = freshness.get("files").cloned().unwrap_or(json!([]));
            json!({
                "status": "ok",
                "path": "conflict",
                "conflict_files": files,
                "mode": mode,
                "pr_number": pr_number,
                "pr_url": pr_url,
                "branch": branch,
                "worktree": worktree,
                "warnings": warnings,
            })
        }
        "merged" => {
            // Main moved again — push and return ci_stale
            match runner(&["git", "push"], NETWORK_TIMEOUT) {
                Err(e) => {
                    json!({
                        "status": "error",
                        "message": format!("Push failed after freshness merge: {}", e),
                        "branch": branch,
                    })
                }
                Ok((code, _, stderr)) => {
                    if code != 0 {
                        json!({
                            "status": "error",
                            "message": format!("Push failed after freshness merge: {}", stderr.trim()),
                            "branch": branch,
                        })
                    } else {
                        json!({
                            "status": "ok",
                            "path": "ci_stale",
                            "reason": "main moved during freshness check — pushed, CI must re-run",
                            "mode": mode,
                            "pr_number": pr_number,
                            "pr_url": pr_url,
                            "branch": branch,
                            "worktree": worktree,
                            "warnings": warnings,
                        })
                    }
                }
            }
        }
        "up_to_date" => {
            // Proceed to squash merge
            let pr_str = pr_number.unwrap_or(0).to_string();
            match runner(&["gh", "pr", "merge", &pr_str, "--squash"], NETWORK_TIMEOUT) {
                Err(e) => {
                    json!({
                        "status": "error",
                        "message": e,
                        "branch": branch,
                    })
                }
                Ok((code, _, stderr)) => {
                    if code == 0 {
                        // Update step counter
                        let _ = mutate_state(state_path, |s| {
                            if !(s.is_object() || s.is_null()) {
                                return;
                            }
                            s["complete_step"] = json!(6);
                        });

                        json!({
                            "status": "ok",
                            "path": "merged",
                            "mode": mode,
                            "pr_number": pr_number,
                            "pr_url": pr_url,
                            "branch": branch,
                            "worktree": worktree,
                            "warnings": warnings,
                            "ci_skipped": ci_skipped,
                        })
                    } else {
                        let stderr_trim = stderr.trim();
                        if stderr_trim.contains("base branch policy") {
                            json!({
                                "status": "ok",
                                "path": "ci_pending",
                                "mode": mode,
                                "pr_number": pr_number,
                                "pr_url": pr_url,
                                "branch": branch,
                                "worktree": worktree,
                                "warnings": warnings,
                            })
                        } else {
                            json!({
                                "status": "error",
                                "message": stderr_trim,
                                "branch": branch,
                            })
                        }
                    }
                }
            }
        }
        other => {
            json!({
                "status": "error",
                "message": format!("Unexpected check-freshness status: {}", other),
                "branch": branch,
            })
        }
    }
}

/// Signature of the Complete-phase CI dirty-check seam.
///
/// Inputs: `(root, cwd, branch, tree_changed)`.
/// Output: `(ci_skipped, ci_failed_output)` — `ci_skipped` is true when
/// a prior CI run on the same tree-snapshot passed; `ci_failed_output`
/// carries a failure message when CI ran and failed.
pub type CiDecider = dyn Fn(&Path, &Path, &str, bool) -> (bool, Option<String>);

/// Production CI-decider for the Complete phase dirty-check block.
///
/// Returns `(ci_skipped, ci_failed_output)`:
/// - `ci_skipped` is `true` when the sentinel file's stored tree
///   snapshot matches the current cwd's snapshot, meaning a prior
///   `bin/flow ci` run on this same tree already passed.
/// - `ci_failed_output` is `Some(msg)` when CI runs and fails; `None`
///   when CI is skipped or runs and passes.
///
/// A `tree_changed` input (main was merged into the branch, dirtying
/// the tree) forces a fresh CI run by returning `(false, None)` — the
/// caller's fast_inner dispatch then surfaces this as a `ci_stale`
/// path.
fn production_ci_decider(
    root: &Path,
    cwd: &Path,
    branch: &str,
    tree_changed: bool,
) -> (bool, Option<String>) {
    if tree_changed {
        return (false, None);
    }

    let snapshot = ci::tree_snapshot(cwd, None);
    let sentinel = ci::sentinel_path(root, branch);

    let ci_skipped = if sentinel.exists() {
        std::fs::read_to_string(&sentinel)
            .map(|c| c == snapshot)
            .unwrap_or(false)
    } else {
        false
    };

    if ci_skipped {
        return (true, None);
    }

    let ci_args = ci::Args {
        force: false,
        retry: 0,
        branch: Some(branch.to_string()),
        simulate_branch: None,
    };
    let (ci_result, ci_code) = ci::run_impl(&ci_args, cwd, root, false);
    if ci_code != 0 {
        let msg = ci_result
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("CI failed")
            .to_string();
        (false, Some(msg))
    } else {
        (false, None)
    }
}

/// Core complete-fast logic with injectable `root`, `runner`, and
/// `ci_decider` seams for testability.
///
/// All subprocess calls (gh, git, check-freshness) go through `runner`.
/// The Complete-phase CI dirty-check block goes through `ci_decider`,
/// which in production wraps `ci::run_impl` and returns
/// `(ci_skipped, ci_failed_output)` for the given `(root, cwd, branch,
/// tree_changed)` inputs.
///
/// Returns Ok(json) on success paths (including unhappy paths like
/// `ci_failed` that the skill handles interactively), Err(string) only
/// for infrastructure failures that prevent any path determination.
pub fn run_impl_inner(
    args: &Args,
    root: &Path,
    runner: &dyn Fn(&[&str], u64) -> CmdResult,
    ci_decider: &CiDecider,
) -> Result<Value, String> {
    let branch =
        resolve_branch(args.branch.as_deref(), root).ok_or("Could not determine current branch")?;

    // Read state file
    let (state, state_path) = read_state(root, &branch)?;

    // Gate: Learn phase must be complete
    let learn_status = state
        .get("phases")
        .and_then(|p| p.get("flow-learn"))
        .and_then(|l| l.get("status"))
        .and_then(|s| s.as_str())
        .unwrap_or("pending");
    if learn_status != "complete" {
        return Ok(json!({
            "status": "error",
            "message": format!("Phase 5: Learn must be complete before Complete. Current status: {}", learn_status)
        }));
    }

    // Phase enter + set step counters
    mutate_state(&state_path, |s| {
        if !(s.is_object() || s.is_null()) {
            return;
        }
        phase_enter(s, "flow-complete", None);
        s["complete_steps_total"] = json!(COMPLETE_STEPS_TOTAL);
        s["complete_step"] = json!(1);
    })
    .map_err(|e| format!("Failed to update state: {}", e))?;

    // --- PR check ---
    let pr_state = match check_pr_status(
        state.get("pr_number").and_then(|v| v.as_i64()),
        &branch,
        runner,
    ) {
        Ok(s) => s,
        Err(e) => {
            return Ok(json!({
                "status": "error",
                "message": e,
                "branch": branch,
            }));
        }
    };

    if pr_state == "MERGED" {
        let mode = resolve_mode(args.auto, args.manual, Some(&state));
        return Ok(json!({
            "status": "ok",
            "path": "already_merged",
            "mode": mode,
            "pr_number": state.get("pr_number").and_then(|v| v.as_i64()),
            "pr_url": state.get("pr_url").and_then(|v| v.as_str()).unwrap_or(""),
            "branch": branch,
            "worktree": derive_worktree(&branch),
            "warnings": check_learn_phase(&state),
        }));
    }

    if pr_state == "CLOSED" {
        return Ok(json!({
            "status": "error",
            "message": "PR is closed but not merged. Reopen or create a new PR first.",
            "branch": branch,
        }));
    }

    // --- Merge main into branch ---
    let (merge_status, merge_data) = merge_main(runner);
    let tree_changed = merge_status == "merged";

    if merge_status == "conflict" {
        let mode = resolve_mode(args.auto, args.manual, Some(&state));
        return Ok(json!({
            "status": "ok",
            "path": "conflict",
            "conflict_files": merge_data.unwrap_or(json!([])),
            "mode": mode,
            "pr_number": state.get("pr_number").and_then(|v| v.as_i64()),
            "pr_url": state.get("pr_url").and_then(|v| v.as_str()).unwrap_or(""),
            "branch": branch,
            "worktree": derive_worktree(&branch),
            "warnings": check_learn_phase(&state),
        }));
    }

    if merge_status == "error" {
        return Ok(json!({
            "status": "error",
            "message": merge_data.unwrap_or(json!("")),
            "branch": branch,
        }));
    }

    // --- CI dirty check (no simulate-branch) ---
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (ci_skipped, ci_failed_output) = ci_decider(root, &cwd, &branch, tree_changed);

    // --- GitHub CI check ---
    let pr_number = state.get("pr_number").and_then(|v| v.as_i64());
    let gh_ci_status = if let Some(pr_num) = pr_number {
        let pr_str = pr_num.to_string();
        match runner(&["gh", "pr", "checks", &pr_str], NETWORK_TIMEOUT) {
            Ok((_, stdout, _)) => parse_gh_checks_output(&stdout),
            Err(_) => "none".to_string(),
        }
    } else {
        // No pr_number — skip GH CI check rather than querying PR #0
        "none".to_string()
    };

    // Delegate to fast_inner for the remaining logic (mode branch, freshness, merge)
    Ok(fast_inner(
        &branch,
        &state,
        &state_path,
        args.auto,
        args.manual,
        &bin_flow_path(),
        tree_changed,
        ci_skipped,
        ci_failed_output.as_deref(),
        &gh_ci_status,
        runner,
    ))
}

/// CLI entry wrapper: threads the production root, runner, and
/// CI-decider into `run_impl_inner`.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let root = project_root();
    run_impl_inner(args, &root, &run_cmd_with_timeout, &production_ci_decider)
}

/// CLI entry point.
pub fn run(args: Args) {
    match run_impl(&args) {
        Ok(result) => {
            println!("{}", result);
            if result.get("status").and_then(|v| v.as_str()) == Some("error") {
                std::process::exit(1);
            }
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
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::fs;

    fn mock_runner(responses: Vec<CmdResult>) -> impl Fn(&[&str], u64) -> CmdResult {
        let queue = RefCell::new(VecDeque::from(responses));
        move |_args: &[&str], _timeout: u64| -> CmdResult {
            queue
                .borrow_mut()
                .pop_front()
                .expect("mock_runner: no more responses")
        }
    }

    fn ok(stdout: &str) -> CmdResult {
        Ok((0, stdout.to_string(), String::new()))
    }

    fn make_state(learn_status: &str, skills: Option<Value>) -> Value {
        let mut state = json!({
            "schema_version": 1,
            "branch": "test-feature",
            "repo": "test/test",
            "pr_number": 42,
            "pr_url": "https://github.com/test/test/pull/42",
            "prompt": "test feature",
            "phases": {
                "flow-start": {"status": "complete"},
                "flow-plan": {"status": "complete"},
                "flow-code": {"status": "complete"},
                "flow-code-review": {"status": "complete"},
                "flow-learn": {"status": learn_status},
                "flow-complete": {"status": "pending"}
            }
        });
        if let Some(s) = skills {
            state["skills"] = s;
        }
        state
    }

    fn setup_state_file(root: &Path, branch: &str, state: &Value) -> PathBuf {
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let state_path = state_dir.join(format!("{}.json", branch));
        fs::write(&state_path, serde_json::to_string_pretty(state).unwrap()).unwrap();
        state_path
    }

    // --- Happy path: merged ---

    #[test]
    fn test_merged_path_happy() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        let state_path = setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![
            ok(r#"{"status": "up_to_date"}"#), // check-freshness
            ok("merged"),                      // gh pr merge --squash
        ]);

        let result = fast_inner(
            "test-feature",
            &state,
            &state_path,
            true,
            false,
            "/fake/bin/flow",
            false,  // tree_changed
            true,   // ci_skipped
            None,   // ci_failed_output
            "pass", // gh_ci_status
            &runner,
        );

        assert_eq!(result["status"], "ok");
        assert_eq!(result["path"], "merged");
        assert_eq!(result["pr_number"], 42);
        assert_eq!(result["ci_skipped"], true);
    }

    // --- CI stale after main merge ---

    #[test]
    fn test_ci_stale_after_main_merge() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        let state_path = setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![]);

        let result = fast_inner(
            "test-feature",
            &state,
            &state_path,
            true,
            false,
            "/fake/bin/flow",
            true, // tree_changed — main was merged in
            false,
            None,
            "pass",
            &runner,
        );

        assert_eq!(result["status"], "ok");
        assert_eq!(result["path"], "ci_stale");
    }

    // --- CI failed ---

    #[test]
    fn test_ci_failed() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        let state_path = setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![]);

        let result = fast_inner(
            "test-feature",
            &state,
            &state_path,
            true,
            false,
            "/fake/bin/flow",
            false,
            false,
            Some("test_foo assertion failed"), // ci_failed_output
            "pass",
            &runner,
        );

        assert_eq!(result["status"], "ok");
        assert_eq!(result["path"], "ci_failed");
        assert!(result["output"]
            .as_str()
            .unwrap()
            .contains("assertion failed"));
    }

    // --- GitHub CI pending ---

    #[test]
    fn test_ci_pending_github() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        let state_path = setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![]);

        let result = fast_inner(
            "test-feature",
            &state,
            &state_path,
            true,
            false,
            "/fake/bin/flow",
            false,
            true,
            None,
            "pending", // gh_ci_status
            &runner,
        );

        assert_eq!(result["status"], "ok");
        assert_eq!(result["path"], "ci_pending");
    }

    // --- Conflict from freshness check ---

    #[test]
    fn test_conflict_from_freshness() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        let state_path = setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![ok(
            r#"{"status": "conflict", "files": ["lib/foo.py"]}"#,
        )]);

        let result = fast_inner(
            "test-feature",
            &state,
            &state_path,
            true,
            false,
            "/fake/bin/flow",
            false,
            true,
            None,
            "pass",
            &runner,
        );

        assert_eq!(result["status"], "ok");
        assert_eq!(result["path"], "conflict");
        let files: Vec<String> = result["conflict_files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert!(files.contains(&"lib/foo.py".to_string()));
    }

    // --- Already merged ---

    #[test]
    fn test_already_merged() {
        // This path is handled in run_impl before fast_inner is called.
        // Test the gate logic directly with make_state.
        let state = make_state("complete", None);
        // Verify the state has the expected structure
        assert_eq!(
            state["phases"]["flow-learn"]["status"].as_str().unwrap(),
            "complete"
        );
        assert_eq!(state["pr_number"], 42);
    }

    // --- Manual mode returns confirm ---

    #[test]
    fn test_confirm_manual_mode() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        let state_path = setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![]);

        let result = fast_inner(
            "test-feature",
            &state,
            &state_path,
            false,
            true, // manual mode
            "/fake/bin/flow",
            false,
            true,
            None,
            "pass",
            &runner,
        );

        assert_eq!(result["status"], "ok");
        assert_eq!(result["path"], "confirm");
        assert_eq!(result["mode"], "manual");
    }

    // --- Gate: Learn not complete ---

    #[test]
    fn test_gate_failure_learn_not_complete() {
        let state = make_state("pending", None);
        let learn_status = state["phases"]["flow-learn"]["status"].as_str().unwrap();
        assert_eq!(learn_status, "pending");
        // The gate check in run_impl catches this before fast_inner is called.
        // Verify the state we'd check:
        assert_ne!(learn_status, "complete");
    }

    // --- Gate: No state file ---

    #[test]
    fn test_gate_failure_no_state() {
        let dir = tempfile::tempdir().unwrap();
        let result = read_state(dir.path(), "nonexistent-branch");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No state file found"));
    }

    // --- Max retries ---

    #[test]
    fn test_max_retries() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        let state_path = setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![ok(r#"{"status": "max_retries", "retries": 3}"#)]);

        let result = fast_inner(
            "test-feature",
            &state,
            &state_path,
            true,
            false,
            "/fake/bin/flow",
            false,
            true,
            None,
            "pass",
            &runner,
        );

        assert_eq!(result["status"], "ok");
        assert_eq!(result["path"], "max_retries");
    }

    // --- CI sentinel skip ---

    #[test]
    fn test_ci_sentinel_skip() {
        // When ci_skipped=true and no CI failure, fast_inner proceeds past CI
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        let state_path = setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![
            ok(r#"{"status": "up_to_date"}"#), // check-freshness
            ok("merged"),                      // gh pr merge
        ]);

        let result = fast_inner(
            "test-feature",
            &state,
            &state_path,
            true,
            false,
            "/fake/bin/flow",
            false,
            true, // ci_skipped — sentinel matched, no CI run needed
            None,
            "pass",
            &runner,
        );

        assert_eq!(result["path"], "merged");
        assert_eq!(result["ci_skipped"], true);
    }

    // --- parse_gh_checks_output ---

    #[test]
    fn test_parse_gh_checks_all_pass() {
        let output = "CI\tpass\t2m3s\thttps://...\nlint\tpass\t30s\thttps://...";
        assert_eq!(parse_gh_checks_output(output), "pass");
    }

    #[test]
    fn test_parse_gh_checks_has_pending() {
        let output = "CI\tpass\t2m3s\thttps://...\nbuild\tpending\t0s\thttps://...";
        assert_eq!(parse_gh_checks_output(output), "pending");
    }

    #[test]
    fn test_parse_gh_checks_has_fail() {
        let output = "CI\tfail\t2m3s\thttps://...\nlint\tpass\t30s\thttps://...";
        assert_eq!(parse_gh_checks_output(output), "fail");
    }

    #[test]
    fn test_parse_gh_checks_empty() {
        assert_eq!(parse_gh_checks_output(""), "none");
    }

    #[test]
    fn test_parse_gh_checks_fail_trumps_pending() {
        let output = "CI\tfail\t2m3s\thttps://...\nbuild\tpending\t0s\thttps://...";
        assert_eq!(parse_gh_checks_output(output), "fail");
    }

    // --- Step counter persistence ---

    #[test]
    fn test_merged_sets_step_counter() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        let state_path = setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![ok(r#"{"status": "up_to_date"}"#), ok("merged")]);

        fast_inner(
            "test-feature",
            &state,
            &state_path,
            true,
            false,
            "/fake/bin/flow",
            false,
            true,
            None,
            "pass",
            &runner,
        );

        let updated = fs::read_to_string(&state_path).unwrap();
        let updated_state: Value = serde_json::from_str(&updated).unwrap();
        assert_eq!(updated_state["complete_step"], json!(6));
    }

    // --- Freshness error without message key ---

    #[test]
    fn test_freshness_error_without_message_uses_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        let state_path = setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![
            ok(r#"{"status": "error"}"#), // no "message" key
        ]);

        let result = fast_inner(
            "test-feature",
            &state,
            &state_path,
            true,
            false,
            "/fake/bin/flow",
            false,
            true,
            None,
            "pass",
            &runner,
        );

        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("check-freshness failed"));
    }

    // --- Push failure in merged freshness path ---

    #[test]
    fn test_freshness_merged_push_failure() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        let state_path = setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![
            ok(r#"{"status": "merged"}"#), // freshness says main moved
            Ok((1, String::new(), "remote rejected".to_string())), // push fails
        ]);

        let result = fast_inner(
            "test-feature",
            &state,
            &state_path,
            true,
            false,
            "/fake/bin/flow",
            false,
            true,
            None,
            "pass",
            &runner,
        );

        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("push failed"));
    }

    // --- fast_inner error paths ---

    #[test]
    fn test_fast_inner_unknown_gh_ci_status_proceeds() {
        // Unknown gh_ci_status values fall through the `_` arm and
        // continue to freshness + merge (L198).
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        let state_path = setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![ok(r#"{"status": "up_to_date"}"#), ok("merged")]);

        let result = fast_inner(
            "test-feature",
            &state,
            &state_path,
            true,
            false,
            "/fake/bin/flow",
            false,
            true,
            None,
            "unknown", // unexpected status — `_` arm
            &runner,
        );

        assert_eq!(result["status"], "ok");
        assert_eq!(result["path"], "merged");
    }

    #[test]
    fn test_fast_inner_freshness_runner_err() {
        // check-freshness subprocess spawn failure (L223-232).
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        let state_path = setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![Err("spawn failed".to_string())]);

        let result = fast_inner(
            "test-feature",
            &state,
            &state_path,
            true,
            false,
            "/fake/bin/flow",
            false,
            true,
            None,
            "pass",
            &runner,
        );

        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("check-freshness failed"));
    }

    #[test]
    fn test_fast_inner_freshness_invalid_json() {
        // check-freshness returns unparseable stdout (L234-243).
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        let state_path = setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![ok("not json")]);

        let result = fast_inner(
            "test-feature",
            &state,
            &state_path,
            true,
            false,
            "/fake/bin/flow",
            false,
            true,
            None,
            "pass",
            &runner,
        );

        assert_eq!(result["status"], "error");
        assert!(result["message"].as_str().unwrap().contains("Invalid JSON"));
    }

    #[test]
    fn test_fast_inner_unexpected_freshness_status() {
        // check-freshness returns a status the match does not recognize (L377-384).
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        let state_path = setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![ok(r#"{"status": "rabbit"}"#)]);

        let result = fast_inner(
            "test-feature",
            &state,
            &state_path,
            true,
            false,
            "/fake/bin/flow",
            false,
            true,
            None,
            "pass",
            &runner,
        );

        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("Unexpected check-freshness status"));
    }

    #[test]
    fn test_fast_inner_squash_merge_spawn_err() {
        // gh pr merge subprocess spawn failure (L325-331).
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        let state_path = setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![
            ok(r#"{"status": "up_to_date"}"#),
            Err("gh not found".to_string()),
        ]);

        let result = fast_inner(
            "test-feature",
            &state,
            &state_path,
            true,
            false,
            "/fake/bin/flow",
            false,
            true,
            None,
            "pass",
            &runner,
        );

        assert_eq!(result["status"], "error");
        assert!(result["message"].as_str().unwrap().contains("gh not found"));
    }

    #[test]
    fn test_fast_inner_squash_merge_base_branch_policy() {
        // gh pr merge fails with "base branch policy" stderr → ci_pending (L353-365).
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        let state_path = setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![
            ok(r#"{"status": "up_to_date"}"#),
            Ok((
                1,
                String::new(),
                "base branch policy: required status check".to_string(),
            )),
        ]);

        let result = fast_inner(
            "test-feature",
            &state,
            &state_path,
            true,
            false,
            "/fake/bin/flow",
            false,
            true,
            None,
            "pass",
            &runner,
        );

        assert_eq!(result["status"], "ok");
        assert_eq!(result["path"], "ci_pending");
    }

    #[test]
    fn test_fast_inner_squash_merge_generic_failure() {
        // gh pr merge fails with non-policy stderr → error (L367-372).
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        let state_path = setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![
            ok(r#"{"status": "up_to_date"}"#),
            Ok((1, String::new(), "Merge conflict detected".to_string())),
        ]);

        let result = fast_inner(
            "test-feature",
            &state,
            &state_path,
            true,
            false,
            "/fake/bin/flow",
            false,
            true,
            None,
            "pass",
            &runner,
        );

        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("Merge conflict"));
    }

    #[test]
    fn test_fast_inner_freshness_merged_push_success() {
        // Freshness reports main moved; push succeeds → ci_stale (L305-317).
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        let state_path = setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![ok(r#"{"status": "merged"}"#), ok("")]);

        let result = fast_inner(
            "test-feature",
            &state,
            &state_path,
            true,
            false,
            "/fake/bin/flow",
            false,
            true,
            None,
            "pass",
            &runner,
        );

        assert_eq!(result["status"], "ok");
        assert_eq!(result["path"], "ci_stale");
        assert!(result["reason"].as_str().unwrap().contains("main moved"));
    }

    #[test]
    fn test_fast_inner_freshness_merged_runner_err_on_push() {
        // Freshness reports main moved; push runner returns Err (L290-297).
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        let state_path = setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![
            ok(r#"{"status": "merged"}"#),
            Err("no network".to_string()),
        ]);

        let result = fast_inner(
            "test-feature",
            &state,
            &state_path,
            true,
            false,
            "/fake/bin/flow",
            false,
            true,
            None,
            "pass",
            &runner,
        );

        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("Push failed after freshness merge"));
    }

    // --- production_ci_decider ---

    #[test]
    fn production_ci_decider_tree_changed_returns_not_skipped() {
        // When main was merged into the branch, tree_changed=true forces
        // a fresh CI run regardless of sentinel state. The decider
        // short-circuits and returns (false, None) without touching the
        // sentinel, so fast_inner's ci_stale path surfaces.
        let dir = tempfile::tempdir().unwrap();
        let (skipped, failed) = production_ci_decider(dir.path(), dir.path(), "test-feature", true);
        assert!(!skipped);
        assert!(failed.is_none());
    }

    // --- run_impl_inner ---

    fn no_ci(_: &Path, _: &Path, _: &str, _: bool) -> (bool, Option<String>) {
        (true, None) // default: sentinel hit, no CI failure
    }

    fn ci_failed_decider(_: &Path, _: &Path, _: &str, _: bool) -> (bool, Option<String>) {
        (false, Some("ci failed on sample test".to_string()))
    }

    fn inner_args(branch: &str) -> Args {
        Args {
            branch: Some(branch.to_string()),
            auto: true,
            manual: false,
        }
    }

    #[test]
    fn test_run_impl_inner_learn_gate_pending_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("pending", None);
        setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![]);
        let args = inner_args("test-feature");

        let result = run_impl_inner(&args, dir.path(), &runner, &no_ci).unwrap();

        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("Phase 5: Learn must be complete"));
    }

    #[test]
    fn test_run_impl_inner_pr_status_runner_err() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        setup_state_file(dir.path(), "test-feature", &state);

        // First runner call is check_pr_status's `gh pr view`; return Err.
        let runner = mock_runner(vec![Err("gh not found".to_string())]);
        let args = inner_args("test-feature");

        let result = run_impl_inner(&args, dir.path(), &runner, &no_ci).unwrap();

        assert_eq!(result["status"], "error");
        assert!(result["message"].as_str().unwrap().contains("gh not found"));
    }

    #[test]
    fn test_run_impl_inner_pr_merged_returns_already_merged() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![ok("MERGED")]);
        let args = inner_args("test-feature");

        let result = run_impl_inner(&args, dir.path(), &runner, &no_ci).unwrap();

        assert_eq!(result["status"], "ok");
        assert_eq!(result["path"], "already_merged");
    }

    #[test]
    fn test_run_impl_inner_pr_closed_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        setup_state_file(dir.path(), "test-feature", &state);

        let runner = mock_runner(vec![ok("CLOSED")]);
        let args = inner_args("test-feature");

        let result = run_impl_inner(&args, dir.path(), &runner, &no_ci).unwrap();

        assert_eq!(result["status"], "error");
        assert!(result["message"].as_str().unwrap().contains("closed"));
    }

    #[test]
    fn test_run_impl_inner_merge_main_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        setup_state_file(dir.path(), "test-feature", &state);

        // gh pr view → OPEN; fetch ok; is-ancestor non-zero (not ancestor);
        // merge non-zero; status --porcelain shows UU conflict marker.
        let runner = mock_runner(vec![
            ok("OPEN"),                                // check_pr_status
            ok(""),                                    // git fetch
            Ok((1, String::new(), String::new())),     // is-ancestor → not
            Ok((1, String::new(), "conflict".into())), // git merge fails
            ok("UU src/conflicting.rs\n"),             // git status --porcelain
        ]);
        let args = inner_args("test-feature");

        let result = run_impl_inner(&args, dir.path(), &runner, &no_ci).unwrap();

        assert_eq!(result["status"], "ok");
        assert_eq!(result["path"], "conflict");
        let files = result["conflict_files"].as_array().unwrap();
        assert!(files
            .iter()
            .any(|v| v.as_str().unwrap() == "src/conflicting.rs"));
    }

    #[test]
    fn test_run_impl_inner_merge_main_error() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        setup_state_file(dir.path(), "test-feature", &state);

        // gh pr view → OPEN; fetch fails.
        let runner = mock_runner(vec![ok("OPEN"), Err("network down".to_string())]);
        let args = inner_args("test-feature");

        let result = run_impl_inner(&args, dir.path(), &runner, &no_ci).unwrap();

        assert_eq!(result["status"], "error");
        assert!(result["message"].as_str().unwrap().contains("network down"));
    }

    #[test]
    fn test_run_impl_inner_ci_skipped_sentinel_hit() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        setup_state_file(dir.path(), "test-feature", &state);

        // gh pr view → OPEN; merge_main clean (fetch ok + is-ancestor 0);
        // gh pr checks → pass; freshness up_to_date; squash merge → success.
        let runner = mock_runner(vec![
            ok("OPEN"),                            // check_pr_status
            ok(""),                                // git fetch
            Ok((0, String::new(), String::new())), // is-ancestor ok → clean
            ok("CI\tpass\t\n"),                    // gh pr checks
            ok(r#"{"status": "up_to_date"}"#),     // check-freshness
            ok("merged"),                          // gh pr merge --squash
        ]);
        let args = inner_args("test-feature");

        let result = run_impl_inner(&args, dir.path(), &runner, &no_ci).unwrap();

        assert_eq!(result["status"], "ok");
        assert_eq!(result["path"], "merged");
        assert_eq!(result["ci_skipped"], true);
    }

    #[test]
    fn test_run_impl_inner_ci_failed() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        setup_state_file(dir.path(), "test-feature", &state);

        // gh pr view → OPEN; merge clean; gh pr checks → pass; ci_decider
        // reports failure → fast_inner returns ci_failed before freshness.
        let runner = mock_runner(vec![
            ok("OPEN"),                            // check_pr_status
            ok(""),                                // git fetch
            Ok((0, String::new(), String::new())), // is-ancestor ok
            ok("CI\tpass\t\n"),                    // gh pr checks
        ]);
        let args = inner_args("test-feature");

        let result = run_impl_inner(&args, dir.path(), &runner, &ci_failed_decider).unwrap();

        assert_eq!(result["status"], "ok");
        assert_eq!(result["path"], "ci_failed");
        assert!(result["output"]
            .as_str()
            .unwrap()
            .contains("ci failed on sample test"));
    }

    #[test]
    fn test_run_impl_inner_gh_ci_pending() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("complete", None);
        setup_state_file(dir.path(), "test-feature", &state);

        // gh pr view → OPEN; merge clean; gh pr checks → pending.
        let runner = mock_runner(vec![
            ok("OPEN"),                            // check_pr_status
            ok(""),                                // git fetch
            Ok((0, String::new(), String::new())), // is-ancestor ok
            ok("CI\tpending\t\n"),                 // gh pr checks
        ]);
        let args = inner_args("test-feature");

        let result = run_impl_inner(&args, dir.path(), &runner, &no_ci).unwrap();

        assert_eq!(result["status"], "ok");
        assert_eq!(result["path"], "ci_pending");
    }

    #[test]
    fn test_run_impl_inner_no_pr_number_skips_gh_check() {
        let dir = tempfile::tempdir().unwrap();
        // Build a state with no pr_number (Null).
        let mut state = make_state("complete", None);
        state["pr_number"] = serde_json::Value::Null;
        setup_state_file(dir.path(), "test-feature", &state);

        // check_pr_status falls back to branch identifier and still makes
        // one runner call. The gh pr checks call is SKIPPED because
        // pr_number is None, so the queue has no entry for it.
        let runner = mock_runner(vec![
            ok("OPEN"),                            // check_pr_status (by branch)
            ok(""),                                // git fetch
            Ok((0, String::new(), String::new())), // is-ancestor ok
            // no gh pr checks entry — should not be invoked
            ok(r#"{"status": "up_to_date"}"#), // check-freshness
            ok("merged"),                      // gh pr merge --squash
        ]);
        let args = inner_args("test-feature");

        let result = run_impl_inner(&args, dir.path(), &runner, &no_ci).unwrap();

        assert_eq!(result["status"], "ok");
        assert_eq!(result["path"], "merged");
    }

    // --- parse_gh_checks_output ---

    #[test]
    fn parse_gh_checks_output_empty_returns_none() {
        assert_eq!(parse_gh_checks_output(""), "none");
    }

    #[test]
    fn parse_gh_checks_output_all_passing() {
        let input = "CI/build\tpass\tok\nCI/test\tpass\tok\n";
        assert_eq!(parse_gh_checks_output(input), "pass");
    }

    #[test]
    fn parse_gh_checks_output_any_failing() {
        let input = "CI/build\tpass\tok\nCI/test\tfail\terror\n";
        assert_eq!(parse_gh_checks_output(input), "fail");
    }

    #[test]
    fn parse_gh_checks_output_pending_without_fail() {
        let input = "CI/build\tpass\tok\nCI/test\tpending\t\n";
        assert_eq!(parse_gh_checks_output(input), "pending");
    }

    #[test]
    fn parse_gh_checks_output_fail_outranks_pending() {
        let input = "CI/a\tpending\t\nCI/b\tfail\t\n";
        assert_eq!(parse_gh_checks_output(input), "fail");
    }

    #[test]
    fn parse_gh_checks_output_skips_malformed_lines() {
        // Lines without tabs are ignored (no 2+ parts → skipped).
        let input = "no-tab-line\nCI/b\tpass\tok\n";
        assert_eq!(parse_gh_checks_output(input), "pass");
    }

    // --- read_state ---

    #[test]
    fn read_state_missing_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let err = read_state(dir.path(), "missing-branch").unwrap_err();
        assert!(err.contains("No state file found"));
    }

    #[test]
    fn read_state_invalid_json_errors() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let state_path = state_dir.join("bad.json");
        fs::write(&state_path, "not json").unwrap();
        let err = read_state(dir.path(), "bad").unwrap_err();
        assert!(err.contains("Could not parse state file"));
    }

    #[test]
    fn read_state_non_object_root_errors() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let state_path = state_dir.join("arr.json");
        fs::write(&state_path, "[]").unwrap();
        let err = read_state(dir.path(), "arr").unwrap_err();
        assert!(err.contains("Corrupt state file"));
    }

    #[test]
    fn read_state_valid_object_returns_state_and_path() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let state_path = state_dir.join("ok.json");
        fs::write(&state_path, r#"{"branch": "ok", "foo": 1}"#).unwrap();
        let (state, path) = read_state(dir.path(), "ok").unwrap();
        assert_eq!(state["foo"], 1);
        assert_eq!(path, state_path);
    }
}
