//! Subprocess tests for `bin/flow base-branch`. Mirrors
//! `src/base_branch_cmd.rs`. Each test spawns the compiled `flow-rs`
//! binary in a fixture repo and asserts stdout/stderr/exit semantics.
//!
//! `base-branch` is a thin wrapper around `git::default_branch_in` —
//! the subcommand has no `--branch` flag and reads no state files.
//! Git is the single source of truth for the integration branch.
//!
//! Subprocess hygiene per `.claude/rules/subprocess-test-hygiene.md`:
//! every spawn neutralizes `GH_TOKEN`, `HOME`, and `FLOW_CI_RUNNING`
//! to keep the child off the host's GitHub account, dotfiles, and any
//! ambient CI recursion guard.

use std::path::Path;
use std::process::{Command, Output};

/// Initialize a git repo on the named branch with one empty commit.
fn init_git_repo(dir: &Path, branch: &str) {
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git command failed");
        assert!(output.status.success(), "git {:?} failed", args);
    };
    run(&["init", "--initial-branch", branch]);
    run(&["config", "user.email", "test@test.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "commit.gpgsign", "false"]);
    run(&["commit", "--allow-empty", "-m", "init"]);
}

/// Create an `origin` remote that points at a local bare repo and set
/// `refs/remotes/origin/HEAD` to the named branch via
/// `git remote set-head`. Returns the bare-repo path so the caller can
/// keep it alive for the test duration.
fn setup_origin_with_head(repo: &Path, head_branch: &str) {
    // Create a bare repo on the same branch and add it as `origin`.
    let bare = repo.parent().unwrap().join(format!(
        "bare-{}",
        repo.file_name().unwrap().to_string_lossy()
    ));
    std::fs::create_dir_all(&bare).unwrap();
    let run = |args: &[&str], cwd: &Path| {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("git command failed");
        assert!(output.status.success(), "git {:?} failed", args);
    };
    run(&["init", "--bare", "--initial-branch", head_branch], &bare);
    // Repo points at the bare as origin and fetches it.
    run(&["remote", "add", "origin", bare.to_str().unwrap()], repo);
    // Push the current branch so origin/HEAD has something to point at.
    // First rename the local branch if needed so it matches head_branch.
    let local_branch_output = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(repo)
        .output()
        .expect("git branch --show-current");
    let current = String::from_utf8_lossy(&local_branch_output.stdout)
        .trim()
        .to_string();
    if current != head_branch {
        run(&["branch", "-m", current.as_str(), head_branch], repo);
    }
    run(&["push", "-u", "origin", head_branch], repo);
    run(&["remote", "set-head", "origin", head_branch], repo);
}

/// Run `flow-rs base-branch` in the given repo. Returns the captured Output.
fn run_base_branch(repo: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("base-branch")
        .current_dir(repo)
        .env("GH_TOKEN", "invalid")
        .env("HOME", repo)
        .env_remove("FLOW_CI_RUNNING")
        .env_remove("FLOW_SIMULATE_BRANCH")
        .output()
        .expect("spawn flow-rs base-branch")
}

#[test]
fn base_branch_subcommand_prints_default_branch_resolved_by_git() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    init_git_repo(&repo, "main");
    setup_origin_with_head(&repo, "main");

    let output = run_base_branch(&repo);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "main\n");
}

#[test]
fn base_branch_subcommand_errors_when_git_cannot_resolve() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    // No git init — `git symbolic-ref` fails outside a git repo.

    let output = run_base_branch(&repo);
    assert_ne!(
        output.status.code(),
        Some(0),
        "expected non-zero exit when git cannot resolve, got {:?}",
        output.status.code()
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked at"),
        "must not panic on git failure; stderr: {}",
        stderr
    );
    assert!(
        !stderr.is_empty(),
        "expected structured stderr message when git cannot resolve, got empty stderr"
    );
}

#[test]
fn base_branch_subcommand_rejects_branch_flag() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    init_git_repo(&repo, "main");
    setup_origin_with_head(&repo, "main");

    // Clap must reject `--branch` because it is no longer a defined arg.
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["base-branch", "--branch", "main"])
        .current_dir(&repo)
        .env("GH_TOKEN", "invalid")
        .env("HOME", &repo)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("spawn flow-rs base-branch");
    assert_ne!(
        output.status.code(),
        Some(0),
        "expected clap to reject removed --branch flag, got 0"
    );
}

#[test]
fn base_branch_subcommand_works_outside_any_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    init_git_repo(&repo, "main");
    setup_origin_with_head(&repo, "main");
    // Note: no .flow-states/ directory created — base-branch must
    // succeed because it no longer reads any state file.

    let output = run_base_branch(&repo);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "main\n");
}
