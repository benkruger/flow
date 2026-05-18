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

/// Create a git repo WITHOUT `refs/remotes/origin/HEAD`, so
/// `default_branch_in` returns Err. Used by the fail-closed tests
/// that exercise the new resolve_base_branch error path.
fn setup_git_repo_no_origin_head(dir: &Path) {
    StdCommand::new("git")
        .args(["init", "--initial-branch", "main"])
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
    // Deliberately no symbolic-ref setup.
}

#[test]
fn cleanup_returns_failed_pull_step_when_default_branch_resolve_fails() {
    // Per-branch cleanup with --pull → default_branch_in fails to
    // resolve the integration branch. Cleanup must still complete
    // (worktree removal, branch deletion, etc.); the pull step
    // surfaces the resolve failure as `git_pull: "failed: ..."`
    // rather than aborting all cleanup.
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo_no_origin_head(dir.path());
    let _wt_rel = setup_feature(dir.path(), "test-feature");

    let (value, code) = run_impl_main(&args_for(
        dir.path(),
        "test-feature",
        ".worktrees/test-feature",
        None,
        true,
    ));
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    let git_pull = value["steps"]["git_pull"].as_str().unwrap_or("");
    assert!(
        git_pull.starts_with("failed: cannot resolve integration branch"),
        "git_pull must surface the resolve failure; got: {}",
        git_pull
    );
}

#[test]
fn cleanup_without_pull_succeeds_when_default_branch_resolve_fails() {
    // Per-branch cleanup without --pull → `default_branch_in` is not
    // called at all (no consumer of `base_branch` runs). Cleanup
    // completes normally even on repos with no origin/HEAD.
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo_no_origin_head(dir.path());
    let _wt_rel = setup_feature(dir.path(), "test-feature");

    let (value, code) = run_impl_main(&args_for(
        dir.path(),
        "test-feature",
        ".worktrees/test-feature",
        None,
        false,
    ));
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
}

/// Create a minimal git repo for testing. Configures
/// `refs/remotes/origin/HEAD` to point at `refs/remotes/origin/main`
/// so `git::default_branch_in` can resolve the integration branch via
/// `git symbolic-ref --short refs/remotes/origin/HEAD`.
fn setup_git_repo(dir: &Path) {
    StdCommand::new("git")
        .args(["init", "--initial-branch", "main"])
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
    // Synthesize `refs/remotes/origin/main` and the symbolic-ref HEAD so
    // `default_branch_in` returns "main" without requiring a real remote.
    let head_sha = String::from_utf8(
        StdCommand::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(dir)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();
    let remote_dir = dir.join(".git/refs/remotes/origin");
    fs::create_dir_all(&remote_dir).unwrap();
    fs::write(remote_dir.join("main"), format!("{}\n", head_sha)).unwrap();
    StdCommand::new("git")
        .args([
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ])
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

/// Prove that `cleanup` resolves the integration branch via
/// `git::default_branch_in` rather than from any state-file field.
/// The bare remote has only `main`; we configure origin/HEAD to
/// point at a `staging` branch (synthesized locally) so the cleanup
/// step's `git pull origin staging` fails — the failure stderr
/// carrying "staging" proves the git-resolved branch reached the
/// pull call.
#[test]
fn cleanup_pulls_default_branch_resolved_by_git() {
    let tmp = tempfile::tempdir().unwrap();
    let parent = tmp.path().canonicalize().unwrap();
    let repo = common::create_git_repo_with_remote(&parent);

    // Repoint origin/HEAD locally at refs/remotes/origin/staging without
    // pushing a real staging branch to the bare remote — pull will fail.
    StdCommand::new("git")
        .args(["update-ref", "refs/remotes/origin/staging", "HEAD"])
        .current_dir(&repo)
        .output()
        .unwrap();
    StdCommand::new("git")
        .args([
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/staging",
        ])
        .current_dir(&repo)
        .output()
        .unwrap();

    // Worktree on a feature branch.
    let wt_rel = ".worktrees/test-feature".to_string();
    StdCommand::new("git")
        .args(["worktree", "add", &wt_rel, "-b", "test-feature"])
        .current_dir(&repo)
        .output()
        .unwrap();

    let branch_dir = repo.join(".flow-states").join("test-feature");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(
        branch_dir.join("state.json"),
        json!({"branch": "test-feature"}).to_string(),
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
        "git_pull step must reference 'staging' to prove git-resolved branch flowed through, got: {}",
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

    // PATH with only git available (no gh). default_branch_in
    // succeeds via real git; the cleanup's gh subprocess call
    // (for --pr 999) fails to spawn — exercises run_cmd's Err
    // branch.
    let git_only_bin = root.join("git-only-bin");
    fs::create_dir_all(&git_only_bin).unwrap();
    let real_git = std::process::Command::new("which")
        .arg("git")
        .output()
        .unwrap();
    let real_git_path = String::from_utf8_lossy(&real_git.stdout).trim().to_string();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real_git_path, git_only_bin.join("git")).unwrap();

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
        .env("PATH", &git_only_bin)
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
        "expected pr_close to fail on gh spawn failure, got: {}",
        pr_close
    );
}

// --- run_impl_main validation ---

#[test]
fn cleanup_missing_branch_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        project_root: dir.path().to_string_lossy().to_string(),
        branch: None,
        worktree: None,
        pr: None,
        pull: false,
    };
    let (value, code) = run_impl_main(&args);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    let msg = value["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("--branch"),
        "expected message to mention --branch, got: {}",
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

// --- delete_adversarial_probe ---
//
// Phase 6 cleanup explicitly disposes of the Review adversarial
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
