//! Integration tests for `bin/flow auto-close-parent`.
//!
//! The command makes multiple `gh api` calls to check parent/milestone
//! state and conditionally close them. Tests install a mock `gh` on
//! PATH and script responses per-URL to exercise every code path.

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
        .arg("auto-close-parent")
        .args(args)
        .current_dir(repo)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

#[test]
fn closes_parent_and_milestone_when_all_closed() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // gh responses:
    //  - `gh api repos/owner/name/issues/5` → JSON with parent 10 and milestone 3
    //  - `gh api repos/owner/name/issues/10/sub_issues` → array all closed
    //  - `gh issue close 10 --repo owner/name` → empty success
    //  - `gh api repos/owner/name/milestones/3` → open_issues: 0
    //  - `gh api ... --method PATCH ... state=closed` → empty success
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [[ \"$*\" == *issues/5/sub_issues* ]]; then\n\
           # Not hit — parent lookup for issue 5 goes through issues/5 endpoint\n\
           exit 1\n\
         fi\n\
         if [[ \"$*\" == *issues/10/sub_issues* ]]; then\n\
           echo '[{\"number\":5,\"state\":\"closed\"},{\"number\":6,\"state\":\"closed\"}]'\n\
           exit 0\n\
         fi\n\
         if [[ \"$*\" == *issue*close* ]]; then\n\
           exit 0\n\
         fi\n\
         if [[ \"$*\" == *milestones/3* && \"$*\" == *PATCH* ]]; then\n\
           exit 0\n\
         fi\n\
         if [[ \"$*\" == *milestones/3* ]]; then\n\
           echo '{\"open_issues\":0,\"closed_issues\":5}'\n\
           exit 0\n\
         fi\n\
         if [[ \"$*\" == *issues/5* ]]; then\n\
           echo '{\"parent_issue\":{\"number\":10},\"milestone\":{\"number\":3}}'\n\
           exit 0\n\
         fi\n\
         exit 1\n",
    );

    let output = run_cmd(
        &repo,
        &["--repo", "owner/name", "--issue-number", "5"],
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
    assert_eq!(data["parent_closed"], true);
    assert_eq!(data["milestone_closed"], true);
}

#[test]
fn does_not_close_parent_when_sub_issues_still_open() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [[ \"$*\" == *issues/10/sub_issues* ]]; then\n\
           echo '[{\"number\":5,\"state\":\"closed\"},{\"number\":6,\"state\":\"open\"}]'\n\
           exit 0\n\
         fi\n\
         if [[ \"$*\" == *issues/5* ]]; then\n\
           echo '{\"parent_issue\":{\"number\":10}}'\n\
           exit 0\n\
         fi\n\
         exit 1\n",
    );

    let output = run_cmd(
        &repo,
        &["--repo", "owner/name", "--issue-number", "5"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["parent_closed"], false);
    assert_eq!(data["milestone_closed"], false);
}

#[test]
fn does_not_close_milestone_when_open_issues_remain() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [[ \"$*\" == *milestones/3* ]]; then\n\
           echo '{\"open_issues\":2,\"closed_issues\":3}'\n\
           exit 0\n\
         fi\n\
         if [[ \"$*\" == *issues/5* ]]; then\n\
           echo '{\"milestone\":{\"number\":3}}'\n\
           exit 0\n\
         fi\n\
         exit 1\n",
    );

    let output = run_cmd(
        &repo,
        &["--repo", "owner/name", "--issue-number", "5"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["milestone_closed"], false);
}

#[test]
fn no_parent_or_milestone_returns_false_for_both() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // Issue with no parent_issue and no milestone fields.
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [[ \"$*\" == *--jq*parent_issue* ]]; then\n\
           echo 'null'\n\
           exit 0\n\
         fi\n\
         if [[ \"$*\" == *--jq*milestone* ]]; then\n\
           echo 'null'\n\
           exit 0\n\
         fi\n\
         if [[ \"$*\" == *issues/5* ]]; then\n\
           echo '{}'\n\
           exit 0\n\
         fi\n\
         exit 1\n",
    );

    let output = run_cmd(
        &repo,
        &["--repo", "owner/name", "--issue-number", "5"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["parent_closed"], false);
    assert_eq!(data["milestone_closed"], false);
}

#[test]
fn parent_close_fails_when_close_command_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [[ \"$*\" == *issue*close* ]]; then\n\
           echo 'permission denied' >&2\n\
           exit 1\n\
         fi\n\
         if [[ \"$*\" == *issues/10/sub_issues* ]]; then\n\
           echo '[{\"number\":5,\"state\":\"closed\"}]'\n\
           exit 0\n\
         fi\n\
         if [[ \"$*\" == *issues/5* ]]; then\n\
           echo '{\"parent_issue\":{\"number\":10}}'\n\
           exit 0\n\
         fi\n\
         exit 1\n",
    );

    let output = run_cmd(
        &repo,
        &["--repo", "owner/name", "--issue-number", "5"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["parent_closed"], false);
}

#[test]
fn initial_fetch_failure_still_succeeds_with_both_false() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // gh always fails — command should still exit 0 with both flags false
    // (auto-close-parent is best-effort throughout).
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 1\n");

    let output = run_cmd(
        &repo,
        &["--repo", "owner/name", "--issue-number", "5"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["parent_closed"], false);
    assert_eq!(data["milestone_closed"], false);
}

#[test]
fn milestone_patch_failure_leaves_flag_false() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [[ \"$*\" == *milestones/3* && \"$*\" == *PATCH* ]]; then\n\
           echo 'cannot patch' >&2\n\
           exit 1\n\
         fi\n\
         if [[ \"$*\" == *milestones/3* ]]; then\n\
           echo '{\"open_issues\":0}'\n\
           exit 0\n\
         fi\n\
         if [[ \"$*\" == *issues/5* ]]; then\n\
           echo '{\"milestone\":{\"number\":3}}'\n\
           exit 0\n\
         fi\n\
         exit 1\n",
    );

    let output = run_cmd(
        &repo,
        &["--repo", "owner/name", "--issue-number", "5"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["milestone_closed"], false);
}
