//! Tests for the tombstone-audit subcommand.
//!
//! These tests exercise the pure functions (PR extraction, GraphQL
//! query/response parsing, date comparison) without network calls.
//!
//! IMPORTANT: Test fixture strings must NOT contain literal tombstone
//! patterns that match the scan regex. Use `tombstone_line()` to build
//! fixture content at runtime, keeping the source clean of false matches.

mod common;

use flow_rs::tombstone_audit::{
    build_merge_query, classify_tombstones, extract_pr_numbers, parse_merge_response,
    scan_test_files, MergeInfo, TombstoneEntry,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

/// Build a tombstone comment line at runtime to avoid the literal pattern
/// appearing in this test file's source (which would contaminate scan results).
fn tombstone_line(pr: u64, suffix: &str) -> String {
    format!("// Tombstone: removed in PR #{}.{}", pr, suffix)
}

/// Build a doc-comment tombstone line.
fn tombstone_doc_line(pr: u64, prefix: &str) -> String {
    format!("/// Tombstone: {} in PR #{}. Must not return.", prefix, pr)
}

/// Build a string-literal tombstone (as found in assertion messages).
fn tombstone_str_line(pr: u64) -> String {
    format!("\"Tombstone: removed in PR #{}. Must not return.\"", pr)
}

// ============================================================
// extract_pr_numbers — regex extraction from tombstone comments
// ============================================================

#[test]
fn extract_pr_numbers_double_slash_comment() {
    let content = tombstone_line(839, " Must not return.");
    let prs = extract_pr_numbers(&content);
    assert!(prs.contains(&839));
}

#[test]
fn extract_pr_numbers_triple_slash_doc_comment() {
    let content = tombstone_doc_line(924, ".flow-states/ scan removed");
    let prs = extract_pr_numbers(&content);
    assert!(prs.contains(&924));
}

#[test]
fn extract_pr_numbers_assertion_message_string() {
    let content = tombstone_str_line(587);
    let prs = extract_pr_numbers(&content);
    assert!(prs.contains(&587));
}

#[test]
fn extract_pr_numbers_multiple_in_one_file() {
    let content = format!(
        "{}\n{}\n{}",
        tombstone_line(839, " Must not return."),
        tombstone_line(924, " Must not return."),
        tombstone_line(849, " Must not return."),
    );
    let prs = extract_pr_numbers(&content);
    assert_eq!(prs.len(), 3);
    assert!(prs.contains(&839));
    assert!(prs.contains(&924));
    assert!(prs.contains(&849));
}

#[test]
fn extract_pr_numbers_deduplicates() {
    let content = format!(
        "{}\n{}\n{}",
        tombstone_line(931, " Must not return."),
        tombstone_line(931, " Must not return."),
        tombstone_line(931, " Must not return."),
    );
    let prs = extract_pr_numbers(&content);
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
    // A comment that says "Tombstone:" but has no PR # pattern
    let content = "// Tombstone: this feature was removed.";
    let prs = extract_pr_numbers(content);
    assert!(prs.is_empty());
}

#[test]
fn extract_pr_numbers_filters_zero() {
    let content = tombstone_line(0, " Must not return.");
    let prs = extract_pr_numbers(&content);
    assert!(!prs.contains(&0), "PR #0 is not a valid GitHub PR number");
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
        tombstone_line(100, " Must not return.\n"),
    )
    .unwrap();
    fs::write(
        tests_dir.join("file_b.rs"),
        tombstone_line(200, " Must not return.\n"),
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
        tombstone_line(999, " Must not return.\n"),
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
        tombstone_line(500, " Must not return.\n"),
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
fn classify_at_threshold_is_current() {
    let mut merge_dates = HashMap::new();
    merge_dates.insert(
        500,
        MergeInfo {
            merged_at: Some("2024-06-01T00:00:00Z".to_string()),
        },
    );
    let entries = vec![TombstoneEntry {
        pr: 500,
        file: "tests/tombstones.rs".to_string(),
    }];
    // merged_at == threshold → current (not stale)
    let (stale, current) =
        classify_tombstones(&entries, &merge_dates, Some("2024-06-01T00:00:00Z"));
    assert!(stale.is_empty());
    assert_eq!(current.len(), 1);
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
    let (stale, current) = classify_tombstones(&entries, &merge_dates, None);
    assert_eq!(stale.len(), 2);
    assert!(current.is_empty());
}

#[test]
fn classify_missing_pr_in_merge_data_skipped() {
    let merge_dates = HashMap::new();
    let entries = vec![TombstoneEntry {
        pr: 999,
        file: "tests/tombstones.rs".to_string(),
    }];
    let (stale, current) =
        classify_tombstones(&entries, &merge_dates, Some("2024-06-01T00:00:00Z"));
    assert!(stale.is_empty());
    assert!(current.is_empty());
}

// ============================================================
// Integration tests — run_impl via CLI binary with gh stubs
// ============================================================

/// Build a `Command` targeting the compiled `flow-rs` test binary.
fn flow_rs() -> Command {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
}

/// Parse JSON from the subprocess stdout, panicking with diagnostics on failure.
fn parse_stdout(output: &std::process::Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "Failed to parse JSON: {}\nstdout: {}\nstderr: {}",
            e,
            stdout,
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// Initialize a minimal git repo so `project_root()` can resolve via
/// `git worktree list --porcelain`.
fn setup_tombstone_repo(dir: &std::path::Path) {
    Command::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "commit.gpgsign", "false"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir)
        .output()
        .unwrap();
}

/// Write a multi-dispatch `gh` stub script that routes on `$1`:
/// - `pr` → echo threshold_response, exit threshold_exit
/// - `api` → echo graphql_response, exit graphql_exit
/// - `repo` → echo repo_response, exit repo_exit
///
/// Returns the stub directory path (to prepend to PATH).
fn write_gh_multi_stub(
    dir: &std::path::Path,
    threshold_response: &str,
    threshold_exit: i32,
    graphql_response: &str,
    graphql_exit: i32,
    repo_response: &str,
    repo_exit: i32,
) -> std::path::PathBuf {
    let stub_dir = dir.join("stubs");
    fs::create_dir_all(&stub_dir).unwrap();
    let stub_path = stub_dir.join("gh");
    // Escape single quotes for safe embedding in bash
    let thresh_escaped = threshold_response.replace('\'', "'\\''");
    let gql_escaped = graphql_response.replace('\'', "'\\''");
    let repo_escaped = repo_response.replace('\'', "'\\''");
    let script = format!(
        r#"#!/bin/bash
case "$1" in
  pr)
    echo '{thresh}'
    exit {thresh_exit}
    ;;
  api)
    echo '{gql}'
    exit {gql_exit}
    ;;
  repo)
    echo '{repo}'
    exit {repo_exit}
    ;;
  *)
    exit 1
    ;;
esac
"#,
        thresh = thresh_escaped,
        thresh_exit = threshold_exit,
        gql = gql_escaped,
        gql_exit = graphql_exit,
        repo = repo_escaped,
        repo_exit = repo_exit,
    );
    fs::write(&stub_path, script).unwrap();
    let mut perms = fs::metadata(&stub_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&stub_path, perms).unwrap();
    stub_dir
}

/// Convenience: write a gh stub with default repo detection (always fails).
fn write_gh_stub_simple(
    dir: &std::path::Path,
    threshold_response: &str,
    threshold_exit: i32,
    graphql_response: &str,
    graphql_exit: i32,
) -> std::path::PathBuf {
    write_gh_multi_stub(
        dir,
        threshold_response,
        threshold_exit,
        graphql_response,
        graphql_exit,
        "",
        1,
    )
}

#[test]
fn run_impl_no_tombstones() {
    let dir = tempfile::tempdir().unwrap();
    setup_tombstone_repo(dir.path());
    // Create an empty tests/ directory — no .rs files with tombstones
    fs::create_dir(dir.path().join("tests")).unwrap();

    let output = flow_rs()
        .args(["tombstone-audit", "--repo", "owner/repo"])
        .current_dir(dir.path())
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();

    assert!(output.status.success(), "Should exit 0 with no tombstones");
    let data = parse_stdout(&output);
    assert_eq!(data["total_tombstones"], 0);
    assert_eq!(data["unique_prs"], 0);
    assert_eq!(data["stale"].as_array().unwrap().len(), 0);
    assert_eq!(data["current"].as_array().unwrap().len(), 0);
}

#[test]
fn run_impl_stale_tombstones() {
    let dir = tempfile::tempdir().unwrap();
    setup_tombstone_repo(dir.path());
    let tests_dir = dir.path().join("tests");
    fs::create_dir(&tests_dir).unwrap();
    fs::write(
        tests_dir.join("tombstones.rs"),
        tombstone_line(839, " Must not return."),
    )
    .unwrap();

    let graphql = r#"{"data":{"repository":{"pr_839":{"mergedAt":"2024-01-15T10:00:00Z"}}}}"#;
    let stub_dir = write_gh_stub_simple(dir.path(), "2024-06-01T00:00:00Z", 0, graphql, 0);

    let output = flow_rs()
        .args(["tombstone-audit", "--repo", "owner/repo"])
        .current_dir(dir.path())
        .env("PATH", format!("{}:/usr/bin:/bin", stub_dir.display()))
        .output()
        .unwrap();

    assert!(output.status.success());
    let data = parse_stdout(&output);
    assert_eq!(data["total_tombstones"], 1);
    assert_eq!(data["stale"].as_array().unwrap().len(), 1);
    assert_eq!(data["stale"][0]["pr"], 839);
    assert_eq!(data["current"].as_array().unwrap().len(), 0);
    assert_eq!(data["threshold"], "2024-06-01T00:00:00Z");
}

#[test]
fn run_impl_current_tombstones() {
    let dir = tempfile::tempdir().unwrap();
    setup_tombstone_repo(dir.path());
    let tests_dir = dir.path().join("tests");
    fs::create_dir(&tests_dir).unwrap();
    fs::write(
        tests_dir.join("tombstones.rs"),
        tombstone_line(924, " Must not return."),
    )
    .unwrap();

    let graphql = r#"{"data":{"repository":{"pr_924":{"mergedAt":"2024-08-01T10:00:00Z"}}}}"#;
    let stub_dir = write_gh_stub_simple(dir.path(), "2024-06-01T00:00:00Z", 0, graphql, 0);

    let output = flow_rs()
        .args(["tombstone-audit", "--repo", "owner/repo"])
        .current_dir(dir.path())
        .env("PATH", format!("{}:/usr/bin:/bin", stub_dir.display()))
        .output()
        .unwrap();

    assert!(output.status.success());
    let data = parse_stdout(&output);
    assert_eq!(data["total_tombstones"], 1);
    assert_eq!(data["stale"].as_array().unwrap().len(), 0);
    assert_eq!(data["current"].as_array().unwrap().len(), 1);
    assert_eq!(data["current"][0]["pr"], 924);
    assert_eq!(data["threshold"], "2024-06-01T00:00:00Z");
}

#[test]
fn run_impl_mixed_stale_and_current() {
    let dir = tempfile::tempdir().unwrap();
    setup_tombstone_repo(dir.path());
    let tests_dir = dir.path().join("tests");
    fs::create_dir(&tests_dir).unwrap();
    let content = format!(
        "{}\n{}",
        tombstone_line(839, " Must not return."),
        tombstone_line(924, " Must not return."),
    );
    fs::write(tests_dir.join("tombstones.rs"), content).unwrap();

    let graphql = r#"{"data":{"repository":{"pr_839":{"mergedAt":"2024-01-15T10:00:00Z"},"pr_924":{"mergedAt":"2024-08-01T10:00:00Z"}}}}"#;
    let stub_dir = write_gh_stub_simple(dir.path(), "2024-06-01T00:00:00Z", 0, graphql, 0);

    let output = flow_rs()
        .args(["tombstone-audit", "--repo", "owner/repo"])
        .current_dir(dir.path())
        .env("PATH", format!("{}:/usr/bin:/bin", stub_dir.display()))
        .output()
        .unwrap();

    assert!(output.status.success());
    let data = parse_stdout(&output);
    assert_eq!(data["total_tombstones"], 2);
    assert_eq!(data["unique_prs"], 2);

    let stale = data["stale"].as_array().unwrap();
    let current = data["current"].as_array().unwrap();
    assert_eq!(stale.len(), 1);
    assert_eq!(current.len(), 1);

    let stale_prs: Vec<u64> = stale.iter().map(|e| e["pr"].as_u64().unwrap()).collect();
    let current_prs: Vec<u64> = current.iter().map(|e| e["pr"].as_u64().unwrap()).collect();
    assert!(stale_prs.contains(&839));
    assert!(current_prs.contains(&924));
}

#[test]
fn run_impl_no_open_prs_all_stale() {
    let dir = tempfile::tempdir().unwrap();
    setup_tombstone_repo(dir.path());
    let tests_dir = dir.path().join("tests");
    fs::create_dir(&tests_dir).unwrap();
    fs::write(
        tests_dir.join("tombstones.rs"),
        tombstone_line(839, " Must not return."),
    )
    .unwrap();

    let graphql = r#"{"data":{"repository":{"pr_839":{"mergedAt":"2024-01-15T10:00:00Z"}}}}"#;
    // Empty threshold response means no open PRs
    let stub_dir = write_gh_stub_simple(dir.path(), "", 0, graphql, 0);

    let output = flow_rs()
        .args(["tombstone-audit", "--repo", "owner/repo"])
        .current_dir(dir.path())
        .env("PATH", format!("{}:/usr/bin:/bin", stub_dir.display()))
        .output()
        .unwrap();

    assert!(output.status.success());
    let data = parse_stdout(&output);
    assert_eq!(data["stale"].as_array().unwrap().len(), 1);
    assert_eq!(data["current"].as_array().unwrap().len(), 0);
    assert!(data["threshold"].is_null());
}

#[test]
fn run_impl_threshold_fetch_error() {
    let dir = tempfile::tempdir().unwrap();
    setup_tombstone_repo(dir.path());
    let tests_dir = dir.path().join("tests");
    fs::create_dir(&tests_dir).unwrap();
    fs::write(
        tests_dir.join("tombstones.rs"),
        tombstone_line(839, " Must not return."),
    )
    .unwrap();

    // gh pr list exits 1 → fetch_threshold returns Err → run_impl returns Ok with threshold_error
    let stub_dir = write_gh_stub_simple(dir.path(), "", 1, "", 0);

    let output = flow_rs()
        .args(["tombstone-audit", "--repo", "owner/repo"])
        .current_dir(dir.path())
        .env("PATH", format!("{}:/usr/bin:/bin", stub_dir.display()))
        .output()
        .unwrap();

    // threshold_error is NOT a run_impl error — it returns Ok with a special status
    assert!(
        output.status.success(),
        "threshold_error should exit 0, not 1"
    );
    let data = parse_stdout(&output);
    assert_eq!(data["status"], "threshold_error");
    assert!(data["message"].as_str().unwrap().contains("failed"));
    assert_eq!(data["total_tombstones"], 1);
}

#[test]
fn run_impl_detect_repo_failure() {
    let dir = tempfile::tempdir().unwrap();
    setup_tombstone_repo(dir.path());
    let tests_dir = dir.path().join("tests");
    fs::create_dir(&tests_dir).unwrap();
    fs::write(
        tests_dir.join("tombstones.rs"),
        tombstone_line(839, " Must not return."),
    )
    .unwrap();

    // No --repo flag → detect_repo runs → gh repo view fails → run_impl returns Err
    let stub_dir = write_gh_multi_stub(dir.path(), "", 0, "", 0, "", 1);

    let output = flow_rs()
        .args(["tombstone-audit"])
        .current_dir(dir.path())
        .env("PATH", format!("{}:/usr/bin:/bin", stub_dir.display()))
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "detect_repo failure should exit non-zero"
    );
    let data = parse_stdout(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap()
        .contains("detect repository"));
}

#[test]
fn run_impl_graphql_failure() {
    let dir = tempfile::tempdir().unwrap();
    setup_tombstone_repo(dir.path());
    let tests_dir = dir.path().join("tests");
    fs::create_dir(&tests_dir).unwrap();
    fs::write(
        tests_dir.join("tombstones.rs"),
        tombstone_line(839, " Must not return."),
    )
    .unwrap();

    // gh api graphql exits 1 → fetch_merge_dates returns empty map → entries skipped
    let stub_dir = write_gh_stub_simple(dir.path(), "2024-06-01T00:00:00Z", 0, "", 1);

    let output = flow_rs()
        .args(["tombstone-audit", "--repo", "owner/repo"])
        .current_dir(dir.path())
        .env("PATH", format!("{}:/usr/bin:/bin", stub_dir.display()))
        .output()
        .unwrap();

    assert!(output.status.success());
    let data = parse_stdout(&output);
    assert_eq!(data["total_tombstones"], 1);
    // No merge data → entries are skipped in classify_tombstones
    assert_eq!(data["stale"].as_array().unwrap().len(), 0);
    assert_eq!(data["current"].as_array().unwrap().len(), 0);
}
