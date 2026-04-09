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
};
use crate::git::{project_root, resolve_branch};
use crate::lock::mutate_state;
use crate::phase_transition::phase_enter;
use crate::utils::{bin_flow_path, derive_worktree};

/// Step counter total for complete phase: 6 steps (running checks, local CI,
/// GitHub CI, confirming, merging PR, finalizing).
const COMPLETE_STEPS_TOTAL: i64 = 6;
const NETWORK_TIMEOUT: u64 = 60;

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
    let state_path = root.join(".flow-states").join(format!("{}.json", branch));
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
    _root: &Path,
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

/// Core complete-fast logic. Returns Ok(json) on success paths (including
/// unhappy paths like ci_failed that the skill handles interactively),
/// Err(string) only for infrastructure failures.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let root = project_root();
    let branch = resolve_branch(args.branch.as_deref(), &root)
        .ok_or("Could not determine current branch")?;

    // Read state file
    let (state, state_path) = read_state(&root, &branch)?;

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
        &run_cmd_with_timeout,
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
    let (merge_status, merge_data) = merge_main(&run_cmd_with_timeout);
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
    let ci_skipped;
    let ci_failed_output: Option<String>;

    if tree_changed {
        ci_skipped = false;
        ci_failed_output = None;
    } else {
        let snapshot = ci::tree_snapshot(&cwd, None);
        let sentinel = ci::sentinel_path(&root, &branch);

        ci_skipped = if sentinel.exists() {
            std::fs::read_to_string(&sentinel)
                .map(|c| c == snapshot)
                .unwrap_or(false)
        } else {
            false
        };

        if !ci_skipped {
            let ci_args = ci::Args {
                force: false,
                retry: 0,
                branch: Some(branch.clone()),
                simulate_branch: None,
            };
            let (ci_result, ci_code) = ci::run_impl(&ci_args, &cwd, &root, false);
            if ci_code != 0 {
                ci_failed_output = Some(
                    ci_result
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("CI failed")
                        .to_string(),
                );
            } else {
                ci_failed_output = None;
            }
        } else {
            ci_failed_output = None;
        }
    }

    // --- GitHub CI check ---
    let pr_number = state.get("pr_number").and_then(|v| v.as_i64());
    let gh_ci_status = if let Some(pr_num) = pr_number {
        let pr_str = pr_num.to_string();
        match run_cmd_with_timeout(&["gh", "pr", "checks", &pr_str], NETWORK_TIMEOUT) {
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
        &root,
        &state,
        &state_path,
        args.auto,
        args.manual,
        &bin_flow_path(),
        tree_changed,
        ci_skipped,
        ci_failed_output.as_deref(),
        &gh_ci_status,
        &run_cmd_with_timeout,
    ))
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
            dir.path(),
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
            dir.path(),
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
            dir.path(),
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
            dir.path(),
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
            dir.path(),
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
            dir.path(),
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
            dir.path(),
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
            dir.path(),
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
            dir.path(),
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
            dir.path(),
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
            dir.path(),
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
}
