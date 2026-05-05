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

#[path = "common/mod.rs"]
mod common;

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

/// Create a worktree and seed the branch directory with a state.json
/// and log file. Returns the worktree's relative path.
fn setup_feature(git_repo: &Path, branch: &str) -> String {
    let wt_rel = format!(".worktrees/{}", branch);
    StdCommand::new("git")
        .args(["worktree", "add", &wt_rel, "-b", branch])
        .current_dir(git_repo)
        .output()
        .unwrap();

    let branch_dir = git_repo.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(
        branch_dir.join("state.json"),
        json!({"branch": branch}).to_string(),
    )
    .unwrap();
    fs::write(branch_dir.join("log"), "test log\n").unwrap();

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

/// Drive the `Some(str)` branch of `read_base_branch` through
/// `cleanup --pull` and prove the state-file value reaches
/// `git pull origin <base_branch>`. The fixture creates a bare
/// remote with only `main`; the state file declares
/// `base_branch: "staging"`. After the helper plumbing,
/// `cleanup` issues `git pull origin staging` against the bare
/// remote, which fails — the failure stderr (carrying "staging")
/// surfaces as the `steps.git_pull` value, proving the value
/// flowed through rather than the hardcoded "main".
#[test]
fn cleanup_pulls_base_branch_from_state() {
    let tmp = tempfile::tempdir().unwrap();
    let parent = tmp.path().canonicalize().unwrap();
    // create_git_repo_with_remote sets up bare main + working repo
    // with origin pointing at it.
    let repo = common::create_git_repo_with_remote(&parent);

    // Worktree on a feature branch.
    let wt_rel = ".worktrees/test-feature".to_string();
    StdCommand::new("git")
        .args(["worktree", "add", &wt_rel, "-b", "test-feature"])
        .current_dir(&repo)
        .output()
        .unwrap();

    // State file with base_branch=staging.
    let branch_dir = repo.join(".flow-states").join("test-feature");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(
        branch_dir.join("state.json"),
        json!({
            "branch": "test-feature",
            "base_branch": "staging",
        })
        .to_string(),
    )
    .unwrap();

    let (value, code) = run_impl_main(&args_for(&repo, "test-feature", &wt_rel, None, true));
    assert_eq!(code, 0, "cleanup should report ok overall, got: {}", value);
    let steps = steps_from(&value);
    let pull_result = steps
        .get("git_pull")
        .cloned()
        .unwrap_or_else(|| "<missing>".to_string());
    assert!(
        pull_result.contains("staging"),
        "git_pull step must reference 'staging' to prove base_branch flowed through, got: {}",
        pull_result
    );
}

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
fn cleanup_removes_branch_dir_with_seeded_artifacts() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let branch_dir = dir.path().join(".flow-states/test-feature");

    // Seed every per-branch artifact the production layout supports
    // so the single recursive remove is exercised across the full set.
    fs::write(branch_dir.join("plan.md"), "# Plan\n").unwrap();
    fs::write(branch_dir.join("dag.md"), "# DAG\n").unwrap();
    fs::write(
        branch_dir.join("phases.json"),
        r#"{"phases":{},"order":[]}"#,
    )
    .unwrap();
    fs::write(branch_dir.join("ci-passed"), "snapshot\n").unwrap();
    fs::write(branch_dir.join("timings.md"), "| Phase | Duration |\n").unwrap();
    fs::write(branch_dir.join("closed-issues.json"), r#"[{"number":42}]"#).unwrap();
    fs::write(branch_dir.join("issues.md"), "| Label | Title |\n").unwrap();
    fs::write(branch_dir.join("commit-msg.txt"), "Subject\n").unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["branch_dir"], "deleted");
    assert!(!branch_dir.exists());
}

#[test]
fn cleanup_branch_dir_skipped_when_already_missing() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    fs::remove_dir_all(dir.path().join(".flow-states/test-feature")).unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["branch_dir"], "skipped");
}

#[test]
fn cleanup_branch_dir_idempotent_across_repeated_calls() {
    // The cleanup may run twice (abort-then-complete in adjacent
    // sessions, or a retry after a partial failure). The second call
    // must report `skipped` rather than failing because the directory
    // was already removed by the first.
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let (value1, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    assert_eq!(steps_from(&value1)["branch_dir"], "deleted");

    let (value2, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    assert_eq!(steps_from(&value2)["branch_dir"], "skipped");
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
    assert_eq!(steps["branch_dir"], "deleted");

    assert!(!dir.path().join(&wt_rel).exists());
    assert!(!dir.path().join(".flow-states/test-feature").exists());
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
            "branch_dir",
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
            "branch_dir",
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
fn cleanup_branch_dir_permission_denied_returns_failed() {
    // A `.flow-states/` whose permissions prevent unlinking children
    // exercises the Err(IO) arm of `fs::remove_dir_all` on a populated
    // branch directory.
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
        steps["branch_dir"].starts_with("failed:"),
        "expected failed for branch_dir, got: {}",
        steps["branch_dir"]
    );
}

// --- Invalid branch ---

#[test]
fn cleanup_invalid_branch_skips_branch_dir() {
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
    assert_eq!(steps["branch_dir"], "skipped: invalid branch");
    // The path-dependent enumeration collapsed to a single entry —
    // legacy per-suffix keys must not appear.
    for legacy_key in [
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
        assert!(
            !steps.contains_key(legacy_key),
            "legacy per-suffix key {legacy_key} must not appear after consolidation"
        );
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
        .env("PATH", "/nonexistent-path-for-flow-test")
        .env("HOME", &root)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last_line = stdout.trim().lines().last().unwrap_or("");
    let data: Value = serde_json::from_str(last_line).expect("json");
    let steps = data["steps"].as_object().unwrap();
    let any_failed = steps
        .values()
        .any(|v| v.as_str().unwrap_or("").starts_with("failed:"));
    assert!(
        any_failed,
        "expected at least one failed step with restricted PATH, got: {:?}",
        steps
    );
}
