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

/// Snapshot the tree of `commit_cwd` and pin the sentinel under
/// `<root>/.flow-states/<branch>/ci-passed`. Used by tests that route
/// finalize-commit through a worktree at `<root>/.worktrees/<branch>/`:
/// the sentinel must reflect the worktree's tree (the destination
/// commit_cwd resolves to inside `run_impl`), not the main clone's
/// tree, otherwise the sentinel mismatches and CI re-runs.
fn write_ci_sentinel_for_worktree(
    root: &std::path::Path,
    branch: &str,
    commit_cwd: &std::path::Path,
) {
    let snapshot = flow_rs::ci::tree_snapshot(commit_cwd, None);
    let sentinel = flow_rs::ci::sentinel_path(root, branch);
    fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
    fs::write(&sentinel, &snapshot).unwrap();
}

/// Set up a real worktree fixture for tests of finalize-commit's
/// git-resolved worktree routing. The base call (`setup_integration_repo_with_ci`)
/// produces a clone on `main` with a controllable `bin/test` script.
/// This helper then creates `branch` in the clone, pushes it to origin
/// so `git pull` has an upstream, and `git worktree add`s a linked
/// worktree at `<clone>/.worktrees/<branch>/`. Returns the clone TempDir,
/// the bare TempDir, and the worktree path (must all be kept alive).
///
/// `branch` must be a non-`main` branch name: git refuses
/// `worktree add` when the target branch is already checked out at the
/// main clone path.
fn setup_worktree_fixture(branch: &str) -> (tempfile::TempDir, tempfile::TempDir, PathBuf) {
    let (clone_dir, bare_dir) = setup_integration_repo_with_ci();
    let clone_path = clone_dir.path();
    let clone_str = clone_path.to_str().unwrap();

    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_str, "branch", branch])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_str, "push", "-u", "origin", branch])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let worktree_path = clone_path.join(".worktrees").join(branch);
    git_assert_ok(
        &Command::new("git")
            .args([
                "-C",
                clone_str,
                "worktree",
                "add",
                worktree_path.to_str().unwrap(),
                branch,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    // Configure worktree-local git identity so commits work even when
    // global git config is absent (CI environments).
    let wt_str = worktree_path.to_str().unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "config", "user.email", "test@test.com"])
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "config", "user.name", "Test"])
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "config", "pull.rebase", "false"])
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "config", "commit.gpgSign", "false"])
            .output()
            .unwrap(),
    );

    (clone_dir, bare_dir, worktree_path)
}

/// Return the SHA at HEAD of the repo at `cwd`. Panics on git failure.
fn head_sha(cwd: &std::path::Path) -> String {
    let out = Command::new("git")
        .args(["-C", cwd.to_str().unwrap(), "rev-parse", "HEAD"])
        .output()
        .unwrap();
    git_assert_ok(&out);
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write_state_with_continue_pending(clone_path: &std::path::Path, branch: &str) {
    let branch_dir = clone_path.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    let state_file = branch_dir.join("state.json");
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
        .join(branch)
        .join("state.json");
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
/// controlled git stub on PATH. The caller writes the message file to
/// `<commit_cwd>/.flow-commit-msg` before invoking — finalize-commit
/// derives the path from the commit cwd, not from an argument.
fn run_finalize_with_stub(
    clone_path: &Path,
    branch: &str,
    stubs: &Path,
    env: &[(&str, &str)],
) -> std::process::Output {
    let current_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", stubs.display(), current_path);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.args(["finalize-commit", branch])
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
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("happy-path");
    let clone_path = clone_dir.path();

    fs::write(worktree_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", worktree_path.to_str().unwrap(), "add", "-A"])
            .output()
            .unwrap(),
    );

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Test commit.").unwrap();

    write_ci_sentinel_for_worktree(clone_path, "happy-path", &worktree_path);

    let args = Args {
        branch: "happy-path".to_string(),
    };
    let result = run_impl(&args, clone_path).unwrap();
    assert_eq!(result["status"], "ok", "got: {}", result);
    assert_eq!(result["pull_merged"], false);
    // Message file was removed after commit.
    assert!(!msg_path.exists());
}

// --- run_impl: git-based commit-destination resolution ---

/// A feature branch checked out at the repo ROOT (not in a
/// `.worktrees/<branch>/` worktree) must commit correctly. Resolving the
/// destination from git's actual checkout location — rather than
/// assuming `<root>/.worktrees/feat-at-root`, which does not exist for a
/// root checkout — lands the commit at the repo root and proceeds past
/// the working-tree-dirty gate to a clean commit.
#[test]
fn finalize_commit_routes_to_feature_branch_checked_out_at_repo_root() {
    let (clone_dir, _bare_dir) = setup_integration_repo_with_ci();
    let clone_path = clone_dir.path();
    let clone_str = clone_path.to_str().unwrap();

    // Check out a feature branch AT the repo root (no linked worktree).
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_str, "checkout", "-b", "feat-at-root"])
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_str, "push", "-u", "origin", "feat-at-root"])
            .output()
            .unwrap(),
    );

    fs::write(clone_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_str, "add", "-A"])
            .output()
            .unwrap(),
    );

    let msg_path = clone_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Feature-at-root commit.").unwrap();

    // The branch resolves to the repo root, so commit_cwd is the clone root.
    write_ci_sentinel_for_worktree(clone_path, "feat-at-root", clone_path);

    let args = Args {
        branch: "feat-at-root".to_string(),
    };
    let result = run_impl(&args, clone_path).unwrap();
    assert_eq!(result["status"], "ok", "got: {}", result);
    assert!(!msg_path.exists());
}

/// When the branch is not checked out in any worktree, the resolver
/// returns `Ok(None)` and run_impl emits a `resolve_cwd` error keyed
/// `branch_not_checked_out` BEFORE the working-tree-dirty gate. The
/// message names the branch and must NOT mention "unstaged changes",
/// which would indicate a misroute into the working-tree-dirty gate
/// against a nonexistent worktree path.
#[test]
fn finalize_commit_errors_when_branch_not_checked_out() {
    let (clone_dir, _bare_dir) = setup_integration_repo_with_ci();
    let clone_path = clone_dir.path();

    let msg_path = clone_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Should not commit.").unwrap();

    let args = Args {
        branch: "never-checked-out".to_string(),
    };
    let result = run_impl(&args, clone_path).unwrap();
    assert_eq!(result["status"], "error", "got: {}", result);
    assert_eq!(result["step"], "resolve_cwd");
    assert_eq!(result["reason"], "branch_not_checked_out");
    let msg = result["message"].as_str().unwrap();
    assert!(
        msg.contains("never-checked-out"),
        "message should name the branch: {}",
        msg
    );
    assert!(
        !msg.contains("unstaged changes"),
        "message must not mention unstaged changes: {}",
        msg
    );
}

/// When git itself cannot resolve worktrees (here: `root` is not a git
/// repository, so `git worktree list --porcelain` exits non-zero), the
/// resolver returns `Err` and run_impl emits a `resolve_cwd` error with
/// NO `branch_not_checked_out` reason — distinguishing a git failure
/// from a legitimately-absent branch.
#[test]
fn finalize_commit_errors_when_worktree_resolution_fails() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let msg_path = root.join(".flow-commit-msg");
    fs::write(&msg_path, "Should not commit.").unwrap();

    let args = Args {
        branch: "some-branch".to_string(),
    };
    let result = run_impl(&args, root).unwrap();
    assert_eq!(result["status"], "error", "got: {}", result);
    assert_eq!(result["step"], "resolve_cwd");
    assert!(
        result.get("reason").is_none(),
        "Err arm must not set a reason field: {}",
        result
    );
}

// --- run_impl: git-resolved worktree routing ---

/// `run_impl` resolves `commit_cwd` by asking git where the branch is
/// checked out (`resolve_worktree_for_branch`), so the commit lands on
/// the branch's worktree HEAD even when the caller's cwd is an
/// unrelated sibling tempdir outside both the main clone and the
/// worktree. The branch argument — not the caller cwd — determines the
/// commit destination.
#[test]
fn finalize_commit_routes_to_worktree_when_caller_cwd_differs() {
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("feature-routing");
    let clone_path = clone_dir.path();

    fs::write(worktree_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", worktree_path.to_str().unwrap(), "add", "-A"])
            .output()
            .unwrap(),
    );

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Routing-from-sibling commit.").unwrap();

    write_ci_sentinel_for_worktree(clone_path, "feature-routing", &worktree_path);

    let main_sha_before = head_sha(clone_path);
    let worktree_sha_before = head_sha(&worktree_path);

    let args = Args {
        branch: "feature-routing".to_string(),
    };
    let result = run_impl(&args, clone_path).unwrap();
    assert_eq!(result["status"], "ok", "got: {}", result);

    let main_sha_after = head_sha(clone_path);
    let worktree_sha_after = head_sha(&worktree_path);

    assert_eq!(
        main_sha_before, main_sha_after,
        "main HEAD must not advance — commit must land in worktree"
    );
    assert_ne!(
        worktree_sha_before, worktree_sha_after,
        "worktree HEAD must advance"
    );
}

/// Monorepo case: the caller's cwd is a subdirectory of the main
/// clone (e.g. `<clone>/hub/`). `run_impl` resolves the commit
/// destination from the `--branch` argument via git, so the commit
/// lands on the feature-branch worktree regardless of where the
/// caller's shell sits inside the main clone.
#[test]
fn finalize_commit_routes_to_worktree_when_caller_cwd_inside_main_subdir() {
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("feature-monorepo");
    let clone_path = clone_dir.path();

    let subdir = clone_path.join("hub");
    fs::create_dir_all(&subdir).unwrap();
    fs::write(subdir.join("placeholder.txt"), "placeholder").unwrap();

    fs::write(worktree_path.join("api.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", worktree_path.to_str().unwrap(), "add", "-A"])
            .output()
            .unwrap(),
    );

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Monorepo subdir commit.").unwrap();

    write_ci_sentinel_for_worktree(clone_path, "feature-monorepo", &worktree_path);

    let main_sha_before = head_sha(clone_path);
    let worktree_sha_before = head_sha(&worktree_path);

    let args = Args {
        branch: "feature-monorepo".to_string(),
    };
    let result = run_impl(&args, clone_path).unwrap();
    assert_eq!(result["status"], "ok", "got: {}", result);

    let main_sha_after = head_sha(clone_path);
    let worktree_sha_after = head_sha(&worktree_path);

    assert_eq!(
        main_sha_before, main_sha_after,
        "main HEAD must not advance — commit must land in worktree"
    );
    assert_ne!(
        worktree_sha_before, worktree_sha_after,
        "worktree HEAD must advance even when caller cwd is inside main subdir"
    );
}

/// Two feature worktrees coexist (`feature-a` and `feature-b`).
/// `run_impl` resolves the destination from the `--branch` argument
/// via git, so naming `feature-b` lands the commit on worktree B and
/// leaves worktree A untouched — two active worktrees never produce
/// an ambiguous destination.
#[test]
fn finalize_commit_routes_to_worktree_when_caller_cwd_on_other_feature_branch() {
    let (clone_dir, _bare_dir, worktree_a) = setup_worktree_fixture("feature-a");
    let clone_path = clone_dir.path();
    let clone_str = clone_path.to_str().unwrap();

    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_str, "branch", "feature-b"])
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_str, "push", "-u", "origin", "feature-b"])
            .output()
            .unwrap(),
    );
    let worktree_b = clone_path.join(".worktrees").join("feature-b");
    git_assert_ok(
        &Command::new("git")
            .args([
                "-C",
                clone_str,
                "worktree",
                "add",
                worktree_b.to_str().unwrap(),
                "feature-b",
            ])
            .output()
            .unwrap(),
    );
    let wt_b_str = worktree_b.to_str().unwrap();
    for (key, val) in [
        ("user.email", "test@test.com"),
        ("user.name", "Test"),
        ("pull.rebase", "false"),
        ("commit.gpgSign", "false"),
    ] {
        git_assert_ok(
            &Command::new("git")
                .args(["-C", wt_b_str, "config", key, val])
                .output()
                .unwrap(),
        );
    }

    fs::write(worktree_b.join("b.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_b_str, "add", "-A"])
            .output()
            .unwrap(),
    );

    let msg_path = worktree_b.join(".flow-commit-msg");
    fs::write(&msg_path, "Sibling-worktree commit.").unwrap();

    write_ci_sentinel_for_worktree(clone_path, "feature-b", &worktree_b);

    let main_sha_before = head_sha(clone_path);
    let worktree_a_sha_before = head_sha(&worktree_a);
    let worktree_b_sha_before = head_sha(&worktree_b);

    let args = Args {
        branch: "feature-b".to_string(),
    };
    let result = run_impl(&args, clone_path).unwrap();
    assert_eq!(result["status"], "ok", "got: {}", result);

    let main_sha_after = head_sha(clone_path);
    let worktree_a_sha_after = head_sha(&worktree_a);
    let worktree_b_sha_after = head_sha(&worktree_b);

    assert_eq!(
        main_sha_before, main_sha_after,
        "main HEAD must not advance"
    );
    assert_eq!(
        worktree_a_sha_before, worktree_a_sha_after,
        "sibling worktree A must not advance — branch arg names B"
    );
    assert_ne!(
        worktree_b_sha_before, worktree_b_sha_after,
        "target worktree B must advance — branch arg names B"
    );
}

// --- run_impl: working_tree_dirty gate ---

/// Stage a file with one content, then modify the working tree
/// without re-staging. The gate's `git diff --quiet` call sees
/// the unstaged divergence and returns the structured error
/// without invoking CI or `git commit`. HEAD must not advance —
/// the whole point of the gate is that CI tests one set of
/// bytes (the working tree) and `git commit` would commit a
/// different set (the index).
#[test]
fn working_tree_dirty_blocks_commit_when_index_differs() {
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("dirty-tree");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    fs::write(worktree_path.join("feature.rs"), "initial\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "-A"])
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "commit", "-m", "seed feature.rs"])
            .output()
            .unwrap(),
    );

    fs::write(worktree_path.join("feature.rs"), "staged-bad\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "feature.rs"])
            .output()
            .unwrap(),
    );

    fs::write(worktree_path.join("feature.rs"), "working-tree-good\n").unwrap();

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Test commit.").unwrap();

    let sha_before = head_sha(&worktree_path);

    let args = Args {
        branch: "dirty-tree".to_string(),
    };
    let result = run_impl(&args, clone_path).unwrap();

    assert_eq!(result["status"], "error", "got: {}", result);
    assert_eq!(result["step"], "working_tree_dirty");
    let msg = result["message"].as_str().unwrap();
    assert!(
        msg.contains("git add"),
        "message must name `git add` recovery: {}",
        msg
    );
    assert!(
        msg.contains("git restore"),
        "message must name `git restore` recovery: {}",
        msg
    );

    let sha_after = head_sha(&worktree_path);
    assert_eq!(sha_before, sha_after, "HEAD must not have advanced");

    // The working-tree-dirty gate is reached after commit_cwd resolves,
    // so it flows through to run_impl's tail deletion: the message file
    // is gone after every post-resolution exit.
    assert!(!msg_path.exists());
}

// --- run_impl: message_file_missing gate ---

/// After `commit_cwd` resolves, the commit-message file path is
/// `<commit_cwd>/.flow-commit-msg` — derived, not caller-supplied.
/// When no file exists at that path (the skill failed to write it, or
/// a prior run already consumed it), run_impl returns a structured
/// `message_file_missing` error that names the expected path. The gate
/// fires before any other gate (working-tree-dirty, CI), so the caller
/// sees a precise reason rather than a downstream `git commit` failure.
#[test]
fn message_file_missing_returns_structured_error() {
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("msg-missing");
    let clone_path = clone_dir.path();

    // Intentionally do NOT write `<worktree>/.flow-commit-msg`.
    let args = Args {
        branch: "msg-missing".to_string(),
    };
    let result = run_impl(&args, clone_path).unwrap();
    assert_eq!(result["status"], "error", "got: {}", result);
    assert_eq!(result["step"], "message_file_missing");
    let msg = result["message"].as_str().unwrap();
    assert!(
        msg.contains(".flow-commit-msg"),
        "message must name the expected path: {}",
        msg
    );
    assert!(
        msg.contains(&worktree_path.to_string_lossy().to_string()),
        "message must name the commit cwd: {}",
        msg
    );
}

/// An empty `.flow-commit-msg` (present but zero bytes) is treated the
/// same as a missing file — `git commit -F` would refuse an empty
/// message, so the gate catches it as `message_file_missing` rather
/// than letting the commit step fail with a less precise reason.
#[test]
fn message_file_empty_returns_structured_error() {
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("msg-empty");
    let clone_path = clone_dir.path();

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "").unwrap();

    let args = Args {
        branch: "msg-empty".to_string(),
    };
    let result = run_impl(&args, clone_path).unwrap();
    assert_eq!(result["status"], "error", "got: {}", result);
    assert_eq!(result["step"], "message_file_missing");
}

// --- run_impl: CI enforcement ---

#[test]
fn ci_fails_blocks_commit() {
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("ci-fail");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    fs::write(worktree_path.join(".ci-should-fail"), "1").unwrap();

    fs::write(worktree_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add feature.rs").unwrap();

    let before = Command::new("git")
        .args(["-C", wt_str, "log", "--oneline"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    git_assert_ok(&before);
    let commits_before = String::from_utf8_lossy(&before.stdout).lines().count();

    let args = Args {
        branch: "ci-fail".to_string(),
    };

    let result = run_impl(&args, clone_path).unwrap();
    assert_eq!(result["status"], "error", "expected CI failure: {}", result);
    assert_eq!(result["step"], "ci");

    let after = Command::new("git")
        .args(["-C", wt_str, "log", "--oneline"])
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
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("ci-sentinel");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    fs::write(worktree_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "feature.rs"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add feature.rs").unwrap();

    write_ci_sentinel_for_worktree(clone_path, "ci-sentinel", &worktree_path);

    let args = Args {
        branch: "ci-sentinel".to_string(),
    };

    let result = run_impl(&args, clone_path).unwrap();
    assert_eq!(result["status"], "ok", "commit should succeed: {}", result);
    assert!(
        !result["sha"].as_str().unwrap().is_empty(),
        "should have a commit SHA"
    );

    let marker = worktree_path.join(".ci-invocation-marker");
    assert!(
        !marker.exists(),
        "CI should not have been invoked (sentinel was fresh)"
    );
}

// --- run_impl: continue_pending state handling ---

#[test]
fn error_clears_continue_pending() {
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("err-clear");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    write_state_with_continue_pending(clone_path, "err-clear");

    fs::write(worktree_path.join(".ci-should-fail"), "1").unwrap();

    fs::write(worktree_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add feature.rs").unwrap();

    let args = Args {
        branch: "err-clear".to_string(),
    };

    let result = run_impl(&args, clone_path).unwrap();
    assert_eq!(result["status"], "error", "expected CI failure: {}", result);
    assert_eq!(result["step"], "ci");

    let state = read_state(clone_path, "err-clear");
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
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("ok-preserve");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    write_state_with_continue_pending(clone_path, "ok-preserve");

    fs::write(worktree_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add feature.rs").unwrap();

    write_ci_sentinel_for_worktree(clone_path, "ok-preserve", &worktree_path);

    let args = Args {
        branch: "ok-preserve".to_string(),
    };

    let result = run_impl(&args, clone_path).unwrap();
    assert_eq!(result["status"], "ok", "commit should succeed: {}", result);

    let state = read_state(clone_path, "ok-preserve");
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
    let (clone_dir, bare_dir, worktree_path) = setup_worktree_fixture("conflict-preserve");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    write_state_with_continue_pending(clone_path, "conflict-preserve");

    // Sibling clone that pushes a conflicting commit to origin's
    // feature branch, so the worktree's `git pull` brings down a
    // change that overlaps the local commit.
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
    let clone2_str = clone2_dir.path().to_str().unwrap();
    for (key, val) in [("user.email", "other@test.com"), ("user.name", "Other")] {
        git_assert_ok(
            &Command::new("git")
                .args(["-C", clone2_str, "config", key, val])
                .output()
                .unwrap(),
        );
    }
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone2_str, "checkout", "conflict-preserve"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    fs::write(
        clone2_dir.path().join("README.md"),
        "# Conflicting content\n",
    )
    .unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone2_str, "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone2_str, "commit", "-m", "Conflicting commit"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone2_str, "push"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    fs::write(
        worktree_path.join("README.md"),
        "# Local conflicting content\n",
    )
    .unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Local change to README").unwrap();

    write_ci_sentinel_for_worktree(clone_path, "conflict-preserve", &worktree_path);

    let args = Args {
        branch: "conflict-preserve".to_string(),
    };

    let result = run_impl(&args, clone_path).unwrap();
    assert_eq!(
        result["status"], "conflict",
        "expected conflict: {}",
        result
    );

    let state = read_state(clone_path, "conflict-preserve");
    assert_eq!(
        state["_continue_pending"], "commit",
        "_continue_pending should be preserved on conflict"
    );
}

// --- run_impl: sentinel refresh ---

#[test]
fn refreshes_sentinel_after_commit() {
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("refresh-sent");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    let src = worktree_path.join("src.rs");
    fs::write(&src, "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add src.rs").unwrap();

    write_ci_sentinel_for_worktree(clone_path, "refresh-sent", &worktree_path);

    let args = Args {
        branch: "refresh-sent".to_string(),
    };

    let result = run_impl(&args, clone_path).unwrap();
    assert_eq!(result["status"], "ok", "commit should succeed: {}", result);
    assert_eq!(result["pull_merged"], false);

    let sentinel = flow_rs::ci::sentinel_path(clone_path, "refresh-sent");
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
    let (clone_dir, bare_dir, worktree_path) = setup_worktree_fixture("no-refresh");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    // Sibling clone that pushes a non-conflicting commit to origin's
    // feature branch. The local pull merges it cleanly, so
    // pull_merged = true and the sentinel must be removed.
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
    let clone2_str = clone2_dir.path().to_str().unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone2_str, "config", "user.email", "other@test.com"])
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone2_str, "config", "user.name", "Other"])
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone2_str, "checkout", "no-refresh"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let other_file = clone2_dir.path().join("other.txt");
    fs::write(&other_file, "other content\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone2_str, "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone2_str, "commit", "-m", "Other commit"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone2_str, "push"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let src = worktree_path.join("local.txt");
    fs::write(&src, "local content\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add local.txt").unwrap();

    write_ci_sentinel_for_worktree(clone_path, "no-refresh", &worktree_path);

    let args = Args {
        branch: "no-refresh".to_string(),
    };

    let result = run_impl(&args, clone_path).unwrap();
    assert_eq!(result["status"], "ok", "commit should succeed: {}", result);
    assert_eq!(result["pull_merged"], true);

    let sentinel = flow_rs::ci::sentinel_path(clone_path, "no-refresh");
    assert!(
        !sentinel.exists(),
        "sentinel should not exist when pull merged"
    );
}

// --- run_impl: state-file not-object guard in mutate_state closure ---

/// Exercises the `if !(state.is_object() || state.is_null()) { return; }`
/// guard inside the mutate_state closure on error-cleanup. The state file
/// is written as a JSON array (not an object), so mutate_state invokes the
/// closure with state as an array, the guard returns early, and no
/// continuation-field reset is attempted.
#[test]
fn error_state_wrong_type_guard_fires() {
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("wrong-state");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    // Overwrite state file with a JSON ARRAY (not an object). mutate_state's
    // closure in run_impl will see state.is_array() and return early via
    // the type guard — no mutation applied, no panic.
    let branch_dir = clone_path.join(".flow-states").join("wrong-state");
    fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    fs::write(&state_path, "[1, 2, 3]").unwrap();

    // Configure CI to fail so run_impl hits the error-cleanup path.
    fs::write(worktree_path.join(".ci-should-fail"), "1").unwrap();

    fs::write(worktree_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add feature.rs").unwrap();

    let args = Args {
        branch: "wrong-state".to_string(),
    };

    let result = run_impl(&args, clone_path).unwrap();
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
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("plan-dev");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    let branch_dir = clone_path.join(".flow-states").join("plan-dev");
    fs::create_dir_all(&branch_dir).unwrap();
    let plan_path = branch_dir.join("plan.md");
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

    let state_file = branch_dir.join("state.json");
    let state = json!({
        "branch": "plan-dev",
        "current_phase": "flow-code",
        "files": {"plan": ".flow-states/plan-dev/plan.md"}
    });
    fs::write(&state_file, serde_json::to_string_pretty(&state).unwrap()).unwrap();

    let tests_dir = worktree_path.join("tests");
    fs::create_dir_all(&tests_dir).unwrap();
    let test_file = tests_dir.join("foo.rs");
    fs::write(
        &test_file,
        "fn test_foo() {\n    let actual = \"drifted\";\n}\n",
    )
    .unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add test_foo").unwrap();

    write_ci_sentinel_for_worktree(clone_path, "plan-dev", &worktree_path);

    let args = Args {
        branch: "plan-dev".to_string(),
    };

    let result = run_impl(&args, clone_path).unwrap();
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

    // The plan-deviation block path reaches run_impl's tail deletion —
    // the one exit whose deletion behavior changes under the new
    // design. The plan-deviation block never reaches `finalize_commit`
    // (where deletion used to live), so the file would have survived
    // before this PR; the tail deletion now disposes of it.
    assert!(!msg_path.exists());
}

/// Two-deviation companion to the single-deviation test. Exercises the
/// plural "s" branch of the `if deviations.len() == 1 { "" } else { "s" }`
/// expressions at the log line and the JSON "message" field — both are
/// the same pluralization pattern so both are covered by the same test.
#[test]
fn plan_deviation_two_deviations_plural_message() {
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("plan-dev2");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    let branch_dir = clone_path.join(".flow-states").join("plan-dev2");
    fs::create_dir_all(&branch_dir).unwrap();
    let plan_path = branch_dir.join("plan.md");
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

    let state_file = branch_dir.join("state.json");
    let state = json!({
        "branch": "plan-dev2",
        "current_phase": "flow-code",
        "files": {"plan": ".flow-states/plan-dev2/plan.md"}
    });
    fs::write(&state_file, serde_json::to_string_pretty(&state).unwrap()).unwrap();

    let tests_dir = worktree_path.join("tests");
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
            .args(["-C", wt_str, "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add test_alpha and test_beta").unwrap();

    write_ci_sentinel_for_worktree(clone_path, "plan-dev2", &worktree_path);

    let args = Args {
        branch: "plan-dev2".to_string(),
    };

    let result = run_impl(&args, clone_path).unwrap();
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

// --- run_impl: no-worktree integration-branch routing ---

/// A bootstrap commit runs on the integration branch checked out at
/// the project root, where no `<root>/.worktrees/<integration>`
/// directory ever exists. `resolve_worktree_for_branch` reports the
/// integration branch as checked out at the project root, so
/// finalize-commit routes its git operations there and the commit
/// proceeds past the working-tree-dirty gate. The fixture is a clone
/// on `main` with no `.worktrees/` directory and a clean staged tree
/// (index == working tree). The response must NOT be
/// `step:"working_tree_dirty"`: that would mean the commit was
/// misrouted to a directory whose tree differs from the root's.
/// Proceeding past the dirty gate (to the CI gate / a clean commit)
/// proves the destination resolved to the project root.
#[test]
fn finalize_commit_routes_to_root_when_no_worktree_exists_for_integration_branch() {
    let (clone_dir, _bare_dir) = setup_integration_repo_with_ci();
    let clone_path = clone_dir.path();
    let clone_str = clone_path.to_str().unwrap();

    // Set refs/remotes/origin/HEAD -> main for a realistic
    // integration-branch fixture. The commit routes to the root
    // because git reports `main` checked out there (resolved via
    // `git worktree list --porcelain`), not because of origin/HEAD.
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_str, "remote", "set-head", "origin", "main"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    fs::write(clone_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", clone_str, "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = clone_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Bootstrap commit on integration branch.").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["finalize-commit", "main"])
        .current_dir(clone_path)
        .env("HOME", clone_path)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("spawn flow-rs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_ne!(
        json["step"], "working_tree_dirty",
        "no-worktree integration commit must route to the project root, \
         not a nonexistent worktree; got: {json}"
    );
}

// --- run_impl: commit-step error paths (via git stub) ---

/// With git absent from PATH, `resolve_worktree_for_branch` (the first
/// git call in `run_impl`) cannot spawn `git worktree list --porcelain`,
/// so it returns `Err` and `run_impl` emits a `resolve_cwd` error before
/// the working-tree-dirty gate, the CI gate, or `finalize_commit` ever
/// run. The message file is preserved because the error returns before
/// `finalize_commit` (the only step that removes it).
#[test]
fn git_unavailable_returns_resolve_cwd_error() {
    let (clone_dir, _bare_dir) = setup_integration_repo_with_ci();
    let clone_path = clone_dir.path();

    let msg_path = clone_path.join(".flow-commit-msg");
    fs::write(&msg_path, "msg").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["finalize-commit", "main"])
        .current_dir(clone_path)
        .env("PATH", "")
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("spawn flow-rs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error", "got: {}", json);
    assert_eq!(json["step"], "resolve_cwd");
    // Message file is preserved on the resolve_cwd path — the error
    // returns before finalize_commit(), the only step that removes it.
    assert!(msg_path.exists());
}

/// Commit fails with nonzero exit + stderr. Covers the
/// `Ok((code, _, stderr))` commit-error branch.
#[test]
fn commit_nonzero_returns_error_step_commit() {
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("commit-fail");
    let clone_path = clone_dir.path();

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "msg").unwrap();

    write_ci_sentinel_for_worktree(clone_path, "commit-fail", &worktree_path);

    let stubs = write_git_stub(clone_path);

    let output = run_finalize_with_stub(
        clone_path,
        "commit-fail",
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
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("pull-fail");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    fs::write(worktree_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "msg").unwrap();

    write_ci_sentinel_for_worktree(clone_path, "pull-fail", &worktree_path);

    let stubs = write_git_stub(clone_path);

    let output = run_finalize_with_stub(
        clone_path,
        "pull-fail",
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
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("pull-conflict");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    fs::write(worktree_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "msg").unwrap();

    write_ci_sentinel_for_worktree(clone_path, "pull-conflict", &worktree_path);

    let stubs = write_git_stub(clone_path);

    let output = run_finalize_with_stub(
        clone_path,
        "pull-conflict",
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
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("push-fail");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    fs::write(worktree_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "msg").unwrap();

    write_ci_sentinel_for_worktree(clone_path, "push-fail", &worktree_path);

    let stubs = write_git_stub(clone_path);

    let output = run_finalize_with_stub(
        clone_path,
        "push-fail",
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
fn empty_branch_returns_err() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let args = Args {
        branch: String::new(),
    };
    let err = run_impl(&args, &root).unwrap_err();
    assert!(err.contains("finalize-commit"));
}

// --- run_impl_main (subprocess) ---

fn flow_rs_no_recursion() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd
}

/// Exercises run_impl's slash-branch arm: a clap-supplied branch
/// containing `/` (e.g., `feature/foo`) reaches `FlowPaths::try_new`
/// which returns None. run_impl pattern-matches and surfaces a
/// structured "Invalid branch name" error per
/// `.claude/rules/external-input-validation.md` "CLI subcommand
/// entry callsite discipline" — never a panic.
#[test]
fn finalize_commit_slash_branch_returns_invalid_branch_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let output = flow_rs_no_recursion()
        .args(["finalize-commit", "feature/foo"])
        .current_dir(&root)
        .env("GIT_CEILING_DIRECTORIES", &root)
        .env("GH_TOKEN", "invalid")
        .env("HOME", &root)
        .output()
        .expect("spawn flow-rs");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked at"),
        "finalize-commit panicked on slash branch; stderr: {}",
        stderr
    );
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let last = stdout
        .lines()
        .rfind(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON in stdout: {}", stdout));
    let json: Value = serde_json::from_str(last).unwrap();
    assert_eq!(json["status"], "error");
    assert!(
        json["message"]
            .as_str()
            .unwrap_or("")
            .contains("Invalid branch name"),
        "expected Invalid branch error, got: {:?}",
        json
    );
}

// Exercises run_impl_main's Err arm: an empty branch arg → run_impl
// returns Err → run_impl_main wraps as {"step":"args"} + exit 1.
#[test]
fn run_impl_main_empty_args_exits_1_with_args_step() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();

    let output = flow_rs_no_recursion()
        .args(["finalize-commit", ""])
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
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("main-err");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    fs::write(worktree_path.join(".ci-should-fail"), "1").unwrap();

    fs::write(worktree_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add feature.rs").unwrap();

    let output = flow_rs_no_recursion()
        .args(["finalize-commit", "main-err"])
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
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("main-ok");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    fs::write(worktree_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "-A"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add feature.rs").unwrap();

    write_ci_sentinel_for_worktree(clone_path, "main-ok", &worktree_path);

    let output = flow_rs_no_recursion()
        .args(["finalize-commit", "main-ok"])
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

// --- CI reason banner ---

#[test]
fn finalize_commit_passes_ci_reason() {
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("ci-reason");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    fs::write(worktree_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "-A"])
            .output()
            .unwrap(),
    );

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Test commit.").unwrap();

    // No sentinel — CI runs and the explicit reason wins.

    let output = flow_rs_no_recursion()
        .args(["finalize-commit", "ci-reason"])
        .current_dir(clone_path)
        .output()
        .expect("spawn flow-rs");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("CI: verifying commit before git commit\n"),
        "expected finalize_commit's explicit reason banner; stderr=\n{}",
        stderr
    );
}

// --- run_impl: post-CI re-stage ---

/// Regression test: finalize-commit must re-stage tracked files
/// that CI modifies during its auto-fix pass (the canonical pattern
/// in target repos: `CI=true → strict / CI unset → auto-fix in
/// place`). Without re-staging between CI completion and the
/// `git diff --cached` capture, `git commit -F` records the pre-CI
/// index bytes while CI tested the post-CI working-tree bytes; the
/// remote strict CI then fails on the unfixed content.
///
/// Fixture: overwrite the worktree's `bin/test` with a script that
/// appends a newline to `README.md` on every invocation. Stage a
/// separate `feature.rs` so `git commit` has something to land
/// independent of the README modification. Skip the CI sentinel so
/// CI actually runs and triggers the auto-fix.
///
/// Asserts `git show HEAD:README.md` returns the appended-newline
/// bytes. Fails against current code because the commit captures
/// the pre-CI staged bytes.
#[test]
fn finalize_commit_restages_after_ci_modifies_tracked_file() {
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("restage-after-ci");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    // Replace bin/test with an auto-fix script that unconditionally
    // appends a newline to README.md. CI dispatches bin/test as
    // part of its [format, lint, build, test] sequence, so the
    // script fires during run_impl's CI gate. Commit and push the
    // modified script so the worktree starts clean (the
    // working-tree-dirty gate would otherwise block the run).
    let bin_test = worktree_path.join("bin").join("test");
    let auto_fix_script = r#"#!/usr/bin/env bash
printf '\n' >> "$(dirname "$0")/../README.md"
exit 0
"#;
    fs::write(&bin_test, auto_fix_script).unwrap();
    #[cfg(unix)]
    {
        fs::set_permissions(&bin_test, fs::Permissions::from_mode(0o755)).unwrap();
    }
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "bin/test"])
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "commit", "-m", "Configure auto-fix bin/test"])
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "push"])
            .output()
            .unwrap(),
    );

    // Stage a new file so `git commit` has content to land
    // independent of the README modification. Without this, the
    // commit's index would be empty before re-staging and `git
    // commit -F` would refuse with "nothing to commit".
    fs::write(worktree_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "feature.rs"])
            .output()
            .unwrap(),
    );

    // Capture README's bytes at HEAD (and in the index, since the
    // working tree matches HEAD after the auto-fix script commit).
    let pre_fix_output = Command::new("git")
        .args(["-C", wt_str, "show", "HEAD:README.md"])
        .output()
        .unwrap();
    git_assert_ok(&pre_fix_output);
    let pre_fix_bytes = String::from_utf8_lossy(&pre_fix_output.stdout).to_string();
    let expected_post_fix_bytes = format!("{}\n", pre_fix_bytes);

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add feature.rs (post-CI re-stage test).").unwrap();

    // Do NOT write a CI sentinel — CI must actually run so bin/test
    // fires the auto-fix and modifies README.md in the working
    // tree.

    let args = Args {
        branch: "restage-after-ci".to_string(),
    };
    let result = run_impl(&args, clone_path).unwrap();
    assert_eq!(result["status"], "ok", "got: {}", result);

    let head_readme = Command::new("git")
        .args(["-C", wt_str, "show", "HEAD:README.md"])
        .output()
        .unwrap();
    git_assert_ok(&head_readme);
    let committed_bytes = String::from_utf8_lossy(&head_readme.stdout).to_string();
    assert_eq!(
        committed_bytes, expected_post_fix_bytes,
        "commit must capture post-CI bytes (re-staged) — pre={:?}, expected_post={:?}, committed={:?}",
        pre_fix_bytes, expected_post_fix_bytes, committed_bytes,
    );
}

/// Invariant: projects whose `bin/*` tools never modify the working
/// tree (the strict `--check` shape) experience zero behavior change
/// from the post-CI re-stage. The committed file must read exactly the
/// pre-staged bytes, and `git add -A` must not sweep extra files into
/// the commit. Locks the non-regression contract for checker-only
/// projects against future changes that might widen what the re-stage
/// includes.
#[test]
fn finalize_commit_restage_noop_when_ci_leaves_working_tree_clean() {
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("restage-noop");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    // The fixture's default bin/test only writes a marker file
    // (gitignored via .git/info/exclude) — no working-tree
    // modifications to tracked files. Stage a source file with
    // known bytes and verify those exact bytes land in the commit.
    let staged_path = worktree_path.join("source.rs");
    let staged_bytes = "fn main() {}\n";
    fs::write(&staged_path, staged_bytes).unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "source.rs"])
            .output()
            .unwrap(),
    );

    // The commit-message file lives at `<commit_cwd>/.flow-commit-msg`
    // — the canonical location finalize-commit derives from its commit
    // cwd. It is untracked and deleted at run_impl's tail, and
    // `git add -u` cannot sweep an untracked file, so the tree-listing
    // assertion below sees only the staged source.rs.
    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add source.rs (no-op re-stage test).").unwrap();

    write_ci_sentinel_for_worktree(clone_path, "restage-noop", &worktree_path);

    let args = Args {
        branch: "restage-noop".to_string(),
    };
    let result = run_impl(&args, clone_path).unwrap();
    assert_eq!(result["status"], "ok", "got: {}", result);

    // Committed file bytes must be exactly what was staged.
    let head_source = Command::new("git")
        .args(["-C", wt_str, "show", "HEAD:source.rs"])
        .output()
        .unwrap();
    git_assert_ok(&head_source);
    let committed_bytes = String::from_utf8_lossy(&head_source.stdout).to_string();
    assert_eq!(
        committed_bytes, staged_bytes,
        "committed bytes must match staged bytes when CI does not modify",
    );

    // The new commit's tree must list only source.rs — `git add -A`
    // must not sweep any other working-tree entries (the marker file
    // is gitignored; verify nothing else slipped through).
    let tree_listing = Command::new("git")
        .args([
            "-C",
            wt_str,
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "-r",
            "HEAD",
        ])
        .output()
        .unwrap();
    git_assert_ok(&tree_listing);
    let names = String::from_utf8_lossy(&tree_listing.stdout).to_string();
    let listed: Vec<&str> = names.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(
        listed,
        vec!["source.rs"],
        "commit must include only the staged source.rs; got: {:?}",
        listed,
    );
}

/// Stub a `git` binary on PATH that exits non-zero on every `add`
/// invocation while forwarding every other subcommand to real git.
/// Returns the directory to prepend to PATH.
fn write_git_add_failing_stub(parent: &Path) -> PathBuf {
    let stubs = parent.join("git-add-fail-stubs");
    fs::create_dir_all(&stubs).unwrap();
    let script = r#"#!/bin/sh
REPO_PATH=""
if [ "$1" = "-C" ]; then
    REPO_PATH="$2"
    shift 2
fi
SUBCMD="$1"
shift
if [ "$SUBCMD" = "add" ]; then
    echo "simulated git add -A failure" >&2
    exit 1
fi
exec /usr/bin/git -C "$REPO_PATH" "$SUBCMD" "$@"
"#;
    let git_path = stubs.join("git");
    fs::write(&git_path, script).unwrap();
    #[cfg(unix)]
    {
        fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    stubs
}

/// Coverage for the `add_code != 0` branch added by the post-CI
/// re-stage. Skips CI via the sentinel, installs a `git` stub on
/// PATH that fails on `add` invocations, and runs finalize-commit
/// as a subprocess so the stub is reachable. Asserts the structured
/// `step:"restage"` error envelope AND the post-error side effect:
/// the error-tail path at the end of `run_impl` must clear
/// `_continue_pending` and `_continue_context` for every error
/// status, restage included. Mirrors the assertion shape of the
/// sibling `error_clears_continue_pending` test for the ci-fail
/// branch so the restage-failure path is locked into the same
/// state-clearing contract.
#[test]
fn finalize_commit_restage_failure_returns_step_restage() {
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("restage-fail");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    // Seed _continue_pending="commit" so the post-error clearing
    // assertion below has something to verify.
    write_state_with_continue_pending(clone_path, "restage-fail");

    let staged_path = worktree_path.join("feature.rs");
    fs::write(&staged_path, "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "feature.rs"])
            .output()
            .unwrap(),
    );

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add feature.rs (restage-fail test).").unwrap();

    write_ci_sentinel_for_worktree(clone_path, "restage-fail", &worktree_path);

    let stubs = write_git_add_failing_stub(clone_path);

    let output = run_finalize_with_stub(clone_path, "restage-fail", &stubs, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let result = last_json_line(&stdout);

    assert_eq!(result["status"], "error", "got: {}", result);
    assert_eq!(result["step"], "restage", "got: {}", result);
    let msg = result["message"]
        .as_str()
        .unwrap_or_else(|| panic!("message must be a string, got: {}", result));
    assert!(
        !msg.is_empty(),
        "restage error message must be non-empty, got: {:?}",
        msg,
    );

    // Verify the error-tail clearing fires on the restage-failure
    // path. The stop-continue hook reads _continue_pending —
    // leaving "commit" set after an error would force-advance the
    // parent phase past the failed commit.
    let state = read_state(clone_path, "restage-fail");
    let pending = state
        .get("_continue_pending")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        pending.is_empty(),
        "_continue_pending must be cleared on restage error, got: {:?}",
        pending,
    );
    let ctx = state
        .get("_continue_context")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        ctx.is_empty(),
        "_continue_context must be cleared on restage error, got: {:?}",
        ctx,
    );
}

/// Regression test: `git add -u` (the re-stage primitive) updates
/// already-tracked files only. An untracked, non-gitignored file
/// present in the worktree when CI completes must NOT land in the
/// commit's tree — the bound prevents the re-stage from silently
/// sweeping scratch files, generated artifacts, or editor-temp
/// files into commits the user never reviewed in
/// `/flow:flow-commit` Round 4. Also asserts the worktree-root
/// commit-message file (`.flow-commit-msg`) stays out of HEAD even
/// though it lives at a non-gitignored path inside the worktree.
#[test]
fn finalize_commit_restage_does_not_sweep_untracked_artifacts() {
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("restage-untracked-bound");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    // Stage a tracked source file so the commit has explicit
    // user-staged content to land.
    fs::write(worktree_path.join("feature.rs"), "fn main() {}\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "feature.rs"])
            .output()
            .unwrap(),
    );

    // Drop an untracked, non-gitignored scratch file in the
    // worktree. With `git add -A` this would sweep into HEAD; with
    // `git add -u` it must not.
    fs::write(
        worktree_path.join("scratch-notes.txt"),
        "unrelated leftover\n",
    )
    .unwrap();

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add feature.rs (untracked-bound test).").unwrap();

    write_ci_sentinel_for_worktree(clone_path, "restage-untracked-bound", &worktree_path);

    let args = Args {
        branch: "restage-untracked-bound".to_string(),
    };
    let result = run_impl(&args, clone_path).unwrap();
    assert_eq!(result["status"], "ok", "got: {}", result);

    // Assert HEAD's tree does NOT include the untracked scratch
    // file or the worktree-root commit-message file.
    let tree_listing = Command::new("git")
        .args(["-C", wt_str, "ls-tree", "-r", "--name-only", "HEAD"])
        .output()
        .unwrap();
    git_assert_ok(&tree_listing);
    let names = String::from_utf8_lossy(&tree_listing.stdout).to_string();
    let listed: Vec<&str> = names.lines().collect();
    assert!(
        !listed.contains(&"scratch-notes.txt"),
        "HEAD must not include untracked scratch file; tree was: {}",
        names,
    );
    assert!(
        !listed.contains(&".flow-commit-msg"),
        "HEAD must not include untracked commit-message file; tree was: {}",
        names,
    );
    assert!(
        listed.contains(&"feature.rs"),
        "HEAD must include the user-staged feature.rs; tree was: {}",
        names,
    );
}

/// Locks the invariant that the pre-existing working-tree-dirty gate
/// at the top of run_impl continues to fire before CI runs, even with
/// the post-CI re-stage in place. A future refactor that moved the
/// re-stage in front of the gate would silently land the working
/// tree's unstaged edits in the commit; this test catches that.
#[test]
fn finalize_commit_working_tree_dirty_gate_still_fires_pre_ci() {
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("dirty-pre-ci");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    // Commit a baseline README contents so we have a stable index
    // entry to diverge from.
    fs::write(worktree_path.join("README.md"), "baseline\n").unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "README.md"])
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "commit", "-m", "Baseline README"])
            .output()
            .unwrap(),
    );

    // Modify the working tree without staging — index still has the
    // baseline bytes, working tree has different bytes.
    fs::write(worktree_path.join("README.md"), "unstaged user edit\n").unwrap();

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Baseline + dirty tree.").unwrap();

    let args = Args {
        branch: "dirty-pre-ci".to_string(),
    };
    let result = run_impl(&args, clone_path).unwrap();

    assert_eq!(result["status"], "error", "got: {}", result);
    assert_eq!(result["step"], "working_tree_dirty", "got: {}", result);
}

/// Topology test: `.gitignore` correctly bounds the `git add -A` sweep
/// added by the post-CI re-stage. A CI tool that writes a gitignored
/// artifact (e.g. `coverage.profraw`) must not introduce that artifact
/// into the commit. Locks the bound between "files CI auto-fixes (must
/// be re-staged)" and "files CI happens to drop in the working tree
/// (must not be swept)".
#[test]
fn finalize_commit_restage_respects_gitignore_for_ci_artifacts() {
    let (clone_dir, _bare_dir, worktree_path) = setup_worktree_fixture("restage-gitignore");
    let clone_path = clone_dir.path();
    let wt_str = worktree_path.to_str().unwrap();

    // Track a .gitignore that excludes a CI-artifact pattern.
    fs::write(
        worktree_path.join(".gitignore"),
        ".flow-states/\ncoverage.profraw\n",
    )
    .unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", ".gitignore"])
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "commit", "-m", "Track gitignore"])
            .output()
            .unwrap(),
    );

    // Replace bin/test with a script that drops a gitignored CI
    // artifact into the worktree on every invocation.
    let bin_test = worktree_path.join("bin").join("test");
    let artifact_script = r#"#!/usr/bin/env bash
printf 'coverage data' > "$(dirname "$0")/../coverage.profraw"
exit 0
"#;
    fs::write(&bin_test, artifact_script).unwrap();
    #[cfg(unix)]
    {
        fs::set_permissions(&bin_test, fs::Permissions::from_mode(0o755)).unwrap();
    }
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "bin/test"])
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args([
                "-C",
                wt_str,
                "commit",
                "-m",
                "Configure artifact-emitting bin/test",
            ])
            .output()
            .unwrap(),
    );
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "push"])
            .output()
            .unwrap(),
    );

    let staged_path = worktree_path.join("source.rs");
    let staged_bytes = "fn main() {}\n";
    fs::write(&staged_path, staged_bytes).unwrap();
    git_assert_ok(
        &Command::new("git")
            .args(["-C", wt_str, "add", "source.rs"])
            .output()
            .unwrap(),
    );

    let msg_path = worktree_path.join(".flow-commit-msg");
    fs::write(&msg_path, "Add source.rs (gitignore-respect test).").unwrap();

    // Do NOT write a CI sentinel — CI must actually run so bin/test
    // produces the gitignored artifact.

    let args = Args {
        branch: "restage-gitignore".to_string(),
    };
    let result = run_impl(&args, clone_path).unwrap();
    assert_eq!(result["status"], "ok", "got: {}", result);

    // The staged source file must land at HEAD with the staged bytes.
    let head_source = Command::new("git")
        .args(["-C", wt_str, "show", "HEAD:source.rs"])
        .output()
        .unwrap();
    git_assert_ok(&head_source);
    let committed_bytes = String::from_utf8_lossy(&head_source.stdout).to_string();
    assert_eq!(
        committed_bytes, staged_bytes,
        "committed bytes must match staged bytes",
    );

    // HEAD's tree must NOT contain the gitignored artifact.
    let tree_listing = Command::new("git")
        .args(["-C", wt_str, "ls-tree", "-r", "--name-only", "HEAD"])
        .output()
        .unwrap();
    git_assert_ok(&tree_listing);
    let names = String::from_utf8_lossy(&tree_listing.stdout).to_string();
    assert!(
        !names.lines().any(|l| l == "coverage.profraw"),
        "HEAD must not include gitignored CI artifact; got: {}",
        names,
    );
}
