//! Tests for hooks/session-start.sh — the SessionStart hook.
//!
//! Ports tests/test_session_start.py to Rust (PR #953).

mod common;

use std::fs;
use std::process::Command;

fn run_session_start(cwd: &std::path::Path) -> std::process::Output {
    let script = common::hooks_dir().join("session-start.sh");
    Command::new("bash")
        .arg(&script)
        .current_dir(cwd)
        .output()
        .unwrap()
}

/// No .flow-states/ directory and no .flow.json → exits 0, no stdout.
#[test]
fn no_state_directory_exits_0_silent() {
    let dir = tempfile::tempdir().unwrap();
    let output = run_session_start(dir.path());
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "",
        "Should produce no stdout"
    );
}

/// Empty state directory and no .flow.json → exits 0, no stdout.
#[test]
fn empty_state_directory_exits_0_silent() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".flow-states")).unwrap();
    let output = run_session_start(dir.path());
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "",
        "Should produce no stdout"
    );
}

// --- Tombstone tests ---

/// Tombstone: state mutation functions removed in PR #938. Must not return.
#[test]
fn session_context_no_state_mutation() {
    let source = common::repo_root()
        .join("src")
        .join("commands")
        .join("session_context.rs");
    let content = fs::read_to_string(&source).unwrap();
    assert!(
        !content.contains("reset_interrupted"),
        "reset_interrupted was removed in PR #938"
    );
    assert!(
        !content.contains("consume_last_failure"),
        "consume_last_failure was removed in PR #938"
    );
    assert!(
        !content.contains("consume_compact_data"),
        "consume_compact_data was removed in PR #938"
    );
}

/// Tombstone: context injection removed in PR #938. Must not return.
#[test]
fn session_context_no_context_injection() {
    let source = common::repo_root()
        .join("src")
        .join("commands")
        .join("session_context.rs");
    let content = fs::read_to_string(&source).unwrap();
    assert!(
        !content.contains("NOTE_INSTRUCTION"),
        "NOTE_INSTRUCTION was removed in PR #938"
    );
    assert!(
        !content.contains("build_single_feature_context"),
        "build_single_feature_context was removed in PR #938"
    );
    assert!(
        !content.contains("build_multi_feature_context"),
        "build_multi_feature_context was removed in PR #938"
    );
}
