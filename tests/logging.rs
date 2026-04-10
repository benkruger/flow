//! Tests that verify session log entries are produced by each module.
//!
//! Each module that performs significant operations must call `append_log`
//! to record entries in `.flow-states/<branch>.log`. These source-content
//! tests assert the logging contract: the module must contain `append_log(`
//! calls. Tasks 3-7 make these tests pass by adding the actual log calls.

mod common;

use std::fs;

/// main.rs must call append_log for phase-transition logging.
#[test]
fn main_rs_uses_append_log() {
    let src = fs::read_to_string(common::repo_root().join("src/main.rs")).unwrap();
    assert!(
        src.contains("append_log("),
        "src/main.rs must call append_log for phase-transition session logging"
    );
}

/// complete_post_merge.rs must call append_log for post-merge step logging.
#[test]
fn complete_post_merge_uses_append_log() {
    let src = fs::read_to_string(common::repo_root().join("src/complete_post_merge.rs")).unwrap();
    assert!(
        src.contains("append_log("),
        "src/complete_post_merge.rs must call append_log for post-merge session logging"
    );
}

/// cleanup.rs must call append_log for cleanup step logging.
#[test]
fn cleanup_uses_append_log() {
    let src = fs::read_to_string(common::repo_root().join("src/cleanup.rs")).unwrap();
    assert!(
        src.contains("append_log("),
        "src/cleanup.rs must call append_log for cleanup session logging"
    );
}

/// complete_finalize.rs must call append_log for orchestration logging.
#[test]
fn complete_finalize_uses_append_log() {
    let src = fs::read_to_string(common::repo_root().join("src/complete_finalize.rs")).unwrap();
    assert!(
        src.contains("append_log("),
        "src/complete_finalize.rs must call append_log for orchestration session logging"
    );
}

/// finalize_commit.rs must call append_log for commit-cycle logging.
#[test]
fn finalize_commit_uses_append_log() {
    let src = fs::read_to_string(common::repo_root().join("src/finalize_commit.rs")).unwrap();
    assert!(
        src.contains("append_log("),
        "src/finalize_commit.rs must call append_log for commit-cycle session logging"
    );
}
