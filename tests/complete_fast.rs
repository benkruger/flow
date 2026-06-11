//! Subprocess integration tests for `bin/flow complete-fast`.
//!
//! Inline `#[cfg(test)] mod tests` and the pub `fast_inner` /
//! `run_impl_inner` / `production_ci_decider` seams were removed.
//! Every path is exercised via the compiled `flow-rs complete-fast`
//! binary with configurable stubs for `bin/flow check-freshness`,
//! `gh pr view` / `gh pr checks` / `gh pr merge`, and `git fetch` /
//! `merge-base` / `merge` / `push` / `status`.
//!
//! The stubs are env-var-controlled shell scripts (FAKE_PR_STATE,
//! FAKE_MERGE_EXIT, FAKE_FRESHNESS_OUT, etc.) so every branch is
//! reachable without spawning real `gh`/`git` against GitHub or the
//! host repo.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

mod common;

const BRANCH: &str = "test-feature";

fn make_repo_fixture(parent: &Path) -> PathBuf {
    let repo = common::create_git_repo_with_remote(parent);
    let repo = repo.canonicalize().expect("canonicalize repo");
    // Gitignore .flow-states/ so state-file writes don't perturb the
    // tree_snapshot used by the CI sentinel check.
    fs::write(repo.join(".gitignore"), ".flow-states/\n").unwrap();
    Command::new("git")
        .args(["add", ".gitignore"])
        .current_dir(&repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "gitignore flow-states"])
        .current_dir(&repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["checkout", "-b", BRANCH])
        .current_dir(&repo)
        .output()
        .unwrap();
    repo
}

/// Fixture WITHOUT `refs/remotes/origin/HEAD` so `default_branch_in`
/// returns Err and complete_fast surfaces the resolve_base_branch
/// error envelope. Uses raw git init (not the bare-repo helper) so
/// no remote/HEAD is configured.
fn make_repo_fixture_no_origin_head(parent: &Path) -> PathBuf {
    let repo = parent.join("repo-no-head");
    fs::create_dir_all(&repo).unwrap();
    let run = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
    };
    run(&["init", "-b", "main"]);
    run(&["config", "user.email", "t@t.com"]);
    run(&["config", "user.name", "T"]);
    run(&["config", "commit.gpgsign", "false"]);
    run(&["commit", "--allow-empty", "-m", "init"]);
    run(&["checkout", "-b", BRANCH]);
    repo.canonicalize().expect("canonicalize repo")
}

#[test]
fn complete_fast_errors_when_default_branch_resolve_fails() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture_no_origin_head(&parent);
    write_state_file(&repo, BRANCH, "complete", "auto");
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);
    let env = [("FAKE_PR_STATE", "OPEN")];
    let output = run_complete_fast(&repo, Some(BRANCH), &flow_bin, &stubs, &env);
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Find the JSON line — last line that starts with `{`.
    let last_json_line = stdout
        .lines()
        .rfind(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON line in stdout; stdout={}", stdout));
    let value: Value = serde_json::from_str(last_json_line)
        .unwrap_or_else(|e| panic!("JSON parse failed: {} (line: {:?})", e, last_json_line));
    assert_eq!(value["status"], "error");
    assert_eq!(value["step"], "resolve_base_branch");
    assert!(
        value["message"]
            .as_str()
            .unwrap_or("")
            .contains("symbolic-ref"),
        "expected resolve failure message naming git symbolic-ref, got: {}",
        value
    );
}

fn write_state_file(
    repo: &Path,
    branch: &str,
    learn_status: &str,
    skills_continue: &str,
) -> PathBuf {
    let branch_dir = repo.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    let state = json!({
        "schema_version": 1,
        "branch": branch,
        "base_branch": "main",
        "repo": "test/test",
        "pr_number": 42,
        "pr_url": "https://github.com/test/test/pull/42",
        "prompt": "test feature",
        "phases": {
            "flow-start": {"status": "complete"},
            "flow-code": {"status": "complete"},
            "flow-review": {"status": "complete"},
            "flow-learn": {"status": learn_status},
            "flow-complete": {"status": "pending"}
        },
        "skills": {
            "flow-complete": {"continue": skills_continue}
        }
    });
    fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    state_path
}

/// Configurable bin/flow stub for check-freshness.
///   FAKE_FRESHNESS_OUT   → stdout (default up_to_date)
///   FAKE_FRESHNESS_EXIT  → exit (default 0)
fn write_flow_stub(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let script = r#"#!/bin/sh
case "$1" in
    check-freshness)
        if [ -n "$FAKE_FRESHNESS_OUT" ]; then
            printf '%s' "$FAKE_FRESHNESS_OUT"
        else
            printf '%s' '{"status":"up_to_date"}'
        fi
        exit ${FAKE_FRESHNESS_EXIT:-0}
        ;;
    *)
        exit 0
        ;;
esac
"#;
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Build PATH stub dir with gh + git stubs.
///   FAKE_PR_STATE        → gh pr view state (default OPEN)
///   FAKE_PR_VIEW_EXIT    → gh pr view exit code (default 0)
///   FAKE_MERGE_EXIT      → gh pr merge exit code (default 0)
///   FAKE_MERGE_STDERR    → gh pr merge stderr (default empty)
///   FAKE_FETCH_EXIT      → git fetch exit code (default 0)
///   FAKE_MERGE_BASE_EXIT → git merge-base exit code (default 0 = already up to date)
///   FAKE_GIT_MERGE_EXIT  → git merge exit code (default 0)
///   FAKE_GIT_MERGE_STDERR → git merge stderr
///   FAKE_GIT_STATUS_OUT  → git status --porcelain stdout (for conflict detection)
///   FAKE_PUSH_EXIT       → git push exit code (default 0)
///   FAKE_PUSH_STDERR     → git push stderr
fn build_path_stubs(parent: &Path) -> PathBuf {
    let stubs = parent.join("stubs");
    fs::create_dir_all(&stubs).unwrap();

    let gh_script = r#"#!/bin/sh
case "$1 $2" in
    "pr view")
        if [ -n "$FAKE_PR_STATE" ]; then
            printf '%s' "$FAKE_PR_STATE"
        else
            printf '%s' 'OPEN'
        fi
        exit ${FAKE_PR_VIEW_EXIT:-0}
        ;;
    "pr merge")
        if [ -n "$FAKE_MERGE_STDERR" ]; then printf '%s' "$FAKE_MERGE_STDERR" >&2; fi
        exit ${FAKE_MERGE_EXIT:-0}
        ;;
    *)
        exit 0
        ;;
esac
"#;
    let gh_path = stubs.join("gh");
    fs::write(&gh_path, gh_script).unwrap();
    fs::set_permissions(&gh_path, fs::Permissions::from_mode(0o755)).unwrap();

    let git_script = r#"#!/bin/sh
case "$1" in
    fetch)
        exit ${FAKE_FETCH_EXIT:-0}
        ;;
    merge-base)
        exit ${FAKE_MERGE_BASE_EXIT:-0}
        ;;
    merge)
        if [ -n "$FAKE_GIT_MERGE_STDERR" ]; then printf '%s' "$FAKE_GIT_MERGE_STDERR" >&2; fi
        exit ${FAKE_GIT_MERGE_EXIT:-0}
        ;;
    push)
        if [ -n "$FAKE_PUSH_STDERR" ]; then printf '%s' "$FAKE_PUSH_STDERR" >&2; fi
        exit ${FAKE_PUSH_EXIT:-0}
        ;;
    status)
        if [ -n "$FAKE_GIT_STATUS_OUT" ]; then
            printf '%s' "$FAKE_GIT_STATUS_OUT"
        fi
        exit 0
        ;;
    *)
        exec /usr/bin/git "$@"
        ;;
esac
"#;
    let git_path = stubs.join("git");
    fs::write(&git_path, git_script).unwrap();
    fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();

    stubs
}

fn run_complete_fast(
    cwd: &Path,
    branch_arg: Option<&str>,
    flow_bin_path: &Path,
    stubs: &Path,
    env: &[(&str, &str)],
) -> Output {
    let current_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", stubs.display(), current_path);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.arg("complete-fast");
    if let Some(b) = branch_arg {
        cmd.arg("--branch").arg(b);
    }
    cmd.current_dir(cwd)
        .env("PATH", new_path)
        .env("FLOW_BIN_PATH", flow_bin_path)
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

/// Pre-populate the CI sentinel with the current tree snapshot so
/// ci_decider returns ci_skipped=true.
fn seed_ci_sentinel(repo: &Path, branch: &str) {
    let snapshot = flow_rs::ci::tree_snapshot(repo, None);
    let sentinel_path = flow_rs::ci::sentinel_path(repo, branch);
    if let Some(parent) = sentinel_path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&sentinel_path, snapshot).unwrap();
}

struct Fixture {
    _dir: tempfile::TempDir,
    repo: PathBuf,
    flow_bin: PathBuf,
    stubs: PathBuf,
}

fn setup(learn_status: &str, skills_continue: &str) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    write_state_file(&repo, BRANCH, learn_status, skills_continue);
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);
    Fixture {
        _dir: dir,
        repo,
        flow_bin,
        stubs,
    }
}

// --- Error paths ---

#[test]
fn no_state_file_exits_1_with_error() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);

    let output = run_complete_fast(&repo, Some(BRANCH), &flow_bin, &stubs, &[]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap_or("")
        .contains("No state file"));
}

#[test]
fn slash_branch_exits_1_structured_error() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);

    let output = run_complete_fast(&repo, Some("feature/foo"), &flow_bin, &stubs, &[]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap_or("")
        .contains("not a valid FLOW branch"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked at"));
}

#[test]
fn no_branch_argument_no_git_repo_errors() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);

    let output = run_complete_fast(&parent, None, &flow_bin, &stubs, &[]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
}

#[test]
fn corrupt_state_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let branch_dir = repo.join(".flow-states").join(BRANCH);
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), "{corrupt").unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);

    let output = run_complete_fast(&repo, Some(BRANCH), &flow_bin, &stubs, &[]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap_or("")
        .contains("Could not parse state file"));
}

#[test]
fn non_object_state_returns_corrupt_error() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let branch_dir = repo.join(".flow-states").join(BRANCH);
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), "[1,2,3]").unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);

    let output = run_complete_fast(&repo, Some(BRANCH), &flow_bin, &stubs, &[]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap_or("")
        .contains("Corrupt state file"));
}

#[test]
fn learn_gate_pending_returns_error() {
    let fx = setup("pending", "auto");
    let output = run_complete_fast(&fx.repo, Some(BRANCH), &fx.flow_bin, &fx.stubs, &[]);
    // status="error" in JSON but exit code is 1 because run_impl returns Ok(error_value).
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"].as_str().unwrap().contains("Phase 5: Learn"));
}

#[test]
fn pr_status_runner_err_returns_error() {
    let fx = setup("complete", "auto");
    // Use nonexistent gh binary by clearing PATH stubs entirely.
    let output = run_complete_fast(
        &fx.repo,
        Some(BRANCH),
        &fx.flow_bin,
        &fx.stubs,
        &[("FAKE_PR_VIEW_EXIT", "1")],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
}

#[test]
fn pr_state_merged_returns_already_merged_path() {
    let fx = setup("complete", "auto");
    let output = run_complete_fast(
        &fx.repo,
        Some(BRANCH),
        &fx.flow_bin,
        &fx.stubs,
        &[("FAKE_PR_STATE", "MERGED")],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["path"], "already_merged");
}

/// `complete_fast` writes the account-window snapshot to the
/// phase-scoped `phases.flow-complete.window_at_enter` field when
/// entering the Complete phase. Without this write,
/// `format_complete_summary`'s `phase_delta` short-circuits to
/// `None` for flow-complete because it reads
/// `phase.window_at_enter`, leaving the Complete row in the Token
/// Cost section with placeholder data.
///
/// Uses `FAKE_PR_STATE=MERGED` so complete_fast hits the
/// `already_merged` early return immediately after the mutate_state
/// block runs — state.json persists with the snapshot for the
/// assertion to read.
#[test]
fn complete_fast_writes_phase_scoped_window_at_enter_for_flow_complete() {
    let fx = setup("complete", "auto");
    let output = run_complete_fast(
        &fx.repo,
        Some(BRANCH),
        &fx.flow_bin,
        &fx.stubs,
        &[("FAKE_PR_STATE", "MERGED")],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok", "stdout: {}", stdout);
    assert_eq!(json["path"], "already_merged");

    let state_path = fx.repo.join(".flow-states").join(BRANCH).join("state.json");
    let content = fs::read_to_string(&state_path).expect("state file must exist");
    let state: Value = serde_json::from_str(&content).expect("state must parse");

    let phase_scoped = &state["phases"]["flow-complete"]["window_at_enter"];
    assert!(
        phase_scoped.is_object(),
        "phases.flow-complete.window_at_enter must be populated; got: {}",
        phase_scoped
    );

    // The snapshot carries `captured_at` from `now()` and (when a
    // valid session_id was available) a session_id field. The
    // session_id may be null on this fixture because state.json has
    // no session_id, but captured_at must always be a non-empty
    // string per `WindowSnapshot`'s schema.
    let captured_at = phase_scoped["captured_at"].as_str().unwrap_or("");
    assert!(
        !captured_at.is_empty(),
        "captured_at must be populated; got: {}",
        phase_scoped
    );
}

#[test]
fn pr_state_closed_returns_error() {
    let fx = setup("complete", "auto");
    let output = run_complete_fast(
        &fx.repo,
        Some(BRANCH),
        &fx.flow_bin,
        &fx.stubs,
        &[("FAKE_PR_STATE", "CLOSED")],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap_or("")
        .contains("PR is closed"));
}

#[test]
fn merge_main_conflict_returns_conflict_path() {
    let fx = setup("complete", "auto");
    // merge-base exits 1 (not ancestor), merge exits 1, status has conflict marker.
    let output = run_complete_fast(
        &fx.repo,
        Some(BRANCH),
        &fx.flow_bin,
        &fx.stubs,
        &[
            ("FAKE_MERGE_BASE_EXIT", "1"),
            ("FAKE_GIT_MERGE_EXIT", "1"),
            ("FAKE_GIT_STATUS_OUT", "UU conflict.txt\n"),
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["path"], "conflict");
    let files: Vec<&str> = json["conflict_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(files, vec!["conflict.txt"]);
}

#[test]
fn merge_main_error_returns_error() {
    let fx = setup("complete", "auto");
    // fetch fails → merge_main returns error.
    let output = run_complete_fast(
        &fx.repo,
        Some(BRANCH),
        &fx.flow_bin,
        &fx.stubs,
        &[("FAKE_FETCH_EXIT", "1")],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
}

#[test]
fn tree_changed_ci_fails_returns_ci_failed() {
    // merge-base exits 1, merge exits 0 → base branch merged in (tree
    // changed). The sentinel is unseeded, so the changed tree misses
    // it and CI runs inline. With no `bin/*` tool scripts present,
    // `ci::run_impl` fails → path: "ci_failed". The pre-inline-CI
    // behavior deferred to "ci_stale" without ever running CI; the
    // inline CI gate now catches the dirty tree before merge.
    let fx = setup("complete", "auto");
    let output = run_complete_fast(
        &fx.repo,
        Some(BRANCH),
        &fx.flow_bin,
        &fx.stubs,
        &[("FAKE_MERGE_BASE_EXIT", "1"), ("FAKE_GIT_MERGE_EXIT", "0")],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["path"], "ci_failed");
}

#[test]
fn tree_changed_ci_passes_proceeds_to_merge() {
    // merge-base exits 1, merge exits 0 → base branch merged in (tree
    // changed). The sentinel is unseeded, so CI runs inline. With
    // passing `bin/*` tool scripts, CI passes and the flow proceeds to
    // the squash merge: path "merged", ci_skipped false. The
    // pre-inline-CI behavior deferred to "ci_stale" and never ran CI
    // on this path.
    let fx = setup("complete", "auto");
    let bin_dir = fx.repo.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    for tool in &["format", "lint", "build", "test"] {
        let p = bin_dir.join(tool);
        fs::write(&p, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let output = run_complete_fast(
        &fx.repo,
        Some(BRANCH),
        &fx.flow_bin,
        &fx.stubs,
        &[("FAKE_MERGE_BASE_EXIT", "1"), ("FAKE_GIT_MERGE_EXIT", "0")],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok", "stdout: {}", stdout);
    assert_eq!(json["path"], "merged");
    assert_eq!(json["ci_skipped"], false);
}

#[test]
fn ci_sentinel_hit_proceeds_with_ci_skipped() {
    let fx = setup("complete", "auto");
    seed_ci_sentinel(&fx.repo, BRANCH);
    // sentinel matches, merge_main="clean" (merge-base exit 0), up_to_date → merged success.
    let output = run_complete_fast(&fx.repo, Some(BRANCH), &fx.flow_bin, &fx.stubs, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["path"], "merged");
    assert_eq!(json["ci_skipped"], true);
}

#[test]
fn ci_sentinel_miss_runs_ci_and_fails() {
    // No sentinel → CI runs via ci::run_impl → bin/test (or bin/format etc)
    // are missing → ci::run_impl returns error → ci_failed path.
    let fx = setup("complete", "auto");
    let output = run_complete_fast(&fx.repo, Some(BRANCH), &fx.flow_bin, &fx.stubs, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["path"], "ci_failed");
}

#[test]
fn mode_manual_returns_confirm() {
    let fx = setup("complete", "manual");
    seed_ci_sentinel(&fx.repo, BRANCH);
    let output = run_complete_fast(&fx.repo, Some(BRANCH), &fx.flow_bin, &fx.stubs, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["path"], "confirm");
    assert_eq!(json["mode"], "manual");
}

#[test]
fn freshness_status_max_retries_returns_max_retries_path() {
    let fx = setup("complete", "auto");
    seed_ci_sentinel(&fx.repo, BRANCH);
    let output = run_complete_fast(
        &fx.repo,
        Some(BRANCH),
        &fx.flow_bin,
        &fx.stubs,
        &[("FAKE_FRESHNESS_OUT", r#"{"status":"max_retries"}"#)],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["path"], "max_retries");
}

#[test]
fn freshness_status_error_with_message_returns_error() {
    let fx = setup("complete", "auto");
    seed_ci_sentinel(&fx.repo, BRANCH);
    let output = run_complete_fast(
        &fx.repo,
        Some(BRANCH),
        &fx.flow_bin,
        &fx.stubs,
        &[(
            "FAKE_FRESHNESS_OUT",
            r#"{"status":"error","message":"network timeout"}"#,
        )],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("network timeout"));
}

#[test]
fn freshness_status_error_without_message_uses_fallback() {
    let fx = setup("complete", "auto");
    seed_ci_sentinel(&fx.repo, BRANCH);
    let output = run_complete_fast(
        &fx.repo,
        Some(BRANCH),
        &fx.flow_bin,
        &fx.stubs,
        &[("FAKE_FRESHNESS_OUT", r#"{"status":"error"}"#)],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("check-freshness failed"));
}

#[test]
fn freshness_status_conflict_returns_conflict_path() {
    let fx = setup("complete", "auto");
    seed_ci_sentinel(&fx.repo, BRANCH);
    let output = run_complete_fast(
        &fx.repo,
        Some(BRANCH),
        &fx.flow_bin,
        &fx.stubs,
        &[(
            "FAKE_FRESHNESS_OUT",
            r#"{"status":"conflict","files":["a.rs"]}"#,
        )],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["path"], "conflict");
}

#[test]
fn freshness_status_conflict_no_files_defaults_to_empty() {
    let fx = setup("complete", "auto");
    seed_ci_sentinel(&fx.repo, BRANCH);
    let output = run_complete_fast(
        &fx.repo,
        Some(BRANCH),
        &fx.flow_bin,
        &fx.stubs,
        &[("FAKE_FRESHNESS_OUT", r#"{"status":"conflict"}"#)],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["path"], "conflict");
    assert_eq!(json["conflict_files"], json!([]));
}

#[test]
fn freshness_status_merged_push_ok_returns_ci_stale() {
    let fx = setup("complete", "auto");
    seed_ci_sentinel(&fx.repo, BRANCH);
    let output = run_complete_fast(
        &fx.repo,
        Some(BRANCH),
        &fx.flow_bin,
        &fx.stubs,
        &[
            ("FAKE_FRESHNESS_OUT", r#"{"status":"merged"}"#),
            ("FAKE_PUSH_EXIT", "0"),
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["path"], "ci_stale");
    assert!(json["reason"]
        .as_str()
        .unwrap()
        .contains("main moved during freshness"));
}

#[test]
fn freshness_status_merged_push_nonzero_returns_error() {
    let fx = setup("complete", "auto");
    seed_ci_sentinel(&fx.repo, BRANCH);
    let output = run_complete_fast(
        &fx.repo,
        Some(BRANCH),
        &fx.flow_bin,
        &fx.stubs,
        &[
            ("FAKE_FRESHNESS_OUT", r#"{"status":"merged"}"#),
            ("FAKE_PUSH_EXIT", "1"),
            ("FAKE_PUSH_STDERR", "remote rejected"),
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("remote rejected"));
}

#[test]
fn freshness_status_up_to_date_merge_ok_returns_merged() {
    let fx = setup("complete", "auto");
    seed_ci_sentinel(&fx.repo, BRANCH);
    let output = run_complete_fast(&fx.repo, Some(BRANCH), &fx.flow_bin, &fx.stubs, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["path"], "merged");
}

#[test]
fn auto_path_base_branch_policy_returns_not_mergeable() {
    // `gh pr merge --squash` refuses with a base-branch-policy stderr
    // (a required GitHub check is failing or pending). complete-fast
    // surfaces the verbatim gh stderr as path: "not_mergeable" with a
    // `reason` field rather than guessing pending vs. failed.
    let fx = setup("complete", "auto");
    seed_ci_sentinel(&fx.repo, BRANCH);
    let output = run_complete_fast(
        &fx.repo,
        Some(BRANCH),
        &fx.flow_bin,
        &fx.stubs,
        &[
            ("FAKE_MERGE_EXIT", "1"),
            (
                "FAKE_MERGE_STDERR",
                "base branch policy prohibits the merge",
            ),
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["path"], "not_mergeable");
    assert!(json["reason"]
        .as_str()
        .unwrap()
        .contains("base branch policy"));
}

#[test]
fn auto_path_base_branch_policy_case_insensitive_returns_not_mergeable() {
    // The not_mergeable classification case-folds the gh stderr per
    // `.claude/rules/security-gates.md` "Normalize Before Comparing".
    // A capitalized "Base branch policy" refusal must still classify
    // as not_mergeable, not fall through to status:error.
    let fx = setup("complete", "auto");
    seed_ci_sentinel(&fx.repo, BRANCH);
    let output = run_complete_fast(
        &fx.repo,
        Some(BRANCH),
        &fx.flow_bin,
        &fx.stubs,
        &[
            ("FAKE_MERGE_EXIT", "1"),
            (
                "FAKE_MERGE_STDERR",
                "Base branch policy prohibits the merge",
            ),
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["path"], "not_mergeable");
}

#[test]
fn freshness_status_up_to_date_merge_generic_failure_returns_error() {
    let fx = setup("complete", "auto");
    seed_ci_sentinel(&fx.repo, BRANCH);
    let output = run_complete_fast(
        &fx.repo,
        Some(BRANCH),
        &fx.flow_bin,
        &fx.stubs,
        &[
            ("FAKE_MERGE_EXIT", "1"),
            ("FAKE_MERGE_STDERR", "merge conflict in base"),
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("merge conflict in base"));
}

#[test]
fn freshness_invalid_json_returns_error() {
    let fx = setup("complete", "auto");
    seed_ci_sentinel(&fx.repo, BRANCH);
    let output = run_complete_fast(
        &fx.repo,
        Some(BRANCH),
        &fx.flow_bin,
        &fx.stubs,
        &[("FAKE_FRESHNESS_OUT", "not-json")],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"].as_str().unwrap().contains("Invalid JSON"));
}

#[test]
fn freshness_unexpected_status_returns_error() {
    let fx = setup("complete", "auto");
    seed_ci_sentinel(&fx.repo, BRANCH);
    let output = run_complete_fast(
        &fx.repo,
        Some(BRANCH),
        &fx.flow_bin,
        &fx.stubs,
        &[("FAKE_FRESHNESS_OUT", r#"{"status":"frobnicate"}"#)],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"].as_str().unwrap().contains("Unexpected"));
}

#[test]
fn no_pr_number_skips_gh_check_and_proceeds() {
    // State with no pr_number → gh_ci_status defaults to "none" → proceeds.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let branch_dir = repo.join(".flow-states").join(BRANCH);
    fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    let state = json!({
        "schema_version": 1,
        "branch": BRANCH,
        "repo": "test/test",
        "pr_url": "https://github.com/test/test/pull/42",
        "phases": {
            "flow-start": {"status": "complete"},
            "flow-code": {"status": "complete"},
            "flow-review": {"status": "complete"},
            "flow-learn": {"status": "complete"},
            "flow-complete": {"status": "pending"}
        },
        "skills": {"flow-complete": {"continue": "auto"}}
    });
    fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);
    seed_ci_sentinel(&repo, BRANCH);

    let output = run_complete_fast(&repo, Some(BRANCH), &flow_bin, &stubs, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    // No pr_number → gh pr merge gets pr "0" which will fail with the stub exit=0 → merged
    // (the FAKE_MERGE_EXIT=0 makes gh pr merge 0 succeed).
    assert_eq!(json["status"], "ok");
}

#[test]
fn state_path_is_directory_returns_read_error() {
    // read_state read_to_string Err arm.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let branch_dir = repo.join(".flow-states").join(BRANCH);
    fs::create_dir_all(&branch_dir).unwrap();
    // Make the state file path a directory → read_to_string returns EISDIR.
    fs::create_dir(branch_dir.join("state.json")).unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);

    let output = run_complete_fast(&repo, Some(BRANCH), &flow_bin, &stubs, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("Could not read state file"));
}

#[test]
fn ci_decider_sentinel_miss_ci_passes_proceeds() {
    // No sentinel + bin/* stubs return 0 → ci::run_impl exits 0 →
    // ci_decider returns (false, None) → proceed to gh pr checks →
    // merged path.
    let fx = setup("complete", "auto");
    // Write bin/* stubs that succeed.
    let bin_dir = fx.repo.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    for tool in &["format", "lint", "build", "test"] {
        let p = bin_dir.join(tool);
        fs::write(&p, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let output = run_complete_fast(&fx.repo, Some(BRANCH), &fx.flow_bin, &fx.stubs, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["path"], "merged");
    assert_eq!(json["ci_skipped"], false);
}

#[test]
fn freshness_spawn_err_returns_error() {
    // Point FLOW_BIN_PATH at a nonexistent binary → check-freshness
    // spawn fails → Err arm in freshness_and_merge.
    let fx = setup("complete", "auto");
    seed_ci_sentinel(&fx.repo, BRANCH);
    let nonexistent = fx.repo.join("does-not-exist").join("flow");
    let output = run_complete_fast(&fx.repo, Some(BRANCH), &nonexistent, &fx.stubs, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("check-freshness failed"));
}

// --- CI reason banner ---

#[test]
fn complete_fast_passes_ci_reason() {
    // No sentinel + bin/* stubs return 0 → ci::run_impl runs CI with the
    // explicit reason supplied by complete_fast::ci_decider.
    let fx = setup("complete", "auto");
    let bin_dir = fx.repo.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    for tool in &["format", "lint", "build", "test"] {
        let p = bin_dir.join(tool);
        fs::write(&p, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let output = run_complete_fast(&fx.repo, Some(BRANCH), &fx.flow_bin, &fx.stubs, &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("CI: verifying tree is clean before Complete merge\n"),
        "expected complete_fast's explicit reason banner; stderr=\n{}",
        stderr
    );
}
