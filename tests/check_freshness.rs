//! Integration tests for `flow-rs check-freshness`. All tests drive
//! through the public entry point `run_impl_main` (or the compiled
//! binary for CLI-dispatch coverage) — no private helpers are
//! imported per `.claude/rules/test-placement.md`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use flow_rs::check_freshness::run_impl_main;
use serde_json::{json, Value};

fn flow_rs() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd
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
/// an initial commit. Returns the canonicalized repo path.
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
    repo.canonicalize().unwrap()
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

// --- CLI integration tests (binary dispatch) ---

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

    git(&repo, &["branch", "feature"]);
    git(&repo, &["switch", "feature"]);

    git(&repo, &["switch", "main"]);
    fs::write(repo.join("new_on_main.txt"), "new content\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "new on main"]);
    git(&repo, &["push", "origin", "main"]);

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
/// so git commands run there would trivially report `up_to_date`.
#[test]
fn cli_runs_git_in_caller_worktree_not_main_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = make_repo(tmp.path());
    attach_bare_remote(tmp.path(), &repo);

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

    fs::write(repo.join("advance.txt"), "advance\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "advance main"]);
    git(&repo, &["push", "origin", "main"]);

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
    assert_eq!(data["status"], "merged");
}

// --- run_impl_main tests (library-level via public entry point) ---

#[test]
fn run_impl_main_max_retries_exits_1() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = make_repo(tmp.path());

    let state_file = tmp.path().join("state.json");
    fs::write(
        &state_file,
        json!({"branch": "test", "freshness_retries": 3}).to_string(),
    )
    .unwrap();

    let raw_args = vec![
        "--state-file".to_string(),
        state_file.to_string_lossy().to_string(),
    ];
    let (value, code) = run_impl_main(&raw_args, &repo);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "max_retries");
    assert_eq!(value["retries"], 3);
}

#[test]
fn run_impl_main_up_to_date_exits_0() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = make_repo(tmp.path());
    attach_bare_remote(tmp.path(), &repo);

    let (value, code) = run_impl_main(&[], &repo);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "up_to_date");
}

/// Drive the `Some(str)` branch of `read_base_branch` through
/// `check_freshness` and prove the state-file value reaches
/// `git fetch origin <base_branch>`. The bare remote has only
/// `main`; the state file declares `base_branch: "staging"`. After
/// the helper plumbing, `check_freshness` issues
/// `git fetch origin staging` against the bare remote, which fails
/// with "couldn't find remote ref staging" — surfaces as
/// `status: "error"` with a `message` carrying "staging".
#[test]
fn check_freshness_uses_base_branch_from_state() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = make_repo(tmp.path());
    attach_bare_remote(tmp.path(), &repo);

    let state_file = tmp.path().join("state.json");
    fs::write(
        &state_file,
        json!({
            "branch": "main",
            "base_branch": "staging",
            "freshness_retries": 0,
        })
        .to_string(),
    )
    .unwrap();

    let raw_args = vec![
        "--state-file".to_string(),
        state_file.to_string_lossy().to_string(),
    ];
    let (value, code) = run_impl_main(&raw_args, &repo);
    assert_eq!(
        code, 1,
        "expected non-zero exit when origin/staging missing, got: {}",
        value
    );
    assert_eq!(
        value["status"], "error",
        "expected error status when origin/staging missing, got: {}",
        value
    );
    let msg = value["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("staging"),
        "fetch error must reference 'staging' to prove base_branch flowed through, got: {}",
        msg
    );
}

#[test]
fn run_impl_main_merged_with_state_file_increments_retries() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = make_repo(tmp.path());
    attach_bare_remote(tmp.path(), &repo);

    git(&repo, &["branch", "feature"]);
    git(&repo, &["switch", "main"]);
    fs::write(repo.join("new.txt"), "x\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "advance"]);
    git(&repo, &["push", "origin", "main"]);
    git(&repo, &["switch", "feature"]);

    let state_file = tmp.path().join("state.json");
    fs::write(
        &state_file,
        json!({"branch": "feature", "freshness_retries": 0}).to_string(),
    )
    .unwrap();

    let raw_args = vec![
        "--state-file".to_string(),
        state_file.to_string_lossy().to_string(),
    ];
    let (value, code) = run_impl_main(&raw_args, &repo);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "merged");
    assert_eq!(value["retries"], 1);

    let state: Value = serde_json::from_str(&fs::read_to_string(&state_file).unwrap()).unwrap();
    assert_eq!(state["freshness_retries"], 1);
}

#[test]
fn run_impl_main_fetch_failure_returns_error() {
    // No remote configured — `git fetch origin main` fails with
    // "'origin' does not appear to be a git repository" or similar.
    let tmp = tempfile::tempdir().unwrap();
    let repo = make_repo(tmp.path());

    let (value, code) = run_impl_main(&[], &repo);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    assert_eq!(value["step"], "fetch");
    assert!(
        !value["message"].as_str().unwrap_or("").is_empty(),
        "expected non-empty fetch error message, got: {}",
        value
    );
}

#[test]
fn run_impl_main_merge_conflict_detected() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = make_repo(tmp.path());
    attach_bare_remote(tmp.path(), &repo);

    // Write content to conflict.txt on main
    fs::write(repo.join("conflict.txt"), "main content\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "add conflict file"]);
    git(&repo, &["push", "origin", "main"]);

    // Branch off at init commit (before conflict.txt was added)
    git(&repo, &["checkout", "-b", "feature", "HEAD~1"]);
    fs::write(repo.join("conflict.txt"), "feature content\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "feature add conflict file"]);

    let state_file = tmp.path().join("state.json");
    fs::write(
        &state_file,
        json!({"branch": "feature", "freshness_retries": 1}).to_string(),
    )
    .unwrap();

    let raw_args = vec![
        "--state-file".to_string(),
        state_file.to_string_lossy().to_string(),
    ];
    let (value, code) = run_impl_main(&raw_args, &repo);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "conflict");
    assert!(value["files"].is_array());
    let files: Vec<String> = value["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(
        files.iter().any(|f| f == "conflict.txt"),
        "expected conflict.txt in files, got: {:?}",
        files
    );
    assert_eq!(value["retries"], 2);
}

#[test]
fn run_impl_main_conflict_without_state_file_returns_no_retries() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = make_repo(tmp.path());
    attach_bare_remote(tmp.path(), &repo);

    fs::write(repo.join("c.txt"), "main\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "main file"]);
    git(&repo, &["push", "origin", "main"]);

    git(&repo, &["checkout", "-b", "feature", "HEAD~1"]);
    fs::write(repo.join("c.txt"), "feature\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "feature file"]);

    let (value, code) = run_impl_main(&[], &repo);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "conflict");
    assert!(value.get("retries").is_none());
}

#[test]
fn run_impl_main_merged_without_state_file_no_retries_key() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = make_repo(tmp.path());
    attach_bare_remote(tmp.path(), &repo);

    git(&repo, &["branch", "feature"]);
    git(&repo, &["switch", "main"]);
    fs::write(repo.join("x.txt"), "y\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "advance"]);
    git(&repo, &["push", "origin", "main"]);
    git(&repo, &["switch", "feature"]);

    let (value, code) = run_impl_main(&[], &repo);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "merged");
    assert!(value.get("retries").is_none());
}

#[test]
fn run_impl_main_state_file_arg_without_value_is_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = make_repo(tmp.path());
    attach_bare_remote(tmp.path(), &repo);

    let raw_args = vec!["--state-file".to_string()];
    let (value, code) = run_impl_main(&raw_args, &repo);
    // No state file → runs normally without retry tracking.
    assert_eq!(code, 0);
    assert_eq!(value["status"], "up_to_date");
}

// --- State-file type tolerance (exercises read_retries / increment_retries) ---

#[test]
fn run_impl_main_state_array_root_skips_retry_tracking() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = make_repo(tmp.path());
    attach_bare_remote(tmp.path(), &repo);

    git(&repo, &["branch", "feature"]);
    git(&repo, &["switch", "main"]);
    fs::write(repo.join("m.txt"), "m\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "advance"]);
    git(&repo, &["push", "origin", "main"]);
    git(&repo, &["switch", "feature"]);

    let state_file = tmp.path().join("state.json");
    fs::write(&state_file, "[1, 2, 3]").unwrap();

    let raw_args = vec![
        "--state-file".to_string(),
        state_file.to_string_lossy().to_string(),
    ];
    let (value, code) = run_impl_main(&raw_args, &repo);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "merged");
    // array-root state skips the object-guarded mutation → retries = 0
    assert_eq!(value["retries"], 0);
}

#[test]
fn run_impl_main_state_missing_key_treats_as_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = make_repo(tmp.path());
    attach_bare_remote(tmp.path(), &repo);

    git(&repo, &["branch", "feature"]);
    git(&repo, &["switch", "main"]);
    fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "advance"]);
    git(&repo, &["push", "origin", "main"]);
    git(&repo, &["switch", "feature"]);

    let state_file = tmp.path().join("state.json");
    fs::write(&state_file, json!({"branch": "feature"}).to_string()).unwrap();

    let raw_args = vec![
        "--state-file".to_string(),
        state_file.to_string_lossy().to_string(),
    ];
    let (value, code) = run_impl_main(&raw_args, &repo);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "merged");
    assert_eq!(value["retries"], 1);
}

#[test]
fn run_impl_main_state_float_retries_tolerated() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = make_repo(tmp.path());
    attach_bare_remote(tmp.path(), &repo);

    git(&repo, &["branch", "feature"]);
    git(&repo, &["switch", "main"]);
    fs::write(repo.join("f.txt"), "f\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "advance"]);
    git(&repo, &["push", "origin", "main"]);
    git(&repo, &["switch", "feature"]);

    let state_file = tmp.path().join("state.json");
    fs::write(
        &state_file,
        r#"{"branch":"feature","freshness_retries":1.0}"#,
    )
    .unwrap();

    let raw_args = vec![
        "--state-file".to_string(),
        state_file.to_string_lossy().to_string(),
    ];
    let (value, code) = run_impl_main(&raw_args, &repo);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "merged");
    assert_eq!(value["retries"], 2);
}

#[test]
fn run_impl_main_state_string_retries_tolerated() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = make_repo(tmp.path());
    attach_bare_remote(tmp.path(), &repo);

    git(&repo, &["branch", "feature"]);
    git(&repo, &["switch", "main"]);
    fs::write(repo.join("s.txt"), "s\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "advance"]);
    git(&repo, &["push", "origin", "main"]);
    git(&repo, &["switch", "feature"]);

    let state_file = tmp.path().join("state.json");
    fs::write(
        &state_file,
        r#"{"branch":"feature","freshness_retries":"2"}"#,
    )
    .unwrap();

    let raw_args = vec![
        "--state-file".to_string(),
        state_file.to_string_lossy().to_string(),
    ];
    let (value, code) = run_impl_main(&raw_args, &repo);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "merged");
    assert_eq!(value["retries"], 3);
}

/// A merge failure that leaves `git status --porcelain` with no
/// conflict markers (e.g. uncommitted changes block the merge) falls
/// through to the final merge-error JSON. Exercised by leaving a
/// dirty working tree before invoking check-freshness.
#[test]
fn run_impl_main_merge_fails_without_conflict_markers() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = make_repo(tmp.path());
    attach_bare_remote(tmp.path(), &repo);

    git(&repo, &["branch", "feature"]);
    git(&repo, &["switch", "main"]);
    fs::write(repo.join("new_main.txt"), "main\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "main advance"]);
    git(&repo, &["push", "origin", "main"]);

    git(&repo, &["switch", "feature"]);
    // Leave an UNTRACKED file whose name collides with the file
    // introduced by main — merge aborts with "untracked working tree
    // files would be overwritten" and status --porcelain shows "??" for
    // the untracked file (no conflict markers).
    fs::write(repo.join("new_main.txt"), "local unrelated\n").unwrap();

    let (value, code) = run_impl_main(&[], &repo);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    assert_eq!(value["step"], "merge");
    // No conflict files → no "files" key.
    assert!(
        value.get("files").is_none(),
        "expected no files key in merge-error fallthrough, got: {}",
        value
    );
}

/// With PATH restricted so `git` cannot be spawned, the fetch call's
/// `run_cmd_with_timeout` returns `Err("Failed to spawn git: ...")`.
/// `run_git` folds that into `(-1, "", spawn_err)` so `check_freshness`
/// returns a fetch error — exercises the Err branch of run_git.
#[test]
fn cli_fetch_spawn_failure_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let repo = make_repo(&root);
    attach_bare_remote(&root, &repo);

    let output = flow_rs()
        .arg("check-freshness")
        .current_dir(&repo)
        // Restrict PATH so flow-rs's child git spawn fails.
        .env("PATH", "/nonexistent-path-for-flow-test")
        .env("HOME", &root)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_last_json(&output.stdout);
    assert_eq!(data["status"], "error");
    assert_eq!(data["step"], "fetch");
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains("failed to spawn"),
        "expected spawn-failure message, got: {}",
        data
    );
}

#[test]
fn run_impl_main_state_unparseable_string_defaults_to_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = make_repo(tmp.path());
    attach_bare_remote(tmp.path(), &repo);

    git(&repo, &["branch", "feature"]);
    git(&repo, &["switch", "main"]);
    fs::write(repo.join("g.txt"), "g\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "advance"]);
    git(&repo, &["push", "origin", "main"]);
    git(&repo, &["switch", "feature"]);

    let state_file = tmp.path().join("state.json");
    fs::write(
        &state_file,
        r#"{"branch":"feature","freshness_retries":"garbage"}"#,
    )
    .unwrap();

    let raw_args = vec![
        "--state-file".to_string(),
        state_file.to_string_lossy().to_string(),
    ];
    let (value, code) = run_impl_main(&raw_args, &repo);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "merged");
    assert_eq!(value["retries"], 1);
}
