//! Consolidated start-gate: git pull + CI baseline (retry 3) + update-deps +
//! post-deps CI (retry 3 if deps changed) in a single command.
//!
//! Returns JSON with status:
//! - "clean" — all gates passed (may include deps_changed, flaky info)
//! - "ci_flaky" — CI was flaky (baseline or post-deps), includes filing context
//! - "ci_failed" — consistent CI failure on baseline (lock held)
//! - "deps_ci_failed" — consistent CI failure after dep update (lock held)
//! - "error" — infrastructure failure (pull failed, deps error)

use std::path::{Path, PathBuf};
use std::process;

use clap::Parser;
use serde_json::{json, Value};

use crate::ci;
use crate::commands::log::append_log;
use crate::commands::start_step::update_step;
use crate::git::project_root;
use crate::output::json_error;
use crate::update_deps::run_update_deps;

const DEPS_TIMEOUT_SECS: u64 = 300;

#[derive(Parser, Debug)]
#[command(name = "start-gate", about = "Consolidated CI and dependency gate")]
pub struct Args {
    /// Branch name for state file lookup and logging
    #[arg(long)]
    pub branch: String,
}

/// Testable entry point.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let root = project_root();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let branch = &args.branch;

    // Update TUI step counter
    let state_path = root.join(".flow-states").join(format!("{}.json", branch));
    update_step(&state_path, 2);

    // Step 1: git pull origin main
    let pull_result = git_pull(&cwd);
    let _ = append_log(
        &root,
        branch,
        &format!(
            "[Phase 1] start-gate — git pull ({})",
            if pull_result.is_ok() { "ok" } else { "error" }
        ),
    );
    if let Err(msg) = pull_result {
        return Ok(json!({
            "status": "error",
            "message": format!("git pull failed: {}", msg),
            "step": "git_pull",
        }));
    }

    // Step 2: CI baseline with retry
    let ci_args = ci::Args {
        force: false,
        retry: 3,
        branch: Some("main".to_string()),
        simulate_branch: None,
    };
    let (ci_result, _ci_code) = ci::run_impl(&ci_args, &cwd, &root, false);
    let _ = append_log(
        &root,
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
            return Ok(json!({
                "status": "ci_failed",
                "output": ci_result["output"],
                "attempts": ci_result["attempts"],
            }));
        }
        return Ok(json!({
            "status": "error",
            "message": ci_result["message"],
            "step": "ci_baseline",
        }));
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
    let (deps_result, _deps_code) = run_update_deps(&cwd, DEPS_TIMEOUT_SECS);
    let _ = append_log(
        &root,
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
    let deps_changed = deps_result["status"] == "ok"
        && deps_result
            .get("changes")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
    let deps_error = deps_result["status"] == "error";

    if deps_error {
        return Ok(json!({
            "status": "error",
            "message": deps_result["message"],
            "step": "update_deps",
        }));
    }

    if deps_skipped || deps_no_changes {
        // No dep changes — return clean (with flaky info if applicable)
        if let Some(info) = flaky_info {
            return Ok(json!({
                "status": "ci_flaky",
                "first_failure_output": info["first_failure_output"],
                "attempts": info["attempts"],
                "flaky_context": info["flaky_context"],
            }));
        }
        return Ok(json!({"status": "clean"}));
    }

    // Step 4: Post-deps CI (only if deps changed)
    if !deps_changed {
        return Ok(json!({
            "status": "error",
            "message": format!("Unexpected deps status: {}", deps_result["status"]),
            "step": "update_deps",
        }));
    }
    let post_ci_args = ci::Args {
        force: false,
        retry: 3,
        branch: Some("main".to_string()),
        simulate_branch: None,
    };
    let (post_ci_result, _post_ci_code) = ci::run_impl(&post_ci_args, &cwd, &root, false);
    let _ = append_log(
        &root,
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
            return Ok(json!({
                "status": "deps_ci_failed",
                "output": post_ci_result["output"],
                "attempts": post_ci_result["attempts"],
            }));
        }
        return Ok(json!({
            "status": "error",
            "message": post_ci_result["message"],
            "step": "ci_post_deps",
        }));
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
    if let Err(e) = commit_deps(&cwd) {
        let _ = append_log(
            &root,
            branch,
            &format!("[Phase 1] start-gate — commit deps (error: {})", e),
        );
        return Ok(json!({
            "status": "error",
            "message": format!("Failed to commit dependency update: {}", e),
            "step": "commit_deps",
        }));
    }
    let _ = append_log(&root, branch, "[Phase 1] start-gate — commit deps (ok)");

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

    Ok(response)
}

/// Commit dependency changes to main and push.
///
/// Runs `git add -A` → `git commit` → `git push origin main`.
/// Called after deps changed and post-deps CI passed. Must only be
/// called while the start lock is held — this serializes all
/// main-branch mutations per the concurrency model. Returns `Err`
/// if any git command fails (including "nothing to commit").
fn commit_deps(cwd: &Path) -> Result<(), String> {
    // Stage all changes left by bin/dependencies
    let add = std::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("git add: {}", e))?;
    if !add.status.success() {
        return Err(format!(
            "git add: {}",
            String::from_utf8_lossy(&add.stderr).trim()
        ));
    }

    // Commit
    let commit = std::process::Command::new("git")
        .args(["commit", "-m", "Update dependencies"])
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("git commit: {}", e))?;
    if !commit.status.success() {
        return Err(format!(
            "git commit: {}",
            String::from_utf8_lossy(&commit.stderr).trim()
        ));
    }

    // Push to remote
    let push = std::process::Command::new("git")
        .args(["push", "origin", "main"])
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("git push: {}", e))?;
    if !push.status.success() {
        return Err(format!(
            "git push: {}",
            String::from_utf8_lossy(&push.stderr).trim()
        ));
    }

    Ok(())
}

/// Run `git pull origin main` with a timeout.
fn git_pull(cwd: &Path) -> Result<(), String> {
    let child = std::process::Command::new("git")
        .args(["pull", "origin", "main"])
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn git pull: {}", e))?;

    let output = child
        .wait_with_output()
        .map_err(|e| format!("git pull wait failed: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(stderr.trim().to_string())
    }
}

/// CLI entry point.
pub fn run(args: Args) {
    match run_impl(&args) {
        Ok(result) => {
            println!("{}", serde_json::to_string(&result).unwrap());
        }
        Err(e) => {
            json_error(&e, &[]);
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    /// Create a git repo with a bare remote and push initial commit.
    fn create_repo_with_remote(parent: &Path) -> (PathBuf, PathBuf) {
        let bare = parent.join("bare.git");
        let repo = parent.join("repo");

        Command::new("git")
            .args(["init", "--bare", "-b", "main", &bare.to_string_lossy()])
            .output()
            .unwrap();

        Command::new("git")
            .args(["clone", &bare.to_string_lossy(), &repo.to_string_lossy()])
            .output()
            .unwrap();

        for (key, val) in [
            ("user.email", "test@test.com"),
            ("user.name", "Test"),
            ("commit.gpgsign", "false"),
        ] {
            Command::new("git")
                .args(["config", key, val])
                .current_dir(&repo)
                .output()
                .unwrap();
        }

        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(&repo)
            .output()
            .unwrap();

        Command::new("git")
            .args(["push", "-u", "origin", "main"])
            .current_dir(&repo)
            .output()
            .unwrap();

        (repo, bare)
    }

    #[test]
    fn commit_deps_commits_and_pushes() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, bare) = create_repo_with_remote(dir.path());

        // Simulate a dep update leaving a dirty file
        fs::write(repo.join("Cargo.lock"), "updated-lock-content").unwrap();

        // Commit the dep changes
        commit_deps(&repo).expect("commit_deps should succeed");

        // Verify: file is committed on main
        let log_output = Command::new("git")
            .args(["log", "--oneline", "-1", "--format=%s"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let msg = String::from_utf8_lossy(&log_output.stdout)
            .trim()
            .to_string();
        assert_eq!(msg, "Update dependencies");

        // Verify: Cargo.lock is tracked
        let show_output = Command::new("git")
            .args(["show", "HEAD:Cargo.lock"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(show_output.status.success());
        let content = String::from_utf8_lossy(&show_output.stdout);
        assert_eq!(content.trim(), "updated-lock-content");

        // Verify: pushed to remote
        let remote_log = Command::new("git")
            .args(["log", "--oneline", "-1", "--format=%s"])
            .current_dir(&bare)
            .output()
            .unwrap();
        let remote_msg = String::from_utf8_lossy(&remote_log.stdout)
            .trim()
            .to_string();
        assert_eq!(remote_msg, "Update dependencies");
    }

    #[test]
    fn commit_deps_error_on_nothing_to_commit() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, _bare) = create_repo_with_remote(dir.path());

        // No changes — commit should fail
        let result = commit_deps(&repo);
        assert!(
            result.is_err(),
            "commit_deps should fail with nothing to commit"
        );
    }
}
