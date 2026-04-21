//! Integration tests for `flow_rs::finalize_commit` — drives the public
//! surface (`finalize_commit_inner` with mock git, `run_impl` with real git,
//! `run_impl_main` via the compiled binary). Private helpers
//! (`remove_message_file`, `emit_deviation_stderr`, `run_git_with_timeout`,
//! `run_git_in_dir`) are exercised indirectly through these entry points.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::process::{Command, Stdio};

use flow_rs::finalize_commit::{finalize_commit_inner, run_impl, Args};
use serde_json::{json, Value};

type GitResult = Result<(i32, String, String), String>;

fn mock_git(responses: Vec<GitResult>) -> impl Fn(&[&str], u64) -> GitResult {
    let queue = RefCell::new(VecDeque::from(responses));
    move |_args: &[&str], _timeout: u64| -> GitResult {
        queue
            .borrow_mut()
            .pop_front()
            .expect("no more mock responses")
    }
}

fn ok(stdout: &str) -> GitResult {
    Ok((0, stdout.to_string(), String::new()))
}

fn fail(stderr: &str) -> GitResult {
    Ok((1, String::new(), stderr.to_string()))
}

fn timeout(msg: &str) -> GitResult {
    Err(msg.to_string())
}

/// Assert a git command succeeded. Panics with stderr on failure.
fn git_assert_ok(output: &std::process::Output) {
    let code = output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "git failed (exit {}): {}",
        code,
        stderr
    );
}

// --- finalize_commit_inner (mock git) ---

#[test]
fn happy_path() {
    let dir = tempfile::tempdir().unwrap();
    let msg = dir.path().join(".flow-commit-msg");
    fs::write(&msg, "Test commit.").unwrap();

    let git = mock_git(vec![
        ok(""),         // git commit
        ok("abc123\n"), // git rev-parse HEAD (post-commit)
        ok(""),         // git pull
        ok(""),         // git push
        ok("abc123\n"), // git rev-parse HEAD (final)
    ]);

    let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
    assert_eq!(result["status"], "ok");
    assert_eq!(result["sha"], "abc123");
    assert_eq!(result["pull_merged"], false);
    // Also covers remove_message_file (unlinks the msg file after commit).
    assert!(!msg.exists());
}

#[test]
fn commit_failure() {
    let dir = tempfile::tempdir().unwrap();
    let msg = dir.path().join(".flow-commit-msg");
    fs::write(&msg, "Test commit.").unwrap();

    let git = mock_git(vec![fail("nothing to commit")]);

    let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
    assert_eq!(result["status"], "error");
    assert_eq!(result["step"], "commit");
    assert_eq!(result["message"], "nothing to commit");
    assert!(!msg.exists());
}

#[test]
fn pull_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let msg = dir.path().join(".flow-commit-msg");
    fs::write(&msg, "Test commit.").unwrap();

    let git = mock_git(vec![
        ok(""),                                                           // git commit
        ok("commit_sha\n"), // git rev-parse HEAD (post-commit)
        fail("CONFLICT"),   // git pull
        Ok((0, "UU file1.py\nAA file2.py\n".to_string(), String::new())), // git status
    ]);

    let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
    assert_eq!(result["status"], "conflict");
    let files: Vec<String> = result["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(files, vec!["file1.py", "file2.py"]);
}

#[test]
fn pull_error_non_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let msg = dir.path().join(".flow-commit-msg");
    fs::write(&msg, "Test commit.").unwrap();

    let git = mock_git(vec![
        ok(""),                         // git commit
        ok("commit_sha\n"),             // git rev-parse HEAD (post-commit)
        fail("Could not resolve host"), // git pull
        ok(""),                         // git status (clean)
    ]);

    let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
    assert_eq!(result["status"], "error");
    assert_eq!(result["step"], "pull");
    assert_eq!(result["message"], "Could not resolve host");
}

#[test]
fn push_failure() {
    let dir = tempfile::tempdir().unwrap();
    let msg = dir.path().join(".flow-commit-msg");
    fs::write(&msg, "Test commit.").unwrap();

    let git = mock_git(vec![
        ok(""),                    // git commit
        ok("commit_sha\n"),        // git rev-parse HEAD (post-commit)
        ok(""),                    // git pull
        fail("permission denied"), // git push
    ]);

    let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
    assert_eq!(result["status"], "error");
    assert_eq!(result["step"], "push");
    assert_eq!(result["message"], "permission denied");
}

#[test]
fn message_file_missing_ok() {
    // Covers remove_message_file's "missing file ignored" branch — the
    // file was never created, yet finalize_commit_inner calls
    // remove_message_file in the Ok arm anyway. The production helper uses
    // `let _ = fs::remove_file(path)` so cleanup is idempotent.
    let dir = tempfile::tempdir().unwrap();
    let msg = dir.path().join(".flow-commit-msg");

    let git = mock_git(vec![
        ok(""),         // git commit
        ok("def456\n"), // git rev-parse HEAD (post-commit)
        ok(""),         // git pull
        ok(""),         // git push
        ok("def456\n"), // git rev-parse HEAD (final)
    ]);

    let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
    assert_eq!(result["status"], "ok");
    assert_eq!(result["pull_merged"], false);
}

#[test]
fn rev_parse_failure() {
    let dir = tempfile::tempdir().unwrap();
    let msg = dir.path().join(".flow-commit-msg");
    fs::write(&msg, "Test commit.").unwrap();

    let git = mock_git(vec![
        ok(""),             // git commit
        ok("commit_sha\n"), // git rev-parse HEAD (post-commit)
        ok(""),             // git pull
        ok(""),             // git push
        fail("bad HEAD"),   // git rev-parse HEAD (final)
    ]);

    let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
    assert_eq!(result["status"], "ok");
    assert_eq!(result["sha"], "");
    assert_eq!(result["pull_merged"], true);
    assert_eq!(
        result["warning"],
        "commit succeeded but SHA retrieval failed"
    );
}

#[test]
fn commit_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let msg = dir.path().join(".flow-commit-msg");
    fs::write(&msg, "Test commit.").unwrap();

    let git = mock_git(vec![timeout("timed out after 30s")]);

    let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
    assert_eq!(result["status"], "error");
    assert_eq!(result["step"], "commit");
    assert!(result["message"].as_str().unwrap().contains("timed out"));
    assert!(!msg.exists());
}

#[test]
fn pull_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let msg = dir.path().join(".flow-commit-msg");
    fs::write(&msg, "Test commit.").unwrap();

    let git = mock_git(vec![
        ok(""),                         // git commit
        ok("commit_sha\n"),             // git rev-parse HEAD (post-commit)
        timeout("timed out after 60s"), // git pull
    ]);

    let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
    assert_eq!(result["status"], "error");
    assert_eq!(result["step"], "pull");
    assert!(result["message"].as_str().unwrap().contains("timed out"));
}

#[test]
fn push_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let msg = dir.path().join(".flow-commit-msg");
    fs::write(&msg, "Test commit.").unwrap();

    let git = mock_git(vec![
        ok(""),                         // git commit
        ok("commit_sha\n"),             // git rev-parse HEAD (post-commit)
        ok(""),                         // git pull
        timeout("timed out after 60s"), // git push
    ]);

    let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
    assert_eq!(result["status"], "error");
    assert_eq!(result["step"], "push");
    assert!(result["message"].as_str().unwrap().contains("timed out"));
}

#[test]
fn rev_parse_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let msg = dir.path().join(".flow-commit-msg");
    fs::write(&msg, "Test commit.").unwrap();

    let git = mock_git(vec![
        ok(""),                         // git commit
        ok("commit_sha\n"),             // git rev-parse HEAD (post-commit)
        ok(""),                         // git pull
        ok(""),                         // git push
        timeout("timed out after 30s"), // git rev-parse HEAD (final)
    ]);

    let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
    assert_eq!(result["status"], "ok");
    assert_eq!(result["sha"], "");
    assert_eq!(result["pull_merged"], true);
    assert_eq!(
        result["warning"],
        "commit succeeded but SHA retrieval timed out"
    );
}

#[test]
fn status_porcelain_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let msg = dir.path().join(".flow-commit-msg");
    fs::write(&msg, "Test commit.").unwrap();

    let git = mock_git(vec![
        ok(""),                         // git commit
        ok("commit_sha\n"),             // git rev-parse HEAD (post-commit)
        fail("Could not resolve host"), // git pull
        timeout("timed out after 30s"), // git status --porcelain
    ]);

    let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
    assert_eq!(result["status"], "error");
    assert_eq!(result["step"], "pull");
    assert_eq!(result["message"], "Could not resolve host");
}

#[test]
fn dd_conflict_detected() {
    let dir = tempfile::tempdir().unwrap();
    let msg = dir.path().join(".flow-commit-msg");
    fs::write(&msg, "Test commit.").unwrap();

    let git = mock_git(vec![
        ok(""),                                                // git commit
        ok("commit_sha\n"),                                    // git rev-parse HEAD (post-commit)
        fail("CONFLICT"),                                      // git pull
        Ok((0, "DD deleted.py\n".to_string(), String::new())), // git status
    ]);

    let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
    assert_eq!(result["status"], "conflict");
    let files: Vec<String> = result["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(files, vec!["deleted.py"]);
}

#[test]
fn pull_merged_false_when_shas_match() {
    let dir = tempfile::tempdir().unwrap();
    let msg = dir.path().join(".flow-commit-msg");
    fs::write(&msg, "Test commit.").unwrap();

    let git = mock_git(vec![
        ok(""),           // git commit
        ok("same_sha\n"), // git rev-parse HEAD (post-commit)
        ok(""),           // git pull (no new content)
        ok(""),           // git push
        ok("same_sha\n"), // git rev-parse HEAD (final — unchanged)
    ]);

    let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
    assert_eq!(result["status"], "ok");
    assert_eq!(result["sha"], "same_sha");
    assert_eq!(result["pull_merged"], false);
}

#[test]
fn pull_merged_true_when_shas_differ() {
    let dir = tempfile::tempdir().unwrap();
    let msg = dir.path().join(".flow-commit-msg");
    fs::write(&msg, "Test commit.").unwrap();

    let git = mock_git(vec![
        ok(""),      // git commit
        ok("aaa\n"), // git rev-parse HEAD (post-commit)
        ok(""),      // git pull (merged remote changes)
        ok(""),      // git push
        ok("bbb\n"), // git rev-parse HEAD (final — changed by pull)
    ]);

    let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
    assert_eq!(result["status"], "ok");
    assert_eq!(result["sha"], "bbb");
    assert_eq!(result["pull_merged"], true);
}

#[test]
fn pull_merged_true_when_post_commit_revparse_fails() {
    let dir = tempfile::tempdir().unwrap();
    let msg = dir.path().join(".flow-commit-msg");
    fs::write(&msg, "Test commit.").unwrap();

    let git = mock_git(vec![
        ok(""),            // git commit
        fail("bad HEAD"),  // git rev-parse HEAD (post-commit — fails)
        ok(""),            // git pull
        ok(""),            // git push
        ok("final_sha\n"), // git rev-parse HEAD (final)
    ]);

    let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
    assert_eq!(result["status"], "ok");
    assert_eq!(result["sha"], "final_sha");
    assert_eq!(result["pull_merged"], true);
}

#[test]
fn pull_merged_true_when_final_revparse_fails() {
    let dir = tempfile::tempdir().unwrap();
    let msg = dir.path().join(".flow-commit-msg");
    fs::write(&msg, "Test commit.").unwrap();

    let git = mock_git(vec![
        ok(""),           // git commit
        ok("post_sha\n"), // git rev-parse HEAD (post-commit)
        ok(""),           // git pull
        ok(""),           // git push
        fail("bad HEAD"), // git rev-parse HEAD (final — fails)
    ]);

    let result = finalize_commit_inner(msg.to_str().unwrap(), "my-branch", &git);
    assert_eq!(result["status"], "ok");
    assert_eq!(result["sha"], "");
    assert_eq!(result["pull_merged"], true);
}

// --- run_impl integration fixtures (real git) ---

/// Set up a bare remote + clone with a passing bin/ci script and .flow-states dir.
/// Returns (clone_dir, bare_dir) as TempDirs that must be kept alive.
fn setup_integration_repo() -> (tempfile::TempDir, tempfile::TempDir) {
    let bare_dir = tempfile::tempdir().unwrap();
    let clone_dir = tempfile::tempdir().unwrap();

    git_assert_ok(
        &Command::new("git")
            .args(["init", "--bare", "--initial-branch", "main"])
            .arg(bare_dir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    git_assert_ok(
        &Command::new("git")
            .args(["clone"])
            .arg(bare_dir.path())
            .arg(clone_dir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let clone_str = clone_dir.path().to_str().unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_str, "config", "user.email", "test@test.com"])
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_str, "config", "user.name", "Test"])
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_str, "config", "pull.rebase", "false"])
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_str, "config", "commit.gpgSign", "false"])
            .output()
            .unwrap(),
    );

    let flow_states = clone_dir.path().join(".flow-states");
    fs::create_dir_all(&flow_states).unwrap();
    let gitignore = clone_dir.path().join(".gitignore");
    fs::write(&gitignore, ".flow-states/\n").unwrap();

    let readme = clone_dir.path().join("README.md");
    fs::write(&readme, "# Test\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_dir.path().to_str().unwrap(), "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args([
                "-C",
                clone_dir.path().to_str().unwrap(),
                "commit",
                "-m",
                "Initial commit",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args([
                "-C",
                clone_dir.path().to_str().unwrap(),
                "push",
                "-u",
                "origin",
                "main",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    (clone_dir, bare_dir)
}

/// Set up a repo with a controllable `bin/test` script.
fn setup_integration_repo_with_ci() -> (tempfile::TempDir, tempfile::TempDir) {
    let (clone_dir, bare_dir) = setup_integration_repo();
    let clone_str = clone_dir.path().to_str().unwrap();

    let bin_dir = clone_dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let bin_test = bin_dir.join("test");
    let script = r#"#!/usr/bin/env bash
echo "invoked" >> "$(dirname "$0")/../.ci-invocation-marker"
if [ -f "$(dirname "$0")/../.ci-should-fail" ] && [ "$(cat "$(dirname "$0")/../.ci-should-fail")" = "1" ]; then
  exit 1
fi
exit 0
"#;
    fs::write(&bin_test, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&bin_test, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let exclude_file = clone_dir.path().join(".git").join("info").join("exclude");
    let existing = fs::read_to_string(&exclude_file).unwrap_or_default();
    fs::write(
        &exclude_file,
        format!("{}.ci-invocation-marker\n.ci-should-fail\n", existing),
    )
    .unwrap();

    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_str, "add", "bin/test"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_str, "commit", "-m", "Add bin/test"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_str, "push"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    (clone_dir, bare_dir)
}

fn write_ci_sentinel(clone_path: &std::path::Path, branch: &str) {
    let snapshot = flow_rs::ci::tree_snapshot(clone_path, None);
    let sentinel = flow_rs::ci::sentinel_path(clone_path, branch);
    fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
    fs::write(&sentinel, &snapshot).unwrap();
}

fn write_state_with_continue_pending(clone_path: &std::path::Path, branch: &str) {
    let flow_states = clone_path.join(".flow-states");
    fs::create_dir_all(&flow_states).unwrap();
    let state_file = flow_states.join(format!("{}.json", branch));
    let state = json!({
        "branch": branch,
        "current_phase": "flow-code",
        "_continue_pending": "commit",
        "_continue_context": "Self-invoke flow:flow-code --continue-step --auto."
    });
    fs::write(&state_file, serde_json::to_string_pretty(&state).unwrap()).unwrap();
}

fn read_state(clone_path: &std::path::Path, branch: &str) -> Value {
    let state_file = clone_path
        .join(".flow-states")
        .join(format!("{}.json", branch));
    let content = fs::read_to_string(&state_file).unwrap();
    serde_json::from_str(&content).unwrap()
}

// --- run_impl: CI enforcement ---

#[test]
fn test_ci_fails_blocks_commit() {
    let (clone_dir, _bare_dir) = setup_integration_repo_with_ci();
    let clone_path = clone_dir.path();

    fs::write(clone_path.join(".ci-should-fail"), "1").unwrap();

    fs::write(clone_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_path.to_str().unwrap(), "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = clone_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add feature.rs").unwrap();

    let before = Command::new("git")
        .args(["-C", clone_path.to_str().unwrap(), "log", "--oneline"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    git_assert_ok(&before);
    let commits_before = String::from_utf8_lossy(&before.stdout).lines().count();

    let args = Args {
        message_file: msg_path.to_str().unwrap().to_string(),
        branch: "main".to_string(),
    };

    let result = run_impl(&args, clone_path, clone_path).unwrap();
    assert_eq!(result["status"], "error", "expected CI failure: {}", result);
    assert_eq!(result["step"], "ci");

    let after = Command::new("git")
        .args(["-C", clone_path.to_str().unwrap(), "log", "--oneline"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    git_assert_ok(&after);
    let commits_after = String::from_utf8_lossy(&after.stdout).lines().count();
    assert_eq!(
        commits_before, commits_after,
        "no commit should have been created when CI fails"
    );
}

#[test]
fn test_ci_sentinel_fresh_skips_ci() {
    let (clone_dir, _bare_dir) = setup_integration_repo_with_ci();
    let clone_path = clone_dir.path();

    fs::write(clone_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_path.to_str().unwrap(), "add", "feature.rs"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = clone_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add feature.rs").unwrap();

    let snapshot = flow_rs::ci::tree_snapshot(clone_path, None);
    let sentinel = flow_rs::ci::sentinel_path(clone_path, "main");
    fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
    fs::write(&sentinel, &snapshot).unwrap();

    let args = Args {
        message_file: msg_path.to_str().unwrap().to_string(),
        branch: "main".to_string(),
    };

    let result = run_impl(&args, clone_path, clone_path).unwrap();
    assert_eq!(result["status"], "ok", "commit should succeed: {}", result);
    assert!(
        !result["sha"].as_str().unwrap().is_empty(),
        "should have a commit SHA"
    );

    let marker = clone_path.join(".ci-invocation-marker");
    assert!(
        !marker.exists(),
        "CI should not have been invoked (sentinel was fresh)"
    );
}

// --- run_impl: continue_pending state handling ---

#[test]
fn run_impl_error_clears_continue_pending() {
    let (clone_dir, _bare_dir) = setup_integration_repo_with_ci();
    let clone_path = clone_dir.path();

    write_state_with_continue_pending(clone_path, "main");

    fs::write(clone_path.join(".ci-should-fail"), "1").unwrap();

    fs::write(clone_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_path.to_str().unwrap(), "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = clone_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add feature.rs").unwrap();

    let args = Args {
        message_file: msg_path.to_str().unwrap().to_string(),
        branch: "main".to_string(),
    };

    let result = run_impl(&args, clone_path, clone_path).unwrap();
    assert_eq!(result["status"], "error", "expected CI failure: {}", result);
    assert_eq!(result["step"], "ci");

    let state = read_state(clone_path, "main");
    let pending = state
        .get("_continue_pending")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        pending.is_empty(),
        "_continue_pending should be cleared after error, got: {:?}",
        pending
    );
    let ctx = state
        .get("_continue_context")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        ctx.is_empty(),
        "_continue_context should be cleared after error, got: {:?}",
        ctx
    );
}

#[test]
fn run_impl_ok_preserves_continue_pending() {
    let (clone_dir, _bare_dir) = setup_integration_repo_with_ci();
    let clone_path = clone_dir.path();

    write_state_with_continue_pending(clone_path, "main");

    fs::write(clone_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_path.to_str().unwrap(), "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = clone_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add feature.rs").unwrap();

    write_ci_sentinel(clone_path, "main");

    let args = Args {
        message_file: msg_path.to_str().unwrap().to_string(),
        branch: "main".to_string(),
    };

    let result = run_impl(&args, clone_path, clone_path).unwrap();
    assert_eq!(result["status"], "ok", "commit should succeed: {}", result);

    let state = read_state(clone_path, "main");
    assert_eq!(
        state["_continue_pending"], "commit",
        "_continue_pending should be preserved on success"
    );
    assert_eq!(
        state["_continue_context"], "Self-invoke flow:flow-code --continue-step --auto.",
        "_continue_context should be preserved on success"
    );
}

#[test]
fn run_impl_conflict_preserves_continue_pending() {
    let (clone_dir, bare_dir) = setup_integration_repo_with_ci();
    let clone_path = clone_dir.path();

    write_state_with_continue_pending(clone_path, "main");

    let clone2_dir = tempfile::tempdir().unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["clone"])
            .arg(bare_dir.path())
            .arg(clone2_dir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );
    for (key, val) in [("user.email", "other@test.com"), ("user.name", "Other")] {
        git_assert_ok(
            &Command::new("git")
                .args([
                    "-C",
                    clone2_dir.path().to_str().unwrap(),
                    "config",
                    key,
                    val,
                ])
                .output()
                .unwrap(),
        );
    }

    fs::write(
        clone2_dir.path().join("README.md"),
        "# Conflicting content\n",
    )
    .unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone2_dir.path().to_str().unwrap(), "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args([
                "-C",
                clone2_dir.path().to_str().unwrap(),
                "commit",
                "-m",
                "Conflicting commit",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone2_dir.path().to_str().unwrap(), "push"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    fs::write(
        clone_path.join("README.md"),
        "# Local conflicting content\n",
    )
    .unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_path.to_str().unwrap(), "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = clone_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Local change to README").unwrap();

    git_assert_ok(
        &Command::new("git")
            .args([
                "-C",
                clone_path.to_str().unwrap(),
                "config",
                "pull.rebase",
                "false",
            ])
            .output()
            .unwrap(),
    );

    write_ci_sentinel(clone_path, "main");

    let args = Args {
        message_file: msg_path.to_str().unwrap().to_string(),
        branch: "main".to_string(),
    };

    let result = run_impl(&args, clone_path, clone_path).unwrap();
    assert_eq!(
        result["status"], "conflict",
        "expected conflict: {}",
        result
    );

    let state = read_state(clone_path, "main");
    assert_eq!(
        state["_continue_pending"], "commit",
        "_continue_pending should be preserved on conflict"
    );
}

// --- run_impl: sentinel refresh ---

#[test]
fn run_impl_refreshes_sentinel_after_commit() {
    let (clone_dir, _bare_dir) = setup_integration_repo_with_ci();
    let clone_path = clone_dir.path().to_str().unwrap().to_string();

    let src = clone_dir.path().join("src.rs");
    fs::write(&src, "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", &clone_path, "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = clone_dir.path().join(".flow-commit-msg");
    fs::write(&msg_path, "Add src.rs").unwrap();

    write_ci_sentinel(clone_dir.path(), "main");

    let args = Args {
        message_file: msg_path.to_str().unwrap().to_string(),
        branch: "main".to_string(),
    };

    let result = run_impl(&args, clone_dir.path(), clone_dir.path()).unwrap();
    assert_eq!(result["status"], "ok", "commit should succeed: {}", result);
    assert_eq!(result["pull_merged"], false);

    let sentinel = flow_rs::ci::sentinel_path(clone_dir.path(), "main");
    assert!(
        sentinel.exists(),
        "sentinel file should exist after clean commit"
    );

    let sentinel_content = fs::read_to_string(&sentinel).unwrap();
    assert_eq!(
        sentinel_content.len(),
        64,
        "sentinel should be a SHA-256 hex string"
    );
    assert!(
        sentinel_content.chars().all(|c| c.is_ascii_hexdigit()),
        "sentinel should contain only hex digits"
    );
}

#[test]
fn run_impl_no_sentinel_refresh_when_pull_merges() {
    let (clone_dir, bare_dir) = setup_integration_repo_with_ci();
    let clone_path = clone_dir.path().to_str().unwrap().to_string();

    let clone2_dir = tempfile::tempdir().unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["clone"])
            .arg(bare_dir.path())
            .arg(clone2_dir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args([
                "-C",
                clone2_dir.path().to_str().unwrap(),
                "config",
                "user.email",
                "other@test.com",
            ])
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args([
                "-C",
                clone2_dir.path().to_str().unwrap(),
                "config",
                "user.name",
                "Other",
            ])
            .output()
            .unwrap(),
    );

    let other_file = clone2_dir.path().join("other.txt");
    fs::write(&other_file, "other content\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone2_dir.path().to_str().unwrap(), "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args([
                "-C",
                clone2_dir.path().to_str().unwrap(),
                "commit",
                "-m",
                "Other commit",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone2_dir.path().to_str().unwrap(), "push"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let src = clone_dir.path().join("local.txt");
    fs::write(&src, "local content\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", &clone_path, "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = clone_dir.path().join(".flow-commit-msg");
    fs::write(&msg_path, "Add local.txt").unwrap();

    write_ci_sentinel(clone_dir.path(), "main");

    let args = Args {
        message_file: msg_path.to_str().unwrap().to_string(),
        branch: "main".to_string(),
    };

    let result = run_impl(&args, clone_dir.path(), clone_dir.path()).unwrap();
    assert_eq!(result["status"], "ok", "commit should succeed: {}", result);
    assert_eq!(result["pull_merged"], true);

    let sentinel = flow_rs::ci::sentinel_path(clone_dir.path(), "main");
    assert!(
        !sentinel.exists(),
        "sentinel should not exist when pull merged"
    );
}

// --- run_impl: staged_diff fallback when `git diff --cached` fails ---

/// Exercises the `_ => String::new()` fallback on line ~336 where
/// `git diff --cached` returns a non-zero exit code. Triggered by running
/// against a directory that isn't a git repo — the `-C <cwd>` flag makes
/// every git subcommand fail with exit 128. CI is bypassed via a
/// sentinel. With an empty staged_diff the plan-deviation scanner
/// collects no deviations and the commit path moves on to `git commit`,
/// which also fails — status becomes "error" with step "commit".
#[test]
fn run_impl_staged_diff_fallback_when_git_diff_fails() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();

    // No git init — every `git -C <root>` call returns exit 128.
    // But we still need a .flow-states dir for the CI sentinel.
    fs::create_dir_all(root.join(".flow-states")).unwrap();

    // Write the CI sentinel matching the snapshot so ci::run_impl
    // takes the fast skip path (no bin/* scripts invoked).
    let snapshot = flow_rs::ci::tree_snapshot(&root, None);
    let sentinel = flow_rs::ci::sentinel_path(&root, "main");
    fs::write(&sentinel, &snapshot).unwrap();

    let msg_path = root.join(".flow-commit-msg");
    fs::write(&msg_path, "msg").unwrap();

    let args = Args {
        message_file: msg_path.to_str().unwrap().to_string(),
        branch: "main".to_string(),
    };

    let result = run_impl(&args, &root, &root).unwrap();
    // Commit fails because it's not a git repo — but crucially the staged_diff
    // fallback (`_ => String::new()`) ran to produce the empty staged_diff
    // that plan_deviation::run_impl then saw. The commit itself fails with
    // step="commit".
    assert_eq!(result["status"], "error");
    assert_eq!(result["step"], "commit");
}

// --- run_impl: state-file not-object guard in mutate_state closure ---

/// Exercises the `if !(state.is_object() || state.is_null()) { return; }`
/// guard inside the mutate_state closure on error-cleanup. The state file
/// is written as a JSON array (not an object), so mutate_state invokes the
/// closure with state as an array, the guard returns early, and no
/// continuation-field reset is attempted.
#[test]
fn run_impl_error_state_wrong_type_guard_fires() {
    let (clone_dir, _bare_dir) = setup_integration_repo_with_ci();
    let clone_path = clone_dir.path();

    // Overwrite state file with a JSON ARRAY (not an object). mutate_state's
    // closure in run_impl will see state.is_array() and return early via
    // the type guard — no mutation applied, no panic.
    let flow_states = clone_path.join(".flow-states");
    fs::create_dir_all(&flow_states).unwrap();
    let state_path = flow_states.join("main.json");
    fs::write(&state_path, "[1, 2, 3]").unwrap();

    // Configure CI to fail so run_impl hits the error-cleanup path.
    fs::write(clone_path.join(".ci-should-fail"), "1").unwrap();

    fs::write(clone_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_path.to_str().unwrap(), "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = clone_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add feature.rs").unwrap();

    let args = Args {
        message_file: msg_path.to_str().unwrap().to_string(),
        branch: "main".to_string(),
    };

    let result = run_impl(&args, clone_path, clone_path).unwrap();
    assert_eq!(result["status"], "error");
    assert_eq!(result["step"], "ci");

    // State file should still be the JSON array — the guard prevented
    // mutation.
    let content = fs::read_to_string(&state_path).unwrap();
    let parsed: Value = serde_json::from_str(&content).unwrap();
    assert!(parsed.is_array(), "state should remain a JSON array");
}

// --- run_impl: plan-deviation gate ---

/// Exercises the `Err(deviations)` branch of `plan_deviation::run_impl` in
/// `run_impl`. Covers `emit_deviation_stderr` (loop bodies + format! calls)
/// and the deviation-rendering JSON response. A plan file names `test_foo`
/// with fixture `expected = "original"`, but the staged diff's `test_foo`
/// body is empty (does not contain "original"), so the gate fires.
#[test]
fn run_impl_plan_deviation_blocks_commit() {
    let (clone_dir, _bare_dir) = setup_integration_repo_with_ci();
    let clone_path = clone_dir.path();

    // Write the plan file with a Tasks section naming a test and a plan
    // value that must appear in the test body.
    let flow_states = clone_path.join(".flow-states");
    fs::create_dir_all(&flow_states).unwrap();
    let plan_path = flow_states.join("main-plan.md");
    let plan_content = r#"# Plan

## Tasks

Task 1: Add `test_foo`.

```rust
fn test_foo() {
    let expected = "original";
}
```
"#;
    fs::write(&plan_path, plan_content).unwrap();

    // Write a state file pointing to the plan file; the plan-deviation
    // scanner reads `state["files"]["plan"]` to locate it.
    let state_file = flow_states.join("main.json");
    let state = json!({
        "branch": "main",
        "current_phase": "flow-code",
        "files": {"plan": ".flow-states/main-plan.md"}
    });
    fs::write(&state_file, serde_json::to_string_pretty(&state).unwrap()).unwrap();

    // Stage a test file whose `test_foo` body does NOT contain "original".
    let tests_dir = clone_path.join("tests");
    fs::create_dir_all(&tests_dir).unwrap();
    let test_file = tests_dir.join("foo.rs");
    fs::write(
        &test_file,
        "fn test_foo() {\n    let actual = \"drifted\";\n}\n",
    )
    .unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_path.to_str().unwrap(), "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = clone_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add test_foo").unwrap();

    // CI sentinel so ci::run_impl takes the fast skip path.
    write_ci_sentinel(clone_path, "main");

    let args = Args {
        message_file: msg_path.to_str().unwrap().to_string(),
        branch: "main".to_string(),
    };

    let result = run_impl(&args, clone_path, clone_path).unwrap();
    assert_eq!(
        result["status"], "error",
        "plan deviation should block: {}",
        result
    );
    assert_eq!(result["step"], "plan_deviation");
    assert!(
        result["message"]
            .as_str()
            .unwrap()
            .contains("unacknowledged plan signature deviation"),
        "unexpected message: {}",
        result["message"]
    );
    let deviations = result["deviations"].as_array().unwrap();
    assert_eq!(deviations.len(), 1);
    assert_eq!(deviations[0]["test_name"], "test_foo");
    assert_eq!(deviations[0]["plan_value"], "original");
}

/// Two-deviation companion to the single-deviation test. Exercises the
/// plural "s" branch of the `if deviations.len() == 1 { "" } else { "s" }`
/// expressions at the log line and the JSON "message" field — both are
/// the same pluralization pattern so both are covered by the same test.
#[test]
fn run_impl_plan_deviation_two_deviations_plural_message() {
    let (clone_dir, _bare_dir) = setup_integration_repo_with_ci();
    let clone_path = clone_dir.path();

    let flow_states = clone_path.join(".flow-states");
    fs::create_dir_all(&flow_states).unwrap();
    let plan_path = flow_states.join("main-plan.md");
    let plan_content = r#"# Plan

## Tasks

Task 1: Add two tests that drift from their plan values.

```rust
fn test_alpha() {
    let expected = "alpha_value";
}
fn test_beta() {
    let expected = "beta_value";
}
```
"#;
    fs::write(&plan_path, plan_content).unwrap();

    let state_file = flow_states.join("main.json");
    let state = json!({
        "branch": "main",
        "current_phase": "flow-code",
        "files": {"plan": ".flow-states/main-plan.md"}
    });
    fs::write(&state_file, serde_json::to_string_pretty(&state).unwrap()).unwrap();

    let tests_dir = clone_path.join("tests");
    fs::create_dir_all(&tests_dir).unwrap();
    let test_file = tests_dir.join("drift.rs");
    fs::write(
        &test_file,
        "fn test_alpha() {\n    let actual = \"alpha_drifted\";\n}\n\
         fn test_beta() {\n    let actual = \"beta_drifted\";\n}\n",
    )
    .unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_path.to_str().unwrap(), "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = clone_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add test_alpha and test_beta").unwrap();

    write_ci_sentinel(clone_path, "main");

    let args = Args {
        message_file: msg_path.to_str().unwrap().to_string(),
        branch: "main".to_string(),
    };

    let result = run_impl(&args, clone_path, clone_path).unwrap();
    assert_eq!(result["status"], "error");
    assert_eq!(result["step"], "plan_deviation");
    let msg = result["message"].as_str().unwrap();
    assert!(
        msg.contains("2 unacknowledged plan signature deviations"),
        "expected plural 'deviations', got: {}",
        msg
    );
    let deviations = result["deviations"].as_array().unwrap();
    assert_eq!(deviations.len(), 2);
}

// --- run_impl error arg validation ---

#[test]
fn run_impl_empty_message_file_returns_err() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let args = Args {
        message_file: String::new(),
        branch: "main".to_string(),
    };
    let err = run_impl(&args, &root, &root).unwrap_err();
    assert!(err.contains("finalize-commit"));
}

#[test]
fn run_impl_empty_branch_returns_err() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let args = Args {
        message_file: "msg.txt".to_string(),
        branch: String::new(),
    };
    let err = run_impl(&args, &root, &root).unwrap_err();
    assert!(err.contains("finalize-commit"));
}

// --- run_impl_main (subprocess) ---

fn flow_rs_no_recursion() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd
}

// Exercises run_impl_main's Err arm: empty message-file / branch args →
// run_impl returns Err → run_impl_main wraps as {"step":"args"} + exit 1.
#[test]
fn run_impl_main_empty_args_exits_1_with_args_step() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();

    let output = flow_rs_no_recursion()
        .args(["finalize-commit", "", ""])
        .current_dir(&root)
        .env("GIT_CEILING_DIRECTORIES", &root)
        .env("GH_TOKEN", "invalid")
        .env("HOME", &root)
        .output()
        .expect("spawn flow-rs");

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let last = stdout
        .lines()
        .rfind(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON in stdout: {}", stdout));
    let json: Value = serde_json::from_str(last).unwrap();
    assert_eq!(json["status"], "error");
    assert_eq!(json["step"], "args");
}

// Exercises run_impl_main's Ok(result) arm with status != "ok" → exit 1.
// CI is configured to fail, so finalize-commit returns step="ci".
#[test]
fn run_impl_main_ok_status_error_exits_1() {
    let (clone_dir, _bare_dir) = setup_integration_repo_with_ci();
    let clone_path = clone_dir.path();

    fs::write(clone_path.join(".ci-should-fail"), "1").unwrap();

    fs::write(clone_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_path.to_str().unwrap(), "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = clone_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add feature.rs").unwrap();

    let output = flow_rs_no_recursion()
        .args(["finalize-commit", msg_path.to_str().unwrap(), "main"])
        .current_dir(clone_path)
        .env("GIT_CEILING_DIRECTORIES", clone_path)
        .env("GH_TOKEN", "invalid")
        .env("HOME", clone_path)
        .output()
        .expect("spawn flow-rs");

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let last = stdout
        .lines()
        .rfind(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON in stdout: {}", stdout));
    let json: Value = serde_json::from_str(last).unwrap();
    assert_eq!(json["status"], "error");
    assert_eq!(json["step"], "ci");
}

// Exercises run_impl_main's Ok(result) arm with status == "ok" → exit 0.
// CI sentinel is fresh, so the fast skip path lets commit succeed.
#[test]
fn run_impl_main_ok_status_ok_exits_0() {
    let (clone_dir, _bare_dir) = setup_integration_repo_with_ci();
    let clone_path = clone_dir.path();

    fs::write(clone_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_path.to_str().unwrap(), "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = clone_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add feature.rs").unwrap();

    write_ci_sentinel(clone_path, "main");

    let output = flow_rs_no_recursion()
        .args(["finalize-commit", msg_path.to_str().unwrap(), "main"])
        .current_dir(clone_path)
        .env("GIT_CEILING_DIRECTORIES", clone_path)
        .env("GH_TOKEN", "invalid")
        .env("HOME", clone_path)
        .output()
        .expect("spawn flow-rs");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let last = stdout
        .lines()
        .rfind(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON in stdout: {}", stdout));
    let json: Value = serde_json::from_str(last).unwrap();
    assert_eq!(json["status"], "ok");
}
