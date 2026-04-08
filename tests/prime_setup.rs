//! Integration tests for `flow-rs prime-setup`.
//!
//! Mirrors tests/test_prime_setup.py (1,135 lines). Tests cover:
//! - Pure function tests (merge_settings, is_subsumed, derive_permissions,
//!   write_version_marker, update_git_exclude, install_script,
//!   install_pre_commit_hook, install_launcher, check_launcher_path)
//! - CLI tests via run_impl
//!
//! Every subprocess call uses Command::output() per rust-port-parity.md
//! Test-Module Subprocess Stdio rule.

use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

use flow_rs::prime_check::{EXCLUDE_ENTRIES, FLOW_DENY, UNIVERSAL_ALLOW};
use flow_rs::prime_setup;

fn fw_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("frameworks")
}

fn read_settings(project: &Path) -> Value {
    let content = fs::read_to_string(project.join(".claude").join("settings.json")).unwrap();
    serde_json::from_str(&content).unwrap()
}

fn write_settings(project: &Path, data: &Value) {
    let claude_dir = project.join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(
        claude_dir.join("settings.json"),
        serde_json::to_string_pretty(data).unwrap(),
    )
    .unwrap();
}

/// Load expected framework permissions from frameworks/<name>/permissions.json.
fn load_framework_perms(framework: &str) -> Vec<String> {
    let path = fw_dir().join(framework).join("permissions.json");
    if !path.exists() {
        return Vec::new();
    }
    let data: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    data["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect()
}

fn make_git_repo(path: &Path) {
    Command::new("git")
        .args(["init"])
        .current_dir(path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(path)
        .output()
        .unwrap();
}

// ── merge_settings ──────────────────────────────────────────

#[test]
fn creates_settings_from_scratch() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::merge_settings(tmp.path(), "rails", &fw_dir()).unwrap();
    let settings = read_settings(tmp.path());
    assert!(settings["permissions"]["allow"].is_array());
    assert!(settings["permissions"]["deny"].is_array());
}

#[test]
fn settings_has_all_allow_entries_rails() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::merge_settings(tmp.path(), "rails", &fw_dir()).unwrap();
    let settings = read_settings(tmp.path());
    let allow: Vec<String> = settings["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    let allow_set: HashSet<String> = allow.iter().cloned().collect();
    let expected: Vec<String> = UNIVERSAL_ALLOW
        .iter()
        .map(|s| s.to_string())
        .chain(load_framework_perms("rails"))
        .collect();
    for entry in &expected {
        if !prime_setup::is_subsumed(entry, &allow_set) {
            assert!(allow.contains(entry), "Missing allow entry: {}", entry);
        }
    }
}

#[test]
fn settings_has_all_allow_entries_python() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::merge_settings(tmp.path(), "python", &fw_dir()).unwrap();
    let settings = read_settings(tmp.path());
    let allow: Vec<String> = settings["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    let allow_set: HashSet<String> = allow.iter().cloned().collect();
    let expected: Vec<String> = UNIVERSAL_ALLOW
        .iter()
        .map(|s| s.to_string())
        .chain(load_framework_perms("python"))
        .collect();
    for entry in &expected {
        if !prime_setup::is_subsumed(entry, &allow_set) {
            assert!(allow.contains(entry), "Missing allow entry: {}", entry);
        }
    }
}

#[test]
fn settings_has_all_deny_entries() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::merge_settings(tmp.path(), "rails", &fw_dir()).unwrap();
    let settings = read_settings(tmp.path());
    let deny: Vec<String> = settings["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    for entry in FLOW_DENY {
        assert!(deny.contains(&entry.to_string()), "Missing deny: {}", entry);
    }
}

#[test]
fn deny_list_includes_git_commit() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::merge_settings(tmp.path(), "rails", &fw_dir()).unwrap();
    let settings = read_settings(tmp.path());
    let deny: Vec<String> = settings["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    assert!(
        deny.contains(&"Bash(git commit *)".to_string()),
        "git commit must be denied to prevent Claude's built-in commit behavior"
    );
}

#[test]
fn allow_list_excludes_git_commit() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::merge_settings(tmp.path(), "rails", &fw_dir()).unwrap();
    let settings = read_settings(tmp.path());
    let allow: Vec<String> = settings["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    assert!(
        !allow.contains(&"Bash(git commit *)".to_string()),
        "git commit must not be in the allow list — it belongs in deny"
    );
}

#[test]
fn settings_sets_default_mode() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::merge_settings(tmp.path(), "rails", &fw_dir()).unwrap();
    let settings = read_settings(tmp.path());
    assert_eq!(settings["permissions"]["defaultMode"], "acceptEdits");
}

#[test]
fn settings_preserves_existing_entries() {
    let tmp = tempfile::tempdir().unwrap();
    write_settings(
        tmp.path(),
        &json!({
            "permissions": {
                "allow": ["Bash(custom command)"],
                "deny": ["Bash(custom deny)"],
            }
        }),
    );
    prime_setup::merge_settings(tmp.path(), "rails", &fw_dir()).unwrap();
    let settings = read_settings(tmp.path());
    let allow: Vec<String> = settings["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    assert!(allow.contains(&"Bash(custom command)".to_string()));
    let deny: Vec<String> = settings["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    assert!(deny.contains(&"Bash(custom deny)".to_string()));
}

#[test]
fn settings_overrides_existing_default_mode() {
    let tmp = tempfile::tempdir().unwrap();
    write_settings(
        tmp.path(),
        &json!({
            "permissions": {
                "allow": [],
                "deny": [],
                "defaultMode": "plan",
            }
        }),
    );
    prime_setup::merge_settings(tmp.path(), "rails", &fw_dir()).unwrap();
    let settings = read_settings(tmp.path());
    assert_eq!(settings["permissions"]["defaultMode"], "acceptEdits");
}

#[test]
fn settings_no_duplicate_entries() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::merge_settings(tmp.path(), "rails", &fw_dir()).unwrap();
    prime_setup::merge_settings(tmp.path(), "rails", &fw_dir()).unwrap();
    let settings = read_settings(tmp.path());
    let allow: Vec<String> = settings["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    let unique: HashSet<&String> = allow.iter().collect();
    assert_eq!(allow.len(), unique.len(), "Duplicate allow entries found");
    let deny: Vec<String> = settings["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    let unique: HashSet<&String> = deny.iter().collect();
    assert_eq!(deny.len(), unique.len(), "Duplicate deny entries found");
}

#[test]
fn settings_file_has_trailing_newline() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::merge_settings(tmp.path(), "rails", &fw_dir()).unwrap();
    let content = fs::read_to_string(tmp.path().join(".claude").join("settings.json")).unwrap();
    assert!(content.ends_with('\n'));
}

// ── Pattern subsumption ─────────────────────────────────────

#[test]
fn broad_pattern_subsumes_narrow() {
    let tmp = tempfile::tempdir().unwrap();
    write_settings(
        tmp.path(),
        &json!({"permissions": {"allow": ["Bash(git *)"]}}),
    );
    prime_setup::merge_settings(tmp.path(), "rails", &fw_dir()).unwrap();
    let settings = read_settings(tmp.path());
    let allow: Vec<String> = settings["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    assert!(!allow.contains(&"Bash(git add *)".to_string()));
    assert!(!allow.contains(&"Bash(git commit *)".to_string()));
    assert!(allow.contains(&"Bash(cd *)".to_string()));
    assert!(allow.contains(&"Agent(flow:ci-fixer)".to_string()));
}

#[test]
fn broad_gh_pattern_subsumes_narrow() {
    let tmp = tempfile::tempdir().unwrap();
    write_settings(
        tmp.path(),
        &json!({"permissions": {"allow": ["Bash(gh pr *)"]}}),
    );
    prime_setup::merge_settings(tmp.path(), "rails", &fw_dir()).unwrap();
    let settings = read_settings(tmp.path());
    let allow: Vec<String> = settings["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    assert!(!allow.contains(&"Bash(gh pr create *)".to_string()));
    assert!(allow.contains(&"Bash(gh issue *)".to_string()));
}

#[test]
fn cross_type_no_subsumption() {
    let tmp = tempfile::tempdir().unwrap();
    write_settings(tmp.path(), &json!({"permissions": {"allow": ["Agent(*)"]}}));
    prime_setup::merge_settings(tmp.path(), "rails", &fw_dir()).unwrap();
    let settings = read_settings(tmp.path());
    let allow: Vec<String> = settings["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    assert!(allow.contains(&"Bash(git add *)".to_string()));
}

#[test]
fn is_subsumed_malformed_candidate() {
    assert!(!prime_setup::is_subsumed(
        "plain-string",
        &HashSet::from(["Bash(git *)".to_string()])
    ));
}

#[test]
fn is_subsumed_skips_exact_match() {
    assert!(!prime_setup::is_subsumed(
        "Bash(git add *)",
        &HashSet::from(["Bash(git add *)".to_string()])
    ));
}

// ── Derived permissions ─────────────────────────────────────

#[test]
fn derive_permissions_ios_xcodeproj() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir(tmp.path().join("MyApp.xcodeproj")).unwrap();
    let result = prime_setup::derive_permissions(tmp.path(), "ios", &fw_dir());
    assert!(result.contains(&"Bash(killall MyApp)".to_string()));
}

#[test]
fn derive_permissions_no_xcodeproj() {
    let tmp = tempfile::tempdir().unwrap();
    let result = prime_setup::derive_permissions(tmp.path(), "ios", &fw_dir());
    assert!(result.is_empty());
}

#[test]
fn derive_permissions_rails_has_none() {
    let tmp = tempfile::tempdir().unwrap();
    let result = prime_setup::derive_permissions(tmp.path(), "rails", &fw_dir());
    assert!(result.is_empty());
}

#[test]
fn derive_permissions_unknown_framework() {
    let tmp = tempfile::tempdir().unwrap();
    let result = prime_setup::derive_permissions(tmp.path(), "nonexistent", &fw_dir());
    assert!(result.is_empty());
}

#[test]
fn derive_permissions_dot_prefix_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    // Dot-prefixed entry should be skipped (Python Path.glob parity)
    fs::create_dir(tmp.path().join(".xcodeproj")).unwrap();
    let result = prime_setup::derive_permissions(tmp.path(), "ios", &fw_dir());
    assert!(result.is_empty());
}

#[test]
fn derived_permissions_merged_into_settings() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir(tmp.path().join("SaltedKitchen.xcodeproj")).unwrap();
    prime_setup::merge_settings(tmp.path(), "ios", &fw_dir()).unwrap();
    let settings = read_settings(tmp.path());
    let allow: Vec<String> = settings["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    assert!(allow.contains(&"Bash(killall SaltedKitchen)".to_string()));
}

#[test]
fn derived_permissions_no_duplicates() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir(tmp.path().join("SaltedKitchen.xcodeproj")).unwrap();
    prime_setup::merge_settings(tmp.path(), "ios", &fw_dir()).unwrap();
    prime_setup::merge_settings(tmp.path(), "ios", &fw_dir()).unwrap();
    let settings = read_settings(tmp.path());
    let allow: Vec<String> = settings["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    let count = allow
        .iter()
        .filter(|e| *e == "Bash(killall SaltedKitchen)")
        .count();
    assert_eq!(count, 1);
}

#[test]
fn derived_permissions_subsumed() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir(tmp.path().join("MyApp.xcodeproj")).unwrap();
    write_settings(
        tmp.path(),
        &json!({"permissions": {"allow": ["Bash(killall *)"]}}),
    );
    prime_setup::merge_settings(tmp.path(), "ios", &fw_dir()).unwrap();
    let settings = read_settings(tmp.path());
    let allow: Vec<String> = settings["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    assert!(!allow.contains(&"Bash(killall MyApp)".to_string()));
}

// ── write_version_marker ────────────────────────────────────

#[test]
fn version_marker_created() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::write_version_marker(tmp.path(), "1.0.0", "rails", None, None, None, None, None)
        .unwrap();
    assert!(tmp.path().join(".flow.json").exists());
    let data: Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join(".flow.json")).unwrap()).unwrap();
    assert_eq!(data["flow_version"], "1.0.0");
    assert_eq!(data["framework"], "rails");
}

#[test]
fn version_marker_trailing_newline() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::write_version_marker(tmp.path(), "1.0.0", "rails", None, None, None, None, None)
        .unwrap();
    let content = fs::read_to_string(tmp.path().join(".flow.json")).unwrap();
    assert!(content.ends_with('\n'));
}

#[test]
fn version_marker_with_config_hash() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::write_version_marker(
        tmp.path(),
        "1.0.0",
        "rails",
        Some("abc123def456"),
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let data: Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join(".flow.json")).unwrap()).unwrap();
    assert_eq!(data["config_hash"], "abc123def456");
}

#[test]
fn version_marker_without_config_hash() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::write_version_marker(tmp.path(), "1.0.0", "rails", None, None, None, None, None)
        .unwrap();
    let data: Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join(".flow.json")).unwrap()).unwrap();
    assert!(data.get("config_hash").is_none());
}

#[test]
fn version_marker_with_setup_hash() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::write_version_marker(
        tmp.path(),
        "1.0.0",
        "rails",
        None,
        Some("abc123def456"),
        None,
        None,
        None,
    )
    .unwrap();
    let data: Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join(".flow.json")).unwrap()).unwrap();
    assert_eq!(data["setup_hash"], "abc123def456");
}

#[test]
fn version_marker_with_skills() {
    let tmp = tempfile::tempdir().unwrap();
    let skills = json!({"flow-start": "manual", "flow-code": "auto"});
    prime_setup::write_version_marker(
        tmp.path(),
        "1.0.0",
        "python",
        None,
        None,
        None,
        None,
        Some(&skills),
    )
    .unwrap();
    let data: Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join(".flow.json")).unwrap()).unwrap();
    assert_eq!(data["skills"], skills);
}

#[test]
fn version_marker_without_skills() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::write_version_marker(tmp.path(), "1.0.0", "rails", None, None, None, None, None)
        .unwrap();
    let data: Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join(".flow.json")).unwrap()).unwrap();
    assert!(data.get("skills").is_none());
}

#[test]
fn version_marker_with_commit_format() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::write_version_marker(
        tmp.path(),
        "1.0.0",
        "rails",
        None,
        None,
        Some("full"),
        None,
        None,
    )
    .unwrap();
    let data: Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join(".flow.json")).unwrap()).unwrap();
    assert_eq!(data["commit_format"], "full");
}

#[test]
fn version_marker_with_plugin_root() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::write_version_marker(
        tmp.path(),
        "1.0.0",
        "rails",
        None,
        None,
        None,
        Some("/some/cache/path"),
        None,
    )
    .unwrap();
    let data: Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join(".flow.json")).unwrap()).unwrap();
    assert_eq!(data["plugin_root"], "/some/cache/path");
}

// ── update_git_exclude ──────────────────────────────────────

#[test]
fn git_exclude_updated() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    let updated = prime_setup::update_git_exclude(tmp.path());
    assert!(updated);
    let content = fs::read_to_string(tmp.path().join(".git").join("info").join("exclude")).unwrap();
    assert!(content.contains(".flow-states/"));
    assert!(content.contains(".worktrees/"));
}

#[test]
fn git_exclude_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    prime_setup::update_git_exclude(tmp.path());
    prime_setup::update_git_exclude(tmp.path());
    let content = fs::read_to_string(tmp.path().join(".git").join("info").join("exclude")).unwrap();
    assert_eq!(content.matches(".flow-states/").count(), 1);
    assert_eq!(content.matches(".worktrees/").count(), 1);
}

#[test]
fn git_exclude_preserves_existing() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    let info_dir = tmp.path().join(".git").join("info");
    fs::create_dir_all(&info_dir).unwrap();
    fs::write(info_dir.join("exclude"), "*.log\n").unwrap();
    prime_setup::update_git_exclude(tmp.path());
    let content = fs::read_to_string(info_dir.join("exclude")).unwrap();
    assert!(content.contains("*.log"));
    assert!(content.contains(".flow-states/"));
}

#[test]
fn git_exclude_not_updated_when_present() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    let info_dir = tmp.path().join(".git").join("info");
    fs::create_dir_all(&info_dir).unwrap();
    let full_content: String = EXCLUDE_ENTRIES.iter().map(|e| format!("{}\n", e)).collect();
    fs::write(info_dir.join("exclude"), &full_content).unwrap();
    let updated = prime_setup::update_git_exclude(tmp.path());
    assert!(!updated);
}

#[test]
fn git_exclude_no_git_returns_false() {
    let tmp = tempfile::tempdir().unwrap();
    let updated = prime_setup::update_git_exclude(tmp.path());
    assert!(!updated);
}

#[test]
fn git_exclude_adds_newline_if_missing() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    let info_dir = tmp.path().join(".git").join("info");
    fs::create_dir_all(&info_dir).unwrap();
    fs::write(info_dir.join("exclude"), "*.tmp").unwrap(); // No trailing newline
    prime_setup::update_git_exclude(tmp.path());
    let content = fs::read_to_string(info_dir.join("exclude")).unwrap();
    assert!(content.contains("*.tmp\n.flow-states/"));
}

#[test]
fn git_exclude_creates_file_when_missing() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    let exclude_path = tmp.path().join(".git").join("info").join("exclude");
    if exclude_path.exists() {
        fs::remove_file(&exclude_path).unwrap();
    }
    prime_setup::update_git_exclude(tmp.path());
    assert!(exclude_path.exists());
    let content = fs::read_to_string(&exclude_path).unwrap();
    assert!(content.contains(".flow-states/"));
}

// ── install_script ──────────────────────────────────────────

#[test]
fn install_script_creates_executable_file() {
    let tmp = tempfile::tempdir().unwrap();
    let target_dir = tmp.path().join("subdir");
    prime_setup::install_script(&target_dir, "my-script", "#!/bin/bash\necho hi\n").unwrap();
    let script = target_dir.join("my-script");
    assert!(target_dir.is_dir());
    assert!(script.exists());
    assert_eq!(
        fs::read_to_string(&script).unwrap(),
        "#!/bin/bash\necho hi\n"
    );
    let mode = fs::metadata(&script).unwrap().permissions().mode();
    assert!(mode & 0o111 != 0, "Script should be executable");
}

// ── install_pre_commit_hook ─────────────────────────────────

#[test]
fn pre_commit_hook_created() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    prime_setup::install_pre_commit_hook(tmp.path()).unwrap();
    assert!(tmp
        .path()
        .join(".git")
        .join("hooks")
        .join("pre-commit")
        .exists());
}

#[test]
fn pre_commit_hook_executable() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    prime_setup::install_pre_commit_hook(tmp.path()).unwrap();
    let hook = tmp.path().join(".git").join("hooks").join("pre-commit");
    let mode = fs::metadata(&hook).unwrap().permissions().mode();
    assert!(mode & 0o111 != 0);
}

#[test]
fn pre_commit_hook_content() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    prime_setup::install_pre_commit_hook(tmp.path()).unwrap();
    let content =
        fs::read_to_string(tmp.path().join(".git").join("hooks").join("pre-commit")).unwrap();
    assert!(content.contains(".flow-commit-msg"));
    assert!(content.contains(".flow-states/"));
    assert!(content.contains("exit 1"));
}

#[test]
fn pre_commit_hook_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    prime_setup::install_pre_commit_hook(tmp.path()).unwrap();
    let first =
        fs::read_to_string(tmp.path().join(".git").join("hooks").join("pre-commit")).unwrap();
    prime_setup::install_pre_commit_hook(tmp.path()).unwrap();
    let second =
        fs::read_to_string(tmp.path().join(".git").join("hooks").join("pre-commit")).unwrap();
    assert_eq!(first, second);
}

// ── install_launcher ────────────────────────────────────────

#[test]
fn install_launcher_creates_file() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::install_launcher(tmp.path()).unwrap();
    assert!(tmp.path().join(".local").join("bin").join("flow").exists());
}

#[test]
fn install_launcher_executable() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::install_launcher(tmp.path()).unwrap();
    let launcher = tmp.path().join(".local").join("bin").join("flow");
    let mode = fs::metadata(&launcher).unwrap().permissions().mode();
    assert!(mode & 0o111 != 0);
}

#[test]
fn install_launcher_content() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::install_launcher(tmp.path()).unwrap();
    let content = fs::read_to_string(tmp.path().join(".local").join("bin").join("flow")).unwrap();
    assert!(content.contains("git rev-parse --show-toplevel"));
    assert!(content.contains(".flow.json"));
    assert!(content.contains("plugin_root"));
    assert!(content.contains("exec \"$plugin_root/bin/flow\""));
}

#[test]
fn install_launcher_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::install_launcher(tmp.path()).unwrap();
    let first = fs::read_to_string(tmp.path().join(".local").join("bin").join("flow")).unwrap();
    prime_setup::install_launcher(tmp.path()).unwrap();
    let second = fs::read_to_string(tmp.path().join(".local").join("bin").join("flow")).unwrap();
    assert_eq!(first, second);
}

#[test]
fn install_launcher_creates_directory() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(!tmp.path().join(".local").join("bin").exists());
    prime_setup::install_launcher(tmp.path()).unwrap();
    assert!(tmp.path().join(".local").join("bin").join("flow").exists());
}

// ── CLI via subprocess ──────────────────────────────────────

fn flow_rs() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"));
    cmd
}

fn parse_stdout(stdout: &[u8]) -> Value {
    let text = String::from_utf8_lossy(stdout);
    let last_line = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .next_back()
        .unwrap_or_else(|| panic!("no stdout lines: {:?}", text));
    serde_json::from_str(last_line.trim())
        .unwrap_or_else(|e| panic!("JSON parse failed: {} (line: {:?})", e, last_line))
}

fn run_setup(project: &Path, framework: &str) -> (Value, i32) {
    let output = flow_rs()
        .arg("prime-setup")
        .arg(project)
        .arg("--framework")
        .arg(framework)
        .output()
        .unwrap();
    let value = parse_stdout(&output.stdout);
    let code = output.status.code().unwrap_or(-1);
    (value, code)
}

#[test]
fn cli_invalid_project_root() {
    let tmp = tempfile::tempdir().unwrap();
    let (data, code) = run_setup(&tmp.path().join("nonexistent"), "rails");
    assert_eq!(data["status"], "error");
    assert_eq!(code, 1);
}

#[test]
fn cli_missing_framework() {
    let tmp = tempfile::tempdir().unwrap();
    let output = flow_rs()
        .arg("prime-setup")
        .arg(tmp.path())
        .output()
        .unwrap();
    assert_ne!(output.status.code(), Some(0));
}

#[test]
fn cli_invalid_framework() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    let (data, code) = run_setup(tmp.path(), "django");
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("framework"));
    assert_eq!(code, 1);
}

#[test]
fn cli_happy_path() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    let (data, code) = run_setup(tmp.path(), "rails");
    assert_eq!(code, 0);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["settings_merged"], true);
    assert_eq!(data["version_marker"], true);
    assert_eq!(data["hook_installed"], true);
    assert_eq!(data["framework"], "rails");
}

#[test]
fn cli_skills_json_written() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    let skills = json!({"flow-start": {"continue": "manual"}, "flow-abort": "auto"});
    let output = flow_rs()
        .arg("prime-setup")
        .arg(tmp.path())
        .arg("--framework")
        .arg("rails")
        .arg("--skills-json")
        .arg(serde_json::to_string(&skills).unwrap())
        .output()
        .unwrap();
    let data = parse_stdout(&output.stdout);
    assert_eq!(data["status"], "ok");
    let flow_data: Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join(".flow.json")).unwrap()).unwrap();
    assert_eq!(flow_data["skills"], skills);
}

#[test]
fn cli_commit_format_written() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    let output = flow_rs()
        .arg("prime-setup")
        .arg(tmp.path())
        .arg("--framework")
        .arg("rails")
        .arg("--commit-format")
        .arg("title-only")
        .output()
        .unwrap();
    let data = parse_stdout(&output.stdout);
    assert_eq!(data["status"], "ok");
    let flow_data: Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join(".flow.json")).unwrap()).unwrap();
    assert_eq!(flow_data["commit_format"], "title-only");
}

#[test]
fn cli_plugin_root_written_and_launcher_installed() {
    let tmp = tempfile::tempdir().unwrap();
    let fake_home = tmp.path().join("fakehome");
    fs::create_dir_all(&fake_home).unwrap();
    make_git_repo(tmp.path());
    let output = flow_rs()
        .arg("prime-setup")
        .arg(tmp.path())
        .arg("--framework")
        .arg("rails")
        .arg("--plugin-root")
        .arg("/some/cache/path")
        .env("HOME", &fake_home)
        .output()
        .unwrap();
    let data = parse_stdout(&output.stdout);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["launcher_installed"], true);
    let flow_data: Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join(".flow.json")).unwrap()).unwrap();
    assert_eq!(flow_data["plugin_root"], "/some/cache/path");
}

#[test]
fn cli_no_plugin_root_no_launcher() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    let (data, code) = run_setup(tmp.path(), "rails");
    assert_eq!(code, 0);
    assert_eq!(data["launcher_installed"], false);
    let flow_data: Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join(".flow.json")).unwrap()).unwrap();
    assert!(flow_data.get("plugin_root").is_none());
}

#[test]
fn cli_invalid_skills_json() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    let output = flow_rs()
        .arg("prime-setup")
        .arg(tmp.path())
        .arg("--framework")
        .arg("rails")
        .arg("--skills-json")
        .arg("not valid json")
        .output()
        .unwrap();
    let data = parse_stdout(&output.stdout);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("skills-json"));
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn cli_happy_path_stores_config_hash() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    let (data, code) = run_setup(tmp.path(), "rails");
    assert_eq!(code, 0);
    assert_eq!(data["status"], "ok");
    let flow_data: Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join(".flow.json")).unwrap()).unwrap();
    let hash = flow_data["config_hash"].as_str().unwrap();
    assert_eq!(hash.len(), 12);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn cli_happy_path_stores_setup_hash() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    let (data, code) = run_setup(tmp.path(), "rails");
    assert_eq!(code, 0);
    assert_eq!(data["status"], "ok");
    let flow_data: Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join(".flow.json")).unwrap()).unwrap();
    let hash = flow_data["setup_hash"].as_str().unwrap();
    assert_eq!(hash.len(), 12);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn cli_primes_project_claude_md() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    fs::write(tmp.path().join("CLAUDE.md"), "# Project\n").unwrap();
    let (data, code) = run_setup(tmp.path(), "rails");
    assert_eq!(code, 0);
    assert_eq!(data["prime_project"], "ok");
    let content = fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap();
    assert!(content.contains("<!-- FLOW:BEGIN -->"));
}

#[test]
fn cli_creates_bin_dependencies() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    let (data, code) = run_setup(tmp.path(), "rails");
    assert_eq!(code, 0);
    assert_eq!(data["dependencies"], "ok");
    assert!(tmp.path().join("bin").join("dependencies").exists());
}

#[test]
fn cli_dependencies_skipped_when_exists() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(bin_dir.join("dependencies"), "#!/bin/bash\ncustom\n").unwrap();
    let (data, code) = run_setup(tmp.path(), "rails");
    assert_eq!(code, 0);
    assert_eq!(data["dependencies"], "skipped");
    assert_eq!(
        fs::read_to_string(bin_dir.join("dependencies")).unwrap(),
        "#!/bin/bash\ncustom\n"
    );
}

// ── Framework exclusion ─────────────────────────────────────

#[test]
fn rails_excludes_python_permissions() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::merge_settings(tmp.path(), "rails", &fw_dir()).unwrap();
    let settings = read_settings(tmp.path());
    let allow: Vec<String> = settings["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    for entry in load_framework_perms("python") {
        assert!(
            !allow.contains(&entry),
            "Rails settings should not contain Python permission: {}",
            entry
        );
    }
}

#[test]
fn python_excludes_rails_permissions() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::merge_settings(tmp.path(), "python", &fw_dir()).unwrap();
    let settings = read_settings(tmp.path());
    let allow: Vec<String> = settings["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    for entry in load_framework_perms("rails") {
        assert!(
            !allow.contains(&entry),
            "Python settings should not contain Rails permission: {}",
            entry
        );
    }
}
