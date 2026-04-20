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
use std::path::Path;
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
        // try_wait on a freshly spawned owned Child is infallible in
        // practice — the kernel returns ECHILD only when another party
        // already reaped the process, which cannot happen for a Child
        // we still own. Treat the Err arm as a genuine unreachable
        // panic per `.claude/rules/testability-means-simplicity.md`.
        match child
            .try_wait()
            .expect("try_wait on owned child is infallible")
        {
            Some(status) => break status,
            None => {
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
        }
    };

    if !exit_status.success() {
        // `code()` returns `None` only when the process was killed by a
        // signal; the tests exercise this via a self-SIGTERM fixture.
        let code_display = exit_status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "signal".to_string());
        return (
            json!({
                "status": "error",
                "message": format!("bin/dependencies failed with exit code {}", code_display),
            }),
            1,
        );
    }

    // Check git status --porcelain for file changes. `git` is a hard
    // dependency of the FLOW toolchain — if spawn fails, the install is
    // broken in a way the caller cannot usefully recover from.
    let status_output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(cwd)
        .output()
        .expect("git binary must be available in PATH");

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
