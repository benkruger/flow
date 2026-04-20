//! Integration tests for `bin/flow cleanup`.
//!
//! Drives the compiled binary against a minimal project fixture so the
//! `run()` entry point and its dispatch into `cleanup::run_impl` are
//! exercised end-to-end. Matches the subprocess-hygiene pattern used in
//! `tests/main_dispatch.rs` — `FLOW_CI_RUNNING` is unset, `GH_TOKEN` is
//! invalidated, and `HOME` is pinned to the test tempdir.
//!
//! Also drives the public library surface of `flow_rs::cleanup` for the
//! unit-test paths migrated from the pre-existing inline `#[cfg(test)]`
//! module in `src/cleanup.rs` per `.claude/rules/test-placement.md`.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::process::Command as StdCommand;
use std::time::Duration;

use flow_rs::cleanup::{
    cleanup, label_result, run_cmd, run_cmd_with_timeout, run_impl_main,
    try_delete_adversarial_test_files, try_delete_file, Args,
};
use serde_json::json;

fn flow_rs_no_recursion() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd
}

/// `flow-rs cleanup <nonexistent-root>` passes Clap but fails the
/// existence check in `cleanup::run_impl` — the `run()` wrapper wraps
/// the error via `json_error` and exits 1.
#[test]
fn cleanup_nonexistent_root_exits_1() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let output = flow_rs_no_recursion()
        .args([
            "cleanup",
            "/nonexistent/path/does/not/exist",
            "--branch",
            "test-branch",
            "--worktree",
            ".worktrees/test-branch",
        ])
        .env("GH_TOKEN", "invalid")
        .env("HOME", &root)
        .output()
        .expect("spawn flow-rs cleanup");
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":\"error\""),
        "expected structured error in stdout, got: {}",
        stdout
    );
}

/// `flow-rs cleanup --help` covers the Args clap parser and help path.
#[test]
fn cleanup_help_exits_0() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let output = flow_rs_no_recursion()
        .args(["cleanup", "--help"])
        .env("GH_TOKEN", "invalid")
        .env("HOME", &root)
        .output()
        .expect("spawn flow-rs cleanup --help");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Usage:"),
        "expected Usage: header in --help output, got: {}",
        stdout
    );
}

/// `flow-rs cleanup` missing required args fails Clap parsing.
#[test]
fn cleanup_missing_args_exits_nonzero() {
    let output = flow_rs_no_recursion()
        .arg("cleanup")
        .output()
        .expect("spawn flow-rs cleanup");
    assert_ne!(
        output.status.code(),
        Some(0),
        "cleanup with no project root should reject, got: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `flow-rs cleanup` in a valid tempdir without a .flow-states directory
/// is a no-op cleanup path — the command must not panic and should
/// report structured JSON on stdout.
#[test]
fn cleanup_empty_tempdir_does_not_panic() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");

    let output = flow_rs_no_recursion()
        .args([
            "cleanup",
            root.to_str().unwrap(),
            "--branch",
            "no-such-branch",
            "--worktree",
            ".worktrees/no-such-branch",
        ])
        .env("GIT_CEILING_DIRECTORIES", &root)
        .env("GH_TOKEN", "invalid")
        .env("HOME", &root)
        .output()
        .expect("spawn flow-rs cleanup");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked at"),
        "cleanup must not panic on empty tempdir, got: {}",
        stderr
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // The command writes structured JSON; we only care that it parses
    // and produces some status (error or ok — both are non-panicking).
    assert!(
        stdout.contains("\"status\":"),
        "expected JSON status in stdout, got: {}",
        stdout
    );
}

// --- Library-level unit tests (migrated from src/cleanup.rs) ---

/// Create a minimal git repo for testing.
fn setup_git_repo(dir: &Path) {
    StdCommand::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .unwrap();
    // Configure identity for CI environments without global git config
    let config_path = dir.join(".git").join("config");
    fs::write(
        &config_path,
        "[user]\n\temail = t@t.com\n\tname = T\n[commit]\n\tgpgsign = false\n",
    )
    .unwrap();
    StdCommand::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir)
        .output()
        .unwrap();
}

/// Create a worktree and state file for testing cleanup.
fn setup_feature(git_repo: &Path, branch: &str) -> String {
    let wt_rel = format!(".worktrees/{}", branch);
    StdCommand::new("git")
        .args(["worktree", "add", &wt_rel, "-b", branch])
        .current_dir(git_repo)
        .output()
        .unwrap();

    // Create state file
    let state_dir = git_repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join(format!("{}.json", branch)),
        json!({"branch": branch}).to_string(),
    )
    .unwrap();

    // Create log file
    fs::write(state_dir.join(format!("{}.log", branch)), "test log\n").unwrap();

    wt_rel
}

// --- Cleanup removes worktree ---

#[test]
fn test_cleanup_removes_worktree() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["worktree"], "removed");
    assert!(!dir.path().join(&wt_rel).exists());
}

// --- State file deletion ---

#[test]
fn test_cleanup_deletes_state_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["state_file"], "deleted");
    assert!(!dir.path().join(".flow-states/test-feature.json").exists());
}

// --- Log file deletion ---

#[test]
fn test_cleanup_deletes_log_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["log_file"], "deleted");
    assert!(!dir.path().join(".flow-states/test-feature.log").exists());
}

// --- Plan file ---

#[test]
fn test_cleanup_deletes_plan_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let plan = dir.path().join(".flow-states/test-feature-plan.md");
    fs::write(&plan, "# Plan\n").unwrap();

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["plan_file"], "deleted");
    assert!(!plan.exists());
}

#[test]
fn test_cleanup_skips_missing_plan_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["plan_file"], "skipped");
}

// --- DAG file ---

#[test]
fn test_cleanup_deletes_dag_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let dag = dir.path().join(".flow-states/test-feature-dag.md");
    fs::write(&dag, "# DAG\n").unwrap();

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["dag_file"], "deleted");
    assert!(!dag.exists());
}

#[test]
fn test_cleanup_skips_missing_dag_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["dag_file"], "skipped");
}

// --- Frozen phases file ---

#[test]
fn test_cleanup_deletes_frozen_phases_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let frozen = dir.path().join(".flow-states/test-feature-phases.json");
    fs::write(&frozen, r#"{"phases": {}, "order": []}"#).unwrap();

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["frozen_phases"], "deleted");
    assert!(!frozen.exists());
}

#[test]
fn test_cleanup_skips_missing_frozen_phases() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["frozen_phases"], "skipped");
}

// --- CI sentinel ---

#[test]
fn test_cleanup_deletes_ci_sentinel() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let sentinel = dir.path().join(".flow-states/test-feature-ci-passed");
    fs::write(&sentinel, "snapshot\n").unwrap();

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["ci_sentinel"], "deleted");
    assert!(!sentinel.exists());
}

#[test]
fn test_cleanup_skips_missing_ci_sentinel() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["ci_sentinel"], "skipped");
}

// --- Timings file ---

#[test]
fn test_cleanup_deletes_timings_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let timings = dir.path().join(".flow-states/test-feature-timings.md");
    fs::write(&timings, "| Phase | Duration |\n").unwrap();

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["timings_file"], "deleted");
    assert!(!timings.exists());
}

#[test]
fn test_cleanup_skips_missing_timings_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["timings_file"], "skipped");
}

// --- Closed issues file ---

#[test]
fn test_cleanup_deletes_closed_issues_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let closed = dir
        .path()
        .join(".flow-states/test-feature-closed-issues.json");
    fs::write(&closed, r#"[{"number": 42}]"#).unwrap();

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["closed_issues_file"], "deleted");
    assert!(!closed.exists());
}

#[test]
fn test_cleanup_skips_missing_closed_issues_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["closed_issues_file"], "skipped");
}

// --- Issues file ---

#[test]
fn test_cleanup_deletes_issues_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let issues = dir.path().join(".flow-states/test-feature-issues.md");
    fs::write(&issues, "| Label | Title |\n").unwrap();

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["issues_file"], "deleted");
    assert!(!issues.exists());
}

#[test]
fn test_cleanup_skips_missing_issues_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["issues_file"], "skipped");
}

// --- adversarial_test ---

#[test]
fn test_cleanup_deletes_adversarial_test_rs() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let adv = dir
        .path()
        .join(".flow-states/test-feature-adversarial_test.rs");
    fs::write(&adv, "// adversarial test\n").unwrap();

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["adversarial_test"], "deleted");
    assert!(!adv.exists());
}

#[test]
fn test_cleanup_skips_missing_adversarial_test() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["adversarial_test"], "skipped");
}

#[test]
fn test_cleanup_deletes_adversarial_test_multiple_extensions() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let adv_rs = dir
        .path()
        .join(".flow-states/test-feature-adversarial_test.rs");
    let adv_py = dir
        .path()
        .join(".flow-states/test-feature-adversarial_test.py");
    fs::write(&adv_rs, "// rs\n").unwrap();
    fs::write(&adv_py, "# py\n").unwrap();

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["adversarial_test"], "deleted");
    assert!(!adv_rs.exists());
    assert!(!adv_py.exists());
}

#[test]
fn test_abort_path_deletes_adversarial_test() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let adv = dir
        .path()
        .join(".flow-states/test-feature-adversarial_test.rs");
    fs::write(&adv, "// adversarial\n").unwrap();

    // Abort path: pr_number=Some(...) exercises the remote_branch/pr_close
    // branches alongside the new step, proving the step runs in both the
    // complete (pr_number=None) and abort (pr_number=Some) entry points.
    let steps = cleanup(dir.path(), "test-feature", &wt_rel, Some(999), false);
    assert_eq!(steps["adversarial_test"], "deleted");
    assert!(!adv.exists());
}

#[test]
fn test_cleanup_adversarial_test_respects_branch_prefix() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    // Another concurrent flow has its own adversarial test file in the
    // same shared .flow-states/ directory. Cleanup for "test-feature" must
    // leave it untouched — this is the N×N concurrent-flow safety invariant.
    let other = dir
        .path()
        .join(".flow-states/other-branch-adversarial_test.rs");
    fs::write(&other, "// other branch\n").unwrap();

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["adversarial_test"], "skipped");
    assert!(other.exists());
}

#[test]
fn test_cleanup_adversarial_test_trailing_dot_precision() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    // A file whose name shares the prefix up to `_test` but diverges
    // before the extension dot. The match must use the literal
    // `{branch}-adversarial_test.` (with trailing dot) so this file is
    // NOT matched. Dropping the trailing dot would delete it.
    let other = dir
        .path()
        .join(".flow-states/test-feature-adversarial_test_other.rs");
    fs::write(&other, "// sibling\n").unwrap();

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["adversarial_test"], "skipped");
    assert!(other.exists());
}

#[test]
fn test_cleanup_skips_adversarial_test_when_flow_states_missing() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    // Remove .flow-states/ entirely to exercise the defensive path where
    // fs::read_dir returns Err. The step must return "skipped", not panic.
    fs::remove_dir_all(dir.path().join(".flow-states")).unwrap();

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["adversarial_test"], "skipped");
}

#[test]
fn test_cleanup_adversarial_test_skips_directory_and_deletes_files() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let states = dir.path().join(".flow-states");
    // Directory entry whose name matches the adversarial-test prefix.
    // The helper must skip it without aborting the deletion loop so
    // the real files below still get removed. Created first so it is
    // likely to precede the regular files in read_dir iteration order.
    let bad_dir = states.join("test-feature-adversarial_test.d");
    fs::create_dir_all(&bad_dir).unwrap();
    let adv_rs = states.join("test-feature-adversarial_test.rs");
    let adv_py = states.join("test-feature-adversarial_test.py");
    let adv_go = states.join("test-feature-adversarial_test.go");
    fs::write(&adv_rs, "// rs\n").unwrap();
    fs::write(&adv_py, "# py\n").unwrap();
    fs::write(&adv_go, "// go\n").unwrap();

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["adversarial_test"], "deleted");
    assert!(!adv_rs.exists());
    assert!(!adv_py.exists());
    assert!(!adv_go.exists());
    // The directory matching the prefix must remain — the helper only
    // deletes regular files and symlinks.
    assert!(bad_dir.exists());
}

// --- PR close ---

#[test]
fn test_cleanup_skips_pr_by_default() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["pr_close"], "skipped");
}

#[test]
fn test_abort_pr_close_fails_gracefully() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, Some(999), false);
    assert!(steps["pr_close"].starts_with("failed:"));
}

// --- Branch deletion ---

#[test]
fn test_cleanup_skips_remote_branch_on_complete() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    // Complete path (pr_number=None) skips remote branch deletion
    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["remote_branch"], "skipped");
}

#[test]
fn test_abort_attempts_remote_branch_deletion() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    // Abort path (pr_number=Some) attempts remote branch deletion
    let steps = cleanup(dir.path(), "test-feature", &wt_rel, Some(999), false);
    // No remote configured, so push --delete will fail — but it tried
    assert!(steps["remote_branch"].starts_with("failed:"));
}

#[test]
fn test_cleanup_deletes_local_branch() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    // Remove worktree first so branch can be deleted
    StdCommand::new("git")
        .args(["worktree", "remove", &wt_rel, "--force"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["local_branch"], "deleted");
}

// --- Missing resources ---

#[test]
fn test_cleanup_skips_missing_worktree() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    // Remove worktree before cleanup
    StdCommand::new("git")
        .args(["worktree", "remove", &wt_rel, "--force"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["worktree"], "skipped");
}

#[test]
fn test_cleanup_skips_missing_state_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    fs::remove_file(dir.path().join(".flow-states/test-feature.json")).unwrap();

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["state_file"], "skipped");
}

#[test]
fn test_cleanup_skips_missing_log_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    fs::remove_file(dir.path().join(".flow-states/test-feature.log")).unwrap();

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["log_file"], "skipped");
}

// --- Full happy path ---

#[test]
fn test_cleanup_full_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);

    assert_eq!(steps["pr_close"], "skipped");
    assert_eq!(steps["worktree"], "removed");
    assert_eq!(steps["remote_branch"], "skipped");
    assert_eq!(steps["local_branch"], "deleted");
    assert_eq!(steps["state_file"], "deleted");
    assert_eq!(steps["plan_file"], "skipped");
    assert_eq!(steps["dag_file"], "skipped");
    assert_eq!(steps["log_file"], "deleted");
    assert_eq!(steps["frozen_phases"], "skipped");
    assert_eq!(steps["ci_sentinel"], "skipped");
    assert_eq!(steps["timings_file"], "skipped");
    assert_eq!(steps["closed_issues_file"], "skipped");
    assert_eq!(steps["issues_file"], "skipped");
    assert_eq!(steps["adversarial_test"], "skipped");

    // Filesystem effects
    assert!(!dir.path().join(&wt_rel).exists());
    assert!(!dir.path().join(".flow-states/test-feature.json").exists());
    assert!(!dir.path().join(".flow-states/test-feature.log").exists());
}

// --- tmp/ directory cleanup ---

#[test]
fn test_cleanup_removes_worktree_tmp_in_flow_repo() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    // Mark as FLOW repo
    fs::write(dir.path().join("flow-phases.json"), "{}").unwrap();
    // Create tmp/ inside the worktree
    let wt_tmp = dir.path().join(&wt_rel).join("tmp");
    fs::create_dir_all(&wt_tmp).unwrap();
    fs::write(wt_tmp.join("release-notes-v1.0.md"), "notes").unwrap();

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["worktree_tmp"], "removed");
}

#[test]
fn test_cleanup_skips_tmp_without_flow_phases() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    // No flow-phases.json — not a FLOW repo
    let wt_tmp = dir.path().join(&wt_rel).join("tmp");
    fs::create_dir_all(&wt_tmp).unwrap();
    fs::write(wt_tmp.join("some-file.txt"), "data").unwrap();

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["worktree_tmp"], "skipped");
}

#[test]
fn test_cleanup_skips_missing_worktree_tmp() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    fs::write(dir.path().join("flow-phases.json"), "{}").unwrap();

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert_eq!(steps["worktree_tmp"], "skipped");
}

// --- --pull flag tests ---

#[test]
fn test_no_pull_flag_no_git_pull_step() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    assert!(!steps.contains_key("git_pull"));
}

#[test]
fn test_pull_flag_present_runs_pull() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, true);
    assert!(steps.contains_key("git_pull"));
    // No remote configured, so pull will fail
    assert!(steps["git_pull"].starts_with("failed:"));
}

// --- Step key ordering ---

#[test]
fn test_step_key_order_matches_python() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);
    let keys: Vec<&String> = steps.keys().collect();

    assert_eq!(
        keys,
        vec![
            "pr_close",
            "worktree_tmp",
            "worktree",
            "remote_branch",
            "local_branch",
            "state_file",
            "plan_file",
            "dag_file",
            "log_file",
            "frozen_phases",
            "ci_sentinel",
            "timings_file",
            "closed_issues_file",
            "issues_file",
            "adversarial_test",
        ]
    );
}

#[test]
fn test_step_key_order_with_pull() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, true);
    let keys: Vec<&String> = steps.keys().collect();

    assert_eq!(
        keys,
        vec![
            "pr_close",
            "worktree_tmp",
            "worktree",
            "remote_branch",
            "local_branch",
            "state_file",
            "plan_file",
            "dag_file",
            "log_file",
            "frozen_phases",
            "ci_sentinel",
            "timings_file",
            "closed_issues_file",
            "issues_file",
            "adversarial_test",
            "git_pull",
        ]
    );
}

// --- CLI: invalid project root ---

#[test]
fn test_invalid_project_root() {
    // run() calls process::exit, so we test the logic instead
    let root = Path::new("/nonexistent/path");
    assert!(!root.is_dir());
}

// --- run_cmd error handling ---

#[test]
fn test_run_cmd_nonexistent_command() {
    let dir = tempfile::tempdir().unwrap();
    let (ok, output) = run_cmd(&["nonexistent_command_12345"], dir.path());
    assert!(!ok);
    assert!(!output.is_empty());
}

// --- run_cmd_with_timeout ---

#[test]
fn run_cmd_with_timeout_success() {
    let dir = tempfile::tempdir().unwrap();
    let (ok, output) = run_cmd_with_timeout(&["echo", "hello"], dir.path(), Duration::from_secs(5));
    assert!(ok);
    assert_eq!(output, "hello");
}

#[test]
fn run_cmd_with_timeout_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let (ok, output) =
        run_cmd_with_timeout(&["sleep", "10"], dir.path(), Duration::from_millis(200));
    assert!(!ok);
    assert_eq!(output, "timeout");
}

#[test]
fn run_cmd_with_timeout_nonzero_exit_with_stderr() {
    // sh -c 'echo err >&2; exit 1' → non-zero status with stderr
    // content; returns (false, stderr).
    let dir = tempfile::tempdir().unwrap();
    let (ok, output) = run_cmd_with_timeout(
        &["sh", "-c", "echo errmsg >&2; exit 1"],
        dir.path(),
        Duration::from_secs(5),
    );
    assert!(!ok);
    assert!(output.contains("errmsg"));
}

#[test]
fn run_cmd_with_timeout_nonzero_exit_empty_stderr() {
    // sh -c 'echo out; exit 1' → non-zero with empty stderr; the
    // fallback arm returns stdout as the output string.
    let dir = tempfile::tempdir().unwrap();
    let (ok, output) = run_cmd_with_timeout(
        &["sh", "-c", "echo outmsg; exit 1"],
        dir.path(),
        Duration::from_secs(5),
    );
    assert!(!ok);
    assert!(output.contains("outmsg"));
}

// --- try_delete_adversarial_test_files error paths ---

#[test]
fn try_delete_adversarial_test_files_multiple_failures_reports_first_error() {
    // When multiple matching entries fail removal, the helper records only
    // the first error and keeps scanning. Exercise the "is_empty is false"
    // branch of the inner error recorder.
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let states = dir.path().join(".flow-states");
    fs::create_dir_all(&states).unwrap();
    fs::write(states.join("test-feature-adversarial_test.rs"), "x").unwrap();
    fs::write(states.join("test-feature-adversarial_test.py"), "y").unwrap();
    fs::set_permissions(&states, fs::Permissions::from_mode(0o500)).unwrap();

    let result = try_delete_adversarial_test_files(&states, "test-feature");

    fs::set_permissions(&states, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        result.starts_with("failed:"),
        "expected failed, got: {}",
        result
    );
}

#[test]
fn cleanup_worktree_tmp_remove_fails_reports_error() {
    // A read-only parent directory makes fs::remove_dir_all on wt/tmp fail
    // while is_dir() still returns true. Exercise the Err arm of the
    // `fs::remove_dir_all(&wt_tmp)` match in cleanup().
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    fs::write(dir.path().join("flow-phases.json"), "{}").unwrap();

    let wt_root = dir.path().join(&wt_rel);
    let wt_tmp = wt_root.join("tmp");
    fs::create_dir_all(&wt_tmp).unwrap();
    fs::write(wt_tmp.join("f.txt"), "x").unwrap();
    // Lock the worktree root so remove_dir_all on tmp fails.
    fs::set_permissions(&wt_root, fs::Permissions::from_mode(0o500)).unwrap();

    let steps = cleanup(dir.path(), "test-feature", &wt_rel, None, false);

    // Restore permissions so tempdir cleanup can succeed.
    fs::set_permissions(&wt_root, fs::Permissions::from_mode(0o755)).unwrap();

    assert!(
        steps["worktree_tmp"].starts_with("failed:"),
        "expected failed, got: {}",
        steps["worktree_tmp"]
    );
}

#[test]
fn try_delete_adversarial_test_files_all_fail_returns_failed() {
    // When the only matching entry cannot be removed (e.g. parent
    // directory is read-only), the helper reports "failed: <err>".
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let states = dir.path().join(".flow-states");
    fs::create_dir_all(&states).unwrap();
    let adv = states.join("test-feature-adversarial_test.rs");
    fs::write(&adv, "x").unwrap();
    // Lock the parent directory so unlink fails.
    fs::set_permissions(&states, fs::Permissions::from_mode(0o500)).unwrap();

    let result = try_delete_adversarial_test_files(&states, "test-feature");

    // Restore before any assertion so tempdir can clean up.
    fs::set_permissions(&states, fs::Permissions::from_mode(0o755)).unwrap();

    assert!(
        result.starts_with("failed:"),
        "expected failed, got: {}",
        result
    );
}

// --- Invalid branch (slash) — FlowPaths::try_new returns None ---

#[test]
fn test_cleanup_invalid_branch_skips_path_dependent_steps() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    // Note: setup_feature with "feature/foo" may fail git ops, so
    // skip worktree creation and call cleanup directly with a slash
    // branch so FlowPaths::try_new returns None.
    let steps = cleanup(
        dir.path(),
        "feature/foo",
        ".worktrees/feature-foo",
        None,
        false,
    );
    for key in [
        "state_file",
        "plan_file",
        "dag_file",
        "log_file",
        "frozen_phases",
        "ci_sentinel",
        "timings_file",
        "closed_issues_file",
        "issues_file",
        "adversarial_test",
    ] {
        assert_eq!(steps[key], "skipped: invalid branch");
    }
}

#[test]
fn test_cleanup_invalid_branch_with_pull_still_runs_pull() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let steps = cleanup(
        dir.path(),
        "feature/foo",
        ".worktrees/feature-foo",
        None,
        true,
    );
    assert!(steps.contains_key("git_pull"));
}

// --- run_impl_main ---

#[test]
fn run_impl_main_nonexistent_root_returns_error() {
    let args = Args {
        project_root: "/nonexistent/path/xyz".to_string(),
        branch: "test".to_string(),
        worktree: ".worktrees/test".to_string(),
        pr: None,
        pull: false,
    };
    let (value, code) = run_impl_main(&args);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
}

// --- label_result ---

#[test]
fn label_result_ok_returns_label() {
    assert_eq!(label_result(true, "closed", "ignored"), "closed");
    assert_eq!(label_result(true, "deleted", ""), "deleted");
    assert_eq!(label_result(true, "pulled", "ignored"), "pulled");
    assert_eq!(label_result(true, "removed", ""), "removed");
}

#[test]
fn label_result_fail_returns_formatted_output() {
    assert_eq!(label_result(false, "closed", "boom"), "failed: boom");
    assert_eq!(label_result(false, "pulled", ""), "failed: ");
}

// --- try_delete_file error path ---

#[test]
fn try_delete_file_permission_denied_returns_failed() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("locked");
    fs::create_dir_all(&sub).unwrap();
    let target = sub.join("f.txt");
    fs::write(&target, "x").unwrap();
    // Lock the parent directory so unlink fails.
    fs::set_permissions(&sub, fs::Permissions::from_mode(0o500)).unwrap();

    let result = try_delete_file(&target);

    // Restore permissions so tempdir cleanup can run.
    fs::set_permissions(&sub, fs::Permissions::from_mode(0o755)).unwrap();

    assert!(
        result.starts_with("failed:"),
        "expected failed, got: {}",
        result
    );
}

#[test]
fn run_impl_main_valid_root_returns_ok() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let args = Args {
        project_root: dir.path().to_string_lossy().to_string(),
        branch: "test-branch".to_string(),
        worktree: ".worktrees/test-branch".to_string(),
        pr: None,
        pull: false,
    };
    let (value, code) = run_impl_main(&args);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
}
