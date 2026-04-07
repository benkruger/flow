//! Shared test helpers for start_* integration tests.
//!
//! Each integration test file in `tests/` is compiled as its own crate.
//! This module is included via `mod common;` and provides helpers that
//! were previously duplicated across start_init, start_gate, start_workspace,
//! start_finalize, and start_setup test files.

#![allow(dead_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

/// Read current plugin version from .claude-plugin/plugin.json.
pub fn current_plugin_version() -> String {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let plugin_path = manifest_dir.join(".claude-plugin").join("plugin.json");
    let content = fs::read_to_string(&plugin_path).expect("plugin.json must exist");
    let data: Value = serde_json::from_str(&content).expect("plugin.json must be valid JSON");
    data["version"]
        .as_str()
        .expect("plugin.json must have version")
        .to_string()
}

/// Create a bare+clone git repo pair for testing.
pub fn create_git_repo_with_remote(parent: &Path) -> PathBuf {
    let bare = parent.join("bare.git");
    let repo = parent.join("repo");

    Command::new("git")
        .args(["init", "--bare", "-b", "main", &bare.to_string_lossy()])
        .output()
        .unwrap();

    Command::new("git")
        .args(["clone", &bare.to_string_lossy(), &repo.to_string_lossy()])
        .output()
        .unwrap();

    for (key, val) in [
        ("user.email", "test@test.com"),
        ("user.name", "Test"),
        ("commit.gpgsign", "false"),
    ] {
        Command::new("git")
            .args(["config", key, val])
            .current_dir(&repo)
            .output()
            .unwrap();
    }

    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&repo)
        .output()
        .unwrap();

    Command::new("git")
        .args(["push", "-u", "origin", "main"])
        .current_dir(&repo)
        .output()
        .unwrap();

    repo
}

/// Write .flow.json with version, framework, and optional skills.
pub fn write_flow_json(repo: &Path, version: &str, framework: &str, skills: Option<&Value>) {
    let mut data = json!({
        "flow_version": version,
        "framework": framework,
    });
    if let Some(sk) = skills {
        data["skills"] = sk.clone();
    }
    fs::write(repo.join(".flow.json"), data.to_string()).unwrap();
}

/// Create a custom gh stub script. Returns the stub directory.
pub fn create_gh_stub(repo: &Path, script: &str) -> PathBuf {
    let stub_dir = repo.join(".stub-bin");
    fs::create_dir_all(&stub_dir).unwrap();
    let gh_stub = stub_dir.join("gh");
    fs::write(&gh_stub, script).unwrap();
    fs::set_permissions(&gh_stub, fs::Permissions::from_mode(0o755)).unwrap();
    stub_dir
}

/// Parse JSON from the last line of stdout (child-inheriting pattern).
pub fn parse_output(output: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last_line = stdout.trim().lines().last().unwrap_or("");
    serde_json::from_str(last_line).unwrap_or_else(|_| json!({"raw": stdout.trim()}))
}
