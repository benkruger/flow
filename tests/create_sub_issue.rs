//! Integration tests for `bin/flow create-sub-issue`.
//!
//! The command resolves both issue numbers to DB IDs via `gh api` and
//! POSTs to the `/sub_issues` endpoint. Tests install a mock `gh` on
//! PATH so subprocess paths are exercised without network access.

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
        .arg("create-sub-issue")
        .args(args)
        .current_dir(repo)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

/// Mock gh that honors `--jq .id` (echoes the bare integer) and
/// succeeds silently on POSTs.
fn create_happy_gh_stub(repo: &Path) -> std::path::PathBuf {
    create_gh_stub(
        repo,
        "#!/bin/bash\n\
         if [[ \"$*\" == *--method*POST* ]]; then\n\
           exit 0\n\
         fi\n\
         case \"$*\" in\n\
           *issues/10*) echo 1000 ; exit 0 ;;\n\
           *issues/20*) echo 2000 ; exit 0 ;;\n\
           *) echo 1 ; exit 0 ;;\n\
         esac\n",
    )
}

#[test]
fn create_sub_issue_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_happy_gh_stub(&repo);

    let output = run_cmd(
        &repo,
        &[
            "--repo",
            "owner/name",
            "--parent-number",
            "10",
            "--child-number",
            "20",
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
    assert_eq!(data["parent"], 10);
    assert_eq!(data["child"], 20);
}

#[test]
fn create_sub_issue_self_reference_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_cmd(
        &repo,
        &[
            "--repo",
            "owner/name",
            "--parent-number",
            "42",
            "--child-number",
            "42",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .contains("self-reference"),
        "Expected self-reference error, got: {}",
        data["message"]
    );
}

#[test]
fn create_sub_issue_fails_when_parent_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         case \"$*\" in\n\
           *issues/10*)\n\
             echo 'parent issue not found' >&2\n\
             exit 1 ;;\n\
           *)\n\
             echo 2000\n\
             exit 0 ;;\n\
         esac\n",
    );

    let output = run_cmd(
        &repo,
        &[
            "--repo",
            "owner/name",
            "--parent-number",
            "10",
            "--child-number",
            "20",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .contains("parent #10"),
        "Expected parent-naming error, got: {}",
        data["message"]
    );
}

#[test]
fn create_sub_issue_fails_when_child_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         case \"$*\" in\n\
           *issues/10*)\n\
             echo 1000\n\
             exit 0 ;;\n\
           *issues/20*)\n\
             echo 'child issue not found' >&2\n\
             exit 1 ;;\n\
           *) exit 0 ;;\n\
         esac\n",
    );

    let output = run_cmd(
        &repo,
        &[
            "--repo",
            "owner/name",
            "--parent-number",
            "10",
            "--child-number",
            "20",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"].as_str().unwrap_or("").contains("child #20"),
        "Expected child-naming error, got: {}",
        data["message"]
    );
}

#[test]
fn create_sub_issue_fails_when_post_fails() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [[ \"$*\" == *--method*POST* ]]; then\n\
           echo 'sub-issue creation refused' >&2\n\
           exit 1\n\
         fi\n\
         echo 1\n\
         exit 0\n",
    );

    let output = run_cmd(
        &repo,
        &[
            "--repo",
            "owner/name",
            "--parent-number",
            "10",
            "--child-number",
            "20",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains("refused"),
        "Expected POST failure in message, got: {}",
        data["message"]
    );
}
