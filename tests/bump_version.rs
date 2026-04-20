//! Tests for `bin/flow bump-version` and its library surface. Migrated
//! from inline `#[cfg(test)]` per `.claude/rules/test-placement.md`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use flow_rs::bump_version::{
    bump_json, bump_skill, read_current_version, run_impl, run_impl_main, validate_version,
};

/// Build a fake plugin root with `flow-phases.json` so `plugin_root()`
/// resolves via the env-var path, plus the standard fake_repo layout.
fn setup_plugin_root() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    fs::write(root.join("flow-phases.json"), "{}").unwrap();

    let plugin_dir = root.join(".claude-plugin");
    fs::create_dir_all(&plugin_dir).unwrap();
    fs::write(
        plugin_dir.join("plugin.json"),
        "{\n  \"name\": \"flow\",\n  \"version\": \"1.0.0\"\n}",
    )
    .unwrap();
    fs::write(
        plugin_dir.join("marketplace.json"),
        r#"{
  "name": "flow-marketplace",
  "metadata": {"version": "1.0.0"},
  "plugins": [{"name": "flow", "version": "1.0.0"}]
}"#,
    )
    .unwrap();

    let skills_dir = root.join("skills");
    for name in &["flow-start", "flow-code"] {
        let skill_dir = skills_dir.join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "# Skill\n\nFLOW v1.0.0 — Phase\n",
        )
        .unwrap();
    }

    let release_dir = root.join(".claude").join("skills").join("flow-release");
    fs::create_dir_all(&release_dir).unwrap();
    fs::write(
        release_dir.join("SKILL.md"),
        "# Release\n\nFLOW v1.0.0 — release\n",
    )
    .unwrap();

    (dir, root)
}

#[test]
fn run_subprocess_success_prints_message_and_exits_zero() {
    let (_dir, root) = setup_plugin_root();
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["bump-version", "2.0.0"])
        .env("CLAUDE_PLUGIN_ROOT", &root)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("spawn flow-rs");
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn run_subprocess_invalid_version_exits_one() {
    let (_dir, root) = setup_plugin_root();
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["bump-version", "v9.9.9"])
        .env("CLAUDE_PLUGIN_ROOT", &root)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("spawn flow-rs");
    assert_eq!(output.status.code(), Some(1));
}

// --- Library-level tests (migrated from inline `#[cfg(test)]`) ---

fn setup_repo(version: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let plugin_dir = root.join(".claude-plugin");
    fs::create_dir_all(&plugin_dir).unwrap();
    fs::write(
        plugin_dir.join("plugin.json"),
        format!("{{\"name\": \"flow\", \"version\": \"{}\"}}", version),
    )
    .unwrap();
    (dir, root)
}

#[test]
fn run_impl_main_none_returns_error_tuple() {
    let (msg, code) = run_impl_main(Some("2.0.0"), None);
    assert_eq!(code, 1);
    assert!(msg.contains("could not find FLOW plugin root"));
}

#[test]
fn run_impl_main_success_returns_message_with_code_zero() {
    let (_dir, root) = setup_repo("1.0.0");
    fs::write(
        root.join(".claude-plugin").join("marketplace.json"),
        r#"{
  "name": "flow-marketplace",
  "metadata": {"version": "1.0.0"},
  "plugins": [{"name": "flow", "version": "1.0.0"}]
}"#,
    )
    .unwrap();
    let (_msg, code) = run_impl_main(Some("2.0.0"), Some(root));
    assert_eq!(code, 0);
}

#[test]
fn run_impl_main_err_path_returns_msg_and_code_one() {
    let (_dir, root) = setup_repo("1.0.0");
    let (msg, code) = run_impl_main(Some("invalid_semver"), Some(root));
    assert_eq!(code, 1);
    assert!(msg.contains("invalid version format"));
}

#[test]
fn validate_version_semver() {
    assert!(validate_version("1.0.0"));
    assert!(validate_version("10.20.30"));
    assert!(!validate_version("1.0"));
    assert!(!validate_version("1.0.0-rc1"));
    assert!(!validate_version("v1.0.0"));
    assert!(!validate_version(""));
    assert!(!validate_version(".0.0"));
    assert!(!validate_version("1..0"));
    assert!(!validate_version("1.0."));
    assert!(!validate_version(".."));
}

#[test]
fn read_current_version_reads_plugin_json() {
    let (_dir, root) = setup_repo("1.2.3");
    let version = read_current_version(&root.join(".claude-plugin").join("plugin.json")).unwrap();
    assert_eq!(version, "1.2.3");
}

#[test]
fn read_current_version_missing_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let err = read_current_version(&dir.path().join("nonexistent.json")).unwrap_err();
    assert!(err.contains("Failed to read"));
}

#[test]
fn read_current_version_invalid_json_errors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plugin.json");
    fs::write(&path, "not json").unwrap();
    let err = read_current_version(&path).unwrap_err();
    assert!(err.contains("Invalid JSON"));
}

#[test]
fn read_current_version_missing_version_field_errors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plugin.json");
    fs::write(&path, r#"{"name": "flow"}"#).unwrap();
    let err = read_current_version(&path).unwrap_err();
    assert!(err.contains("No \"version\" field"));
}

#[test]
fn bump_json_replaces_version_string() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plugin.json");
    fs::write(&path, r#"{"version": "1.0.0", "name": "flow"}"#).unwrap();
    let changed = bump_json(&path, "1.0.0", "2.0.0").unwrap();
    assert!(changed);
    let contents = fs::read_to_string(&path).unwrap();
    assert!(contents.contains("\"version\": \"2.0.0\""));
}

#[test]
fn bump_json_no_change_when_version_absent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plugin.json");
    fs::write(&path, r#"{"version": "1.0.0"}"#).unwrap();
    let changed = bump_json(&path, "9.9.9", "2.0.0").unwrap();
    assert!(!changed);
}

#[test]
fn bump_skill_replaces_banner() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("SKILL.md");
    fs::write(&path, "FLOW v1.0.0 — Start\nbody").unwrap();
    let changed = bump_skill(&path, "1.0.0", "2.0.0").unwrap();
    assert!(changed);
    let contents = fs::read_to_string(&path).unwrap();
    assert!(contents.contains("FLOW v2.0.0"));
}

#[test]
fn bump_skill_no_change_when_banner_absent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("SKILL.md");
    fs::write(&path, "no banner here").unwrap();
    let changed = bump_skill(&path, "1.0.0", "2.0.0").unwrap();
    assert!(!changed);
}

#[test]
fn run_impl_missing_version_arg_errors() {
    let (_dir, root) = setup_repo("1.0.0");
    let err = run_impl(None, &root).unwrap_err();
    assert!(err.contains("Usage:"));
}

#[test]
fn run_impl_invalid_version_format_errors() {
    let (_dir, root) = setup_repo("1.0.0");
    let err = run_impl(Some("not-a-version"), &root).unwrap_err();
    assert!(err.contains("invalid version format"));
}

#[test]
fn run_impl_missing_plugin_json_errors() {
    let dir = tempfile::tempdir().unwrap();
    let err = run_impl(Some("2.0.0"), dir.path()).unwrap_err();
    assert!(err.contains("plugin.json"));
    assert!(err.contains("not found"));
}

#[test]
fn run_impl_same_version_errors() {
    let (_dir, root) = setup_repo("1.0.0");
    let err = run_impl(Some("1.0.0"), &root).unwrap_err();
    assert!(err.contains("already 1.0.0"));
}

#[test]
fn run_impl_bumps_plugin_json_and_reports() {
    let (_dir, root) = setup_repo("1.0.0");
    let output = run_impl(Some("2.0.0"), &root).unwrap();
    assert!(output.contains("Bumped 1.0.0 -> 2.0.0"));
    assert!(output.contains("plugin.json"));
    let contents = fs::read_to_string(root.join(".claude-plugin").join("plugin.json")).unwrap();
    assert!(contents.contains("\"version\": \"2.0.0\""));
}

#[test]
fn run_impl_bumps_marketplace_json_when_present() {
    let (_dir, root) = setup_repo("1.0.0");
    let marketplace = root.join(".claude-plugin").join("marketplace.json");
    fs::write(&marketplace, r#"{"version": "1.0.0"}"#).unwrap();
    let output = run_impl(Some("2.0.0"), &root).unwrap();
    assert!(output.contains("marketplace.json"));
}

#[test]
fn run_impl_bumps_skill_banners_sorted_by_name() {
    let (_dir, root) = setup_repo("1.0.0");
    let skills_dir = root.join("skills");
    for name in ["z-skill", "a-skill"] {
        let skill_dir = skills_dir.join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "FLOW v1.0.0 — Test\n").unwrap();
    }
    let hidden = skills_dir.join(".hidden");
    fs::create_dir_all(&hidden).unwrap();
    fs::write(hidden.join("SKILL.md"), "FLOW v1.0.0 — Hidden\n").unwrap();
    fs::create_dir_all(skills_dir.join("empty-skill")).unwrap();

    let output = run_impl(Some("2.0.0"), &root).unwrap();
    assert!(output.contains("a-skill"));
    assert!(output.contains("z-skill"));
    assert!(!output.contains(".hidden"));
    assert!(!output.contains("empty-skill"));
    let hidden_content = fs::read_to_string(hidden.join("SKILL.md")).unwrap();
    assert!(hidden_content.contains("FLOW v1.0.0"));
}

#[test]
fn bump_json_write_failure_errors() {
    let dir = tempfile::tempdir().unwrap();
    let readonly = dir.path().join("readonly");
    fs::create_dir_all(&readonly).unwrap();
    let path = readonly.join("plugin.json");
    fs::write(&path, r#"{"version": "1.0.0"}"#).unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&path, perms).unwrap();

    let err = bump_json(&path, "1.0.0", "2.0.0").unwrap_err();
    assert!(err.contains("Failed to write"));

    let mut perms = fs::metadata(&path).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(&path, perms).unwrap();
}

#[test]
fn bump_json_missing_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let err = bump_json(&dir.path().join("no-such.json"), "1.0.0", "2.0.0").unwrap_err();
    assert!(err.contains("Failed to read"));
}

#[test]
fn bump_skill_write_failure_errors() {
    let dir = tempfile::tempdir().unwrap();
    let readonly = dir.path().join("readonly");
    fs::create_dir_all(&readonly).unwrap();
    let path = readonly.join("SKILL.md");
    fs::write(&path, "FLOW v1.0.0 — Test").unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&path, perms).unwrap();

    let err = bump_skill(&path, "1.0.0", "2.0.0").unwrap_err();
    assert!(err.contains("Failed to write"));

    let mut perms = fs::metadata(&path).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(&path, perms).unwrap();
}

#[test]
fn bump_skill_missing_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let err = bump_skill(&dir.path().join("no-such.md"), "1.0.0", "2.0.0").unwrap_err();
    assert!(err.contains("Failed to read"));
}

#[test]
fn run_impl_skills_dir_is_file_errors() {
    let (_dir, root) = setup_repo("1.0.0");
    let skills_path = root.join("skills");
    fs::write(&skills_path, "I am a file, not a dir").unwrap();
    let err = run_impl(Some("2.0.0"), &root).unwrap_err();
    assert!(err.contains("Failed to read skills dir"));
}

#[test]
fn run_impl_bumps_flow_release_skill_when_present() {
    let (_dir, root) = setup_repo("1.0.0");
    let release_dir = root.join(".claude").join("skills").join("flow-release");
    fs::create_dir_all(&release_dir).unwrap();
    fs::write(release_dir.join("SKILL.md"), "FLOW v1.0.0 — Release\n").unwrap();
    let output = run_impl(Some("2.0.0"), &root).unwrap();
    assert!(output.contains("flow-release"));
}

#[test]
fn run_impl_marketplace_json_no_match_skips_push() {
    let (_dir, root) = setup_repo("1.0.0");
    fs::write(
        root.join(".claude-plugin").join("marketplace.json"),
        r#"{"version": "9.9.9"}"#,
    )
    .unwrap();
    let output = run_impl(Some("2.0.0"), &root).unwrap();
    assert!(!output.contains("marketplace.json"));
}

#[test]
fn run_impl_skill_file_no_match_skips_push() {
    let (_dir, root) = setup_repo("1.0.0");
    let skill_dir = root.join("skills").join("no-banner-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(skill_dir.join("SKILL.md"), "No banner here\n").unwrap();
    let output = run_impl(Some("2.0.0"), &root).unwrap();
    assert!(!output.contains("no-banner-skill"));
}

#[test]
fn run_impl_release_skill_no_match_skips_push() {
    let (_dir, root) = setup_repo("1.0.0");
    let release_dir = root.join(".claude").join("skills").join("flow-release");
    fs::create_dir_all(&release_dir).unwrap();
    fs::write(release_dir.join("SKILL.md"), "No banner here\n").unwrap();
    let output = run_impl(Some("2.0.0"), &root).unwrap();
    assert!(!output.contains("flow-release"));
}

#[test]
fn run_impl_propagates_read_current_version_err() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let plugin_dir = root.join(".claude-plugin");
    fs::create_dir_all(&plugin_dir).unwrap();
    fs::write(plugin_dir.join("plugin.json"), r#"{"name": "flow"}"#).unwrap();
    let err = run_impl(Some("2.0.0"), &root).unwrap_err();
    assert!(err.contains("No \"version\" field"));
}

#[test]
fn run_impl_propagates_bump_json_err_on_plugin_json() {
    let (_dir, root) = setup_repo("1.0.0");
    let plugin_json = root.join(".claude-plugin").join("plugin.json");
    let mut perms = fs::metadata(&plugin_json).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&plugin_json, perms).unwrap();
    let err = run_impl(Some("2.0.0"), &root).unwrap_err();
    assert!(err.contains("Failed to write"));

    let mut perms = fs::metadata(&plugin_json).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(&plugin_json, perms).unwrap();
}

#[test]
fn run_impl_propagates_bump_json_err_on_marketplace_json() {
    let (_dir, root) = setup_repo("1.0.0");
    let marketplace = root.join(".claude-plugin").join("marketplace.json");
    fs::write(&marketplace, r#"{"version": "1.0.0"}"#).unwrap();
    let mut perms = fs::metadata(&marketplace).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&marketplace, perms).unwrap();
    let err = run_impl(Some("2.0.0"), &root).unwrap_err();
    assert!(err.contains("Failed to write"));

    let mut perms = fs::metadata(&marketplace).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(&marketplace, perms).unwrap();
}

#[test]
fn run_impl_propagates_bump_skill_err_on_skill_file() {
    let (_dir, root) = setup_repo("1.0.0");
    let skill_dir = root.join("skills").join("a-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    let skill_file = skill_dir.join("SKILL.md");
    fs::write(&skill_file, "FLOW v1.0.0 — Test\n").unwrap();
    let mut perms = fs::metadata(&skill_file).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&skill_file, perms).unwrap();
    let err = run_impl(Some("2.0.0"), &root).unwrap_err();
    assert!(err.contains("Failed to write"));

    let mut perms = fs::metadata(&skill_file).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(&skill_file, perms).unwrap();
}

#[test]
fn run_impl_propagates_bump_skill_err_on_release_skill() {
    let (_dir, root) = setup_repo("1.0.0");
    let release_dir = root.join(".claude").join("skills").join("flow-release");
    fs::create_dir_all(&release_dir).unwrap();
    let release_skill = release_dir.join("SKILL.md");
    fs::write(&release_skill, "FLOW v1.0.0 — Release\n").unwrap();
    let mut perms = fs::metadata(&release_skill).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&release_skill, perms).unwrap();
    let err = run_impl(Some("2.0.0"), &root).unwrap_err();
    assert!(err.contains("Failed to write"));

    let mut perms = fs::metadata(&release_skill).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(&release_skill, perms).unwrap();
}
