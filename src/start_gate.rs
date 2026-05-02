//! Consolidated start-gate: git pull + CI baseline (retry 3) + update-deps +
//! post-deps CI (retry 3 if deps changed) in a single command.
//!
//! Returns JSON with status:
//! - "clean" — all gates passed (may include deps_changed, flaky info)
//! - "ci_flaky" — CI was flaky (baseline or post-deps), includes filing context
//! - "ci_failed" — consistent CI failure on baseline (lock held)
//! - "deps_ci_failed" — consistent CI failure after dep update (lock held)
//! - "error" — infrastructure failure (pull failed, non-consistent CI error,
//!   deps error, commit-deps error)
//!
//! Logic is driven entirely through the compiled binary; integration tests
//! use real git and controllable `bin/*` stubs in a `TempDir` fixture.

use std::path::Path;
use std::process::{Command, Stdio};

use clap::Parser;
use serde_json::{json, Value};

use crate::ci;
use crate::commands::log::append_log;
use crate::commands::start_step::update_step;
use crate::flow_paths::FlowPaths;
use crate::git::read_base_branch;
use crate::update_deps::run_update_deps;

/// Resolve the integration branch for `run_impl_main`. Routes through
/// `git::read_base_branch` (the no-silent-fallback helper) and falls
/// back to `"main"` when the read returns an error so legacy state
/// files and minimal test fixtures keep working without re-encoding
/// the fallback at every call site.
fn resolve_base_branch(state_path: &Path) -> String {
    read_base_branch(state_path).unwrap_or_else(|_| "main".to_string())
}

const DEPS_TIMEOUT_SECS: u64 = 300;

#[derive(Parser, Debug)]
#[command(name = "start-gate", about = "Consolidated CI and dependency gate")]
pub struct Args {
    /// Branch name for state file lookup and logging
    #[arg(long)]
    pub branch: String,
}

/// Main-arm entry point: drives git pull + baseline CI + update-deps +
/// post-deps CI + commit-deps sequentially. `run_impl_with_deps` always
/// returns `Value` — business errors appear in the `status: "error"`
/// payload with exit code `0`.
pub fn run_impl_main(args: &Args, root: &Path, cwd: &Path) -> (Value, i32) {
    let branch = &args.branch;

    // Update TUI step counter
    let state_path = FlowPaths::new(root, branch).state_file();
    update_step(&state_path, 2);

    // Read the integration branch the user is working off of (captured
    // at flow-start time in init_state). All subsequent git pull/push,
    // CI baseline, and deps-commit operations target this branch
    // instead of a hardcoded "main", so repos whose default branch is
    // e.g. `staging` coordinate against their actual integration branch.
    let base_branch = resolve_base_branch(&state_path);

    // Step 1: git pull origin <base_branch>
    let pull_result = git_pull(cwd, &base_branch);
    let _ = append_log(
        root,
        branch,
        &format!(
            "[Phase 1] start-gate — git pull ({})",
            if pull_result.is_ok() { "ok" } else { "error" }
        ),
    );
    if let Err(msg) = pull_result {
        return (
            json!({
                "status": "error",
                "message": format!("git pull failed: {}", msg),
                "step": "git_pull",
            }),
            0,
        );
    }

    // Step 2: CI baseline with retry
    let ci_args = ci::Args {
        force: false,
        retry: 3,
        branch: Some(base_branch.clone()),
        simulate_branch: None,
        format: false,
        lint: false,
        build: false,
        test: false,
        audit: false,
        clean: false,
        trailing: Vec::new(),
    };
    let (ci_result, _ci_code) = ci::run_impl(&ci_args, cwd, root, false);
    let _ = append_log(
        root,
        branch,
        &format!(
            "[Phase 1] start-gate — CI baseline ({})",
            ci_result["status"]
        ),
    );

    let mut flaky_info: Option<Value> = None;

    if ci_result["status"] == "error" {
        if ci_result
            .get("consistent")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return (
                json!({
                    "status": "ci_failed",
                    "output": ci_result["output"],
                    "attempts": ci_result["attempts"],
                }),
                0,
            );
        }
        return (
            json!({
                "status": "error",
                "message": ci_result["message"],
                "step": "ci_baseline",
            }),
            0,
        );
    }

    // Check for flaky baseline
    if ci_result
        .get("flaky")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        flaky_info = Some(json!({
            "first_failure_output": ci_result["first_failure_output"],
            "attempts": ci_result["attempts"],
            "flaky_context": "CI baseline on pristine main during flow-start",
        }));
    }

    // Step 3: Update dependencies
    let (deps_result, _deps_code) = run_update_deps(cwd, DEPS_TIMEOUT_SECS);
    let _ = append_log(
        root,
        branch,
        &format!(
            "[Phase 1] start-gate — update-deps ({})",
            deps_result["status"]
        ),
    );

    let deps_skipped = deps_result["status"] == "skipped";
    let deps_no_changes = deps_result["status"] == "ok"
        && !deps_result
            .get("changes")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
    let deps_error = deps_result["status"] == "error";

    if deps_error {
        return (
            json!({
                "status": "error",
                "message": deps_result["message"],
                "step": "update_deps",
            }),
            0,
        );
    }

    if deps_skipped || deps_no_changes {
        // No dep changes — return clean (with flaky info if applicable)
        if let Some(info) = flaky_info {
            return (
                json!({
                    "status": "ci_flaky",
                    "first_failure_output": info["first_failure_output"],
                    "attempts": info["attempts"],
                    "flaky_context": info["flaky_context"],
                }),
                0,
            );
        }
        return (json!({"status": "clean"}), 0);
    }

    // Step 4: Post-deps CI. Reaching this point means dependencies were
    // updated (the deps_error, deps_skipped, and deps_no_changes branches
    // all returned early above).
    let post_ci_args = ci::Args {
        force: false,
        retry: 3,
        branch: Some(base_branch.clone()),
        simulate_branch: None,
        format: false,
        lint: false,
        build: false,
        test: false,
        audit: false,
        clean: false,
        trailing: Vec::new(),
    };
    let (post_ci_result, _post_ci_code) = ci::run_impl(&post_ci_args, cwd, root, false);
    let _ = append_log(
        root,
        branch,
        &format!(
            "[Phase 1] start-gate — post-deps CI ({})",
            post_ci_result["status"]
        ),
    );

    if post_ci_result["status"] == "error" {
        if post_ci_result
            .get("consistent")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return (
                json!({
                    "status": "deps_ci_failed",
                    "output": post_ci_result["output"],
                    "attempts": post_ci_result["attempts"],
                }),
                0,
            );
        }
        return (
            json!({
                "status": "error",
                "message": post_ci_result["message"],
                "step": "ci_post_deps",
            }),
            0,
        );
    }

    // Check for flaky post-deps
    if post_ci_result
        .get("flaky")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        // Post-deps flaky overrides baseline flaky context
        flaky_info = Some(json!({
            "first_failure_output": post_ci_result["first_failure_output"],
            "attempts": post_ci_result["attempts"],
            "flaky_context": "CI post-deps gate during flow-start after dependency update",
        }));
    }

    // Commit dependency changes to the integration branch while holding
    // the start lock.
    if let Err(e) = commit_deps(cwd, &base_branch) {
        let _ = append_log(
            root,
            branch,
            &format!("[Phase 1] start-gate — commit deps (error: {})", e),
        );
        return (
            json!({
                "status": "error",
                "message": format!("Failed to commit dependency update: {}", e),
                "step": "commit_deps",
            }),
            0,
        );
    }
    let _ = append_log(root, branch, "[Phase 1] start-gate — commit deps (ok)");

    // Build response
    let mut response = json!({
        "status": "clean",
        "deps_changed": true,
    });

    if let Some(info) = flaky_info {
        response["status"] = json!("ci_flaky");
        response["first_failure_output"] = info["first_failure_output"].clone();
        response["attempts"] = info["attempts"].clone();
        response["flaky_context"] = info["flaky_context"].clone();
    }

    (response, 0)
}

/// Commit dependency changes to the integration branch and push.
///
/// Runs `git add -A` → `git commit` → `git push origin <base_branch>`.
/// Called after deps changed and post-deps CI passed. Must only be
/// called while the start lock is held — this serializes all
/// integration-branch mutations per the concurrency model. Returns `Err`
/// if any git command fails (including "nothing to commit").
fn commit_deps(cwd: &Path, base_branch: &str) -> Result<(), String> {
    // Spawning `git` cannot fail in practice on any supported target
    // — `git` is always on PATH and `Command::output()` only returns
    // Err when the binary cannot be executed at all. A failure there
    // is a programmer-visible panic rather than a silent skip.
    //
    // `git add -A` also cannot fail in normal repo states — we are
    // inside the deps-changed branch (post-deps CI already ran git
    // commands against this repo), and `git add -A` succeeds even
    // when there is nothing to add. Its result is discarded; any
    // subsequent commit failure surfaces the real error via the
    // git commit exit check below.
    let _ = Command::new("git")
        .args(["add", "-A"])
        .current_dir(cwd)
        .output()
        .expect("git add -A spawn");

    let commit = Command::new("git")
        .args(["commit", "-m", "Update dependencies"])
        .current_dir(cwd)
        .output()
        .expect("git commit spawn");
    if !commit.status.success() {
        return Err(format!(
            "git commit: {}",
            String::from_utf8_lossy(&commit.stderr).trim()
        ));
    }

    let push = Command::new("git")
        .args(["push", "origin", base_branch])
        .current_dir(cwd)
        .output()
        .expect("git push spawn");
    if !push.status.success() {
        return Err(format!(
            "git push: {}",
            String::from_utf8_lossy(&push.stderr).trim()
        ));
    }

    Ok(())
}

/// Run `git pull origin <base_branch>`.
fn git_pull(cwd: &Path, base_branch: &str) -> Result<(), String> {
    // Spawning `git` and waiting for it cannot fail in practice on
    // any supported target.
    let child = Command::new("git")
        .args(["pull", "origin", base_branch])
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("git pull spawn");

    let output = child.wait_with_output().expect("git pull wait_with_output");

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(stderr.trim().to_string())
    }
}
