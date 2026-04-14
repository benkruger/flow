//! Integration tests for `bin/flow link-blocked-by`.
//!
//! The command resolves both issue numbers to DB IDs via `gh api` and
//! then POSTs to the dependencies endpoint. Tests install a mock `gh`
//! binary on PATH so the subprocess paths are exercised without
//! network access.

mod common;

use std::path::Path;
use std::process::{Command, Output};

use common::{create_gh_stub, create_git_repo_with_remote, parse_output};

fn run_link(repo: &Path, args: &[&str], stub_dir: &Path) -> Output {
    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("link-blocked-by")
        .args(args)
        .current_dir(repo)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

/// Mock gh that honors `--jq .id` by echoing the extracted integer
/// directly (mirroring real `gh api ... --jq .id` output) and succeeds
/// silently on POSTs.
fn create_happy_gh_stub(repo: &Path) -> std::path::PathBuf {
    create_gh_stub(
        repo,
        "#!/bin/bash\n\
         # POST to /dependencies/blocked_by succeeds silently.\n\
         if [[ \"$*\" == *--method*POST* ]]; then\n\
           exit 0\n\
         fi\n\
         # `gh api repos/.../issues/N --jq .id` — output bare integer.\n\
         case \"$*\" in\n\
           *issues/42*) echo 4200 ; exit 0 ;;\n\
           *issues/99*) echo 9900 ; exit 0 ;;\n\
           *) echo 1 ; exit 0 ;;\n\
         esac\n",
    )
}

#[test]
fn link_blocked_by_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_happy_gh_stub(&repo);

    let output = run_link(
        &repo,
        &[
            "--repo",
            "owner/name",
            "--blocked-number",
            "42",
            "--blocking-number",
            "99",
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
    assert_eq!(data["blocked"], 42);
    assert_eq!(data["blocking"], 99);
}

#[test]
fn link_blocked_by_self_reference_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_link(
        &repo,
        &[
            "--repo",
            "owner/name",
            "--blocked-number",
            "42",
            "--blocking-number",
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
fn link_blocked_by_fails_when_blocked_issue_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // gh fails on the first issue resolution (blocked = 42).
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         case \"$*\" in\n\
           *issues/42*)\n\
             echo 'issue 42 not found' >&2\n\
             exit 1 ;;\n\
           *)\n\
             echo '{\"id\": 9900}'\n\
             exit 0 ;;\n\
         esac\n",
    );

    let output = run_link(
        &repo,
        &[
            "--repo",
            "owner/name",
            "--blocked-number",
            "42",
            "--blocking-number",
            "99",
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
            .contains("blocked #42"),
        "Expected error naming blocked issue, got: {}",
        data["message"]
    );
}

#[test]
fn link_blocked_by_fails_when_blocking_issue_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // First resolution (blocked = 42) succeeds; second (blocking = 99) fails.
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         case \"$*\" in\n\
           *issues/42*)\n\
             echo 4200\n\
             exit 0 ;;\n\
           *issues/99*)\n\
             echo 'issue 99 not found' >&2\n\
             exit 1 ;;\n\
           *)\n\
             exit 0 ;;\n\
         esac\n",
    );

    let output = run_link(
        &repo,
        &[
            "--repo",
            "owner/name",
            "--blocked-number",
            "42",
            "--blocking-number",
            "99",
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
            .contains("blocking #99"),
        "Expected error naming blocking issue, got: {}",
        data["message"]
    );
}

#[test]
fn link_blocked_by_fails_when_dependency_post_fails() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // Both ID lookups succeed but the POST to /dependencies/blocked_by fails.
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [[ \"$*\" == *--method*POST* ]]; then\n\
           echo 'dependency creation refused' >&2\n\
           exit 1\n\
         fi\n\
         echo 1\n\
         exit 0\n",
    );

    let output = run_link(
        &repo,
        &[
            "--repo",
            "owner/name",
            "--blocked-number",
            "42",
            "--blocking-number",
            "99",
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
            .contains("dependency")
            || data["message"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .contains("refused"),
        "Expected POST failure in message, got: {}",
        data["message"]
    );
}
