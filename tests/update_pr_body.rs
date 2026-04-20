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
