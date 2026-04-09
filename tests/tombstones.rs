//! Consolidated tombstone tests.
//!
//! Tombstone tests assert that intentionally removed features, files,
//! and code patterns do not return. If a merge conflict resolution
//! re-introduces deleted content, the corresponding test fails.
//!
//! Standalone tombstones (file-existence, source-content) live here.
//! Topical tombstones that are integral to a test domain (e.g.
//! skill_contracts, structural) stay in their respective test files.

mod common;

use std::fs;

// ============================================================
// Python QA file deletions (PR #931)
// ============================================================

#[test]
fn test_no_python_qa_mode() {
    // Tombstone: removed in PR #931. Must not return.
    assert!(
        !common::repo_root().join("lib/qa-mode.py").exists(),
        "lib/qa-mode.py was ported to Rust (src/qa_mode.rs) and must not be re-added"
    );
}

#[test]
fn test_no_python_qa_reset() {
    // Tombstone: removed in PR #931. Must not return.
    assert!(
        !common::repo_root().join("lib/qa-reset.py").exists(),
        "lib/qa-reset.py was ported to Rust (src/qa_reset.rs) and must not be re-added"
    );
}

#[test]
fn test_no_python_qa_verify() {
    // Tombstone: removed in PR #931. Must not return.
    assert!(
        !common::repo_root().join("lib/qa-verify.py").exists(),
        "lib/qa-verify.py was ported to Rust (src/qa_verify.rs) and must not be re-added"
    );
}

#[test]
fn test_no_python_scaffold_qa() {
    // Tombstone: removed in PR #931. Must not return.
    assert!(
        !common::repo_root().join("lib/scaffold-qa.py").exists(),
        "lib/scaffold-qa.py was ported to Rust (src/scaffold_qa.rs) and must not be re-added"
    );
}

#[test]
fn test_no_python_test_qa_mode() {
    // Tombstone: removed in PR #931. Must not return.
    assert!(
        !common::repo_root().join("tests/test_qa_mode.py").exists(),
        "tests/test_qa_mode.py was replaced by Rust inline tests and must not be re-added"
    );
}

#[test]
fn test_no_python_test_qa_reset() {
    // Tombstone: removed in PR #931. Must not return.
    assert!(
        !common::repo_root().join("tests/test_qa_reset.py").exists(),
        "tests/test_qa_reset.py was replaced by Rust inline tests and must not be re-added"
    );
}

#[test]
fn test_no_python_test_qa_verify() {
    // Tombstone: removed in PR #931. Must not return.
    assert!(
        !common::repo_root().join("tests/test_qa_verify.py").exists(),
        "tests/test_qa_verify.py was replaced by Rust inline tests and must not be re-added"
    );
}

#[test]
fn test_no_python_test_scaffold_qa() {
    // Tombstone: removed in PR #931. Must not return.
    assert!(
        !common::repo_root()
            .join("tests/test_scaffold_qa.py")
            .exists(),
        "tests/test_scaffold_qa.py was replaced by Rust inline tests and must not be re-added"
    );
}

// ============================================================
// Python cleanup file deletions (PR #839)
// ============================================================

#[test]
fn tombstone_no_python_cleanup() {
    // Tombstone: removed in PR #839. Must not return.
    assert!(
        !common::repo_root().join("lib/cleanup.py").exists(),
        "lib/cleanup.py was ported to Rust and must not be re-added"
    );
}

#[test]
fn tombstone_no_python_test_cleanup() {
    // Tombstone: removed in PR #839. Must not return.
    assert!(
        !common::repo_root().join("tests/test_cleanup.py").exists(),
        "tests/test_cleanup.py was ported to Rust and must not be re-added"
    );
}

// ============================================================
// resolve_branch .flow-states/ scan removal (PR #924)
// ============================================================

/// Tombstone: .flow-states/ scan removed in PR #924. Must not return.
#[test]
fn resolve_branch_no_scan_tombstone() {
    let source =
        fs::read_to_string(common::repo_root().join("src/git.rs")).expect("src/git.rs must exist");
    // Find the resolve_branch_impl function body
    let start = source.find("fn resolve_branch_impl(").unwrap();
    let end = source[start..].find("\n}\n").unwrap() + start;
    let func_body = &source[start..end];
    assert!(
        !func_body.contains("read_dir"),
        "resolve_branch_impl must not scan .flow-states/ via read_dir — removed in PR #924"
    );
    assert!(
        !func_body.contains("candidates"),
        "resolve_branch_impl must not collect candidates — removed in PR #924"
    );
}

// ============================================================
// check_discussion_mode removal from run() (PR #954)
// ============================================================

#[test]
fn test_run_does_not_call_check_discussion_mode() {
    // Tombstone: check_discussion_mode removed from run() in PR #954.
    // check_first_stop now handles both discussion mode and pending
    // continuations. Must not return to run().
    let source = fs::read_to_string(common::repo_root().join("src/hooks/stop_continue.rs"))
        .expect("src/hooks/stop_continue.rs must exist");
    // Find the run() function body — it starts after "pub fn run()"
    let run_start = source.find("pub fn run()").expect("run() must exist");
    let run_body = &source[run_start..];
    // The run() body ends at the next function or #[cfg(test)]
    let run_end = run_body.find("#[cfg(test)]").unwrap_or(run_body.len());
    let run_text = &run_body[..run_end];
    assert!(
        !run_text.contains("check_discussion_mode"),
        "run() must not call check_discussion_mode — superseded by check_first_stop in PR #954"
    );
}

// ============================================================
// QA verify removed check types (PR #729)
// ============================================================

#[test]
fn test_qa_verify_no_decomposed_issue_check() {
    // Tombstone: removed in PR #729. Must not return.
    // verify_impl must not register a check named "Decomposed issue created".
    let source = fs::read_to_string(common::repo_root().join("src/qa_verify.rs"))
        .expect("src/qa_verify.rs must exist");
    assert!(
        !source.contains("\"Decomposed issue created\""),
        "qa_verify must not contain a 'Decomposed issue created' check — removed in PR #729"
    );
}

#[test]
fn test_qa_verify_no_body_files_check() {
    // Tombstone: removed in PR #729. Must not return.
    // verify_impl must not register a check named "No leftover body files".
    let source = fs::read_to_string(common::repo_root().join("src/qa_verify.rs"))
        .expect("src/qa_verify.rs must exist");
    assert!(
        !source.contains("\"No leftover body files\""),
        "qa_verify must not contain a 'No leftover body files' check — removed in PR #729"
    );
}

// ============================================================
// phase_enter code_review_step initialization removal (PR #925)
// ============================================================

/// Tombstone: code_review_step initialization moved to phase-enter command in PR #925.
#[test]
fn enter_code_review_does_not_set_code_review_step() {
    // phase_enter() must not set code_review_step — phase-enter command handles it.
    let source = fs::read_to_string(common::repo_root().join("src/phase_transition.rs"))
        .expect("src/phase_transition.rs must exist");
    let start = source
        .find("pub fn phase_enter(")
        .expect("phase_enter function must exist");
    // Find the end of the function (next pub fn or end of file)
    let after_start = &source[start..];
    let end = after_start[1..]
        .find("\npub fn ")
        .map(|i| i + 1)
        .unwrap_or(after_start.len());
    let func_body = &after_start[..end];
    assert!(
        !func_body.contains("\"code_review_step\""),
        "phase_enter() must not set code_review_step — moved to phase-enter command in PR #925"
    );
}

// ============================================================
// issueDependenciesSummary removal (PR #849)
// ============================================================

#[test]
fn build_blocker_query_no_issue_dependencies_summary() {
    // Tombstone: replaced with blockedBy connection in PR #849. Must not return.
    let source = fs::read_to_string(common::repo_root().join("src/analyze_issues.rs"))
        .expect("src/analyze_issues.rs must exist");
    assert!(
        !source.contains("issueDependenciesSummary"),
        "issueDependenciesSummary was replaced with blockedBy connection"
    );
}
