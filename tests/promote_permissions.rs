//! Integration tests for `flow-rs promote-permissions`.
//!
//! All subprocess calls use Command::output() to avoid leaking child
//! output to the test harness.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

fn flow_rs() -> Command {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
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

fn setup_settings(worktree: &Path, data: Value) -> PathBuf {
    let claude_dir = worktree.join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    let settings_path = claude_dir.join("settings.json");
    fs::write(&settings_path, serde_json::to_string_pretty(&data).unwrap()).unwrap();
    settings_path
}

fn setup_local(worktree: &Path, data: Value) -> PathBuf {
    let claude_dir = worktree.join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    let local_path = claude_dir.join("settings.local.json");
    fs::write(&local_path, serde_json::to_string_pretty(&data).unwrap()).unwrap();
    local_path
}

fn run_promote(worktree: &Path) -> (Value, i32) {
    let output = flow_rs()
        .args(["promote-permissions", "--worktree-path"])
        .arg(worktree)
        .output()
        .unwrap();
    let value = parse_stdout(&output.stdout);
    let code = output.status.code().unwrap_or(-1);
    (value, code)
}

#[test]
fn no_local_file_returns_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    setup_settings(
        tmp.path(),
        json!({"permissions": {"allow": ["Bash(git *)"]}}),
    );
    let (data, _code) = run_promote(tmp.path());
    assert_eq!(data["status"], "skipped");
    assert_eq!(data["reason"], "no_local_file");
}

#[test]
fn empty_allow_list_returns_ok_and_deletes_local() {
    let tmp = tempfile::tempdir().unwrap();
    setup_settings(
        tmp.path(),
        json!({"permissions": {"allow": ["Bash(git *)"]}}),
    );
    let local = setup_local(tmp.path(), json!({"permissions": {"allow": []}}));
    let (data, _code) = run_promote(tmp.path());
    assert_eq!(data["status"], "ok");
    assert_eq!(data["promoted"].as_array().unwrap().len(), 0);
    assert_eq!(data["already_present"], 0);
    assert!(!local.exists());
}

#[test]
fn new_entries_promoted() {
    let tmp = tempfile::tempdir().unwrap();
    let settings = setup_settings(
        tmp.path(),
        json!({"permissions": {"allow": ["Bash(git *)"]}}),
    );
    let local = setup_local(
        tmp.path(),
        json!({"permissions": {"allow": ["Bash(npm run *)"], "deny": []}}),
    );
    let (data, _code) = run_promote(tmp.path());
    assert_eq!(data["status"], "ok");
    assert_eq!(data["promoted"], json!(["Bash(npm run *)"]));
    assert_eq!(data["already_present"], 0);
    assert!(!local.exists());

    let updated: Value = serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    let allow = updated["permissions"]["allow"].as_array().unwrap();
    assert!(allow.iter().any(|v| v == "Bash(npm run *)"));
    assert!(allow.iter().any(|v| v == "Bash(git *)"));
}

#[test]
fn all_duplicates_counted_without_promotion() {
    let tmp = tempfile::tempdir().unwrap();
    setup_settings(
        tmp.path(),
        json!({"permissions": {"allow": ["Bash(git *)", "Bash(npm run *)"]}}),
    );
    let local = setup_local(
        tmp.path(),
        json!({"permissions": {"allow": ["Bash(git *)", "Bash(npm run *)"]}}),
    );
    let (data, _code) = run_promote(tmp.path());
    assert_eq!(data["status"], "ok");
    assert_eq!(data["promoted"].as_array().unwrap().len(), 0);
    assert_eq!(data["already_present"], 2);
    assert!(!local.exists());
}

#[test]
fn mixed_new_and_existing() {
    let tmp = tempfile::tempdir().unwrap();
    let settings = setup_settings(
        tmp.path(),
        json!({"permissions": {"allow": ["Bash(git *)"]}}),
    );
    let local = setup_local(
        tmp.path(),
        json!({"permissions": {"allow": ["Bash(git *)", "Bash(make *)", "Bash(curl *)"]}}),
    );
    let (data, _code) = run_promote(tmp.path());
    assert_eq!(data["status"], "ok");
    let mut promoted: Vec<String> = data["promoted"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    promoted.sort();
    assert_eq!(promoted, vec!["Bash(curl *)", "Bash(make *)"]);
    assert_eq!(data["already_present"], 1);
    assert!(!local.exists());

    let updated: Value = serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(updated["permissions"]["allow"].as_array().unwrap().len(), 3);
}

#[test]
fn preserves_existing_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let settings = setup_settings(
        tmp.path(),
        json!({
            "permissions": {"allow": ["Bash(git *)"], "deny": ["Bash(rm -rf *)"]},
            "attribution": {"commit": "", "pr": ""},
        }),
    );
    setup_local(
        tmp.path(),
        json!({"permissions": {"allow": ["Bash(npm run *)"]}}),
    );
    let (data, _code) = run_promote(tmp.path());
    assert_eq!(data["status"], "ok");

    let updated: Value = serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(updated["attribution"], json!({"commit": "", "pr": ""}));
    assert_eq!(updated["permissions"]["deny"], json!(["Bash(rm -rf *)"]));
}

#[test]
fn deletion_verification() {
    let tmp = tempfile::tempdir().unwrap();
    setup_settings(tmp.path(), json!({"permissions": {"allow": []}}));
    let local = setup_local(
        tmp.path(),
        json!({"permissions": {"allow": ["Bash(git *)"]}}),
    );
    assert!(local.exists());
    run_promote(tmp.path());
    assert!(!local.exists());
}

#[test]
fn malformed_local_json_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    setup_settings(tmp.path(), json!({"permissions": {"allow": []}}));
    let claude_dir = tmp.path().join(".claude");
    fs::write(claude_dir.join("settings.local.json"), "{bad json").unwrap();

    let (data, code) = run_promote(tmp.path());
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap()
        .contains("settings.local.json"));
    assert_eq!(code, 1);
}

#[test]
fn malformed_settings_json_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let claude_dir = tmp.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(claude_dir.join("settings.json"), "{bad json").unwrap();
    setup_local(
        tmp.path(),
        json!({"permissions": {"allow": ["Bash(git *)"]}}),
    );

    let (data, code) = run_promote(tmp.path());
    assert_eq!(data["status"], "error");
    assert!(data["message"].as_str().unwrap().contains("settings.json"));
    assert_eq!(code, 1);
}

#[test]
fn missing_permissions_key_in_local() {
    let tmp = tempfile::tempdir().unwrap();
    setup_settings(
        tmp.path(),
        json!({"permissions": {"allow": ["Bash(git *)"]}}),
    );
    let local = setup_local(tmp.path(), json!({"attribution": {"commit": ""}}));
    let (data, _code) = run_promote(tmp.path());
    assert_eq!(data["status"], "ok");
    assert_eq!(data["promoted"].as_array().unwrap().len(), 0);
    assert_eq!(data["already_present"], 0);
    assert!(!local.exists());
}

#[test]
fn missing_allow_key_in_local() {
    let tmp = tempfile::tempdir().unwrap();
    setup_settings(
        tmp.path(),
        json!({"permissions": {"allow": ["Bash(git *)"]}}),
    );
    let local = setup_local(tmp.path(), json!({"permissions": {"deny": ["Bash(rm *)"]}}));
    let (data, _code) = run_promote(tmp.path());
    assert_eq!(data["status"], "ok");
    assert_eq!(data["promoted"].as_array().unwrap().len(), 0);
    assert_eq!(data["already_present"], 0);
    assert!(!local.exists());
}

#[test]
fn settings_json_missing_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    setup_local(
        tmp.path(),
        json!({"permissions": {"allow": ["Bash(git *)"]}}),
    );
    let (data, code) = run_promote(tmp.path());
    assert_eq!(data["status"], "error");
    assert!(data["message"].as_str().unwrap().contains("settings.json"));
    assert_eq!(code, 1);
}

#[test]
fn settings_json_no_permissions_key() {
    let tmp = tempfile::tempdir().unwrap();
    let settings = setup_settings(tmp.path(), json!({"attribution": {"commit": ""}}));
    setup_local(
        tmp.path(),
        json!({"permissions": {"allow": ["Bash(git *)"]}}),
    );
    let (data, _code) = run_promote(tmp.path());
    assert_eq!(data["status"], "ok");
    assert_eq!(data["promoted"], json!(["Bash(git *)"]));

    let updated: Value = serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    assert!(updated["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "Bash(git *)"));
}

#[test]
fn write_error_on_readonly_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let settings = setup_settings(tmp.path(), json!({"permissions": {"allow": []}}));
    setup_local(
        tmp.path(),
        json!({"permissions": {"allow": ["Bash(git *)"]}}),
    );

    // Make settings.json read-only so the write fails.
    let mut perms = fs::metadata(&settings).unwrap().permissions();
    perms.set_mode(0o444);
    fs::set_permissions(&settings, perms).unwrap();

    let (data, code) = run_promote(tmp.path());

    // Restore write permission so tempdir cleanup can remove the file.
    let mut perms = fs::metadata(&settings).unwrap().permissions();
    perms.set_mode(0o644);
    fs::set_permissions(&settings, perms).unwrap();

    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap()
        .contains("Could not write settings.json"));
    assert_eq!(code, 1);
}

#[test]
fn local_delete_fails_silently() {
    let tmp = tempfile::tempdir().unwrap();
    let settings = setup_settings(tmp.path(), json!({"permissions": {"allow": []}}));
    let local = setup_local(
        tmp.path(),
        json!({"permissions": {"allow": ["Bash(git *)"]}}),
    );

    // Make .claude/ directory read+execute only (no write) so remove_file fails.
    let claude_dir = tmp.path().join(".claude");
    let mut perms = fs::metadata(&claude_dir).unwrap().permissions();
    perms.set_mode(0o555);
    fs::set_permissions(&claude_dir, perms).unwrap();

    let (data, _code) = run_promote(tmp.path());

    // Restore write permission so tempdir cleanup succeeds.
    let mut perms = fs::metadata(&claude_dir).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&claude_dir, perms).unwrap();

    assert_eq!(data["status"], "ok");
    assert_eq!(data["promoted"], json!(["Bash(git *)"]));
    assert!(
        local.exists(),
        "settings.local.json still exists after failed delete"
    );

    let updated: Value = serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    assert!(updated["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "Bash(git *)"));
}

#[test]
fn cli_missing_worktree_path_arg() {
    let output = flow_rs().args(["promote-permissions"]).output().unwrap();
    assert_ne!(output.status.code(), Some(0));
}

#[test]
fn cli_happy_path() {
    let tmp = tempfile::tempdir().unwrap();
    setup_settings(
        tmp.path(),
        json!({"permissions": {"allow": ["Bash(git *)"]}}),
    );
    setup_local(
        tmp.path(),
        json!({"permissions": {"allow": ["Bash(npm run *)"]}}),
    );
    let (data, code) = run_promote(tmp.path());
    assert_eq!(data["status"], "ok");
    assert_eq!(data["promoted"], json!(["Bash(npm run *)"]));
    assert_eq!(code, 0);
}

#[test]
fn cli_no_local_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    setup_settings(tmp.path(), json!({"permissions": {"allow": []}}));
    let (data, code) = run_promote(tmp.path());
    assert_eq!(data["status"], "skipped");
    assert_eq!(data["reason"], "no_local_file");
    assert_eq!(code, 0);
}

#[test]
fn settings_non_object_top_level_returns_error() {
    // settings.json containing a JSON array at root level is rejected
    // before IndexMut access that would otherwise panic.
    let tmp = tempfile::tempdir().unwrap();
    let claude_dir = tmp.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(claude_dir.join("settings.json"), "[1, 2, 3]").unwrap();
    setup_local(
        tmp.path(),
        json!({"permissions": {"allow": ["Bash(git *)"]}}),
    );
    let (data, code) = run_promote(tmp.path());
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap()
        .contains("not a JSON object"));
    assert_eq!(code, 1);
}

#[test]
fn settings_permissions_as_array_does_not_panic() {
    // Guards the contract that `promote()` tolerates a malformed
    // `permissions` value: if `settings.json` stores `permissions` as
    // an array instead of an object, assigning
    // `settings_data["permissions"]["allow"]` would otherwise trigger
    // a `serde_json` `IndexMut` panic (exit 101). The guard replaces
    // a malformed permissions value with an empty object so the
    // merge proceeds without panicking.
    let tmp = tempfile::tempdir().unwrap();
    let claude_dir = tmp.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(
        claude_dir.join("settings.json"),
        serde_json::to_string_pretty(&json!({"permissions": ["Bash(git *)"]})).unwrap(),
    )
    .unwrap();
    setup_local(
        tmp.path(),
        json!({"permissions": {"allow": ["Bash(npm run *)"]}}),
    );

    let output = flow_rs()
        .args(["promote-permissions", "--worktree-path"])
        .arg(tmp.path())
        .output()
        .unwrap();
    // Exit 101 is a Rust panic; any other code is a controlled response.
    assert_ne!(
        output.status.code(),
        Some(101),
        "binary panicked on permissions-as-array input (stdout: {:?})",
        String::from_utf8_lossy(&output.stdout)
    );
    let data = parse_stdout(&output.stdout);
    assert!(data.get("status").is_some(), "expected JSON status field");
}

#[test]
fn settings_permissions_as_string_does_not_panic() {
    // Defensive: the same guard must hold for every non-object value
    // (string, number, bool) — not just arrays.
    let tmp = tempfile::tempdir().unwrap();
    let claude_dir = tmp.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(
        claude_dir.join("settings.json"),
        serde_json::to_string_pretty(&json!({"permissions": "malformed"})).unwrap(),
    )
    .unwrap();
    setup_local(
        tmp.path(),
        json!({"permissions": {"allow": ["Bash(git *)"]}}),
    );

    let output = flow_rs()
        .args(["promote-permissions", "--worktree-path"])
        .arg(tmp.path())
        .output()
        .unwrap();
    assert_ne!(output.status.code(), Some(101));
    let data = parse_stdout(&output.stdout);
    assert!(data.get("status").is_some());
}
