//! Integration tests for `flow-rs detect-framework`.
//!
//! Mirrors tests/test_detect_framework.py (183 lines, 17 test cases).
//! Uses tempfile fixtures + CLAUDE_PLUGIN_ROOT=CARGO_MANIFEST_DIR to
//! point the Rust binary at the real frameworks/ directory. Every
//! subprocess call uses Command::output() per rust-port-parity.md
//! Test-Module Subprocess Stdio rule.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

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

fn detected_names(data: &Value) -> Vec<String> {
    data["detected"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap().to_string())
        .collect()
}

fn available_names(data: &Value) -> Vec<String> {
    data["available"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap().to_string())
        .collect()
}

fn make_project(tmp: &Path) -> PathBuf {
    let project = tmp.join("project");
    fs::create_dir_all(&project).unwrap();
    project
}

#[test]
fn detects_rails_when_gemfile_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    fs::write(project.join("Gemfile"), "source 'https://rubygems.org'\n").unwrap();

    let output = flow_rs()
        .args(["detect-framework", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let data = parse_stdout(&output.stdout);
    assert_eq!(data["status"], "ok");
    assert!(detected_names(&data).contains(&"rails".to_string()));
}

#[test]
fn detects_python_when_pyproject_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    fs::write(project.join("pyproject.toml"), "[project]\n").unwrap();

    let output = flow_rs()
        .args(["detect-framework", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let data = parse_stdout(&output.stdout);
    assert!(detected_names(&data).contains(&"python".to_string()));
}

#[test]
fn detects_python_when_requirements_txt_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    fs::write(project.join("requirements.txt"), "flask\n").unwrap();

    let output = flow_rs()
        .args(["detect-framework", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let data = parse_stdout(&output.stdout);
    assert!(detected_names(&data).contains(&"python".to_string()));
}

#[test]
fn detects_python_when_setup_py_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    fs::write(project.join("setup.py"), "from setuptools import setup\n").unwrap();

    let output = flow_rs()
        .args(["detect-framework", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let data = parse_stdout(&output.stdout);
    assert!(detected_names(&data).contains(&"python".to_string()));
}

#[test]
fn detects_both_when_gemfile_and_pyproject_exist() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    fs::write(project.join("Gemfile"), "").unwrap();
    fs::write(project.join("pyproject.toml"), "").unwrap();

    let output = flow_rs()
        .args(["detect-framework", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let data = parse_stdout(&output.stdout);
    let names = detected_names(&data);
    assert!(names.contains(&"rails".to_string()));
    assert!(names.contains(&"python".to_string()));
}

#[test]
fn detects_ios_when_xcodeproj_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    fs::create_dir(project.join("MyApp.xcodeproj")).unwrap();

    let output = flow_rs()
        .args(["detect-framework", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let data = parse_stdout(&output.stdout);
    assert!(detected_names(&data).contains(&"ios".to_string()));
}

#[test]
fn detects_ios_with_glob_pattern() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    fs::create_dir(project.join("AnotherApp.xcodeproj")).unwrap();

    let output = flow_rs()
        .args(["detect-framework", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let data = parse_stdout(&output.stdout);
    let ios: Vec<&Value> = data["detected"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["name"] == "ios")
        .collect();
    assert_eq!(ios.len(), 1);
    assert_eq!(ios[0]["display_name"], "iOS");
}

#[test]
fn detects_go_when_go_mod_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    fs::write(
        project.join("go.mod"),
        "module example.com/myapp\n\ngo 1.21\n",
    )
    .unwrap();

    let output = flow_rs()
        .args(["detect-framework", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let data = parse_stdout(&output.stdout);
    assert!(detected_names(&data).contains(&"go".to_string()));
}

#[test]
fn detects_go_display_name() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    fs::write(project.join("go.mod"), "module example.com/myapp\n").unwrap();

    let output = flow_rs()
        .args(["detect-framework", project.to_str().unwrap()])
        .output()
        .unwrap();
    let data = parse_stdout(&output.stdout);
    let go: Vec<&Value> = data["detected"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["name"] == "go")
        .collect();
    assert_eq!(go.len(), 1);
    assert_eq!(go[0]["display_name"], "Go");
}

#[test]
fn detects_rust_when_cargo_toml_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"myapp\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();

    let output = flow_rs()
        .args(["detect-framework", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let data = parse_stdout(&output.stdout);
    assert!(detected_names(&data).contains(&"rust".to_string()));
}

#[test]
fn detects_rust_display_name() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"myapp\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();

    let output = flow_rs()
        .args(["detect-framework", project.to_str().unwrap()])
        .output()
        .unwrap();
    let data = parse_stdout(&output.stdout);
    let rust: Vec<&Value> = data["detected"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["name"] == "rust")
        .collect();
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0]["display_name"], "Rust");
}

#[test]
fn detects_nothing_when_no_marker_files() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());

    let output = flow_rs()
        .args(["detect-framework", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let data = parse_stdout(&output.stdout);
    assert_eq!(data["detected"].as_array().unwrap().len(), 0);
}

#[test]
fn result_includes_display_name_for_rails() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    fs::write(project.join("Gemfile"), "").unwrap();

    let output = flow_rs()
        .args(["detect-framework", project.to_str().unwrap()])
        .output()
        .unwrap();
    let data = parse_stdout(&output.stdout);
    let rails: Vec<&Value> = data["detected"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["name"] == "rails")
        .collect();
    assert_eq!(rails[0]["display_name"], "Rails");
}

#[test]
fn lists_available_frameworks() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());

    let output = flow_rs()
        .args(["detect-framework", project.to_str().unwrap()])
        .output()
        .unwrap();
    let data = parse_stdout(&output.stdout);
    let names = available_names(&data);
    assert!(names.contains(&"rails".to_string()));
    assert!(names.contains(&"python".to_string()));
    assert!(names.contains(&"ios".to_string()));
    assert!(names.contains(&"go".to_string()));
    assert!(names.contains(&"rust".to_string()));
}

#[test]
fn cli_happy_path_emits_status_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    fs::write(project.join("Gemfile"), "").unwrap();

    let output = flow_rs()
        .args(["detect-framework", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let data = parse_stdout(&output.stdout);
    assert_eq!(data["status"], "ok");
}

#[test]
fn cli_missing_args_errors() {
    let output = flow_rs().args(["detect-framework"]).output().unwrap();
    assert_ne!(output.status.code(), Some(0));
}

#[test]
fn cli_invalid_project_root_errors() {
    let output = flow_rs()
        .args(["detect-framework", "/nonexistent/path/does/not/exist"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let data = parse_stdout(&output.stdout);
    assert_eq!(data["status"], "error");
}

#[test]
fn hidden_xcodeproj_dir_does_not_detect_ios() {
    // Adversarial regression (PR #882): Python `Path.glob("*.xcodeproj")`
    // skips dot-prefixed entries. The Rust port must match — otherwise a
    // stray `.xcodeproj` directory falsely detects as iOS.
    let tmp = tempfile::tempdir().unwrap();
    let project = make_project(tmp.path());
    fs::create_dir(project.join(".xcodeproj")).unwrap();

    let output = flow_rs()
        .args(["detect-framework", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let data = parse_stdout(&output.stdout);
    assert!(
        !detected_names(&data).contains(&"ios".to_string()),
        "hidden .xcodeproj directory must not trigger iOS detection"
    );
}
