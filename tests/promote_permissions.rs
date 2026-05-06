//! Integration tests for `flow-rs promote-permissions`.
//!
//! All subprocess calls use Command::output() to avoid leaking child
//! output to the test harness.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use flow_rs::promote_permissions::{promote, read_json, run_impl, Args};
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

// --- active-flow gate ---

/// Setup helper for the active-flow gate tests: create a main-repo
/// dir with `.flow-states/<branch>/state.json` and a worktree at
/// `.worktrees/<branch>/` carrying a `.git` marker. Both paths are
/// canonicalized for stable comparisons on macOS.
fn setup_active_flow_repo(parent: &Path, branch: &str) -> (PathBuf, PathBuf) {
    let main_root = parent.canonicalize().expect("canonicalize tempdir");
    let branch_dir = main_root.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), "{}").unwrap();
    let worktree = main_root.join(".worktrees").join(branch);
    fs::create_dir_all(&worktree).unwrap();
    fs::write(worktree.join(".git"), "gitdir: fake\n").unwrap();
    (main_root, worktree)
}

#[test]
fn promote_subprocess_active_flow_without_confirm_skips() {
    // Active flow on the worktree's branch + no --confirm-on-flow-branch
    // → status:skipped, reason:active_flow. Settings are NOT mutated and
    // the local file is preserved so a subsequent confirmed call (e.g.
    // from flow-learn) can complete the merge.
    let dir = tempfile::tempdir().unwrap();
    let (_main_root, worktree) = setup_active_flow_repo(dir.path(), "feat-x");
    let settings_path = setup_settings(
        &worktree,
        json!({"permissions": {"allow": ["Bash(git *)"]}}),
    );
    let local_path = setup_local(
        &worktree,
        json!({"permissions": {"allow": ["Bash(npm run *)"]}}),
    );

    let output = flow_rs()
        .args(["promote-permissions", "--worktree-path"])
        .arg(&worktree)
        .output()
        .unwrap();
    let data = parse_stdout(&output.stdout);
    assert_eq!(data["status"], "skipped");
    assert_eq!(data["reason"], "active_flow");
    assert_eq!(data["branch"], "feat-x");

    // Settings unchanged — Bash(npm run *) NOT promoted.
    let settings: Value =
        serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
    let allow = settings["permissions"]["allow"].as_array().unwrap();
    assert_eq!(allow.len(), 1);
    assert_eq!(allow[0], "Bash(git *)");

    // Local file preserved for the confirmed retry path.
    assert!(local_path.exists());
}

#[test]
fn promote_subprocess_active_flow_with_confirm_proceeds() {
    // Active flow + --confirm-on-flow-branch → gate is silent and the
    // merge runs to completion. This is the path flow-learn Step 4
    // takes when accumulating session permissions into settings.json.
    let dir = tempfile::tempdir().unwrap();
    let (_main_root, worktree) = setup_active_flow_repo(dir.path(), "feat-x");
    let settings_path = setup_settings(
        &worktree,
        json!({"permissions": {"allow": ["Bash(git *)"]}}),
    );
    setup_local(
        &worktree,
        json!({"permissions": {"allow": ["Bash(npm run *)"]}}),
    );

    let output = flow_rs()
        .args([
            "promote-permissions",
            "--confirm-on-flow-branch",
            "--worktree-path",
        ])
        .arg(&worktree)
        .output()
        .unwrap();
    let data = parse_stdout(&output.stdout);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["promoted"], json!(["Bash(npm run *)"]));

    let settings: Value =
        serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
    let allow = settings["permissions"]["allow"].as_array().unwrap();
    assert_eq!(allow.len(), 2);
}

#[test]
fn promote_subprocess_no_active_flow_proceeds_without_confirm() {
    // No `.flow-states/` ancestor → no flow active → gate stays silent
    // and the merge runs as before (preserves prime-time and one-off
    // promote-permissions calls outside any flow).
    let dir = tempfile::tempdir().unwrap();
    let main_root = dir.path().canonicalize().unwrap();
    let settings_path = setup_settings(
        &main_root,
        json!({"permissions": {"allow": ["Bash(git *)"]}}),
    );
    setup_local(
        &main_root,
        json!({"permissions": {"allow": ["Bash(npm run *)"]}}),
    );

    let output = flow_rs()
        .args(["promote-permissions", "--worktree-path"])
        .arg(&main_root)
        .output()
        .unwrap();
    let data = parse_stdout(&output.stdout);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["promoted"], json!(["Bash(npm run *)"]));

    let settings: Value =
        serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
    assert_eq!(
        settings["permissions"]["allow"].as_array().unwrap().len(),
        2
    );
}

#[test]
fn promote_subprocess_inactive_flow_for_branch_proceeds() {
    // `.flow-states/` exists at main_root but NO state.json for the
    // worktree's branch → is_flow_active returns false → gate silent.
    // Mirrors the matching write_rule case (state file present for an
    // unrelated branch).
    let dir = tempfile::tempdir().unwrap();
    let main_root = dir.path().canonicalize().unwrap();
    fs::create_dir_all(main_root.join(".flow-states/other-branch")).unwrap();
    fs::write(main_root.join(".flow-states/other-branch/state.json"), "{}").unwrap();
    let worktree = main_root.join(".worktrees/feat-x");
    fs::create_dir_all(&worktree).unwrap();
    fs::write(worktree.join(".git"), "gitdir: fake\n").unwrap();
    setup_settings(
        &worktree,
        json!({"permissions": {"allow": ["Bash(git *)"]}}),
    );
    setup_local(
        &worktree,
        json!({"permissions": {"allow": ["Bash(npm run *)"]}}),
    );

    let output = flow_rs()
        .args(["promote-permissions", "--worktree-path"])
        .arg(&worktree)
        .output()
        .unwrap();
    let data = parse_stdout(&output.stdout);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["promoted"], json!(["Bash(npm run *)"]));
}

#[test]
fn promote_subprocess_active_flow_branch_undetectable_proceeds() {
    // `.flow-states/` exists at main_root, but worktree_path lacks a
    // `.worktrees/` segment so detect_branch_from_path returns None
    // (and there's no real git repo to fall back to). The gate cannot
    // correlate to a branch → returns None → merge proceeds.
    let dir = tempfile::tempdir().unwrap();
    let main_root = dir.path().canonicalize().unwrap();
    fs::create_dir_all(main_root.join(".flow-states")).unwrap();
    setup_settings(
        &main_root,
        json!({"permissions": {"allow": ["Bash(git *)"]}}),
    );
    setup_local(
        &main_root,
        json!({"permissions": {"allow": ["Bash(make *)"]}}),
    );

    let output = flow_rs()
        .args(["promote-permissions", "--worktree-path"])
        .arg(&main_root)
        .output()
        .unwrap();
    let data = parse_stdout(&output.stdout);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["promoted"], json!(["Bash(make *)"]));
}

#[test]
fn promote_subprocess_relative_worktree_path_resolves_against_cwd() {
    // Drives the relative-path branch in active_flow_gate: an
    // unqualified `--worktree-path .worktrees/feat-x` resolves
    // against the subprocess cwd.
    let dir = tempfile::tempdir().unwrap();
    let (_main_root, worktree) = setup_active_flow_repo(dir.path(), "feat-x");
    setup_settings(
        &worktree,
        json!({"permissions": {"allow": ["Bash(git *)"]}}),
    );
    setup_local(
        &worktree,
        json!({"permissions": {"allow": ["Bash(npm run *)"]}}),
    );

    // Run from the canonicalized parent (main_root). Relative path =
    // ".worktrees/feat-x".
    let output = flow_rs()
        .args([
            "promote-permissions",
            "--worktree-path",
            ".worktrees/feat-x",
        ])
        .current_dir(_main_root)
        .output()
        .unwrap();
    let data = parse_stdout(&output.stdout);
    assert_eq!(data["status"], "skipped");
    assert_eq!(data["reason"], "active_flow");
}

#[test]
fn promote_subprocess_submodule_subdirectory_does_not_bypass_gate() {
    // Adversarial regression: a git submodule (or any subdirectory
    // carrying its own `.git` file) inside a worktree previously
    // tricked `detect_branch_from_path` into returning `<branch>/<sub>`
    // — a slash-containing branch that `is_flow_active` rejected,
    // silently disabling the gate. The fix in `worktree_branch_from_path`
    // bypasses the `.git` walk-up and extracts the first `.worktrees/<X>/`
    // segment, restoring the active-flow correlation.
    let dir = tempfile::tempdir().unwrap();
    let (_main_root, worktree) = setup_active_flow_repo(dir.path(), "feat-x");
    let sub = worktree.join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join(".git"), "gitdir: submodule\n").unwrap();
    setup_settings(&sub, json!({"permissions": {"allow": []}}));
    setup_local(&sub, json!({"permissions": {"allow": ["Bash(rm -rf *)"]}}));

    let output = flow_rs()
        .args(["promote-permissions", "--worktree-path"])
        .arg(&sub)
        .output()
        .unwrap();
    let data = parse_stdout(&output.stdout);
    assert_eq!(data["status"], "skipped");
    assert_eq!(data["reason"], "active_flow");
    // Settings unchanged — the dangerous Bash(rm -rf *) entry must NOT
    // have been promoted from the submodule subdirectory.
    let settings: Value =
        serde_json::from_str(&fs::read_to_string(sub.join(".claude/settings.json")).unwrap())
            .unwrap();
    assert_eq!(
        settings["permissions"]["allow"].as_array().unwrap().len(),
        0
    );
}

// --- Library-level tests (migrated from src/promote_permissions.rs) ---

fn setup_dir_lib() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn write_local_lib(dir: &Path, content: &str) {
    let claude_dir = dir.join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(claude_dir.join("settings.local.json"), content).unwrap();
}

fn write_settings_lib(dir: &Path, content: &str) {
    let claude_dir = dir.join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(claude_dir.join("settings.json"), content).unwrap();
}

#[test]
fn promote_non_object_settings_returns_error() {
    // settings.json containing a JSON array is rejected before
    // the IndexMut assignment that would otherwise panic.
    let dir = setup_dir_lib();
    write_local_lib(
        dir.path(),
        r#"{"permissions": {"allow": ["Bash(echo *)"]}}"#,
    );
    write_settings_lib(dir.path(), "[1, 2, 3]");
    let result = promote(dir.path());
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("not a JSON object"));
}

#[test]
fn run_impl_skipped_is_ok() {
    let dir = setup_dir_lib();
    let args = Args {
        worktree_path: dir.path().to_string_lossy().to_string(),
        confirm_on_flow_branch: false,
    };
    let result = run_impl(&args).unwrap();
    assert_eq!(result["status"], "skipped");
}

#[test]
fn run_impl_error_is_err() {
    let dir = setup_dir_lib();
    write_local_lib(
        dir.path(),
        r#"{"permissions": {"allow": ["Bash(echo *)"]}}"#,
    );
    // No settings.json → error
    let args = Args {
        worktree_path: dir.path().to_string_lossy().to_string(),
        confirm_on_flow_branch: false,
    };
    let result = run_impl(&args);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err()["status"], "error");
}

#[test]
fn read_json_fs_read_error_returns_io_message() {
    // Reading a path that does not exist drives `fs::read` to Err,
    // exercising the `map_err(|e| e.to_string())?` propagation on
    // line 158 — the only uncovered region in this module when the
    // happy path is the sole exerciser.
    let missing = std::path::Path::new("/nonexistent/path/does/not/exist.json");
    let result = read_json(missing);
    assert!(result.is_err());
    let msg = result.unwrap_err();
    assert!(!msg.is_empty(), "expected non-empty io error message");
}
