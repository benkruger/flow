//! Consolidated start-gate: git pull + CI baseline (retry 3) + update-deps +
//! post-deps CI (retry 3 if deps changed) in a single command.
//!
//! Returns JSON with status:
//! - "clean" — all gates passed (may include deps_changed, flaky info)
//! - "ci_flaky" — CI was flaky (baseline or post-deps), includes filing context
//! - "ci_failed" — consistent CI failure on baseline (lock held)
//! - "deps_ci_failed" — consistent CI failure after dep update (lock held)
//! - "error" — infrastructure failure (pull failed, deps error)
//!
//! # Dependency-injected core
//!
//! [`run_impl_with_deps`] is the fully-testable core: it accepts the
//! project root and cwd as `&Path` parameters and the git-pull,
//! CI-runner, deps-runner, and commit-deps steps as injectable
//! closures. Integration tests exercise every branch against a
//! `TempDir` fixture with stub closures, so the non-consistent CI
//! error paths and `commit_deps` failure path are testable without
//! spawning git or CI. Production [`run_impl`] binds the closures to
//! [`git_pull`], [`ci::run_impl`], [`run_update_deps`], and
//! [`commit_deps`].

use std::path::Path;

use clap::Parser;
use serde_json::{json, Value};

use crate::ci;
use crate::commands::log::append_log;
use crate::commands::start_step::update_step;
use crate::flow_paths::FlowPaths;
use crate::update_deps::run_update_deps;

const DEPS_TIMEOUT_SECS: u64 = 300;

#[derive(Parser, Debug)]
#[command(name = "start-gate", about = "Consolidated CI and dependency gate")]
pub struct Args {
    /// Branch name for state file lookup and logging
    #[arg(long)]
    pub branch: String,
}

/// Testable core with injected project root, cwd, and subprocess
/// steps. Production [`run_impl`] binds the closures to
/// [`git_pull`], [`ci::run_impl`], [`run_update_deps`], and
/// [`commit_deps`]. Tests inject stubs returning canned values.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_impl_with_deps(
    args: &Args,
    root: &Path,
    cwd: &Path,
    git_pull_fn: &dyn Fn(&Path) -> Result<(), String>,
    ci_runner: &dyn Fn(&ci::Args, &Path, &Path, bool) -> (Value, i32),
    deps_runner: &dyn Fn(&Path, u64) -> (Value, i32),
    commit_deps_fn: &dyn Fn(&Path) -> Result<(), String>,
) -> Value {
    let branch = &args.branch;

    // Update TUI step counter
    let state_path = FlowPaths::new(root, branch).state_file();
    update_step(&state_path, 2);

    // Step 1: git pull origin main
    let pull_result = git_pull_fn(cwd);
    let _ = append_log(
        root,
        branch,
        &format!(
            "[Phase 1] start-gate — git pull ({})",
            if pull_result.is_ok() { "ok" } else { "error" }
        ),
    );
    if let Err(msg) = pull_result {
        return json!({
            "status": "error",
            "message": format!("git pull failed: {}", msg),
            "step": "git_pull",
        });
    }

    // Step 2: CI baseline with retry
    let ci_args = ci::Args {
        force: false,
        retry: 3,
        branch: Some("main".to_string()),
        simulate_branch: None,
        format: false,
        lint: false,
        build: false,
        test: false,
        audit: false,
        clean: false,
        trailing: Vec::new(),
    };
    let (ci_result, _ci_code) = ci_runner(&ci_args, cwd, root, false);
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
            return json!({
                "status": "ci_failed",
                "output": ci_result["output"],
                "attempts": ci_result["attempts"],
            });
        }
        return json!({
            "status": "error",
            "message": ci_result["message"],
            "step": "ci_baseline",
        });
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
    let (deps_result, _deps_code) = deps_runner(cwd, DEPS_TIMEOUT_SECS);
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
        return json!({
            "status": "error",
            "message": deps_result["message"],
            "step": "update_deps",
        });
    }

    if deps_skipped || deps_no_changes {
        // No dep changes — return clean (with flaky info if applicable)
        if let Some(info) = flaky_info {
            return json!({
                "status": "ci_flaky",
                "first_failure_output": info["first_failure_output"],
                "attempts": info["attempts"],
                "flaky_context": info["flaky_context"],
            });
        }
        return json!({"status": "clean"});
    }

    // Step 4: Post-deps CI. Reaching this point means dependencies were
    // updated (the deps_error, deps_skipped, and deps_no_changes branches
    // all returned early above).
    let post_ci_args = ci::Args {
        force: false,
        retry: 3,
        branch: Some("main".to_string()),
        simulate_branch: None,
        format: false,
        lint: false,
        build: false,
        test: false,
        audit: false,
        clean: false,
        trailing: Vec::new(),
    };
    let (post_ci_result, _post_ci_code) = ci_runner(&post_ci_args, cwd, root, false);
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
            return json!({
                "status": "deps_ci_failed",
                "output": post_ci_result["output"],
                "attempts": post_ci_result["attempts"],
            });
        }
        return json!({
            "status": "error",
            "message": post_ci_result["message"],
            "step": "ci_post_deps",
        });
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

    // Commit dependency changes to main while holding the start lock
    if let Err(e) = commit_deps_fn(cwd) {
        let _ = append_log(
            root,
            branch,
            &format!("[Phase 1] start-gate — commit deps (error: {})", e),
        );
        return json!({
            "status": "error",
            "message": format!("Failed to commit dependency update: {}", e),
            "step": "commit_deps",
        });
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

    response
}

/// Main-arm entry point: returns the `(Value, i32)` contract that
/// `dispatch::dispatch_json` consumes. Takes `root: &Path` and
/// `cwd: &Path` per `.claude/rules/rust-patterns.md` "Main-arm
/// dispatch" so integration tests can pass a `TempDir` fixture
/// instead of the host `project_root()`/`current_dir()`.
/// `run_impl_with_deps` always returns `Value` — business errors
/// appear in the `status: "error"` payload with exit code `0`.
pub fn run_impl_main(args: &Args, root: &Path, cwd: &Path) -> (Value, i32) {
    (
        run_impl_with_deps(
            args,
            root,
            cwd,
            &git_pull,
            &ci::run_impl,
            &run_update_deps,
            &commit_deps,
        ),
        0,
    )
}

/// Commit dependency changes to main and push.
///
/// Runs `git add -A` → `git commit` → `git push origin main`.
/// Called after deps changed and post-deps CI passed. Must only be
/// called while the start lock is held — this serializes all
/// main-branch mutations per the concurrency model. Returns `Err`
/// if any git command fails (including "nothing to commit").
pub fn commit_deps(cwd: &Path) -> Result<(), String> {
    // Spawning `git` cannot fail in practice on any supported target
    // — `git` is always on PATH and `Command::output()` only returns
    // Err when the binary cannot be executed at all. A failure there
    // is a programmer-visible panic rather than a silent skip.
    let add = std::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(cwd)
        .output()
        .expect("git add -A spawn");
    if !add.status.success() {
        return Err(format!(
            "git add: {}",
            String::from_utf8_lossy(&add.stderr).trim()
        ));
    }

    let commit = std::process::Command::new("git")
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

    let push = std::process::Command::new("git")
        .args(["push", "origin", "main"])
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

/// Run `git pull origin main`.
pub fn git_pull(cwd: &Path) -> Result<(), String> {
    // Spawning `git` and waiting for it cannot fail in practice on
    // any supported target.
    let child = std::process::Command::new("git")
        .args(["pull", "origin", "main"])
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
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
