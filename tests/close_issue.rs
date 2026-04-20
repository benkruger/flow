//! Integration tests for `bin/flow close-issue` (`src/close_issue.rs`).
//!
//! The module shells out to `gh issue close` and, when no `--repo` is
//! provided, to `git remote -v` via `detect_repo`. Tests install a
//! mock `gh` on PATH via `common::create_gh_stub` so the subprocess
//! paths are exercised without network access.

mod common;

use std::path::Path;
use std::process::{Child, Command, Output, Stdio};

use common::{create_gh_stub, create_git_repo_with_remote, parse_output};
use flow_rs::close_issue::{
    close_issue_by_number, close_issue_with_runner, close_issue_with_runner_and_timeout,
    run_impl_main, Args,
};

fn run_close_issue(repo: &Path, args: &[&str], stub_dir: &Path) -> Output {
    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("close-issue")
        .args(args)
        .current_dir(repo)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

#[test]
fn close_issue_happy_path_with_repo_flag() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // gh exits 0 on any invocation.
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_close_issue(
        &repo,
        &["--repo", "owner/name", "--number", "42"],
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
}

#[test]
fn close_issue_gh_failure_returns_stderr_message() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // gh exits 1 with a stderr error message.
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho 'issue not found' >&2\nexit 1\n");

    let output = run_close_issue(
        &repo,
        &["--repo", "owner/name", "--number", "999"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .contains("issue not found"),
        "Expected stderr in message, got: {}",
        data["message"]
    );
}

#[test]
fn close_issue_gh_failure_falls_back_to_stdout_when_stderr_empty() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // gh exits 1 with message on stdout only.
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho 'problem on stdout'\nexit 1\n");

    let output = run_close_issue(&repo, &["--repo", "owner/name", "--number", "7"], &stub_dir);

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .contains("problem on stdout"),
        "Expected stdout in message, got: {}",
        data["message"]
    );
}

#[test]
fn close_issue_gh_failure_with_no_output_returns_unknown_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // gh exits 1 with nothing on either stream.
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 1\n");

    let output = run_close_issue(&repo, &["--repo", "owner/name", "--number", "1"], &stub_dir);

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["message"], "Unknown error");
}

#[test]
fn close_issue_detects_repo_when_flag_omitted() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // `detect_repo` requires a github.com-style URL. Override the fake
    // remote that the helper sets up.
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
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_close_issue(&repo, &["--number", "42"], &stub_dir);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
}

#[test]
fn close_issue_exits_when_repo_undetectable_and_no_flag() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // The helper's origin is a local `bare.git` path, which does NOT
    // match the github.com pattern `detect_repo` requires. No --repo
    // flag means `detect_repo_or_fail` must error-exit.
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_close_issue(&repo, &["--number", "42"], &stub_dir);

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .contains("Could not detect repo"),
        "Expected repo-detection error, got: {}",
        data["message"]
    );
}

#[test]
fn close_issue_gh_spawn_failure_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // Install no gh stub — running with an empty PATH makes the
    // spawn of `gh` fail, exercising the spawn-error path.
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("close-issue")
        .args(["--repo", "owner/name", "--number", "1"])
        .current_dir(&repo)
        .env("PATH", "")
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains("spawn"),
        "Expected spawn-related error, got: {}",
        data["message"]
    );
}

// --- Library-level unit tests (migrated from src/close_issue.rs) ---

// --- close_issue_with_runner ---

#[test]
fn close_issue_with_runner_returns_none_on_success() {
    let factory = |_args: &[&str]| {
        Command::new("sh")
            .args(["-c", "exit 0"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    };
    let result = close_issue_with_runner("owner/repo", 42, &factory);
    assert!(result.is_none());
}

#[test]
fn close_issue_with_runner_returns_stderr_on_nonzero() {
    let factory = |_args: &[&str]| {
        Command::new("sh")
            .args(["-c", "echo boom 1>&2; exit 1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    };
    let err = close_issue_with_runner("owner/repo", 42, &factory).unwrap();
    assert!(err.contains("boom"));
}

/// Exercises lines 91-94 — the polling timeout fires when the
/// elapsed time exceeds the configured timeout. Use the `_with_timeout`
/// seam with `0` so the elapsed-time check trips on the first poll
/// even though the child is still running.
#[test]
fn close_issue_with_runner_and_timeout_zero_returns_timeout_message() {
    let factory = |_args: &[&str]| {
        // sleep 60s — never completes within the test budget.
        Command::new("sh")
            .args(["-c", "sleep 60"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    };
    let err = close_issue_with_runner_and_timeout("owner/repo", 42, 0, &factory).unwrap();
    assert!(
        err.contains("timed out after 0 seconds"),
        "expected timeout message, got: {}",
        err
    );
}

#[test]
fn close_issue_with_runner_returns_spawn_error() {
    let factory = |_args: &[&str]| -> std::io::Result<Child> {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such binary",
        ))
    };
    let err = close_issue_with_runner("owner/repo", 42, &factory).unwrap();
    assert!(err.contains("Failed to spawn"));
}

// --- run_impl_main ---

#[test]
fn close_issue_run_impl_main_no_repo_returns_error_tuple() {
    let args = Args {
        repo: None,
        number: 42,
    };
    let resolver = || None;
    let (value, code) = run_impl_main(args, &resolver);
    assert_eq!(value["status"], "error");
    assert_eq!(code, 1);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Could not detect repo"));
}

#[test]
fn close_issue_with_runner_returns_stdout_when_stderr_empty() {
    // Drives the `if !stdout.is_empty()` branch: subprocess exits
    // non-zero with stdout but no stderr → returns stdout content.
    let factory = |_args: &[&str]| {
        Command::new("sh")
            .args(["-c", "echo from-stdout; exit 1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    };
    let err = close_issue_with_runner("owner/repo", 42, &factory).unwrap();
    assert!(err.contains("from-stdout"));
}

#[test]
fn close_issue_with_runner_returns_unknown_error_when_streams_empty() {
    // Drives the final `Some("Unknown error".to_string())` branch:
    // exit non-zero with no stdout and no stderr.
    let factory = |_args: &[&str]| {
        Command::new("sh")
            .args(["-c", "exit 1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    };
    let err = close_issue_with_runner("owner/repo", 42, &factory).unwrap();
    assert_eq!(err, "Unknown error");
}

/// Stub `gh` on PATH that always exits 0. Used to exercise the
/// `close_issue_by_number` production wrapper without spawning a
/// real network call.
fn install_succeeding_gh_stub() -> tempfile::TempDir {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let stub_dir = tempfile::tempdir().unwrap();
    let stub = stub_dir.path().join("gh");
    let mut f = std::fs::File::create(&stub).unwrap();
    f.write_all(b"#!/bin/bash\nexit 0\n").unwrap();
    let mut perms = std::fs::metadata(&stub).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&stub, perms).unwrap();
    stub_dir
}

/// Run a closure with PATH temporarily set to include the stub dir.
/// Serialized via Mutex to prevent parallel test races on PATH.
fn with_stub_path<F: FnOnce()>(stub_dir: &std::path::Path, f: F) {
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
fn close_issue_by_number_production_wrapper_succeeds_with_stub() {
    let stub_dir = install_succeeding_gh_stub();
    with_stub_path(stub_dir.path(), || {
        let result = close_issue_by_number("owner/repo", 42);
        assert!(result.is_none(), "expected success, got: {:?}", result);
    });
}

#[test]
fn close_issue_run_impl_main_with_repo_arg_calls_close_issue_by_number() {
    // args.repo Some path → resolver not called → close_issue_by_number
    // → returns ok with stub gh.
    let stub_dir = install_succeeding_gh_stub();
    with_stub_path(stub_dir.path(), || {
        let args = Args {
            repo: Some("owner/repo".to_string()),
            number: 42,
        };
        let (value, code) = run_impl_main(args, &|| None);
        assert_eq!(value["status"], "ok");
        assert_eq!(code, 0);
    });
}

#[test]
fn close_issue_run_impl_main_resolver_some_calls_close_issue_by_number() {
    // args.repo None + resolver Some → close_issue_by_number
    let stub_dir = install_succeeding_gh_stub();
    with_stub_path(stub_dir.path(), || {
        let args = Args {
            repo: None,
            number: 42,
        };
        let resolver = || Some("owner/repo".to_string());
        let (value, code) = run_impl_main(args, &resolver);
        assert_eq!(value["status"], "ok");
        assert_eq!(code, 0);
    });
}
