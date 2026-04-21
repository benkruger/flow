//! Integration tests for `flow_rs::finalize_commit` — drives the public
//! surface (`run_impl` with real git, `run_impl_main` via the compiled
//! binary). Private helpers (`remove_message_file`, `emit_deviation_stderr`,
//! `run_git_in_dir`, `finalize_commit`) are exercised indirectly through
//! the public entry points. No `finalize_commit_inner` pub-for-testing
//! seam — every git call goes through the real binary inside a fixture
//! repo or a stub on PATH.

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use flow_rs::finalize_commit::{run_impl, Args};
use serde_json::{json, Value};

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

/// Write a `git` stub that forwards specific subcommands to the real
/// `/usr/bin/git` and overrides others via env-var-controlled FAKE_*
/// knobs. Returns the PATH-entry directory containing the stub.
///
/// Supported subcommand overrides (applied when the corresponding env
/// var is set):
///   - `commit`: FAKE_COMMIT_EXIT, FAKE_COMMIT_STDERR
///   - `pull`:   FAKE_PULL_EXIT, FAKE_PULL_STDERR
///   - `push`:   FAKE_PUSH_EXIT, FAKE_PUSH_STDERR
///   - `status`: FAKE_STATUS_OUT (when set, overrides git status --porcelain output)
///
/// Calls not matched by any of the above — including `rev-parse`, `diff`,
/// `log`, `ls-files`, `hash-object`, etc. — exec real git. This keeps
/// `ci::tree_snapshot` and other internals working while letting tests
/// selectively simulate pull/push/commit failures.
fn write_git_stub(parent: &Path) -> PathBuf {
    let stubs = parent.join("git-stubs");
    fs::create_dir_all(&stubs).unwrap();
    let script = r#"#!/bin/sh
# Strip the leading `-C <path>` pair that run_git_in_dir always prepends.
REPO_PATH=""
if [ "$1" = "-C" ]; then
    REPO_PATH="$2"
    shift 2
fi
SUBCMD="$1"
shift
case "$SUBCMD" in
    commit)
        if [ -n "$FAKE_COMMIT_STDERR" ]; then printf '%s' "$FAKE_COMMIT_STDERR" >&2; fi
        if [ -n "$FAKE_COMMIT_EXIT" ]; then exit "$FAKE_COMMIT_EXIT"; fi
        exec /usr/bin/git -C "$REPO_PATH" commit "$@"
        ;;
    pull)
        if [ -n "$FAKE_PULL_STDERR" ]; then printf '%s' "$FAKE_PULL_STDERR" >&2; fi
        if [ -n "$FAKE_PULL_EXIT" ]; then exit "$FAKE_PULL_EXIT"; fi
        exec /usr/bin/git -C "$REPO_PATH" pull "$@"
        ;;
    push)
        if [ -n "$FAKE_PUSH_STDERR" ]; then printf '%s' "$FAKE_PUSH_STDERR" >&2; fi
        if [ -n "$FAKE_PUSH_EXIT" ]; then exit "$FAKE_PUSH_EXIT"; fi
        exec /usr/bin/git -C "$REPO_PATH" push "$@"
        ;;
    status)
        # Only intercept the --porcelain form finalize_commit uses after
        # a pull failure; pass everything else through to real git.
        if [ "$1" = "--porcelain" ] && [ -n "${FAKE_STATUS_OUT+set}" ]; then
            printf '%s' "$FAKE_STATUS_OUT"
            exit 0
        fi
        exec /usr/bin/git -C "$REPO_PATH" status "$@"
        ;;
    *)
        exec /usr/bin/git -C "$REPO_PATH" "$SUBCMD" "$@"
        ;;
esac
"#;
    let git_path = stubs.join("git");
    fs::write(&git_path, script).unwrap();
    #[cfg(unix)]
    {
        fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    stubs
}

/// Helper: run `bin/flow finalize-commit` against a repo fixture with a
/// controlled git stub on PATH.
fn run_finalize_with_stub(
    clone_path: &Path,
    msg_path: &Path,
    branch: &str,
    stubs: &Path,
    env: &[(&str, &str)],
) -> std::process::Output {
    let current_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", stubs.display(), current_path);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.args(["finalize-commit", msg_path.to_str().unwrap(), branch])
        .current_dir(clone_path)
        .env("PATH", new_path)
        .env_remove("FLOW_CI_RUNNING");
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().expect("spawn flow-rs")
}

fn last_json_line(stdout: &str) -> Value {
    let last = stdout
        .lines()
        .rfind(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON line in stdout; stdout={}", stdout));
    serde_json::from_str(last)
        .unwrap_or_else(|e| panic!("failed to parse JSON line '{}': {}", last, e))
}

// --- run_impl: happy path (real git) ---

#[test]
fn happy_path_commit_pull_push_succeed() {
    let (clone_dir, _bare_dir) = setup_integration_repo_with_ci();
    let clone_path = clone_dir.path();

    fs::write(clone_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_path.to_str().unwrap(), "add", "-A"])
            .output()
            .unwrap(),
    );

    let msg_path = clone_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Test commit.").unwrap();

    write_ci_sentinel(clone_path, "main");

    let args = Args {
        message_file: msg_path.to_str().unwrap().to_string(),
        branch: "main".to_string(),
    };
    let result = run_impl(&args, clone_path, clone_path).unwrap();
    assert_eq!(result["status"], "ok", "got: {}", result);
    assert_eq!(result["pull_merged"], false);
    // Message file was removed after commit.
    assert!(!msg_path.exists());
}

// --- run_impl: CI enforcement ---

#[test]
fn ci_fails_blocks_commit() {
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
fn ci_sentinel_fresh_skips_ci() {
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
fn error_clears_continue_pending() {
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
fn ok_preserves_continue_pending() {
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
fn conflict_preserves_continue_pending() {
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
fn refreshes_sentinel_after_commit() {
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
fn no_sentinel_refresh_when_pull_merges() {
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

/// Exercises the `_ => String::new()` fallback where `git diff --cached`
/// returns a non-zero exit code. Triggered by running against a directory
/// that isn't a git repo — every git subcommand fails with exit 128.
/// CI is bypassed via a sentinel. With an empty staged_diff the
/// plan-deviation scanner collects no deviations and the commit path moves
/// on to `git commit`, which also fails — status becomes "error" with
/// step "commit".
#[test]
fn staged_diff_fallback_when_git_diff_fails() {
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
fn error_state_wrong_type_guard_fires() {
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
fn plan_deviation_blocks_commit() {
    let (clone_dir, _bare_dir) = setup_integration_repo_with_ci();
    let clone_path = clone_dir.path();

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

    let state_file = flow_states.join("main.json");
    let state = json!({
        "branch": "main",
        "current_phase": "flow-code",
        "files": {"plan": ".flow-states/main-plan.md"}
    });
    fs::write(&state_file, serde_json::to_string_pretty(&state).unwrap()).unwrap();

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
fn plan_deviation_two_deviations_plural_message() {
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

// --- run_impl: commit-step error paths (via git stub) ---

/// Commit spawn failure: with git absent from PATH, the compiled flow-rs
/// subprocess fails to spawn `git commit`. `run_cmd_with_timeout` returns
/// Err, which `finalize_commit` converts to a commit-error JSON via its
/// Err arm. CI is bypassed via a sentinel matching the "no-git" snapshot
/// (tree_snapshot hashes all-empty git outputs to a deterministic value
/// the test pre-computes in an empty directory).
#[test]
fn commit_spawn_failure_returns_error_step_commit() {
    let (clone_dir, _bare_dir) = setup_integration_repo_with_ci();
    let clone_path = clone_dir.path();

    let msg_path = clone_path.join(".flow-commit-msg");
    fs::write(&msg_path, "msg").unwrap();

    // Pre-compute the "no-git" tree_snapshot hash using an empty dir —
    // every git_stdout call in tree_snapshot returns "" (git exits
    // non-zero since it's not a git repo), producing the same hash the
    // flow-rs subprocess will compute when git is absent from PATH.
    let empty_dir = tempfile::tempdir().unwrap();
    let no_git_snapshot = flow_rs::ci::tree_snapshot(empty_dir.path(), None);
    let sentinel = flow_rs::ci::sentinel_path(clone_path, "main");
    fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
    fs::write(&sentinel, &no_git_snapshot).unwrap();

    // PATH is empty: ci::run_impl calls tree_snapshot → git absent →
    // every git call returns Err → program_stdout "" → snapshot matches
    // sentinel → CI skips. Then finalize_commit calls run_cmd_with_timeout
    // for `git commit` which spawn-fails → Err branch fires.
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["finalize-commit", msg_path.to_str().unwrap(), "main"])
        .current_dir(clone_path)
        .env("PATH", "")
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("spawn flow-rs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error", "got: {}", json);
    assert_eq!(json["step"], "commit");
    let msg = json["message"].as_str().unwrap();
    assert!(
        msg.to_lowercase().contains("failed to spawn"),
        "expected spawn failure in message, got: {}",
        msg
    );
    // Message file removed even on Err path.
    assert!(!msg_path.exists());
}

/// Commit fails with nonzero exit + stderr. Covers the
/// `Ok((code, _, stderr))` commit-error branch.
#[test]
fn commit_nonzero_returns_error_step_commit() {
    let (clone_dir, _bare_dir) = setup_integration_repo_with_ci();
    let clone_path = clone_dir.path();

    let msg_path = clone_path.join(".flow-commit-msg");
    fs::write(&msg_path, "msg").unwrap();

    write_ci_sentinel(clone_path, "main");

    let stubs = write_git_stub(clone_path);

    let output = run_finalize_with_stub(
        clone_path,
        &msg_path,
        "main",
        &stubs,
        &[
            ("FAKE_COMMIT_EXIT", "1"),
            ("FAKE_COMMIT_STDERR", "nothing to commit"),
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error", "got: {}", json);
    assert_eq!(json["step"], "commit");
    assert_eq!(json["message"], "nothing to commit");
    // Message file removed after commit-step exit.
    assert!(!msg_path.exists());
}

/// Commit spawns successfully; pull fails with nonzero exit + stderr; git
/// status --porcelain returns clean. Covers pull-error no-conflict path.
#[test]
fn pull_nonzero_no_conflict_returns_error_step_pull() {
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
    fs::write(&msg_path, "msg").unwrap();

    write_ci_sentinel(clone_path, "main");

    let stubs = write_git_stub(clone_path);

    let output = run_finalize_with_stub(
        clone_path,
        &msg_path,
        "main",
        &stubs,
        &[
            ("FAKE_PULL_EXIT", "1"),
            ("FAKE_PULL_STDERR", "Could not resolve host"),
            ("FAKE_STATUS_OUT", ""),
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error", "got: {}", json);
    assert_eq!(json["step"], "pull");
    assert_eq!(json["message"], "Could not resolve host");
}

/// Commit ok; pull fails; git status --porcelain reports UU/AA conflict
/// markers. Covers pull-conflict path, emits "conflict" status with
/// files array.
#[test]
fn pull_nonzero_with_conflict_returns_conflict_status() {
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
    fs::write(&msg_path, "msg").unwrap();

    write_ci_sentinel(clone_path, "main");

    let stubs = write_git_stub(clone_path);

    let output = run_finalize_with_stub(
        clone_path,
        &msg_path,
        "main",
        &stubs,
        &[
            ("FAKE_PULL_EXIT", "1"),
            ("FAKE_PULL_STDERR", "CONFLICT"),
            ("FAKE_STATUS_OUT", "UU file1.rs\nAA file2.rs\n"),
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "conflict", "got: {}", json);
    let files: Vec<String> = json["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(files, vec!["file1.rs", "file2.rs"]);
}

/// Commit ok, pull ok, push fails with nonzero exit. Covers push-error
/// branch.
#[test]
fn push_nonzero_returns_error_step_push() {
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
    fs::write(&msg_path, "msg").unwrap();

    write_ci_sentinel(clone_path, "main");

    let stubs = write_git_stub(clone_path);

    let output = run_finalize_with_stub(
        clone_path,
        &msg_path,
        "main",
        &stubs,
        &[
            ("FAKE_PUSH_EXIT", "1"),
            ("FAKE_PUSH_STDERR", "permission denied"),
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error", "got: {}", json);
    assert_eq!(json["step"], "push");
    assert_eq!(json["message"], "permission denied");
}

// --- run_impl error arg validation ---

#[test]
fn empty_message_file_returns_err() {
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
fn empty_branch_returns_err() {
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
