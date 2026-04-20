//! Integration tests for `bin/flow write-rule`.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_git_repo_with_remote, parse_output};
use flow_rs::write_rule::{read_content_file, write_rule};

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

#[test]
fn write_rule_target_parent_blocked_by_file_errors() {
    // Drives the write_rule Err arm of run(): create_dir_all fails when
    // a regular file occupies the parent path that needs to be a dir.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let content_file = dir.path().join("c.md");
    fs::write(&content_file, "body").unwrap();
    let blocker = dir.path().join("blocker");
    fs::write(&blocker, "I am a file, not a directory").unwrap();
    let target = blocker.join("nested").join("rule.md");

    let output = run_write_rule(
        &repo,
        &[
            "--path",
            target.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Could not create directories"));
}

// --- Library-level tests (migrated from src/write_rule.rs) ---

// --- read_content_file ---

#[test]
fn read_content_file_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    let content_file = dir.path().join("content.md");
    fs::write(&content_file, "# My Rule\n\nDo the thing.\n").unwrap();

    let content = read_content_file(content_file.to_str().unwrap()).unwrap();
    assert_eq!(content, "# My Rule\n\nDo the thing.\n");
    assert!(!content_file.exists());
}

#[test]
fn read_content_file_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nonexistent.md");

    let result = read_content_file(missing.to_str().unwrap());
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Could not read content file"));
}

// --- write_rule ---

#[test]
fn write_rule_happy_path_lib() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("rules").join("topic.md");
    fs::create_dir_all(target.parent().unwrap()).unwrap();

    let result = write_rule(target.to_str().unwrap(), "# Topic\n\nRule text.\n");
    assert!(result.is_ok());
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "# Topic\n\nRule text.\n"
    );
}

#[test]
fn write_rule_creates_parent_dirs_lib() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir
        .path()
        .join("deep")
        .join("nested")
        .join("dir")
        .join("rule.md");

    let result = write_rule(target.to_str().unwrap(), "content");
    assert!(result.is_ok());
    assert_eq!(fs::read_to_string(&target).unwrap(), "content");
}

#[test]
fn write_rule_overwrites_existing_lib() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("rule.md");
    fs::write(&target, "old content").unwrap();

    let result = write_rule(target.to_str().unwrap(), "new content");
    assert!(result.is_ok());
    assert_eq!(fs::read_to_string(&target).unwrap(), "new content");
}

#[test]
fn write_rule_write_error_lib() {
    let dir = tempfile::tempdir().unwrap();
    let readonly = dir.path().join("readonly");
    fs::create_dir_all(&readonly).unwrap();

    // Make the directory read-only
    let mut perms = fs::metadata(&readonly).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&readonly, perms).unwrap();

    let target = readonly.join("rule.md");
    let result = write_rule(target.to_str().unwrap(), "content");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Could not write"));

    // Restore permissions for cleanup
    let mut perms = fs::metadata(&readonly).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(&readonly, perms).unwrap();
}

#[test]
fn write_rule_create_dir_error_lib() {
    let dir = tempfile::tempdir().unwrap();
    // Place a regular file where the parent directory needs to be.
    let blocker = dir.path().join("blocker");
    fs::write(&blocker, "I am a file").unwrap();

    let target = blocker.join("nested").join("rule.md");
    let result = write_rule(target.to_str().unwrap(), "content");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Could not create directories"));
}

#[test]
fn write_rule_empty_path_errors_lib() {
    // Empty string path: parent() returns None so create_dir_all is
    // skipped, and fs::write on an empty path returns an OS error.
    let result = write_rule("", "content");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Could not write"));
}

// --- end-to-end ---

#[test]
fn end_to_end_write_lib() {
    let dir = tempfile::tempdir().unwrap();
    let content_file = dir.path().join("content.md");
    fs::write(&content_file, "# Rule\n\nDo it.\n").unwrap();
    let target = dir.path().join(".claude").join("rules").join("topic.md");

    let content = read_content_file(content_file.to_str().unwrap()).unwrap();
    let result = write_rule(target.to_str().unwrap(), &content);

    assert!(result.is_ok());
    assert_eq!(fs::read_to_string(&target).unwrap(), "# Rule\n\nDo it.\n");
    assert!(!content_file.exists());
}
