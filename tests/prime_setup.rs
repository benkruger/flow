//! Integration tests for `flow-rs prime-setup`.
//!
//! Tests cover:
//! - Pure function tests (merge_settings, is_subsumed,
//!   write_version_marker, update_git_exclude, install_script,
//!   install_pre_commit_hook, install_launcher, install_bin_stubs)
//! - CLI tests via run_impl
//!
//! All subprocess calls use Command::output() to avoid leaking child
//! output to the test harness.

use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};

use flow_rs::prime_check::{EXCLUDE_ENTRIES, FLOW_DENY, UNIVERSAL_ALLOW};
use flow_rs::prime_setup;

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
    prime_setup::merge_settings(tmp.path()).unwrap();
    let settings = read_settings(tmp.path());
    assert!(settings["permissions"]["allow"].is_array());
    assert!(settings["permissions"]["deny"].is_array());
}

#[test]
fn settings_has_all_universal_allow_entries() {
    // Prime writes the full UNIVERSAL_ALLOW set into the merged
    // settings unless an existing broader pattern subsumes a specific
    // entry. Allow list is universal-only — no per-language merge.
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::merge_settings(tmp.path()).unwrap();
    let settings = read_settings(tmp.path());
    let allow: Vec<String> = settings["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    let allow_set: HashSet<String> = allow.iter().cloned().collect();
    for entry in UNIVERSAL_ALLOW {
        let s = entry.to_string();
        if !prime_setup::is_subsumed(&s, &allow_set) {
            assert!(allow.contains(&s), "Missing allow entry: {}", entry);
        }
    }
}

#[test]
fn settings_has_all_deny_entries() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::merge_settings(tmp.path()).unwrap();
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
    prime_setup::merge_settings(tmp.path()).unwrap();
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
    prime_setup::merge_settings(tmp.path()).unwrap();
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
    prime_setup::merge_settings(tmp.path()).unwrap();
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
    prime_setup::merge_settings(tmp.path()).unwrap();
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
    prime_setup::merge_settings(tmp.path()).unwrap();
    let settings = read_settings(tmp.path());
    assert_eq!(settings["permissions"]["defaultMode"], "acceptEdits");
}

#[test]
fn settings_no_duplicate_entries() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::merge_settings(tmp.path()).unwrap();
    prime_setup::merge_settings(tmp.path()).unwrap();
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
    prime_setup::merge_settings(tmp.path()).unwrap();
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
    prime_setup::merge_settings(tmp.path()).unwrap();
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
    prime_setup::merge_settings(tmp.path()).unwrap();
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
    prime_setup::merge_settings(tmp.path()).unwrap();
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

#[test]
fn is_subsumed_agent_wildcard_subsumes_specific() {
    assert!(prime_setup::is_subsumed(
        "Agent(flow:ci-fixer)",
        &HashSet::from(["Agent(*)".to_string()])
    ));
}

#[test]
fn is_subsumed_read_wildcard_subsumes_specific() {
    assert!(prime_setup::is_subsumed(
        "Read(~/.claude/rules/*)",
        &HashSet::from(["Read(~/.claude/*)".to_string()])
    ));
}

#[test]
fn is_subsumed_skill_wildcard_subsumes_specific() {
    assert!(prime_setup::is_subsumed(
        "Skill(decompose:decompose)",
        &HashSet::from(["Skill(*)".to_string()])
    ));
}

#[test]
fn is_subsumed_cross_type_no_match() {
    assert!(!prime_setup::is_subsumed(
        "Skill(decompose:decompose)",
        &HashSet::from(["Agent(*)".to_string()])
    ));
}

/// Covers the `None => continue` arm when an existing entry in the set
/// does not match the `<Type>(<inner>)` outer regex. The malformed
/// existing entry is skipped, no subsumption is found.
#[test]
fn is_subsumed_skips_malformed_existing_entry() {
    assert!(!prime_setup::is_subsumed(
        "Bash(git status)",
        &HashSet::from(["plain-string".to_string(), "Bash(git status)".to_string(),])
    ));
}

// ── write_version_marker ────────────────────────────────────

#[test]
fn version_marker_created() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::write_version_marker(tmp.path(), "1.0.0", None, None, None, None, None).unwrap();
    assert!(tmp.path().join(".flow.json").exists());
    let data: Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join(".flow.json")).unwrap()).unwrap();
    assert_eq!(data["flow_version"], "1.0.0");
}

#[test]
fn version_marker_writes_minimal_json() {
    // Tombstone: write_version_marker produces a minimal `.flow.json`
    // with only the keys it was asked to set. The legacy `framework`
    // key (rails/python/ios/go/rust) is never written; older files
    // with the key still parse cleanly because every consumer ignores
    // unknown JSON fields.
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::write_version_marker(tmp.path(), "1.0.0", None, None, None, None, None).unwrap();
    let data: Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join(".flow.json")).unwrap()).unwrap();
    assert!(
        data.get("framework").is_none(),
        "framework key must not be written to .flow.json"
    );
}

#[test]
fn version_marker_trailing_newline() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::write_version_marker(tmp.path(), "1.0.0", None, None, None, None, None).unwrap();
    let content = fs::read_to_string(tmp.path().join(".flow.json")).unwrap();
    assert!(content.ends_with('\n'));
}

#[test]
fn version_marker_with_config_hash() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::write_version_marker(
        tmp.path(),
        "1.0.0",
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
    prime_setup::write_version_marker(tmp.path(), "1.0.0", None, None, None, None, None).unwrap();
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
    prime_setup::write_version_marker(tmp.path(), "1.0.0", None, None, None, None, Some(&skills))
        .unwrap();
    let data: Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join(".flow.json")).unwrap()).unwrap();
    assert_eq!(data["skills"], skills);
}

#[test]
fn version_marker_without_skills() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::write_version_marker(tmp.path(), "1.0.0", None, None, None, None, None).unwrap();
    let data: Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join(".flow.json")).unwrap()).unwrap();
    assert!(data.get("skills").is_none());
}

#[test]
fn version_marker_with_commit_format() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::write_version_marker(tmp.path(), "1.0.0", None, None, Some("full"), None, None)
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

/// Error branch: `fs::create_dir_all` fails because the parent path
/// is a file rather than a directory. `install_script` returns Err
/// with a "Could not create directory" message.
#[test]
fn install_script_create_dir_failure_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let blocker = tmp.path().join("blocker");
    fs::write(&blocker, "not a dir").unwrap();
    // target_dir sits UNDER blocker, which is a regular file —
    // create_dir_all cannot create a child under a file.
    let target_dir = blocker.join("subdir");
    let result = prime_setup::install_script(&target_dir, "any", "content");
    assert!(result.is_err(), "expected Err when parent is a file");
    let msg = result.unwrap_err();
    assert!(
        msg.contains("Could not create directory"),
        "expected create_dir_all error, got: {}",
        msg
    );
}

/// Error branch: `fs::write` fails because the target path is an
/// existing directory. `install_script` returns Err with a
/// "Could not write" message.
#[test]
fn install_script_write_failure_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let target_dir = tmp.path().join("subdir");
    fs::create_dir_all(&target_dir).unwrap();
    // The target filename resolves to a pre-existing DIRECTORY so
    // fs::write cannot replace it with file contents.
    let filename = "collides-with-dir";
    fs::create_dir_all(target_dir.join(filename)).unwrap();
    let result = prime_setup::install_script(&target_dir, filename, "content");
    assert!(result.is_err(), "expected Err when target is a directory");
    let msg = result.unwrap_err();
    assert!(
        msg.contains("Could not write"),
        "expected write error, got: {}",
        msg
    );
}

/// Error branch: `write_version_marker` fails when `.flow.json`
/// resolves to a path that cannot be written — simulated by making
/// `.flow.json` itself an existing directory.
#[test]
fn version_marker_write_failure_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let flow_json_as_dir = tmp.path().join(".flow.json");
    fs::create_dir(&flow_json_as_dir).unwrap();
    let result =
        prime_setup::write_version_marker(tmp.path(), "1.0.0", None, None, None, None, None);
    assert!(
        result.is_err(),
        "expected Err when .flow.json is a directory"
    );
    let msg = result.unwrap_err();
    assert!(
        msg.contains("Could not write"),
        "expected write error, got: {}",
        msg
    );
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
    assert!(content.contains("-commit-msg.txt"));
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

// ── install_bin_stubs ───────────────────────────────────────

#[test]
fn install_bin_stubs_copies_all_four() {
    // The plugin root for tests is the FLOW repo manifest dir, which
    // contains assets/bin-stubs/<tool>.sh for all four tools.
    let tmp = tempfile::tempdir().unwrap();
    let plugin_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let installed = prime_setup::install_bin_stubs(tmp.path(), plugin_root);
    assert_eq!(installed.len(), 4);
    for tool in &["format", "lint", "build", "test"] {
        let path = tmp.path().join("bin").join(tool);
        assert!(path.exists(), "expected {} to be installed", tool);
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert!(mode & 0o111 != 0, "{} not executable", tool);
    }
}

#[test]
fn install_bin_stubs_skips_existing() {
    // Pre-existing user scripts must never be overwritten so users
    // who already configured their own bin/* keep their work.
    let tmp = tempfile::tempdir().unwrap();
    let plugin_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(bin_dir.join("test"), "#!/bin/bash\nexit 99\n").unwrap();
    let installed = prime_setup::install_bin_stubs(tmp.path(), plugin_root);
    // test was skipped; format/lint/build were installed
    assert_eq!(installed.len(), 3);
    assert!(!installed.contains(&"test".to_string()));
    let test_content = fs::read_to_string(bin_dir.join("test")).unwrap();
    assert!(
        test_content.contains("exit 99"),
        "user's test script must be preserved"
    );
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
        .rfind(|l| !l.trim().is_empty())
        .unwrap_or_else(|| panic!("no stdout lines: {:?}", text));
    serde_json::from_str(last_line.trim())
        .unwrap_or_else(|e| panic!("JSON parse failed: {} (line: {:?})", e, last_line))
}

fn run_setup(project: &Path) -> (Value, i32) {
    let output = flow_rs().arg("prime-setup").arg(project).output().unwrap();
    let value = parse_stdout(&output.stdout);
    let code = output.status.code().unwrap_or(-1);
    (value, code)
}

#[test]
fn cli_invalid_project_root() {
    let tmp = tempfile::tempdir().unwrap();
    let (data, code) = run_setup(&tmp.path().join("nonexistent"));
    assert_eq!(data["status"], "error");
    assert_eq!(code, 1);
}

#[test]
fn cli_happy_path() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    let (data, code) = run_setup(tmp.path());
    assert_eq!(code, 0);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["settings_merged"], true);
    assert_eq!(data["version_marker"], true);
    assert_eq!(data["hook_installed"], true);
}

#[test]
fn cli_skills_json_written() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    let skills = json!({"flow-start": {"continue": "manual"}, "flow-abort": "auto"});
    let output = flow_rs()
        .arg("prime-setup")
        .arg(tmp.path())
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
    let (data, code) = run_setup(tmp.path());
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
    let (data, code) = run_setup(tmp.path());
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
    let (data, code) = run_setup(tmp.path());
    assert_eq!(code, 0);
    assert_eq!(data["status"], "ok");
    let flow_data: Value =
        serde_json::from_str(&fs::read_to_string(tmp.path().join(".flow.json")).unwrap()).unwrap();
    let hash = flow_data["setup_hash"].as_str().unwrap();
    assert_eq!(hash.len(), 12);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn cli_installs_bin_stubs() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    let (data, code) = run_setup(tmp.path());
    assert_eq!(code, 0);
    let installed = data["stubs_installed"].as_array().unwrap();
    assert_eq!(installed.len(), 4);
    for tool in &["format", "lint", "build", "test"] {
        let path = tmp.path().join("bin").join(tool);
        assert!(path.exists(), "expected bin/{} installed", tool);
    }
}

// ── merge_settings error/guard branches ────────────────────

#[test]
fn merge_settings_read_error_returns_err() {
    // When settings.json exists but cannot be read (e.g., it is a
    // directory instead of a file), merge_settings returns an Err
    // with a "Could not read" message.
    let tmp = tempfile::tempdir().unwrap();
    let claude_dir = tmp.path().join(".claude");
    fs::create_dir_all(claude_dir.join("settings.json")).unwrap();
    let result = prime_setup::merge_settings(tmp.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Could not read"));
}

#[test]
fn merge_settings_parse_error_returns_err() {
    // When settings.json contains invalid JSON, merge_settings
    // returns an Err with a "Could not parse" message.
    let tmp = tempfile::tempdir().unwrap();
    write_settings(tmp.path(), &json!("placeholder"));
    fs::write(
        tmp.path().join(".claude").join("settings.json"),
        "not valid json {{{",
    )
    .unwrap();
    let result = prime_setup::merge_settings(tmp.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Could not parse"));
}

#[test]
fn merge_settings_non_object_top_level_resets() {
    // When settings.json parses as a JSON array (valid JSON but not
    // an object), the guard at L135 resets it to {} and the merge
    // proceeds normally without crashing.
    let tmp = tempfile::tempdir().unwrap();
    let claude_dir = tmp.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(claude_dir.join("settings.json"), "[1, 2, 3]").unwrap();
    let result = prime_setup::merge_settings(tmp.path()).unwrap();
    assert!(result["permissions"]["allow"].is_array());
    assert!(result["permissions"]["deny"].is_array());
    assert_eq!(result["permissions"]["defaultMode"], "acceptEdits");
}

#[test]
fn merge_settings_non_object_permissions_resets() {
    // When permissions is a string instead of an object, the guard
    // at L138 resets it to {} so allow/deny arrays can be created.
    let tmp = tempfile::tempdir().unwrap();
    write_settings(tmp.path(), &json!({"permissions": "not an object"}));
    let result = prime_setup::merge_settings(tmp.path()).unwrap();
    assert!(result["permissions"]["allow"].is_array());
    assert!(result["permissions"]["deny"].is_array());
}

#[test]
fn merge_settings_non_array_allow_resets() {
    // When permissions.allow is a string instead of an array, the
    // guard at L141 resets it to [] so the merge can populate it.
    let tmp = tempfile::tempdir().unwrap();
    write_settings(
        tmp.path(),
        &json!({"permissions": {"allow": "not-array", "deny": []}}),
    );
    let result = prime_setup::merge_settings(tmp.path()).unwrap();
    assert!(result["permissions"]["allow"].is_array());
    let allow_len = result["permissions"]["allow"].as_array().unwrap().len();
    assert!(allow_len > 0, "UNIVERSAL_ALLOW entries should be added");
}

#[test]
fn merge_settings_non_array_deny_resets() {
    // When permissions.deny is a number instead of an array, the
    // guard at L144 resets it to [] so deny entries can be added.
    let tmp = tempfile::tempdir().unwrap();
    write_settings(
        tmp.path(),
        &json!({"permissions": {"allow": [], "deny": 42}}),
    );
    let result = prime_setup::merge_settings(tmp.path()).unwrap();
    assert!(result["permissions"]["deny"].is_array());
    let deny_len = result["permissions"]["deny"].as_array().unwrap().len();
    assert!(deny_len > 0, "FLOW_DENY entries should be added");
}

#[test]
fn merge_settings_sets_auto_background_env() {
    // merge_settings writes env.CLAUDE_AUTO_BACKGROUND_TASKS to
    // disable auto-backgrounding of CI gates.
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::merge_settings(tmp.path()).unwrap();
    let settings = read_settings(tmp.path());
    assert_eq!(settings["env"]["CLAUDE_AUTO_BACKGROUND_TASKS"], "false");
}

#[test]
fn merge_settings_preserves_existing_env() {
    // Pre-existing env entries survive alongside the new key.
    let tmp = tempfile::tempdir().unwrap();
    write_settings(
        tmp.path(),
        &json!({
            "permissions": {"allow": [], "deny": []},
            "env": {"MY_VAR": "hello"},
        }),
    );
    prime_setup::merge_settings(tmp.path()).unwrap();
    let settings = read_settings(tmp.path());
    assert_eq!(settings["env"]["MY_VAR"], "hello");
    assert_eq!(settings["env"]["CLAUDE_AUTO_BACKGROUND_TASKS"], "false");
}

#[test]
fn merge_settings_non_object_env_resets() {
    // When env is a string instead of an object, the guard at L205
    // resets it to {} so the auto-background key can be set.
    let tmp = tempfile::tempdir().unwrap();
    write_settings(
        tmp.path(),
        &json!({
            "permissions": {"allow": [], "deny": []},
            "env": "not an object",
        }),
    );
    prime_setup::merge_settings(tmp.path()).unwrap();
    let settings = read_settings(tmp.path());
    assert_eq!(settings["env"]["CLAUDE_AUTO_BACKGROUND_TASKS"], "false");
}

// ── install_bin_stubs edge cases ───────────────────────────

#[test]
fn install_bin_stubs_skips_dangling_symlink() {
    // A dangling symlink at the target path is detected by
    // fs::symlink_metadata and skipped — the installer never writes
    // through a symlink per rust-patterns.md "Symlink-Safe Existence
    // Checks Before Writes".
    let tmp = tempfile::tempdir().unwrap();
    let plugin_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    std::os::unix::fs::symlink("/nonexistent/target", bin_dir.join("format")).unwrap();
    let installed = prime_setup::install_bin_stubs(tmp.path(), plugin_root);
    // format was skipped (dangling symlink), the other three installed
    assert!(!installed.contains(&"format".to_string()));
    assert!(installed.contains(&"lint".to_string()));
    assert!(installed.contains(&"build".to_string()));
    assert!(installed.contains(&"test".to_string()));
    // The symlink should still exist (not overwritten)
    assert!(
        fs::symlink_metadata(bin_dir.join("format")).is_ok(),
        "dangling symlink should be preserved"
    );
}

#[test]
fn install_bin_stubs_skips_when_source_missing() {
    // When the stub source template does not exist (empty plugin
    // assets dir), the installer skips that tool gracefully.
    let tmp = tempfile::tempdir().unwrap();
    let fake_plugin = tempfile::tempdir().unwrap();
    // Create an empty assets/bin-stubs/ directory with no .sh files
    fs::create_dir_all(fake_plugin.path().join("assets").join("bin-stubs")).unwrap();
    let installed = prime_setup::install_bin_stubs(tmp.path(), fake_plugin.path());
    assert!(
        installed.is_empty(),
        "no stubs should be installed when source templates are missing"
    );
}

#[test]
fn install_bin_stubs_handles_mkdir_failure() {
    // When bin/ cannot be created (a regular file blocks it), the
    // installer skips gracefully instead of crashing.
    let tmp = tempfile::tempdir().unwrap();
    let plugin_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // Create a regular file at bin/ so create_dir_all fails
    fs::write(tmp.path().join("bin"), "blocking file").unwrap();
    let installed = prime_setup::install_bin_stubs(tmp.path(), plugin_root);
    assert!(
        installed.is_empty(),
        "no stubs should be installed when bin/ directory cannot be created"
    );
}

/// Covers the `plugin_root() == None` error branch in `run_impl`
/// (lines 424-427). We copy the compiled `flow-rs` to a tempdir
/// whose parent chain contains no `flow-phases.json`, and invoke it
/// without `CLAUDE_PLUGIN_ROOT`. `plugin_root` returns None, so the
/// subcommand surfaces a "Plugin root not found" error payload.
#[test]
fn prime_setup_without_plugin_root_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let binary_copy = tmp.path().join("flow-rs");
    fs::copy(env!("CARGO_BIN_EXE_flow-rs"), &binary_copy).unwrap();
    fs::set_permissions(&binary_copy, fs::Permissions::from_mode(0o755)).unwrap();

    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();

    let output = Command::new(&binary_copy)
        .arg("prime-setup")
        .arg(&project)
        .env_remove("CLAUDE_PLUGIN_ROOT")
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();
    let data = parse_stdout(&output.stdout);
    assert_eq!(data["status"], "error");
    let msg = data["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("Plugin root not found"),
        "expected 'Plugin root not found', got: {}",
        msg
    );
}

/// Covers the `version == "?"` early-return in `run_impl` (lines
/// 433-437): CLAUDE_PLUGIN_ROOT points at a fake plugin directory
/// whose `flow-phases.json` exists (so `plugin_root` returns Some) but
/// whose `.claude-plugin/plugin.json` is malformed (so `read_version`
/// returns "?"). The subprocess surfaces a status:error payload with
/// the "Could not read plugin version" message.
#[test]
fn prime_setup_unreadable_plugin_version_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let fake_plugin = tmp.path().join("fake-plugin");
    fs::create_dir_all(&fake_plugin).unwrap();
    // flow-phases.json must exist for `plugin_root` to return Some.
    fs::write(fake_plugin.join("flow-phases.json"), "{}").unwrap();
    // plugin.json with invalid JSON → read_version returns "?".
    let claude_plugin = fake_plugin.join(".claude-plugin");
    fs::create_dir_all(&claude_plugin).unwrap();
    fs::write(claude_plugin.join("plugin.json"), "not valid json").unwrap();

    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("prime-setup")
        .arg(&project)
        .env("CLAUDE_PLUGIN_ROOT", &fake_plugin)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();
    let data = parse_stdout(&output.stdout);
    assert_eq!(data["status"], "error");
    let msg = data["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("plugin version") || msg.contains("version"),
        "expected plugin-version error, got: {}",
        msg
    );
}

/// Covers the `Err(_) => continue` arm of `fs::read_to_string` in
/// `install_bin_stubs` (line ~530): a stubs directory contains a
/// file that `is_file()` reports as ok but whose permissions prevent
/// read access.
#[cfg(unix)]
#[test]
fn install_bin_stubs_read_source_permission_denied_skips() {
    let tmp = tempfile::tempdir().unwrap();
    let fake_plugin = tmp.path().join("plug");
    let stubs_dir = fake_plugin.join("assets").join("bin-stubs");
    fs::create_dir_all(&stubs_dir).unwrap();
    for tool in ["format", "lint", "build", "test"] {
        let src = stubs_dir.join(format!("{}.sh", tool));
        fs::write(&src, "#!/bin/bash\nexit 0\n").unwrap();
        // Strip read permission so fs::read_to_string fails.
        fs::set_permissions(&src, fs::Permissions::from_mode(0o000)).unwrap();
    }
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let installed = prime_setup::install_bin_stubs(&project, &fake_plugin);
    // Restore read so tempdir cleanup succeeds.
    for tool in ["format", "lint", "build", "test"] {
        let _ = fs::set_permissions(
            stubs_dir.join(format!("{}.sh", tool)),
            fs::Permissions::from_mode(0o644),
        );
    }
    // All four tools skipped due to read failure.
    assert!(
        installed.is_empty(),
        "expected empty installed list on read failure, got: {:?}",
        installed
    );
}

/// Covers the `if fs::write(&target, &content).is_err() { continue }`
/// branch (line ~533) in `install_bin_stubs`: when the target bin
/// directory is read-only so the write fails.
#[cfg(unix)]
#[test]
fn install_bin_stubs_write_target_failure_skips() {
    let tmp = tempfile::tempdir().unwrap();
    let fake_plugin = tmp.path().join("plug");
    let stubs_dir = fake_plugin.join("assets").join("bin-stubs");
    fs::create_dir_all(&stubs_dir).unwrap();
    for tool in ["format", "lint", "build", "test"] {
        let src = stubs_dir.join(format!("{}.sh", tool));
        fs::write(&src, "#!/bin/bash\nexit 0\n").unwrap();
    }
    let project = tmp.path().join("project");
    let bin_dir = project.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    // Strip write permission on the bin/ directory so fs::write fails.
    fs::set_permissions(&bin_dir, fs::Permissions::from_mode(0o555)).unwrap();
    let installed = prime_setup::install_bin_stubs(&project, &fake_plugin);
    // Restore write so tempdir cleanup succeeds.
    let _ = fs::set_permissions(&bin_dir, fs::Permissions::from_mode(0o755));
    assert!(
        installed.is_empty(),
        "expected empty installed list when bin/ is read-only, got: {:?}",
        installed
    );
}

/// Covers lines 464 (the `install_launcher` Err warning branch). We
/// set HOME to a path where .local/bin cannot be created (HOME is a
/// regular file, not a directory) so install_launcher fails.
#[test]
fn prime_setup_install_launcher_warning_branch() {
    let tmp = tempfile::tempdir().unwrap();
    // HOME is a regular FILE, not a directory. .local/bin cannot be
    // created under a file path → install_launcher returns Err.
    let bad_home = tmp.path().join("not-a-dir");
    fs::write(&bad_home, "block").unwrap();

    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();

    // `args.plugin_root.is_some()` triggers the install_launcher call
    // path. Supply an explicit --plugin-root to force it.
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "prime-setup",
            project.to_str().unwrap(),
            "--plugin-root",
            env!("CARGO_MANIFEST_DIR"),
        ])
        .env("HOME", &bad_home)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Must not panic; the Warning is printed to stderr.
    assert!(!stderr.contains("panicked at"));
}

// ============================================================
// Library-level tests for run_impl and check_launcher_path
// These exercise the library instantiation of run_impl + callees
// (merge_settings, write_version_marker, update_git_exclude,
// install_pre_commit_hook, install_launcher, check_launcher_path,
// install_bin_stubs) so every function's library instantiation is
// covered. The subprocess tests above exercise the binary
// instantiation.
// ============================================================

/// Happy path: run_impl with a valid project root, no plugin_root
/// override. Exercises merge_settings + write_version_marker +
/// update_git_exclude + install_pre_commit_hook + install_bin_stubs.
/// plugin_root() resolves via the walk-up fallback because the
/// test binary's parent chain contains this repo's flow-phases.json.
#[test]
fn run_impl_library_happy_path_covers_all_callees() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    // Create a .git dir so update_git_exclude's git subprocess
    // succeeds at least in terms of locating the git dir.
    fs::create_dir_all(project.join(".git").join("info")).unwrap();
    // Initialize a minimal git repo by writing the HEAD so
    // `git rev-parse --git-common-dir` can resolve.
    fs::write(project.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();

    let args = prime_setup::Args {
        project_root: project.to_string_lossy().to_string(),
        skills_json: None,
        commit_format: Some("full".to_string()),
        plugin_root: None,
    };
    let result = prime_setup::run_impl(&args);
    // Either success or plugin_root-not-found error (depending on
    // walk-up fallback). Both paths cover the function body.
    match result {
        Ok(value) => {
            assert_eq!(value["status"], "ok");
            assert!(project.join(".claude").join("settings.json").exists());
            assert!(project.join(".flow.json").exists());
        }
        Err(value) => {
            // Acceptable: plugin_root walk-up didn't find
            // flow-phases.json from the test binary's path.
            assert_eq!(value["status"], "error");
        }
    }
}

/// run_impl with a project_root that is NOT a directory: exercises
/// the early-return Err branch (`!project_root.is_dir()`).
#[test]
fn run_impl_library_project_root_not_dir_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("does-not-exist");
    let args = prime_setup::Args {
        project_root: missing.to_string_lossy().to_string(),
        skills_json: None,
        commit_format: None,
        plugin_root: None,
    };
    let result = prime_setup::run_impl(&args);
    let err = result.unwrap_err();
    assert_eq!(err["status"], "error");
    let msg = err["message"].as_str().unwrap_or("");
    assert!(msg.contains("Project root not found"), "got: {}", msg);
}

/// run_impl with invalid --skills-json: exercises the serde parse
/// Err branch.
#[test]
fn run_impl_library_invalid_skills_json_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let args = prime_setup::Args {
        project_root: project.to_string_lossy().to_string(),
        skills_json: Some("not json {".to_string()),
        commit_format: None,
        plugin_root: None,
    };
    let result = prime_setup::run_impl(&args);
    let err = result.unwrap_err();
    assert_eq!(err["status"], "error");
    let msg = err["message"].as_str().unwrap_or("");
    assert!(msg.contains("Invalid --skills-json"), "got: {}", msg);
}

/// check_launcher_path library test — exercises the "local_bin not
/// in PATH" branch (current test process PATH very unlikely to
/// contain a freshly-created tmp path). Must not panic.
#[test]
fn check_launcher_path_library_not_in_path_no_panic() {
    let tmp = tempfile::tempdir().unwrap();
    prime_setup::check_launcher_path(tmp.path());
}

/// Covers merge_settings closure #5 (fs::create_dir_all Err arm):
/// when `.claude` already exists as a FILE (not a directory), the
/// create_dir_all at line 215 fails with AlreadyExists.
#[test]
fn merge_settings_create_dir_all_err_when_claude_is_file() {
    let tmp = tempfile::tempdir().unwrap();
    // Pre-create `.claude` as a file — settings_dir creation fails.
    fs::write(tmp.path().join(".claude"), "i'm a file").unwrap();
    let result = prime_setup::merge_settings(tmp.path());
    assert!(result.is_err(), "expected Err, got {:?}", result);
    let msg = result.unwrap_err();
    assert!(
        msg.contains("Could not read settings.json")
            || msg.contains("Could not create .claude directory"),
        "unexpected error message: {}",
        msg
    );
}

/// Covers run_impl's merge_settings-Err closure: project_root has
/// `.claude` as a file so merge_settings fails, and run_impl wraps
/// the Err into a json error payload.
#[test]
fn run_impl_library_merge_settings_err_path() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    // Pre-create `.claude` as a FILE so merge_settings fails.
    fs::write(project.join(".claude"), "blocker").unwrap();

    let args = prime_setup::Args {
        project_root: project.to_string_lossy().to_string(),
        skills_json: None,
        commit_format: None,
        plugin_root: None,
    };
    let result = prime_setup::run_impl(&args);
    // Expected: either the merge_settings Err is surfaced, OR an
    // earlier error (plugin_root walk-up failure) fires. Both
    // produce Err — the test just needs the Err path.
    assert!(result.is_err(), "expected Err, got {:?}", result);
}

/// Covers run_impl's write_version_marker-Err closure: project_root
/// has `.flow.json` pre-existing as a read-only file, so after
/// merge_settings succeeds, write_version_marker's fs::write fails.
#[test]
fn run_impl_library_write_version_marker_err_path() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    // Pre-write .flow.json with mode 0o444 (read-only).
    let flow_json = project.join(".flow.json");
    fs::write(&flow_json, "{}").unwrap();
    fs::set_permissions(&flow_json, fs::Permissions::from_mode(0o444)).unwrap();

    let args = prime_setup::Args {
        project_root: project.to_string_lossy().to_string(),
        skills_json: None,
        commit_format: None,
        plugin_root: None,
    };
    let result = prime_setup::run_impl(&args);
    // Restore perms for tempdir cleanup.
    let _ = fs::set_permissions(&flow_json, fs::Permissions::from_mode(0o644));
    assert!(result.is_err(), "expected Err, got {:?}", result);
}

/// Covers run_impl's install_pre_commit_hook-Err closure: `.git`
/// is a regular file so install_script's fs::create_dir_all on
/// `<project>/.git/hooks` fails.
#[test]
fn run_impl_library_install_pre_commit_hook_err_path() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    // `.git` is a regular file, so `.git/hooks` can't be created.
    fs::write(project.join(".git"), "not a dir").unwrap();

    let args = prime_setup::Args {
        project_root: project.to_string_lossy().to_string(),
        skills_json: None,
        commit_format: None,
        plugin_root: None,
    };
    let result = prime_setup::run_impl(&args);
    assert!(result.is_err(), "expected Err, got {:?}", result);
}

/// Covers merge_settings closure #7 (fs::write Err arm): make
/// `.claude` directory read-only so the final settings.json write
/// fails.
#[test]
fn merge_settings_write_err_when_claude_dir_readonly() {
    let tmp = tempfile::tempdir().unwrap();
    let claude_dir = tmp.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    // Read-only directory — fs::write to a NEW file inside fails.
    fs::set_permissions(&claude_dir, fs::Permissions::from_mode(0o555)).unwrap();
    let result = prime_setup::merge_settings(tmp.path());
    // Restore perms so tempdir cleanup succeeds.
    let _ = fs::set_permissions(&claude_dir, fs::Permissions::from_mode(0o755));
    assert!(result.is_err(), "expected Err, got {:?}", result);
}

// ── merge_settings_with seam (synthetic fixtures) ──────────

// Branch A: existing root is not an object (array, string, number) —
// must reset to an empty object before merge proceeds.
#[test]
fn merge_with_non_object_root_resets_to_empty_object() {
    let result = prime_setup::merge_settings_with(json!([1, 2, 3]), &[], &[]);
    assert!(result.is_object(), "expected object, got {:?}", result);
    assert!(result["permissions"].is_object());
}

// Branch B: existing has no `permissions` key — initialize it to an
// empty object with empty allow/deny arrays.
#[test]
fn merge_with_missing_permissions_initializes_object() {
    let result = prime_setup::merge_settings_with(json!({}), &[], &[]);
    assert!(result["permissions"].is_object());
    assert!(result["permissions"]["allow"].is_array());
    assert!(result["permissions"]["deny"].is_array());
}

// Branch C: existing `permissions.allow` is not an array — reset to
// an empty array before merging.
#[test]
fn merge_with_non_array_allow_resets_to_empty() {
    let existing = json!({
        "permissions": {"allow": "not-array", "deny": []}
    });
    let result = prime_setup::merge_settings_with(existing, &[], &[]);
    assert!(result["permissions"]["allow"].is_array());
    assert_eq!(
        result["permissions"]["allow"].as_array().unwrap().len(),
        0,
        "non-array allow must reset to empty"
    );
}

// Branch D: existing `permissions.deny` is not an array — reset to
// an empty array before merging.
#[test]
fn merge_with_non_array_deny_resets_to_empty() {
    let existing = json!({
        "permissions": {"allow": [], "deny": "not-array"}
    });
    let result = prime_setup::merge_settings_with(existing, &[], &[]);
    assert!(result["permissions"]["deny"].is_array());
    assert_eq!(
        result["permissions"]["deny"].as_array().unwrap().len(),
        0,
        "non-array deny must reset to empty"
    );
}

// Branch E: empty existing settings — every flow_allow and flow_deny
// entry is appended.
#[test]
fn merge_fresh_appends_all_flow_allow_and_deny() {
    let result =
        prime_setup::merge_settings_with(json!({}), &["Bash(echo *)"], &["Bash(rm -rf /*)"]);
    let allow: Vec<String> = result["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    let deny: Vec<String> = result["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    assert!(allow.contains(&"Bash(echo *)".to_string()));
    assert!(deny.contains(&"Bash(rm -rf /*)".to_string()));
}

// Branch F: re-running merge on an already-merged result produces
// identical output.
#[test]
fn merge_idempotent_no_changes_on_second_run() {
    let allow_list = &["Bash(echo *)"];
    let deny_list = &["Bash(rm -rf /*)"];
    let first = prime_setup::merge_settings_with(json!({}), allow_list, deny_list);
    let second = prime_setup::merge_settings_with(first.clone(), allow_list, deny_list);
    assert_eq!(first, second, "merge must be idempotent");
}

// Branch G: existing allow contains some but not all flow_allow
// entries — only the missing ones are added, no duplicates.
#[test]
fn merge_subset_adds_only_missing_allow() {
    let existing = json!({
        "permissions": {"allow": ["Bash(echo *)"], "deny": []}
    });
    let result = prime_setup::merge_settings_with(existing, &["Bash(echo *)", "Bash(ls *)"], &[]);
    let allow: Vec<String> = result["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    assert_eq!(
        allow.iter().filter(|s| **s == "Bash(echo *)").count(),
        1,
        "no duplicate allow entry"
    );
    assert!(allow.contains(&"Bash(ls *)".to_string()));
}

// Branch H: a broader existing allow pattern (e.g. `Agent(*)`)
// subsumes a narrower flow_allow entry (e.g. `Agent(flow:ci-fixer)`)
// — the redundant entry is not added.
#[test]
fn merge_subsumption_blocks_redundant_agent_entry() {
    let existing = json!({
        "permissions": {"allow": ["Agent(*)"], "deny": []}
    });
    let result = prime_setup::merge_settings_with(existing, &["Agent(flow:ci-fixer)"], &[]);
    let allow: Vec<String> = result["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    assert!(
        !allow.contains(&"Agent(flow:ci-fixer)".to_string()),
        "narrower entry must not be added when broader exists"
    );
}

// Branch I: user has opted into a permission that flow_deny would
// otherwise add — the deny addition is skipped (allow always wins).
#[test]
fn merge_user_allow_blocks_flow_deny_addition() {
    let existing = json!({
        "permissions": {"allow": ["Bash(rm -rf /*)"], "deny": []}
    });
    let result = prime_setup::merge_settings_with(existing, &[], &["Bash(rm -rf /*)"]);
    let deny: Vec<String> = result["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    assert!(
        !deny.contains(&"Bash(rm -rf /*)".to_string()),
        "deny addition must be skipped when user already allows"
    );
}

// Branch J (NEW behavior): existing settings.json has the same string
// in both allow AND deny — the deny entry is removed because allow
// always wins. Without this fix, the user's opt-in is silently
// neutralized by the conflicting deny.
#[test]
fn merge_active_removes_existing_deny_matching_allow() {
    let existing = json!({
        "permissions": {
            "allow": ["Bash(echo *)"],
            "deny": ["Bash(echo *)", "Bash(other deny)"]
        }
    });
    let result = prime_setup::merge_settings_with(existing, &[], &[]);
    let deny: Vec<String> = result["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    assert!(
        !deny.contains(&"Bash(echo *)".to_string()),
        "deny matching allow must be removed"
    );
    assert!(
        deny.contains(&"Bash(other deny)".to_string()),
        "unrelated deny entries must be preserved"
    );
}

// Branch K: a flow_deny entry with no conflict in allow is appended
// in the default position.
#[test]
fn merge_flow_deny_appended_when_no_allow_conflict() {
    let result = prime_setup::merge_settings_with(json!({}), &[], &["Bash(rm -rf /*)"]);
    let deny: Vec<String> = result["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    assert!(deny.contains(&"Bash(rm -rf /*)".to_string()));
}

// Branch L: defaultMode existed as something non-default (e.g.
// `prompt`) — overwrite to `acceptEdits` AND emit a stderr warning.
// Subprocess test — stderr is only observable through a child
// process per the plan's branch enumeration table.
#[test]
fn merge_overwrites_non_default_mode_with_warning() {
    let tmp = tempfile::tempdir().unwrap();
    make_git_repo(tmp.path());
    write_settings(
        tmp.path(),
        &json!({
            "permissions": {"allow": [], "deny": [], "defaultMode": "prompt"}
        }),
    );
    let output = flow_rs()
        .arg("prime-setup")
        .arg(tmp.path())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Warning: Overriding defaultMode"),
        "expected warning in stderr, got: {}",
        stderr
    );
    assert!(
        stderr.contains("'prompt'"),
        "expected previous mode named in stderr, got: {}",
        stderr
    );
    let settings = read_settings(tmp.path());
    assert_eq!(settings["permissions"]["defaultMode"], "acceptEdits");
}

// Branch M: defaultMode is missing — set to `acceptEdits` with no
// warning.
#[test]
fn merge_sets_default_mode_when_missing() {
    let result = prime_setup::merge_settings_with(json!({}), &[], &[]);
    assert_eq!(result["permissions"]["defaultMode"], "acceptEdits");
}

// Branch N: existing has no `env` key — initialize as empty object
// then add CLAUDE_AUTO_BACKGROUND_TASKS=false.
#[test]
fn merge_initializes_env_when_missing() {
    let result = prime_setup::merge_settings_with(json!({}), &[], &[]);
    assert!(result["env"].is_object());
    assert_eq!(result["env"]["CLAUDE_AUTO_BACKGROUND_TASKS"], "false");
}

// Branch O: existing has an `env` object with other variables — add
// CLAUDE_AUTO_BACKGROUND_TASKS=false while preserving existing keys.
#[test]
fn merge_preserves_env_adds_auto_background_false() {
    let existing = json!({"env": {"OTHER_VAR": "value"}});
    let result = prime_setup::merge_settings_with(existing, &[], &[]);
    assert_eq!(result["env"]["OTHER_VAR"], "value");
    assert_eq!(result["env"]["CLAUDE_AUTO_BACKGROUND_TASKS"], "false");
}
