//! Smoke-test artifact contract for `hello.sh`.
//!
//! `hello.sh` is the FLOW plugin's designated smoke-test artifact
//! for end-to-end lifecycle regression passes. Each QA pass updates
//! line 2 to record the current QA date; this test pins that
//! greeting so an unintended edit fails CI.

mod common;

use std::fs;

#[test]
fn hello_sh_contains_current_qa_greeting() {
    let path = common::repo_root().join("hello.sh");
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
    let expected = r#"echo "Hello, FLOW! (QA 2026-05-16)""#;
    assert!(
        content.contains(expected),
        "hello.sh must contain the current QA-dated greeting `{}`; got:\n{}",
        expected,
        content
    );
}
