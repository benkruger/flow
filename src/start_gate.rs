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
//! closures. Inline tests exercise every branch against a `TempDir`
//! fixture with stub closures, so the non-consistent CI error paths
//! and `commit_deps` failure path are testable without spawning git
//! or CI. Production [`run_impl`] binds the closures to
//! [`git_pull`], [`ci::run_impl`], [`run_update_deps`], and
//! [`commit_deps`].

use std::path::{Path, PathBuf};

use clap::Parser;
use serde_json::{json, Value};

use crate::ci;
use crate::commands::log::append_log;
use crate::commands::start_step::update_step;
use crate::flow_paths::FlowPaths;
use crate::git::project_root;
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

/// Production entry point: binds [`run_impl_with_deps`] to the real
/// git, CI, deps-update, and commit-deps subprocess runners, using
/// [`project_root`] and `current_dir()` for the root and cwd.
pub fn run_impl(args: &Args) -> Value {
    let root = project_root();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    run_impl_with_deps(
        args,
        &root,
        &cwd,
        &git_pull,
        &ci::run_impl,
        &run_update_deps,
        &commit_deps,
    )
}

/// Main-arm entry point: returns the `(Value, i32)` contract that
/// `dispatch::dispatch_json` consumes. Takes `root: &Path` and
/// `cwd: &Path` per `.claude/rules/rust-patterns.md` "Main-arm
/// dispatch" so inline tests can pass a `TempDir` fixture instead of
/// the host `project_root()`/`current_dir()`. `run_impl_with_deps`
/// always returns `Value` — business errors appear in the
/// `status: "error"` payload with exit code `0`.
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

    #[test]
    fn commit_deps_git_push_failure() {
        // Exercises the git push error path in commit_deps (lines 282-293).
        // Create a repo, stage+commit changes, then delete the remote
        // so push fails.
        let dir = tempfile::tempdir().unwrap();
        let (repo, bare) = create_repo_with_remote(dir.path());

        // Write a file so add + commit succeed
        fs::write(repo.join("Cargo.lock"), "updated").unwrap();

        // Remove the bare remote so push fails
        fs::remove_dir_all(&bare).unwrap();

        let result = commit_deps(&repo);
        assert!(result.is_err(), "commit_deps should fail on push");
        let err = result.unwrap_err();
        assert!(
            err.contains("git push"),
            "error should mention git push, got: {}",
            err
        );
    }

    // --- run_impl_with_deps ---

    /// Seed a minimal state file so `update_step` has a target.
    fn seed_state(root: &Path, branch: &str) {
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(
            state_dir.join(format!("{}.json", branch)),
            r#"{"schema_version":1,"branch":"demo"}"#,
        )
        .unwrap();
    }

    fn ok_ci(_args: &ci::Args, _cwd: &Path, _root: &Path, _force: bool) -> (Value, i32) {
        (json!({"status": "ok"}), 0)
    }

    fn err_ci_non_consistent(
        _args: &ci::Args,
        _cwd: &Path,
        _root: &Path,
        _force: bool,
    ) -> (Value, i32) {
        // status=error without consistent=true → non-consistent branch
        (
            json!({"status": "error", "message": "CI failed with transient error"}),
            1,
        )
    }

    fn deps_no_changes_ok(_cwd: &Path, _timeout: u64) -> (Value, i32) {
        (json!({"status": "ok", "changes": false}), 0)
    }

    fn deps_changed_ok(_cwd: &Path, _timeout: u64) -> (Value, i32) {
        (json!({"status": "ok", "changes": true}), 0)
    }

    fn commit_ok(_cwd: &Path) -> Result<(), String> {
        Ok(())
    }

    #[test]
    fn start_gate_pull_error_returns_infrastructure_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        seed_state(&root, "pull-err-branch");
        let args = Args {
            branch: "pull-err-branch".to_string(),
        };
        let pull_err = |_: &Path| -> Result<(), String> { Err("remote unreachable".to_string()) };

        let result = run_impl_with_deps(
            &args,
            &root,
            &root,
            &pull_err,
            &ok_ci,
            &deps_no_changes_ok,
            &commit_ok,
        );
        assert_eq!(result["status"], "error");
        assert_eq!(result["step"], "git_pull");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("remote unreachable"));
    }

    #[test]
    fn start_gate_baseline_ci_non_consistent_error_returns_infrastructure_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        seed_state(&root, "ci-err-branch");
        let args = Args {
            branch: "ci-err-branch".to_string(),
        };
        let pull_ok = |_: &Path| -> Result<(), String> { Ok(()) };

        let result = run_impl_with_deps(
            &args,
            &root,
            &root,
            &pull_ok,
            &err_ci_non_consistent,
            &deps_no_changes_ok,
            &commit_ok,
        );
        assert_eq!(result["status"], "error");
        assert_eq!(result["step"], "ci_baseline");
    }

    #[test]
    fn start_gate_post_ci_non_consistent_error_returns_infrastructure_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        seed_state(&root, "post-ci-err-branch");
        let args = Args {
            branch: "post-ci-err-branch".to_string(),
        };
        let pull_ok = |_: &Path| -> Result<(), String> { Ok(()) };
        // Baseline passes, post-deps CI returns non-consistent error.
        let calls = std::cell::RefCell::new(0usize);
        let two_phase_ci =
            |args: &ci::Args, cwd: &Path, root: &Path, force: bool| -> (Value, i32) {
                *calls.borrow_mut() += 1;
                if *calls.borrow() == 1 {
                    ok_ci(args, cwd, root, force)
                } else {
                    err_ci_non_consistent(args, cwd, root, force)
                }
            };

        let result = run_impl_with_deps(
            &args,
            &root,
            &root,
            &pull_ok,
            &two_phase_ci,
            &deps_changed_ok,
            &commit_ok,
        );
        assert_eq!(result["status"], "error");
        assert_eq!(result["step"], "ci_post_deps");
        assert_eq!(*calls.borrow(), 2, "CI runner must be called twice");
    }

    #[test]
    fn start_gate_commit_deps_failure_returns_commit_deps_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        seed_state(&root, "commit-fail-branch");
        let args = Args {
            branch: "commit-fail-branch".to_string(),
        };
        let pull_ok = |_: &Path| -> Result<(), String> { Ok(()) };
        let commit_err =
            |_: &Path| -> Result<(), String> { Err("remote rejected push".to_string()) };

        let result = run_impl_with_deps(
            &args,
            &root,
            &root,
            &pull_ok,
            &ok_ci,
            &deps_changed_ok,
            &commit_err,
        );
        assert_eq!(result["status"], "error");
        assert_eq!(result["step"], "commit_deps");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("remote rejected push"));
    }

    #[test]
    fn start_gate_deps_changed_passes_returns_clean() {
        // Sanity: happy path via the seam (covers the final Ok branch
        // with deps_changed: true).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        seed_state(&root, "happy-branch");
        let args = Args {
            branch: "happy-branch".to_string(),
        };
        let pull_ok = |_: &Path| -> Result<(), String> { Ok(()) };

        let result = run_impl_with_deps(
            &args,
            &root,
            &root,
            &pull_ok,
            &ok_ci,
            &deps_changed_ok,
            &commit_ok,
        );
        assert_eq!(result["status"], "clean");
        assert_eq!(result["deps_changed"], true);
    }

    #[test]
    fn start_gate_baseline_flaky_then_deps_no_changes_returns_ci_flaky() {
        // Covers the flaky-baseline + no-dep-changes branch combination.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        seed_state(&root, "flaky-branch");
        let args = Args {
            branch: "flaky-branch".to_string(),
        };
        let pull_ok = |_: &Path| -> Result<(), String> { Ok(()) };
        let flaky_ci = |_: &ci::Args, _: &Path, _: &Path, _: bool| -> (Value, i32) {
            (
                json!({
                    "status": "ok",
                    "flaky": true,
                    "first_failure_output": "transient timeout",
                    "attempts": 2,
                }),
                0,
            )
        };

        let result = run_impl_with_deps(
            &args,
            &root,
            &root,
            &pull_ok,
            &flaky_ci,
            &deps_no_changes_ok,
            &commit_ok,
        );
        assert_eq!(result["status"], "ci_flaky");
        assert_eq!(
            result["flaky_context"],
            "CI baseline on pristine main during flow-start"
        );
    }

    #[test]
    fn start_gate_deps_error_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        seed_state(&root, "deps-err-branch");
        let args = Args {
            branch: "deps-err-branch".to_string(),
        };
        let pull_ok = |_: &Path| -> Result<(), String> { Ok(()) };
        let deps_error = |_: &Path, _: u64| -> (Value, i32) {
            (
                json!({"status": "error", "message": "deps subprocess died"}),
                1,
            )
        };

        let result = run_impl_with_deps(
            &args,
            &root,
            &root,
            &pull_ok,
            &ok_ci,
            &deps_error,
            &commit_ok,
        );
        assert_eq!(result["status"], "error");
        assert_eq!(result["step"], "update_deps");
    }

    #[test]
    fn start_gate_ci_consistent_fail_returns_ci_failed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        seed_state(&root, "consist-fail-branch");
        let args = Args {
            branch: "consist-fail-branch".to_string(),
        };
        let pull_ok = |_: &Path| -> Result<(), String> { Ok(()) };
        let consist_fail = |_: &ci::Args, _: &Path, _: &Path, _: bool| -> (Value, i32) {
            (
                json!({
                    "status": "error",
                    "consistent": true,
                    "output": "final failure",
                    "attempts": 3,
                }),
                1,
            )
        };

        let result = run_impl_with_deps(
            &args,
            &root,
            &root,
            &pull_ok,
            &consist_fail,
            &deps_no_changes_ok,
            &commit_ok,
        );
        assert_eq!(result["status"], "ci_failed");
        assert_eq!(result["output"], "final failure");
    }

    #[test]
    fn start_gate_post_ci_consistent_fail_returns_deps_ci_failed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        seed_state(&root, "deps-consist-fail-branch");
        let args = Args {
            branch: "deps-consist-fail-branch".to_string(),
        };
        let pull_ok = |_: &Path| -> Result<(), String> { Ok(()) };
        let calls = std::cell::RefCell::new(0usize);
        let two_phase = |args: &ci::Args, cwd: &Path, root: &Path, force: bool| -> (Value, i32) {
            *calls.borrow_mut() += 1;
            if *calls.borrow() == 1 {
                ok_ci(args, cwd, root, force)
            } else {
                (
                    json!({
                        "status": "error",
                        "consistent": true,
                        "output": "post-deps fail",
                        "attempts": 3,
                    }),
                    1,
                )
            }
        };

        let result = run_impl_with_deps(
            &args,
            &root,
            &root,
            &pull_ok,
            &two_phase,
            &deps_changed_ok,
            &commit_ok,
        );
        assert_eq!(result["status"], "deps_ci_failed");
    }

    #[test]
    fn start_gate_post_ci_flaky_returns_ci_flaky_with_deps_context() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        seed_state(&root, "post-flaky-branch");
        let args = Args {
            branch: "post-flaky-branch".to_string(),
        };
        let pull_ok = |_: &Path| -> Result<(), String> { Ok(()) };
        let calls = std::cell::RefCell::new(0usize);
        let two_phase = |args: &ci::Args, cwd: &Path, root: &Path, force: bool| -> (Value, i32) {
            *calls.borrow_mut() += 1;
            if *calls.borrow() == 1 {
                ok_ci(args, cwd, root, force)
            } else {
                (
                    json!({
                        "status": "ok",
                        "flaky": true,
                        "first_failure_output": "post-deps transient",
                        "attempts": 2,
                    }),
                    0,
                )
            }
        };

        let result = run_impl_with_deps(
            &args,
            &root,
            &root,
            &pull_ok,
            &two_phase,
            &deps_changed_ok,
            &commit_ok,
        );
        assert_eq!(result["status"], "ci_flaky");
        assert_eq!(
            result["flaky_context"],
            "CI post-deps gate during flow-start after dependency update"
        );
    }

    // --- run_impl_main ---

    #[test]
    fn start_gate_run_impl_main_err_path() {
        // Drive the git-pull-error scenario through run_impl_main
        // against a TempDir. run_impl_main calls run_impl_with_deps
        // bound to the real git_pull, ci::run_impl, run_update_deps,
        // and commit_deps — in this test the tempdir is not a git
        // repo so real git_pull returns Err, which run_impl_with_deps
        // propagates as a status:"error" step:"git_pull" payload.
        // run_impl_main wraps with exit 0 per the business-error
        // convention.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        seed_state(&root, "main-err-branch");
        let args = Args {
            branch: "main-err-branch".to_string(),
        };
        let (v, code) = run_impl_main(&args, &root, &root);
        assert_eq!(code, 0, "exit code is 0 for business errors");
        assert_eq!(v["status"], "error");
        assert_eq!(v["step"], "git_pull");
    }
}
