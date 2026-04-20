//! Integration tests for `bin/flow auto-close-parent` and its library surface.
//!
//! Migrated from inline `#[cfg(test)]` per
//! `.claude/rules/test-placement.md`.

mod common;

use std::path::Path;
use std::process::{Command, Output};

use common::{create_gh_stub, create_git_repo_with_remote, parse_output};
use flow_rs::auto_close_parent::{
    all_sub_issues_closed, check_milestone_closed, check_parent_closed, fetch_issue_fields,
    parse_issue_fields, run_api, run_impl_main, safe_default_ok, should_close_milestone, Args,
    GhApiRunner,
};

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

// --- Library-level tests (migrated from inline `#[cfg(test)]`) ---

#[test]
fn safe_default_ok_returns_ok_with_both_flags_false() {
    let (value, code) = safe_default_ok();
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["parent_closed"], false);
    assert_eq!(value["milestone_closed"], false);
}

#[test]
fn parse_issue_fields_both_present() {
    let json = r#"{"parent_issue": {"number": 10}, "milestone": {"number": 3}}"#;
    let (parent, milestone) = parse_issue_fields(json);
    assert_eq!(parent, Some(10));
    assert_eq!(milestone, Some(3));
}

#[test]
fn parse_issue_fields_absent() {
    let (parent, milestone) = parse_issue_fields("{}");
    assert_eq!(parent, None);
    assert_eq!(milestone, None);
}

#[test]
fn parse_issue_fields_invalid_json() {
    let (parent, milestone) = parse_issue_fields("not json");
    assert_eq!(parent, None);
    assert_eq!(milestone, None);
}

#[test]
fn parse_issue_fields_parent_not_dict() {
    let json = r#"{"parent_issue": "not_a_dict", "milestone": {"number": 3}}"#;
    let (parent, milestone) = parse_issue_fields(json);
    assert_eq!(parent, None);
    assert_eq!(milestone, Some(3));
}

#[test]
fn parse_issue_fields_milestone_number_not_int() {
    let json = r#"{"parent_issue": {"number": 10}, "milestone": {"number": "not_int"}}"#;
    let (parent, milestone) = parse_issue_fields(json);
    assert_eq!(parent, Some(10));
    assert_eq!(milestone, None);
}

#[test]
fn parse_issue_fields_null_values() {
    let json = r#"{"parent_issue": null, "milestone": null}"#;
    let (parent, milestone) = parse_issue_fields(json);
    assert_eq!(parent, None);
    assert_eq!(milestone, None);
}

#[test]
fn all_sub_issues_closed_all_closed_lib() {
    let json = r#"[{"number": 5, "state": "closed"}, {"number": 6, "state": "closed"}]"#;
    assert!(all_sub_issues_closed(json));
}

#[test]
fn all_sub_issues_closed_some_open_lib() {
    let json = r#"[{"number": 5, "state": "closed"}, {"number": 6, "state": "open"}]"#;
    assert!(!all_sub_issues_closed(json));
}

#[test]
fn all_sub_issues_closed_empty_lib() {
    assert!(!all_sub_issues_closed("[]"));
}

#[test]
fn all_sub_issues_closed_invalid_json_lib() {
    assert!(!all_sub_issues_closed("not json"));
}

#[test]
fn all_sub_issues_closed_missing_state_field_lib() {
    let json = r#"[{"number": 5}]"#;
    assert!(!all_sub_issues_closed(json));
}

#[test]
fn should_close_milestone_zero_open_lib() {
    let json = r#"{"open_issues": 0, "closed_issues": 5}"#;
    assert!(should_close_milestone(json));
}

#[test]
fn should_close_milestone_has_open_lib() {
    let json = r#"{"open_issues": 2, "closed_issues": 3}"#;
    assert!(!should_close_milestone(json));
}

#[test]
fn should_close_milestone_missing_field_lib() {
    let json = r#"{"closed_issues": 5}"#;
    assert!(!should_close_milestone(json));
}

#[test]
fn should_close_milestone_invalid_json_lib() {
    assert!(!should_close_milestone("not json"));
}

#[test]
fn should_close_milestone_null_open_issues_lib() {
    let json = r#"{"open_issues": null}"#;
    assert!(!should_close_milestone(json));
}

#[test]
fn auto_close_parent_run_impl_main_all_runner_failures_returns_ok() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let args = Args {
        repo: "owner/repo".to_string(),
        issue_number: 999,
    };
    let runner: &GhApiRunner = &|_, _| Err("simulated".to_string());
    let (value, code) = run_impl_main(args, &cwd, runner);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["parent_closed"], false);
    assert_eq!(value["milestone_closed"], false);
}

fn install_failing_gh_stub() -> tempfile::TempDir {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let stub_dir = tempfile::tempdir().unwrap();
    let stub = stub_dir.path().join("gh");
    let mut f = std::fs::File::create(&stub).unwrap();
    f.write_all(b"#!/bin/bash\nexit 1\n").unwrap();
    let mut perms = std::fs::metadata(&stub).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&stub, perms).unwrap();
    stub_dir
}

fn with_stub_path<F: FnOnce()>(stub_dir: &Path, f: F) {
    use std::sync::Mutex;
    static PATH_LOCK: Mutex<()> = Mutex::new(());
    let _guard = PATH_LOCK.lock().unwrap();
    let original = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", stub_dir.display(), original);
    unsafe {
        std::env::set_var("PATH", new_path);
    }
    f();
    unsafe {
        std::env::set_var("PATH", original);
    }
}

#[test]
fn check_parent_closed_standalone_null_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|_, _| Ok("null\n".to_string());
    assert!(!check_parent_closed("owner/repo", 5, None, &cwd, runner));
}

#[test]
fn check_parent_closed_standalone_empty_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|_, _| Ok(String::new());
    assert!(!check_parent_closed("owner/repo", 5, None, &cwd, runner));
}

#[test]
fn check_parent_closed_standalone_unparseable_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|_, _| Ok("not_an_int".to_string());
    assert!(!check_parent_closed("owner/repo", 5, None, &cwd, runner));
}

#[test]
fn check_parent_closed_standalone_succeeds_then_closes() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let queue: std::cell::RefCell<std::collections::VecDeque<String>> =
        std::cell::RefCell::new(std::collections::VecDeque::from(vec![
            "10\n".to_string(),
            r#"[{"number":5,"state":"closed"}]"#.to_string(),
            String::new(),
        ]));
    let runner: &GhApiRunner = &move |_, _| Ok(queue.borrow_mut().pop_front().unwrap_or_default());
    assert!(check_parent_closed("owner/repo", 5, None, &cwd, runner));
}

#[test]
fn check_milestone_closed_standalone_null_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|_, _| Ok("null\n".to_string());
    assert!(!check_milestone_closed("owner/repo", 5, None, &cwd, runner));
}

#[test]
fn check_milestone_closed_standalone_empty_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|_, _| Ok(String::new());
    assert!(!check_milestone_closed("owner/repo", 5, None, &cwd, runner));
}

#[test]
fn check_milestone_closed_standalone_unparseable_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|_, _| Ok("not_an_int".to_string());
    assert!(!check_milestone_closed("owner/repo", 5, None, &cwd, runner));
}

#[test]
fn check_milestone_closed_standalone_succeeds_then_closes() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let queue: std::cell::RefCell<std::collections::VecDeque<String>> =
        std::cell::RefCell::new(std::collections::VecDeque::from(vec![
            "3\n".to_string(),
            r#"{"open_issues":0}"#.to_string(),
            String::new(),
        ]));
    let runner: &GhApiRunner = &move |_, _| Ok(queue.borrow_mut().pop_front().unwrap_or_default());
    assert!(check_milestone_closed("owner/repo", 5, None, &cwd, runner));
}

#[test]
fn check_parent_closed_close_command_fails_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let queue: std::cell::RefCell<std::collections::VecDeque<Result<String, String>>> =
        std::cell::RefCell::new(std::collections::VecDeque::from(vec![
            Ok(r#"[{"number":5,"state":"closed"}]"#.to_string()),
            Err("close failed".to_string()),
        ]));
    let runner: &GhApiRunner = &move |_, _| {
        queue
            .borrow_mut()
            .pop_front()
            .expect("test runner queue exhausted")
    };
    assert!(!check_parent_closed(
        "owner/repo",
        5,
        Some(10),
        &cwd,
        runner
    ));
}

#[test]
fn check_milestone_closed_patch_command_fails_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let queue: std::cell::RefCell<std::collections::VecDeque<Result<String, String>>> =
        std::cell::RefCell::new(std::collections::VecDeque::from(vec![
            Ok(r#"{"open_issues":0}"#.to_string()),
            Err("patch failed".to_string()),
        ]));
    let runner: &GhApiRunner = &move |_, _| {
        queue
            .borrow_mut()
            .pop_front()
            .expect("test runner queue exhausted")
    };
    assert!(!check_milestone_closed(
        "owner/repo",
        5,
        Some(3),
        &cwd,
        runner
    ));
}

#[test]
fn check_parent_closed_sub_issues_open_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner =
        &|_, _| Ok(r#"[{"number":5,"state":"closed"},{"number":6,"state":"open"}]"#.to_string());
    assert!(!check_parent_closed(
        "owner/repo",
        5,
        Some(10),
        &cwd,
        runner
    ));
}

#[test]
fn check_parent_closed_sub_issues_fetch_error_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|_, _| Err("network error".to_string());
    assert!(!check_parent_closed(
        "owner/repo",
        5,
        Some(10),
        &cwd,
        runner
    ));
}

#[test]
fn check_milestone_closed_milestone_fetch_error_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|_, _| Err("network error".to_string());
    assert!(!check_milestone_closed(
        "owner/repo",
        5,
        Some(3),
        &cwd,
        runner
    ));
}

#[test]
fn check_milestone_closed_open_issues_nonzero_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|_, _| Ok(r#"{"open_issues":2}"#.to_string());
    assert!(!check_milestone_closed(
        "owner/repo",
        5,
        Some(3),
        &cwd,
        runner
    ));
}

#[test]
fn run_api_with_failing_gh_returns_err_lib() {
    let stub_dir = install_failing_gh_stub();
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().canonicalize().unwrap();
    with_stub_path(stub_dir.path(), || {
        let result = run_api(&["gh", "api", "repos/x/y/issues/1"], &cwd);
        assert!(result.is_err());
    });
}

#[test]
fn fetch_issue_fields_production_with_failing_gh_returns_none_none() {
    let stub_dir = install_failing_gh_stub();
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().canonicalize().unwrap();
    with_stub_path(stub_dir.path(), || {
        let (parent, milestone) = fetch_issue_fields("owner/repo", 5, &cwd, &run_api);
        assert_eq!(parent, None);
        assert_eq!(milestone, None);
    });
}

#[test]
fn check_parent_closed_production_with_failing_gh_returns_false() {
    let stub_dir = install_failing_gh_stub();
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().canonicalize().unwrap();
    with_stub_path(stub_dir.path(), || {
        assert!(!check_parent_closed("owner/repo", 5, None, &cwd, &run_api));
    });
}

#[test]
fn check_milestone_closed_production_with_failing_gh_returns_false() {
    let stub_dir = install_failing_gh_stub();
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().canonicalize().unwrap();
    with_stub_path(stub_dir.path(), || {
        assert!(!check_milestone_closed(
            "owner/repo",
            5,
            None,
            &cwd,
            &run_api
        ));
    });
}

#[test]
fn run_impl_main_production_with_failing_gh_returns_ok_both_false() {
    let stub_dir = install_failing_gh_stub();
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().canonicalize().unwrap();
    with_stub_path(stub_dir.path(), || {
        let args = Args {
            repo: "owner/repo".to_string(),
            issue_number: 5,
        };
        let (value, code) = run_impl_main(args, &cwd, &run_api);
        assert_eq!(code, 0);
        assert_eq!(value["status"], "ok");
    });
}

#[test]
fn auto_close_parent_run_impl_main_happy_path_closes_both() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let args = Args {
        repo: "owner/repo".to_string(),
        issue_number: 5,
    };
    let queue: std::cell::RefCell<std::collections::VecDeque<String>> =
        std::cell::RefCell::new(std::collections::VecDeque::from(vec![
            r#"{"parent_issue":{"number":10},"milestone":{"number":3}}"#.to_string(),
            r#"[{"number":5,"state":"closed"},{"number":6,"state":"closed"}]"#.to_string(),
            String::new(),
            r#"{"open_issues":0}"#.to_string(),
            String::new(),
        ]));
    let runner: &GhApiRunner = &move |_, _| Ok(queue.borrow_mut().pop_front().unwrap_or_default());
    let (value, code) = run_impl_main(args, &cwd, runner);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["parent_closed"], true);
    assert_eq!(value["milestone_closed"], true);
}
