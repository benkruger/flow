//! CLI integration tests for `flow-rs check-freshness`.
//!
//! Mirrors the five Python `test_cli_*` integration tests from
//! `tests/test_check_freshness.py`. Each test spins up a real git
//! repository with a bare remote and invokes the compiled flow-rs
//! binary as a subprocess.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

fn flow_rs() -> Command {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
}

/// Run a git command in `cwd` and panic with stderr on failure.
fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|e| panic!("git spawn failed: {}", e));
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Create a git repo at `<tmp>/repo` with main branch, user config, and
/// an initial commit. Returns the repo path.
fn make_repo(tmp: &Path) -> PathBuf {
    let repo = tmp.join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@test.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    git(&repo, &["config", "commit.gpgsign", "false"]);
    fs::write(repo.join("README.md"), "initial\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "init"]);
    repo
}

/// Create a bare remote at `<tmp>/bare.git`, add it to `repo` as origin,
/// and push main. Returns the bare remote path.
fn attach_bare_remote(tmp: &Path, repo: &Path) -> PathBuf {
    let bare = tmp.join("bare.git");
    git(tmp, &["init", "--bare", bare.to_str().unwrap()]);
    git(repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(repo, &["push", "-u", "origin", "main"]);
    bare
}

/// Parse the last non-empty line of stdout as JSON.
fn parse_last_json(stdout: &[u8]) -> Value {
    let text = String::from_utf8_lossy(stdout);
    let line = text
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .unwrap_or_else(|| panic!("no stdout lines: {}", text));
    serde_json::from_str(line.trim())
        .unwrap_or_else(|e| panic!("JSON parse failed: {} (line: {:?})", e, line))
}

// --- CLI integration tests ---

#[test]
fn cli_up_to_date() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = make_repo(tmp.path());
    attach_bare_remote(tmp.path(), &repo);

    let output = flow_rs()
        .arg("check-freshness")
        .current_dir(&repo)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_last_json(&output.stdout);
    assert_eq!(data["status"], "up_to_date");
}

#[test]
fn cli_merged() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = make_repo(tmp.path());
    attach_bare_remote(tmp.path(), &repo);

    // Create a feature branch at the current head
    git(&repo, &["branch", "feature"]);
    git(&repo, &["switch", "feature"]);

    // Advance main with a new commit, then push
    git(&repo, &["switch", "main"]);
    fs::write(repo.join("new_on_main.txt"), "new content\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "new on main"]);
    git(&repo, &["push", "origin", "main"]);

    // Switch back to feature branch — behind main by one commit
    git(&repo, &["switch", "feature"]);

    let output = flow_rs()
        .arg("check-freshness")
        .current_dir(&repo)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_last_json(&output.stdout);
    assert_eq!(data["status"], "merged");
}

#[test]
fn cli_with_state_file() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = make_repo(tmp.path());
    attach_bare_remote(tmp.path(), &repo);

    // Create feature branch, then advance main
    git(&repo, &["branch", "feature"]);
    git(&repo, &["switch", "main"]);
    fs::write(repo.join("main_file.txt"), "content\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "advance main"]);
    git(&repo, &["push", "origin", "main"]);
    git(&repo, &["switch", "feature"]);

    // Create a state file with freshness_retries: 0
    let state_file = tmp.path().join("state.json");
    fs::write(
        &state_file,
        json!({"branch": "feature", "freshness_retries": 0}).to_string(),
    )
    .unwrap();

    let output = flow_rs()
        .arg("check-freshness")
        .arg("--state-file")
        .arg(&state_file)
        .current_dir(&repo)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_last_json(&output.stdout);
    assert_eq!(data["status"], "merged");
    assert_eq!(data["retries"], 1);

    // Verify state file was updated on disk
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_file).unwrap()).unwrap();
    assert_eq!(state["freshness_retries"], 1);
}

#[test]
fn cli_max_retries() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = make_repo(tmp.path());

    let state_file = tmp.path().join("state.json");
    fs::write(
        &state_file,
        json!({"branch": "test", "freshness_retries": 3}).to_string(),
    )
    .unwrap();

    let output = flow_rs()
        .arg("check-freshness")
        .arg("--state-file")
        .arg(&state_file)
        .current_dir(&repo)
        .output()
        .unwrap();

    // Max retries exits with code 1 and prints max_retries status.
    assert_eq!(output.status.code(), Some(1));
    let data = parse_last_json(&output.stdout);
    assert_eq!(data["status"], "max_retries");
    assert_eq!(data["retries"], 3);
}

#[test]
fn cli_unknown_args_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = make_repo(tmp.path());
    attach_bare_remote(tmp.path(), &repo);

    let output = flow_rs()
        .arg("check-freshness")
        .arg("--unknown")
        .arg("value")
        .current_dir(&repo)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_last_json(&output.stdout);
    assert_eq!(data["status"], "up_to_date");
}

/// Regression: check-freshness must inherit CWD from the caller. When
/// invoked from a linked worktree, the main repo's HEAD is still `main`
/// so git commands run there would trivially report `up_to_date`. This
/// test sets up a repo where main is ahead of a feature worktree and
/// runs check-freshness from INSIDE the feature worktree — it must
/// report `merged` (feature brought up to date), not a false
/// `up_to_date` from the main repo's perspective.
#[test]
fn cli_runs_git_in_caller_worktree_not_main_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = make_repo(tmp.path());
    attach_bare_remote(tmp.path(), &repo);

    // Create a linked worktree on a feature branch whose HEAD is the
    // initial commit (so it is behind main once we advance main below).
    let worktree = tmp.path().join("feature-wt");
    git(
        &repo,
        &[
            "worktree",
            "add",
            worktree.to_str().unwrap(),
            "-b",
            "feature",
        ],
    );

    // Advance main in the main repo, then push.
    fs::write(repo.join("advance.txt"), "advance\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "advance main"]);
    git(&repo, &["push", "origin", "main"]);

    // Run check-freshness from the LINKED WORKTREE. A buggy
    // implementation that resolves the CWD via `project_root()` would
    // run git commands in the main repo (where HEAD=main) and return
    // up_to_date trivially. A correct implementation inherits the
    // feature worktree's CWD and merges origin/main into feature.
    let output = flow_rs()
        .arg("check-freshness")
        .current_dir(&worktree)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_last_json(&output.stdout);
    assert_eq!(
        data["status"], "merged",
        "expected merged (feature was behind main), got: {}",
        data
    );
}
