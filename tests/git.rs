//! Integration tests for `src/git.rs`. Exercises the pure helpers
//! (`project_root_with_stdout`, `project_root_from_output`,
//! `current_branch_from_output`, `resolve_branch_impl`) directly, plus
//! the subprocess-backed public wrappers against real git fixtures.

use std::fs;
use std::io;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output};

use flow_rs::git::{
    current_branch, current_branch_from_output, current_branch_in, project_root,
    project_root_from_output, project_root_with_stdout, resolve_branch, resolve_branch_impl,
    resolve_branch_in,
};

fn ok_output(stdout: &str) -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: stdout.as_bytes().to_vec(),
        stderr: vec![],
    }
}

fn fail_output(stderr: &str) -> Output {
    Output {
        status: ExitStatus::from_raw(128 << 8),
        stdout: vec![],
        stderr: stderr.as_bytes().to_vec(),
    }
}

fn spawn_error() -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, "git not found")
}

/// Initialize a git repo in the given directory with an initial commit
/// on the named branch.
fn init_git_repo(dir: &Path, initial_branch: &str) {
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git command failed");
        assert!(output.status.success(), "git {:?} failed", args);
    };
    run(&["init", "--initial-branch", initial_branch]);
    run(&["config", "user.email", "test@test.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "commit.gpgsign", "false"]);
    run(&["commit", "--allow-empty", "-m", "init"]);
}

// --- project_root_with_stdout ---

#[test]
fn project_root_with_stdout_extracts_first_worktree_line() {
    let stdout = "worktree /path/to/repo\nHEAD abc123\nbranch refs/heads/main\n\nworktree /path/to/other\nHEAD def456\nbranch refs/heads/feature\n";
    assert_eq!(
        project_root_with_stdout(stdout),
        PathBuf::from("/path/to/repo")
    );
}

#[test]
fn project_root_with_stdout_no_worktree_line_returns_dot() {
    let stdout = "HEAD abc123\nbranch refs/heads/main\n";
    assert_eq!(project_root_with_stdout(stdout), PathBuf::from("."));
}

#[test]
fn project_root_with_stdout_empty_returns_dot() {
    assert_eq!(project_root_with_stdout(""), PathBuf::from("."));
}

#[test]
fn project_root_with_stdout_trims_trailing_whitespace() {
    assert_eq!(
        project_root_with_stdout("worktree /path/with/trailing   \n"),
        PathBuf::from("/path/with/trailing")
    );
}

// --- project_root_from_output ---

#[test]
fn project_root_from_output_success_parses_stdout() {
    let output = ok_output("worktree /a/b/c\n");
    assert_eq!(
        project_root_from_output(Ok(output)),
        PathBuf::from("/a/b/c")
    );
}

#[test]
fn project_root_from_output_non_success_returns_dot() {
    // git exited non-zero (e.g. "not a git repo"). Fallback to ".".
    assert_eq!(
        project_root_from_output(Ok(fail_output("fatal: not a git repo"))),
        PathBuf::from(".")
    );
}

#[test]
fn project_root_from_output_spawn_error_returns_dot() {
    // git missing from PATH. Fallback to ".".
    assert_eq!(
        project_root_from_output(Err(spawn_error())),
        PathBuf::from(".")
    );
}

// --- project_root (subprocess) ---

#[test]
fn project_root_in_real_repo_returns_existing_path() {
    let root = project_root();
    assert!(root.exists() || root == Path::new("."));
}

// --- current_branch_from_output ---

#[test]
fn current_branch_from_output_simulated_non_empty_short_circuits() {
    // Should not even look at `output` — pass an Err and confirm the
    // simulated value still wins.
    let result = current_branch_from_output(Some("main".to_string()), Err(spawn_error()));
    assert_eq!(result, Some("main".to_string()));
}

#[test]
fn current_branch_from_output_simulated_empty_falls_through() {
    let out = ok_output("from-git\n");
    let result = current_branch_from_output(Some(String::new()), Ok(out));
    assert_eq!(result, Some("from-git".to_string()));
}

#[test]
fn current_branch_from_output_none_simulated_uses_output() {
    let out = ok_output("from-git\n");
    let result = current_branch_from_output(None, Ok(out));
    assert_eq!(result, Some("from-git".to_string()));
}

#[test]
fn current_branch_from_output_spawn_error_returns_none() {
    let result = current_branch_from_output(None, Err(spawn_error()));
    assert_eq!(result, None);
}

#[test]
fn current_branch_from_output_non_success_returns_none() {
    let result = current_branch_from_output(None, Ok(fail_output("detached?")));
    assert_eq!(result, None);
}

#[test]
fn current_branch_from_output_empty_stdout_returns_none() {
    // Detached HEAD: git succeeds but prints an empty branch.
    let result = current_branch_from_output(None, Ok(ok_output("\n")));
    assert_eq!(result, None);
}

// --- current_branch (subprocess) ---

#[test]
fn current_branch_in_real_repo_returns_without_panic() {
    // Process cwd is the flow repo. current_branch queries git; the
    // exact branch depends on the test harness state.
    let _ = current_branch();
}

// --- current_branch_in ---

#[test]
fn current_branch_in_reads_cwd_repo() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "my-feature");
    let branch = current_branch_in(dir.path());
    assert_eq!(branch, Some("my-feature".to_string()));
}

#[test]
fn current_branch_in_detached_head() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "main");
    let sha = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let sha = String::from_utf8_lossy(&sha.stdout).trim().to_string();
    let output = Command::new("git")
        .args(["checkout", &sha])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let branch = current_branch_in(dir.path());
    assert_eq!(branch, None);
}

#[test]
fn current_branch_in_non_git_dir_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let branch = current_branch_in(dir.path());
    assert_eq!(branch, None);
}

// --- resolve_branch_impl ---

#[test]
fn resolve_branch_impl_override_wins() {
    let dir = tempfile::tempdir().unwrap();
    let result = resolve_branch_impl(Some("explicit"), dir.path(), Some("ignored".to_string()));
    assert_eq!(result, Some("explicit".to_string()));
}

#[test]
fn resolve_branch_impl_state_file_exists_returns_branch() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();
    fs::write(
        state_dir.join("test-branch.json"),
        r#"{"branch": "test-branch"}"#,
    )
    .unwrap();
    let result = resolve_branch_impl(None, dir.path(), Some("test-branch".to_string()));
    assert_eq!(result, Some("test-branch".to_string()));
}

#[test]
fn resolve_branch_impl_no_state_file_returns_current_branch() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir(&state_dir).unwrap();
    fs::write(state_dir.join("other-a.json"), r#"{"branch": "a"}"#).unwrap();
    fs::write(state_dir.join("other-b.json"), r#"{"branch": "b"}"#).unwrap();

    let result = resolve_branch_impl(None, dir.path(), Some("main".to_string()));
    assert_eq!(result, Some("main".to_string()));
}

#[test]
fn resolve_branch_impl_slash_branch_falls_through() {
    // FlowPaths::try_new rejects slash-containing branches.
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".flow-states")).unwrap();
    let result = resolve_branch_impl(None, dir.path(), Some("feature/foo".to_string()));
    assert_eq!(result, Some("feature/foo".to_string()));
}

#[test]
fn resolve_branch_impl_none_branch_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let result = resolve_branch_impl(None, dir.path(), None);
    assert_eq!(result, None);
}

// --- resolve_branch (public wrapper) ---

#[test]
fn resolve_branch_override_wins() {
    let dir = tempfile::tempdir().unwrap();
    let branch = resolve_branch(Some("explicit-branch"), dir.path());
    assert_eq!(branch, Some("explicit-branch".to_string()));
}

// --- resolve_branch_in ---

#[test]
fn resolve_branch_in_override_wins() {
    let repo = tempfile::tempdir().unwrap();
    init_git_repo(repo.path(), "main");
    let root = tempfile::tempdir().unwrap();
    let branch = resolve_branch_in(Some("explicit"), repo.path(), root.path());
    assert_eq!(branch, Some("explicit".to_string()));
}

#[test]
fn resolve_branch_in_reads_branch_from_cwd() {
    let repo = tempfile::tempdir().unwrap();
    init_git_repo(repo.path(), "cwd-branch");
    let root = tempfile::tempdir().unwrap();
    let branch = resolve_branch_in(None, repo.path(), root.path());
    assert_eq!(branch, Some("cwd-branch".to_string()));
}

#[test]
fn resolve_branch_in_matches_state_file() {
    let repo = tempfile::tempdir().unwrap();
    init_git_repo(repo.path(), "matched");
    let root = tempfile::tempdir().unwrap();
    let state_dir = root.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("matched.json"), r#"{"branch": "matched"}"#).unwrap();

    let branch = resolve_branch_in(None, repo.path(), root.path());
    assert_eq!(branch, Some("matched".to_string()));
}
