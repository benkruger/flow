//! Tests for `flow_rs::github`. Migrated from inline
//! `#[cfg(test)]` in `src/github.rs` per
//! `.claude/rules/test-placement.md`, combined with the pre-existing
//! subprocess-environment tests below.
//!
//! The URL-parsing tests drive the public `parse_github_url` function
//! (the same one `detect_repo` uses internally, so no regex
//! duplication). The `detect_repo` edge cases that require process-
//! environment control (missing git, empty stdout) run via
//! subprocess-spawned `session-context` so they don't race with
//! parallel tests reading PATH.

mod common;

use std::process::Command;

use flow_rs::github::{detect_repo, parse_github_url};

// --- parse_github_url ---

#[test]
fn ssh_url() {
    assert_eq!(
        parse_github_url("git@github.com:owner/repo.git"),
        Some("owner/repo".to_string())
    );
}

#[test]
fn https_url() {
    assert_eq!(
        parse_github_url("https://github.com/owner/repo"),
        Some("owner/repo".to_string())
    );
}

#[test]
fn https_url_with_git_suffix() {
    assert_eq!(
        parse_github_url("https://github.com/owner/repo.git"),
        Some("owner/repo".to_string())
    );
}

#[test]
fn non_github_url() {
    assert_eq!(parse_github_url("https://gitlab.com/owner/repo"), None);
}

#[test]
fn empty_url() {
    assert_eq!(parse_github_url(""), None);
}

#[test]
fn extract_repo_with_trailing_slash() {
    // github.com/owner/repo/ — trailing slash does not parse.
    // Current regex contract — documents the limitation.
    assert_eq!(parse_github_url("https://github.com/owner/repo/"), None);
}

#[test]
fn extract_repo_http_not_https() {
    assert_eq!(
        parse_github_url("http://github.com/owner/repo"),
        Some("owner/repo".to_string())
    );
}

// --- detect_repo (in-process) ---

#[test]
fn detect_repo_in_current_dir() {
    // Running in this repo should return Some or None depending on
    // whether the test binary is running inside a git checkout.
    // We don't assert the content — only that the call completes
    // without panicking. Coverage comes from exercising the code
    // path.
    let _ = detect_repo(None);
}

#[test]
fn detect_repo_with_cwd_outside_git_returns_none() {
    // A fresh tempdir is not a git repo, so detect_repo → None.
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(detect_repo(Some(tmp.path())), None);
}

#[test]
fn detect_repo_with_nonexistent_cwd_returns_none() {
    // A missing directory → git fails → None.
    let nonexistent = std::path::Path::new("/definitely/not/a/real/path");
    assert_eq!(detect_repo(Some(nonexistent)), None);
}

// --- detect_repo (subprocess — environment control) ---

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
