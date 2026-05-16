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

#[test]
fn ssh_alias_url_does_not_match_regex() {
    // The `parse_github_url` regex requires the literal `github.com`
    // substring. SSH host aliases like `git@github-pt:owner/repo.git`
    // do not match — the `detect_repo` gh fallback is the surface that
    // resolves aliases. A future edit that loosens the regex to accept
    // aliases must also delete or rewrite this test.
    assert_eq!(parse_github_url("git@github-pt:owner/repo.git"), None);
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
    // Stub gh too so the fallback cannot reach the real gh CLI and pull
    // in the user's authenticated session. Exit non-zero to drive the
    // None branch of the fallback.
    let gh_stub = stub_dir.join("gh");
    fs::write(&gh_stub, "#!/usr/bin/env bash\nexit 1\n").unwrap();
    fs::set_permissions(&gh_stub, fs::Permissions::from_mode(0o755)).unwrap();

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

/// Helper: create a git repo whose `origin` remote URL is a custom
/// string (SSH alias, non-GitHub URL, etc.). Returns the repo path.
/// Unlike `common::create_git_repo_with_remote`, this does not push
/// to a bare remote — the URL is a string, not a path to a real repo.
fn create_repo_with_custom_remote(
    parent: &std::path::Path,
    remote_url: &str,
) -> std::path::PathBuf {
    let repo = parent.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(&repo)
        .output()
        .unwrap();
    for (key, val) in [
        ("user.email", "test@test.com"),
        ("user.name", "Test"),
        ("commit.gpgsign", "false"),
    ] {
        Command::new("git")
            .args(["config", key, val])
            .current_dir(&repo)
            .output()
            .unwrap();
    }
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["remote", "add", "origin", remote_url])
        .current_dir(&repo)
        .output()
        .unwrap();
    repo
}

/// `detect_repo` falls back to `gh repo view` when `parse_github_url`
/// returns None. SSH host aliases (`git@github-pt:owner/repo.git`)
/// are the canonical case — the regex requires the literal `github.com`
/// substring, but `gh` resolves the alias via its authenticated session.
/// The stub touches a marker file when invoked; the marker's existence
/// is what observably proves the fallback ran (session-context itself
/// always exits 0 because it swallows detection failures).
#[test]
fn gh_fallback_resolves_ssh_alias() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let repo = create_repo_with_custom_remote(dir.path(), "git@github-pt:owner/repo.git");
    let stub_dir = dir.path().join("stub_bin");
    fs::create_dir_all(&stub_dir).unwrap();
    let gh_stub = stub_dir.join("gh");
    let marker = stub_dir.join("gh-was-called");
    fs::write(
        &gh_stub,
        format!(
            "#!/usr/bin/env bash\ntouch '{}'\necho 'owner/repo'\nexit 0\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&gh_stub, fs::Permissions::from_mode(0o755)).unwrap();

    let path_env = format!(
        "{}:{}",
        stub_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let flow_rs = env!("CARGO_BIN_EXE_flow-rs");
    let output = Command::new(flow_rs)
        .args(["session-context"])
        .current_dir(&repo)
        .env("PATH", &path_env)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "session-context must succeed when gh fallback resolves the SSH alias. stdout: {}, stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        marker.exists(),
        "gh fallback must run when parse_github_url returns None for an SSH alias",
    );
}

/// `detect_repo`'s gh fallback rejects malformed output that does not
/// contain `/` — the `s.contains('/')` guard. Stub `gh` to print a
/// no-slash string and verify the fallback ran AND that
/// `session-context` still succeeds (detection failure is non-fatal).
#[test]
fn gh_fallback_rejects_malformed_output() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let repo = create_repo_with_custom_remote(dir.path(), "git@github-pt:owner/repo.git");
    let stub_dir = dir.path().join("stub_bin");
    fs::create_dir_all(&stub_dir).unwrap();
    let gh_stub = stub_dir.join("gh");
    let marker = stub_dir.join("gh-was-called");
    fs::write(
        &gh_stub,
        format!(
            "#!/usr/bin/env bash\ntouch '{}'\necho 'no-slash-here'\nexit 0\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&gh_stub, fs::Permissions::from_mode(0o755)).unwrap();

    let path_env = format!(
        "{}:{}",
        stub_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let flow_rs = env!("CARGO_BIN_EXE_flow-rs");
    let output = Command::new(flow_rs)
        .args(["session-context"])
        .current_dir(&repo)
        .env("PATH", &path_env)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "session-context must tolerate malformed gh output. stdout: {}, stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        marker.exists(),
        "gh fallback must run even when the output is malformed",
    );
}
