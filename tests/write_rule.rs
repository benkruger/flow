//! Integration tests for `bin/flow write-rule`.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_git_repo_with_remote, parse_output};

fn run_write_rule(repo: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("write-rule")
        .args(args)
        .current_dir(repo)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

#[test]
fn write_rule_writes_content_and_deletes_source() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let content_file = dir.path().join("content.md");
    fs::write(&content_file, "# Rule Body\n\nContent here.\n").unwrap();
    let target = dir.path().join(".claude").join("rules").join("test.md");

    let output = run_write_rule(
        &repo,
        &[
            "--path",
            target.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["path"], target.to_string_lossy().as_ref());

    // Content written
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "# Rule Body\n\nContent here.\n"
    );
    // Source file deleted
    assert!(!content_file.exists());
}

#[test]
fn write_rule_missing_content_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let missing = dir.path().join("no-such.md");
    let target = dir.path().join("target.md");

    let output = run_write_rule(
        &repo,
        &[
            "--path",
            target.to_str().unwrap(),
            "--content-file",
            missing.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Could not read content file"));
}

#[test]
fn write_rule_overwrites_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let content_file = dir.path().join("new.md");
    fs::write(&content_file, "new body").unwrap();
    let target = dir.path().join("existing.md");
    fs::write(&target, "old body").unwrap();

    let output = run_write_rule(
        &repo,
        &[
            "--path",
            target.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read_to_string(&target).unwrap(), "new body");
}

#[test]
fn write_rule_creates_nested_parent_directories() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let content_file = dir.path().join("c.md");
    fs::write(&content_file, "nested").unwrap();
    let target = dir.path().join("a").join("b").join("c").join("rule.md");

    let output = run_write_rule(
        &repo,
        &[
            "--path",
            target.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(target.exists());
    assert_eq!(fs::read_to_string(&target).unwrap(), "nested");
}
