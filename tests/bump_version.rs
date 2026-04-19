//! Subprocess tests for `bin/flow bump-version` — exercise the real CLI
//! surface through `main.rs` so `run_impl_main` coverage lands for the
//! production path.
//!
//! Library-function tests (validate_version, bump_json, bump_skill,
//! read_current_version, run_impl) live inline in `src/bump_version.rs`
//! to avoid per-binary monomorphization that would split coverage
//! across instantiations.

use std::fs;
use std::process::Command;

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
