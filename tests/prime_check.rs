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
    flow_rs::prime_check::compute_config_hash().unwrap()
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
