//! Integration tests for `flow-rs prime-check`.
//!
//! Points the Rust binary at the real plugin via
//! CLAUDE_PLUGIN_ROOT=CARGO_MANIFEST_DIR so plugin.json version and the
//! real `src/prime_setup.rs` bytes are used for hash computation. All
//! subprocess calls use Command::output() to avoid leaking child
//! output to the test harness.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

fn plugin_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn flow_rs() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.env("CLAUDE_PLUGIN_ROOT", plugin_root());
    cmd
}

fn parse_stdout(stdout: &[u8]) -> Value {
    let text = String::from_utf8_lossy(stdout);
    let last_line = text
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .unwrap_or_else(|| panic!("no stdout lines: {:?}", text));
    serde_json::from_str(last_line.trim())
        .unwrap_or_else(|e| panic!("JSON parse failed: {} (line: {:?})", e, last_line))
}

fn current_plugin_version() -> String {
    let plugin_json_path = plugin_root().join(".claude-plugin").join("plugin.json");
    let content = fs::read_to_string(&plugin_json_path).unwrap();
    let data: Value = serde_json::from_str(&content).unwrap();
    data["version"].as_str().unwrap().to_string()
}

fn run_prime_check(cwd: &Path) -> (Value, i32) {
    let output = flow_rs()
        .arg("prime-check")
        .current_dir(cwd)
        .output()
        .unwrap();
    let value = parse_stdout(&output.stdout);
    let code = output.status.code().unwrap_or(-1);
    (value, code)
}

fn write_flow_json(dir: &Path, data: Value) {
    fs::write(
        dir.join(".flow.json"),
        serde_json::to_string(&data).unwrap(),
    )
    .unwrap();
}

// --- Basic error and happy-path tests ---

#[test]
fn fails_when_flow_json_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let (data, code) = run_prime_check(tmp.path());
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap()
        .contains("/flow:flow-prime"));
    assert_eq!(code, 0);
}

#[test]
fn fails_when_flow_version_mismatch_no_hashes() {
    let tmp = tempfile::tempdir().unwrap();
    write_flow_json(tmp.path(), json!({"flow_version": "0.0.0"}));
    let (data, code) = run_prime_check(tmp.path());
    assert_eq!(data["status"], "error");
    assert!(data["message"].as_str().unwrap().contains("mismatch"));
    assert_eq!(code, 0);
}

#[test]
fn happy_path_minimal() {
    // A minimal version-only marker is sufficient for prime-check
    // to return ok. .flow.json no longer requires any other fields.
    let tmp = tempfile::tempdir().unwrap();
    let version = current_plugin_version();
    write_flow_json(tmp.path(), json!({"flow_version": version}));
    let (data, code) = run_prime_check(tmp.path());
    assert_eq!(data["status"], "ok");
    assert_eq!(code, 0);
}

#[test]
fn happy_path_unknown_legacy_keys_ignored() {
    // Tombstone for the legacy `framework` key in `.flow.json`. Older
    // versions wrote rails/python/ios/go/rust here; current consumers
    // must ignore the key cleanly so an upgrade does not require a
    // re-prime. This test pins that contract by feeding the key in
    // and asserting prime-check still returns ok.
    let tmp = tempfile::tempdir().unwrap();
    let version = current_plugin_version();
    write_flow_json(
        tmp.path(),
        json!({"flow_version": version, "framework": "rails"}),
    );
    let (data, code) = run_prime_check(tmp.path());
    assert_eq!(data["status"], "ok");
    assert_eq!(code, 0);
}

// --- Auto-upgrade path tests ---
//
// These tests use the Rust public API (compute_config_hash /
// compute_setup_hash) to build the "stored" hashes so the Rust binary
// can verify them. This is a self-consistency test — the hashes built
// here must match what prime-check computes at runtime.

fn computed_config_hash() -> String {
    flow_rs::prime_check::compute_config_hash()
}

fn computed_setup_hash() -> String {
    flow_rs::prime_check::compute_setup_hash(&plugin_root()).unwrap()
}

#[test]
fn auto_upgrades_when_both_hashes_match() {
    let tmp = tempfile::tempdir().unwrap();
    let config_hash = computed_config_hash();
    let setup_hash = computed_setup_hash();
    write_flow_json(
        tmp.path(),
        json!({
            "flow_version": "0.0.1",
            "config_hash": config_hash,
            "setup_hash": setup_hash,
        }),
    );
    let (data, _code) = run_prime_check(tmp.path());
    assert_eq!(data["status"], "ok");
    assert_eq!(data["auto_upgraded"], true);
    assert_eq!(data["old_version"], "0.0.1");
    assert_eq!(data["new_version"], current_plugin_version());
}

#[test]
fn auto_upgrade_updates_version_in_file() {
    let tmp = tempfile::tempdir().unwrap();
    let config_hash = computed_config_hash();
    let setup_hash = computed_setup_hash();
    write_flow_json(
        tmp.path(),
        json!({
            "flow_version": "0.0.1",
            "config_hash": config_hash,
            "setup_hash": setup_hash,
        }),
    );
    run_prime_check(tmp.path());
    let updated: Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join(".flow.json")).unwrap()).unwrap();
    assert_eq!(updated["flow_version"], current_plugin_version());
}

#[test]
fn auto_upgrade_preserves_existing_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let config_hash = computed_config_hash();
    let setup_hash = computed_setup_hash();
    let skills = json!({"flow-start": {"continue": "auto"}});
    write_flow_json(
        tmp.path(),
        json!({
            "flow_version": "0.0.1",
            "config_hash": config_hash,
            "setup_hash": setup_hash,
            "skills": skills,
        }),
    );
    run_prime_check(tmp.path());
    let updated: Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join(".flow.json")).unwrap()).unwrap();
    assert_eq!(updated["config_hash"], config_hash);
    assert_eq!(updated["setup_hash"], setup_hash);
    assert_eq!(
        updated["skills"],
        json!({"flow-start": {"continue": "auto"}})
    );
}

#[test]
fn requires_reinit_when_config_hash_missing() {
    let tmp = tempfile::tempdir().unwrap();
    write_flow_json(
        tmp.path(),
        json!({
            "flow_version": "0.0.1",
        }),
    );
    let (data, _code) = run_prime_check(tmp.path());
    assert_eq!(data["status"], "error");
    assert!(data["message"].as_str().unwrap().contains("mismatch"));
}

#[test]
fn requires_reinit_when_config_hash_mismatches() {
    let tmp = tempfile::tempdir().unwrap();
    let setup_hash = computed_setup_hash();
    write_flow_json(
        tmp.path(),
        json!({
            "flow_version": "0.0.1",
            "config_hash": "000000000000",
            "setup_hash": setup_hash,
        }),
    );
    let (data, _code) = run_prime_check(tmp.path());
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap()
        .contains("/flow:flow-prime"));
}

#[test]
fn requires_reinit_when_setup_hash_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let config_hash = computed_config_hash();
    write_flow_json(
        tmp.path(),
        json!({
            "flow_version": "0.0.1",
            "config_hash": config_hash,
        }),
    );
    let (data, _code) = run_prime_check(tmp.path());
    assert_eq!(data["status"], "error");
    assert!(data["message"].as_str().unwrap().contains("mismatch"));
}

#[test]
fn requires_reinit_when_setup_hash_mismatches() {
    let tmp = tempfile::tempdir().unwrap();
    let config_hash = computed_config_hash();
    write_flow_json(
        tmp.path(),
        json!({
            "flow_version": "0.0.1",
            "config_hash": config_hash,
            "setup_hash": "000000000000",
        }),
    );
    let (data, _code) = run_prime_check(tmp.path());
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap()
        .contains("/flow:flow-prime"));
}

// --- Infrastructure-failure branches in run_impl ---

/// Subprocess: `prime-check` when `CLAUDE_PLUGIN_ROOT` points at a
/// directory that has no `.claude-plugin/plugin.json`. Exercises the
/// `fs::read_to_string` Err branch inside `run_impl`, which produces
/// a structured error rather than panicking.
#[test]
fn prime_check_reports_missing_plugin_json_via_subprocess() {
    let tmp = tempfile::tempdir().unwrap();
    let bogus_plugin = tempfile::tempdir().unwrap();
    // plugin_root exists but has no .claude-plugin/plugin.json.
    write_flow_json(
        tmp.path(),
        json!({
            "flow_version": "0.0.1",
            "config_hash": "x",
            "setup_hash": "y",
        }),
    );

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("prime-check")
        .current_dir(tmp.path())
        .env("CLAUDE_PLUGIN_ROOT", bogus_plugin.path())
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();

    // Infrastructure failure is printed to stdout as status=error.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":\"error\""),
        "expected status=error for missing plugin.json, got: {}",
        stdout
    );
}

/// Subprocess: `prime-check` when plugin.json exists but cannot be
/// parsed as JSON. Exercises the `serde_json::from_str` Err branch in
/// `run_impl`.
#[test]
fn prime_check_reports_malformed_plugin_json_via_subprocess() {
    let tmp = tempfile::tempdir().unwrap();
    let bogus_plugin = tempfile::tempdir().unwrap();
    fs::create_dir_all(bogus_plugin.path().join(".claude-plugin")).unwrap();
    fs::write(
        bogus_plugin
            .path()
            .join(".claude-plugin")
            .join("plugin.json"),
        "{not valid json",
    )
    .unwrap();
    write_flow_json(
        tmp.path(),
        json!({
            "flow_version": "0.0.1",
            "config_hash": "x",
            "setup_hash": "y",
        }),
    );

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("prime-check")
        .current_dir(tmp.path())
        .env("CLAUDE_PLUGIN_ROOT", bogus_plugin.path())
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":\"error\""),
        "expected status=error for malformed plugin.json, got: {}",
        stdout
    );
}

/// Subprocess: `prime-check` when plugin.json is valid JSON but
/// missing the `version` field. Exercises the
/// `ok_or_else("plugin.json missing version")` branch in `run_impl`.
#[test]
fn prime_check_reports_missing_plugin_version_via_subprocess() {
    let tmp = tempfile::tempdir().unwrap();
    let bogus_plugin = tempfile::tempdir().unwrap();
    fs::create_dir_all(bogus_plugin.path().join(".claude-plugin")).unwrap();
    fs::write(
        bogus_plugin
            .path()
            .join(".claude-plugin")
            .join("plugin.json"),
        r#"{"name": "flow"}"#,
    )
    .unwrap();
    write_flow_json(
        tmp.path(),
        json!({
            "flow_version": "0.0.1",
            "config_hash": "x",
            "setup_hash": "y",
        }),
    );

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("prime-check")
        .current_dir(tmp.path())
        .env("CLAUDE_PLUGIN_ROOT", bogus_plugin.path())
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":\"error\""),
        "expected status=error for plugin.json missing version, got: {}",
        stdout
    );
    assert!(
        stdout.contains("version"),
        "expected 'version' in message, got: {}",
        stdout
    );
}

// --- run_impl_main (main-arm dispatch seam) ---

#[test]
fn run_impl_main_none_plugin_root_returns_error_and_exit_one() {
    let tmp = tempfile::tempdir().unwrap();
    let (value, code) = flow_rs::prime_check::run_impl_main(tmp.path(), None);
    assert_eq!(value["status"], "error");
    assert_eq!(value["message"].as_str().unwrap(), "Plugin root not found");
    assert_eq!(code, 1);
}

#[test]
fn run_impl_main_ok_returns_value_and_exit_zero() {
    let tmp = tempfile::tempdir().unwrap();
    write_flow_json(
        tmp.path(),
        json!({"flow_version": current_plugin_version()}),
    );
    let (value, code) = flow_rs::prime_check::run_impl_main(tmp.path(), Some(plugin_root()));
    assert_eq!(value["status"], "ok");
    assert_eq!(code, 0);
}

#[test]
fn run_impl_main_err_returns_error_and_exit_one() {
    // plugin_root points at a dir without .claude-plugin/plugin.json
    // so run_impl returns Err and run_impl_main wraps it as
    // status=error exit 1.
    let tmp = tempfile::tempdir().unwrap();
    let bogus_plugin = tempfile::tempdir().unwrap();
    write_flow_json(
        tmp.path(),
        json!({
            "flow_version": "0.0.1",
            "config_hash": "x",
            "setup_hash": "y",
        }),
    );
    let (value, code) =
        flow_rs::prime_check::run_impl_main(tmp.path(), Some(bogus_plugin.path().to_path_buf()));
    assert_eq!(value["status"], "error");
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Could not read"));
    assert_eq!(code, 1);
}

// --- run_impl error-propagation branches ---
//
// The subprocess tests above also exercise these paths through the
// real CLI, but cargo-llvm-cov does not always attribute subprocess
// coverage back to the library code. Direct library-level tests make
// the coverage for each `?` branch in run_impl explicit and visible
// to the per-file gate.

#[test]
fn run_impl_errors_when_plugin_json_unreadable() {
    // plugin_root is a tempdir with no .claude-plugin/plugin.json.
    // The first `?` in run_impl propagates the fs::read_to_string Err.
    let tmp = tempfile::tempdir().unwrap();
    let bogus_plugin = tempfile::tempdir().unwrap();
    write_flow_json(tmp.path(), json!({"flow_version": "0.0.1"}));
    let err = flow_rs::prime_check::run_impl(tmp.path(), bogus_plugin.path()).unwrap_err();
    assert!(err.contains("Could not read"), "got: {}", err);
}

#[test]
fn run_impl_errors_when_plugin_json_malformed() {
    // plugin.json exists but is not valid JSON.
    let tmp = tempfile::tempdir().unwrap();
    let bogus_plugin = tempfile::tempdir().unwrap();
    fs::create_dir_all(bogus_plugin.path().join(".claude-plugin")).unwrap();
    fs::write(
        bogus_plugin
            .path()
            .join(".claude-plugin")
            .join("plugin.json"),
        "{not valid json",
    )
    .unwrap();
    write_flow_json(tmp.path(), json!({"flow_version": "0.0.1"}));
    let err = flow_rs::prime_check::run_impl(tmp.path(), bogus_plugin.path()).unwrap_err();
    assert!(err.contains("Could not parse plugin.json"), "got: {}", err);
}

#[test]
fn run_impl_errors_when_plugin_json_missing_version() {
    // plugin.json is valid JSON but lacks the `version` field.
    let tmp = tempfile::tempdir().unwrap();
    let bogus_plugin = tempfile::tempdir().unwrap();
    fs::create_dir_all(bogus_plugin.path().join(".claude-plugin")).unwrap();
    fs::write(
        bogus_plugin
            .path()
            .join(".claude-plugin")
            .join("plugin.json"),
        r#"{"name": "flow"}"#,
    )
    .unwrap();
    write_flow_json(tmp.path(), json!({"flow_version": "0.0.1"}));
    let err = flow_rs::prime_check::run_impl(tmp.path(), bogus_plugin.path()).unwrap_err();
    assert!(err.contains("missing version"), "got: {}", err);
}

// --- compute_setup_hash error branch ---

#[test]
fn compute_setup_hash_errors_when_prime_setup_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let err = flow_rs::prime_check::compute_setup_hash(tmp.path()).unwrap_err();
    assert!(err.contains("Could not read"), "got: {}", err);
}

#[test]
fn run_impl_propagates_compute_setup_hash_error_in_mismatch_path() {
    // Version-mismatch path calls compute_setup_hash(plugin_root). When
    // the plugin_root has a valid plugin.json but no src/prime_setup.rs,
    // compute_setup_hash returns Err and run_impl's `?` propagates it.
    let tmp = tempfile::tempdir().unwrap();
    let bogus_plugin = tempfile::tempdir().unwrap();
    fs::create_dir_all(bogus_plugin.path().join(".claude-plugin")).unwrap();
    fs::write(
        bogus_plugin
            .path()
            .join(".claude-plugin")
            .join("plugin.json"),
        r#"{"version": "9.9.9-synthetic"}"#,
    )
    .unwrap();
    write_flow_json(tmp.path(), json!({"flow_version": "0.0.1-prior"}));

    let err = flow_rs::prime_check::run_impl(tmp.path(), bogus_plugin.path()).unwrap_err();
    assert!(err.contains("Could not read"), "got: {}", err);
    assert!(err.contains("prime_setup.rs"), "got: {}", err);
}

#[test]
fn run_impl_propagates_write_error_in_auto_upgrade_path() {
    // Auto-upgrade writes the updated flow_version back to .flow.json.
    // When .flow.json is a directory, not a file, fs::write fails and
    // the `?` operator propagates the error.
    let tmp = tempfile::tempdir().unwrap();
    // Set up a valid auto-upgrade-ready .flow.json with matching hashes.
    // But .flow.json is a directory, not a file — fs::read_to_string
    // returns Err, so run_impl short-circuits at the "not initialized"
    // branch instead. Put a readable .flow.json inside a nested tempdir
    // and then make the PARENT read-only so fs::write fails on the new
    // file creation.
    let config_hash = flow_rs::prime_check::compute_config_hash();
    let setup_hash = flow_rs::prime_check::compute_setup_hash(&plugin_root()).unwrap();

    // Write .flow.json normally, then swap it for a directory. This way
    // read_flow_json succeeds (we pre-populate the content into memory
    // via JSON-in-Rust setup) — but we need read_flow_json to actually
    // read the bytes. Instead, use a read-only filesystem approach: make
    // .flow.json non-writable AND add a second blocker so the Rust
    // fs::write call fails when it tries to truncate+write.
    write_flow_json(
        tmp.path(),
        json!({
            "flow_version": "0.0.1-prior",
            "config_hash": config_hash,
            "setup_hash": setup_hash,
        }),
    );

    // Remove write permission from .flow.json itself so fs::write fails
    // on the open(O_WRONLY|O_TRUNC) syscall. Read permission is kept so
    // read_flow_json succeeds earlier in run_impl.
    use std::os::unix::fs::PermissionsExt;
    let path = tmp.path().join(".flow.json");
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o444);
    fs::set_permissions(&path, perms).unwrap();

    let result = flow_rs::prime_check::run_impl(tmp.path(), &plugin_root());

    // Restore perms so tempdir cleanup works cleanly.
    let mut rperms = fs::metadata(&path).unwrap().permissions();
    rperms.set_mode(0o644);
    fs::set_permissions(&path, rperms).unwrap();

    let err = result.unwrap_err();
    assert!(err.contains("Could not write"), "got: {}", err);
}
