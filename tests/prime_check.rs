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

// --- Hash-stability behavioral guards ---
//
// The config_hash and setup_hash values stored in every existing
// `.flow.json` file in the wild were computed by the formatter and
// algorithm at the time of priming. Any behavioral change to the
// hashing path silently invalidates those stored hashes and forces
// every user to re-prime. The following tests guard the contract.

/// `EXCLUDE_ENTRIES` carries the per-language basename patterns
/// that match the Code Review adversarial agent's probe test files
/// inside the project's test tree. The patterns land in
/// `.git/info/exclude` at prime time so user `git status` output
/// does not surface the throwaway probe alongside intentional
/// changes. Each pattern matches exactly one of the per-language
/// probe basenames the `assets/bin-stubs/test.sh` examples
/// recommend; together they cover Rust, Python, JS/TS, Go, Rails
/// Minitest, RSpec, and Swift. None of the patterns use a leading
/// wildcard, so a user-named legitimate test cannot be silently
/// excluded.
#[test]
fn exclude_entries_includes_adversarial_basenames() {
    use flow_rs::prime_check::EXCLUDE_ENTRIES;
    let expected = [
        "test_adversarial_flow.*",    // Rust, Python, JS/TS
        "adversarial_flow_test.go",   // Go
        "adversarial_flow_test.rb",   // Rails Minitest
        "adversarial_flow_spec.rb",   // RSpec
        "AdversarialFlowTests.swift", // Swift XCTestCase
    ];
    for pat in &expected {
        assert!(
            EXCLUDE_ENTRIES.contains(pat),
            "EXCLUDE_ENTRIES must contain {pat:?} — got {:?}",
            EXCLUDE_ENTRIES
        );
    }
    assert!(
        !EXCLUDE_ENTRIES.contains(&"*_adversarial_flow_test.rb"),
        "EXCLUDE_ENTRIES must not contain the leading-wildcard pattern \
         *_adversarial_flow_test.rb — replaced by exact basename \
         adversarial_flow_test.rb to prevent silent exclusion of \
         user-named legitimate tests"
    );
}

/// `FLOW_DENY` carries the patterns that prime writes to
/// `.claude/settings.json` `permissions.deny`. The five `find` shapes
/// below cover the destructive-by-construction flag forms that have
/// no benign read-only use under FLOW: `-exec`, `-execdir`, `-ok`,
/// `-okdir`, and `-delete` invoke arbitrary commands or unlink files
/// without confirmation. Layer 7 of `validate-pretool` consults the
/// merged settings.deny on every Bash call, so target projects
/// inherit these blocks at next prime.
#[test]
fn flow_deny_includes_find_destructive_flags() {
    use flow_rs::prime_check::FLOW_DENY;
    let expected = [
        "Bash(find * -exec *)",
        "Bash(find * -execdir *)",
        "Bash(find * -ok *)",
        "Bash(find * -okdir *)",
        "Bash(find * -delete*)",
    ];
    for pat in &expected {
        assert!(
            FLOW_DENY.contains(pat),
            "FLOW_DENY must contain {pat:?} — got {:?}",
            FLOW_DENY
        );
    }
}

/// Guards that `compute_config_hash` produces the same hex output for
/// repeated calls within a single process. If the function ever picks
/// up a non-deterministic input (clock, env var, random nonce), this
/// test catches it before it lands.
#[test]
fn compute_config_hash_is_deterministic_across_runs() {
    let a = flow_rs::prime_check::compute_config_hash();
    let b = flow_rs::prime_check::compute_config_hash();
    assert_eq!(a, b, "compute_config_hash must be deterministic");
    assert_eq!(a.len(), 12, "hash should be 12 hex chars");
    assert!(
        a.chars().all(|c| c.is_ascii_hexdigit()),
        "hash should be hex: {}",
        a
    );
}

/// Frozen-golden guard for `compute_config_hash`. The hex value below
/// is the SHA-256 prefix produced by the current `PythonDefaultFormatter`
/// over the current `UNIVERSAL_ALLOW` + `FLOW_DENY` + `EXCLUDE_ENTRIES`
/// constants. Any change to the formatter, key order, or hash inputs
/// will fail this test — which is correct, because such a change would
/// invalidate every stored `.flow.json` hash and force a re-prime for
/// every user.
///
/// When intentionally evolving the constants (e.g. adding a new entry
/// to UNIVERSAL_ALLOW), update the golden hex value in the same commit
/// as the constant change. The plan-deviation gate in `finalize-commit`
/// will block until the new value is acknowledged.
#[test]
fn compute_config_hash_uses_python_default_formatter() {
    let hash = flow_rs::prime_check::compute_config_hash();
    // Read CURRENT_CONFIG_HASH below: this value pins the output
    // produced by the in-tree constants and formatter at the time the
    // test was authored. Update it together with any intentional change
    // to UNIVERSAL_ALLOW / FLOW_DENY / EXCLUDE_ENTRIES / hash format.
    const CURRENT_CONFIG_HASH: &str = "88edad2bc7b7";
    assert_eq!(
        hash, CURRENT_CONFIG_HASH,
        "config_hash drift — PythonDefaultFormatter or input constants changed; \
         every stored .flow.json hash is invalidated. If intentional, update \
         CURRENT_CONFIG_HASH to {}.",
        hash
    );
}

/// Guards that `compute_setup_hash` is sensitive to changes in the
/// `prime_setup.rs` source bytes. If the function were ever to hash a
/// fixed string (e.g. a stale cached value), or to skip the file-read
/// step, this test would catch it: the synthetic mutated source
/// produces a different hash than the original.
#[test]
fn compute_setup_hash_changes_when_source_changes() {
    use std::os::unix::fs::PermissionsExt;

    // Build a fake plugin root with a controlled prime_setup.rs.
    let tmp = tempfile::tempdir().unwrap();
    let src_dir = tmp.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();
    let setup_path = src_dir.join("prime_setup.rs");

    fs::write(&setup_path, b"// version a").unwrap();
    let hash_a = flow_rs::prime_check::compute_setup_hash(tmp.path()).unwrap();

    fs::write(&setup_path, b"// version b").unwrap();
    let hash_b = flow_rs::prime_check::compute_setup_hash(tmp.path()).unwrap();

    assert_ne!(
        hash_a, hash_b,
        "compute_setup_hash must reflect changes in prime_setup.rs source"
    );
    assert_eq!(hash_a.len(), 12);
    assert_eq!(hash_b.len(), 12);

    // Restore perms so tempdir cleanup works.
    let _ = fs::set_permissions(&setup_path, fs::Permissions::from_mode(0o644));
}

/// Mismatch case: config_hash matches but setup_hash is missing.
/// Auto-upgrade requires BOTH hashes to match — only one matching
/// must still trigger the re-prime path.
#[test]
fn prime_check_returns_mismatch_error_when_only_config_hash_matches() {
    let tmp = tempfile::tempdir().unwrap();
    let config_hash = computed_config_hash();
    write_flow_json(
        tmp.path(),
        json!({
            "flow_version": "0.0.1",
            "config_hash": config_hash,
            "setup_hash": "deadbeefcafe",
        }),
    );
    let (data, _code) = run_prime_check(tmp.path());
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap()
            .contains("/flow:flow-prime"),
        "should direct user to re-prime"
    );
}

/// Mismatch case: setup_hash matches but config_hash is wrong.
/// Symmetric to the only-config-matches case — re-prime is required.
#[test]
fn prime_check_returns_mismatch_error_when_only_setup_hash_matches() {
    let tmp = tempfile::tempdir().unwrap();
    let setup_hash = computed_setup_hash();
    write_flow_json(
        tmp.path(),
        json!({
            "flow_version": "0.0.1",
            "config_hash": "deadbeefcafe",
            "setup_hash": setup_hash,
        }),
    );
    let (data, _code) = run_prime_check(tmp.path());
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap()
            .contains("/flow:flow-prime"),
        "should direct user to re-prime"
    );
}

/// Mismatch case: NEITHER hash matches. Catches a regression where
/// the auto-upgrade decision logic might fall through to "ok" when
/// both hashes are wrong (e.g. by accidentally treating `Err` from
/// hash comparison as a non-failure).
#[test]
fn prime_check_returns_mismatch_error_when_neither_hash_matches() {
    let tmp = tempfile::tempdir().unwrap();
    write_flow_json(
        tmp.path(),
        json!({
            "flow_version": "0.0.1",
            "config_hash": "000000000000",
            "setup_hash": "111111111111",
        }),
    );
    let (data, _code) = run_prime_check(tmp.path());
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap()
            .contains("/flow:flow-prime"),
        "should direct user to re-prime when both hashes mismatch"
    );
}
