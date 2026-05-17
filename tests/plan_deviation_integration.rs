//! Subprocess integration tests for the plan-deviation gate
//! inside `finalize_commit::run_impl`.
//!
//! Each test builds a fully-populated fixture repo (git repo
//! with a bare remote, bin/* stubs, a feature branch, state
//! file, plan file, staged changes, optional log file) and
//! spawns `flow-rs finalize-commit` against it to verify one
//! of the five branches the deviation gate adds to
//! `finalize_commit::run_impl`:
//!
//! - A: no plan file → proceed
//! - B: diff touches no plan-named tests → proceed
//! - C: diff matches plan → proceed
//! - D: diff diverges, log acknowledges → proceed
//! - E: diff diverges, no log → block with
//!   `step = "plan_deviation"` and a structured stderr message.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;

const BRANCH: &str = "devtest-branch";

/// Plan body that names `fn test_foo` with fixture value
/// `"expected"`. Drives the drift detection for branches C/D/E.
const DRIFTING_PLAN: &str = concat!(
    "## Tasks\n\n",
    "Task 1 — test foo.\n\n",
    "```rust\n",
    "fn test_foo() {\n",
    "    let key = \"expected\";\n",
    "}\n",
    "```\n",
);

/// Staged test file whose body contains `"actual"` — drifts
/// from the plan's `"expected"` value.
const DRIFTING_RS: &str = concat!(
    "#[test]\n",
    "fn test_foo() {\n",
    "    let key = \"actual\";\n",
    "    let _ = key;\n",
    "}\n",
);

/// Staged test file whose body contains `"expected"` — matches
/// the plan's fixture value, so the gate sees no drift.
const MATCHING_RS: &str = concat!(
    "#[test]\n",
    "fn test_foo() {\n",
    "    let key = \"expected\";\n",
    "    let _ = key;\n",
    "}\n",
);

/// Staged test file that adds an unrelated test the plan does
/// not name. The gate sees `test_foo` absent from the diff and
/// skips.
const UNRELATED_RS: &str = concat!(
    "#[test]\n",
    "fn test_unrelated() {\n",
    "    let key = \"anything\";\n",
    "    let _ = key;\n",
    "}\n",
);

/// State file pointing at the plan file on disk.
const STATE_WITH_PLAN: &str = r#"{
    "branch": "devtest-branch",
    "current_phase": "flow-code",
    "files": {"plan": ".flow-states/devtest-branch/plan.md"}
}"#;

/// State file with no `files.plan` key — branch A.
const STATE_NO_PLAN: &str = r#"{
    "branch": "devtest-branch",
    "current_phase": "flow-code"
}"#;

fn run_git(repo: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("spawn git");
    if !out.status.success() {
        panic!(
            "git {:?} failed in {}:\nstdout: {}\nstderr: {}",
            args,
            repo.display(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// Build a git fixture under `parent` with:
/// - a bare remote (from `common::create_git_repo_with_remote`)
/// - executable `bin/{format,lint,build,test}` stubs that all
///   `exit 0` so CI passes, committed on `main` so they appear
///   in every branch forked from main
/// - a feature branch `BRANCH` forked from main and pushed to
///   origin
/// - a linked worktree at `<repo>/.worktrees/<BRANCH>/` checking
///   out `BRANCH`
///
/// Returns `(repo, worktree)` — both canonicalized. `repo` is
/// the project_root finalize-commit sees, and `worktree` is the
/// path commit_cwd resolves to inside `run_impl` for `BRANCH`.
fn make_repo_fixture(parent: &Path) -> (PathBuf, PathBuf) {
    let repo = common::create_git_repo_with_remote(parent);
    let repo = repo.canonicalize().expect("canonicalize repo");

    let bin_dir = repo.join("bin");
    fs::create_dir_all(&bin_dir).expect("create bin dir");
    for tool in ["format", "lint", "build", "test"] {
        let path = bin_dir.join(tool);
        fs::write(&path, "#!/bin/sh\nexit 0\n").expect("write stub");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod stub");
        }
    }
    run_git(&repo, &["add", "bin"]);
    run_git(&repo, &["commit", "-m", "add bin stubs"]);
    run_git(&repo, &["push", "origin", "main"]);

    run_git(&repo, &["branch", BRANCH]);
    run_git(&repo, &["push", "-u", "origin", BRANCH]);

    let worktree = repo.join(".worktrees").join(BRANCH);
    run_git(
        &repo,
        &[
            "worktree",
            "add",
            worktree.to_str().expect("worktree path utf8"),
            BRANCH,
        ],
    );

    for (key, val) in [
        ("user.email", "test@test.com"),
        ("user.name", "Test"),
        ("pull.rebase", "false"),
        ("commit.gpgsign", "false"),
    ] {
        run_git(&worktree, &["config", key, val]);
    }

    (repo, worktree)
}

/// Seed `.flow-states/` files (at project_root, not the worktree)
/// and stage the test file change inside the worktree.
///
/// `state` is the full JSON content of the state file.
/// `plan` is the plan Markdown to write (skipped when `None`).
/// `test_rs` is the content of `tests/foo.rs` staged via
/// `git add` inside `worktree`. `log` is the log content to write
/// (skipped when `None`). Writes a `.flow-commit-msg` file inside
/// the worktree for `finalize-commit`'s `git commit -F` step.
fn seed_flow_state(
    repo: &Path,
    worktree: &Path,
    state: &str,
    plan: Option<&str>,
    test_rs: &str,
    log: Option<&str>,
) {
    let branch_dir = repo.join(".flow-states").join(BRANCH);
    fs::create_dir_all(&branch_dir).expect("create branch dir");
    fs::write(branch_dir.join("state.json"), state).expect("write state");
    if let Some(plan) = plan {
        fs::write(branch_dir.join("plan.md"), plan).expect("write plan");
    }
    if let Some(log) = log {
        fs::write(branch_dir.join("log"), log).expect("write log");
    }

    let test_dir = worktree.join("tests");
    fs::create_dir_all(&test_dir).expect("create tests dir");
    fs::write(test_dir.join("foo.rs"), test_rs).expect("write test file");
    run_git(worktree, &["add", "tests/foo.rs"]);

    fs::write(worktree.join(".flow-commit-msg"), "test commit\n").expect("write commit msg");
}

/// Spawn `flow-rs finalize-commit <msg-file> <branch>` against
/// the prepared fixture. `current_dir(repo)` so `project_root()`
/// (via `git worktree list --porcelain`) reports the main clone
/// as the integration root. The `<msg-file>` arg is an absolute
/// path under the worktree because `finalize-commit` reads it via
/// `git -C <worktree> commit -F <abs-path>`.
fn run_finalize_commit(repo: &Path, worktree: &Path) -> (i32, String, String) {
    let msg_file = worktree.join(".flow-commit-msg");
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "finalize-commit",
            msg_file.to_str().expect("msg file utf8"),
            BRANCH,
        ])
        .current_dir(repo)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("spawn flow-rs");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

/// Return the last JSON object line in `stdout`. `finalize-commit`
/// prints several log lines before the final JSON result; this
/// helper isolates the final result line.
fn last_json_line(stdout: &str) -> serde_json::Value {
    let last = stdout
        .lines()
        .rfind(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON line in stdout; stdout={}", stdout));
    serde_json::from_str(last)
        .unwrap_or_else(|e| panic!("failed to parse JSON line '{}': {}", last, e))
}

#[test]
fn finalize_commit_no_plan_file_proceeds_to_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let parent = dir.path().canonicalize().expect("canonicalize parent");
    let (repo, worktree) = make_repo_fixture(&parent);
    seed_flow_state(&repo, &worktree, STATE_NO_PLAN, None, UNRELATED_RS, None);

    let (code, stdout, stderr) = run_finalize_commit(&repo, &worktree);
    let json = last_json_line(&stdout);
    assert_eq!(
        json["status"], "ok",
        "branch A (no plan file) must proceed; stdout={}\nstderr={}",
        stdout, stderr
    );
    assert_eq!(code, 0);
}

#[test]
fn finalize_commit_diff_touches_no_plan_named_tests_proceeds() {
    let dir = tempfile::tempdir().expect("tempdir");
    let parent = dir.path().canonicalize().expect("canonicalize parent");
    let (repo, worktree) = make_repo_fixture(&parent);
    seed_flow_state(
        &repo,
        &worktree,
        STATE_WITH_PLAN,
        Some(DRIFTING_PLAN),
        UNRELATED_RS,
        None,
    );

    let (code, stdout, stderr) = run_finalize_commit(&repo, &worktree);
    let json = last_json_line(&stdout);
    assert_eq!(
        json["status"], "ok",
        "branch B (unrelated diff) must proceed; stdout={}\nstderr={}",
        stdout, stderr
    );
    assert_eq!(code, 0);
}

#[test]
fn finalize_commit_diff_matches_plan_proceeds() {
    let dir = tempfile::tempdir().expect("tempdir");
    let parent = dir.path().canonicalize().expect("canonicalize parent");
    let (repo, worktree) = make_repo_fixture(&parent);
    seed_flow_state(
        &repo,
        &worktree,
        STATE_WITH_PLAN,
        Some(DRIFTING_PLAN),
        MATCHING_RS,
        None,
    );

    let (code, stdout, stderr) = run_finalize_commit(&repo, &worktree);
    let json = last_json_line(&stdout);
    assert_eq!(
        json["status"], "ok",
        "branch C (matching diff) must proceed; stdout={}\nstderr={}",
        stdout, stderr
    );
    assert_eq!(code, 0);
}

#[test]
fn finalize_commit_diff_diverges_but_log_acknowledges_proceeds() {
    let dir = tempfile::tempdir().expect("tempdir");
    let parent = dir.path().canonicalize().expect("canonicalize parent");
    let (repo, worktree) = make_repo_fixture(&parent);
    let log =
        "2026-04-15T10:00:00-08:00 [Phase 3] Plan signature deviation: test_foo drifted from expected to actual.\n";
    seed_flow_state(
        &repo,
        &worktree,
        STATE_WITH_PLAN,
        Some(DRIFTING_PLAN),
        DRIFTING_RS,
        Some(log),
    );

    let (code, stdout, stderr) = run_finalize_commit(&repo, &worktree);
    let json = last_json_line(&stdout);
    assert_eq!(
        json["status"], "ok",
        "branch D (acknowledged drift) must proceed; stdout={}\nstderr={}",
        stdout, stderr
    );
    assert_eq!(code, 0);
}

#[test]
fn finalize_commit_diff_diverges_without_log_blocks() {
    let dir = tempfile::tempdir().expect("tempdir");
    let parent = dir.path().canonicalize().expect("canonicalize parent");
    let (repo, worktree) = make_repo_fixture(&parent);
    seed_flow_state(
        &repo,
        &worktree,
        STATE_WITH_PLAN,
        Some(DRIFTING_PLAN),
        DRIFTING_RS,
        None,
    );

    let (code, stdout, stderr) = run_finalize_commit(&repo, &worktree);
    let json = last_json_line(&stdout);
    assert_eq!(
        json["status"], "error",
        "branch E (unacknowledged drift) must block; stdout={}\nstderr={}",
        stdout, stderr
    );
    assert_eq!(
        json["step"], "plan_deviation",
        "error step must be plan_deviation; stdout={}",
        stdout
    );
    assert!(
        stderr.contains("test_foo"),
        "stderr must name the drifting test; stderr={}",
        stderr
    );
    assert!(
        stderr.contains("bin/flow log"),
        "stderr must show the unblock command; stderr={}",
        stderr
    );
    assert_eq!(code, 1);

    // Verify the gate recorded the block in the durable log.
    let log_path = repo.join(".flow-states").join(BRANCH).join("log");
    let log_content = fs::read_to_string(&log_path).unwrap_or_default();
    assert!(
        log_content.contains("plan_deviation (blocked"),
        "log file must contain the blocked-entry line; log={}",
        log_content
    );
}
