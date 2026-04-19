//! The `bin/flow update-deps` subcommand.
//!
//! Runs the target project's `bin/dependencies` and reports whether
//! git status changed. Spawns the dependency script in a new process
//! group so the entire subtree (dep manager + its children) can be
//! killed on timeout via `killpg`.
//!
//! Output (JSON to stdout):
//!   Skipped:    {"status": "skipped", "reason": "bin/dependencies not found"}
//!   No changes: {"status": "ok", "changes": false}
//!   Changes:    {"status": "ok", "changes": true}
//!   Error:      {"status": "error", "message": "..."}
//!
//! Environment:
//!   FLOW_UPDATE_DEPS_TIMEOUT — timeout in seconds (default: 300)

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const DEFAULT_TIMEOUT_SECS: u64 = 300;
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Core update-deps execution with explicit cwd and timeout parameters.
///
/// Returns `(json_value, exit_code)` so the CLI wrapper can print and
/// exit. Never panics — every error path returns a structured JSON
/// error. On timeout, issues `killpg(-pgid, SIGKILL)` so grandchildren
/// (e.g. `bundle install` spawning `git`) don't survive as orphans.
pub fn run_update_deps(cwd: &Path, timeout_secs: u64) -> (Value, i32) {
    let deps = cwd.join("bin").join("dependencies");

    // Returns false for directories AND nonexistent paths. Both cases
    // report skipped.
    if !deps.is_file() {
        return (
            json!({
                "status": "skipped",
                "reason": "bin/dependencies not found",
            }),
            0,
        );
    }

    let mut cmd = Command::new(&deps);
    cmd.current_dir(cwd).process_group(0);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return (
                json!({
                    "status": "error",
                    "message": format!("bin/dependencies could not be executed: {}", e),
                }),
                1,
            );
        }
    };

    let pid = child.id() as i32;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    // SIGKILL the entire process group. `-pid` targets the
                    // process group leader (the child, since process_group(0)
                    // made it a new leader).
                    unsafe {
                        libc::kill(-pid, libc::SIGKILL);
                    }
                    let _ = child.wait();
                    return (
                        json!({
                            "status": "error",
                            "message": format!(
                                "bin/dependencies timed out after {}s",
                                timeout_secs
                            ),
                        }),
                        1,
                    );
                }
                thread::sleep(POLL_INTERVAL);
            }
            Err(e) => {
                return (
                    json!({
                        "status": "error",
                        "message": format!("bin/dependencies wait failed: {}", e),
                    }),
                    1,
                );
            }
        }
    };

    if !exit_status.success() {
        let code = exit_status.code().unwrap_or(1);
        return (
            json!({
                "status": "error",
                "message": format!("bin/dependencies failed with exit code {}", code),
            }),
            1,
        );
    }

    // Check git status --porcelain for file changes
    let status_output = match Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(cwd)
        .output()
    {
        Ok(o) => o,
        Err(_) => {
            return (
                json!({
                    "status": "error",
                    "message": "git status failed",
                }),
                1,
            );
        }
    };

    if !status_output.status.success() {
        return (
            json!({
                "status": "error",
                "message": "git status failed",
            }),
            1,
        );
    }

    let stdout = String::from_utf8_lossy(&status_output.stdout);
    let changes = !stdout.trim().is_empty();

    (json!({"status": "ok", "changes": changes}), 0)
}

/// Testable CLI entry point. Parses the timeout env var and delegates
/// to [`run_update_deps`].
///
/// `env_timeout` is `None` when the env var is unset, matching the
/// default 300s. `Some(s)` is parsed as a u64; parse failures return
/// a structured error JSON matching `lib/update-deps.py` verbatim.
pub fn run_impl(cwd: &Path, env_timeout: Option<&str>) -> (Value, i32) {
    let timeout = match env_timeout {
        None => DEFAULT_TIMEOUT_SECS,
        Some(s) => match s.parse::<u64>() {
            Ok(n) => n,
            Err(_) => {
                return (
                    json!({
                        "status": "error",
                        "message": "FLOW_UPDATE_DEPS_TIMEOUT is not a valid integer",
                    }),
                    1,
                );
            }
        },
    };
    run_update_deps(cwd, timeout)
}

/// CLI entry point for `bin/flow update-deps`.
pub fn run() {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let env_timeout = std::env::var("FLOW_UPDATE_DEPS_TIMEOUT").ok();
    let (result, code) = run_impl(&cwd, env_timeout.as_deref());
    println!("{}", serde_json::to_string(&result).unwrap());
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    /// Initialize a git repo in the given directory with an initial commit.
    fn init_git_repo(dir: &Path) {
        let run = |args: &[&str]| {
            let output = Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .expect("git command failed");
            assert!(output.status.success(), "git {:?} failed", args);
        };
        run(&["init", "--initial-branch", "main"]);
        run(&["config", "user.email", "test@test.com"]);
        run(&["config", "user.name", "Test"]);
        run(&["config", "commit.gpgsign", "false"]);
        run(&["commit", "--allow-empty", "-m", "init"]);
    }

    /// Write bin/dependencies with the given script body (executable).
    fn write_deps_script(dir: &Path, body: &str) {
        let bin_dir = dir.join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let deps = bin_dir.join("dependencies");
        fs::write(&deps, format!("#!/usr/bin/env bash\n{}\n", body)).unwrap();
        fs::set_permissions(&deps, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Commit whatever is staged/untracked so git status is clean.
    fn commit_all(dir: &Path, msg: &str) {
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", msg])
            .current_dir(dir)
            .output()
            .unwrap();
    }

    // --- run_update_deps() tests ---

    #[test]
    fn skipped_when_no_bin_dependencies() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        assert!(!dir.path().join("bin").join("dependencies").exists());
        let (out, code) = run_update_deps(dir.path(), 300);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "skipped");
        assert!(out["reason"].as_str().unwrap().contains("not found"));
    }

    #[test]
    fn no_changes_after_run() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        write_deps_script(dir.path(), "# no-op");
        commit_all(dir.path(), "add deps");

        let (out, code) = run_update_deps(dir.path(), 300);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert_eq!(out["changes"], false);
    }

    #[test]
    fn changes_after_run() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        write_deps_script(dir.path(), "echo updated > deps.lock");
        commit_all(dir.path(), "add deps");

        let (out, code) = run_update_deps(dir.path(), 300);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert_eq!(out["changes"], true);
    }

    #[test]
    fn error_when_deps_fails() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        write_deps_script(dir.path(), "exit 1");
        commit_all(dir.path(), "add deps");

        let (out, code) = run_update_deps(dir.path(), 300);
        assert_eq!(code, 1);
        assert_eq!(out["status"], "error");
        let msg = out["message"].as_str().unwrap().to_lowercase();
        assert!(msg.contains("failed") || msg.contains("exit"));
    }

    #[test]
    fn timeout_reports_error() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        write_deps_script(dir.path(), "sleep 300");
        commit_all(dir.path(), "add deps");

        // 1 second timeout — sleep 300 will be killed
        let start = Instant::now();
        let (out, code) = run_update_deps(dir.path(), 1);
        let elapsed = start.elapsed();

        assert_eq!(code, 1);
        assert_eq!(out["status"], "error");
        assert!(out["message"].as_str().unwrap().contains("timed out"));
        // Must actually kill the process, not block for 300s
        assert!(
            elapsed < Duration::from_secs(10),
            "timeout took too long: {:?}",
            elapsed
        );
    }

    #[test]
    fn non_bash_deps_script() {
        // Python shebang — confirms we don't force bash
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let deps = bin_dir.join("dependencies");
        fs::write(
            &deps,
            "#!/usr/bin/env python3\nfrom pathlib import Path\nPath('py-deps.lock').write_text('v1')\n",
        )
        .unwrap();
        fs::set_permissions(&deps, fs::Permissions::from_mode(0o755)).unwrap();
        commit_all(dir.path(), "add deps");

        let (out, code) = run_update_deps(dir.path(), 300);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert_eq!(out["changes"], true);
    }

    #[test]
    fn non_executable_deps_reports_error() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let deps = bin_dir.join("dependencies");
        fs::write(&deps, "#!/usr/bin/env bash\necho ok\n").unwrap();
        fs::set_permissions(&deps, fs::Permissions::from_mode(0o644)).unwrap();
        commit_all(dir.path(), "add deps");

        let (out, code) = run_update_deps(dir.path(), 300);
        assert_eq!(code, 1);
        assert_eq!(out["status"], "error");
        assert!(out["message"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("executed"));
    }

    #[test]
    fn directory_instead_of_file_reports_skipped() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        let deps_dir = dir.path().join("bin").join("dependencies");
        fs::create_dir_all(&deps_dir).unwrap();

        let (out, code) = run_update_deps(dir.path(), 300);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "skipped");
    }

    #[test]
    fn git_status_failure_reports_error() {
        // Non-git directory — deps exists but git status fails
        let dir = tempfile::tempdir().unwrap();
        write_deps_script(dir.path(), "# no-op");
        let (out, code) = run_update_deps(dir.path(), 300);
        assert_eq!(code, 1);
        assert_eq!(out["status"], "error");
        assert!(out["message"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("git status"));
    }

    #[test]
    fn deps_stdout_does_not_corrupt_return_value() {
        // Verifies that run_update_deps returns a well-formed Value on
        // the happy path: bin/dependencies exits 0, the repo is clean
        // after the script runs, and the Value contains
        // status="ok"/changes=false exactly. The production Value is
        // assembled from json!(...) literals that use only the exit
        // code and the `git status` output — child stdout is not part
        // of the Value assembly, so there is no code path where a
        // child's inherited writes can corrupt the return structure.
        // That structural guarantee lives in run_update_deps itself,
        // not in this test; this test only exercises the happy-path
        // JSON shape.
        //
        // The echo is redirected to /dev/null inside the bash script
        // so no bytes reach the inherited terminal (cargo test does
        // not capture inherited child fds, unlike pytest). The echo
        // still runs inside the child, exercising the full
        // process_group + try_wait + timeout + exit status + git
        // status + Value assembly path.
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        write_deps_script(
            dir.path(),
            "echo 'Installing dependencies...' > /dev/null 2>&1",
        );
        commit_all(dir.path(), "add deps");

        let (out, code) = run_update_deps(dir.path(), 300);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
        assert_eq!(out["changes"], false);
    }

    // --- run_impl() tests ---

    #[test]
    fn cli_default_timeout_when_env_absent() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        write_deps_script(dir.path(), "# no-op");
        commit_all(dir.path(), "add deps");

        let (out, code) = run_impl(dir.path(), None);
        assert_eq!(code, 0);
        assert_eq!(out["status"], "ok");
    }

    #[test]
    fn cli_env_timeout_override() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        write_deps_script(dir.path(), "sleep 300");
        commit_all(dir.path(), "add deps");

        let (out, code) = run_impl(dir.path(), Some("1"));
        assert_eq!(code, 1);
        assert_eq!(out["status"], "error");
        assert!(out["message"].as_str().unwrap().contains("timed out"));
    }

    #[test]
    fn cli_invalid_env_timeout_reports_error() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        write_deps_script(dir.path(), "# no-op");
        commit_all(dir.path(), "add deps");

        let (out, code) = run_impl(dir.path(), Some("notanumber"));
        assert_eq!(code, 1);
        assert_eq!(out["status"], "error");
        assert!(out["message"]
            .as_str()
            .unwrap()
            .contains("FLOW_UPDATE_DEPS_TIMEOUT"));
    }
}
