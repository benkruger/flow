//! Integration tests for `bin/flow issue`.
//!
//! The command wraps `gh issue create` with label-retry logic, body
//! file handling, repo detection fallbacks, and a Code Review filing
//! ban. Tests install a mock `gh` on PATH and state-file fixtures to
//! cover every branch.

mod common;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::rc::Rc;
use std::time::Duration;

use common::{create_gh_stub, create_git_repo_with_remote, parse_output};
use flow_rs::issue::{
    create_issue_with_runner, extract_error, fetch_database_id_with_runner, parse_issue_number,
    read_body_file, retry_with_label_with_runner, run_gh_cmd_inner, run_impl_main, Args, GhRunner,
};
use serde_json::json;

fn run_cmd(repo: &Path, args: &[&str], stub_dir: &Path) -> Output {
    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("issue")
        .args(args)
        .current_dir(repo)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

#[test]
fn issue_create_happy_path_with_repo_flag() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\necho 'https://github.com/owner/name/issues/42'\nexit 0\n",
    );

    let output = run_cmd(
        &repo,
        &["--repo", "owner/name", "--title", "Test issue"],
        &stub_dir,
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["url"], "https://github.com/owner/name/issues/42");
    assert_eq!(data["number"], 42);
}

#[test]
fn issue_create_with_label_success() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\necho 'https://github.com/o/r/issues/5'\nexit 0\n",
    );

    let output = run_cmd(
        &repo,
        &["--repo", "o/r", "--title", "Labeled", "--label", "bug"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["number"], 5);
}

#[test]
fn issue_create_label_not_found_retries_with_create() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // First issue-create call fails with "label not found".
    // Then `gh label create` succeeds.
    // Second issue-create call (retry with label) succeeds.
    let counter = dir.path().join(".counter");
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\n\
             COUNTER=\"{}\"\n\
             if [ ! -f \"$COUNTER\" ]; then echo 0 > \"$COUNTER\"; fi\n\
             if [ \"$1\" = \"label\" ] && [ \"$2\" = \"create\" ]; then\n\
               exit 0\n\
             fi\n\
             N=$(cat \"$COUNTER\")\n\
             N=$((N + 1))\n\
             echo $N > \"$COUNTER\"\n\
             if [ \"$N\" -eq 1 ]; then\n\
               echo 'could not add label: label not found' >&2\n\
               exit 1\n\
             fi\n\
             echo 'https://github.com/o/r/issues/10'\n\
             exit 0\n",
            counter.display()
        ),
    );

    let output = run_cmd(
        &repo,
        &["--repo", "o/r", "--title", "T", "--label", "new-label"],
        &stub_dir,
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["number"], 10);
}

#[test]
fn issue_create_label_not_found_and_create_fails_retries_without_label() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // First issue-create fails with "label not found"
    // Then `gh label create` ALSO fails
    // Then second issue-create (without label) succeeds
    let counter = dir.path().join(".counter");
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\n\
             COUNTER=\"{}\"\n\
             if [ ! -f \"$COUNTER\" ]; then echo 0 > \"$COUNTER\"; fi\n\
             if [ \"$1\" = \"label\" ] && [ \"$2\" = \"create\" ]; then\n\
               exit 1\n\
             fi\n\
             N=$(cat \"$COUNTER\")\n\
             N=$((N + 1))\n\
             echo $N > \"$COUNTER\"\n\
             if [ \"$N\" -eq 1 ]; then\n\
               echo 'label not found' >&2\n\
               exit 1\n\
             fi\n\
             echo 'https://github.com/o/r/issues/11'\n\
             exit 0\n",
            counter.display()
        ),
    );

    let output = run_cmd(
        &repo,
        &["--repo", "o/r", "--title", "T", "--label", "untouchable"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["number"], 11);
}

#[test]
fn issue_create_gh_failure_unrelated_to_label_propagates() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho 'some other error' >&2\nexit 1\n");

    let output = run_cmd(&repo, &["--repo", "o/r", "--title", "T"], &stub_dir);

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("some other error"));
}

#[test]
fn issue_create_with_body_file_reads_and_deletes_it() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let body_file = repo.join(".flow-issue-body");
    fs::write(&body_file, "Issue body text").unwrap();

    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\necho 'https://github.com/o/r/issues/7'\nexit 0\n",
    );

    let output = run_cmd(
        &repo,
        &[
            "--repo",
            "o/r",
            "--title",
            "T",
            "--body-file",
            body_file.to_str().unwrap(),
        ],
        &stub_dir,
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    // body file should be deleted after reading
    assert!(!body_file.exists(), "body file should be consumed");
}

#[test]
fn issue_create_missing_body_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");
    let missing = repo.join(".nonexistent-body");

    let output = run_cmd(
        &repo,
        &[
            "--repo",
            "o/r",
            "--title",
            "T",
            "--body-file",
            missing.to_str().unwrap(),
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Could not read body file"));
}

#[test]
fn issue_create_resolves_repo_from_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_file = dir.path().join("state.json");
    fs::write(
        &state_file,
        json!({"repo": "state-owner/state-name"}).to_string(),
    )
    .unwrap();
    // gh prints the URL; the stub accepts any --repo.
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\necho 'https://github.com/state-owner/state-name/issues/1'\nexit 0\n",
    );

    let output = run_cmd(
        &repo,
        &["--title", "T", "--state-file", state_file.to_str().unwrap()],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert!(data["url"]
        .as_str()
        .unwrap()
        .starts_with("https://github.com/state-owner/state-name/issues/"));
}

#[test]
fn issue_create_no_repo_and_no_detection_exits_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // Helper sets a local `bare.git` remote which detect_repo rejects
    // (requires github.com URL). No --repo → error exit.
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_cmd(&repo, &["--title", "T"], &stub_dir);

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Could not detect repo"));
}

// --- read_body_file ---

#[test]
fn read_body_file_reads_and_deletes() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(".flow-issue-body");
    fs::write(&file, "Issue body with | pipes and && ampersands").unwrap();

    let result = read_body_file(file.to_str().unwrap(), dir.path());

    assert_eq!(result.unwrap(), "Issue body with | pipes and && ampersands");
    assert!(!file.exists());
}

#[test]
fn read_body_file_missing_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("nonexistent.md");

    let result = read_body_file(file.to_str().unwrap(), dir.path());

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Could not read body file"));
}

#[test]
fn read_body_file_empty_returns_empty_string() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(".flow-issue-body");
    fs::write(&file, "").unwrap();

    let result = read_body_file(file.to_str().unwrap(), dir.path());

    assert_eq!(result.unwrap(), "");
    assert!(!file.exists());
}

#[test]
fn read_body_file_rich_markdown_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(".flow-issue-body");
    let content = "## Summary\n\n| Column | Value |\n|--------|-------|\n| A | B |\n";
    fs::write(&file, content).unwrap();

    let result = read_body_file(file.to_str().unwrap(), dir.path());

    assert_eq!(result.unwrap(), content);
}

#[test]
fn read_body_file_relative_resolved_against_root() {
    let dir = tempfile::tempdir().unwrap();
    let project_dir = dir.path().join("project");
    fs::create_dir_all(&project_dir).unwrap();
    let file = project_dir.join(".flow-issue-body");
    fs::write(&file, "Resolved body").unwrap();

    let result = read_body_file(".flow-issue-body", &project_dir);

    assert_eq!(result.unwrap(), "Resolved body");
    assert!(!file.exists());
}

#[test]
fn read_body_file_absolute_ignores_root() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(".flow-issue-body");
    fs::write(&file, "Absolute body").unwrap();

    let other_root = dir.path().join("other");
    fs::create_dir_all(&other_root).unwrap();

    let result = read_body_file(file.to_str().unwrap(), &other_root);

    assert_eq!(result.unwrap(), "Absolute body");
}

#[test]
fn read_body_file_relative_missing_returns_error() {
    let dir = tempfile::tempdir().unwrap();

    let result = read_body_file("nonexistent.md", dir.path());

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Could not read body file"));
}

// --- parse_issue_number ---

#[test]
fn parse_issue_number_standard_url() {
    assert_eq!(
        parse_issue_number("https://github.com/owner/repo/issues/42"),
        Some(42)
    );
}

#[test]
fn parse_issue_number_large_number() {
    assert_eq!(
        parse_issue_number("https://github.com/owner/repo/issues/99999"),
        Some(99999)
    );
}

#[test]
fn parse_issue_number_invalid_url() {
    assert_eq!(parse_issue_number("not a url"), None);
}

#[test]
fn parse_issue_number_empty_string() {
    assert_eq!(parse_issue_number(""), None);
}

#[test]
fn parse_issue_number_pull_request_url() {
    assert_eq!(
        parse_issue_number("https://github.com/owner/repo/pull/42"),
        None
    );
}

// --- extract_error ---

#[test]
fn extract_error_prefers_stderr() {
    assert_eq!(extract_error("stderr msg", "stdout msg"), "stderr msg");
}

#[test]
fn extract_error_falls_back_to_stdout() {
    assert_eq!(extract_error("", "stdout msg"), "stdout msg");
}

#[test]
fn extract_error_unknown_when_both_empty() {
    assert_eq!(extract_error("", ""), "Unknown error");
}

// --- Args parsing ---

#[test]
fn args_parses_milestone() {
    use clap::Parser;
    let args =
        Args::try_parse_from(["issue", "--title", "Test issue", "--milestone", "v1.0"]).unwrap();
    assert_eq!(args.milestone.as_deref(), Some("v1.0"));
}

#[test]
fn args_milestone_defaults_to_none() {
    use clap::Parser;
    let args = Args::try_parse_from(["issue", "--title", "Test issue"]).unwrap();
    assert!(args.milestone.is_none());
}

#[test]
fn args_parses_override_code_review_ban() {
    use clap::Parser;
    let args =
        Args::try_parse_from(["issue", "--title", "Test", "--override-code-review-ban"]).unwrap();
    assert!(args.override_code_review_ban);
}

#[test]
fn args_override_defaults_to_false() {
    use clap::Parser;
    let args = Args::try_parse_from(["issue", "--title", "Test"]).unwrap();
    assert!(!args.override_code_review_ban);
}

// --- _with_runner seams ---

type GhResult = Result<String, String>;

fn mock_runner(responses: Vec<GhResult>) -> impl Fn(&[&str], Option<Duration>) -> GhResult {
    let queue = RefCell::new(VecDeque::from(responses));
    move |_args: &[&str], _timeout: Option<Duration>| -> GhResult {
        queue
            .borrow_mut()
            .pop_front()
            .expect("no more mock responses")
    }
}

#[test]
fn create_issue_with_runner_returns_result_on_runner_ok() {
    let runner = mock_runner(vec![
        Ok("https://github.com/owner/name/issues/42".to_string()),
        Ok("12345".to_string()),
    ]);
    let result =
        create_issue_with_runner("owner/name", "Title", None, None, None, &runner).unwrap();
    assert_eq!(result.url, "https://github.com/owner/name/issues/42");
    assert_eq!(result.number, Some(42));
    assert_eq!(result.id, Some(12345));
}

#[test]
fn create_issue_with_runner_propagates_err_when_label_none() {
    let runner = mock_runner(vec![Err("network down".to_string())]);
    let err =
        create_issue_with_runner("owner/name", "Title", None, None, None, &runner).unwrap_err();
    assert!(err.contains("network down"));
}

#[test]
fn create_issue_with_runner_label_not_found_triggers_retry() {
    let runner = mock_runner(vec![
        Err("could not add label: label not found".to_string()),
        Ok(String::new()),
        Ok("https://github.com/owner/name/issues/77".to_string()),
        Ok("9999".to_string()),
    ]);
    let result =
        create_issue_with_runner("owner/name", "Title", Some("Bug"), None, None, &runner).unwrap();
    assert_eq!(result.number, Some(77));
    assert_eq!(result.id, Some(9999));
}

#[test]
fn create_issue_with_runner_propagates_unrelated_err() {
    let runner = mock_runner(vec![Err("authentication failed".to_string())]);
    let err = create_issue_with_runner("owner/name", "Title", Some("Bug"), None, None, &runner)
        .unwrap_err();
    assert!(err.contains("authentication failed"));
}

#[test]
fn create_issue_with_runner_passes_body_and_milestone_to_runner() {
    let captured: Rc<RefCell<Vec<Vec<String>>>> = Rc::new(RefCell::new(Vec::new()));
    let captured_clone = captured.clone();
    let runner = move |args: &[&str], _t: Option<Duration>| {
        captured_clone
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        if args.contains(&"create") {
            Ok("https://github.com/owner/name/issues/1".to_string())
        } else {
            Ok("4242".to_string())
        }
    };
    let result = create_issue_with_runner(
        "owner/name",
        "Title",
        None,
        Some("body text"),
        Some("v1.0"),
        &runner,
    )
    .unwrap();
    assert_eq!(result.number, Some(1));
    let calls = captured.borrow().clone();
    let create_call = calls
        .iter()
        .find(|c| c.iter().any(|a| a == "create"))
        .unwrap();
    assert!(create_call.iter().any(|a| a == "--body"));
    assert!(create_call.iter().any(|a| a == "body text"));
    assert!(create_call.iter().any(|a| a == "--milestone"));
    assert!(create_call.iter().any(|a| a == "v1.0"));
}

#[test]
fn retry_with_label_with_runner_passes_body_and_milestone_to_runner() {
    let captured: Rc<RefCell<Vec<Vec<String>>>> = Rc::new(RefCell::new(Vec::new()));
    let captured_clone = captured.clone();
    let runner = move |args: &[&str], _t: Option<Duration>| {
        captured_clone
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        if args.contains(&"label") {
            Ok(String::new())
        } else if args.contains(&"create") {
            Ok("https://github.com/owner/name/issues/9".to_string())
        } else {
            Ok("9000".to_string())
        }
    };
    let result = retry_with_label_with_runner(
        "owner/name",
        "Title",
        "Flow",
        Some("retry body"),
        Some("v2.0"),
        Duration::from_secs(5),
        &runner,
    )
    .unwrap();
    assert_eq!(result.number, Some(9));
    let calls = captured.borrow().clone();
    let retry_call = calls
        .iter()
        .find(|c| c.iter().any(|a| a == "issue") && c.iter().any(|a| a == "create"))
        .unwrap();
    assert!(retry_call.iter().any(|a| a == "--body"));
    assert!(retry_call.iter().any(|a| a == "retry body"));
    assert!(retry_call.iter().any(|a| a == "--milestone"));
    assert!(retry_call.iter().any(|a| a == "v2.0"));
}

#[test]
fn retry_with_label_with_runner_label_created_then_retry_succeeds() {
    let runner = mock_runner(vec![
        Ok(String::new()),
        Ok("https://github.com/owner/name/issues/55".to_string()),
        Ok("5555".to_string()),
    ]);
    let result = retry_with_label_with_runner(
        "owner/name",
        "Title",
        "Flow",
        None,
        None,
        Duration::from_secs(5),
        &runner,
    )
    .unwrap();
    assert_eq!(result.number, Some(55));
}

#[test]
fn retry_with_label_with_runner_label_create_fails_retries_without_label() {
    let runner = mock_runner(vec![
        Err("label create permission denied".to_string()),
        Ok("https://github.com/owner/name/issues/33".to_string()),
        Ok("3333".to_string()),
    ]);
    let result = retry_with_label_with_runner(
        "owner/name",
        "Title",
        "Flow",
        None,
        None,
        Duration::from_secs(5),
        &runner,
    )
    .unwrap();
    assert_eq!(result.number, Some(33));
}

#[test]
fn retry_with_label_with_runner_retry_fails_propagates_err() {
    let runner = mock_runner(vec![Ok(String::new()), Err("retry timeout".to_string())]);
    let err = retry_with_label_with_runner(
        "owner/name",
        "Title",
        "Flow",
        None,
        None,
        Duration::from_secs(5),
        &runner,
    )
    .unwrap_err();
    assert!(err.contains("retry timeout"));
}

#[test]
fn fetch_database_id_with_runner_returns_id_on_ok_numeric() {
    let runner = mock_runner(vec![Ok("42".to_string())]);
    let (id, err) = fetch_database_id_with_runner("owner/name", 1, &runner);
    assert_eq!(id, Some(42));
    assert!(err.is_none());
}

#[test]
fn fetch_database_id_with_runner_returns_err_on_invalid_id() {
    let runner = mock_runner(vec![Ok("not-a-number".to_string())]);
    let (id, err) = fetch_database_id_with_runner("owner/name", 1, &runner);
    assert!(id.is_none());
    assert!(err.unwrap().contains("Invalid ID"));
}

#[test]
fn fetch_database_id_with_runner_propagates_runner_err() {
    let runner = mock_runner(vec![Err("api down".to_string())]);
    let (id, err) = fetch_database_id_with_runner("owner/name", 1, &runner);
    assert!(id.is_none());
    assert!(err.unwrap().contains("api down"));
}

// --- run_impl_main: Code Review filing gate (drives through public
// `run_impl_main` surface — the private `should_reject_for_code_review`
// helper is only reachable from within `issue.rs`). ---

fn default_args() -> Args {
    Args {
        repo: Some("owner/name".to_string()),
        title: "Test".to_string(),
        label: None,
        body_file: None,
        state_file: None,
        milestone: None,
        override_code_review_ban: false,
    }
}

fn success_runner() -> Box<GhRunner> {
    Box::new(mock_runner(vec![
        Ok("https://github.com/owner/name/issues/1".to_string()),
        Ok("10".to_string()),
    ]))
}

#[test]
fn gate_blocks_when_current_phase_is_code_review() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some(r#"{"current_phase":"flow-code-review"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let runner = mock_runner(vec![]);
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo, &runner);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    let msg = value["message"].as_str().unwrap();
    assert!(msg.contains("Code Review"));
    assert!(msg.contains("override-code-review-ban"));
}

#[test]
fn gate_allows_with_override_in_code_review() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some(r#"{"current_phase":"flow-code-review"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let runner = success_runner();
    let mut args = default_args();
    args.override_code_review_ban = true;
    let (value, code) = run_impl_main(args, dir.path(), &state, &repo, &runner);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
}

#[test]
fn gate_allows_in_learn_phase() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some(r#"{"current_phase":"flow-learn"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let runner = success_runner();
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo, &runner);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
}

#[test]
fn gate_allows_in_code_phase() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some(r#"{"current_phase":"flow-code"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let runner = success_runner();
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo, &runner);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
}

#[test]
fn gate_allows_in_start_phase() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some(r#"{"current_phase":"flow-start"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let runner = success_runner();
    let (_value, code) = run_impl_main(default_args(), dir.path(), &state, &repo, &runner);
    assert_eq!(code, 0);
}

#[test]
fn gate_allows_when_no_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let state = || None;
    let repo = || Some("owner/name".to_string());
    let runner = success_runner();
    let (_value, code) = run_impl_main(default_args(), dir.path(), &state, &repo, &runner);
    assert_eq!(code, 0);
}

#[test]
fn gate_fails_closed_when_state_malformed() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some("not json".to_string());
    let repo = || Some("owner/name".to_string());
    let runner = mock_runner(vec![]);
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo, &runner);
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("not valid JSON"));
}

#[test]
fn gate_fails_closed_when_current_phase_missing() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some(r#"{"branch":"x"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let runner = mock_runner(vec![]);
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo, &runner);
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("missing or not a string"));
}

#[test]
fn gate_fails_closed_when_current_phase_is_array() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some(r#"{"current_phase":["flow-code-review"]}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let runner = mock_runner(vec![]);
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo, &runner);
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("missing or not a string"));
}

#[test]
fn gate_fails_closed_when_state_has_bom() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some("\u{feff}{\"current_phase\":\"flow-code-review\"}".to_string());
    let repo = || Some("owner/name".to_string());
    let runner = mock_runner(vec![]);
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo, &runner);
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Code Review"));
}

#[test]
fn gate_fails_closed_when_state_has_bom_and_no_code_review() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some("\u{feff}{\"current_phase\":\"flow-learn\"}".to_string());
    let repo = || Some("owner/name".to_string());
    let runner = mock_runner(vec![]);
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo, &runner);
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("not valid JSON"));
}

#[test]
fn gate_allows_when_state_is_empty_string() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some(String::new());
    let repo = || Some("owner/name".to_string());
    let runner = success_runner();
    let (_value, code) = run_impl_main(default_args(), dir.path(), &state, &repo, &runner);
    assert_eq!(code, 0);
}

#[test]
fn gate_allows_when_state_is_whitespace_only() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some("   \n  ".to_string());
    let repo = || Some("owner/name".to_string());
    let runner = success_runner();
    let (_value, code) = run_impl_main(default_args(), dir.path(), &state, &repo, &runner);
    assert_eq!(code, 0);
}

#[test]
fn gate_blocks_when_current_phase_is_whitespace_padded() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some(r#"{"current_phase":" flow-code-review "}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let runner = mock_runner(vec![]);
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo, &runner);
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Code Review"));
}

#[test]
fn gate_blocks_when_current_phase_is_uppercase() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some(r#"{"current_phase":"FLOW-CODE-REVIEW"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let runner = mock_runner(vec![]);
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo, &runner);
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Code Review"));
}

#[test]
fn gate_blocks_when_current_phase_has_trailing_nul() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some("{\"current_phase\":\"flow-code-review\\u0000\"}".to_string());
    let repo = || Some("owner/name".to_string());
    let runner = mock_runner(vec![]);
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo, &runner);
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Code Review"));
}

#[test]
fn gate_blocks_when_current_phase_duplicate_key_serde_last_wins() {
    let dir = tempfile::tempdir().unwrap();
    let state =
        || Some(r#"{"current_phase":"flow-code-review","current_phase":"flow-learn"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let runner = mock_runner(vec![]);
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo, &runner);
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Code Review"));
}

#[test]
fn gate_blocks_when_duplicate_key_in_reverse_order() {
    let dir = tempfile::tempdir().unwrap();
    let state =
        || Some(r#"{"current_phase":"flow-learn","current_phase":"flow-code-review"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let runner = mock_runner(vec![]);
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo, &runner);
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Code Review"));
}

#[test]
fn gate_raw_scanner_advances_past_current_phase_without_colon() {
    // Covers the raw scanner's `strip_prefix(':')` None fallthrough
    // (line 163): the 15-byte literal `"current_phase"` appears
    // without a trailing `:`. After trim_start, strip_prefix(':')
    // returns None and the scanner advances. The parser path then
    // fails to parse the malformed content and the gate fails CLOSED.
    let dir = tempfile::tempdir().unwrap();
    let state = || Some(r#""current_phase" prefix no colon"#.to_string());
    let repo = || Some("owner/name".to_string());
    let runner = mock_runner(vec![]);
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo, &runner);
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("not valid JSON"));
}

#[test]
fn gate_fails_closed_when_current_phase_value_has_no_closing_quote() {
    // Covers the raw scanner's `value_body.find('"')` None fallthrough
    // (line 161): the state has `"current_phase":"flow-code-review`
    // without a closing quote. The raw scanner finds `:` and opening
    // quote but no end quote, falls through, then the JSON parser
    // rejects it and the gate fails CLOSED.
    let dir = tempfile::tempdir().unwrap();
    let state = || Some(r#"{"current_phase":"flow-code-review"#.to_string());
    let repo = || Some("owner/name".to_string());
    let runner = mock_runner(vec![]);
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo, &runner);
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("not valid JSON"));
}

#[test]
fn gate_blocks_when_current_phase_value_has_padding_in_raw_text() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some(r#"{"current_phase":" flow-code-review "}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let runner = mock_runner(vec![]);
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo, &runner);
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Code Review"));
}

// --- run_impl_main: repo resolution from state file (drives through
// the private `resolve_repo_from_state` helper via the `--state-file`
// arg path). ---

type CapturedCalls = Rc<RefCell<Vec<Vec<String>>>>;

fn capturing_runner() -> (
    CapturedCalls,
    impl Fn(&[&str], Option<Duration>) -> GhResult,
) {
    let captured: Rc<RefCell<Vec<Vec<String>>>> = Rc::new(RefCell::new(Vec::new()));
    let captured_clone = captured.clone();
    let runner = move |args: &[&str], _t: Option<Duration>| -> GhResult {
        captured_clone
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        if args.contains(&"create") {
            Ok("https://github.com/captured/repo/issues/1".to_string())
        } else {
            Ok("42".to_string())
        }
    };
    (captured, runner)
}

fn repo_arg_from(calls: &[Vec<String>]) -> Option<String> {
    for call in calls {
        if let Some(pos) = call.iter().position(|a| a == "--repo") {
            return call.get(pos + 1).cloned();
        }
    }
    None
}

#[test]
fn resolve_repo_from_valid_state_uses_state_repo() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = dir.path().join("state.json");
    fs::write(&state_file, r#"{"repo": "cached/repo", "branch": "test"}"#).unwrap();
    let state = || None;
    let repo = || Some("fallback/repo".to_string());
    let (captured, runner) = capturing_runner();
    let mut args = default_args();
    args.repo = None;
    args.state_file = Some(state_file.to_str().unwrap().to_string());
    let (_, code) = run_impl_main(args, dir.path(), &state, &repo, &runner);
    assert_eq!(code, 0);
    assert_eq!(
        repo_arg_from(&captured.borrow()),
        Some("cached/repo".to_string())
    );
}

#[test]
fn resolve_repo_from_corrupt_state_falls_back_to_resolver() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = dir.path().join("bad.json");
    fs::write(&state_file, "{corrupt").unwrap();
    let state = || None;
    let repo = || Some("fallback/repo".to_string());
    let (captured, runner) = capturing_runner();
    let mut args = default_args();
    args.repo = None;
    args.state_file = Some(state_file.to_str().unwrap().to_string());
    let (_, code) = run_impl_main(args, dir.path(), &state, &repo, &runner);
    assert_eq!(code, 0);
    assert_eq!(
        repo_arg_from(&captured.borrow()),
        Some("fallback/repo".to_string())
    );
}

#[test]
fn resolve_repo_from_state_no_repo_key_falls_back_to_resolver() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = dir.path().join("state.json");
    fs::write(&state_file, r#"{"branch": "test"}"#).unwrap();
    let state = || None;
    let repo = || Some("fallback/repo".to_string());
    let (captured, runner) = capturing_runner();
    let mut args = default_args();
    args.repo = None;
    args.state_file = Some(state_file.to_str().unwrap().to_string());
    let (_, code) = run_impl_main(args, dir.path(), &state, &repo, &runner);
    assert_eq!(code, 0);
    assert_eq!(
        repo_arg_from(&captured.borrow()),
        Some("fallback/repo".to_string())
    );
}

#[test]
fn resolve_repo_from_missing_state_file_falls_back_to_resolver() {
    let dir = tempfile::tempdir().unwrap();
    let state = || None;
    let repo = || Some("fallback/repo".to_string());
    let (captured, runner) = capturing_runner();
    let mut args = default_args();
    args.repo = None;
    args.state_file = Some("/nonexistent/state.json".to_string());
    let (_, code) = run_impl_main(args, dir.path(), &state, &repo, &runner);
    assert_eq!(code, 0);
    assert_eq!(
        repo_arg_from(&captured.borrow()),
        Some("fallback/repo".to_string())
    );
}

#[test]
fn resolve_repo_from_state_file_with_no_repo_and_no_fallback_errors() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = dir.path().join("state.json");
    fs::write(&state_file, r#"{"branch": "test"}"#).unwrap();
    let state = || None;
    let repo = || None;
    let runner = mock_runner(vec![]);
    let mut args = default_args();
    args.repo = None;
    args.state_file = Some(state_file.to_str().unwrap().to_string());
    let (value, code) = run_impl_main(args, dir.path(), &state, &repo, &runner);
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Could not detect repo"));
}

// --- run_impl_main: top-level dispatch branches ---

#[test]
fn run_impl_main_repo_resolver_only_path_uses_resolver_repo() {
    // Covers the outermost `else` branch `Some(r) => r,` — no
    // --repo, no --state-file, repo_resolver succeeds.
    let dir = tempfile::tempdir().unwrap();
    let state = || None;
    let repo = || Some("resolver/repo".to_string());
    let (captured, runner) = capturing_runner();
    let mut args = default_args();
    args.repo = None;
    args.state_file = None;
    let (_, code) = run_impl_main(args, dir.path(), &state, &repo, &runner);
    assert_eq!(code, 0);
    assert_eq!(
        repo_arg_from(&captured.borrow()),
        Some("resolver/repo".to_string())
    );
}

#[test]
fn issue_run_impl_main_blocked_by_code_review_returns_error_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some(r#"{"current_phase":"flow-code-review"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let runner = mock_runner(vec![]);
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo, &runner);
    assert_eq!(value["status"], "error");
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Code Review"));
}

#[test]
fn issue_run_impl_main_no_repo_returns_error_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let state = || None;
    let repo = || None;
    let runner = mock_runner(vec![]);
    let mut args = default_args();
    args.repo = None;
    let (value, code) = run_impl_main(args, dir.path(), &state, &repo, &runner);
    assert_eq!(value["status"], "error");
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Could not detect repo"));
}

#[test]
fn issue_run_impl_main_body_file_missing_returns_error_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let state = || None;
    let repo = || Some("owner/name".to_string());
    let runner = mock_runner(vec![]);
    let mut args = default_args();
    args.body_file = Some("nonexistent-body.md".to_string());
    let (value, code) = run_impl_main(args, dir.path(), &state, &repo, &runner);
    assert_eq!(value["status"], "error");
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Could not read body file"));
}

#[test]
fn issue_run_impl_main_happy_path_returns_ok_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let state = || None;
    let repo = || Some("owner/name".to_string());
    let runner = mock_runner(vec![
        Ok("https://github.com/owner/name/issues/100".to_string()),
        Ok("777".to_string()),
    ]);
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo, &runner);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["number"], 100);
    assert_eq!(value["id"], 777);
}

// --- run_gh_cmd_inner ---

#[test]
fn run_gh_cmd_inner_success_returns_stdout() {
    let factory = |_args: &[&str]| {
        Command::new("sh")
            .args(["-c", "echo ok"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    };
    let out = run_gh_cmd_inner(&["irrelevant"], Some(Duration::from_secs(5)), &factory).unwrap();
    assert_eq!(out, "ok");
}

#[test]
fn run_gh_cmd_inner_nonzero_returns_extracted_error() {
    let factory = |_args: &[&str]| {
        Command::new("sh")
            .args(["-c", "echo boom 1>&2; exit 1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    };
    let err =
        run_gh_cmd_inner(&["irrelevant"], Some(Duration::from_secs(5)), &factory).unwrap_err();
    assert!(err.contains("boom"));
}

#[test]
fn run_gh_cmd_inner_timeout_kills_child_returns_err() {
    let factory = |_args: &[&str]| {
        Command::new("sleep")
            .arg("5")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    };
    let err =
        run_gh_cmd_inner(&["irrelevant"], Some(Duration::from_secs(1)), &factory).unwrap_err();
    assert!(
        err.to_lowercase().contains("timed out"),
        "expected timeout error, got {}",
        err
    );
}

#[test]
fn run_gh_cmd_inner_no_timeout_success_returns_stdout() {
    // Covers the `else` branch of `if let Some(dur) = timeout` —
    // the no-timeout path does a direct `wait_with_output`.
    let factory = |_args: &[&str]| {
        Command::new("sh")
            .args(["-c", "echo hi"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    };
    let out = run_gh_cmd_inner(&["irrelevant"], None, &factory).unwrap();
    assert_eq!(out, "hi");
}

#[test]
fn run_gh_cmd_inner_no_timeout_nonzero_returns_extracted_error() {
    let factory = |_args: &[&str]| {
        Command::new("sh")
            .args(["-c", "echo oops 1>&2; exit 1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    };
    let err = run_gh_cmd_inner(&["irrelevant"], None, &factory).unwrap_err();
    assert!(err.contains("oops"));
}

#[test]
fn run_gh_cmd_inner_spawn_error_returns_err() {
    let factory = |_args: &[&str]| -> std::io::Result<Child> {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such binary",
        ))
    };
    let err =
        run_gh_cmd_inner(&["irrelevant"], Some(Duration::from_secs(5)), &factory).unwrap_err();
    assert!(err.contains("no such binary") || err.contains("Failed to spawn"));
}
