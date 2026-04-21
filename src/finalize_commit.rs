//! Commit, cleanup, pull, push.
//!
//! Two gates run before committing:
//!
//! 1. **CI gate** — calls [`ci::run_impl`]. If CI fails, returns an error
//!    and commits nothing. When the CI sentinel is fresh (CI already passed
//!    for this tree state), the check noops instantly.
//! 2. **Plan-deviation gate** — calls [`plan_deviation::run_impl`] to
//!    cross-reference plan-named test fixture values against the staged
//!    diff. If an unacknowledged drift is detected, returns an error with
//!    `step = "plan_deviation"` and a structured stderr message.
//!
//! Usage:
//!   bin/flow finalize-commit <message-file> <branch>
//!
//! Output (JSON to stdout):
//!   Success:   {"status": "ok", "sha": "<commit-hash>", "pull_merged": <bool>}
//!   Warning:   {"status": "ok", "sha": "", "pull_merged": true, "warning": "..."}
//!   Conflict:  {"status": "conflict", "files": ["file1.py", ...]}
//!   Error:     {"status": "error", "step": "ci|plan_deviation|commit|pull|push", "message": "..."}

use std::fs;

use clap::Parser;
use serde_json::{json, Value};

use crate::commands::log::append_log;
use crate::complete_preflight::{run_cmd_with_timeout, LOCAL_TIMEOUT, NETWORK_TIMEOUT};
use crate::flow_paths::FlowPaths;
use crate::lock::mutate_state;
use crate::phase_config::phase_number;
use crate::plan_deviation::Deviation;
use crate::utils::parse_conflict_files;

#[derive(Parser, Debug)]
#[command(
    name = "finalize-commit",
    about = "Finalize a commit: commit, cleanup, pull, push"
)]
pub struct Args {
    /// Path to the commit message file
    pub message_file: String,
    /// Branch name for git pull
    pub branch: String,
}

/// Remove the commit message file, ignoring errors.
fn remove_message_file(path: &str) {
    let _ = fs::remove_file(path);
}

/// Print a user-facing block message for unacknowledged plan
/// signature deviations. Each deviation shows the plan file
/// line, the fixture key, and the plan value that is missing
/// from the staged test body. The trailing section lists the
/// `bin/flow log` template the user runs to acknowledge each
/// deviation before re-running the commit.
fn emit_deviation_stderr(branch: &str, deviations: &[Deviation]) {
    eprintln!("BLOCKED: Plan signature deviation detected.");
    eprintln!();
    for dev in deviations {
        eprintln!("Test: {}", dev.test_name);
        eprintln!(
            "  Plan value (line {}): {} = \"{}\"",
            dev.plan_line, dev.fixture_key, dev.plan_value
        );
        eprintln!(
            "  Staged diff does not contain \"{}\" in the test body.",
            dev.plan_value
        );
        eprintln!();
    }
    eprintln!("If this deviation is intentional, log it before committing:");
    eprintln!();
    for dev in deviations {
        eprintln!(
            "  bin/flow log {} \"[Phase 3] Plan signature deviation: {} drifted from {} to <new value>. Reason: <why>\"",
            branch, dev.test_name, dev.plan_value
        );
    }
    eprintln!();
    eprintln!("Then re-run the commit.");
}

/// Core finalize-commit logic with injectable git runner for testing.
#[allow(clippy::type_complexity)]
pub fn finalize_commit_inner(
    message_file: &str,
    branch: &str,
    git: &dyn Fn(&[&str], u64) -> Result<(i32, String, String), String>,
) -> Value {
    // Step 1: git commit -F <message_file>
    match git(&["commit", "-F", message_file], LOCAL_TIMEOUT) {
        Err(e) => {
            remove_message_file(message_file);
            return json!({
                "status": "error",
                "step": "commit",
                "message": format!("git commit {}", e)
            });
        }
        Ok((code, _, stderr)) => {
            remove_message_file(message_file);
            if code != 0 {
                return json!({
                    "status": "error",
                    "step": "commit",
                    "message": stderr.trim()
                });
            }
        }
    }

    // Capture post-commit SHA for pull_merged detection.
    // If this fails, default to pull_merged=true (safe: don't refresh sentinel).
    let post_commit_sha =
        git(&["rev-parse", "HEAD"], LOCAL_TIMEOUT)
            .ok()
            .and_then(|(code, stdout, _)| {
                if code == 0 {
                    Some(stdout.trim().to_string())
                } else {
                    None
                }
            });

    // Step 2: git pull origin <branch>
    match git(&["pull", "origin", branch], NETWORK_TIMEOUT) {
        Err(e) => {
            return json!({
                "status": "error",
                "step": "pull",
                "message": format!("git pull {}", e)
            });
        }
        Ok((code, _, stderr)) if code != 0 => {
            // Check for merge conflicts
            match git(&["status", "--porcelain"], LOCAL_TIMEOUT) {
                Err(_) => {
                    return json!({
                        "status": "error",
                        "step": "pull",
                        "message": stderr.trim()
                    });
                }
                Ok((_, stdout, _)) => {
                    let conflicts = parse_conflict_files(&stdout);
                    if !conflicts.is_empty() {
                        return json!({"status": "conflict", "files": conflicts});
                    }
                    return json!({
                        "status": "error",
                        "step": "pull",
                        "message": stderr.trim()
                    });
                }
            }
        }
        Ok(_) => {} // pull succeeded
    }

    // Step 3: git push
    match git(&["push"], NETWORK_TIMEOUT) {
        Err(e) => {
            return json!({
                "status": "error",
                "step": "push",
                "message": format!("git push {}", e)
            });
        }
        Ok((code, _, stderr)) if code != 0 => {
            return json!({
                "status": "error",
                "step": "push",
                "message": stderr.trim()
            });
        }
        Ok(_) => {}
    }

    // Step 4: git rev-parse HEAD
    match git(&["rev-parse", "HEAD"], LOCAL_TIMEOUT) {
        Err(_) => json!({
            "status": "ok",
            "sha": "",
            "pull_merged": true,
            "warning": "commit succeeded but SHA retrieval timed out"
        }),
        Ok((code, _, _)) if code != 0 => json!({
            "status": "ok",
            "sha": "",
            "pull_merged": true,
            "warning": "commit succeeded but SHA retrieval failed"
        }),
        Ok((_, stdout, _)) => {
            let final_sha = stdout.trim();
            let pull_merged = post_commit_sha.as_deref() != Some(final_sha);
            json!({"status": "ok", "sha": final_sha, "pull_merged": pull_merged})
        }
    }
}

/// Adapter: prepends `git -C <cwd>` so `run_impl` can target a specific
/// directory without `set_current_dir` (which races in parallel tests).
/// Delegates to the shared `run_cmd_with_timeout` helper, which owns the
/// subprocess-spawn/poll/kill logic and is covered by its own tests.
fn run_git_in_dir(
    cwd: &std::path::Path,
    args: &[&str],
    timeout_secs: u64,
) -> Result<(i32, String, String), String> {
    let mut cmd_args = vec!["git", "-C", cwd.to_str().unwrap_or(".")];
    cmd_args.extend_from_slice(args);
    run_cmd_with_timeout(&cmd_args, timeout_secs)
}

/// Testable entry point: enforces CI, runs finalize-commit, then maintains
/// the CI sentinel (refresh on clean pull, delete on merge-pull).
///
/// `cwd` and `root` are passed explicitly so integration tests can avoid
/// `set_current_dir` (which is process-wide and races with parallel tests).
///
/// Returns `Result<Value, String>` where `Ok` carries any JSON response
/// including status-error payloads (CI failure, commit failure, etc.) and
/// `Err` carries only infrastructure errors (empty arguments).
pub fn run_impl(
    args: &Args,
    cwd: &std::path::Path,
    root: &std::path::Path,
) -> Result<Value, String> {
    if args.message_file.is_empty() || args.branch.is_empty() {
        return Err("Usage: bin/flow finalize-commit <message-file> <branch>".to_string());
    }

    // Derive phase number from state file's current_phase for log prefixes.
    let pn = {
        let state_path = FlowPaths::new(root, &args.branch).state_file();
        std::fs::read_to_string(&state_path)
            .ok()
            .and_then(|c| serde_json::from_str::<Value>(&c).ok())
            .and_then(|s| {
                s.get("current_phase")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .map(|p| phase_number(&p))
            .unwrap_or(0)
    };

    // Enforce CI before committing. run_impl checks the sentinel first —
    // if CI already passed for this tree state, it noops instantly.
    let ci_args = crate::ci::Args {
        force: false,
        retry: 0,
        branch: Some(args.branch.clone()),
        simulate_branch: None,
        format: false,
        lint: false,
        build: false,
        test: false,
        audit: false,
        clean: false,
        trailing: Vec::new(),
    };
    let (ci_result, ci_code) = crate::ci::run_impl(&ci_args, cwd, root, false);

    let result = if ci_code != 0 {
        let msg = ci_result["message"]
            .as_str()
            .unwrap_or("bin/flow ci failed");
        let _ = append_log(
            root,
            &args.branch,
            &format!("[Phase {}] finalize-commit — ci (failed)", pn),
        );
        json!({
            "status": "error",
            "step": "ci",
            "message": msg,
        })
    } else {
        let _ = append_log(
            root,
            &args.branch,
            &format!("[Phase {}] finalize-commit — ci (ok)", pn),
        );
        let cwd_owned = cwd.to_path_buf();
        let git = |git_args: &[&str], timeout: u64| -> Result<(i32, String, String), String> {
            run_git_in_dir(&cwd_owned, git_args, timeout)
        };

        // Capture the staged diff for the plan-deviation gate.
        // An error or non-zero exit from `git diff --cached`
        // produces an empty diff, which makes the gate a no-op
        // (the plan has no tests to cross-reference against).
        let staged_diff = match git(&["diff", "--cached"], LOCAL_TIMEOUT) {
            Ok((0, stdout, _)) => stdout,
            _ => String::new(),
        };

        // Plan signature deviation gate. Blocks the commit when
        // a plan-named test's fixture value drifts without a
        // matching log acknowledgment. The gate is mechanical
        // enforcement of `.claude/rules/plan-commit-atomicity.md`
        // "Plan Signature Deviations Must Be Logged".
        match crate::plan_deviation::run_impl(root, &args.branch, &staged_diff) {
            Ok(()) => finalize_commit_inner(&args.message_file, &args.branch, &git),
            Err(deviations) => {
                emit_deviation_stderr(&args.branch, &deviations);
                let _ = append_log(
                    root,
                    &args.branch,
                    &format!(
                        "[Phase {}] finalize-commit — plan_deviation (blocked: {} deviation{})",
                        pn,
                        deviations.len(),
                        if deviations.len() == 1 { "" } else { "s" }
                    ),
                );
                let deviation_json: Vec<Value> = deviations
                    .iter()
                    .map(|d| {
                        json!({
                            "test_name": d.test_name,
                            "fixture_key": d.fixture_key,
                            "plan_value": d.plan_value,
                            "plan_line": d.plan_line,
                        })
                    })
                    .collect();
                json!({
                    "status": "error",
                    "step": "plan_deviation",
                    "message": format!(
                        "{} unacknowledged plan signature deviation{}",
                        deviations.len(),
                        if deviations.len() == 1 { "" } else { "s" }
                    ),
                    "deviations": deviation_json,
                })
            }
        }
    };

    // Log final result
    let final_status = result["status"].as_str().unwrap_or("unknown");
    let _ = append_log(
        root,
        &args.branch,
        &format!(
            "[Phase {}] finalize-commit — done (\"{}\")",
            pn, final_status
        ),
    );

    // Clear continuation flags on error so the stop-continue hook
    // does not force-advance the parent phase after a failed commit.
    // Conflict is NOT cleared — the commit skill retries after resolving.
    if result["status"] == "error" {
        let state_path = FlowPaths::new(root, &args.branch).state_file();
        if state_path.exists() {
            let _ = mutate_state(&state_path, &mut |state| {
                if !(state.is_object() || state.is_null()) {
                    return;
                }
                state["_continue_pending"] = Value::String(String::new());
                state["_continue_context"] = Value::String(String::new());
            });
        }
    }

    // Sentinel maintenance after commit:
    // - pull_merged == false: tree unchanged by pull → refresh sentinel to current snapshot.
    // - pull_merged == true: pull brought in new content → remove stale sentinel so the
    //   next CI run re-tests. (CI's run_once created the sentinel before the commit;
    //   the pull invalidated it.)
    if result["status"] == "ok" {
        let sentinel = crate::ci::sentinel_path(root, &args.branch);
        if result.get("pull_merged") == Some(&json!(false)) {
            let snapshot = crate::ci::tree_snapshot(cwd, None);
            let _ = fs::write(&sentinel, &snapshot);
        } else {
            let _ = fs::remove_file(&sentinel);
        }
    }

    Ok(result)
}

/// Main-arm dispatch: returns (value, exit code). Err wraps into JSON.
pub fn run_impl_main(args: &Args) -> (serde_json::Value, i32) {
    let cwd = std::env::current_dir().unwrap_or(std::path::PathBuf::from("."));
    let root = crate::git::project_root();
    match run_impl(args, &cwd, &root) {
        Err(msg) => (
            json!({"status": "error", "message": msg, "step": "args"}),
            1,
        ),
        Ok(result) => {
            let code = if result["status"] == "ok" { 0 } else { 1 };
            (result, code)
        }
    }
}
