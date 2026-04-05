//! Integration tests for `flow-rs prime-project`.
//!
//! Mirrors tests/test_prime_project.py (124 lines, 11 test cases).
//! Uses env!("CARGO_MANIFEST_DIR")/frameworks for real priming.md
//! fixtures (rails and python). Every subprocess call uses
//! Command::output() per rust-port-parity.md Test-Module Subprocess
//! Stdio rule.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const MARKER_BEGIN: &str = "<!-- FLOW:BEGIN -->";
const MARKER_END: &str = "<!-- FLOW:END -->";

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

fn run_prime(project_root: &Path, framework: &str) -> (Value, i32) {
    let output = flow_rs()
        .arg("prime-project")
        .arg(project_root)
        .arg("--framework")
        .arg(framework)
        .output()
        .unwrap();
    let value = parse_stdout(&output.stdout);
    let code = output.status.code().unwrap_or(-1);
    (value, code)
}

fn make_project(tmp: &Path) -> PathBuf {
    tmp.to_path_buf()
}

#[test]
fn inserts_priming_content_into_existing_claude_md() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    let claude_md = project.join("CLAUDE.md");
    fs::write(&claude_md, "# My Project\n\nExisting content.\n").unwrap();

    let (data, code) = run_prime(&project, "rails");
    assert_eq!(data["status"], "ok");
    assert_eq!(code, 0);

    let content = fs::read_to_string(&claude_md).unwrap();
    assert!(content.contains(MARKER_BEGIN));
    assert!(content.contains(MARKER_END));
    assert!(content.contains("Existing content."));
    assert!(content.contains("Architecture Patterns"));
}

#[test]
fn idempotent_replacement() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    let claude_md = project.join("CLAUDE.md");
    fs::write(&claude_md, "# My Project\n\nExisting content.\n").unwrap();

    run_prime(&project, "rails");
    let first = fs::read_to_string(&claude_md).unwrap();
    run_prime(&project, "rails");
    let second = fs::read_to_string(&claude_md).unwrap();
    assert_eq!(first, second);
}

#[test]
fn replaces_content_when_switching_framework() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    let claude_md = project.join("CLAUDE.md");
    fs::write(&claude_md, "# My Project\n").unwrap();

    run_prime(&project, "rails");
    assert!(fs::read_to_string(&claude_md).unwrap().contains("Rails Conventions"));

    run_prime(&project, "python");
    let content = fs::read_to_string(&claude_md).unwrap();
    assert!(content.contains("Python Conventions"));
    assert!(!content.contains("Rails Conventions"));
}

#[test]
fn error_when_no_claude_md() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    let (data, code) = run_prime(&project, "rails");
    assert_eq!(data["status"], "error");
    assert!(data["message"].as_str().unwrap().contains("CLAUDE.md"));
    assert_eq!(code, 1);
}

#[test]
fn error_when_invalid_framework() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    fs::write(project.join("CLAUDE.md"), "# My Project\n").unwrap();
    let (data, code) = run_prime(&project, "nonexistent");
    assert_eq!(data["status"], "error");
    assert_eq!(code, 1);
}

#[test]
fn preserves_content_before_and_after_markers() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    let claude_md = project.join("CLAUDE.md");
    fs::write(&claude_md, "# My Project\n\nBefore.\n\nAfter marker stuff.\n").unwrap();

    run_prime(&project, "rails");
    let content = fs::read_to_string(&claude_md).unwrap();
    assert!(content.starts_with("# My Project\n\nBefore.\n\nAfter marker stuff.\n"));
    assert!(content.contains(MARKER_BEGIN));
    assert!(content.contains(MARKER_END));
}

#[test]
fn blank_line_after_begin_marker_for_md022_compliance() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    let claude_md = project.join("CLAUDE.md");
    fs::write(&claude_md, "# My Project\n").unwrap();
    run_prime(&project, "rails");
    let content = fs::read_to_string(&claude_md).unwrap();
    assert!(
        content.contains(&format!("{}\n\n", MARKER_BEGIN)),
        "Expected blank line after <!-- FLOW:BEGIN --> for MD022 compliance"
    );
}

#[test]
fn cli_happy_path_exit_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    fs::write(project.join("CLAUDE.md"), "# My Project\n").unwrap();
    let (data, code) = run_prime(&project, "rails");
    assert_eq!(data["status"], "ok");
    assert_eq!(code, 0);
}

#[test]
fn cli_missing_project_root_arg() {
    let output = flow_rs().args(["prime-project"]).output().unwrap();
    assert_ne!(output.status.code(), Some(0));
}

#[test]
fn cli_missing_framework_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    let output = flow_rs()
        .arg("prime-project")
        .arg(&project)
        .output()
        .unwrap();
    assert_ne!(output.status.code(), Some(0));
}

#[test]
fn cli_prime_error_exits_with_1() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    // No CLAUDE.md — prime() returns error
    let (data, code) = run_prime(&project, "rails");
    assert_eq!(data["status"], "error");
    assert_eq!(code, 1);
}
