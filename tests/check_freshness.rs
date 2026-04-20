//! CLI integration tests and library-level tests for
//! `flow-rs check-freshness`. Migrated from inline `#[cfg(test)]` per
//! `.claude/rules/test-placement.md`.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use flow_rs::check_freshness::{
    check_freshness, check_freshness_impl, exit_code_for_status, run_git_cmd, run_impl_main,
    CmdResult,
};
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

// --- Library-level tests (migrated from inline `#[cfg(test)]`) ---

fn mock_runner(responses: Vec<CmdResult>) -> impl FnMut(&[&str], u64) -> CmdResult {
    let mut iter = responses.into_iter();
    move |_args, _timeout| iter.next().expect("Unexpected extra git call")
}

fn ok_result() -> CmdResult {
    CmdResult::Ok {
        returncode: 0,
        stdout: String::new(),
        stderr: String::new(),
    }
}

fn err_result(returncode: i32, stderr: &str) -> CmdResult {
    CmdResult::Ok {
        returncode,
        stdout: String::new(),
        stderr: stderr.to_string(),
    }
}

fn stdout_ok(stdout: &str) -> CmdResult {
    CmdResult::Ok {
        returncode: 0,
        stdout: stdout.to_string(),
        stderr: String::new(),
    }
}

fn make_state_file(dir: &Path, retries: i64) -> PathBuf {
    let path = dir.join("state.json");
    fs::write(
        &path,
        json!({"branch": "test", "freshness_retries": retries}).to_string(),
    )
    .unwrap();
    path
}

#[test]
fn test_up_to_date() {
    let responses = vec![ok_result(), ok_result()];
    let mut git = mock_runner(responses);
    let result = check_freshness_impl(None, &mut git);
    assert_eq!(result, json!({"status": "up_to_date"}));
}

#[test]
fn test_merged() {
    let responses = vec![ok_result(), err_result(1, ""), stdout_ok("Merge made\n")];
    let mut git = mock_runner(responses);
    let result = check_freshness_impl(None, &mut git);
    assert_eq!(result, json!({"status": "merged"}));
}

#[test]
fn test_conflict() {
    let responses = vec![
        ok_result(),
        err_result(1, ""),
        err_result(1, "CONFLICT"),
        stdout_ok("UU file1.py\nAA file2.py\n"),
    ];
    let mut git = mock_runner(responses);
    let result = check_freshness_impl(None, &mut git);
    assert_eq!(
        result,
        json!({"status": "conflict", "files": ["file1.py", "file2.py"]})
    );
}

#[test]
fn test_fetch_failure() {
    let responses = vec![err_result(1, "Could not resolve host")];
    let mut git = mock_runner(responses);
    let result = check_freshness_impl(None, &mut git);
    assert_eq!(
        result,
        json!({
            "status": "error",
            "step": "fetch",
            "message": "Could not resolve host",
        })
    );
}

#[test]
fn test_merge_error_non_conflict() {
    let responses = vec![
        ok_result(),
        err_result(1, ""),
        err_result(1, "merge failed"),
        stdout_ok(""),
    ];
    let mut git = mock_runner(responses);
    let result = check_freshness_impl(None, &mut git);
    assert_eq!(
        result,
        json!({
            "status": "error",
            "step": "merge",
            "message": "merge failed",
        })
    );
}

#[test]
fn test_fetch_timeout() {
    let responses = vec![CmdResult::Timeout];
    let mut git = mock_runner(responses);
    let result = check_freshness_impl(None, &mut git);
    assert_eq!(
        result,
        json!({
            "status": "error",
            "step": "fetch",
            "message": "git fetch timed out after 60s",
        })
    );
}

#[test]
fn test_merge_base_timeout() {
    let responses = vec![
        ok_result(),
        CmdResult::Timeout,
        stdout_ok("Already up to date"),
    ];
    let mut git = mock_runner(responses);
    let result = check_freshness_impl(None, &mut git);
    assert_eq!(result, json!({"status": "merged"}));
}

#[test]
fn test_merge_timeout() {
    let responses = vec![ok_result(), err_result(1, ""), CmdResult::Timeout];
    let mut git = mock_runner(responses);
    let result = check_freshness_impl(None, &mut git);
    assert_eq!(
        result,
        json!({
            "status": "error",
            "step": "merge",
            "message": "git merge timed out after 60s",
        })
    );
}

#[test]
fn test_status_porcelain_timeout() {
    let responses = vec![
        ok_result(),
        err_result(1, ""),
        err_result(1, "CONFLICT"),
        CmdResult::Timeout,
    ];
    let mut git = mock_runner(responses);
    let result = check_freshness_impl(None, &mut git);
    assert_eq!(
        result,
        json!({
            "status": "error",
            "step": "merge",
            "message": "CONFLICT",
        })
    );
}

#[test]
fn test_correct_git_commands_up_to_date() {
    let calls = RefCell::new(Vec::<(Vec<String>, u64)>::new());
    let responses = RefCell::new(vec![ok_result(), ok_result()].into_iter());
    let mut git = |args: &[&str], timeout: u64| -> CmdResult {
        calls
            .borrow_mut()
            .push((args.iter().map(|s| s.to_string()).collect(), timeout));
        responses.borrow_mut().next().unwrap()
    };
    check_freshness_impl(None, &mut git);
    let recorded = calls.into_inner();
    assert_eq!(recorded.len(), 2);
    assert_eq!(
        recorded[0].0,
        ["git", "fetch", "origin", "main"]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(recorded[0].1, 60);
}

#[test]
fn test_correct_git_commands_merged() {
    let calls = RefCell::new(Vec::<(Vec<String>, u64)>::new());
    let responses = RefCell::new(vec![ok_result(), err_result(1, ""), ok_result()].into_iter());
    let mut git = |args: &[&str], timeout: u64| -> CmdResult {
        calls
            .borrow_mut()
            .push((args.iter().map(|s| s.to_string()).collect(), timeout));
        responses.borrow_mut().next().unwrap()
    };
    check_freshness_impl(None, &mut git);
    let recorded = calls.into_inner();
    assert_eq!(recorded.len(), 3);
}

#[test]
fn test_dd_conflict_detected() {
    let responses = vec![
        ok_result(),
        err_result(1, ""),
        err_result(1, "CONFLICT"),
        stdout_ok("DD deleted.py\n"),
    ];
    let mut git = mock_runner(responses);
    let result = check_freshness_impl(None, &mut git);
    assert_eq!(result["status"], "conflict");
    assert_eq!(result["files"], json!(["deleted.py"]));
}

#[test]
fn test_retry_increment() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = make_state_file(dir.path(), 0);
    let responses = vec![ok_result(), err_result(1, ""), ok_result()];
    let mut git = mock_runner(responses);
    let result = check_freshness_impl(Some(&state_file), &mut git);
    assert_eq!(result, json!({"status": "merged", "retries": 1}));
}

#[test]
fn test_retry_max_reached() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = make_state_file(dir.path(), 3);
    let mut git = mock_runner(vec![]);
    let result = check_freshness_impl(Some(&state_file), &mut git);
    assert_eq!(result, json!({"status": "max_retries", "retries": 3}));
}

#[test]
fn test_retry_no_state_file() {
    let responses = vec![ok_result(), err_result(1, ""), ok_result()];
    let mut git = mock_runner(responses);
    let result = check_freshness_impl(None, &mut git);
    assert_eq!(result, json!({"status": "merged"}));
    assert!(result.get("retries").is_none());
}

#[test]
fn test_retry_not_incremented_on_up_to_date() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = make_state_file(dir.path(), 1);
    let responses = vec![ok_result(), ok_result()];
    let mut git = mock_runner(responses);
    let result = check_freshness_impl(Some(&state_file), &mut git);
    assert_eq!(result, json!({"status": "up_to_date"}));
}

#[test]
fn test_retry_increment_on_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = make_state_file(dir.path(), 1);
    let responses = vec![
        ok_result(),
        err_result(1, ""),
        err_result(1, "CONFLICT"),
        stdout_ok("UU conflict.py\n"),
    ];
    let mut git = mock_runner(responses);
    let result = check_freshness_impl(Some(&state_file), &mut git);
    assert_eq!(result["status"], "conflict");
    assert_eq!(result["retries"], 2);
}

#[test]
fn test_retry_array_root_state_skips_read_and_increment() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, "[1, 2, 3]").unwrap();
    let responses = vec![ok_result(), err_result(1, ""), ok_result()];
    let mut git = mock_runner(responses);
    let result = check_freshness_impl(Some(&path), &mut git);
    assert_eq!(result, json!({"status": "merged", "retries": 0}));
}

#[test]
fn test_retry_missing_key_in_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, json!({"branch": "test"}).to_string()).unwrap();
    let responses = vec![ok_result(), err_result(1, ""), ok_result()];
    let mut git = mock_runner(responses);
    let result = check_freshness_impl(Some(&path), &mut git);
    assert_eq!(result, json!({"status": "merged", "retries": 1}));
}

#[test]
fn test_retry_value_as_float() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, r#"{"branch":"test","freshness_retries":1.0}"#).unwrap();
    let responses = vec![ok_result(), err_result(1, ""), ok_result()];
    let mut git = mock_runner(responses);
    let result = check_freshness_impl(Some(&path), &mut git);
    assert_eq!(result, json!({"status": "merged", "retries": 2}));
}

#[test]
fn test_retry_value_as_string() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, r#"{"branch":"test","freshness_retries":"2"}"#).unwrap();
    let responses = vec![ok_result(), err_result(1, ""), ok_result()];
    let mut git = mock_runner(responses);
    let result = check_freshness_impl(Some(&path), &mut git);
    assert_eq!(result, json!({"status": "merged", "retries": 3}));
}

#[test]
fn test_retry_value_as_unparseable_string_defaults_to_zero() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, r#"{"branch":"test","freshness_retries":"garbage"}"#).unwrap();
    let responses = vec![ok_result(), err_result(1, ""), ok_result()];
    let mut git = mock_runner(responses);
    let result = check_freshness_impl(Some(&path), &mut git);
    assert_eq!(result, json!({"status": "merged", "retries": 1}));
}

fn expect_ok(result: CmdResult) -> (i32, String, String) {
    match result {
        CmdResult::Ok {
            returncode,
            stdout,
            stderr,
        } => (returncode, stdout, stderr),
        CmdResult::Timeout => panic!("expected Ok, got Timeout"),
    }
}

fn expect_timeout(result: CmdResult) {
    match result {
        CmdResult::Timeout => {}
        CmdResult::Ok {
            returncode,
            stdout,
            stderr,
        } => panic!(
            "expected Timeout, got Ok(returncode={}, stdout={:?}, stderr={:?})",
            returncode, stdout, stderr
        ),
    }
}

#[test]
fn run_git_cmd_success_returns_returncode_zero() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let (code, _, _) = expect_ok(run_git_cmd(&["/usr/bin/true"], 5, &cwd));
    assert_eq!(code, 0);
}

#[test]
fn run_git_cmd_nonzero_exit_returns_returncode() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let (code, _, _) = expect_ok(run_git_cmd(&["/usr/bin/false"], 5, &cwd));
    assert_eq!(code, 1);
}

#[test]
fn run_git_cmd_spawn_failure_returns_127() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let (code, _, _) = expect_ok(run_git_cmd(&["/no/such/binary/here-deadbeef"], 5, &cwd));
    assert_eq!(code, 127);
}

#[test]
fn run_git_cmd_timeout_returns_timeout_variant() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    expect_timeout(run_git_cmd(&["/bin/sleep", "10"], 1, &cwd));
}

#[test]
fn run_git_cmd_signal_killed_returns_negative_one() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let (code, _, _) = expect_ok(run_git_cmd(&["/bin/sh", "-c", "kill -9 $$"], 5, &cwd));
    assert_eq!(code, -1);
}

#[test]
fn run_git_cmd_captures_stdout_and_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let (code, stdout, stderr) = expect_ok(run_git_cmd(
        &["/bin/sh", "-c", "echo hi; echo err >&2"],
        5,
        &cwd,
    ));
    assert_eq!(code, 0);
    assert!(stdout.contains("hi"));
    assert!(stderr.contains("err"));
}

#[test]
#[should_panic(expected = "expected Ok")]
fn expect_ok_panics_on_timeout() {
    expect_ok(CmdResult::Timeout);
}

#[test]
#[should_panic(expected = "Unexpected extra git call")]
fn mock_runner_panics_when_responses_exhausted() {
    let mut git = mock_runner(vec![ok_result()]);
    git(&[], 0);
    git(&[], 0);
}

#[test]
#[should_panic(expected = "expected Timeout")]
fn expect_timeout_panics_on_ok() {
    expect_timeout(CmdResult::Ok {
        returncode: 0,
        stdout: String::new(),
        stderr: String::new(),
    });
}

#[test]
fn check_freshness_production_wrapper_returns_json_object() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let result = check_freshness(None, &cwd);
    assert!(result.is_object());
    assert!(result.get("status").is_some());
}

#[test]
fn run_impl_main_without_state_file_arg_parses_no_state() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let raw_args: Vec<String> = Vec::new();
    let (value, code) = run_impl_main(&raw_args, &cwd);
    assert_eq!(code, 1);
    assert!(value.is_object());
}

#[test]
fn run_impl_main_with_state_file_arg_parses_path() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let state_path = cwd.join("state.json");
    fs::write(&state_path, json!({"branch": "test"}).to_string()).unwrap();
    let raw_args = vec![
        "--state-file".to_string(),
        state_path.to_string_lossy().to_string(),
    ];
    let (_value, _code) = run_impl_main(&raw_args, &cwd);
}

#[test]
fn run_impl_main_ignores_unknown_flags() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let raw_args = vec!["--unknown-flag".to_string(), "ignored".to_string()];
    let (_value, _code) = run_impl_main(&raw_args, &cwd);
}

#[test]
fn exit_code_for_status_up_to_date_returns_zero() {
    assert_eq!(exit_code_for_status(&json!({"status": "up_to_date"})), 0);
}

#[test]
fn exit_code_for_status_merged_returns_zero() {
    assert_eq!(exit_code_for_status(&json!({"status": "merged"})), 0);
}

#[test]
fn exit_code_for_status_error_returns_one() {
    assert_eq!(exit_code_for_status(&json!({"status": "error"})), 1);
}

#[test]
fn exit_code_for_status_missing_status_returns_one() {
    assert_eq!(exit_code_for_status(&json!({})), 1);
}

#[test]
fn run_impl_main_state_file_arg_without_value_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let raw_args = vec!["--state-file".to_string()];
    let (_value, _code) = run_impl_main(&raw_args, &cwd);
}
