//! Integration tests for the start-lock subcommand CLI entry points.

mod common;

use std::process::Command;

use common::parse_output;

fn run_start_lock(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("start-lock")
        .args(args)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap()
}

#[test]
fn run_acquire_missing_feature_exits_1() {
    // Exercises `run()` line 248-249: --acquire without --feature
    // prints an error and exits 1.
    let output = run_start_lock(&["--acquire"]);
    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    let msg = data["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("feature required"),
        "error should mention missing feature, got: {}",
        msg
    );
}

#[test]
fn run_release_missing_feature_exits_1() {
    // Exercises `run()` line 264-265: --release without --feature
    // prints an error and exits 1.
    let output = run_start_lock(&["--release"]);
    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    let msg = data["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("feature required"),
        "error should mention missing feature, got: {}",
        msg
    );
}

#[test]
fn run_no_flag_exits_1() {
    // Exercises `run()` line 278-279: no --acquire/--release/--check
    // prints an error and exits 1.
    let output = run_start_lock(&[]);
    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    let msg = data["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("--acquire") || msg.contains("--release") || msg.contains("--check"),
        "error should mention valid flags, got: {}",
        msg
    );
}
