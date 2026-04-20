//! CLI smoke test for `flow-rs upgrade-check`.
//!
//! Exercises the real binary end-to-end with a fake `gh` shell script on
//! PATH and a tempdir plugin.json via `FLOW_PLUGIN_JSON`. Uses
//! `Command::env()` (per-subprocess) to avoid parent-process env races
//! with parallel tests.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use flow_rs::upgrade_check::{
    resolve_plugin_json_path, resolve_plugin_json_path_with_root, resolve_timeout, run, run_gh_cmd,
    run_gh_with_command, upgrade_check_impl, Args, GhResult,
};
use serde_json::{json, Value};

fn flow_rs() -> Command {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
}

#[test]
fn cli_current_version_smoke() {
    let dir = tempfile::tempdir().unwrap();

    // Write a fake plugin.json with a known version + github repository.
    let plugin_json = dir.path().join("plugin.json");
    fs::write(
        &plugin_json,
        r#"{"version":"1.0.0","repository":"https://github.com/example/test"}"#,
    )
    .unwrap();

    // Create a fake `gh` shell script that prints the same version, making
    // this a "current version" scenario.
    let gh = dir.path().join("gh");
    fs::write(&gh, "#!/usr/bin/env bash\necho 'v1.0.0'\n").unwrap();
    let mut perms = fs::metadata(&gh).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&gh, perms).unwrap();

    // Prepend the tempdir to PATH so the fake `gh` wins over any real one.
    // Command::env() applies only to the spawned child — it does not leak
    // into the parent process, so concurrent tests are safe.
    let orig_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", dir.path().display(), orig_path);

    let output = flow_rs()
        .arg("upgrade-check")
        .env("FLOW_PLUGIN_JSON", &plugin_json)
        .env("PATH", new_path)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Parse the last non-empty stdout line as JSON.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last_line = stdout
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .unwrap_or_else(|| panic!("no stdout lines: {}", stdout));
    let data: Value = serde_json::from_str(last_line.trim())
        .unwrap_or_else(|e| panic!("JSON parse failed: {} (line: {:?})", e, last_line));

    assert_eq!(data["status"], "current");
    assert_eq!(data["installed"], "1.0.0");
}

// --- library-level tests for upgrade_check_impl ---

fn write_plugin_json(dir: &Path, content: &str) -> PathBuf {
    let path = dir.join("plugin.json");
    fs::write(&path, content).unwrap();
    path
}

#[test]
fn current_version() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = write_plugin_json(
        dir.path(),
        r#"{"version":"1.0.0","repository":"https://github.com/foo/bar"}"#,
    );
    let mut gh = |_owner_repo: &str, _t: u64| GhResult::Ok {
        returncode: 0,
        stdout: "v1.0.0".to_string(),
        stderr: String::new(),
    };
    let result = upgrade_check_impl(&plugin, 10, &mut gh);
    assert_eq!(result, json!({"status": "current", "installed": "1.0.0"}));
}

#[test]
fn upgrade_available() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = write_plugin_json(
        dir.path(),
        r#"{"version":"1.0.0","repository":"https://github.com/foo/bar"}"#,
    );
    let mut gh = |_owner_repo: &str, _t: u64| GhResult::Ok {
        returncode: 0,
        stdout: "v1.1.0".to_string(),
        stderr: String::new(),
    };
    let result = upgrade_check_impl(&plugin, 10, &mut gh);
    assert_eq!(
        result,
        json!({
            "status": "upgrade_available",
            "installed": "1.0.0",
            "latest": "1.1.0",
        })
    );
}

#[test]
fn gh_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = write_plugin_json(
        dir.path(),
        r#"{"version":"1.0.0","repository":"https://github.com/foo/bar"}"#,
    );
    let mut gh = |_owner_repo: &str, _t: u64| GhResult::NotFound;
    let result = upgrade_check_impl(&plugin, 10, &mut gh);
    assert_eq!(result["status"], "unknown");
    assert!(result["reason"].as_str().unwrap().contains("not found"));
}

#[test]
fn network_failure() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = write_plugin_json(
        dir.path(),
        r#"{"version":"1.0.0","repository":"https://github.com/foo/bar"}"#,
    );
    let mut gh = |_owner_repo: &str, _t: u64| GhResult::Ok {
        returncode: 1,
        stdout: String::new(),
        stderr: "connection refused".to_string(),
    };
    let result = upgrade_check_impl(&plugin, 10, &mut gh);
    assert_eq!(result["status"], "unknown");
    assert!(result["reason"].as_str().unwrap().contains("failed"));
}

#[test]
fn no_releases() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = write_plugin_json(
        dir.path(),
        r#"{"version":"1.0.0","repository":"https://github.com/foo/bar"}"#,
    );
    let mut gh = |_owner_repo: &str, _t: u64| GhResult::Ok {
        returncode: 0,
        stdout: String::new(),
        stderr: String::new(),
    };
    let result = upgrade_check_impl(&plugin, 10, &mut gh);
    assert_eq!(result["status"], "unknown");
    assert!(result["reason"].as_str().unwrap().contains("No releases"));
}

#[test]
fn malformed_tag() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = write_plugin_json(
        dir.path(),
        r#"{"version":"1.0.0","repository":"https://github.com/foo/bar"}"#,
    );
    let mut gh = |_owner_repo: &str, _t: u64| GhResult::Ok {
        returncode: 0,
        stdout: "not-a-version".to_string(),
        stderr: String::new(),
    };
    let result = upgrade_check_impl(&plugin, 10, &mut gh);
    assert_eq!(result["status"], "unknown");
    assert!(result["reason"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("parse"));
}

#[test]
fn no_repository_url() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = write_plugin_json(dir.path(), r#"{"version":"1.0.0"}"#);
    let mut gh = |_owner_repo: &str, _t: u64| -> GhResult {
        panic!("gh should not be called when repository is missing");
    };
    let result = upgrade_check_impl(&plugin, 10, &mut gh);
    assert_eq!(result["status"], "unknown");
    assert!(result["reason"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("repository"));
}

#[test]
fn timeout_returns_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = write_plugin_json(
        dir.path(),
        r#"{"version":"1.0.0","repository":"https://github.com/foo/bar"}"#,
    );
    let mut gh = |_owner_repo: &str, _t: u64| GhResult::Timeout;
    let result = upgrade_check_impl(&plugin, 10, &mut gh);
    assert_eq!(result["status"], "unknown");
    assert!(result["reason"].as_str().unwrap().contains("timed out"));
}

#[test]
fn malformed_installed_version_error_cites_installed() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = write_plugin_json(
        dir.path(),
        r#"{"version":"not-semver","repository":"https://github.com/foo/bar"}"#,
    );
    let mut gh = |_owner_repo: &str, _t: u64| GhResult::Ok {
        returncode: 0,
        stdout: "v1.0.0".to_string(),
        stderr: String::new(),
    };
    let result = upgrade_check_impl(&plugin, 10, &mut gh);
    assert_eq!(result["status"], "unknown");
    let reason = result["reason"].as_str().unwrap();
    assert!(reason.contains("not-semver"));
    assert!(!reason.contains("v1.0.0"));
}

#[test]
fn plugin_json_missing_returns_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("does-not-exist.json");
    let mut gh = |_: &str, _: u64| -> GhResult { panic!("gh must not be called") };
    let result = upgrade_check_impl(&missing, 10, &mut gh);
    assert_eq!(result["status"], "unknown");
    assert!(result["reason"].as_str().unwrap().contains("read"));
}

#[test]
fn plugin_json_invalid_returns_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = write_plugin_json(dir.path(), "not json at all");
    let mut gh = |_: &str, _: u64| -> GhResult { panic!("gh must not be called") };
    let result = upgrade_check_impl(&plugin, 10, &mut gh);
    assert_eq!(result["status"], "unknown");
    assert!(result["reason"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("invalid"));
}

#[test]
fn plugin_json_missing_version_returns_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = write_plugin_json(dir.path(), r#"{"repository":"https://github.com/foo/bar"}"#);
    let mut gh = |_: &str, _: u64| -> GhResult { panic!("gh must not be called") };
    let result = upgrade_check_impl(&plugin, 10, &mut gh);
    assert_eq!(result["status"], "unknown");
    assert!(result["reason"].as_str().unwrap().contains("version"));
}

// --- run_gh_with_command branch coverage ---

/// Happy path: a fake `gh` that exits 0 with stdout produces an Ok
/// result with the stdout contents.
#[test]
fn run_gh_with_command_ok_exit() {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "echo 'v1.2.3'"]);
    let result = run_gh_with_command(cmd, 10);
    match result {
        GhResult::Ok {
            returncode, stdout, ..
        } => {
            assert_eq!(returncode, 0);
            assert!(stdout.contains("v1.2.3"));
        }
        other => panic!("expected Ok, got {:?}", other),
    }
}

/// Non-zero exit flows through as Ok with the exit code preserved.
#[test]
fn run_gh_with_command_nonzero_exit() {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "exit 7"]);
    let result = run_gh_with_command(cmd, 10);
    match result {
        GhResult::Ok { returncode, .. } => assert_eq!(returncode, 7),
        other => panic!("expected Ok, got {:?}", other),
    }
}

/// Spawn failure (nonexistent binary) returns NotFound.
#[test]
fn run_gh_with_command_spawn_failure_returns_not_found() {
    let cmd = Command::new("/nonexistent/binary/zzz-definitely-not-there");
    let result = run_gh_with_command(cmd, 10);
    assert!(matches!(result, GhResult::NotFound));
}

/// Child that sleeps past the timeout is SIGKILLed and returns Timeout.
#[test]
fn run_gh_with_command_timeout_returns_timeout() {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "sleep 10"]);
    let start = std::time::Instant::now();
    let result = run_gh_with_command(cmd, 1);
    let elapsed = start.elapsed();
    assert!(matches!(result, GhResult::Timeout));
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "timeout took too long: {:?}",
        elapsed
    );
}

/// `run_gh_cmd` is the production binding over `run_gh_with_command`
/// with the gh-specific arg vector. We cannot guarantee `gh` is in
/// PATH in the test environment, so simply exercise that the function
/// returns SOME variant without panicking.
#[test]
fn run_gh_cmd_returns_some_variant() {
    let result = run_gh_cmd("foo/bar", 1);
    let _ = result;
}

/// Exercises the `status.code() == None` branch when the child exits
/// via signal. `kill -KILL $$` forces SIGKILL, which produces an
/// ExitStatus with no code.
#[test]
fn run_gh_with_command_signal_exit_returncode_neg_one() {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "kill -KILL $$"]);
    let result = run_gh_with_command(cmd, 10);
    match result {
        GhResult::Ok { returncode, .. } => {
            assert_eq!(returncode, -1, "signal-killed child must yield -1");
        }
        other => panic!("expected Ok variant, got {:?}", other),
    }
}

// --- resolve_* helpers ---

#[test]
fn resolve_plugin_json_path_with_env() {
    let p = resolve_plugin_json_path(Some("/custom/path/plugin.json".to_string()));
    assert_eq!(p, PathBuf::from("/custom/path/plugin.json"));
}

#[test]
fn resolve_plugin_json_path_fallback_uses_default_or_plugin_root() {
    let p = resolve_plugin_json_path(None);
    assert!(p.ends_with("plugin.json"));
}

#[test]
fn resolve_plugin_json_path_with_root_some() {
    let root = PathBuf::from("/custom/root");
    let p = resolve_plugin_json_path_with_root(None, Some(root));
    assert_eq!(p, PathBuf::from("/custom/root/.claude-plugin/plugin.json"));
}

#[test]
fn resolve_plugin_json_path_with_root_none_falls_back_to_relative_default() {
    let p = resolve_plugin_json_path_with_root(None, None);
    assert_eq!(p, PathBuf::from(".claude-plugin/plugin.json"));
}

#[test]
fn resolve_plugin_json_path_with_root_env_wins_over_root() {
    let p = resolve_plugin_json_path_with_root(
        Some("/override.json".to_string()),
        Some(PathBuf::from("/ignored")),
    );
    assert_eq!(p, PathBuf::from("/override.json"));
}

#[test]
fn resolve_timeout_with_valid_env() {
    assert_eq!(resolve_timeout(Some("42".to_string())), 42);
}

#[test]
fn resolve_timeout_with_invalid_env_returns_default() {
    assert_eq!(resolve_timeout(Some("not-a-number".to_string())), 10);
}

#[test]
fn resolve_timeout_with_no_env_returns_default() {
    assert_eq!(resolve_timeout(None), 10);
}

/// Calls the production `run()` entry point in-process. It reads env
/// vars FLOW_PLUGIN_JSON/FLOW_UPGRADE_TIMEOUT, resolves a plugin.json
/// path, calls upgrade_check_impl, and prints JSON. The test only
/// verifies the call doesn't panic — stdout output is ignored.
#[test]
fn run_entry_point_does_not_panic() {
    run(Args {});
}
