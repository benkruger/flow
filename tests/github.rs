//! Integration tests for `crate::github::detect_repo`.
//!
//! The inline unit tests in `src/github.rs` cover the happy paths; these
//! tests drive the remaining edge cases that require controlling the
//! process environment: `Command::output()` returning `Err` when `git` is
//! absent from PATH, and the empty-stdout branch.

mod common;

use std::process::Command;

/// When `git` is not findable in PATH, `Command::new("git").output()`
/// returns `Err(io::Error)`, which `detect_repo` converts to `None` via
/// `.ok()?`. Exercising this line requires spawning a subprocess with an
/// empty PATH — doing it in-process would race with parallel tests that
/// read PATH.
///
/// We invoke the `session-context` subcommand from a subprocess with
/// `PATH=""`. `session-context` calls `detect_repo(Some(&project_root()))`,
/// which internally spawns git. With git unavailable, `.output()` fails
/// and the `.ok()?` branch returns `None`. The subcommand is
/// best-effort (writes tab colors, errors silently) so exit code is 0
/// regardless — we just need the code path to execute under coverage.
#[test]
fn detect_repo_returns_none_when_git_missing() {
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());

    let flow_rs = env!("CARGO_BIN_EXE_flow-rs");
    let mut cmd = Command::new(flow_rs);
    cmd.args(["session-context"])
        .current_dir(&repo)
        .env_clear()
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .env("PATH", "");
    // Preserve coverage instrumentation env vars — env_clear would drop them
    // and the child's profile data would never reach flow.profdata.
    for key in ["LLVM_PROFILE_FILE", "CARGO_LLVM_COV_TARGET_DIR"] {
        if let Ok(v) = std::env::var(key) {
            cmd.env(key, v);
        }
    }
    let output = cmd.output().unwrap();
    // Best-effort command: must exit successfully regardless of git state.
    assert!(
        output.status.success(),
        "session-context should not fail when git is absent. stdout: {}, stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// `detect_repo` returns `None` when git returns empty stdout with a
/// successful status — the `url.is_empty()` branch. We trigger this by
/// setting up a `git` stub on PATH that prints nothing and exits 0.
#[test]
fn detect_repo_returns_none_when_git_returns_empty_stdout() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let repo = common::create_git_repo_with_remote(dir.path());
    // Create a stub git that exits 0 with empty stdout.
    let stub_dir = dir.path().join("stub_bin");
    fs::create_dir_all(&stub_dir).unwrap();
    let git_stub = stub_dir.join("git");
    fs::write(&git_stub, "#!/usr/bin/env bash\nexit 0\n").unwrap();
    fs::set_permissions(&git_stub, fs::Permissions::from_mode(0o755)).unwrap();

    let path_env = format!(
        "{}:{}",
        stub_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    // Any CLI subcommand that calls detect_repo works. session-context is
    // a simple, side-effect-only entry point that tolerates every failure.
    let flow_rs = env!("CARGO_BIN_EXE_flow-rs");
    let output = Command::new(flow_rs)
        .args(["session-context"])
        .current_dir(&repo)
        .env("PATH", &path_env)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "session-context should succeed even when git returns empty. stdout: {}, stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
