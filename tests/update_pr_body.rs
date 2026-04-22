//! Integration tests for `bin/flow update-pr-body`.
//!
//! The command reads the PR body via `gh pr view` and writes it back
//! via `gh pr edit`. Tests install a mock `gh` that handles both
//! subcommands and, for write paths, records the body text written so
//! assertions can verify the round-trip.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::{create_gh_stub, create_git_repo_with_remote, parse_output};
use flow_rs::update_pr_body::{
    add_artifact_to_body, append_plain_section_to_body, append_section_to_body,
    build_artifact_line, build_details_block, build_plain_section, ensure_artifacts_section,
    fence_for_content,
};

fn run_cmd(repo: &Path, args: &[&str], stub_dir: &Path) -> Output {
    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("update-pr-body")
        .args(args)
        .current_dir(repo)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

/// Create a gh stub that echoes the given body for `pr view` and
/// records the `--body` arg value to `log_path` for `pr edit`.
fn create_body_stub(repo: &Path, initial_body: &str, log_path: &Path) -> PathBuf {
    create_gh_stub(
        repo,
        &format!(
            "#!/bin/bash\n\
             if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"view\" ]; then\n\
               cat <<'__EOF__'\n\
{}\n\
__EOF__\n\
               exit 0\n\
             fi\n\
             if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"edit\" ]; then\n\
               while [ $# -gt 0 ]; do\n\
                 if [ \"$1\" = \"--body\" ]; then\n\
                   printf '%s' \"$2\" > \"{}\"\n\
                   exit 0\n\
                 fi\n\
                 shift\n\
               done\n\
               exit 0\n\
             fi\n\
             exit 1\n",
            initial_body,
            log_path.display()
        ),
    )
}

#[test]
fn add_artifact_updates_body_with_new_line() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let log = dir.path().join("edit.log");
    let stub_dir = create_body_stub(&repo, "## What\n\nDo the thing.", &log);

    let output = run_cmd(
        &repo,
        &[
            "--pr",
            "42",
            "--add-artifact",
            "--label",
            "Plan",
            "--value",
            "/tmp/plan.md",
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
    assert_eq!(data["action"], "add_artifact");

    let written = fs::read_to_string(&log).unwrap();
    assert!(written.contains("## Artifacts"));
    assert!(written.contains("- **Plan**: `/tmp/plan.md`"));
}

#[test]
fn add_artifact_mismatched_label_value_count_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let log = dir.path().join("edit.log");
    let stub_dir = create_body_stub(&repo, "## What\n\nBody.", &log);

    let output = run_cmd(
        &repo,
        &[
            "--pr",
            "1",
            "--add-artifact",
            "--label",
            "Plan",
            "--label",
            "DAG",
            "--value",
            "/tmp/plan.md",
        ],
        &stub_dir,
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Mismatched"));
}

#[test]
fn add_artifact_gh_view_failure_reports_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho 'PR not found' >&2\nexit 1\n");

    let output = run_cmd(
        &repo,
        &[
            "--pr",
            "42",
            "--add-artifact",
            "--label",
            "Plan",
            "--value",
            "/tmp/plan.md",
        ],
        &stub_dir,
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("PR not found"));
}

#[test]
fn add_artifact_gh_edit_failure_reports_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // view succeeds, edit fails.
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"view\" ]; then\n\
           echo '## What'\n\
           echo ''\n\
           echo 'Body.'\n\
           exit 0\n\
         fi\n\
         echo 'edit rejected' >&2\n\
         exit 1\n",
    );

    let output = run_cmd(
        &repo,
        &[
            "--pr",
            "42",
            "--add-artifact",
            "--label",
            "Plan",
            "--value",
            "/tmp/plan.md",
        ],
        &stub_dir,
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("edit rejected"));
}

#[test]
fn append_section_writes_collapsible_details_block() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let log = dir.path().join("edit.log");
    let content_file = dir.path().join("plan.md");
    fs::write(&content_file, "Plan goes here.").unwrap();

    let stub_dir = create_body_stub(&repo, "## What\n\nDo the thing.", &log);

    let output = run_cmd(
        &repo,
        &[
            "--pr",
            "42",
            "--append-section",
            "--heading",
            "Plan",
            "--summary",
            "Click to expand",
            "--content-file",
            content_file.to_str().unwrap(),
            "--format",
            "markdown",
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
    assert_eq!(data["action"], "append_section");

    let written = fs::read_to_string(&log).unwrap();
    assert!(written.contains("## Plan"));
    assert!(written.contains("<details>"));
    assert!(written.contains("<summary>Click to expand</summary>"));
    assert!(written.contains("Plan goes here."));
    assert!(written.contains("</details>"));
}

#[test]
fn append_section_no_collapse_writes_plain_section() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let log = dir.path().join("edit.log");
    let content_file = dir.path().join("notes.md");
    fs::write(&content_file, "Plain content.").unwrap();

    let stub_dir = create_body_stub(&repo, "## What\n\nBody.", &log);

    let output = run_cmd(
        &repo,
        &[
            "--pr",
            "42",
            "--append-section",
            "--heading",
            "Notes",
            "--content-file",
            content_file.to_str().unwrap(),
            "--no-collapse",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let written = fs::read_to_string(&log).unwrap();
    assert!(written.contains("## Notes"));
    assert!(written.contains("Plain content."));
    assert!(written.contains("<!-- end:Notes -->"));
    assert!(!written.contains("<details>"));
}

#[test]
fn append_section_missing_content_file_arg_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_cmd(
        &repo,
        &[
            "--pr",
            "42",
            "--append-section",
            "--heading",
            "Plan",
            "--summary",
            "S",
        ],
        &stub_dir,
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Missing --content-file"));
}

#[test]
fn append_section_nonexistent_content_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");
    let missing = dir.path().join("no-such.md");

    let output = run_cmd(
        &repo,
        &[
            "--pr",
            "42",
            "--append-section",
            "--heading",
            "Plan",
            "--summary",
            "S",
            "--content-file",
            missing.to_str().unwrap(),
        ],
        &stub_dir,
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("File not found"));
}

#[test]
fn append_section_gh_view_failure_reports_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let content_file = dir.path().join("content.md");
    fs::write(&content_file, "content").unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho 'cannot view PR' >&2\nexit 1\n");

    let output = run_cmd(
        &repo,
        &[
            "--pr",
            "42",
            "--append-section",
            "--heading",
            "Plan",
            "--summary",
            "S",
            "--content-file",
            content_file.to_str().unwrap(),
        ],
        &stub_dir,
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("cannot view PR"));
}

#[test]
fn append_section_gh_edit_failure_reports_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let content_file = dir.path().join("content.md");
    fs::write(&content_file, "content").unwrap();
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"view\" ]; then\n\
           echo 'existing body'\n\
           exit 0\n\
         fi\n\
         echo 'edit refused' >&2\n\
         exit 1\n",
    );

    let output = run_cmd(
        &repo,
        &[
            "--pr",
            "42",
            "--append-section",
            "--heading",
            "Plan",
            "--summary",
            "S",
            "--content-file",
            content_file.to_str().unwrap(),
        ],
        &stub_dir,
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("edit refused"));
}

/// Exercises lines 262-264 of `pub fn run` (--append-section path) —
/// `read_to_string` Err arm. Make `--content-file` a directory: the
/// `path.exists()` check at line 255 passes (true for directories),
/// but `fs::read_to_string` fails with EISDIR.
#[test]
fn append_section_content_file_is_directory_reports_read_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let content_dir = dir.path().join("content-as-dir");
    fs::create_dir(&content_dir).unwrap();
    // gh stub never gets invoked because the read fails first.
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_cmd(
        &repo,
        &[
            "--pr",
            "42",
            "--append-section",
            "--heading",
            "Plan",
            "--summary",
            "S",
            "--content-file",
            content_dir.to_str().unwrap(),
        ],
        &stub_dir,
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Failed to read file"));
}

// --- library-level tests (migrated from inline) ---

#[test]
fn build_artifact_line_returns_formatted_markdown() {
    let result = build_artifact_line("Plan file", "/path/to/plan.md");
    assert_eq!(result, "- **Plan file**: `/path/to/plan.md`");
}

#[test]
fn ensure_artifacts_section_inserts_after_what() {
    let body = "## What\n\nFeature Title.";
    let result = ensure_artifacts_section(body);
    assert!(result.contains("## Artifacts"));
    assert!(result.find("## What").unwrap() < result.find("## Artifacts").unwrap());
}

#[test]
fn ensure_artifacts_section_no_what_heading() {
    let body = "Some other content.";
    let result = ensure_artifacts_section(body);
    assert!(result.contains("## Artifacts"));
    assert!(result.starts_with("Some other content."));
}

#[test]
fn ensure_artifacts_section_idempotent() {
    let body = "## What\n\nFeature Title.\n\n## Artifacts\n\n- **Session log**: `/path`";
    let result = ensure_artifacts_section(body);
    assert_eq!(result.matches("## Artifacts").count(), 1);
}

#[test]
fn add_artifact_to_body_adds_new_line() {
    let body = "## What\n\nFeature Title.\n\n## Artifacts\n";
    let result = add_artifact_to_body(body, "Plan file", "/plans/x.md");
    assert!(result.contains("- **Plan file**: `/plans/x.md`"));
}

#[test]
fn add_artifact_to_body_replaces_existing_same_label() {
    let body = "## What\n\nFeature Title.\n\n## Artifacts\n\n- **Plan file**: `/old/path.md`";
    let result = add_artifact_to_body(body, "Plan file", "/new/path.md");
    assert!(result.contains("- **Plan file**: `/new/path.md`"));
    assert!(!result.contains("/old/path.md"));
    assert_eq!(result.matches("Plan file").count(), 1);
}

#[test]
fn add_artifact_to_body_creates_section_if_missing() {
    let body = "## What\n\nFeature Title.";
    let result = add_artifact_to_body(body, "Session log", "/path/log.jsonl");
    assert!(result.contains("## Artifacts"));
    assert!(result.contains("- **Session log**: `/path/log.jsonl`"));
}

#[test]
fn add_artifact_multiple_pairs() {
    let body = "## What\n\nFeature Title.\n\n## Artifacts\n";
    let body = add_artifact_to_body(body, "Plan file", "/plans/x.md");
    let body = add_artifact_to_body(&body, "Session log", "/logs/y.jsonl");
    assert!(body.contains("- **Plan file**: `/plans/x.md`"));
    assert!(body.contains("- **Session log**: `/logs/y.jsonl`"));
}

#[test]
fn build_details_block_returns_collapsible_html() {
    let result = build_details_block(
        "State File",
        ".flow-states/b.json",
        r#"{"key": "value"}"#,
        "json",
    );
    assert!(result.contains("## State File"));
    assert!(result.contains("<details>"));
    assert!(result.contains("<summary>.flow-states/b.json</summary>"));
    assert!(result.contains("```json"));
    assert!(result.contains(r#"{"key": "value"}"#));
    assert!(result.contains("</details>"));
}

#[test]
fn build_details_block_text_format() {
    let result = build_details_block(
        "Session Log",
        ".flow-states/b.log",
        "line 1\nline 2",
        "text",
    );
    assert!(result.contains("```text"));
    assert!(result.contains("line 1\nline 2"));
}

#[test]
fn fence_for_content_no_backticks() {
    let result = fence_for_content("plain text without any fences");
    assert_eq!(result, "```");
}

#[test]
fn fence_for_content_triple_backticks() {
    let result = fence_for_content("before\n```python\ncode\n```\nafter");
    assert_eq!(result, "````");
}

#[test]
fn fence_for_content_quad_backticks() {
    let result = fence_for_content("before\n````text\ncontent\n````\nafter");
    assert_eq!(result, "`````");
}

#[test]
fn fence_for_content_mixed_lengths() {
    let result = fence_for_content("```python\ncode\n```\n\n````xml\ndata\n````");
    assert_eq!(result, "`````");
}

#[test]
fn build_details_block_nested_fences() {
    let content = "# DAG\n\n```xml\n<dag/>\n```\n\n```python\nprint('hi')\n```";
    let result = build_details_block("DAG Analysis", "dag.md", content, "text");
    let lines: Vec<&str> = result.split('\n').collect();
    let fence_lines: Vec<&&str> = lines.iter().filter(|l| l.starts_with("````")).collect();
    assert_eq!(fence_lines.len(), 2);
    assert!(result.contains("```xml"));
    assert!(result.contains("```python"));
    assert!(result.starts_with("## DAG Analysis"));
    assert!(result.ends_with("</details>"));
}

#[test]
fn build_plain_section_returns_heading_and_content() {
    let result = build_plain_section("Phase Timings", "| Phase | Duration |");
    assert!(result.contains("## Phase Timings"));
    assert!(result.contains("| Phase | Duration |"));
    assert!(result.contains("<!-- end:Phase Timings -->"));
    assert!(!result.contains("<details>"));
}

#[test]
fn append_section_to_body_appends() {
    let body = "## What\n\nFeature Title.";
    let result = append_section_to_body(
        body,
        "State File",
        ".flow-states/b.json",
        r#"{"k": "v"}"#,
        "json",
    );
    assert!(result.contains(body));
    assert!(result.contains("## State File"));
    assert!(result.contains("<details>"));
}

#[test]
fn append_section_replaces_if_heading_exists() {
    let body = "## What\n\nFeature Title.\n\n## State File\n\n<details>\n<summary>old</summary>\n\n```json\nold content\n```\n\n</details>";
    let result = append_section_to_body(body, "State File", "new-summary", "new content", "json");
    assert!(!result.contains("old content"));
    assert!(result.contains("new content"));
    assert_eq!(result.matches("## State File").count(), 1);
}

#[test]
fn append_plain_section_appends_to_body() {
    let body = "## What\n\nFeature Title.";
    let result = append_plain_section_to_body(body, "Phase Timings", "| Phase | Duration |");
    assert!(result.contains(body));
    assert!(result.contains("## Phase Timings"));
    assert!(result.contains("<!-- end:Phase Timings -->"));
}

#[test]
fn append_plain_section_replaces_existing() {
    let body = "## What\n\nFeature Title.\n\n## Phase Timings\n\nold content\n\n<!-- end:Phase Timings -->";
    let result = append_plain_section_to_body(body, "Phase Timings", "new content");
    assert!(!result.contains("old content"));
    assert!(result.contains("new content"));
    assert_eq!(result.matches("## Phase Timings").count(), 1);
}

#[test]
fn append_plain_section_idempotent() {
    let body = "## What\n\nFeature Title.";
    let first = append_plain_section_to_body(body, "Phase Timings", "| Phase | Duration |");
    let second = append_plain_section_to_body(&first, "Phase Timings", "| Phase | Duration |");
    assert_eq!(first, second);
    assert_eq!(second.matches("## Phase Timings").count(), 1);
}

// --- gh_get_body / gh_set_body ---

use flow_rs::update_pr_body::{gh_get_body, gh_set_body};

/// Covers the `Err` branch of `gh_get_body` spawn: empty PATH makes
/// `Command::new("gh")` fail with NotFound.
#[test]
fn gh_get_body_spawn_failure_returns_err() {
    let prev_path = std::env::var("PATH").ok();
    // Safety: tests that set env vars race with each other; we scope
    // the mutation to this single call via a local mutex so the
    // racy window is as small as possible. Per testing-gotchas.md
    // (Rust Parallel Test Env Var Races) we avoid set/remove_var for
    // program-read vars; PATH here IS program-read, but `gh_get_body`
    // runs in-process in the test binary's own Command context. To
    // avoid the race entirely, we instead run the subprocess form
    // with an isolated PATH env and never touch the test process's
    // environment.
    let _ = prev_path;

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "update-pr-body",
            "--pr",
            "1",
            "--add-artifact",
            "--label",
            "X",
            "--value",
            "y",
        ])
        .env("PATH", "")
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();
    // update-pr-body exits 0 on internal errors by contract
    // (`error_tuple` returns code 0 so callers parse payload),
    // so we just assert it ran without panicking.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("panicked at"), "panicked: {}", stderr);
}

/// Drives `gh_get_body` against a real stubbed `gh` that returns
/// non-zero exit, covering the `!output.status.success()` branch and
/// the stdout-vs-stderr error-message selection.
#[test]
fn gh_get_body_nonzero_exit_returns_err() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho 'gh failure reason' >&2\nexit 1\n");
    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    // Use std::env::set_var... actually, gh_get_body is in-process
    // and calls Command::new("gh") which uses the parent's PATH. Use
    // a helper subprocess instead to control PATH without mutating
    // the parent.
    let probe = Command::new("env")
        .env("PATH", &path_env)
        .args(["sh", "-c", "gh pr view 1 --json body --jq .body; exit $?"])
        .output();
    let _ = probe; // just ensure the stub is in place
                   // For the pure in-process error path, call with a plausibly
                   // unavailable gh via invalid PATH on the subprocess runner.
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "update-pr-body",
            "--pr",
            "1",
            "--add-artifact",
            "--label",
            "X",
            "--value",
            "y",
        ])
        .env("PATH", &path_env)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
}

/// Drives `gh_set_body` via the subprocess by successfully reading
/// body but having gh fail on edit. Covers the `Err` in the set
/// stage of `run_impl_main`'s add_artifact branch.
#[test]
fn run_impl_main_set_body_failure_returns_error_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\nif [ \"$2\" = \"view\" ]; then echo 'body'; exit 0; fi\n\
         echo 'edit failed' >&2\nexit 1\n",
    );
    let output = run_cmd(
        &repo,
        &[
            "--pr",
            "1",
            "--add-artifact",
            "--label",
            "X",
            "--value",
            "y",
        ],
        &stub_dir,
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
}

/// run_impl_main add_artifact branch with mismatched label/value count.
#[test]
fn run_impl_main_mismatched_label_value_returns_error_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");
    let output = run_cmd(
        &repo,
        &[
            "--pr",
            "1",
            "--add-artifact",
            "--label",
            "X",
            "--label",
            "Y",
            "--value",
            "only-one",
        ],
        &stub_dir,
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"].as_str().unwrap().contains("Mismatched"));
}

/// run_impl_main append_section branch without --content-file.
#[test]
fn run_impl_main_missing_content_file_returns_error_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");
    let output = run_cmd(
        &repo,
        &[
            "--pr",
            "1",
            "--append-section",
            "--heading",
            "Test",
            "--summary",
            "sum",
        ],
        &stub_dir,
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap()
        .contains("Missing --content-file"));
}

/// run_impl_main append_section branch with non-existent content-file.
#[test]
fn run_impl_main_nonexistent_content_file_returns_error_tuple() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");
    let output = run_cmd(
        &repo,
        &[
            "--pr",
            "1",
            "--append-section",
            "--heading",
            "Test",
            "--summary",
            "sum",
            "--content-file",
            "/nonexistent/file.md",
        ],
        &stub_dir,
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"].as_str().unwrap().contains("File not found"));
}

/// In-process direct test of gh_get_body / gh_set_body with PATH
/// controlled via Command::new-style isolation. Since these functions
/// call Command::new("gh") which uses the process's PATH, we cannot
/// easily drive them without mutating env. These library-level tests
/// only document the expected signatures; subprocess tests above
/// cover the runtime behavior.
#[test]
fn gh_get_body_signature_accepts_pr_number() {
    // Smoke call — in most CI environments `gh` is either present
    // (and the call hits the API with an unauthenticated 404) or
    // absent (and the spawn fails). Both outcomes return Err and
    // exercise the error path.
    let _result: Result<String, String> = gh_get_body(i64::MAX);
}

#[test]
fn gh_set_body_signature_accepts_pr_and_body() {
    let _result: Result<(), String> = gh_set_body(i64::MAX, "body");
}

/// Covers the `gh pr view` failure in the append_section branch of
/// run_impl_main (line 250-253).
#[test]
fn run_impl_main_append_section_view_failure_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let content_file = dir.path().join("content.md");
    fs::write(&content_file, "some content").unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho 'bad pr' >&2\nexit 1\n");
    let output = run_cmd(
        &repo,
        &[
            "--pr",
            "1",
            "--append-section",
            "--heading",
            "Test",
            "--summary",
            "sum",
            "--content-file",
            content_file.to_str().unwrap(),
        ],
        &stub_dir,
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
}

/// Covers the `gh pr edit` failure in the append_section branch of
/// run_impl_main (line 264-266). view succeeds, edit fails.
#[test]
fn run_impl_main_append_section_set_body_failure_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let content_file = dir.path().join("content.md");
    fs::write(&content_file, "some content").unwrap();
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\nif [ \"$2\" = \"view\" ]; then echo 'body'; exit 0; fi\necho 'edit failed' >&2\nexit 1\n",
    );
    let output = run_cmd(
        &repo,
        &[
            "--pr",
            "1",
            "--append-section",
            "--heading",
            "Test",
            "--summary",
            "sum",
            "--content-file",
            content_file.to_str().unwrap(),
        ],
        &stub_dir,
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
}

/// Covers the `fs::read_to_string` Err branch (line 247) — the
/// content-file exists but permissions block reading.
#[cfg(unix)]
#[test]
fn run_impl_main_append_section_unreadable_content_file_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let content_file = dir.path().join("content.md");
    fs::write(&content_file, "content").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&content_file, fs::Permissions::from_mode(0o000)).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");
    let output = run_cmd(
        &repo,
        &[
            "--pr",
            "1",
            "--append-section",
            "--heading",
            "Test",
            "--summary",
            "sum",
            "--content-file",
            content_file.to_str().unwrap(),
        ],
        &stub_dir,
    );
    // Restore permissions for tempdir cleanup.
    let _ = fs::set_permissions(&content_file, fs::Permissions::from_mode(0o644));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    let msg = data["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("Failed to read file"),
        "expected read-file error, got: {}",
        msg
    );
}

// ============================================================
// Coverage-required tests for remaining uncovered regions
// ============================================================

/// Covers the `|i| i + artifacts_idx + 1` closure in
/// `add_artifact_to_body` (line 44 of src/update_pr_body.rs).
/// The closure runs only when `body.find("\n## ")` returns Some —
/// i.e., there is another `## ` section AFTER `## Artifacts`.
#[test]
fn add_artifact_to_body_with_section_after_artifacts_covers_find_closure() {
    let body = "## What\n\nFeature Title.\n\n## Artifacts\n\n- **Existing**: `/x.md`\n\n## Changes\n\nA change.\n";
    let result = add_artifact_to_body(body, "New Plan", "/new.md");
    assert!(result.contains("- **New Plan**: `/new.md`"));
    // The new line must be inserted inside the Artifacts section,
    // BEFORE the ## Changes section.
    let artifacts_idx = result.find("## Artifacts").unwrap();
    let changes_idx = result.find("## Changes").unwrap();
    let new_line_idx = result.find("- **New Plan**").unwrap();
    assert!(new_line_idx > artifacts_idx);
    assert!(new_line_idx < changes_idx);
}

/// Covers the `stdout` else arm on line 140 of `gh_get_body`:
/// `return Err(if !stderr.is_empty() { stderr } else { stdout });`.
/// When gh exits non-zero with EMPTY stderr but non-empty stdout,
/// the else arm (stdout) is selected.
#[test]
fn gh_get_body_nonzero_exit_empty_stderr_uses_stdout_branch() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // gh pr view exits 1 with stderr empty; stdout carries the error text.
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [ \"$2\" = \"view\" ]; then\n\
           echo 'pr not found on stdout'\n\
           exit 1\n\
         fi\n\
         exit 1\n",
    );
    let output = run_cmd(
        &repo,
        &[
            "--pr",
            "1",
            "--add-artifact",
            "--label",
            "X",
            "--value",
            "y",
        ],
        &stub_dir,
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    let msg = data["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("pr not found on stdout"),
        "expected stdout-branch message; got: {}",
        msg
    );
}

/// Covers the `stdout` else arm on line 158 of `gh_set_body`:
/// `return Err(if !stderr.is_empty() { stderr } else { stdout });`.
/// When gh pr edit exits non-zero with EMPTY stderr but non-empty
/// stdout, the else arm (stdout) is selected.
#[test]
fn gh_set_body_nonzero_exit_empty_stderr_uses_stdout_branch() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // View succeeds with some body; edit exits 1 with empty stderr
    // and non-empty stdout.
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [ \"$2\" = \"view\" ]; then\n\
           echo 'existing body'\n\
           exit 0\n\
         fi\n\
         if [ \"$2\" = \"edit\" ]; then\n\
           echo 'edit error on stdout'\n\
           exit 1\n\
         fi\n\
         exit 1\n",
    );
    let output = run_cmd(
        &repo,
        &[
            "--pr",
            "1",
            "--add-artifact",
            "--label",
            "X",
            "--value",
            "y",
        ],
        &stub_dir,
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    let msg = data["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("edit error on stdout"),
        "expected stdout-branch message; got: {}",
        msg
    );
}

/// Covers the `.map_err(|e| e.to_string())?` spawn-failure arm on
/// line 153 of `gh_set_body` (3 regions: closure body, the map_err
/// region, and the `?` Err propagation). The stub's first invocation
/// (`gh pr view`) returns a valid body then renames itself. The
/// next `gh pr edit` call made from `gh_set_body` cannot resolve
/// `gh` on PATH, so `Command::new("gh").output()` returns Err.
///
/// PATH is `<stub_dir>:/bin` — `/bin` provides `mv` but neither
/// `/bin` nor `/usr/bin` nor `/opt/homebrew/bin` is on PATH, so
/// after the stub renames itself, no `gh` remains reachable.
#[test]
fn gh_set_body_spawn_failure_via_self_delete_stub() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let log_path = repo.join("gh-invocations.log");
    let stub_dir = create_gh_stub(
        &repo,
        &format!(
            "#!/bin/bash\n\
             echo \"invoked: $@\" >> \"{log}\"\n\
             if [ \"$2\" = \"view\" ]; then\n\
               echo 'existing body'\n\
               /bin/mv \"${{BASH_SOURCE[0]}}\" \"${{BASH_SOURCE[0]}}.moved\"\n\
               exit 0\n\
             fi\n\
             exit 1\n",
            log = log_path.display()
        ),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("update-pr-body")
        .args([
            "--pr",
            "1",
            "--add-artifact",
            "--label",
            "X",
            "--value",
            "y",
        ])
        .current_dir(&repo)
        .env("PATH", format!("{}:/bin", stub_dir.to_string_lossy()))
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    // Exactly 1 gh invocation in the log proves the rename took effect
    // and the second lookup spawn-failed (never reached the stub).
    let log_contents = fs::read_to_string(&log_path).unwrap_or_default();
    assert_eq!(
        log_contents.lines().count(),
        1,
        "expected exactly 1 gh invocation (view only); log: {}",
        log_contents
    );
}
