//! Integration tests for `bin/flow cleanup`. Drive through the public
//! `run_impl_main` entry point (and the compiled binary for
//! CLI-dispatch coverage) — no private helpers imported per
//! `.claude/rules/test-placement.md`.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::process::Command as StdCommand;

use flow_rs::cleanup::{run_impl_main, Args};
use serde_json::{json, Value};

fn flow_rs_no_recursion() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd
}

/// Create a minimal git repo for testing.
fn setup_git_repo(dir: &Path) {
    StdCommand::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .unwrap();
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

    let state_dir = git_repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join(format!("{}.json", branch)),
        json!({"branch": branch}).to_string(),
    )
    .unwrap();
    fs::write(state_dir.join(format!("{}.log", branch)), "test log\n").unwrap();

    wt_rel
}

fn args_for(dir: &Path, branch: &str, wt_rel: &str, pr: Option<i64>, pull: bool) -> Args {
    Args {
        project_root: dir.to_string_lossy().to_string(),
        branch: branch.to_string(),
        worktree: wt_rel.to_string(),
        pr,
        pull,
    }
}

fn steps_from(value: &Value) -> indexmap::IndexMap<String, String> {
    value["steps"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
        .collect()
}

// --- CLI integration tests (binary dispatch) ---

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
    assert!(
        stdout.contains("\"status\":"),
        "expected JSON status in stdout, got: {}",
        stdout
    );
}

// --- Library-level tests via run_impl_main ---

#[test]
fn cleanup_removes_worktree() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let (value, code) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    assert_eq!(code, 0);
    let steps = steps_from(&value);
    assert_eq!(steps["worktree"], "removed");
    assert!(!dir.path().join(&wt_rel).exists());
}

#[test]
fn cleanup_deletes_state_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let (value, code) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    assert_eq!(code, 0);
    let steps = steps_from(&value);
    assert_eq!(steps["state_file"], "deleted");
    assert!(!dir.path().join(".flow-states/test-feature.json").exists());
}

#[test]
fn cleanup_deletes_log_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["log_file"], "deleted");
    assert!(!dir.path().join(".flow-states/test-feature.log").exists());
}

#[test]
fn cleanup_deletes_plan_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let plan = dir.path().join(".flow-states/test-feature-plan.md");
    fs::write(&plan, "# Plan\n").unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["plan_file"], "deleted");
    assert!(!plan.exists());
}

#[test]
fn cleanup_skips_missing_plan_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["plan_file"], "skipped");
}

#[test]
fn cleanup_deletes_dag_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let dag = dir.path().join(".flow-states/test-feature-dag.md");
    fs::write(&dag, "# DAG\n").unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["dag_file"], "deleted");
    assert!(!dag.exists());
}

#[test]
fn cleanup_skips_missing_dag_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["dag_file"], "skipped");
}

#[test]
fn cleanup_deletes_frozen_phases_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let frozen = dir.path().join(".flow-states/test-feature-phases.json");
    fs::write(&frozen, r#"{"phases": {}, "order": []}"#).unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["frozen_phases"], "deleted");
    assert!(!frozen.exists());
}

#[test]
fn cleanup_skips_missing_frozen_phases() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["frozen_phases"], "skipped");
}

#[test]
fn cleanup_deletes_ci_sentinel() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let sentinel = dir.path().join(".flow-states/test-feature-ci-passed");
    fs::write(&sentinel, "snapshot\n").unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["ci_sentinel"], "deleted");
    assert!(!sentinel.exists());
}

#[test]
fn cleanup_skips_missing_ci_sentinel() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["ci_sentinel"], "skipped");
}

#[test]
fn cleanup_deletes_timings_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let timings = dir.path().join(".flow-states/test-feature-timings.md");
    fs::write(&timings, "| Phase | Duration |\n").unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["timings_file"], "deleted");
    assert!(!timings.exists());
}

#[test]
fn cleanup_skips_missing_timings_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["timings_file"], "skipped");
}

#[test]
fn cleanup_deletes_closed_issues_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let closed = dir
        .path()
        .join(".flow-states/test-feature-closed-issues.json");
    fs::write(&closed, r#"[{"number": 42}]"#).unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["closed_issues_file"], "deleted");
    assert!(!closed.exists());
}

#[test]
fn cleanup_skips_missing_closed_issues_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["closed_issues_file"], "skipped");
}

#[test]
fn cleanup_deletes_issues_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let issues = dir.path().join(".flow-states/test-feature-issues.md");
    fs::write(&issues, "| Label | Title |\n").unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["issues_file"], "deleted");
    assert!(!issues.exists());
}

#[test]
fn cleanup_skips_missing_issues_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["issues_file"], "skipped");
}

#[test]
fn cleanup_deletes_adversarial_test_rs() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let adv = dir
        .path()
        .join(".flow-states/test-feature-adversarial_test.rs");
    fs::write(&adv, "// adversarial test\n").unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["adversarial_test"], "deleted");
    assert!(!adv.exists());
}

#[test]
fn cleanup_skips_missing_adversarial_test() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["adversarial_test"], "skipped");
}

#[test]
fn cleanup_deletes_adversarial_test_multiple_extensions() {
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

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["adversarial_test"], "deleted");
    assert!(!adv_rs.exists());
    assert!(!adv_py.exists());
}

#[test]
fn abort_path_deletes_adversarial_test() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let adv = dir
        .path()
        .join(".flow-states/test-feature-adversarial_test.rs");
    fs::write(&adv, "// adversarial\n").unwrap();

    let (value, _) = run_impl_main(&args_for(
        dir.path(),
        "test-feature",
        &wt_rel,
        Some(999),
        false,
    ));
    let steps = steps_from(&value);
    assert_eq!(steps["adversarial_test"], "deleted");
    assert!(!adv.exists());
}

#[test]
fn cleanup_adversarial_test_respects_branch_prefix() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let other = dir
        .path()
        .join(".flow-states/other-branch-adversarial_test.rs");
    fs::write(&other, "// other branch\n").unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["adversarial_test"], "skipped");
    assert!(other.exists());
}

#[test]
fn cleanup_adversarial_test_trailing_dot_precision() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let other = dir
        .path()
        .join(".flow-states/test-feature-adversarial_test_other.rs");
    fs::write(&other, "// sibling\n").unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["adversarial_test"], "skipped");
    assert!(other.exists());
}

#[test]
fn cleanup_skips_adversarial_test_when_flow_states_missing() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    fs::remove_dir_all(dir.path().join(".flow-states")).unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["adversarial_test"], "skipped");
}

#[test]
fn cleanup_adversarial_test_skips_directory_and_deletes_files() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let states = dir.path().join(".flow-states");
    let bad_dir = states.join("test-feature-adversarial_test.d");
    fs::create_dir_all(&bad_dir).unwrap();
    let adv_rs = states.join("test-feature-adversarial_test.rs");
    let adv_py = states.join("test-feature-adversarial_test.py");
    let adv_go = states.join("test-feature-adversarial_test.go");
    fs::write(&adv_rs, "// rs\n").unwrap();
    fs::write(&adv_py, "# py\n").unwrap();
    fs::write(&adv_go, "// go\n").unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["adversarial_test"], "deleted");
    assert!(!adv_rs.exists());
    assert!(!adv_py.exists());
    assert!(!adv_go.exists());
    assert!(bad_dir.exists());
}

#[test]
fn cleanup_skips_pr_by_default() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["pr_close"], "skipped");
}

#[test]
fn abort_pr_close_fails_gracefully() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let (value, _) = run_impl_main(&args_for(
        dir.path(),
        "test-feature",
        &wt_rel,
        Some(999),
        false,
    ));
    let steps = steps_from(&value);
    assert!(steps["pr_close"].starts_with("failed:"));
}

#[test]
fn cleanup_skips_remote_branch_on_complete() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["remote_branch"], "skipped");
}

#[test]
fn abort_attempts_remote_branch_deletion() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let (value, _) = run_impl_main(&args_for(
        dir.path(),
        "test-feature",
        &wt_rel,
        Some(999),
        false,
    ));
    let steps = steps_from(&value);
    assert!(steps["remote_branch"].starts_with("failed:"));
}

#[test]
fn cleanup_deletes_local_branch() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    StdCommand::new("git")
        .args(["worktree", "remove", &wt_rel, "--force"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["local_branch"], "deleted");
}

#[test]
fn cleanup_skips_missing_worktree() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    StdCommand::new("git")
        .args(["worktree", "remove", &wt_rel, "--force"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["worktree"], "skipped");
}

#[test]
fn cleanup_skips_missing_state_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    fs::remove_file(dir.path().join(".flow-states/test-feature.json")).unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["state_file"], "skipped");
}

#[test]
fn cleanup_skips_missing_log_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    fs::remove_file(dir.path().join(".flow-states/test-feature.log")).unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["log_file"], "skipped");
}

#[test]
fn cleanup_full_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let (value, code) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    assert_eq!(code, 0);
    let steps = steps_from(&value);

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

    assert!(!dir.path().join(&wt_rel).exists());
    assert!(!dir.path().join(".flow-states/test-feature.json").exists());
    assert!(!dir.path().join(".flow-states/test-feature.log").exists());
}

#[test]
fn cleanup_removes_worktree_tmp_in_flow_repo() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    fs::write(dir.path().join("flow-phases.json"), "{}").unwrap();
    let wt_tmp = dir.path().join(&wt_rel).join("tmp");
    fs::create_dir_all(&wt_tmp).unwrap();
    fs::write(wt_tmp.join("release-notes-v1.0.md"), "notes").unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["worktree_tmp"], "removed");
}

#[test]
fn cleanup_skips_tmp_without_flow_phases() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let wt_tmp = dir.path().join(&wt_rel).join("tmp");
    fs::create_dir_all(&wt_tmp).unwrap();
    fs::write(wt_tmp.join("some-file.txt"), "data").unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["worktree_tmp"], "skipped");
}

#[test]
fn cleanup_skips_missing_worktree_tmp() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    fs::write(dir.path().join("flow-phases.json"), "{}").unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["worktree_tmp"], "skipped");
}

#[test]
fn no_pull_flag_no_git_pull_step() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert!(!steps.contains_key("git_pull"));
}

#[test]
fn pull_flag_present_runs_pull() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, true));
    let steps = steps_from(&value);
    assert!(steps.contains_key("git_pull"));
    assert!(steps["git_pull"].starts_with("failed:"));
}

#[test]
fn step_key_order_matches_expected() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
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
fn step_key_order_with_pull() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, true));
    let steps = steps_from(&value);
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

// --- Error paths ---

#[test]
fn cleanup_worktree_tmp_remove_fails_reports_error() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    fs::write(dir.path().join("flow-phases.json"), "{}").unwrap();

    let wt_root = dir.path().join(&wt_rel);
    let wt_tmp = wt_root.join("tmp");
    fs::create_dir_all(&wt_tmp).unwrap();
    fs::write(wt_tmp.join("f.txt"), "x").unwrap();
    fs::set_permissions(&wt_root, fs::Permissions::from_mode(0o500)).unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));

    fs::set_permissions(&wt_root, fs::Permissions::from_mode(0o755)).unwrap();

    let steps = steps_from(&value);
    assert!(
        steps["worktree_tmp"].starts_with("failed:"),
        "expected failed, got: {}",
        steps["worktree_tmp"]
    );
}

#[test]
fn try_delete_adversarial_test_files_all_fail_returns_failed() {
    // Seed MULTIPLE adversarial-test files so the loop encounters the
    // second error after the first one is already recorded — exercises
    // the `!first_error.is_empty()` branch in the inner error recorder.
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let states = dir.path().join(".flow-states");
    fs::write(states.join("test-feature-adversarial_test.rs"), "x").unwrap();
    fs::write(states.join("test-feature-adversarial_test.py"), "y").unwrap();
    fs::set_permissions(&states, fs::Permissions::from_mode(0o500)).unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));

    fs::set_permissions(&states, fs::Permissions::from_mode(0o755)).unwrap();

    let steps = steps_from(&value);
    assert!(
        steps["adversarial_test"].starts_with("failed:"),
        "expected failed, got: {}",
        steps["adversarial_test"]
    );
}

#[test]
fn try_delete_file_permission_denied_returns_failed() {
    // A state file in a directory whose permissions prevent unlinking
    // exercises the Err branch of fs::remove_file inside try_delete_file.
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let states = dir.path().join(".flow-states");
    fs::set_permissions(&states, fs::Permissions::from_mode(0o500)).unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));

    fs::set_permissions(&states, fs::Permissions::from_mode(0o755)).unwrap();

    let steps = steps_from(&value);
    assert!(
        steps["state_file"].starts_with("failed:"),
        "expected failed for state_file, got: {}",
        steps["state_file"]
    );
}

// --- Invalid branch ---

#[test]
fn cleanup_invalid_branch_skips_path_dependent_steps() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());

    let (value, _) = run_impl_main(&args_for(
        dir.path(),
        "feature/foo",
        ".worktrees/feature-foo",
        None,
        false,
    ));
    let steps = steps_from(&value);
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
fn cleanup_invalid_branch_with_pull_still_runs_pull() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let (value, _) = run_impl_main(&args_for(
        dir.path(),
        "feature/foo",
        ".worktrees/feature-foo",
        None,
        true,
    ));
    let steps = steps_from(&value);
    assert!(steps.contains_key("git_pull"));
}

// --- run_impl_main error ---

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

/// A fake `gh` that exits non-zero and writes to stdout (not stderr)
/// exercises the empty-stderr fallback branch in `run_cmd`. Spawned
/// via subprocess with fake bin prepended to PATH.
#[test]
fn cli_run_cmd_nonzero_exit_empty_stderr_returns_stdout() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    setup_git_repo(&root);
    let _wt_rel = setup_feature(&root, "test-feature");

    // Fake gh: writes to stdout, no stderr, exits 1.
    let fake_bin = root.join("fakebin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_gh = fake_bin.join("gh");
    fs::write(
        &fake_gh,
        "#!/usr/bin/env bash\necho 'fake gh stdout error'\nexit 1\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake_gh, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let path_with_fake = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = flow_rs_no_recursion()
        .args([
            "cleanup",
            root.to_str().unwrap(),
            "--branch",
            "test-feature",
            "--worktree",
            ".worktrees/test-feature",
            "--pr",
            "999",
        ])
        .env("PATH", path_with_fake)
        .env("HOME", &root)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last_line = stdout.trim().lines().last().unwrap_or("");
    let data: Value = serde_json::from_str(last_line).expect("json");
    let steps = data["steps"].as_object().unwrap();
    // pr_close must report the fake gh's stdout in the failed message.
    let pr_close = steps["pr_close"].as_str().unwrap();
    assert!(
        pr_close.starts_with("failed:"),
        "expected failed pr_close, got: {}",
        pr_close
    );
    assert!(
        pr_close.contains("fake gh stdout error"),
        "expected stdout in failure message, got: {}",
        pr_close
    );
}

// --- run_cmd error branch (spawn failure) ---
//
// Spawn a subprocess with PATH that doesn't contain `gh`/`git` so the
// run_cmd internal spawn fails. This exercises the `Err(e)` arm of
// Command::output(). We run a FULL cleanup (so multiple run_cmd calls
// fail) to ensure the branch is hit.

#[test]
fn cli_run_cmd_spawn_err_produces_failed_step() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    setup_git_repo(&root);
    let _wt_rel = setup_feature(&root, "test-feature");

    let output = flow_rs_no_recursion()
        .args([
            "cleanup",
            root.to_str().unwrap(),
            "--branch",
            "test-feature",
            "--worktree",
            ".worktrees/test-feature",
            "--pr",
            "999",
        ])
        // Restrict PATH so gh/git spawn fails with Err from Command::output.
        .env("PATH", "/nonexistent-path-for-flow-test")
        .env("HOME", &root)
        .output()
        .unwrap();
    // Command ran — verify it didn't panic and produced JSON output.
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Last line is JSON.
    let last_line = stdout.trim().lines().last().unwrap_or("");
    let data: Value = serde_json::from_str(last_line).expect("json");
    let steps = data["steps"].as_object().unwrap();
    // At least one step should have "failed:" prefix from spawn failure.
    let any_failed = steps
        .values()
        .any(|v| v.as_str().unwrap_or("").starts_with("failed:"));
    assert!(
        any_failed,
        "expected at least one failed step with restricted PATH, got: {:?}",
        steps
    );
}
