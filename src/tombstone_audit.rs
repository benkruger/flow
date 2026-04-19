//! Tombstone audit: scan test files for tombstone PR references,
//! query GitHub for merge dates, and classify as stale or current.
//!
//! A tombstone is stale when the PR that removed the feature was merged
//! before the oldest open PR was created — meaning no active branch
//! could have forked before the deletion.

use crate::git::project_root;
use clap::Parser;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// CLI arguments for the tombstone-audit subcommand.
#[derive(Parser, Debug)]
pub struct Args {
    /// Repository in owner/repo format (default: auto-detect from git remote)
    #[arg(long)]
    pub repo: Option<String>,
}

/// A tombstone entry found in a test file.
#[derive(Debug, Clone)]
pub struct TombstoneEntry {
    /// PR number referenced in the tombstone comment.
    pub pr: u64,
    /// Relative path to the test file containing the tombstone.
    pub file: String,
}

/// Merge info for a PR from the GraphQL response.
#[derive(Debug, Clone)]
pub struct MergeInfo {
    /// ISO 8601 timestamp when the PR was merged, or None if unmerged.
    pub merged_at: Option<String>,
}

/// Extract unique PR numbers from tombstone comments in source content.
///
/// Matches the pattern `Tombstone:.*PR #(\d+)` in any context
/// (double-slash comments, doc comments, assertion message strings).
/// Returns a deduplicated set of PR numbers.
pub fn extract_pr_numbers(content: &str) -> HashSet<u64> {
    let re = Regex::new(r"Tombstone:.*?PR #(\d+)").unwrap();
    re.captures_iter(content)
        .filter_map(|cap| cap[1].parse::<u64>().ok())
        .filter(|&n| n > 0) // PR #0 is not a valid GitHub PR number
        .collect()
}

/// Scan all `tests/*.rs` files under the given root for tombstone PR references.
///
/// Returns a list of `TombstoneEntry` with the PR number and relative file path.
/// Each unique (PR, file) pair produces one entry.
pub fn scan_test_files(root: &Path) -> Vec<TombstoneEntry> {
    let tests_dir = root.join("tests");
    if !tests_dir.is_dir() {
        return Vec::new();
    }

    let mut entries = Vec::new();
    let read_dir = match std::fs::read_dir(&tests_dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let prs = extract_pr_numbers(&content);
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();
        for pr in prs {
            entries.push(TombstoneEntry {
                pr,
                file: relative.clone(),
            });
        }
    }

    entries
}

/// Build a GraphQL query to fetch `mergedAt` for a list of PR numbers.
///
/// Uses aliased fields so all PRs can be fetched in a single query.
/// Follows the same pattern as `analyze_issues::build_blocker_query`.
pub fn build_merge_query(pr_numbers: &[u64]) -> String {
    let fragments: Vec<String> = pr_numbers
        .iter()
        .map(|n| format!("pr_{}: pullRequest(number: {}) {{ mergedAt }}", n, n))
        .collect();
    let body = fragments.join(" ");
    format!(
        "query($owner: String!, $repo: String!) {{ repository(owner: $owner, name: $repo) {{ {} }} }}",
        body
    )
}

/// Parse a GraphQL response containing `mergedAt` fields for PRs.
///
/// Returns a map from PR number to `MergeInfo`.
/// Missing PRs or null `mergedAt` values produce `MergeInfo { merged_at: None }`.
/// Returns an empty map on parse failure.
pub fn parse_merge_response(json_str: &str, pr_numbers: &[u64]) -> HashMap<u64, MergeInfo> {
    let data: Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return HashMap::new(),
    };

    let repo_data = data.get("data").and_then(|d| d.get("repository"));

    let mut result = HashMap::new();
    for &pr in pr_numbers {
        let key = format!("pr_{}", pr);
        let merged_at = repo_data
            .and_then(|r| r.get(&key))
            .and_then(|pr_obj| pr_obj.get("mergedAt"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        result.insert(pr, MergeInfo { merged_at });
    }

    result
}

/// Classify tombstone entries as stale or current based on merge dates.
///
/// - `threshold`: ISO 8601 timestamp of the oldest open PR's creation date.
///   If `None`, there are no open PRs and all merged tombstones are stale.
/// - Unmerged PRs (merged_at is None) are skipped.
/// - PRs with no merge data are skipped.
/// - Stale: merged before the threshold (or threshold is None).
/// - Current: merged at or after the threshold.
pub fn classify_tombstones(
    entries: &[TombstoneEntry],
    merge_dates: &HashMap<u64, MergeInfo>,
    threshold: Option<&str>,
) -> (Vec<TombstoneEntry>, Vec<TombstoneEntry>) {
    let mut stale = Vec::new();
    let mut current = Vec::new();

    for entry in entries {
        let info = match merge_dates.get(&entry.pr) {
            Some(info) => info,
            None => continue, // No data for this PR — skip
        };

        let merged_at = match &info.merged_at {
            Some(ts) => ts,
            None => continue, // Unmerged — skip
        };

        match threshold {
            None => {
                // No open PRs — all merged tombstones are stale
                stale.push(entry.clone());
            }
            Some(thresh) => {
                // Simple string comparison works for ISO 8601 timestamps
                if merged_at.as_str() < thresh {
                    stale.push(entry.clone());
                } else {
                    current.push(entry.clone());
                }
            }
        }
    }

    (stale, current)
}

/// Detect the repository from git remote.
fn detect_repo() -> Option<String> {
    let output = std::process::Command::new("gh")
        .args([
            "repo",
            "view",
            "--json",
            "nameWithOwner",
            "-q",
            ".nameWithOwner",
        ])
        .output()
        .ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if s.contains('/') {
            return Some(s);
        }
    }
    None
}

/// Fetch the oldest open PR creation date as the staleness threshold.
///
/// Returns `Ok(Some(date))` when open PRs exist, `Ok(None)` when no
/// open PRs exist (empty or "null" response), and `Err` on API failure.
/// Callers must distinguish Ok(None) (all stale) from Err (skip audit).
fn fetch_threshold(repo: &str) -> Result<Option<String>, String> {
    let output = std::process::Command::new("gh")
        .args([
            "pr",
            "list",
            "--repo",
            repo,
            "--state",
            "open",
            "--json",
            "createdAt",
            "--jq",
            "min_by(.createdAt).createdAt",
        ])
        .output()
        .map_err(|e| format!("gh pr list failed to execute: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "gh pr list failed with exit code {}",
            output.status.code().unwrap_or(-1)
        ));
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !s.is_empty() && s != "null" {
        Ok(Some(s))
    } else {
        Ok(None)
    }
}

/// Fetch merge dates for PRs via batched GraphQL query.
fn fetch_merge_dates(repo: &str, pr_numbers: &[u64]) -> HashMap<u64, MergeInfo> {
    if pr_numbers.is_empty() {
        return HashMap::new();
    }

    let parts: Vec<&str> = repo.splitn(2, '/').collect();
    if parts.len() != 2 {
        return HashMap::new();
    }
    let (owner, name) = (parts[0], parts[1]);

    let query = build_merge_query(pr_numbers);

    let output = match std::process::Command::new("gh")
        .args([
            "api",
            "graphql",
            "-f",
            &format!("query={}", query),
            "-f",
            &format!("owner={}", owner),
            "-f",
            &format!("repo={}", name),
        ])
        .output()
    {
        Ok(o) => o,
        Err(_) => return HashMap::new(),
    };

    if !output.status.success() {
        return HashMap::new();
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    parse_merge_response(&json_str, pr_numbers)
}

/// Entry point following the run_impl pattern.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let root = project_root();

    // Scan all test files for tombstone PR references
    let entries = scan_test_files(&root);
    if entries.is_empty() {
        return Ok(json!({
            "stale": [],
            "current": [],
            "total_tombstones": 0,
            "unique_prs": 0,
            "threshold": null
        }));
    }

    // Deduplicate PR numbers
    let unique_prs: Vec<u64> = {
        let set: HashSet<u64> = entries.iter().map(|e| e.pr).collect();
        let mut v: Vec<u64> = set.into_iter().collect();
        v.sort();
        v
    };

    // Detect repo
    let repo = match &args.repo {
        Some(r) => r.clone(),
        None => detect_repo().ok_or("Could not detect repository from git remote")?,
    };

    // Fetch threshold (oldest open PR creation date)
    // Ok(Some) = threshold date, Ok(None) = no open PRs, Err = API failure
    let threshold = match fetch_threshold(&repo) {
        Ok(t) => t,
        Err(e) => {
            return Ok(json!({
                "status": "threshold_error",
                "message": e,
                "stale": [],
                "current": [],
                "total_tombstones": entries.len(),
                "unique_prs": unique_prs.len(),
                "threshold": null
            }));
        }
    };

    // Fetch merge dates for all unique PRs
    let merge_dates = fetch_merge_dates(&repo, &unique_prs);

    // Classify
    let (stale, current) = classify_tombstones(&entries, &merge_dates, threshold.as_deref());

    // Build output
    let stale_json: Vec<Value> = stale
        .iter()
        .map(|e| {
            let merged_at = merge_dates
                .get(&e.pr)
                .and_then(|m| m.merged_at.as_deref())
                .unwrap_or("unknown");
            json!({
                "pr": e.pr,
                "merged_at": merged_at,
                "file": e.file,
            })
        })
        .collect();

    let current_json: Vec<Value> = current
        .iter()
        .map(|e| {
            let merged_at = merge_dates
                .get(&e.pr)
                .and_then(|m| m.merged_at.as_deref())
                .unwrap_or("unknown");
            json!({
                "pr": e.pr,
                "merged_at": merged_at,
                "file": e.file,
            })
        })
        .collect();

    Ok(json!({
        "stale": stale_json,
        "current": current_json,
        "total_tombstones": entries.len(),
        "unique_prs": unique_prs.len(),
        "threshold": threshold,
    }))
}
