//! Integration tests for `bin/flow create-milestone`.
//!
//! The command POSTs to `repos/.../milestones` via `gh api` and parses
//! the returned JSON for number + html_url. Tests install a mock `gh`
//! on PATH so subprocess paths are exercised without network access.

mod common;

use std::path::Path;
use std::process::{Command, Output};

use common::{create_gh_stub, create_git_repo_with_remote, parse_output};

fn run_cmd(repo: &Path, args: &[&str], stub_dir: &Path) -> Output {
    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("create-milestone")
        .args(args)
        .current_dir(repo)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

#[test]
fn create_milestone_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         echo '{\"number\": 5, \"html_url\": \"https://github.com/owner/name/milestone/5\"}'\n\
         exit 0\n",
    );

    let output = run_cmd(
        &repo,
        &[
            "--repo",
            "owner/name",
            "--title",
            "v1.0",
            "--due-date",
            "2026-06-01",
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
    assert_eq!(data["number"], 5);
    assert_eq!(data["url"], "https://github.com/owner/name/milestone/5");
}

#[test]
fn create_milestone_gh_failure_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho 'bad request' >&2\nexit 1\n");

    let output = run_cmd(
        &repo,
        &[
            "--repo",
            "owner/name",
            "--title",
            "v1.0",
            "--due-date",
            "2026-06-01",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("bad request"));
}

#[test]
fn create_milestone_invalid_json_response_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho 'not json at all'\nexit 0\n");

    let output = run_cmd(
        &repo,
        &[
            "--repo",
            "owner/name",
            "--title",
            "v1.0",
            "--due-date",
            "2026-06-01",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Invalid JSON"));
}

#[test]
fn create_milestone_missing_number_field_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\necho '{\"html_url\": \"https://x/y\"}'\nexit 0\n",
    );

    let output = run_cmd(
        &repo,
        &[
            "--repo",
            "owner/name",
            "--title",
            "v1.0",
            "--due-date",
            "2026-06-01",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("missing 'number'"));
}

#[test]
fn create_milestone_missing_url_defaults_to_empty_string() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // number present but html_url missing — should succeed with empty URL.
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho '{\"number\": 7}'\nexit 0\n");

    let output = run_cmd(
        &repo,
        &[
            "--repo",
            "owner/name",
            "--title",
            "v2.0",
            "--due-date",
            "2027-01-01",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["number"], 7);
    assert_eq!(data["url"], "");
}
