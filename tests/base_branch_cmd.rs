//! Subprocess tests for `bin/flow base-branch`. Mirrors
//! `src/base_branch_cmd.rs`. Each test spawns the compiled `flow-rs`
//! binary in a fixture git repo with a hand-written state file, and
//! asserts stdout/stderr/exit semantics.
//!
//! Subprocess hygiene per `.claude/rules/subprocess-test-hygiene.md`:
//! every spawn neutralizes `GH_TOKEN`, `HOME`, and `FLOW_CI_RUNNING`
//! to keep the child off the host's GitHub account, dotfiles, and any
//! ambient CI recursion guard.

use std::fs;
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

/// Write a minimal state file at `.flow-states/<branch>/state.json`.
fn write_state(repo: &Path, branch: &str, content: &str) {
    let dir = repo.join(".flow-states").join(branch);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("state.json"), content).unwrap();
}

/// Run `flow-rs base-branch` in the given repo with optional `--branch`
/// override. Returns the captured Output.
fn run_base_branch(repo: &Path, branch_override: Option<&str>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.arg("base-branch");
    if let Some(b) = branch_override {
        cmd.arg("--branch").arg(b);
    }
    cmd.current_dir(repo)
        .env("GH_TOKEN", "invalid")
        .env("HOME", repo)
        .env_remove("FLOW_CI_RUNNING")
        .env_remove("FLOW_SIMULATE_BRANCH")
        .output()
        .expect("spawn flow-rs base-branch")
}

#[test]
fn base_branch_subcommand_prints_value_when_state_present_main() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    init_git_repo(&repo, "feature");
    write_state(&repo, "feature", r#"{"base_branch": "main"}"#);

    let output = run_base_branch(&repo, None);
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
fn base_branch_subcommand_prints_value_when_state_present_staging() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    init_git_repo(&repo, "feature");
    write_state(&repo, "feature", r#"{"base_branch": "staging"}"#);

    let output = run_base_branch(&repo, None);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "staging\n");
}

#[test]
fn base_branch_subcommand_with_branch_flag_resolves_to_named_branch_state() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    // Repo is on branch "current" but state file lives under "other".
    init_git_repo(&repo, "current");
    write_state(&repo, "other", r#"{"base_branch": "develop"}"#);

    let output = run_base_branch(&repo, Some("other"));
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "develop\n");
}

#[test]
fn base_branch_subcommand_errs_when_no_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    init_git_repo(&repo, "feature");
    // No state file written.

    let output = run_base_branch(&repo, None);
    assert_ne!(
        output.status.code(),
        Some(0),
        "expected non-zero exit, got {:?}",
        output.status.code()
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.is_empty(),
        "expected structured stderr message when state file missing, got empty stderr"
    );
}

#[test]
fn base_branch_subcommand_errs_when_state_corrupt() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    init_git_repo(&repo, "feature");
    write_state(&repo, "feature", "{ not valid json");

    let output = run_base_branch(&repo, None);
    assert_ne!(
        output.status.code(),
        Some(0),
        "expected non-zero exit on corrupt state, got {:?}",
        output.status.code()
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.is_empty(),
        "expected structured stderr message on corrupt state, got empty stderr"
    );
}

#[test]
fn base_branch_subcommand_errs_when_state_empty() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    init_git_repo(&repo, "feature");
    write_state(&repo, "feature", "");

    let output = run_base_branch(&repo, None);
    assert_ne!(
        output.status.code(),
        Some(0),
        "expected non-zero exit on empty state, got {:?}",
        output.status.code()
    );
}

#[test]
fn base_branch_subcommand_errs_when_field_wrong_type() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    init_git_repo(&repo, "feature");
    write_state(&repo, "feature", r#"{"base_branch": 42}"#);

    let output = run_base_branch(&repo, None);
    assert_ne!(
        output.status.code(),
        Some(0),
        "expected non-zero exit on wrong-type field, got {:?}",
        output.status.code()
    );
}

/// Branch overrides accepted on the CLI come from outside the process
/// (the shell can pass any string), so the subcommand must validate
/// the branch before joining it onto `.flow-states/`. Slash-containing
/// branches must produce a structured error, never a panic. Per
/// `.claude/rules/external-input-validation.md` and
/// `.claude/rules/branch-path-safety.md`, the subcommand uses
/// `FlowPaths::try_new` with a structured error on `None`.
#[test]
fn base_branch_subcommand_errs_when_branch_flag_is_slash_branch() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    init_git_repo(&repo, "feature");

    let output = run_base_branch(&repo, Some("feature/foo"));
    assert_ne!(
        output.status.code(),
        Some(0),
        "expected non-zero exit on slash-branch, got {:?}",
        output.status.code()
    );
    // Must not panic — stderr should be a structured message, not a
    // Rust backtrace.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked at"),
        "slash-branch must not panic; stderr: {}",
        stderr
    );
}

/// When no `--branch` override is given and the repo has no current
/// branch (non-git directory), `resolve_branch` returns `None` and
/// the subcommand exits with a "Could not determine current branch"
/// message. Covers the `resolve_branch` None arm of `run_impl_main`.
#[test]
fn base_branch_subcommand_errs_when_no_current_branch() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    // No `git init` — current_branch returns None.

    let output = run_base_branch(&repo, None);
    assert_ne!(
        output.status.code(),
        Some(0),
        "expected non-zero exit when no current branch, got {:?}",
        output.status.code()
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Could not determine current branch"),
        "expected 'Could not determine current branch' in stderr, got: {}",
        stderr
    );
}

#[test]
fn base_branch_subcommand_errs_when_branch_flag_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    init_git_repo(&repo, "feature");

    let output = run_base_branch(&repo, Some(""));
    assert_ne!(
        output.status.code(),
        Some(0),
        "expected non-zero exit on empty --branch, got {:?}",
        output.status.code()
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked at"),
        "empty --branch must not panic; stderr: {}",
        stderr
    );
}
