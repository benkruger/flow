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
/// and log file. Returns the worktree's relative path. The state file
/// includes the `worktree` field so cleanup_all's
/// `is_safe_worktree_rel` validator accepts it (an empty/missing
/// worktree value is rejected as a state-derived path-construction
/// hazard).
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
        json!({"branch": branch, "worktree": &wt_rel}).to_string(),
    )
    .unwrap();
    fs::write(branch_dir.join("log"), "test log\n").unwrap();

    wt_rel
}

fn args_for(dir: &Path, branch: &str, wt_rel: &str, pr: Option<i64>, pull: bool) -> Args {
    Args {
        project_root: dir.to_string_lossy().to_string(),
        branch: Some(branch.to_string()),
        worktree: Some(wt_rel.to_string()),
        pr,
        pull,
        all: false,
        dry_run: false,
    }
}

/// Build Args for the `--all` cleanup_all entry shape. `branch`,
/// `worktree`, `pr`, and `pull` are unused in `--all` mode.
fn args_all(dir: &Path, dry_run: bool) -> Args {
    Args {
        project_root: dir.to_string_lossy().to_string(),
        branch: None,
        worktree: None,
        pr: None,
        pull: false,
        all: true,
        dry_run,
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
            "adversarial_probe",
            "worktree",
            "remote_branch",
            "local_branch",
            "branch_dir",
            "queue_entry",
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
            "adversarial_probe",
            "worktree",
            "remote_branch",
            "local_branch",
            "branch_dir",
            "queue_entry",
            "git_pull",
        ]
    );
}

// --- queue_entry step ---

#[test]
fn cleanup_queue_entry_removes_present_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let queue_dir = dir.path().join(".flow-states/start-queue");
    fs::create_dir_all(&queue_dir).unwrap();
    let queue_file = queue_dir.join("test-feature");
    fs::write(&queue_file, "").unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["queue_entry"], "removed");
    assert!(!queue_file.exists(), "queue entry file must be removed");
}

#[test]
fn cleanup_queue_entry_skipped_when_absent() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    // No .flow-states/start-queue/ directory at all.

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(steps["queue_entry"], "skipped");
}

#[test]
fn cleanup_queue_entry_failed_on_unwritable_parent() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");

    let queue_dir = dir.path().join(".flow-states/start-queue");
    fs::create_dir_all(&queue_dir).unwrap();
    let queue_file = queue_dir.join("test-feature");
    fs::write(&queue_file, "").unwrap();
    fs::set_permissions(&queue_dir, fs::Permissions::from_mode(0o500)).unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));

    // Restore so TempDir can drop cleanly.
    fs::set_permissions(&queue_dir, fs::Permissions::from_mode(0o755)).unwrap();

    let steps = steps_from(&value);
    assert!(
        steps["queue_entry"].starts_with("failed:"),
        "expected failed, got: {}",
        steps["queue_entry"]
    );
}

// --- Error paths ---

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
        branch: Some("test".to_string()),
        worktree: Some(".worktrees/test".to_string()),
        pr: None,
        pull: false,
        all: false,
        dry_run: false,
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

// --- cleanup_all (--all) ---

/// Write a `state.json` for a flow without creating a real worktree.
/// The cleanup_all walk only needs the file to exist; per-flow
/// `cleanup()` tolerates missing worktrees / branches by reporting
/// "skipped"/"failed" for those steps. PR number is optional.
fn setup_flow_state(git_repo: &Path, branch: &str, pr_number: Option<i64>) {
    let branch_dir = git_repo.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    let mut state = json!({
        "branch": branch,
        "worktree": format!(".worktrees/{}", branch),
        "base_branch": "main",
    });
    if let Some(n) = pr_number {
        state["pr_number"] = json!(n);
    }
    fs::write(branch_dir.join("state.json"), state.to_string()).unwrap();
}

#[test]
fn cleanup_all_empty_states_dir_returns_empty_inventory() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    // No .flow-states/ directory at all.

    let value = run_impl_main(&args_all(dir.path(), false)).0;
    assert_eq!(value["status"], "ok");
    assert_eq!(value["dry_run"], false);
    assert_eq!(value["flows"].as_array().unwrap().len(), 0);
    assert_eq!(value["orchestrate_json"], "skipped");
    assert_eq!(value["main_dir"], "skipped");
    assert_eq!(value["queue_sweep"], "skipped");
}

#[test]
fn cleanup_all_single_flow_calls_per_branch_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let _wt_rel = setup_feature(dir.path(), "test-feature");

    let value = run_impl_main(&args_all(dir.path(), false)).0;
    let flows = value["flows"].as_array().unwrap();
    assert_eq!(flows.len(), 1, "expected exactly one flow, got: {}", value);
    assert_eq!(flows[0]["branch"], "test-feature");
    let steps = flows[0]["steps"].as_object().expect("steps must be object");
    assert!(
        steps.contains_key("branch_dir"),
        "expected branch_dir step, got: {:?}",
        steps.keys().collect::<Vec<_>>()
    );
    assert!(!dir.path().join(".flow-states/test-feature").exists());
}

#[test]
fn cleanup_all_multiple_flows_iterates_each() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    setup_flow_state(dir.path(), "alpha", None);
    setup_flow_state(dir.path(), "bravo", None);
    setup_flow_state(dir.path(), "charlie", None);

    let value = run_impl_main(&args_all(dir.path(), false)).0;
    let flows = value["flows"].as_array().unwrap();
    assert_eq!(flows.len(), 3);
    let branches: Vec<&str> = flows
        .iter()
        .map(|f| f["branch"].as_str().unwrap())
        .collect();
    // Subdirectories are sorted by file_name in find_state_files-style walk.
    assert_eq!(branches, vec!["alpha", "bravo", "charlie"]);
    for branch in &["alpha", "bravo", "charlie"] {
        assert!(
            !dir.path().join(".flow-states").join(branch).exists(),
            "branch_dir for {} must be gone",
            branch
        );
    }
}

#[test]
fn cleanup_all_skips_subdirs_without_state_json() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    // Subdir without state.json — looks like the base-branch CI sentinel.
    let main_subdir = dir.path().join(".flow-states/main");
    fs::create_dir_all(&main_subdir).unwrap();
    fs::write(main_subdir.join("ci-passed"), "snapshot").unwrap();

    let value = run_impl_main(&args_all(dir.path(), false)).0;
    let flows = value["flows"].as_array().unwrap();
    let names: Vec<&str> = flows
        .iter()
        .map(|f| f["branch"].as_str().unwrap())
        .collect();
    assert!(
        !names.contains(&"main"),
        "main/ has no state.json — must not appear in flows[], got: {:?}",
        names
    );
}

#[test]
fn cleanup_all_skips_unreadable_state_json() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());

    // One good flow.
    setup_flow_state(dir.path(), "good", None);

    // One malformed state.json.
    let bad_dir = dir.path().join(".flow-states/bad");
    fs::create_dir_all(&bad_dir).unwrap();
    fs::write(bad_dir.join("state.json"), "{ this is not valid json").unwrap();

    let value = run_impl_main(&args_all(dir.path(), false)).0;
    let flows = value["flows"].as_array().unwrap();
    assert_eq!(
        flows.len(),
        2,
        "expected both flows reported, got: {}",
        value
    );

    // The bad flow has an `error` field; the good flow does not.
    let bad = flows.iter().find(|f| f["branch"] == "bad").unwrap();
    assert!(
        bad["error"].is_string(),
        "bad flow must report error, got: {}",
        bad
    );
    let good = flows.iter().find(|f| f["branch"] == "good").unwrap();
    assert!(
        good["steps"].is_object(),
        "good flow must process: {}",
        good
    );
}

#[test]
fn cleanup_all_removes_orchestrate_json() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let states_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&states_dir).unwrap();
    fs::write(states_dir.join("orchestrate.json"), "{}").unwrap();

    let value = run_impl_main(&args_all(dir.path(), false)).0;
    assert_eq!(value["orchestrate_json"], "deleted");
    assert!(!states_dir.join("orchestrate.json").exists());
}

#[test]
fn cleanup_all_skips_missing_orchestrate_json() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let states_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&states_dir).unwrap();
    // No orchestrate.json.

    let value = run_impl_main(&args_all(dir.path(), false)).0;
    assert_eq!(value["orchestrate_json"], "skipped");
}

#[test]
fn cleanup_all_removes_main_directory() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let main_dir = dir.path().join(".flow-states/main");
    fs::create_dir_all(&main_dir).unwrap();
    fs::write(main_dir.join("ci-passed"), "snapshot").unwrap();

    let value = run_impl_main(&args_all(dir.path(), false)).0;
    assert_eq!(value["main_dir"], "removed");
    assert!(!main_dir.exists());
}

#[test]
fn cleanup_all_skips_missing_main_directory() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let states_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&states_dir).unwrap();
    // No main/ subdir.

    let value = run_impl_main(&args_all(dir.path(), false)).0;
    assert_eq!(value["main_dir"], "skipped");
}

#[test]
fn cleanup_all_sweeps_residual_queue_entries() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let queue_dir = dir.path().join(".flow-states/start-queue");
    fs::create_dir_all(&queue_dir).unwrap();
    fs::write(queue_dir.join("orphan-1"), "").unwrap();
    fs::write(queue_dir.join("orphan-2"), "").unwrap();

    let value = run_impl_main(&args_all(dir.path(), false)).0;
    assert_eq!(value["queue_sweep"], "swept 2 entries");
    assert!(!queue_dir.join("orphan-1").exists());
    assert!(!queue_dir.join("orphan-2").exists());
    // queue_dir itself remains for future flows.
    assert!(queue_dir.is_dir(), "start-queue/ directory must remain");
}

#[test]
fn cleanup_all_dry_run_returns_inventory_no_disk_mutation() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    setup_flow_state(dir.path(), "alpha", None);
    setup_flow_state(dir.path(), "bravo", None);
    let states_dir = dir.path().join(".flow-states");
    fs::write(states_dir.join("orchestrate.json"), "{}").unwrap();
    let main_subdir = states_dir.join("main");
    fs::create_dir_all(&main_subdir).unwrap();
    let queue_dir = states_dir.join("start-queue");
    fs::create_dir_all(&queue_dir).unwrap();
    fs::write(queue_dir.join("orphan"), "").unwrap();

    let value = run_impl_main(&args_all(dir.path(), true)).0;
    assert_eq!(value["dry_run"], true);

    // Dry-run reports flows but does not exercise per-branch cleanup.
    let flows = value["flows"].as_array().unwrap();
    assert_eq!(flows.len(), 2);
    for flow in flows {
        assert!(
            flow.get("steps").is_none(),
            "dry-run flows must NOT carry steps, got: {}",
            flow
        );
    }

    // Tail steps report the would-be values.
    assert_eq!(value["orchestrate_json"], "would_remove");
    assert_eq!(value["main_dir"], "would_remove");
    assert_eq!(value["queue_sweep"], "would_sweep 1 entries");

    // Disk is unchanged.
    assert!(states_dir.join("alpha").is_dir());
    assert!(states_dir.join("bravo").is_dir());
    assert!(states_dir.join("orchestrate.json").exists());
    assert!(main_subdir.is_dir());
    assert!(queue_dir.join("orphan").exists());
}

#[test]
fn cleanup_all_leaves_root_dirs_standing() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    setup_flow_state(dir.path(), "test-feature", None);
    let queue_dir = dir.path().join(".flow-states/start-queue");
    fs::create_dir_all(&queue_dir).unwrap();
    fs::write(queue_dir.join("test-feature"), "").unwrap();

    let _ = run_impl_main(&args_all(dir.path(), false));

    // The directory shells survive so subsequent flow-starts do not
    // need to recreate them.
    assert!(
        dir.path().join(".flow-states").is_dir(),
        ".flow-states/ root must remain"
    );
    assert!(
        queue_dir.is_dir(),
        ".flow-states/start-queue/ root must remain"
    );
}

// --- run_impl_main validation (--all / --branch mutual exclusion) ---

#[test]
fn cleanup_neither_branch_nor_all_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        project_root: dir.path().to_string_lossy().to_string(),
        branch: None,
        worktree: None,
        pr: None,
        pull: false,
        all: false,
        dry_run: false,
    };
    let (value, code) = run_impl_main(&args);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    let msg = value["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("--branch") && msg.contains("--all"),
        "expected message to name both flags, got: {}",
        msg
    );
}

#[test]
fn cleanup_branch_without_worktree_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        project_root: dir.path().to_string_lossy().to_string(),
        branch: Some("test-feature".to_string()),
        worktree: None,
        pr: None,
        pull: false,
        all: false,
        dry_run: false,
    };
    let (value, code) = run_impl_main(&args);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    let msg = value["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("--worktree"),
        "expected message to mention --worktree, got: {}",
        msg
    );
}

// --- cleanup_all coverage gates ---

#[test]
fn cleanup_all_pr_number_passed_through() {
    // Covers the `Some(n) => Value::from(n)` arm where state.json
    // carries a pr_number.
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    setup_flow_state(dir.path(), "with-pr", Some(1234));

    let value = run_impl_main(&args_all(dir.path(), true)).0;
    let flows = value["flows"].as_array().unwrap();
    assert_eq!(flows.len(), 1);
    assert_eq!(flows[0]["pr_number"], 1234);
}

#[test]
fn cleanup_all_state_json_unreadable_reports_read_error() {
    // Covers the `Err(e) => Err(format!("read error: ..."))` arm in
    // the per-flow walk: state.json exists but cannot be read.
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let branch_dir = dir.path().join(".flow-states/unreadable");
    fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    fs::write(&state_path, "{}").unwrap();
    fs::set_permissions(&state_path, fs::Permissions::from_mode(0o000)).unwrap();

    let value = run_impl_main(&args_all(dir.path(), true)).0;

    // Restore so TempDir can drop.
    fs::set_permissions(&state_path, fs::Permissions::from_mode(0o644)).unwrap();

    let flows = value["flows"].as_array().unwrap();
    let bad = flows
        .iter()
        .find(|f| f["branch"] == "unreadable")
        .expect("flow must appear in flows[]");
    let err = bad["error"].as_str().unwrap_or("");
    assert!(
        err.starts_with("read error:"),
        "expected read error message, got: {}",
        err
    );
}

#[test]
fn cleanup_all_dry_run_reports_skipped_when_tail_artifacts_absent() {
    // Covers the dry_run==true + file/dir absent branches for
    // orchestrate_json and main_dir tail steps.
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let states_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&states_dir).unwrap();
    // No orchestrate.json, no main/ subdir, no start-queue/ entries.

    let value = run_impl_main(&args_all(dir.path(), true)).0;
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["orchestrate_json"], "skipped");
    assert_eq!(value["main_dir"], "skipped");
    assert_eq!(value["queue_sweep"], "skipped");
}

#[test]
fn cleanup_all_orchestrate_json_remove_fails() {
    // Covers the orchestrate.json `Err(e) => format!("failed: ...")`
    // arm: file exists, but parent directory is read-only so
    // `remove_file` fails with EACCES.
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let states_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&states_dir).unwrap();
    fs::write(states_dir.join("orchestrate.json"), "{}").unwrap();
    fs::set_permissions(&states_dir, fs::Permissions::from_mode(0o500)).unwrap();

    let value = run_impl_main(&args_all(dir.path(), false)).0;

    // Restore for TempDir cleanup.
    fs::set_permissions(&states_dir, fs::Permissions::from_mode(0o755)).unwrap();

    let oj = value["orchestrate_json"].as_str().unwrap();
    assert!(
        oj.starts_with("failed:"),
        "expected failed orchestrate_json, got: {}",
        oj
    );
}

#[test]
fn cleanup_all_main_dir_remove_fails() {
    // Covers the main_dir `Err(e) => format!("failed: ...")` arm.
    // `.flow-states/main/` exists with an inner file, and
    // `.flow-states/main/` is read-only so `remove_dir_all` cannot
    // unlink the inner file.
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let main_subdir = dir.path().join(".flow-states/main");
    fs::create_dir_all(&main_subdir).unwrap();
    fs::write(main_subdir.join("ci-passed"), "snapshot").unwrap();
    fs::set_permissions(&main_subdir, fs::Permissions::from_mode(0o500)).unwrap();

    let value = run_impl_main(&args_all(dir.path(), false)).0;

    // Restore for TempDir cleanup.
    fs::set_permissions(&main_subdir, fs::Permissions::from_mode(0o755)).unwrap();

    let md = value["main_dir"].as_str().unwrap();
    assert!(
        md.starts_with("failed:"),
        "expected failed main_dir, got: {}",
        md
    );
}

#[test]
fn cleanup_all_states_dir_unreadable_skips_per_flow_walk() {
    // Covers the `if let Ok(entries) = fs::read_dir(&states_dir)`
    // Err arm. With states_dir at 0o000, `is_dir()` still returns
    // true (inode stat passes through the parent's exec bit), but
    // `read_dir` fails with EACCES, so the per-flow walk is skipped
    // and `flows[]` is empty.
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let states_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&states_dir).unwrap();
    fs::set_permissions(&states_dir, fs::Permissions::from_mode(0o000)).unwrap();

    let value = run_impl_main(&args_all(dir.path(), false)).0;

    // Restore for TempDir cleanup.
    fs::set_permissions(&states_dir, fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(value["status"], "ok");
    assert_eq!(value["flows"].as_array().unwrap().len(), 0);
}

#[test]
fn cleanup_all_queue_sweep_total_failure() {
    // Covers the queue_sweep failed-everything path: read_dir
    // succeeds (queue_dir is r-x), but every fs::remove_file fails
    // because the parent has no write bit. Two files exercise the
    // `if first_err.is_none()` branch (true on iter 1, false on
    // iter 2) AND the `count == 0` path that produces "failed: ...".
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let queue_dir = dir.path().join(".flow-states/start-queue");
    fs::create_dir_all(&queue_dir).unwrap();
    fs::write(queue_dir.join("a"), "").unwrap();
    fs::write(queue_dir.join("b"), "").unwrap();
    fs::set_permissions(&queue_dir, fs::Permissions::from_mode(0o500)).unwrap();

    let value = run_impl_main(&args_all(dir.path(), false)).0;

    // Restore for TempDir cleanup.
    fs::set_permissions(&queue_dir, fs::Permissions::from_mode(0o755)).unwrap();

    let qs = value["queue_sweep"].as_str().unwrap();
    assert!(
        qs.starts_with("failed:"),
        "expected total failure with two unremovable entries, got: {}",
        qs
    );
}

// --- is_safe_worktree_rel rejection paths (Code Review fixes) ---

/// Helper for the rejection tests: run cleanup_all over a single
/// flow with the given malformed worktree value and assert the
/// flow's entry carries an `error` field starting with the rejection
/// prefix. The validator is private so tests drive through the
/// public `cleanup_all` surface.
fn assert_worktree_rejected(worktree: Value, branch: &str) {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let branch_dir = dir.path().join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    let mut state = json!({"branch": branch, "base_branch": "main"});
    state["worktree"] = worktree;
    fs::write(branch_dir.join("state.json"), state.to_string()).unwrap();

    let value = run_impl_main(&args_all(dir.path(), false)).0;
    let flows = value["flows"].as_array().unwrap();
    let flow = flows
        .iter()
        .find(|f| f["branch"] == branch)
        .expect("flow must appear in flows[]");
    assert!(
        flow["error"]
            .as_str()
            .unwrap_or("")
            .starts_with("rejected worktree path:"),
        "expected rejection error, got: {}",
        flow
    );
    assert!(
        flow.get("steps").is_none(),
        "rejected flow must not carry steps; got: {}",
        flow
    );
}

#[test]
fn cleanup_all_rejects_empty_worktree() {
    assert_worktree_rejected(json!(""), "empty-wt");
}

#[test]
fn cleanup_all_rejects_worktree_with_nul_byte() {
    assert_worktree_rejected(json!(".worktrees/foo\u{0}bar"), "nul-wt");
}

#[test]
fn cleanup_all_rejects_absolute_worktree_path() {
    assert_worktree_rejected(json!("/etc/passwd"), "absolute-wt");
}

#[test]
fn cleanup_all_rejects_worktree_with_dotdot_segment() {
    assert_worktree_rejected(json!(".worktrees/../sibling"), "dotdot-wt");
}

#[test]
fn cleanup_all_rejects_worktree_with_dot_segment() {
    assert_worktree_rejected(json!(".worktrees/./foo"), "dot-wt");
}

// --- run_impl_main mutual-exclusion errors (Code Review fixes) ---

#[test]
fn cleanup_all_with_branch_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        project_root: dir.path().to_string_lossy().to_string(),
        branch: Some("test".to_string()),
        worktree: None,
        pr: None,
        pull: false,
        all: true,
        dry_run: false,
    };
    let (value, code) = run_impl_main(&args);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    assert!(value["message"].as_str().unwrap_or("").contains("--branch"));
}

#[test]
fn cleanup_all_with_worktree_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        project_root: dir.path().to_string_lossy().to_string(),
        branch: None,
        worktree: Some(".worktrees/test".to_string()),
        pr: None,
        pull: false,
        all: true,
        dry_run: false,
    };
    let (value, code) = run_impl_main(&args);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    assert!(value["message"]
        .as_str()
        .unwrap_or("")
        .contains("--worktree"));
}

#[test]
fn cleanup_all_with_pr_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        project_root: dir.path().to_string_lossy().to_string(),
        branch: None,
        worktree: None,
        pr: Some(123),
        pull: false,
        all: true,
        dry_run: false,
    };
    let (value, code) = run_impl_main(&args);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    assert!(value["message"].as_str().unwrap_or("").contains("--pr"));
}

#[test]
fn cleanup_all_with_pull_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        project_root: dir.path().to_string_lossy().to_string(),
        branch: None,
        worktree: None,
        pr: None,
        pull: true,
        all: true,
        dry_run: false,
    };
    let (value, code) = run_impl_main(&args);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    assert!(value["message"].as_str().unwrap_or("").contains("--pull"));
}

#[test]
fn cleanup_dry_run_without_all_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        project_root: dir.path().to_string_lossy().to_string(),
        branch: Some("test".to_string()),
        worktree: Some(".worktrees/test".to_string()),
        pr: None,
        pull: false,
        all: false,
        dry_run: true,
    };
    let (value, code) = run_impl_main(&args);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    assert!(value["message"]
        .as_str()
        .unwrap_or("")
        .contains("--dry-run"));
}

/// Covers the inner `Err(e) => Err(format!("read error: ..."))` arm
/// of cleanup_all's byte-capped read where `fs::File::open` succeeds
/// but `read_to_string` fails. Invalid UTF-8 in state.json triggers
/// `read_to_string` failure (the function requires valid UTF-8) while
/// `File::open` returns Ok.
#[test]
fn cleanup_all_state_json_invalid_utf8_reports_read_error() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let branch_dir = dir.path().join(".flow-states/invalid-utf8");
    fs::create_dir_all(&branch_dir).unwrap();
    // 0xFF is an invalid UTF-8 start byte. fs::File::open succeeds;
    // read_to_string fails with InvalidData.
    fs::write(branch_dir.join("state.json"), [0xFFu8, 0xFEu8, 0xFFu8]).unwrap();

    let value = run_impl_main(&args_all(dir.path(), false)).0;
    let flows = value["flows"].as_array().unwrap();
    let flow = flows
        .iter()
        .find(|f| f["branch"] == "invalid-utf8")
        .unwrap();
    let err = flow["error"].as_str().unwrap_or("");
    assert!(
        err.starts_with("read error:"),
        "expected read-error from inner read_to_string failure, got: {}",
        err
    );
}

// --- tolerant_i64_opt for pr_number string fixture (Code Review F2 fix) ---

#[test]
fn cleanup_all_pr_number_string_coerces_via_tolerant_i64() {
    // Per .claude/rules/state-files.md "Counter and State Field Type
    // Tolerance", consumers must accept int, float, and string
    // representations. A state file with `"pr_number": "5678"`
    // (string) is now coerced to Some(5678) instead of being silently
    // dropped.
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let branch_dir = dir.path().join(".flow-states/string-pr");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(
        branch_dir.join("state.json"),
        json!({
            "branch": "string-pr",
            "worktree": ".worktrees/string-pr",
            "base_branch": "main",
            "pr_number": "5678",
        })
        .to_string(),
    )
    .unwrap();

    let value = run_impl_main(&args_all(dir.path(), true)).0;
    let flows = value["flows"].as_array().unwrap();
    let flow = flows.iter().find(|f| f["branch"] == "string-pr").unwrap();
    assert_eq!(
        flow["pr_number"], 5678,
        "pr_number string must coerce to integer"
    );
}

// --- delete_adversarial_probe ---
//
// Phase 6 cleanup explicitly disposes of the Code Review adversarial
// probe before `git worktree remove` removes the worktree directory.
// The step's outcome surfaces in the `steps` JSON as
// `"adversarial_probe"` so users have an audit-trail entry of the
// disposal. The probe path is resolved by spawning the worktree's
// `bin/test --adversarial-path`; the file is removed via
// `fs::remove_file` (no permission allow-list dependency, idempotent
// on `NotFound`).
//
// Test coverage maps to the documented outcomes:
// - `"deleted"` — probe present in worktree, file removed
// - `"missing"` — bin/test resolves a path but file not present
// - `"skipped"` — bin/test exits 2 (unconfigured), bin/test missing,
//   or worktree directory missing
// - subdirectory variant per EXCLUDE_ENTRIES (RSpec/Rails) handled

/// Write an executable `bin/test` that prints `path` on
/// `--adversarial-path` and exits 0. Simulates a configured project.
fn write_bin_test_with_adversarial_path(worktree: &Path, path: &str) {
    let bin_dir = worktree.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let script = format!(
        "#!/usr/bin/env bash\nset -eu\nif [ \"${{1:-}}\" = \"--adversarial-path\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nexit 0\n",
        path
    );
    let bin_test = bin_dir.join("test");
    fs::write(&bin_test, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(&bin_test).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&bin_test, perms).unwrap();
}

/// Write an executable `bin/test` that exits 2 with a stderr
/// message — simulates an unconfigured stub the user has not yet
/// set up.
fn write_bin_test_unconfigured(worktree: &Path) {
    let bin_dir = worktree.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let script = "#!/usr/bin/env bash\nif [ \"${1:-}\" = \"--adversarial-path\" ]; then\n  printf 'unconfigured\\n' 1>&2\n  exit 2\nfi\nexit 0\n";
    let bin_test = bin_dir.join("test");
    fs::write(&bin_test, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(&bin_test).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&bin_test, perms).unwrap();
}

#[test]
fn cleanup_deletes_adversarial_probe_when_present() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let wt = dir.path().join(&wt_rel);

    write_bin_test_with_adversarial_path(&wt, "tests/test_adversarial_flow.rs");

    let probe = wt.join("tests/test_adversarial_flow.rs");
    fs::create_dir_all(probe.parent().unwrap()).unwrap();
    fs::write(&probe, "// adversarial probe\n").unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(
        steps["adversarial_probe"], "deleted",
        "probe present in worktree must be removed by delete_adversarial_probe"
    );
}

#[test]
fn cleanup_adversarial_probe_in_subdirectory_variant_deleted() {
    // Subdirectory variant per `EXCLUDE_ENTRIES` (RSpec/Rails layout).
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let wt = dir.path().join(&wt_rel);

    write_bin_test_with_adversarial_path(&wt, "spec/adversarial_flow_spec.rb");

    let probe = wt.join("spec/adversarial_flow_spec.rb");
    fs::create_dir_all(probe.parent().unwrap()).unwrap();
    fs::write(&probe, "# adversarial probe\n").unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(
        steps["adversarial_probe"], "deleted",
        "subdirectory-variant probe must be removed via the bin/test-resolved path"
    );
}

#[test]
fn cleanup_adversarial_probe_missing_when_path_resolves_but_file_absent() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let wt = dir.path().join(&wt_rel);

    write_bin_test_with_adversarial_path(&wt, "tests/test_adversarial_flow.rs");
    // No probe file created — bin/test resolves a path but it is absent.

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(
        steps["adversarial_probe"], "missing",
        "resolved path with no probe file must report \"missing\""
    );
}

#[test]
fn cleanup_adversarial_probe_skipped_when_bin_test_exits_two() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let wt = dir.path().join(&wt_rel);

    write_bin_test_unconfigured(&wt);

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(
        steps["adversarial_probe"], "skipped",
        "unconfigured bin/test (exit 2) must skip the probe step"
    );
}

#[test]
fn cleanup_adversarial_probe_skipped_when_bin_test_missing() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    // No bin/test in the worktree.

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(
        steps["adversarial_probe"], "skipped",
        "missing bin/test must skip the probe step (no path to resolve)"
    );
}

#[test]
fn cleanup_adversarial_probe_skipped_when_worktree_missing() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    // setup_feature NOT called — worktree directory does not exist.
    let wt_rel = ".worktrees/test-feature";

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(
        steps["adversarial_probe"], "skipped",
        "missing worktree must skip the probe step"
    );
}

#[test]
fn cleanup_adversarial_probe_skipped_when_bin_test_prints_empty() {
    // bin/test exits 0 but prints empty stdout — protects against a
    // misconfigured project whose `bin/test --adversarial-path` is
    // wired up but returns nothing useful.
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let wt = dir.path().join(&wt_rel);

    let bin_dir = wt.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let script = "#!/usr/bin/env bash\nexit 0\n";
    let bin_test = bin_dir.join("test");
    fs::write(&bin_test, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(&bin_test).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&bin_test, perms).unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(
        steps["adversarial_probe"], "skipped",
        "empty stdout from bin/test must skip — no path to resolve"
    );
}

#[test]
fn cleanup_deletes_adversarial_probe_at_absolute_path() {
    // bin/test returns an absolute path. The cleanup step must use
    // the path verbatim (not join it onto the worktree root).
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let wt = dir.path().join(&wt_rel);

    // Pick an absolute path for the probe — inside the worktree but
    // referenced absolutely so the production code's
    // `Path::is_absolute` branch fires.
    let probe = wt.join("tests/test_adversarial_flow.rs");
    fs::create_dir_all(probe.parent().unwrap()).unwrap();
    fs::write(&probe, "// adversarial probe\n").unwrap();
    let abs_probe_str = probe.to_string_lossy().to_string();

    write_bin_test_with_adversarial_path(&wt, &abs_probe_str);

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert_eq!(
        steps["adversarial_probe"], "deleted",
        "absolute probe path must be honored verbatim"
    );
    assert!(
        !probe.exists(),
        "probe file at absolute path must be removed"
    );
}

#[test]
fn cleanup_adversarial_probe_rejects_path_traversal_via_relative_path() {
    // `bin/test --adversarial-path` prints `../../escape_target.txt`.
    // delete_adversarial_probe joins onto wt_path, producing
    // <worktree>/../../escape_target.txt which resolves to a file
    // outside the worktree. The containment guard
    // (resolve_probe_inside_worktree) must reject this rather than
    // allowing fs::remove_file to delete an out-of-worktree file.
    let dir = tempfile::tempdir().unwrap();
    let project_root = dir.path().canonicalize().unwrap();
    setup_git_repo(&project_root);
    let wt_rel = setup_feature(&project_root, "test-feature");
    let wt = project_root.join(&wt_rel);

    let escape_target = project_root.join("escape_target.txt");
    fs::write(&escape_target, "DO NOT DELETE — outside worktree\n").unwrap();
    assert!(escape_target.exists(), "fixture sentinel must exist");

    write_bin_test_with_adversarial_path(&wt, "../../escape_target.txt");

    let (value, _) = run_impl_main(&args_for(
        &project_root,
        "test-feature",
        &wt_rel,
        None,
        false,
    ));
    let steps = steps_from(&value);

    assert_eq!(
        steps["adversarial_probe"], "skipped",
        "relative-path traversal must be rejected as `skipped`, got: {}",
        steps["adversarial_probe"]
    );
    assert!(
        escape_target.exists(),
        "out-of-worktree file must still exist after the rejected probe path"
    );
}

#[test]
fn cleanup_adversarial_probe_rejects_absolute_path_outside_worktree() {
    // `bin/test --adversarial-path` prints an absolute path that is
    // outside the worktree. The absolute branch must canonicalize and
    // verify worktree containment rather than accepting the path
    // verbatim.
    let dir = tempfile::tempdir().unwrap();
    let project_root = dir.path().canonicalize().unwrap();
    setup_git_repo(&project_root);
    let wt_rel = setup_feature(&project_root, "test-feature");
    let wt = project_root.join(&wt_rel);

    let sibling_dir = dir.path().join("sibling");
    fs::create_dir_all(&sibling_dir).unwrap();
    let escape_target = sibling_dir.canonicalize().unwrap().join("escape.txt");
    fs::write(&escape_target, "outside worktree sentinel\n").unwrap();
    assert!(escape_target.exists(), "fixture sentinel must exist");

    let abs_str = escape_target.to_string_lossy().to_string();
    write_bin_test_with_adversarial_path(&wt, &abs_str);

    let (value, _) = run_impl_main(&args_for(
        &project_root,
        "test-feature",
        &wt_rel,
        None,
        false,
    ));
    let steps = steps_from(&value);

    assert_eq!(
        steps["adversarial_probe"], "skipped",
        "absolute path outside worktree must be rejected as `skipped`, got: {}",
        steps["adversarial_probe"]
    );
    assert!(
        escape_target.exists(),
        "out-of-worktree file at absolute path must still exist"
    );
}

#[test]
fn cleanup_adversarial_probe_rejects_path_terminating_in_dotdot() {
    // bin/test prints a path terminating in `..` (over a non-existent
    // intermediate component). `Path::file_name()` returns None for
    // paths terminating in `..`, so the helper bails out with None
    // rather than walking up forever or accepting the path.
    let dir = tempfile::tempdir().unwrap();
    let project_root = dir.path().canonicalize().unwrap();
    setup_git_repo(&project_root);
    let wt_rel = setup_feature(&project_root, "test-feature");
    let wt = project_root.join(&wt_rel);

    write_bin_test_with_adversarial_path(&wt, "nonexistent_dir/..");

    let (value, _) = run_impl_main(&args_for(
        &project_root,
        "test-feature",
        &wt_rel,
        None,
        false,
    ));
    let steps = steps_from(&value);

    assert_eq!(
        steps["adversarial_probe"], "skipped",
        "path terminating in `..` over non-existent components must be rejected"
    );
}

#[test]
fn cleanup_adversarial_probe_rejects_missing_path_outside_worktree() {
    // bin/test prints a path whose deepest existing ancestor is
    // outside the worktree. The walker climbs to the existing
    // ancestor, canonicalizes, re-appends the suffix — and the final
    // starts_with(wt_canon) check rejects.
    let dir = tempfile::tempdir().unwrap();
    let project_root = dir.path().canonicalize().unwrap();
    setup_git_repo(&project_root);
    let wt_rel = setup_feature(&project_root, "test-feature");
    let wt = project_root.join(&wt_rel);

    // `<wt>/../external_missing/file.txt` walks up to the project
    // root (existing), then re-appends `external_missing/file.txt`
    // to land at `<project_root>/external_missing/file.txt` — outside
    // the worktree, even though no component on the path exists yet.
    write_bin_test_with_adversarial_path(&wt, "../external_missing/file.txt");

    let (value, _) = run_impl_main(&args_for(
        &project_root,
        "test-feature",
        &wt_rel,
        None,
        false,
    ));
    let steps = steps_from(&value);

    assert_eq!(
        steps["adversarial_probe"], "skipped",
        "missing path whose canonicalized ancestor lies outside the worktree must be rejected"
    );
}

#[test]
fn cleanup_adversarial_probe_failed_when_path_is_directory() {
    // `bin/test --adversarial-path` resolves to a path that points at
    // a DIRECTORY. `fs::remove_file` returns a non-NotFound error
    // (EISDIR/EPERM depending on platform). The step must report the
    // failure rather than swallowing it as "missing" or "deleted".
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path());
    let wt_rel = setup_feature(dir.path(), "test-feature");
    let wt = dir.path().join(&wt_rel);

    write_bin_test_with_adversarial_path(&wt, "tests/probe_dir");

    // Create a directory at the resolved path.
    let probe_dir = wt.join("tests/probe_dir");
    fs::create_dir_all(&probe_dir).unwrap();

    let (value, _) = run_impl_main(&args_for(dir.path(), "test-feature", &wt_rel, None, false));
    let steps = steps_from(&value);
    assert!(
        steps["adversarial_probe"].starts_with("failed:"),
        "directory at probe path must surface as a `failed:` outcome, got: {}",
        steps["adversarial_probe"]
    );
}
