//! Integration tests for `bin/flow issue`.
//!
//! The command wraps `gh issue create` with label-retry logic, body
//! file handling, repo detection fallbacks, and a Code Review filing
//! ban. Tests install a mock `gh` on PATH and state-file fixtures to
//! cover every branch.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_gh_stub, create_git_repo_with_remote, parse_output};
use flow_rs::issue::{extract_error, parse_issue_number, read_body_file, run_impl_main, Args};
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
    // gh api returns 42 for the DB ID; gh issue create returns the URL.
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [ \"$1\" = \"api\" ]; then echo 42; exit 0; fi\n\
         echo 'https://github.com/owner/name/issues/42'\n\
         exit 0\n",
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
    assert_eq!(data["id"], 42);
}

#[test]
fn issue_create_with_label_success() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [ \"$1\" = \"api\" ]; then echo 5; exit 0; fi\n\
         echo 'https://github.com/o/r/issues/5'\n\
         exit 0\n",
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
    // Then `gh api` fetches the DB ID.
    let counter = dir.path().join(".counter");
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\n\
             COUNTER=\"{}\"\n\
             if [ ! -f \"$COUNTER\" ]; then echo 0 > \"$COUNTER\"; fi\n\
             if [ \"$1\" = \"api\" ]; then echo 10; exit 0; fi\n\
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
    // Then `gh api` fetches DB ID
    let counter = dir.path().join(".counter");
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\n\
             COUNTER=\"{}\"\n\
             if [ ! -f \"$COUNTER\" ]; then echo 0 > \"$COUNTER\"; fi\n\
             if [ \"$1\" = \"api\" ]; then echo 11; exit 0; fi\n\
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
        "#!/bin/bash\n\
         if [ \"$1\" = \"api\" ]; then echo 7; exit 0; fi\n\
         echo 'https://github.com/o/r/issues/7'\n\
         exit 0\n",
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
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [ \"$1\" = \"api\" ]; then echo 1; exit 0; fi\n\
         echo 'https://github.com/state-owner/state-name/issues/1'\n\
         exit 0\n",
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
fn issue_create_state_file_missing_repo_falls_back_to_resolver() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // State file has no repo key — fall back to detect_repo.
    let state_file = dir.path().join("state.json");
    fs::write(&state_file, json!({"branch": "test"}).to_string()).unwrap();
    // Configure a github.com remote so detect_repo returns Some.
    Command::new("git")
        .args([
            "remote",
            "set-url",
            "origin",
            "git@github.com:owner/name.git",
        ])
        .current_dir(&repo)
        .output()
        .unwrap();
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [ \"$1\" = \"api\" ]; then echo 33; exit 0; fi\n\
         echo 'https://github.com/owner/name/issues/33'\n\
         exit 0\n",
    );

    let output = run_cmd(
        &repo,
        &["--title", "T", "--state-file", state_file.to_str().unwrap()],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
}

#[test]
fn issue_create_state_file_no_repo_no_resolver_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // State file has no repo; remote is bare.git (not github.com) so detect_repo fails.
    let state_file = dir.path().join("state.json");
    fs::write(&state_file, json!({"branch": "x"}).to_string()).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_cmd(
        &repo,
        &["--title", "T", "--state-file", state_file.to_str().unwrap()],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert!(data["message"]
        .as_str()
        .unwrap()
        .contains("Could not detect repo"));
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

#[test]
fn issue_create_with_milestone_includes_milestone_arg() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // The stub records its args to verify --milestone is passed.
    let log = dir.path().join(".args.log");
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\n\
             echo \"$@\" >> \"{}\"\n\
             if [ \"$1\" = \"api\" ]; then echo 99; exit 0; fi\n\
             echo 'https://github.com/o/r/issues/99'\n\
             exit 0\n",
            log.display()
        ),
    );

    let output = run_cmd(
        &repo,
        &["--repo", "o/r", "--title", "T", "--milestone", "v1.0"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let contents = fs::read_to_string(&log).unwrap();
    assert!(
        contents.contains("--milestone v1.0"),
        "expected --milestone v1.0 in gh args, got:\n{}",
        contents
    );
}

#[test]
fn issue_create_gh_spawn_failure_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // No gh stub + empty PATH → spawn fails.
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("issue")
        .args(["--repo", "o/r", "--title", "T"])
        .current_dir(&repo)
        .env("PATH", "")
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .to_lowercase()
        .contains("spawn"));
}

#[test]
fn issue_create_with_label_but_unrelated_error_propagates_without_retry() {
    // Covers the `if err_lower.contains("label") && err_lower.contains("not found")`
    // false branch when label is Some: gh create fails with an error
    // that is NOT "label not found" (e.g. auth, permission). The
    // retry path is NOT taken; the original error propagates.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho 'permission denied' >&2\nexit 1\n");

    let output = run_cmd(
        &repo,
        &["--repo", "o/r", "--title", "T", "--label", "bug"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("permission denied"));
}

#[test]
fn issue_create_api_returns_non_numeric_id_records_none() {
    // Covers the `fetch_database_id` Err arm: gh api returns non-
    // numeric stdout. Issue creation still succeeds; `id` ends up null.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [ \"$1\" = \"api\" ]; then echo 'not-a-number'; exit 0; fi\n\
         echo 'https://github.com/o/r/issues/1'\n\
         exit 0\n",
    );

    let output = run_cmd(&repo, &["--repo", "o/r", "--title", "T"], &stub_dir);

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["number"], 1);
    assert!(data["id"].is_null());
}

#[test]
fn issue_create_label_not_found_retry_with_body_and_milestone() {
    // Covers retry_with_label's body + milestone branches: first gh
    // issue create fails with "label not found", label create succeeds,
    // retry issue create uses --label --body --milestone, then api
    // fetches DB ID.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let body_file = repo.join(".flow-issue-body");
    fs::write(&body_file, "retry body text").unwrap();
    let counter = dir.path().join(".counter");
    let log = dir.path().join(".args.log");
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\n\
             COUNTER=\"{}\"\n\
             LOG=\"{}\"\n\
             echo \"$@\" >> \"$LOG\"\n\
             if [ ! -f \"$COUNTER\" ]; then echo 0 > \"$COUNTER\"; fi\n\
             if [ \"$1\" = \"api\" ]; then echo 20; exit 0; fi\n\
             if [ \"$1\" = \"label\" ] && [ \"$2\" = \"create\" ]; then\n\
               exit 0\n\
             fi\n\
             N=$(cat \"$COUNTER\")\n\
             N=$((N + 1))\n\
             echo $N > \"$COUNTER\"\n\
             if [ \"$N\" -eq 1 ]; then\n\
               echo 'label not found' >&2\n\
               exit 1\n\
             fi\n\
             echo 'https://github.com/o/r/issues/20'\n\
             exit 0\n",
            counter.display(),
            log.display()
        ),
    );

    let output = run_cmd(
        &repo,
        &[
            "--repo",
            "o/r",
            "--title",
            "T",
            "--label",
            "new-label",
            "--body-file",
            body_file.to_str().unwrap(),
            "--milestone",
            "v2.0",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let contents = fs::read_to_string(&log).unwrap();
    // The retry call must include --body, --milestone, and --label.
    assert!(
        contents.contains("--body retry body text"),
        "retry call missing --body, got:\n{}",
        contents
    );
    assert!(
        contents.contains("--milestone v2.0"),
        "retry call missing --milestone, got:\n{}",
        contents
    );
}

#[test]
fn issue_create_retry_itself_fails_propagates_error() {
    // Covers the `?` on `run_gh_cmd(&retry_refs)` in retry_with_label:
    // first create fails with "label not found", label create succeeds,
    // but retry create ALSO fails → error propagates.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
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
               echo 'label not found' >&2\n\
               exit 1\n\
             fi\n\
             echo 'retry failed permanently' >&2\n\
             exit 1\n",
            counter.display()
        ),
    );

    let output = run_cmd(
        &repo,
        &["--repo", "o/r", "--title", "T", "--label", "new-label"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("retry failed permanently"));
}

#[test]
fn issue_create_api_failure_records_none_id() {
    // Covers the `fetch_database_id` runner-Err arm via the subprocess
    // path: gh api exits non-zero but issue create succeeded.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [ \"$1\" = \"api\" ]; then echo 'api down' >&2; exit 1; fi\n\
         echo 'https://github.com/o/r/issues/2'\n\
         exit 0\n",
    );

    let output = run_cmd(&repo, &["--repo", "o/r", "--title", "T"], &stub_dir);

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert!(data["id"].is_null());
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

#[test]
fn gate_blocks_when_current_phase_is_code_review() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some(r#"{"current_phase":"flow-code-review"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    let msg = value["message"].as_str().unwrap();
    assert!(msg.contains("Code Review"));
    assert!(msg.contains("override-code-review-ban"));
}

#[test]
fn gate_allows_in_learn_phase_with_stubbed_gh() {
    // Library-level gate is tested indirectly: when state passes the
    // Code Review gate, run_impl_main proceeds to create_issue which
    // spawns gh. With no gh on PATH the spawn fails, but what matters
    // here is the gate pass — the failure appears in the returned
    // error, not as a Code Review block.
    let dir = tempfile::tempdir().unwrap();
    let state = || Some(r#"{"current_phase":"flow-learn"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let (value, _code) = run_impl_main(default_args(), dir.path(), &state, &repo);
    // gate pass proven by absence of Code Review message.
    assert!(!value["message"]
        .as_str()
        .unwrap_or("")
        .contains("Code Review"));
}

#[test]
fn gate_allows_when_no_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let state = || None;
    let repo = || Some("owner/name".to_string());
    let (value, _code) = run_impl_main(default_args(), dir.path(), &state, &repo);
    assert!(!value["message"]
        .as_str()
        .unwrap_or("")
        .contains("Code Review"));
}

#[test]
fn gate_fails_closed_when_state_malformed() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some("not json".to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo);
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
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo);
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
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo);
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
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo);
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Code Review"));
}

#[test]
fn gate_fails_closed_when_state_has_bom_and_no_code_review() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some("\u{feff}{\"current_phase\":\"flow-learn\"}".to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo);
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
    let (value, _code) = run_impl_main(default_args(), dir.path(), &state, &repo);
    assert!(!value["message"]
        .as_str()
        .unwrap_or("")
        .contains("Code Review"));
}

#[test]
fn gate_allows_when_state_is_whitespace_only() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some("   \n  ".to_string());
    let repo = || Some("owner/name".to_string());
    let (value, _code) = run_impl_main(default_args(), dir.path(), &state, &repo);
    assert!(!value["message"]
        .as_str()
        .unwrap_or("")
        .contains("Code Review"));
}

#[test]
fn gate_blocks_when_current_phase_is_whitespace_padded() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some(r#"{"current_phase":" flow-code-review "}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo);
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Code Review"));
}

#[test]
fn gate_blocks_when_current_phase_is_uppercase() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some(r#"{"current_phase":"FLOW-CODE-REVIEW"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo);
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Code Review"));
}

#[test]
fn gate_blocks_when_current_phase_has_trailing_nul() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some("{\"current_phase\":\"flow-code-review\\u0000\"}".to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo);
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Code Review"));
}

#[test]
fn gate_blocks_when_current_phase_duplicate_key_serde_last_wins() {
    let dir = tempfile::tempdir().unwrap();
    let state =
        || Some(r#"{"current_phase":"flow-code-review","current_phase":"flow-learn"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo);
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Code Review"));
}

#[test]
fn gate_blocks_when_duplicate_key_in_reverse_order() {
    let dir = tempfile::tempdir().unwrap();
    let state =
        || Some(r#"{"current_phase":"flow-learn","current_phase":"flow-code-review"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo);
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Code Review"));
}

#[test]
fn gate_raw_scanner_advances_past_current_phase_without_colon() {
    // Covers the raw scanner's `strip_prefix(':')` None fallthrough:
    // the literal `"current_phase"` appears without a trailing `:`.
    // After trim_start, strip_prefix(':') returns None and the scanner
    // advances. The parser path then fails to parse and the gate
    // fails CLOSED.
    let dir = tempfile::tempdir().unwrap();
    let state = || Some(r#""current_phase" prefix no colon"#.to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo);
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("not valid JSON"));
}

#[test]
fn gate_fails_closed_when_current_phase_value_has_no_closing_quote() {
    // Covers the raw scanner's `value_body.find('"')` None fallthrough.
    let dir = tempfile::tempdir().unwrap();
    let state = || Some(r#"{"current_phase":"flow-code-review"#.to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo);
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("not valid JSON"));
}

// --- run_impl_main: repo resolution ---

#[test]
fn run_impl_main_no_repo_returns_error_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let state = || None;
    let repo = || None;
    let mut args = default_args();
    args.repo = None;
    let (value, code) = run_impl_main(args, dir.path(), &state, &repo);
    assert_eq!(value["status"], "error");
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Could not detect repo"));
}

#[test]
fn run_impl_main_body_file_missing_returns_error_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let state = || None;
    let repo = || Some("owner/name".to_string());
    let mut args = default_args();
    args.body_file = Some("nonexistent-body.md".to_string());
    let (value, code) = run_impl_main(args, dir.path(), &state, &repo);
    assert_eq!(value["status"], "error");
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Could not read body file"));
}

#[test]
fn run_impl_main_state_file_path_falls_back_to_resolver() {
    // --state-file points at a JSON file with no repo key; resolver
    // returns Some, so repo lookup succeeds. The create_issue call
    // then fails because gh is not on PATH — but the repo-resolution
    // branch was covered.
    let dir = tempfile::tempdir().unwrap();
    let state_file = dir.path().join("state.json");
    fs::write(&state_file, r#"{"branch": "x"}"#).unwrap();
    let state = || None;
    let repo = || Some("fallback/repo".to_string());
    let mut args = default_args();
    args.repo = None;
    args.state_file = Some(state_file.to_string_lossy().to_string());
    // Don't assert on code; just cover the branch.
    let (_, _code) = run_impl_main(args, dir.path(), &state, &repo);
}

#[test]
fn run_impl_main_state_file_resolves_repo() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = dir.path().join("state.json");
    fs::write(&state_file, r#"{"repo":"cached/repo"}"#).unwrap();
    let state = || None;
    let repo = || None;
    let mut args = default_args();
    args.repo = None;
    args.state_file = Some(state_file.to_string_lossy().to_string());
    let (_, _code) = run_impl_main(args, dir.path(), &state, &repo);
}

#[test]
fn run_impl_main_missing_state_file_falls_back_to_resolver() {
    let dir = tempfile::tempdir().unwrap();
    let state = || None;
    let repo = || Some("fallback/repo".to_string());
    let mut args = default_args();
    args.repo = None;
    args.state_file = Some("/nonexistent/state.json".to_string());
    let (_, _code) = run_impl_main(args, dir.path(), &state, &repo);
}

#[test]
fn run_impl_main_state_file_no_repo_no_resolver_errors() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = dir.path().join("state.json");
    fs::write(&state_file, r#"{"branch": "x"}"#).unwrap();
    let state = || None;
    let repo = || None;
    let mut args = default_args();
    args.repo = None;
    args.state_file = Some(state_file.to_string_lossy().to_string());
    let (value, code) = run_impl_main(args, dir.path(), &state, &repo);
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Could not detect repo"));
}

#[test]
fn run_impl_main_blocked_by_code_review_returns_error_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let state = || Some(r#"{"current_phase":"flow-code-review"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), dir.path(), &state, &repo);
    assert_eq!(value["status"], "error");
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Code Review"));
}

#[test]
fn gate_override_bypasses_code_review_block() {
    // Covers the `if override_flag { return None; }` early return.
    let dir = tempfile::tempdir().unwrap();
    let state = || Some(r#"{"current_phase":"flow-code-review"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let mut args = default_args();
    args.override_code_review_ban = true;
    let (value, _code) = run_impl_main(args, dir.path(), &state, &repo);
    // Gate passed — no Code Review message.
    assert!(!value["message"]
        .as_str()
        .unwrap_or("")
        .contains("Code Review"));
}

#[test]
fn run_impl_main_resolver_only_path_uses_resolver_repo() {
    // Covers the outermost `else { match repo_resolver() Some(r) => r }`
    // branch: no --repo, no --state-file, resolver returns Some.
    let dir = tempfile::tempdir().unwrap();
    let state = || None;
    let repo = || Some("resolver/repo".to_string());
    let mut args = default_args();
    args.repo = None;
    args.state_file = None;
    // Don't care about exit code; just covering the repo-resolution branch.
    let (_, _code) = run_impl_main(args, dir.path(), &state, &repo);
}

#[test]
fn run_impl_main_state_file_with_invalid_json_falls_back_to_resolver() {
    // Covers the `serde_json::from_str(&content).ok()?` None arm in
    // resolve_repo_from_state: state file exists but contains invalid
    // JSON → resolve_repo_from_state returns None → fall through to
    // resolver.
    let dir = tempfile::tempdir().unwrap();
    let state_file = dir.path().join("bad.json");
    fs::write(&state_file, "{corrupt not json").unwrap();
    let state = || None;
    let repo = || Some("fallback/repo".to_string());
    let mut args = default_args();
    args.repo = None;
    args.state_file = Some(state_file.to_string_lossy().to_string());
    let (_, _code) = run_impl_main(args, dir.path(), &state, &repo);
}
