//! Tombstone tests for QA tooling Python-to-Rust port.
//!
//! These tests assert that the deleted Python files do not return.
//! If a merge conflict resolution re-introduces any of these files,
//! the corresponding test fails immediately.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// --- Python source files ---

#[test]
fn test_no_python_qa_mode() {
    // Tombstone: removed in PR #931. Must not return.
    assert!(
        !repo_root().join("lib/qa-mode.py").exists(),
        "lib/qa-mode.py was ported to Rust (src/qa_mode.rs) and must not be re-added"
    );
}

#[test]
fn test_no_python_qa_reset() {
    // Tombstone: removed in PR #931. Must not return.
    assert!(
        !repo_root().join("lib/qa-reset.py").exists(),
        "lib/qa-reset.py was ported to Rust (src/qa_reset.rs) and must not be re-added"
    );
}

#[test]
fn test_no_python_qa_verify() {
    // Tombstone: removed in PR #931. Must not return.
    assert!(
        !repo_root().join("lib/qa-verify.py").exists(),
        "lib/qa-verify.py was ported to Rust (src/qa_verify.rs) and must not be re-added"
    );
}

#[test]
fn test_no_python_scaffold_qa() {
    // Tombstone: removed in PR #931. Must not return.
    assert!(
        !repo_root().join("lib/scaffold-qa.py").exists(),
        "lib/scaffold-qa.py was ported to Rust (src/scaffold_qa.rs) and must not be re-added"
    );
}

// --- Python test files ---

#[test]
fn test_no_python_test_qa_mode() {
    // Tombstone: removed in PR #931. Must not return.
    assert!(
        !repo_root().join("tests/test_qa_mode.py").exists(),
        "tests/test_qa_mode.py was replaced by Rust inline tests and must not be re-added"
    );
}

#[test]
fn test_no_python_test_qa_reset() {
    // Tombstone: removed in PR #931. Must not return.
    assert!(
        !repo_root().join("tests/test_qa_reset.py").exists(),
        "tests/test_qa_reset.py was replaced by Rust inline tests and must not be re-added"
    );
}

#[test]
fn test_no_python_test_qa_verify() {
    // Tombstone: removed in PR #931. Must not return.
    assert!(
        !repo_root().join("tests/test_qa_verify.py").exists(),
        "tests/test_qa_verify.py was replaced by Rust inline tests and must not be re-added"
    );
}

#[test]
fn test_no_python_test_scaffold_qa() {
    // Tombstone: removed in PR #931. Must not return.
    assert!(
        !repo_root().join("tests/test_scaffold_qa.py").exists(),
        "tests/test_scaffold_qa.py was replaced by Rust inline tests and must not be re-added"
    );
}
