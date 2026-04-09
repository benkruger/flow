//! Tests for the tombstone-audit subcommand.
//!
//! These tests exercise the pure functions (PR extraction, GraphQL
//! query/response parsing, date comparison) without network calls.

mod common;

use flow_rs::tombstone_audit::{
    build_merge_query, classify_tombstones, extract_pr_numbers, parse_merge_response,
    scan_test_files, MergeInfo, TombstoneEntry,
};
use std::collections::{HashMap, HashSet};
use std::fs;

// ============================================================
// extract_pr_numbers — regex extraction from tombstone comments
// ============================================================

#[test]
fn extract_pr_numbers_double_slash_comment() {
    let content = r#"
    // Tombstone: removed in PR #839. Must not return.
    "#;
    let prs = extract_pr_numbers(content);
    assert!(prs.contains(&839));
}

#[test]
fn extract_pr_numbers_triple_slash_doc_comment() {
    let content = r#"
    /// Tombstone: .flow-states/ scan removed in PR #924. Must not return.
    "#;
    let prs = extract_pr_numbers(content);
    assert!(prs.contains(&924));
}

#[test]
fn extract_pr_numbers_assertion_message_string() {
    let content = r#"
        "Tombstone: removed in PR #587. Must not return."
    "#;
    let prs = extract_pr_numbers(content);
    assert!(prs.contains(&587));
}

#[test]
fn extract_pr_numbers_multiple_in_one_file() {
    let content = r#"
    // Tombstone: removed in PR #839. Must not return.
    // Tombstone: removed in PR #924. Must not return.
    // Tombstone: replaced with blockedBy connection in PR #849. Must not return.
    "#;
    let prs = extract_pr_numbers(content);
    assert_eq!(prs.len(), 3);
    assert!(prs.contains(&839));
    assert!(prs.contains(&924));
    assert!(prs.contains(&849));
}

#[test]
fn extract_pr_numbers_deduplicates() {
    let content = r#"
    // Tombstone: removed in PR #931. Must not return.
    // Tombstone: removed in PR #931. Must not return.
    // Tombstone: removed in PR #931. Must not return.
    "#;
    let prs = extract_pr_numbers(content);
    assert_eq!(prs.len(), 1);
    assert!(prs.contains(&931));
}

#[test]
fn extract_pr_numbers_no_tombstones() {
    let content = r#"
    fn test_something() {
        assert!(true);
    }
    "#;
    let prs = extract_pr_numbers(content);
    assert!(prs.is_empty());
}

#[test]
fn extract_pr_numbers_tombstone_without_pr_reference() {
    // A tombstone comment that doesn't follow the PR # pattern
    let content = r#"
    // Tombstone: this feature was removed.
    "#;
    let prs = extract_pr_numbers(content);
    assert!(prs.is_empty());
}

// ============================================================
// scan_test_files — multi-file scanning
// ============================================================

#[test]
fn scan_test_files_finds_tombstones_across_files() {
    let dir = tempfile::tempdir().unwrap();
    let tests_dir = dir.path().join("tests");
    fs::create_dir(&tests_dir).unwrap();

    fs::write(
        tests_dir.join("file_a.rs"),
        "// Tombstone: removed in PR #100. Must not return.\n",
    )
    .unwrap();
    fs::write(
        tests_dir.join("file_b.rs"),
        "// Tombstone: removed in PR #200. Must not return.\n",
    )
    .unwrap();

    let entries = scan_test_files(dir.path());
    let prs: HashSet<u64> = entries.iter().map(|e| e.pr).collect();
    assert!(prs.contains(&100));
    assert!(prs.contains(&200));
}

#[test]
fn scan_test_files_skips_non_rs_files() {
    let dir = tempfile::tempdir().unwrap();
    let tests_dir = dir.path().join("tests");
    fs::create_dir(&tests_dir).unwrap();

    fs::write(
        tests_dir.join("notes.txt"),
        "// Tombstone: removed in PR #999. Must not return.\n",
    )
    .unwrap();
    fs::write(tests_dir.join("real.rs"), "fn test() {}\n").unwrap();

    let entries = scan_test_files(dir.path());
    let prs: HashSet<u64> = entries.iter().map(|e| e.pr).collect();
    assert!(!prs.contains(&999));
}

#[test]
fn scan_test_files_records_file_path() {
    let dir = tempfile::tempdir().unwrap();
    let tests_dir = dir.path().join("tests");
    fs::create_dir(&tests_dir).unwrap();

    fs::write(
        tests_dir.join("tombstones.rs"),
        "// Tombstone: removed in PR #500. Must not return.\n",
    )
    .unwrap();

    let entries = scan_test_files(dir.path());
    assert_eq!(entries.len(), 1);
    assert!(entries[0].file.ends_with("tombstones.rs"));
    assert_eq!(entries[0].pr, 500);
}

// ============================================================
// build_merge_query — GraphQL query construction
// ============================================================

#[test]
fn build_merge_query_single_pr() {
    let query = build_merge_query(&[839]);
    assert!(query.contains("$owner: String!"));
    assert!(query.contains("$repo: String!"));
    assert!(query.contains("pr_839: pullRequest(number: 839)"));
    assert!(query.contains("mergedAt"));
}

#[test]
fn build_merge_query_multiple_prs() {
    let query = build_merge_query(&[839, 924, 849]);
    assert!(query.contains("pr_839: pullRequest(number: 839)"));
    assert!(query.contains("pr_924: pullRequest(number: 924)"));
    assert!(query.contains("pr_849: pullRequest(number: 849)"));
}

#[test]
fn build_merge_query_empty() {
    let query = build_merge_query(&[]);
    assert!(query.contains("repository"));
}

// ============================================================
// parse_merge_response — GraphQL response parsing
// ============================================================

#[test]
fn parse_merge_response_merged_pr() {
    let json = r#"{
        "data": {
            "repository": {
                "pr_839": { "mergedAt": "2024-01-15T10:00:00Z" }
            }
        }
    }"#;
    let result = parse_merge_response(json, &[839]);
    assert_eq!(
        result.get(&839).unwrap().merged_at.as_deref(),
        Some("2024-01-15T10:00:00Z")
    );
}

#[test]
fn parse_merge_response_unmerged_pr() {
    let json = r#"{
        "data": {
            "repository": {
                "pr_100": { "mergedAt": null }
            }
        }
    }"#;
    let result = parse_merge_response(json, &[100]);
    assert!(result.get(&100).unwrap().merged_at.is_none());
}

#[test]
fn parse_merge_response_missing_pr() {
    let json = r#"{
        "data": {
            "repository": {}
        }
    }"#;
    let result = parse_merge_response(json, &[999]);
    // Missing PR should still have an entry with None
    assert!(result.get(&999).unwrap().merged_at.is_none());
}

#[test]
fn parse_merge_response_malformed_json() {
    let result = parse_merge_response("not json", &[839]);
    assert!(result.is_empty());
}

#[test]
fn parse_merge_response_multiple_prs() {
    let json = r#"{
        "data": {
            "repository": {
                "pr_839": { "mergedAt": "2024-01-15T10:00:00Z" },
                "pr_924": { "mergedAt": "2024-06-01T12:00:00Z" },
                "pr_100": { "mergedAt": null }
            }
        }
    }"#;
    let result = parse_merge_response(json, &[839, 924, 100]);
    assert_eq!(result.len(), 3);
    assert!(result.get(&839).unwrap().merged_at.is_some());
    assert!(result.get(&924).unwrap().merged_at.is_some());
    assert!(result.get(&100).unwrap().merged_at.is_none());
}

// ============================================================
// classify_tombstones — stale vs current determination
// ============================================================

#[test]
fn classify_stale_when_merged_before_threshold() {
    let mut merge_dates = HashMap::new();
    merge_dates.insert(
        839,
        MergeInfo {
            merged_at: Some("2024-01-15T10:00:00Z".to_string()),
        },
    );
    let entries = vec![TombstoneEntry {
        pr: 839,
        file: "tests/tombstones.rs".to_string(),
    }];
    let (stale, current) =
        classify_tombstones(&entries, &merge_dates, Some("2024-06-01T00:00:00Z"));
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].pr, 839);
    assert!(current.is_empty());
}

#[test]
fn classify_current_when_merged_after_threshold() {
    let mut merge_dates = HashMap::new();
    merge_dates.insert(
        924,
        MergeInfo {
            merged_at: Some("2024-08-01T10:00:00Z".to_string()),
        },
    );
    let entries = vec![TombstoneEntry {
        pr: 924,
        file: "tests/tombstones.rs".to_string(),
    }];
    let (stale, current) =
        classify_tombstones(&entries, &merge_dates, Some("2024-06-01T00:00:00Z"));
    assert!(stale.is_empty());
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].pr, 924);
}

#[test]
fn classify_skip_unmerged_pr() {
    let mut merge_dates = HashMap::new();
    merge_dates.insert(100, MergeInfo { merged_at: None });
    let entries = vec![TombstoneEntry {
        pr: 100,
        file: "tests/tombstones.rs".to_string(),
    }];
    let (stale, current) =
        classify_tombstones(&entries, &merge_dates, Some("2024-06-01T00:00:00Z"));
    // Unmerged PRs are skipped — not stale, not current
    assert!(stale.is_empty());
    assert!(current.is_empty());
}

#[test]
fn classify_no_open_prs_all_stale() {
    let mut merge_dates = HashMap::new();
    merge_dates.insert(
        839,
        MergeInfo {
            merged_at: Some("2024-01-15T10:00:00Z".to_string()),
        },
    );
    merge_dates.insert(
        924,
        MergeInfo {
            merged_at: Some("2024-08-01T10:00:00Z".to_string()),
        },
    );
    let entries = vec![
        TombstoneEntry {
            pr: 839,
            file: "tests/tombstones.rs".to_string(),
        },
        TombstoneEntry {
            pr: 924,
            file: "tests/tombstones.rs".to_string(),
        },
    ];
    // threshold=None means no open PRs — all merged tombstones are stale
    let (stale, current) = classify_tombstones(&entries, &merge_dates, None);
    assert_eq!(stale.len(), 2);
    assert!(current.is_empty());
}

#[test]
fn classify_missing_pr_in_merge_data_skipped() {
    let merge_dates = HashMap::new(); // empty — no data for any PR
    let entries = vec![TombstoneEntry {
        pr: 999,
        file: "tests/tombstones.rs".to_string(),
    }];
    let (stale, current) =
        classify_tombstones(&entries, &merge_dates, Some("2024-06-01T00:00:00Z"));
    // No merge data → skip
    assert!(stale.is_empty());
    assert!(current.is_empty());
}
