//! Tests for the `plan_from_issue` subcommand — the sentinel-based plan
//! extractor that replaces the heuristic `plan-extract` path.
//!
//! The contract: scan an issue body for the literal sentinel pair
//! `<!-- FLOW-PLAN-BEGIN -->` and `<!-- FLOW-PLAN-END -->`, and return
//! the bytes between verbatim. No heading promotion, no truncation
//! detection, no scanner gates — the issue is the plan, the markers
//! delimit it, end of contract.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_gh_stub, create_git_repo_with_remote, parse_output};
use flow_rs::plan_from_issue::{
    extract_plan, write_plan, ExtractError, FetchError, WriteError, PLAN_BODY_BYTE_CAP,
};

const BEGIN: &str = "<!-- FLOW-PLAN-BEGIN -->";
const END: &str = "<!-- FLOW-PLAN-END -->";

// --- extract_plan ---

#[test]
fn extract_plan_happy_path_returns_content_between_markers() {
    let body = format!(
        "Issue prelude.\n\n{}\n## Plan\n\nContent here.\n{}\n\nIssue postlude.",
        BEGIN, END
    );
    let result = extract_plan(&body).expect("extraction succeeds");
    assert!(result.contains("## Plan"));
    assert!(result.contains("Content here."));
    assert!(!result.contains("Issue prelude"));
    assert!(!result.contains("Issue postlude"));
}

#[test]
fn extract_plan_rejects_when_both_markers_missing() {
    let body = "Some issue body with no sentinels at all.";
    let err = extract_plan(body).expect_err("extraction must fail");
    assert!(matches!(err, ExtractError::MarkersMissing));
}

#[test]
fn extract_plan_rejects_when_only_begin_present() {
    let body = format!("Prelude.\n{}\n## Plan\nContent.\n", BEGIN);
    let err = extract_plan(&body).expect_err("extraction must fail");
    assert!(matches!(err, ExtractError::MarkersMalformed));
}

#[test]
fn extract_plan_rejects_when_only_end_present() {
    let body = format!("Prelude.\n## Plan\nContent.\n{}\n", END);
    let err = extract_plan(&body).expect_err("extraction must fail");
    assert!(matches!(err, ExtractError::MarkersMalformed));
}

#[test]
fn extract_plan_uses_first_begin_when_multiple_begins_present() {
    let body = format!(
        "{}\nFirst plan.\n{}\nMiddle.\n{}\nSecond plan.\n{}",
        BEGIN, END, BEGIN, END
    );
    let result = extract_plan(&body).expect("extraction succeeds");
    assert!(result.contains("First plan."));
    assert!(!result.contains("Second plan."));
}

#[test]
fn extract_plan_uses_first_end_after_begin_when_multiple_ends_present() {
    let body = format!("{}\nReal plan content.\n{}\nNoise.\n{}", BEGIN, END, END);
    let result = extract_plan(&body).expect("extraction succeeds");
    assert!(result.contains("Real plan content."));
    assert!(!result.contains("Noise."));
}

#[test]
fn extract_plan_rejects_empty_content_between_markers() {
    let body = format!("{}{}", BEGIN, END);
    let err = extract_plan(&body).expect_err("extraction must fail");
    assert!(matches!(err, ExtractError::Empty));
}

#[test]
fn extract_plan_rejects_whitespace_only_content_between_markers() {
    let body = format!("{}\n   \n\t\n{}", BEGIN, END);
    let err = extract_plan(&body).expect_err("extraction must fail");
    assert!(matches!(err, ExtractError::Empty));
}

#[test]
fn extract_plan_rejects_when_end_appears_before_begin() {
    let body = format!("{}\nbackwards\n{}", END, BEGIN);
    let err = extract_plan(&body).expect_err("extraction must fail");
    assert!(matches!(err, ExtractError::MarkersMalformed));
}

#[test]
fn extract_plan_handles_crlf_line_endings() {
    let body = format!(
        "Prelude.\r\n{}\r\n## Plan\r\n\r\nContent.\r\n{}\r\nPostlude.",
        BEGIN, END
    );
    let result = extract_plan(&body).expect("extraction succeeds");
    assert!(result.contains("## Plan"));
    assert!(result.contains("Content."));
}

#[test]
fn extract_plan_rejects_body_larger_than_byte_cap() {
    let mut body = String::with_capacity(PLAN_BODY_BYTE_CAP + 1024);
    body.push_str(BEGIN);
    body.push('\n');
    while body.len() < PLAN_BODY_BYTE_CAP + 100 {
        body.push_str("padding line that consumes bytes\n");
    }
    body.push_str(END);
    let err = extract_plan(&body).expect_err("extraction must fail");
    assert!(matches!(err, ExtractError::TooLarge));
}

#[test]
fn extract_plan_accepts_body_at_byte_cap_boundary() {
    let mut body = String::new();
    body.push_str(BEGIN);
    body.push('\n');
    body.push_str("plan content\n");
    body.push_str(END);
    assert!(body.len() <= PLAN_BODY_BYTE_CAP);
    let result = extract_plan(&body).expect("under-cap body extracts cleanly");
    assert!(result.contains("plan content"));
}

#[test]
fn extract_plan_byte_cap_constant_is_one_megabyte() {
    assert_eq!(PLAN_BODY_BYTE_CAP, 1_048_576);
}

// --- ExtractError Display ---

#[test]
fn extract_error_display_markers_missing() {
    let msg = format!("{}", ExtractError::MarkersMissing);
    assert!(msg.contains("FLOW-PLAN-BEGIN"));
    assert!(msg.contains("FLOW-PLAN-END"));
}

#[test]
fn extract_error_display_markers_malformed() {
    let msg = format!("{}", ExtractError::MarkersMalformed);
    assert!(msg.contains("FLOW-PLAN"));
    assert!(msg.contains("unmatched") || msg.contains("out-of-order"));
}

#[test]
fn extract_error_display_empty() {
    let msg = format!("{}", ExtractError::Empty);
    assert!(msg.contains("empty"));
    assert!(msg.contains("FLOW-PLAN"));
}

#[test]
fn extract_error_display_too_large() {
    let msg = format!("{}", ExtractError::TooLarge);
    assert!(msg.contains("MiB") || msg.contains("cap"));
}

// --- write_plan ---

#[test]
fn write_plan_writes_content_to_canonical_path() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let path = write_plan(&root, "feature-x", "## Plan\n\nContent.\n").unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert_eq!(written, "## Plan\n\nContent.\n");
    assert!(path.ends_with(".flow-states/feature-x/plan.md"));
}

#[test]
fn write_plan_creates_branch_subdirectory_when_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let path = write_plan(&root, "fresh", "content").unwrap();
    assert!(path.exists());
    assert!(path.parent().unwrap().exists());
}

#[test]
fn write_plan_overwrites_existing_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    write_plan(&root, "ow", "first").unwrap();
    let path = write_plan(&root, "ow", "second").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "second");
}

#[test]
fn write_plan_rejects_branch_with_slash() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let err = write_plan(&root, "feat/x", "content").unwrap_err();
    assert!(matches!(err, WriteError::InvalidBranch(_)));
}

#[test]
fn write_plan_rejects_empty_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let err = write_plan(&root, "", "content").unwrap_err();
    assert!(matches!(err, WriteError::InvalidBranch(_)));
}

#[test]
fn write_plan_rejects_dot_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let err = write_plan(&root, ".", "content").unwrap_err();
    assert!(matches!(err, WriteError::InvalidBranch(_)));
}

#[test]
fn write_plan_io_error_when_root_is_a_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    // Create a file at .flow-states so create_dir_all fails with NotADirectory.
    fs::write(root.join(".flow-states"), "blocking file").unwrap();
    let err = write_plan(&root, "blocked", "content").unwrap_err();
    assert!(matches!(err, WriteError::Io(_)));
}

#[test]
fn write_plan_io_error_when_plan_path_is_a_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    // Pre-create plan.md as a directory so fs::write fails AFTER
    // ensure_branch_dir succeeds — exercises the post-mkdir Io branch.
    fs::create_dir_all(root.join(".flow-states/dirpath/plan.md")).unwrap();
    let err = write_plan(&root, "dirpath", "content").unwrap_err();
    assert!(matches!(err, WriteError::Io(_)));
    let msg = format!("{}", err);
    assert!(msg.contains("filesystem error"));
}

// --- WriteError Display ---

#[test]
fn write_error_display_invalid_branch() {
    let msg = format!("{}", WriteError::InvalidBranch("feat/x".to_string()));
    assert!(msg.contains("invalid branch"));
    assert!(msg.contains("feat/x"));
}

#[test]
fn write_error_display_io() {
    let msg = format!("{}", WriteError::Io("disk full".to_string()));
    assert!(msg.contains("filesystem"));
    assert!(msg.contains("disk full"));
}

// --- FetchError Display ---

#[test]
fn fetch_error_display_issue_not_found() {
    let msg = format!("{}", FetchError::IssueNotFound { issue: 42 });
    assert!(msg.contains("42"));
    assert!(msg.contains("not found"));
}

#[test]
fn fetch_error_display_issue_closed() {
    let msg = format!("{}", FetchError::IssueClosed { issue: 99 });
    assert!(msg.contains("99"));
    assert!(msg.contains("closed"));
}

#[test]
fn fetch_error_display_gh_failed() {
    let msg = format!("{}", FetchError::GhFailed("auth needed".to_string()));
    assert!(msg.contains("gh"));
    assert!(msg.contains("auth needed"));
}

// --- bin/flow plan-from-issue (subprocess tests) ---

fn flow_rs_no_recursion() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd
}

fn run_plan_from_issue(repo: &Path, args: &[&str], stub_dir: &Path) -> Output {
    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    flow_rs_no_recursion()
        .arg("plan-from-issue")
        .args(args)
        .current_dir(repo)
        .env("PATH", &path_env)
        .env("HOME", repo)
        .env("GH_TOKEN", "invalid")
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

#[test]
fn plan_from_issue_happy_path_writes_plan_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // Fake gh returns a JSON body wrapping a sentinel-delimited plan.
    let body = "Prelude.\\n<!-- FLOW-PLAN-BEGIN -->\\n## Plan\\nContent.\\n<!-- FLOW-PLAN-END -->\\nPostlude.";
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\necho '{{\"body\":\"{}\",\"state\":\"OPEN\"}}'\nexit 0\n",
            body
        ),
    );

    let output = run_plan_from_issue(
        &repo,
        &["--issue", "42", "--branch", "feat-test"],
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
    assert_eq!(data["branch"], "feat-test");
    assert_eq!(data["issue"], 42);
    assert!(data["plan_path"]
        .as_str()
        .unwrap()
        .ends_with(".flow-states/feat-test/plan.md"));

    let plan_path = data["plan_path"].as_str().unwrap();
    let written = fs::read_to_string(plan_path).unwrap();
    assert!(written.contains("## Plan"));
    assert!(written.contains("Content."));
}

#[test]
fn plan_from_issue_returns_issue_not_found_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\necho 'GraphQL: Could not resolve to an Issue with the number of 999.' >&2\nexit 1\n",
    );

    let output = run_plan_from_issue(&repo, &["--issue", "999", "--branch", "feat-x"], &stub_dir);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["reason"], "issue_not_found");
    assert_eq!(data["issue"], 999);
}

#[test]
fn plan_from_issue_returns_issue_closed_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\necho '{\"body\":\"closed issue body\",\"state\":\"CLOSED\"}'\nexit 0\n",
    );

    let output = run_plan_from_issue(&repo, &["--issue", "100", "--branch", "feat-y"], &stub_dir);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["reason"], "issue_closed");
    assert_eq!(data["issue"], 100);
}

#[test]
fn plan_from_issue_returns_gh_fetch_failed_for_other_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\necho 'authentication required' >&2\nexit 1\n",
    );

    let output = run_plan_from_issue(&repo, &["--issue", "5", "--branch", "feat-z"], &stub_dir);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["reason"], "gh_fetch_failed");
}

#[test]
fn plan_from_issue_returns_plan_markers_missing_when_body_has_no_sentinels() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\necho '{\"body\":\"plain prose with no sentinels\",\"state\":\"OPEN\"}'\nexit 0\n",
    );

    let output = run_plan_from_issue(&repo, &["--issue", "1", "--branch", "f"], &stub_dir);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["reason"], "plan_markers_missing");
}

#[test]
fn plan_from_issue_returns_plan_markers_malformed_when_only_begin_present() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\necho '{\"body\":\"<!-- FLOW-PLAN-BEGIN --> content\",\"state\":\"OPEN\"}'\nexit 0\n",
    );

    let output = run_plan_from_issue(&repo, &["--issue", "2", "--branch", "fe"], &stub_dir);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["reason"], "plan_markers_malformed");
}

#[test]
fn plan_from_issue_returns_plan_empty_when_content_between_markers_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\necho '{\"body\":\"<!-- FLOW-PLAN-BEGIN -->   <!-- FLOW-PLAN-END -->\",\"state\":\"OPEN\"}'\nexit 0\n",
    );

    let output = run_plan_from_issue(&repo, &["--issue", "3", "--branch", "fee"], &stub_dir);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["reason"], "plan_empty");
}

#[test]
fn plan_from_issue_returns_invalid_branch_error_for_slash_branch() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\necho '{\"body\":\"<!-- FLOW-PLAN-BEGIN -->\\nplan\\n<!-- FLOW-PLAN-END -->\",\"state\":\"OPEN\"}'\nexit 0\n",
    );

    let output = run_plan_from_issue(
        &repo,
        &["--issue", "4", "--branch", "feat/slash"],
        &stub_dir,
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["reason"], "invalid_branch");
}

#[test]
fn plan_from_issue_gh_spawn_failure_returns_gh_fetch_failed() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // Empty PATH so gh cannot be located.
    let output = flow_rs_no_recursion()
        .args(["plan-from-issue", "--issue", "1", "--branch", "f"])
        .current_dir(&repo)
        .env("PATH", "")
        .env("HOME", &repo)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["reason"], "gh_fetch_failed");
    assert!(data["message"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("spawn"));
}

#[test]
fn plan_from_issue_returns_gh_fetch_failed_when_gh_outputs_garbage_json() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho 'not valid json at all'\nexit 0\n");

    let output = run_plan_from_issue(&repo, &["--issue", "7", "--branch", "fa"], &stub_dir);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["reason"], "gh_fetch_failed");
    assert!(data["message"].as_str().unwrap().contains("parse json"));
}

#[test]
fn plan_from_issue_returns_write_failed_when_flow_states_blocked_by_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // Pre-create a file at .flow-states/ so create_dir_all for the
    // branch subdirectory cannot succeed.
    fs::write(repo.join(".flow-states"), "blocking file").unwrap();
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\necho '{\"body\":\"<!-- FLOW-PLAN-BEGIN -->\\nplan\\n<!-- FLOW-PLAN-END -->\",\"state\":\"OPEN\"}'\nexit 0\n",
    );

    let output = run_plan_from_issue(&repo, &["--issue", "8", "--branch", "blocked"], &stub_dir);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["reason"], "write_failed");
}

#[test]
fn plan_from_issue_returns_plan_too_large_when_body_exceeds_cap() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // Build a body that exceeds PLAN_BODY_BYTE_CAP. Echo a JSON document
    // whose body field is a long quoted string between sentinels.
    let padding_chars = PLAN_BODY_BYTE_CAP + 100;
    let stub_script = format!(
        "#!/bin/bash\nprintf '{{\\\"body\\\":\\\"<!-- FLOW-PLAN-BEGIN -->%*s<!-- FLOW-PLAN-END -->\\\",\\\"state\\\":\\\"OPEN\\\"}}\\n' {} ' '\nexit 0\n",
        padding_chars
    );
    let stub_dir = create_gh_stub(&repo, &stub_script);

    let output = run_plan_from_issue(&repo, &["--issue", "9", "--branch", "huge"], &stub_dir);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["reason"], "plan_too_large");
}

// --- tasks_total in success envelope ---

#[test]
fn plan_from_issue_returns_tasks_total_when_body_has_three_task_headings() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let body = "<!-- FLOW-PLAN-BEGIN -->\\n## Plan\\n\\n#### Task 1: First\\nBody one.\\n\\n#### Task 2: Second\\nBody two.\\n\\n#### Task 3: Third\\nBody three.\\n<!-- FLOW-PLAN-END -->";
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\necho '{{\"body\":\"{}\",\"state\":\"OPEN\"}}'\nexit 0\n",
            body
        ),
    );

    let output = run_plan_from_issue(
        &repo,
        &["--issue", "11", "--branch", "feat-tasks-three"],
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
    assert_eq!(data["tasks_total"], 3);
}

#[test]
fn plan_from_issue_returns_tasks_total_zero_when_body_has_no_task_headings() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let body = "<!-- FLOW-PLAN-BEGIN -->\\n## Plan\\n\\nNo tasks here, just prose.\\n<!-- FLOW-PLAN-END -->";
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\necho '{{\"body\":\"{}\",\"state\":\"OPEN\"}}'\nexit 0\n",
            body
        ),
    );

    let output = run_plan_from_issue(
        &repo,
        &["--issue", "12", "--branch", "feat-tasks-none"],
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
    assert_eq!(data["tasks_total"], 0);
}

/// Tilde-fenced (`~~~`) example blocks must be recognized as fences so
/// task-shaped lines inside them do not inflate `tasks_total`. Plan
/// authors documenting Markdown shapes use tilde fences to avoid
/// backtick-nesting headaches.
#[test]
fn plan_from_issue_tasks_total_skips_headings_inside_tilde_fenced_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let body = "<!-- FLOW-PLAN-BEGIN -->\\n## Plan\\n\\n#### Task 1: Real\\n\\n~~~markdown\\n#### Task 99: Doc example\\n#### Task 100: Another fake\\n~~~\\n\\n#### Task 2: Another real\\n<!-- FLOW-PLAN-END -->";
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\necho '{{\"body\":\"{}\",\"state\":\"OPEN\"}}'\nexit 0\n",
            body
        ),
    );

    let output = run_plan_from_issue(
        &repo,
        &["--issue", "13", "--branch", "feat-tilde"],
        &stub_dir,
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["tasks_total"], 2);
}

/// A prose paragraph wrapped onto a line beginning with three
/// backticks (followed by inline-code-span markers) must NOT
/// open a fence. Real task headings on subsequent lines must still
/// count.
#[test]
fn plan_from_issue_tasks_total_does_not_treat_inline_code_span_as_fence() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let body = "<!-- FLOW-PLAN-BEGIN -->\\n## Plan\\n\\nThe decompose step uses\\n```inline-token```-style markers to disambiguate.\\n\\n#### Task 1: First\\n#### Task 2: Second\\n<!-- FLOW-PLAN-END -->";
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\necho '{{\"body\":\"{}\",\"state\":\"OPEN\"}}'\nexit 0\n",
            body
        ),
    );

    let output = run_plan_from_issue(
        &repo,
        &["--issue", "14", "--branch", "feat-prose-tick"],
        &stub_dir,
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["tasks_total"], 2);
}

/// A UTF-8 BOM at the start of the extracted plan body must not
/// hide the first task heading. BOM-emitting editors prepend the
/// `\u{FEFF}` sequence; the count must still match the visible
/// task structure.
#[test]
fn plan_from_issue_tasks_total_handles_utf8_bom_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // ﻿ is the JSON escape for the UTF-8 BOM, which serde_json
    // decodes into the body string. The BOM lands immediately after
    // the FLOW-PLAN-BEGIN sentinel and the newline, becoming the
    // first byte of the extracted plan content.
    let body = "<!-- FLOW-PLAN-BEGIN -->\\n\\uFEFF#### Task 1: First\\n#### Task 2: Second\\n<!-- FLOW-PLAN-END -->";
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\necho '{{\"body\":\"{}\",\"state\":\"OPEN\"}}'\nexit 0\n",
            body
        ),
    );

    let output = run_plan_from_issue(&repo, &["--issue", "15", "--branch", "feat-bom"], &stub_dir);

    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["tasks_total"], 2);
}

/// Quad-backtick (` ```` `) fences enclose nested triple-backtick
/// fences; the outer fence's closer requires at least 4 backticks.
/// Task headings inside the outer fence must not count, even when
/// a 3-backtick line appears mid-block.
#[test]
fn plan_from_issue_tasks_total_handles_nested_triple_backtick_inside_quad_fence() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let body = "<!-- FLOW-PLAN-BEGIN -->\\n## Plan\\n\\n#### Task 1: Real\\n\\n````markdown\\n```\\n#### Task 99: Inside nested fence\\n```\\n````\\n\\n#### Task 2: Another real\\n<!-- FLOW-PLAN-END -->";
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\necho '{{\"body\":\"{}\",\"state\":\"OPEN\"}}'\nexit 0\n",
            body
        ),
    );

    let output = run_plan_from_issue(
        &repo,
        &["--issue", "16", "--branch", "feat-nested"],
        &stub_dir,
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["tasks_total"], 2);
}

/// Triple-backtick fences with no info string still open and close
/// correctly; task-shaped lines inside them must not count. Locks
/// in the baseline fence-handling contract that the adversarial
/// regression suite extends.
#[test]
fn plan_from_issue_tasks_total_skips_headings_inside_plain_triple_backtick_fence() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let body = "<!-- FLOW-PLAN-BEGIN -->\\n## Plan\\n\\n#### Task 1: Real\\n\\n```\\n#### Task 99: Inside fence\\n```\\n\\n#### Task 2: Another real\\n<!-- FLOW-PLAN-END -->";
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\necho '{{\"body\":\"{}\",\"state\":\"OPEN\"}}'\nexit 0\n",
            body
        ),
    );

    let output = run_plan_from_issue(
        &repo,
        &["--issue", "17", "--branch", "feat-plain-fence"],
        &stub_dir,
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["tasks_total"], 2);
}

/// Headings at non-`####` depths must not match. Locks in the
/// `#### Task ` prefix discipline so a future loosening of the
/// scanner cannot silently match `### Task 1` or `##### Task 2`.
#[test]
fn plan_from_issue_tasks_total_rejects_other_heading_depths() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let body = "<!-- FLOW-PLAN-BEGIN -->\\n## Plan\\n\\n### Task 1: Too shallow\\n##### Task 2: Too deep\\n#### Task 3: Just right\\n<!-- FLOW-PLAN-END -->";
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\necho '{{\"body\":\"{}\",\"state\":\"OPEN\"}}'\nexit 0\n",
            body
        ),
    );

    let output = run_plan_from_issue(
        &repo,
        &["--issue", "18", "--branch", "feat-depth"],
        &stub_dir,
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["tasks_total"], 1);
}

/// Lines that begin with one or two fence characters (e.g., a
/// line opening with a single backtick for an inline code span)
/// must not be treated as fences. A 3+ run is required by
/// CommonMark §4.5; below that, the line falls through to the
/// task-heading check.
#[test]
fn plan_from_issue_tasks_total_ignores_lines_with_fewer_than_three_fence_chars() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let body = "<!-- FLOW-PLAN-BEGIN -->\\n## Plan\\n\\n`solo` backtick prose\\n``two-tick`` prose\\n~ single tilde\\n#### Task 1: Real\\n#### Task 2: Real\\n<!-- FLOW-PLAN-END -->";
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\necho '{{\"body\":\"{}\",\"state\":\"OPEN\"}}'\nexit 0\n",
            body
        ),
    );

    let output = run_plan_from_issue(
        &repo,
        &["--issue", "20", "--branch", "feat-short-tick"],
        &stub_dir,
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["tasks_total"], 2);
}

/// A line beginning with 3+ fence chars but followed by non-
/// whitespace content is NOT a valid closer (CommonMark §4.5).
/// The fence stays open, so task headings between the fake-closer
/// and the real closer are correctly skipped.
#[test]
fn plan_from_issue_tasks_total_fence_stays_open_when_closer_has_trailing_content() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let body = "<!-- FLOW-PLAN-BEGIN -->\\n#### Task 1: Real\\n\\n```\\n#### Task 99: Inside fence\\n``` not-a-closer\\n#### Task 100: Still inside fence\\n```\\n\\n#### Task 2: Real\\n<!-- FLOW-PLAN-END -->";
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\necho '{{\"body\":\"{}\",\"state\":\"OPEN\"}}'\nexit 0\n",
            body
        ),
    );

    let output = run_plan_from_issue(
        &repo,
        &["--issue", "21", "--branch", "feat-bad-closer"],
        &stub_dir,
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["tasks_total"], 2);
}

/// `#### Task ` followed by non-digit must not count. Locks in
/// the digit-required discipline that distinguishes a real task
/// heading from a prose heading like `#### Task summary`.
#[test]
fn plan_from_issue_tasks_total_requires_digit_after_task_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let body = "<!-- FLOW-PLAN-BEGIN -->\\n#### Task summary: prose-only heading\\n#### Task : missing number\\n#### Task 1: real\\n<!-- FLOW-PLAN-END -->";
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\necho '{{\"body\":\"{}\",\"state\":\"OPEN\"}}'\nexit 0\n",
            body
        ),
    );

    let output = run_plan_from_issue(
        &repo,
        &["--issue", "19", "--branch", "feat-digit"],
        &stub_dir,
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["tasks_total"], 1);
}

// --- files.plan population ---

/// After `write_plan` succeeds, `run_impl_main` records the relative
/// plan path in the state file's `files.plan` so downstream consumers
/// (phase-enter, render_pr_body, tui_data, plan_deviation) read the
/// pointer without recomputing it. The test seeds an existing
/// `.flow-states/<branch>/state.json` (run_impl_main does not create
/// it — the real flow's init-state Step 3 already did), runs the
/// subprocess through a fake gh returning a sentinel-delimited body,
/// and asserts the concrete relative path lands in files.plan.
#[test]
fn plan_from_issue_populates_files_plan() {
    // Plan fixture: key "expected" carries value
    // ".flow-states/<branch>/plan.md" (the branch-templated relative
    // path the run writes into files.plan).
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let branch = "feat-files-plan";
    // Seed an existing state file — run_impl_main mutates it, never
    // creates it.
    let state_dir = repo.join(".flow-states").join(branch);
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("state.json"), "{\"files\":{}}").unwrap();

    let body = "<!-- FLOW-PLAN-BEGIN -->\\n## Plan\\nContent.\\n<!-- FLOW-PLAN-END -->";
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\necho '{{\"body\":\"{}\",\"state\":\"OPEN\"}}'\nexit 0\n",
            body
        ),
    );

    let output = run_plan_from_issue(&repo, &["--issue", "30", "--branch", branch], &stub_dir);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");

    let state_content = fs::read_to_string(state_dir.join("state.json")).unwrap();
    let state: serde_json::Value = serde_json::from_str(&state_content).unwrap();
    let expected = format!(".flow-states/{}/plan.md", branch);
    assert_eq!(state["files"]["plan"].as_str().unwrap(), expected);
}

/// Regression (Review adversarial + reviewer): a hand-edited or
/// corrupted state file whose `files` field is a non-object value (a
/// string here; a JSON array hits the identical reset branch) must NOT
/// panic the best-effort `files.plan` write. The per-level object guard
/// in run_impl_main resets a wrong-type `files` to an empty object
/// before the nested `IndexMut` assignment, so the run still exits 0
/// (no serde_json IndexMut-on-non-object panic) with files.plan
/// populated. Covers the `if !state["files"].is_object()` reset branch.
#[test]
fn plan_from_issue_nonobject_files_does_not_panic_and_sets_files_plan() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let branch = "feat-files-string";
    let state_dir = repo.join(".flow-states").join(branch);
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("state.json"), "{\"files\":\"corrupted\"}").unwrap();

    let body = "<!-- FLOW-PLAN-BEGIN -->\\n## Plan\\nContent.\\n<!-- FLOW-PLAN-END -->";
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\necho '{{\"body\":\"{}\",\"state\":\"OPEN\"}}'\nexit 0\n",
            body
        ),
    );

    let output = run_plan_from_issue(&repo, &["--issue", "31", "--branch", branch], &stub_dir);

    assert_eq!(
        output.status.code(),
        Some(0),
        "subprocess must exit 0 (no panic) even when files is a non-object string; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");

    let state_content = fs::read_to_string(state_dir.join("state.json")).unwrap();
    let state: serde_json::Value = serde_json::from_str(&state_content).unwrap();
    let expected = format!(".flow-states/{}/plan.md", branch);
    assert_eq!(state["files"]["plan"].as_str().unwrap(), expected);
}

/// Regression (Review adversarial + reviewer): a state file whose ROOT
/// is a non-object value (a JSON array here) must NOT panic the
/// best-effort `files.plan` write. The root guard
/// (`if !(state.is_object() || state.is_null())`) skips the write
/// entirely, so the run still exits 0 and the array root is left
/// untouched. Covers the root-guard early-return branch.
#[test]
fn plan_from_issue_nonobject_root_skips_files_plan_without_panic() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let branch = "feat-array-root";
    let state_dir = repo.join(".flow-states").join(branch);
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("state.json"), "[1,2,3]").unwrap();

    let body = "<!-- FLOW-PLAN-BEGIN -->\\n## Plan\\nContent.\\n<!-- FLOW-PLAN-END -->";
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\necho '{{\"body\":\"{}\",\"state\":\"OPEN\"}}'\nexit 0\n",
            body
        ),
    );

    let output = run_plan_from_issue(&repo, &["--issue", "32", "--branch", branch], &stub_dir);

    assert_eq!(
        output.status.code(),
        Some(0),
        "subprocess must exit 0 (no panic) even when the state root is an array; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");

    // The root guard skipped the write; the array root is preserved
    // and carries no `files` key.
    let state_content = fs::read_to_string(state_dir.join("state.json")).unwrap();
    let state: serde_json::Value = serde_json::from_str(&state_content).unwrap();
    assert!(state.is_array());
    assert!(state.get("files").is_none());
}

#[test]
fn plan_from_issue_rejects_oversized_gh_stdout_before_parse() {
    // Tenant 4 (Correctness): Review pre-mortem flagged that an
    // adversarial gh response could grow the parsed `Value` allocation
    // unbounded. fetch_issue_body now checks raw stdout length against
    // GH_STDOUT_BYTE_CAP before invoking serde_json::from_str. Stub gh
    // emits a stdout payload exceeding the cap; the runner must reject
    // with reason="gh_fetch_failed" and a message naming the cap.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // Emit a syntactically-valid but oversized JSON envelope. The body
    // field carries enough padding that the raw stdout length exceeds
    // GH_STDOUT_BYTE_CAP (= PLAN_BODY_BYTE_CAP + 64 KiB). The cap fires
    // before the JSON parse, so the response need not be valid JSON.
    let padding = PLAN_BODY_BYTE_CAP + 70_000;
    let stub_script = format!(
        "#!/bin/bash\nprintf '{{\\\"body\\\":\\\"%*s\\\",\\\"state\\\":\\\"OPEN\\\"}}\\n' {} ' '\nexit 0\n",
        padding
    );
    let stub_dir = create_gh_stub(&repo, &stub_script);

    let output = run_plan_from_issue(&repo, &["--issue", "10", "--branch", "ovr"], &stub_dir);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["reason"], "gh_fetch_failed");
    assert!(data["message"]
        .as_str()
        .unwrap()
        .contains("gh stdout exceeds"));
}
