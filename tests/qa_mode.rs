//! Integration tests for `src/qa_mode.rs`.
//!
//! Covers the CLI wrapper surface that inline unit tests cannot reach:
//! `run()` process-exit paths, the `run_impl` branch that defaults
//! `flow_json_path` to `<project_root>/.flow.json`, and IO error
//! branches inside `start_impl` / `stop_impl` where the target path
//! is a directory or carries invalid JSON.
//!
//! Inline tests in `src/qa_mode.rs` already cover `start_impl`,
//! `stop_impl`, and `run_impl` happy and flag-missing paths with
//! explicit `--flow-json` values. This file covers the gaps those
//! tests cannot exercise in-process.

use std::fs;
use std::path::Path;
use std::process::Command;

use flow_rs::qa_mode::{self, run_impl, start_impl, stop_impl, Args};
use serde_json::{json, Value};

/// Subprocess: `bin/flow qa-mode --start` without `--local-path`.
/// Exercises `run()`'s `Ok(result)` arm when `run_impl` emits a
/// status=error response for the missing flag. The subprocess prints
/// the error JSON to stdout and exits 1 via `process::exit(1)`.
#[test]
fn qa_mode_cli_start_without_local_path_exits_nonzero_with_error_json() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let flow_json = root.join(".flow.json");
    fs::write(
        &flow_json,
        serde_json::to_string(&json!({
            "flow_version": "0.0.0",
            "plugin_root": "/some/path"
        }))
        .unwrap(),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "qa-mode",
            "--start",
            "--flow-json",
            flow_json.to_str().unwrap(),
        ])
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("failed to spawn flow-rs");

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit 1 on missing --local-path, got {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":\"error\""),
        "expected status=error in stdout, got: {}",
        stdout
    );
    assert!(
        stdout.contains("--local-path"),
        "expected '--local-path' mention in message, got: {}",
        stdout
    );
}

/// Subprocess: `bin/flow qa-mode --stop` happy round-trip. Drives the
/// `run()` `Ok(result)` arm when the result carries `status == "ok"`
/// and the function returns without calling `process::exit`. The
/// subprocess must exit 0.
#[test]
fn qa_mode_cli_stop_happy_path_exits_zero_and_restores_plugin_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let flow_json = root.join(".flow.json");
    fs::write(
        &flow_json,
        serde_json::to_string(&json!({
            "flow_version": "0.0.0",
            "plugin_root": "/local/dev/path",
            "plugin_root_backup": "/original/cache/path"
        }))
        .unwrap(),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "qa-mode",
            "--stop",
            "--flow-json",
            flow_json.to_str().unwrap(),
        ])
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("failed to spawn flow-rs");

    assert_eq!(
        output.status.code(),
        Some(0),
        "expected exit 0 on happy stop, got {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":\"ok\""),
        "expected status=ok in stdout, got: {}",
        stdout
    );

    let disk = fs::read_to_string(&flow_json).unwrap();
    let value: serde_json::Value = serde_json::from_str(&disk).unwrap();
    assert_eq!(value["plugin_root"], "/original/cache/path");
    assert!(value.get("plugin_root_backup").is_none());
}

/// Subprocess: `bin/flow qa-mode --stop` when the `.flow.json` path
/// is missing, exercising `run()` error-exit path where `stop_impl`
/// returns the missing-file error. Complements the inline
/// `test_stop_missing_flow_json` which drives `stop_impl` directly
/// without going through the CLI wrapper.
#[test]
fn qa_mode_cli_stop_missing_flow_json_exits_nonzero_with_error_json() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let missing_flow_json = root.join("nowhere").join(".flow.json");

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "qa-mode",
            "--stop",
            "--flow-json",
            missing_flow_json.to_str().unwrap(),
        ])
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("failed to spawn flow-rs");

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit 1 on missing .flow.json, got {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":\"error\""),
        "expected status=error in stdout, got: {}",
        stdout
    );
}

/// Subprocess: `run_impl` defaults `flow_json_path` to
/// `<project_root>/.flow.json` when `--flow-json` is omitted. Spawns
/// the binary with `current_dir` set to a tempdir containing a git
/// repo and a seeded `.flow.json`, so `project_root()` resolves to
/// that tempdir and the default path branch fires. Exercises the
/// `args.flow_json.is_none()` branch of `run_impl` that inline
/// tests cannot reach because they always pass an explicit path.
#[test]
fn qa_mode_cli_default_flow_json_resolves_to_project_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();

    // Minimal git repo so `project_root()` recognizes this tempdir.
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(&root)
        .output()
        .expect("git init failed");

    let flow_json = root.join(".flow.json");
    fs::write(
        &flow_json,
        serde_json::to_string(&json!({
            "flow_version": "0.0.0",
            "plugin_root": "/original/cache",
            "plugin_root_backup": "/backup/path"
        }))
        .unwrap(),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["qa-mode", "--stop"])
        .current_dir(&root)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("failed to spawn flow-rs");

    assert_eq!(
        output.status.code(),
        Some(0),
        "expected exit 0 with default flow_json resolution, got {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let disk = fs::read_to_string(&flow_json).unwrap();
    let value: serde_json::Value = serde_json::from_str(&disk).unwrap();
    assert_eq!(
        value["plugin_root"], "/backup/path",
        "expected restored plugin_root after stop via default path"
    );
}

/// Library-level: `start_impl` with a `.flow.json` path that is a
/// directory rather than a file. `read_to_string` fails, and the
/// error branch returns `status=error` with a `read` failure message.
/// Inline tests cover parse-failure via `test_start_missing_plugin_root`
/// and similar; this test covers the `read_to_string` Err branch
/// specifically.
#[test]
fn qa_mode_start_impl_flow_json_is_directory_returns_read_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    // Create the ".flow.json" path as a directory so read_to_string
    // fails with EISDIR. The file's `exists()` guard passes (a
    // directory "exists") so control flow reaches read_to_string.
    let flow_json_as_dir = root.join(".flow.json");
    fs::create_dir(&flow_json_as_dir).unwrap();

    let local_source = root.join("flow-source");
    fs::create_dir_all(local_source.join("bin")).unwrap();
    fs::write(local_source.join("bin").join("flow"), "#!/bin/bash\n").unwrap();

    let result = qa_mode::start_impl(&flow_json_as_dir, &local_source);
    assert_eq!(result["status"], "error");
    let message = result["message"].as_str().expect("error carries message");
    assert!(
        message.to_lowercase().contains("read")
            || message.to_lowercase().contains("is a directory"),
        "expected read-error in message, got: {}",
        message
    );
}

/// Library-level: `start_impl` with a `.flow.json` that contains
/// invalid JSON. `serde_json::from_str` fails, and the error branch
/// returns `status=error` with a parse-failure message.
#[test]
fn qa_mode_start_impl_invalid_json_returns_parse_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let flow_json = root.join(".flow.json");
    fs::write(&flow_json, "{not valid json").unwrap();

    let local_source = root.join("flow-source");
    fs::create_dir_all(local_source.join("bin")).unwrap();
    fs::write(local_source.join("bin").join("flow"), "#!/bin/bash\n").unwrap();

    let result = qa_mode::start_impl(&flow_json, &local_source);
    assert_eq!(result["status"], "error");
    let message = result["message"].as_str().expect("error carries message");
    assert!(
        message.to_lowercase().contains("parse") || message.to_lowercase().contains("expected"),
        "expected parse-error in message, got: {}",
        message
    );
}

/// Library-level: `stop_impl` with a `.flow.json` path that is a
/// directory rather than a file — `read_to_string` fails, and the
/// error branch returns `status=error` with a read failure message.
#[test]
fn qa_mode_stop_impl_flow_json_is_directory_returns_read_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let flow_json_as_dir = root.join(".flow.json");
    fs::create_dir(&flow_json_as_dir).unwrap();

    let result = qa_mode::stop_impl(&flow_json_as_dir);
    assert_eq!(result["status"], "error");
    let message = result["message"].as_str().expect("error carries message");
    assert!(
        message.to_lowercase().contains("read")
            || message.to_lowercase().contains("is a directory"),
        "expected read-error in message, got: {}",
        message
    );
}

/// Library-level: `stop_impl` with a `.flow.json` that contains
/// invalid JSON. `serde_json::from_str` fails, returning
/// `status=error` with a parse-failure message.
#[test]
fn qa_mode_stop_impl_invalid_json_returns_parse_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let flow_json = root.join(".flow.json");
    fs::write(&flow_json, "not json at all").unwrap();

    let result = qa_mode::stop_impl(&flow_json);
    assert_eq!(result["status"], "error");
    let message = result["message"].as_str().expect("error carries message");
    assert!(
        message.to_lowercase().contains("parse") || message.to_lowercase().contains("expected"),
        "expected parse-error in message, got: {}",
        message
    );
}

// --- library-level tests migrated from inline mod ---

fn write_flow_json(path: &Path, data: &Value) {
    fs::write(path, serde_json::to_string(data).unwrap() + "\n").unwrap();
}

fn read_flow_json(path: &Path) -> Value {
    let content = fs::read_to_string(path).unwrap();
    serde_json::from_str(&content).unwrap()
}

// --- start_impl ---

#[test]
fn test_start_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    let flow_json = dir.path().join(".flow.json");
    let local_source = dir.path().join("flow-source");
    fs::create_dir_all(local_source.join("bin")).unwrap();
    fs::write(local_source.join("bin").join("flow"), "#!/bin/bash\n").unwrap();

    write_flow_json(
        &flow_json,
        &json!({
            "flow_version": "0.39.0",
            "commit_format": "full",
            "plugin_root": "/original/cache/path"
        }),
    );

    let result = start_impl(&flow_json, &local_source);

    assert_eq!(result["status"], "ok");
    assert_eq!(
        result["plugin_root"],
        local_source.to_string_lossy().as_ref()
    );
    assert_eq!(result["backup"], "/original/cache/path");

    let data = read_flow_json(&flow_json);
    assert_eq!(data["plugin_root"], local_source.to_string_lossy().as_ref());
    assert_eq!(data["plugin_root_backup"], "/original/cache/path");
}

#[test]
fn test_start_missing_flow_json() {
    let dir = tempfile::tempdir().unwrap();
    let flow_json = dir.path().join(".flow.json");
    let local_source = dir.path().join("flow-source");

    let result = start_impl(&flow_json, &local_source);

    assert_eq!(result["status"], "error");
    let msg = result["message"].as_str().unwrap().to_lowercase();
    assert!(msg.contains("not found") || msg.contains("does not exist"));
}

#[test]
fn test_start_write_failure_returns_error() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let flow_json = dir.path().join(".flow.json");
    let local_source = dir.path().join("flow-source");
    fs::create_dir_all(local_source.join("bin")).unwrap();
    fs::write(local_source.join("bin").join("flow"), "#!/bin/bash\n").unwrap();
    write_flow_json(
        &flow_json,
        &json!({
            "plugin_root": "/some/path",
            "flow_version": "0.39.0",
            "commit_format": "full"
        }),
    );
    let mut perms = fs::metadata(&flow_json).unwrap().permissions();
    perms.set_mode(0o444);
    fs::set_permissions(&flow_json, perms).unwrap();

    let result = start_impl(&flow_json, &local_source);

    let mut p = fs::metadata(&flow_json).unwrap().permissions();
    p.set_mode(0o644);
    let _ = fs::set_permissions(&flow_json, p);

    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("Failed to write"));
}

#[test]
fn test_stop_write_failure_returns_error() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let flow_json = dir.path().join(".flow.json");
    write_flow_json(
        &flow_json,
        &json!({
            "plugin_root": "/dev/source",
            "plugin_root_backup": "/original/path",
            "flow_version": "0.39.0"
        }),
    );
    let mut perms = fs::metadata(&flow_json).unwrap().permissions();
    perms.set_mode(0o444);
    fs::set_permissions(&flow_json, perms).unwrap();

    let result = stop_impl(&flow_json);

    let mut p = fs::metadata(&flow_json).unwrap().permissions();
    p.set_mode(0o644);
    let _ = fs::set_permissions(&flow_json, p);

    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("Failed to write"));
}

#[test]
fn test_start_missing_plugin_root() {
    let dir = tempfile::tempdir().unwrap();
    let flow_json = dir.path().join(".flow.json");
    let local_source = dir.path().join("flow-source");
    fs::create_dir_all(local_source.join("bin")).unwrap();
    fs::write(local_source.join("bin").join("flow"), "#!/bin/bash\n").unwrap();

    write_flow_json(
        &flow_json,
        &json!({"flow_version": "0.39.0", "commit_format": "full"}),
    );

    let result = start_impl(&flow_json, &local_source);
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("plugin_root"));
}

#[test]
fn test_start_already_in_dev_mode() {
    let dir = tempfile::tempdir().unwrap();
    let flow_json = dir.path().join(".flow.json");
    let local_source = dir.path().join("flow-source");
    fs::create_dir_all(local_source.join("bin")).unwrap();
    fs::write(local_source.join("bin").join("flow"), "#!/bin/bash\n").unwrap();

    write_flow_json(
        &flow_json,
        &json!({
            "flow_version": "0.39.0",
            "plugin_root": "/some/path",
            "plugin_root_backup": "/original/path"
        }),
    );

    let result = start_impl(&flow_json, &local_source);
    assert_eq!(result["status"], "error");
    let msg = result["message"].as_str().unwrap().to_lowercase();
    assert!(msg.contains("already") || msg.contains("dev mode"));
}

#[test]
fn test_start_invalid_local_path_not_exists() {
    let dir = tempfile::tempdir().unwrap();
    let flow_json = dir.path().join(".flow.json");

    write_flow_json(
        &flow_json,
        &json!({"flow_version": "0.39.0", "plugin_root": "/original/path"}),
    );

    let result = start_impl(&flow_json, &dir.path().join("nonexistent"));
    assert_eq!(result["status"], "error");
    let msg = result["message"].as_str().unwrap().to_lowercase();
    assert!(msg.contains("not found") || msg.contains("does not exist"));
}

#[test]
fn test_start_invalid_local_path_no_bin_flow() {
    let dir = tempfile::tempdir().unwrap();
    let flow_json = dir.path().join(".flow.json");
    let local_source = dir.path().join("flow-source");
    fs::create_dir(&local_source).unwrap();

    write_flow_json(
        &flow_json,
        &json!({"flow_version": "0.39.0", "plugin_root": "/original/path"}),
    );

    let result = start_impl(&flow_json, &local_source);
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("bin/flow"));
}

#[test]
fn test_start_preserves_other_keys() {
    let dir = tempfile::tempdir().unwrap();
    let flow_json = dir.path().join(".flow.json");
    let local_source = dir.path().join("flow-source");
    fs::create_dir_all(local_source.join("bin")).unwrap();
    fs::write(local_source.join("bin").join("flow"), "#!/bin/bash\n").unwrap();

    write_flow_json(
        &flow_json,
        &json!({
            "flow_version": "0.39.0",
            "extra_unknown_field": "preserve me",
            "config_hash": "abc123",
            "setup_hash": "def456",
            "commit_format": "conventional",
            "plugin_root": "/original/cache/path",
            "skills": {"flow-start": {"continue": "auto"}}
        }),
    );

    start_impl(&flow_json, &local_source);

    let data = read_flow_json(&flow_json);
    assert_eq!(data["flow_version"], "0.39.0");
    assert_eq!(data["extra_unknown_field"], "preserve me");
    assert_eq!(data["config_hash"], "abc123");
    assert_eq!(data["setup_hash"], "def456");
    assert_eq!(data["commit_format"], "conventional");
    assert_eq!(data["skills"]["flow-start"]["continue"], "auto");
    assert_eq!(data["plugin_root"], local_source.to_string_lossy().as_ref());
    assert_eq!(data["plugin_root_backup"], "/original/cache/path");
}

// --- stop_impl ---

#[test]
fn test_stop_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    let flow_json = dir.path().join(".flow.json");

    write_flow_json(
        &flow_json,
        &json!({
            "flow_version": "0.39.0",
            "plugin_root": "/local/dev/path",
            "plugin_root_backup": "/original/cache/path"
        }),
    );

    let result = stop_impl(&flow_json);
    assert_eq!(result["status"], "ok");
    assert_eq!(result["restored"], "/original/cache/path");

    let data = read_flow_json(&flow_json);
    assert_eq!(data["plugin_root"], "/original/cache/path");
    assert!(data.get("plugin_root_backup").is_none());
}

#[test]
fn test_stop_not_in_dev_mode() {
    let dir = tempfile::tempdir().unwrap();
    let flow_json = dir.path().join(".flow.json");

    write_flow_json(
        &flow_json,
        &json!({"flow_version": "0.39.0", "plugin_root": "/some/path"}),
    );

    let result = stop_impl(&flow_json);
    assert_eq!(result["status"], "error");
    let msg = result["message"].as_str().unwrap().to_lowercase();
    assert!(msg.contains("not in dev mode") || msg.contains("backup"));
}

#[test]
fn test_stop_missing_flow_json() {
    let dir = tempfile::tempdir().unwrap();
    let flow_json = dir.path().join(".flow.json");

    let result = stop_impl(&flow_json);
    assert_eq!(result["status"], "error");
    let msg = result["message"].as_str().unwrap().to_lowercase();
    assert!(msg.contains("not found") || msg.contains("does not exist"));
}

#[test]
fn test_stop_preserves_other_keys() {
    let dir = tempfile::tempdir().unwrap();
    let flow_json = dir.path().join(".flow.json");

    write_flow_json(
        &flow_json,
        &json!({
            "flow_version": "0.39.0",
            "commit_format": "full",
            "config_hash": "abc123",
            "plugin_root": "/local/dev/path",
            "plugin_root_backup": "/original/cache/path",
            "skills": {"flow-code": {"commit": "auto"}}
        }),
    );

    stop_impl(&flow_json);

    let data = read_flow_json(&flow_json);
    assert_eq!(data["flow_version"], "0.39.0");
    assert_eq!(data["commit_format"], "full");
    assert_eq!(data["config_hash"], "abc123");
    assert_eq!(data["skills"]["flow-code"]["commit"], "auto");
    assert_eq!(data["plugin_root"], "/original/cache/path");
    assert!(data.get("plugin_root_backup").is_none());
}

// --- run_impl ---

#[test]
fn test_cli_start_missing_local_path() {
    let dir = tempfile::tempdir().unwrap();
    let flow_json = dir.path().join(".flow.json");
    write_flow_json(
        &flow_json,
        &json!({"flow_version": "0.39.0", "plugin_root": "/p"}),
    );

    let args = Args {
        start: true,
        stop: false,
        local_path: None,
        flow_json: Some(flow_json.to_string_lossy().to_string()),
    };

    let result = run_impl(&args).unwrap();
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("--local-path"));
}

#[test]
fn test_cli_start_happy_path_lib() {
    let dir = tempfile::tempdir().unwrap();
    let flow_json = dir.path().join(".flow.json");
    write_flow_json(
        &flow_json,
        &json!({"flow_version": "0.39.0", "plugin_root": "/original/path"}),
    );
    let local_source = dir.path().join("local-flow");
    fs::create_dir_all(local_source.join("bin")).unwrap();
    fs::write(local_source.join("bin").join("flow"), "").unwrap();

    let args = Args {
        start: true,
        stop: false,
        local_path: Some(local_source.to_string_lossy().to_string()),
        flow_json: Some(flow_json.to_string_lossy().to_string()),
    };

    let result = run_impl(&args).unwrap();
    assert_eq!(result["status"], "ok");
    let disk = read_flow_json(&flow_json);
    assert_eq!(disk["plugin_root_backup"], "/original/path");
}

#[test]
fn test_cli_stop_happy_path_lib() {
    let dir = tempfile::tempdir().unwrap();
    let flow_json = dir.path().join(".flow.json");
    write_flow_json(
        &flow_json,
        &json!({
            "flow_version": "0.39.0",
            "plugin_root": "/local/dev",
            "plugin_root_backup": "/original"
        }),
    );

    let args = Args {
        start: false,
        stop: true,
        local_path: None,
        flow_json: Some(flow_json.to_string_lossy().to_string()),
    };

    let result = run_impl(&args).unwrap();
    assert_eq!(result["status"], "ok");
    let disk = read_flow_json(&flow_json);
    assert_eq!(disk["plugin_root"], "/original");
    assert!(disk.get("plugin_root_backup").is_none());
}

#[test]
fn test_cli_stop_when_not_in_dev_mode_errors() {
    let dir = tempfile::tempdir().unwrap();
    let flow_json = dir.path().join(".flow.json");
    write_flow_json(
        &flow_json,
        &json!({"flow_version": "0.39.0", "plugin_root": "/original"}),
    );

    let args = Args {
        start: false,
        stop: true,
        local_path: None,
        flow_json: Some(flow_json.to_string_lossy().to_string()),
    };

    let result = run_impl(&args).unwrap();
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("Not in dev mode"));
}
