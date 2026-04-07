//! Tests for `src/bump_version.rs` — port of tests/test_bump_version.py.
//!
//! Pure-function tests call the library directly. CLI tests call run_impl
//! with a fake repo fixture. All subprocess calls use Command::output()
//! per rust-port-parity.md Test-Module Subprocess Stdio rule.

use std::fs;
use std::path::Path;

use flow_rs::bump_version::{bump_json, bump_skill, read_current_version, run_impl, validate_version, Args};

// --- validate_version ---

#[test]
fn test_validate_version_valid() {
    assert!(validate_version("1.2.3"));
    assert!(validate_version("0.0.0"));
    assert!(validate_version("10.20.30"));
}

#[test]
fn test_validate_version_invalid() {
    assert!(!validate_version("v1.2.3"));
    assert!(!validate_version("1.2"));
    assert!(!validate_version("abc"));
    assert!(!validate_version("../../etc/passwd"));
}

// --- read_current_version ---

#[test]
fn test_read_current_version() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("plugin.json");
    fs::write(&p, r#"{"version": "2.5.0"}"#).unwrap();
    assert_eq!(read_current_version(&p).unwrap(), "2.5.0");
}

// --- bump_json ---

#[test]
fn test_bump_json_updates_version() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("test.json");
    fs::write(&p, "{\n  \"version\": \"1.0.0\"\n}").unwrap();
    assert!(bump_json(&p, "1.0.0", "2.0.0").unwrap());
    let data: serde_json::Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
    assert_eq!(data["version"], "2.0.0");
}

#[test]
fn test_bump_json_no_match() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("test.json");
    fs::write(&p, "{\n  \"version\": \"3.0.0\"\n}").unwrap();
    assert!(!bump_json(&p, "1.0.0", "2.0.0").unwrap());
}

#[test]
fn test_bump_json_multiple_version_fields() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("marketplace.json");
    fs::write(
        &p,
        r#"{
  "name": "flow-marketplace",
  "metadata": {"version": "1.0.0"},
  "plugins": [{"name": "flow", "version": "1.0.0"}]
}"#,
    )
    .unwrap();
    assert!(bump_json(&p, "1.0.0", "2.0.0").unwrap());
    let data: serde_json::Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
    assert_eq!(data["metadata"]["version"], "2.0.0");
    assert_eq!(data["plugins"][0]["version"], "2.0.0");
}

// --- bump_skill ---

#[test]
fn test_bump_skill_replaces_banners() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("SKILL.md");
    fs::write(&p, "  FLOW v1.0.0 — Start\n  FLOW v1.0.0 — End\n").unwrap();
    assert!(bump_skill(&p, "1.0.0", "2.0.0").unwrap());
    let text = fs::read_to_string(&p).unwrap();
    assert!(text.contains("FLOW v2.0.0"));
    assert!(!text.contains("FLOW v1.0.0"));
}

#[test]
fn test_bump_skill_no_match() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("SKILL.md");
    fs::write(&p, "No version here\n").unwrap();
    assert!(!bump_skill(&p, "1.0.0", "2.0.0").unwrap());
}

// --- CLI integration tests via run_impl ---

/// Helper: create a minimal fake repo structure for bump_version tests.
fn fake_repo(dir: &Path) {
    let plugin_dir = dir.join(".claude-plugin");
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

    let skills_dir = dir.join("skills");
    for name in &["flow-start", "flow-code"] {
        let skill_dir = skills_dir.join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "# Skill\n\n```\n  FLOW v1.0.0 — Phase — STARTING\n```\n\n```\n  FLOW v1.0.0 — Phase — COMPLETE\n```\n",
        )
        .unwrap();
    }

    let release_dir = dir.join(".claude").join("skills").join("flow-release");
    fs::create_dir_all(&release_dir).unwrap();
    fs::write(
        release_dir.join("SKILL.md"),
        "# Release\n\n```\n  FLOW v1.0.0 — release — STARTING\n```\n",
    )
    .unwrap();
}

#[test]
fn test_cli_successful_bump() {
    let dir = tempfile::tempdir().unwrap();
    fake_repo(dir.path());

    let args = Args {
        version: Some("2.0.0".to_string()),
    };
    let result = run_impl(&args, dir.path());
    assert!(result.is_ok(), "run_impl failed: {:?}", result.err());

    // Check plugin.json
    let data: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(dir.path().join(".claude-plugin/plugin.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(data["version"], "2.0.0");

    // Check marketplace.json
    let data: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(dir.path().join(".claude-plugin/marketplace.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(data["metadata"]["version"], "2.0.0");
    assert_eq!(data["plugins"][0]["version"], "2.0.0");

    // Check skill banners
    for entry in fs::read_dir(dir.path().join("skills")).unwrap() {
        let skill_file = entry.unwrap().path().join("SKILL.md");
        let text = fs::read_to_string(&skill_file).unwrap();
        assert!(text.contains("FLOW v2.0.0"), "Missing v2.0.0 in {:?}", skill_file);
        assert!(!text.contains("FLOW v1.0.0"), "Stale v1.0.0 in {:?}", skill_file);
    }

    // Check release skill
    let text = fs::read_to_string(
        dir.path()
            .join(".claude/skills/flow-release/SKILL.md"),
    )
    .unwrap();
    assert!(text.contains("FLOW v2.0.0"));
    assert!(!text.contains("FLOW v1.0.0"));
}

#[test]
fn test_cli_invalid_version() {
    let dir = tempfile::tempdir().unwrap();
    fake_repo(dir.path());

    let args = Args {
        version: Some("v1.0.0".to_string()),
    };
    let result = run_impl(&args, dir.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("invalid version format"));
}

#[test]
fn test_cli_same_version() {
    let dir = tempfile::tempdir().unwrap();
    fake_repo(dir.path());

    let args = Args {
        version: Some("1.0.0".to_string()),
    };
    let result = run_impl(&args, dir.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("already"));
}

#[test]
fn test_cli_no_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args { version: None };
    let result = run_impl(&args, dir.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Usage"));
}

#[test]
fn test_cli_plugin_json_not_found() {
    let dir = tempfile::tempdir().unwrap();
    // No fake_repo setup — empty directory

    let args = Args {
        version: Some("2.0.0".to_string()),
    };
    let result = run_impl(&args, dir.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not found"));
}
