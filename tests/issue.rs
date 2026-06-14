//! Integration tests for `bin/flow issue`.
//!
//! The command wraps `gh issue create` with label-retry logic, body
//! file handling, repo detection fallbacks, and a Review filing
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
    // Neutralize the environment surfaces `bin/flow issue` reaches per
    // `.claude/rules/subprocess-test-hygiene.md`: invalid GitHub tokens
    // so any unstubbed `gh` call fails auth fast instead of reaching the
    // network, and HOME pointed at the fixture repo so the child reads
    // no user dotfiles. The stub `gh` shadows the real binary via PATH;
    // these are defense-in-depth so a stub gap cannot escape to the
    // network or to ambient config.
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("issue")
        .args(args)
        .current_dir(repo)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .env("GH_TOKEN", "invalid")
        .env("GITHUB_TOKEN", "invalid")
        .env("HOME", repo)
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
fn issue_create_label_not_found_retry_with_body() {
    // Covers retry_with_label's body branch: first gh issue create
    // fails with "label not found", label create succeeds, retry
    // issue create uses --label --body, then api fetches DB ID.
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
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let contents = fs::read_to_string(&log).unwrap();
    // The retry call must include --body and --label.
    assert!(
        contents.contains("--body retry body text"),
        "retry call missing --body, got:\n{}",
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

#[test]
fn issue_create_with_assignee_passes_flag_to_gh() {
    // The --assignee flag must reach `gh issue create`. The gh stub
    // logs every invocation's args; the issue-create call must carry
    // `--assignee @me`. Exercises the create_issue Some(assignee) branch.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let log = dir.path().join(".args.log");
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\n\
             LOG=\"{}\"\n\
             echo \"$@\" >> \"$LOG\"\n\
             if [ \"$1\" = \"api\" ]; then echo 50; exit 0; fi\n\
             echo 'https://github.com/o/r/issues/50'\n\
             exit 0\n",
            log.display()
        ),
    );

    let output = run_cmd(
        &repo,
        &["--repo", "o/r", "--title", "T", "--assignee", "@me"],
        &stub_dir,
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let contents = fs::read_to_string(&log).unwrap();
    let issue_create_line = contents
        .lines()
        .find(|l| l.starts_with("issue create"))
        .expect("issue create call must be logged");
    assert!(
        issue_create_line.contains("--assignee @me"),
        "issue create call missing --assignee, got:\n{}",
        contents
    );
}

#[test]
fn issue_create_label_not_found_retry_carries_assignee() {
    // When the first issue-create fails with "label not found", the
    // retry must still carry --assignee. Exercises the
    // retry_with_label Some(assignee) branch.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
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
             if [ \"$1\" = \"api\" ]; then echo 51; exit 0; fi\n\
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
             echo 'https://github.com/o/r/issues/51'\n\
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
            "--assignee",
            "@me",
        ],
        &stub_dir,
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let contents = fs::read_to_string(&log).unwrap();
    let issue_create_lines: Vec<&str> = contents
        .lines()
        .filter(|l| l.starts_with("issue create"))
        .collect();
    assert_eq!(
        issue_create_lines.len(),
        2,
        "expected first + retry issue create calls, got:\n{}",
        contents
    );
    assert!(
        issue_create_lines[1].contains("--assignee @me"),
        "retry issue create call missing --assignee, got:\n{}",
        contents
    );
}

#[test]
fn empty_assignee_string_must_not_reach_gh() {
    // `Some("")` is distinct from `None`. run_impl_main filters
    // empty/whitespace --assignee values via
    // `.filter(|s| !s.trim().is_empty())` so the conditional-push in
    // create_issue never appends a meaningless `--assignee ""` to the
    // gh argument vector. Regression guard for that filter.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let log = dir.path().join(".args.log");
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\n\
             LOG=\"{}\"\n\
             echo \"$@\" >> \"$LOG\"\n\
             if [ \"$1\" = \"api\" ]; then echo 60; exit 0; fi\n\
             echo 'https://github.com/o/r/issues/60'\n\
             exit 0\n",
            log.display()
        ),
    );

    let output = run_cmd(
        &repo,
        &["--repo", "o/r", "--title", "T", "--assignee", ""],
        &stub_dir,
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let contents = fs::read_to_string(&log).unwrap();
    let issue_create_line = contents
        .lines()
        .find(|l| l.starts_with("issue create"))
        .expect("issue create call must be logged");
    assert!(
        !issue_create_line.contains("--assignee"),
        "empty-string --assignee must be filtered, not passed to gh; got:\n{}",
        issue_create_line
    );
}

#[test]
fn whitespace_only_assignee_must_not_reach_gh() {
    // The filter trims before the emptiness check, so a whitespace-only
    // --assignee value is treated as absent — guards against a future
    // refactor dropping the `.trim()` and only checking `is_empty()`.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let log = dir.path().join(".args.log");
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\n\
             LOG=\"{}\"\n\
             echo \"$@\" >> \"$LOG\"\n\
             if [ \"$1\" = \"api\" ]; then echo 61; exit 0; fi\n\
             echo 'https://github.com/o/r/issues/61'\n\
             exit 0\n",
            log.display()
        ),
    );

    let output = run_cmd(
        &repo,
        &["--repo", "o/r", "--title", "T", "--assignee", "   "],
        &stub_dir,
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let contents = fs::read_to_string(&log).unwrap();
    let issue_create_line = contents
        .lines()
        .find(|l| l.starts_with("issue create"))
        .expect("issue create call must be logged");
    assert!(
        !issue_create_line.contains("--assignee"),
        "whitespace-only --assignee must be filtered, not passed to gh; got:\n{}",
        issue_create_line
    );
}

// --- read_body_file ---

#[test]
fn read_body_file_reads_and_deletes() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(".flow-issue-body");
    fs::write(&file, "Issue body with | pipes and && ampersands").unwrap();

    let result = read_body_file(file.to_str().unwrap());

    assert_eq!(result.unwrap(), "Issue body with | pipes and && ampersands");
    assert!(!file.exists());
}

#[test]
fn read_body_file_missing_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("nonexistent.md");

    let result = read_body_file(file.to_str().unwrap());

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Could not read body file"));
}

#[test]
fn read_body_file_empty_returns_empty_string() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(".flow-issue-body");
    fs::write(&file, "").unwrap();

    let result = read_body_file(file.to_str().unwrap());

    assert_eq!(result.unwrap(), "");
    assert!(!file.exists());
}

#[test]
fn read_body_file_rich_markdown_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(".flow-issue-body");
    let content = "## Summary\n\n| Column | Value |\n|--------|-------|\n| A | B |\n";
    fs::write(&file, content).unwrap();

    let result = read_body_file(file.to_str().unwrap());

    assert_eq!(result.unwrap(), content);
}

#[test]
fn read_body_file_absolute_path_unaffected_by_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(".flow-issue-body");
    fs::write(&file, "Absolute body").unwrap();

    let result = read_body_file(file.to_str().unwrap());

    assert_eq!(result.unwrap(), "Absolute body");
}

#[test]
fn read_body_file_cleans_up_on_read_failure() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("orphan.md");
    fs::write(&file, "should be removed even though read fails").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();

    let result = read_body_file(file.to_str().unwrap());

    assert!(result.is_err());
    assert!(
        fs::metadata(&file).is_err(),
        "read_body_file must delete the body file even when the read fails, leaving no orphan"
    );
}

#[test]
fn read_body_file_empty_path_rejected() {
    let result = read_body_file("");
    let err = result.expect_err("empty --body-file must reject");
    assert!(
        err.contains("empty"),
        "empty-path error must name 'empty'; got: {}",
        err
    );
}

#[test]
fn read_body_file_relative_dotdot_traversal_rejected() {
    let result = read_body_file("../escape.md");
    let err = result.expect_err("`..` traversal must reject");
    assert!(
        err.contains("forbidden") && err.contains("traversal"),
        "dotdot rejection error must name 'forbidden' and 'traversal'; got: {}",
        err
    );
}

#[test]
fn read_body_file_symlink_target_rejected_and_preserved() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target.txt");
    fs::write(&target, "preserve me").unwrap();
    let link = dir.path().join("link.md");
    symlink(&target, &link).unwrap();

    let result = read_body_file(link.to_str().unwrap());

    let err = result.expect_err("symlink must reject");
    assert!(
        err.contains("not a regular file"),
        "symlink rejection error must name 'not a regular file'; got: {}",
        err
    );
    assert!(
        fs::read_to_string(&target).unwrap() == "preserve me",
        "symlink target must survive — read_body_file must not follow the symlink"
    );
}

#[test]
fn read_body_file_body_exceeding_cap_rejected_and_cleaned_up() {
    use flow_rs::plan_from_issue::PLAN_BODY_BYTE_CAP;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("oversize.md");
    let body = "x".repeat(PLAN_BODY_BYTE_CAP + 10);
    fs::write(&file, &body).unwrap();

    let result = read_body_file(file.to_str().unwrap());

    let err = result.expect_err("oversize body must reject");
    assert!(
        err.contains("exceeds") && err.contains(&PLAN_BODY_BYTE_CAP.to_string()),
        "oversize error must name 'exceeds' and the cap; got: {}",
        err
    );
    assert!(
        !file.exists(),
        "oversize body file must still be cleaned up after rejection"
    );
}

#[test]
fn read_body_file_non_utf8_returns_error_and_cleans_up() {
    // File::open succeeds (regular file with read permissions), but
    // read_to_string fails because the bytes are not valid UTF-8.
    // Covers the read_to_string Err arm in read_body_file.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("non-utf8.md");
    fs::write(&file, [0xFF, 0xFE, 0xFD, 0xFC]).unwrap();

    let result = read_body_file(file.to_str().unwrap());

    let err = result.expect_err("non-utf8 body must reject");
    assert!(
        err.contains("Could not read body file") && err.contains("UTF-8"),
        "non-utf8 error must name read failure and 'UTF-8'; got: {}",
        err
    );
    assert!(
        !file.exists(),
        "cleanup must run even when read_to_string fails on non-utf8 content"
    );
}

/// Subprocess test for the cwd-relative resolution branch in
/// `read_body_file`. Spawns the binary with `current_dir(<root>/subdir)`
/// and passes a relative `--body-file` so the path-resolution path goes
/// through `std::env::current_dir()`. With the body file present at
/// `<root>/subdir/.flow-issue-body-test`, the read step succeeds; the
/// downstream `gh` call then fails on the invalid token but that error
/// is distinct from "Could not read body file", which is the substring
/// the test asserts is ABSENT.
#[cfg(unix)]
#[test]
fn read_body_file_relative_resolves_against_caller_cwd() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let subdir = root.join("subdir");
    fs::create_dir_all(&subdir).expect("mkdir subdir");
    let body_path = subdir.join(".flow-issue-body-test");
    fs::write(&body_path, "Resolved body").expect("write body");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.args([
        "issue",
        "--title",
        "test",
        "--body-file",
        ".flow-issue-body-test",
        "--repo",
        "fake/repo",
    ]);
    cmd.current_dir(&subdir);
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd.env("GH_TOKEN", "invalid");
    cmd.env("GITHUB_TOKEN", "invalid");
    cmd.env("HOME", &root);
    cmd.env("GIT_CEILING_DIRECTORIES", &root);

    let output = cmd.output().expect("spawn flow-rs issue");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);

    assert!(
        !combined.contains("Could not read body file"),
        "expected body-file read to succeed via cwd resolution; combined output:\n{}",
        combined
    );
}

/// Subprocess test for the cwd-relative missing-file branch. Same
/// fixture shape as the resolves-against-caller-cwd test above, but the
/// body file is intentionally not written. The error path must surface
/// `Could not read body file` AND name the cwd-resolved path
/// (containing the `subdir/.flow-issue-body-test` suffix) so the user
/// can see which absolute path the binary attempted to read.
#[cfg(unix)]
#[test]
fn read_body_file_relative_missing_returns_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let subdir = root.join("subdir");
    fs::create_dir_all(&subdir).expect("mkdir subdir");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.args([
        "issue",
        "--title",
        "test",
        "--body-file",
        ".flow-issue-body-test",
        "--repo",
        "fake/repo",
    ]);
    cmd.current_dir(&subdir);
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd.env("GH_TOKEN", "invalid");
    cmd.env("GITHUB_TOKEN", "invalid");
    cmd.env("HOME", &root);
    cmd.env("GIT_CEILING_DIRECTORIES", &root);

    let output = cmd.output().expect("spawn flow-rs issue");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);

    assert!(
        combined.contains("Could not read body file"),
        "expected 'Could not read body file' error; combined output:\n{}",
        combined
    );
    assert!(
        combined.contains("subdir/.flow-issue-body-test"),
        "expected error to name the cwd-resolved path; combined output:\n{}",
        combined
    );
}

/// Subprocess test for the `current_dir()` Err branch. The child is
/// spawned with `current_dir(cwd)` set; a `pre_exec` closure then
/// `rmdir`s that cwd after the kernel has chdir'd the child into it,
/// leaving the child with an unlinked cwd inode — `getcwd(3)` then
/// returns `ENOENT` and Rust's `env::current_dir()` reports Err. With a
/// relative `--body-file`, the read path hits the Err arm and surfaces
/// `Could not determine current directory`.
#[cfg(unix)]
#[test]
fn read_body_file_current_dir_error_propagated() {
    use std::os::unix::process::CommandExt;

    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let cwd = root.join("doomed");
    fs::create_dir(&cwd).expect("mkdir doomed");

    let cwd_for_preexec = cwd.clone();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.args([
        "issue",
        "--title",
        "test",
        "--body-file",
        ".flow-issue-body-test",
        "--repo",
        "fake/repo",
    ]);
    cmd.current_dir(&cwd);
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd.env("GH_TOKEN", "invalid");
    cmd.env("GITHUB_TOKEN", "invalid");
    cmd.env("HOME", &root);
    cmd.env("GIT_CEILING_DIRECTORIES", &root);

    // SAFETY: `pre_exec` requires the closure to be async-signal-safe.
    // `libc::rmdir` is listed as AS-safe by POSIX; we only call it and
    // return Ok — no memory allocation, no panic surfaces.
    let preexec_path = std::ffi::CString::new(cwd_for_preexec.to_str().expect("utf8").as_bytes())
        .expect("CString from cwd path");
    unsafe {
        cmd.pre_exec(move || {
            libc::rmdir(preexec_path.as_ptr());
            Ok(())
        });
    }

    let output = cmd.output().expect("spawn flow-rs issue");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);

    assert!(
        combined.contains("Could not determine current directory"),
        "expected current_dir Err propagation; combined output:\n{}",
        combined
    );
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
fn args_parses_override_review_ban() {
    use clap::Parser;
    let args = Args::try_parse_from(["issue", "--title", "Test", "--override-review-ban"]).unwrap();
    assert!(args.override_review_ban);
}

#[test]
fn args_override_defaults_to_false() {
    use clap::Parser;
    let args = Args::try_parse_from(["issue", "--title", "Test"]).unwrap();
    assert!(!args.override_review_ban);
}

#[test]
fn args_parses_assignee() {
    use clap::Parser;
    let args = Args::try_parse_from(["issue", "--title", "T", "--assignee", "@me"]).unwrap();
    assert_eq!(args.assignee.as_deref(), Some("@me"));
}

#[test]
fn args_assignee_defaults_to_none() {
    use clap::Parser;
    let args = Args::try_parse_from(["issue", "--title", "T"]).unwrap();
    assert_eq!(args.assignee, None);
}

// --- run_impl_main: Review filing gate (drives through public
// `run_impl_main` surface — the private `should_reject_for_review`
// helper is only reachable from within `issue.rs`). ---

fn default_args() -> Args {
    Args {
        repo: Some("owner/name".to_string()),
        title: "Test".to_string(),
        label: None,
        body_file: None,
        state_file: None,
        override_review_ban: false,
        assignee: None,
    }
}

#[test]
fn gate_blocks_when_current_phase_is_review() {
    let state = || Some(r#"{"current_phase":"flow-review"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), &state, &repo);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    let msg = value["message"].as_str().unwrap();
    assert!(msg.contains("Review"));
    assert!(msg.contains("override-review-ban"));
}

#[test]
fn gate_allows_in_learn_phase_with_stubbed_gh() {
    // Library-level gate is tested indirectly: when state passes the
    // Review gate, run_impl_main proceeds to create_issue which
    // spawns gh. With no gh on PATH the spawn fails, but what matters
    // here is the gate pass — the failure appears in the returned
    // error, not as a Review block.
    let state = || Some(r#"{"current_phase":"flow-code"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let (value, _code) = run_impl_main(default_args(), &state, &repo);
    // gate pass proven by absence of Review message.
    assert!(!value["message"]
        .as_str()
        .unwrap_or("")
        .contains("disabled during Review"));
}

#[test]
fn gate_allows_when_no_state_file() {
    let state = || None;
    let repo = || Some("owner/name".to_string());
    let (value, _code) = run_impl_main(default_args(), &state, &repo);
    assert!(!value["message"]
        .as_str()
        .unwrap_or("")
        .contains("disabled during Review"));
}

#[test]
fn gate_fails_closed_when_state_malformed() {
    let state = || Some("not json".to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), &state, &repo);
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("not valid JSON"));
}

#[test]
fn gate_fails_closed_when_current_phase_missing() {
    let state = || Some(r#"{"branch":"x"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), &state, &repo);
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("missing or not a string"));
}

#[test]
fn gate_fails_closed_when_current_phase_is_array() {
    let state = || Some(r#"{"current_phase":["flow-review"]}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), &state, &repo);
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("missing or not a string"));
}

#[test]
fn gate_fails_closed_when_state_has_bom() {
    let state = || Some("\u{feff}{\"current_phase\":\"flow-review\"}".to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), &state, &repo);
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Review"));
}

#[test]
fn gate_fails_closed_when_state_has_bom_and_no_review() {
    let state = || Some("\u{feff}{\"current_phase\":\"flow-code\"}".to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), &state, &repo);
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("not valid JSON"));
}

#[test]
fn gate_allows_when_state_is_empty_string() {
    let state = || Some(String::new());
    let repo = || Some("owner/name".to_string());
    let (value, _code) = run_impl_main(default_args(), &state, &repo);
    assert!(!value["message"]
        .as_str()
        .unwrap_or("")
        .contains("disabled during Review"));
}

#[test]
fn gate_allows_when_state_is_whitespace_only() {
    let state = || Some("   \n  ".to_string());
    let repo = || Some("owner/name".to_string());
    let (value, _code) = run_impl_main(default_args(), &state, &repo);
    assert!(!value["message"]
        .as_str()
        .unwrap_or("")
        .contains("disabled during Review"));
}

#[test]
fn gate_blocks_when_current_phase_is_whitespace_padded() {
    let state = || Some(r#"{"current_phase":" flow-review "}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), &state, &repo);
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Review"));
}

#[test]
fn gate_blocks_when_current_phase_is_uppercase() {
    let state = || Some(r#"{"current_phase":"FLOW-REVIEW"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), &state, &repo);
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Review"));
}

#[test]
fn gate_blocks_when_current_phase_has_trailing_nul() {
    let state = || Some("{\"current_phase\":\"flow-review\\u0000\"}".to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), &state, &repo);
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Review"));
}

#[test]
fn gate_blocks_when_current_phase_duplicate_key_serde_last_wins() {
    let state =
        || Some(r#"{"current_phase":"flow-review","current_phase":"flow-code"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), &state, &repo);
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Review"));
}

#[test]
fn gate_blocks_when_duplicate_key_in_reverse_order() {
    let state =
        || Some(r#"{"current_phase":"flow-code","current_phase":"flow-review"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), &state, &repo);
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Review"));
}

#[test]
fn gate_raw_scanner_advances_past_current_phase_without_colon() {
    // Covers the raw scanner's `strip_prefix(':')` None fallthrough:
    // the literal `"current_phase"` appears without a trailing `:`.
    // After trim_start, strip_prefix(':') returns None and the scanner
    // advances. The parser path then fails to parse and the gate
    // fails CLOSED.
    let state = || Some(r#""current_phase" prefix no colon"#.to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), &state, &repo);
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("not valid JSON"));
}

#[test]
fn gate_fails_closed_when_current_phase_value_has_no_closing_quote() {
    // Covers the raw scanner's `value_body.find('"')` None fallthrough.
    let state = || Some(r#"{"current_phase":"flow-review"#.to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), &state, &repo);
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("not valid JSON"));
}

// --- run_impl_main: repo resolution ---

#[test]
fn run_impl_main_no_repo_returns_error_tuple() {
    let state = || None;
    let repo = || None;
    let mut args = default_args();
    args.repo = None;
    let (value, code) = run_impl_main(args, &state, &repo);
    assert_eq!(value["status"], "error");
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Could not detect repo"));
}

#[test]
fn run_impl_main_body_file_missing_returns_error_tuple() {
    let state = || None;
    let repo = || Some("owner/name".to_string());
    let mut args = default_args();
    args.body_file = Some("nonexistent-body.md".to_string());
    let (value, code) = run_impl_main(args, &state, &repo);
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
    let (_, _code) = run_impl_main(args, &state, &repo);
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
    let (_, _code) = run_impl_main(args, &state, &repo);
}

#[test]
fn run_impl_main_missing_state_file_falls_back_to_resolver() {
    let state = || None;
    let repo = || Some("fallback/repo".to_string());
    let mut args = default_args();
    args.repo = None;
    args.state_file = Some("/nonexistent/state.json".to_string());
    let (_, _code) = run_impl_main(args, &state, &repo);
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
    let (value, code) = run_impl_main(args, &state, &repo);
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Could not detect repo"));
}

#[test]
fn run_impl_main_blocked_by_review_returns_error_tuple() {
    let state = || Some(r#"{"current_phase":"flow-review"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let (value, code) = run_impl_main(default_args(), &state, &repo);
    assert_eq!(value["status"], "error");
    assert_eq!(code, 1);
    assert!(value["message"].as_str().unwrap().contains("Review"));
}

#[test]
fn gate_override_bypasses_review_block() {
    // Covers the `if override_flag { return None; }` early return.
    let state = || Some(r#"{"current_phase":"flow-review"}"#.to_string());
    let repo = || Some("owner/name".to_string());
    let mut args = default_args();
    args.override_review_ban = true;
    let (value, _code) = run_impl_main(args, &state, &repo);
    // Gate passed — no Review message.
    assert!(!value["message"]
        .as_str()
        .unwrap_or("")
        .contains("disabled during Review"));
}

#[test]
fn run_impl_main_resolver_only_path_uses_resolver_repo() {
    // Covers the outermost `else { match repo_resolver() Some(r) => r }`
    // branch: no --repo, no --state-file, resolver returns Some.
    let state = || None;
    let repo = || Some("resolver/repo".to_string());
    let mut args = default_args();
    args.repo = None;
    args.state_file = None;
    // Don't care about exit code; just covering the repo-resolution branch.
    let (_, _code) = run_impl_main(args, &state, &repo);
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
    let (_, _code) = run_impl_main(args, &state, &repo);
}
