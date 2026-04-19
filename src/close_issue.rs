//! Close a single GitHub issue via gh CLI.
//!
//! Usage:
//!   bin/flow close-issue --number <N> [--repo <repo>]
//!
//! Output (JSON to stdout):
//!   Success: {"status": "ok"}
//!   Error:   {"status": "error", "message": "..."}

use std::process::{Child, Command, Stdio};
use std::time::Duration;

use clap::Parser;
use serde_json::{json, Value};

use crate::complete_preflight::LOCAL_TIMEOUT;
use crate::github::detect_repo;

#[derive(Parser, Debug)]
#[command(name = "close-issue", about = "Close a GitHub issue")]
pub struct Args {
    /// Repository (owner/name)
    #[arg(long)]
    pub repo: Option<String>,

    /// Issue number
    #[arg(long)]
    pub number: i64,
}

/// Close a GitHub issue via injected child_factory. Returns Some(error)
/// on failure or None on success. Tests inject sh/sleep child factories
/// to exercise the success, non-zero-exit, timeout, and spawn-error
/// branches without spawning real `gh`.
pub fn close_issue_with_runner(
    repo: &str,
    number: i64,
    child_factory: &dyn Fn(&[&str]) -> std::io::Result<Child>,
) -> Option<String> {
    let timeout = Duration::from_secs(LOCAL_TIMEOUT);

    let args: Vec<String> = vec![
        "issue".to_string(),
        "close".to_string(),
        "--repo".to_string(),
        repo.to_string(),
        number.to_string(),
    ];
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    let mut child = match child_factory(&arg_refs) {
        Ok(c) => c,
        Err(e) => return Some(format!("Failed to spawn: {}", e)),
    };

    let start = std::time::Instant::now();
    let poll_interval = Duration::from_millis(50);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let output = match child.wait_with_output() {
                    Ok(o) => o,
                    Err(e) => return Some(e.to_string()),
                };
                if output.status.success() {
                    return None;
                }
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !stderr.is_empty() {
                    return Some(stderr);
                }
                if !stdout.is_empty() {
                    return Some(stdout);
                }
                return Some("Unknown error".to_string());
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Some(format!("Command timed out after {} seconds", LOCAL_TIMEOUT));
                }
                std::thread::sleep(poll_interval.min(timeout - start.elapsed()));
            }
            Err(e) => return Some(e.to_string()),
        }
    }
}

/// Close a GitHub issue and return error message or None on success.
pub fn close_issue_by_number(repo: &str, number: i64) -> Option<String> {
    close_issue_with_runner(repo, number, &|args| {
        Command::new("gh")
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    })
}

/// Main-arm dispatcher with injected repo_resolver. Returns
/// `(value, exit_code)`. The repo_resolver closure returns the detected
/// repo (or None when `git remote` has no origin); production binds it
/// to `detect_repo(None)`. Tests pass closures returning Some/None.
pub fn run_impl_main(args: Args, repo_resolver: &dyn Fn() -> Option<String>) -> (Value, i32) {
    let repo = match args.repo {
        Some(r) => r,
        None => match repo_resolver() {
            Some(r) => r,
            None => {
                return (
                    json!({"status": "error", "message": "Could not detect repo from git remote. Use --repo owner/name."}),
                    1,
                );
            }
        },
    };

    if let Some(e) = close_issue_by_number(&repo, args.number) {
        return (json!({"status": "error", "message": e}), 1);
    }

    (json!({"status": "ok"}), 0)
}

pub fn run(args: Args) -> ! {
    let (value, code) = run_impl_main(args, &|| detect_repo(None));
    crate::dispatch::dispatch_json(value, code)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // → returns ok with stub gh. Counter-based check: assert resolver
        // call count is 0 instead of panicking from inside the closure.
        let stub_dir = install_succeeding_gh_stub();
        with_stub_path(stub_dir.path(), || {
            let call_count = std::cell::RefCell::new(0);
            let args = Args {
                repo: Some("owner/repo".to_string()),
                number: 42,
            };
            let resolver = || -> Option<String> {
                *call_count.borrow_mut() += 1;
                None
            };
            let (value, code) = run_impl_main(args, &resolver);
            assert_eq!(value["status"], "ok");
            assert_eq!(code, 0);
            assert_eq!(
                *call_count.borrow(),
                0,
                "resolver must not be called when args.repo is Some"
            );
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
}
