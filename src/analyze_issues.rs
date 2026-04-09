//! Analyze open GitHub issues for the flow-issues skill.
//!
//! Handles mechanical work: JSON parsing, file path extraction,
//! label detection, stale detection. Outputs condensed per-issue
//! briefs so the LLM only needs to rank by impact.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use regex::Regex;
use serde_json::Value;

/// Known directory prefixes for file path extraction.
const DIR_PREFIXES: &[&str] = &[
    "lib/",
    "skills/",
    "tests/",
    "docs/",
    "hooks/",
    "frameworks/",
    ".claude/",
    "bin/",
    "agents/",
    "src/",
    "config/",
    "app/",
];

/// Extract file paths from issue body text.
///
/// Recognizes paths with known directory prefixes and paths containing
/// slashes with recognized file extensions. Returns deduplicated sorted list.
pub fn extract_file_paths(body: &str) -> Vec<String> {
    let mut paths: HashSet<String> = HashSet::new();

    // Match paths with known directory prefixes
    for prefix in DIR_PREFIXES {
        let escaped = regex::escape(prefix);
        let pattern = format!("{}{}", escaped, r"[\w./\-]+");
        let re = Regex::new(&pattern).unwrap();
        for mat in re.find_iter(body) {
            paths.insert(mat.as_str().to_string());
        }
    }

    // Match paths with file extensions (must contain /)
    // Python uses lookbehind/lookahead; regex crate doesn't support those.
    // Use (?:^|[^\w]) prefix and (?:$|[^\w]) suffix with capture group for the path.
    let file_ext_re = Regex::new(
        r"(?:^|[^\w])([\w./\-]+/[\w.\-]+\.(?:py|md|json|sh|yml|yaml|rb|js|ts|html|css|toml))(?:$|[^\w])",
    )
    .unwrap();
    for cap in file_ext_re.captures_iter(body) {
        paths.insert(cap[1].to_string());
    }

    let mut result: Vec<String> = paths.into_iter().collect();
    result.sort();
    result
}

/// Label detection result.
pub struct LabelFlags {
    pub in_progress: bool,
    pub decomposed: bool,
    pub blocked: bool,
}

/// Check for Flow In-Progress, Decomposed, and Blocked labels.
pub fn detect_labels(labels: &[Value]) -> LabelFlags {
    let label_names: HashSet<String> = labels
        .iter()
        .filter_map(|l| l.get("name")?.as_str().map(String::from))
        .collect();

    LabelFlags {
        in_progress: label_names.contains("Flow In-Progress"),
        decomposed: label_names
            .iter()
            .any(|n| n.eq_ignore_ascii_case("decomposed")),
        blocked: label_names
            .iter()
            .any(|n| n.eq_ignore_ascii_case("blocked")),
    }
}

/// Label categories checked in order.
const LABEL_CATEGORIES: &[&str] = &[
    "Rule",
    "Flow",
    "Flaky Test",
    "Tech Debt",
    "Documentation Drift",
];

/// Assign a category based on label names first, then content fallback.
pub fn categorize(label_names: &HashSet<String>, title: &str, body: &str) -> String {
    for &label in LABEL_CATEGORIES {
        if label_names.contains(label) {
            return label.to_string();
        }
    }

    let combined = format!("{} {}", title, body);
    let bug_re = Regex::new(r"(?i)\b(bug|fix|crash|error|broken|fail|wrong|incorrect)\b").unwrap();
    let enhancement_re =
        Regex::new(r"(?i)\b(add|new|feature|enhance|improve|support|implement)\b").unwrap();

    if bug_re.is_match(&combined) {
        return "Bug".to_string();
    }
    if enhancement_re.is_match(&combined) {
        return "Enhancement".to_string();
    }
    "Other".to_string()
}

/// Stale check result.
pub struct StaleInfo {
    pub stale: bool,
    pub stale_missing: usize,
}

/// Check if an issue is stale (>60 days old with missing file refs).
pub fn check_stale(file_paths: &[String], age_days: i64) -> StaleInfo {
    if age_days < 60 || file_paths.is_empty() {
        return StaleInfo {
            stale: false,
            stale_missing: 0,
        };
    }

    let missing = file_paths
        .iter()
        .filter(|fp| !Path::new(fp).exists())
        .count();
    StaleInfo {
        stale: missing > 0,
        stale_missing: missing,
    }
}

/// Truncate body to max_length, adding ellipsis if needed.
/// Uses char count (not byte count) per rust-port-parity rule.
pub fn truncate_body(body: &str, max_length: usize) -> String {
    if body.chars().count() <= max_length {
        return body.to_string();
    }
    let truncated: String = body.chars().take(max_length).collect();
    format!("{}...", truncated)
}

/// Build the GraphQL query for fetching blocker details.
///
/// Returns the full query string with aliased fragments for each issue number.
/// Uses the `blockedBy` connection to get actual blocker issue numbers and state.
pub fn build_blocker_query(issue_numbers: &[i64]) -> String {
    let fragments: Vec<String> = issue_numbers
        .iter()
        .map(|n| {
            format!(
                "issue_{}: issue(number: {}) {{ blockedBy(first: 10) {{ nodes {{ number state }} }} }}",
                n, n
            )
        })
        .collect();
    let body = fragments.join(" ");
    format!(
        "query($owner: String!, $repo: String!) {{ repository(owner: $owner, name: $repo) {{ {} }} }}",
        body
    )
}

/// Parse a GraphQL response for blocker details.
///
/// Extracts `blockedBy.nodes` for each issue number.
/// Returns HashMap mapping issue number to list of open blocker issue numbers.
/// Only includes blockers where `state == "OPEN"` — closed blockers are resolved.
/// Handles null values at any level gracefully (defaults to empty vec).
pub fn parse_blocker_response(json_str: &str, issue_numbers: &[i64]) -> HashMap<i64, Vec<i64>> {
    let data: Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return HashMap::new(),
    };

    // Navigate: data.data.repository
    let repo_data = data.get("data").and_then(|d| d.get("repository"));

    // repo_data may be null or absent
    let repo_obj = match repo_data {
        Some(Value::Object(m)) => Some(m),
        _ => None,
    };

    let mut blockers = HashMap::new();
    for &number in issue_numbers {
        let key = format!("issue_{}", number);
        let nodes = repo_obj
            .and_then(|m| m.get(&key))
            .and_then(|issue| issue.get("blockedBy"))
            .and_then(|blocked_by| blocked_by.get("nodes"))
            .and_then(|n| n.as_array());

        let blocker_numbers: Vec<i64> = match nodes {
            Some(arr) => arr
                .iter()
                .filter(|node| {
                    node.get("state")
                        .and_then(|s| s.as_str())
                        .map(|s| s == "OPEN")
                        .unwrap_or(false)
                })
                .filter_map(|node| node.get("number").and_then(|n| n.as_i64()))
                .collect(),
            None => Vec::new(),
        };
        blockers.insert(number, blocker_numbers);
    }

    blockers
}

/// Fetch native blocked-by details for issues via GitHub GraphQL API.
///
/// Uses `blockedBy(first: 10)` connection with batched aliased queries.
/// Returns HashMap mapping issue number to list of open blocker issue numbers.
/// Returns empty HashMap on any failure (graceful degradation).
/// Timeout: 30 seconds (matches Python).
pub fn fetch_blockers(repo: &str, issue_numbers: &[i64]) -> HashMap<i64, Vec<i64>> {
    if issue_numbers.is_empty() {
        return HashMap::new();
    }

    if !repo.contains('/') {
        return HashMap::new();
    }

    let parts: Vec<&str> = repo.splitn(2, '/').collect();
    let owner = parts[0];
    let name = parts[1];

    let query = build_blocker_query(issue_numbers);

    let mut child = match std::process::Command::new("gh")
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
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };

    // Drain stdout in a thread to prevent pipe buffer deadlock, then
    // poll for exit with a 30s timeout (matches Python timeout=30).
    let stdout_handle = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut pipe) = stdout_handle {
            use std::io::Read;
            let _ = pipe.read_to_string(&mut buf);
        }
        buf
    });

    let timeout = std::time::Duration::from_secs(30);
    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return HashMap::new();
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(_) => break None,
        }
    };

    let stdout = reader.join().unwrap_or_default();

    match status {
        Some(s) if s.success() => parse_blocker_response(&stdout, issue_numbers),
        _ => HashMap::new(),
    }
}

/// Analyze a list of issues from gh issue list JSON.
///
/// Separates in-progress issues from available issues and enriches
/// each available issue with labels, category, age, stale info, etc.
/// The `blocker_map` maps issue numbers to lists of open blocker issue numbers.
pub fn analyze_issues(issues: &[Value], blocker_map: &HashMap<i64, Vec<i64>>) -> Value {
    if issues.is_empty() {
        return serde_json::json!({
            "status": "ok",
            "total": 0,
            "in_progress": [],
            "issues": [],
        });
    }

    let mut in_progress = Vec::new();
    let mut available = Vec::new();

    for issue in issues {
        let number = issue["number"].as_i64().unwrap_or(0);
        let body = issue.get("body").and_then(|b| b.as_str()).unwrap_or("");
        let labels_arr = issue.get("labels").and_then(|l| l.as_array());
        let labels_vec: Vec<Value> = labels_arr.cloned().unwrap_or_default();

        let label_names: HashSet<String> = labels_vec
            .iter()
            .filter_map(|l| l.get("name")?.as_str().map(String::from))
            .collect();
        let mut label_list: Vec<String> = label_names.iter().cloned().collect();
        label_list.sort();

        let label_flags = detect_labels(&labels_vec);

        if label_flags.in_progress {
            in_progress.push(serde_json::json!({
                "number": number,
                "title": issue["title"],
                "url": issue.get("url").cloned().unwrap_or(Value::String(String::new())),
            }));
            continue;
        }

        let file_paths = extract_file_paths(body);

        let created_at_str = issue
            .get("createdAt")
            .and_then(|c| c.as_str())
            .unwrap_or("");
        let age_days = if let Ok(created) = chrono::DateTime::parse_from_rfc3339(created_at_str) {
            let now = chrono::Utc::now();
            (now - created.with_timezone(&chrono::Utc)).num_days()
        } else {
            // Try ISO format with Z suffix replaced
            let normalized = created_at_str.replace('Z', "+00:00");
            if let Ok(created) = chrono::DateTime::parse_from_rfc3339(&normalized) {
                let now = chrono::Utc::now();
                (now - created.with_timezone(&chrono::Utc)).num_days()
            } else {
                0
            }
        };

        let stale_info = check_stale(&file_paths, age_days);
        let category = categorize(&label_names, issue["title"].as_str().unwrap_or(""), body);

        let blocked_by = blocker_map.get(&number).cloned().unwrap_or_default();
        let native_blocked = !blocked_by.is_empty();

        available.push(serde_json::json!({
            "number": number,
            "title": issue["title"],
            "url": issue.get("url").cloned().unwrap_or(Value::String(String::new())),
            "labels": label_list,
            "category": category,
            "age_days": age_days,
            "decomposed": label_flags.decomposed,
            "blocked": label_flags.blocked || native_blocked,
            "native_blocked": native_blocked,
            "blocked_by": blocked_by,
            "stale": stale_info.stale,
            "stale_missing": stale_info.stale_missing,
            "file_paths": file_paths,
            "brief": truncate_body(body, 200),
        }));
    }

    serde_json::json!({
        "status": "ok",
        "total": issues.len(),
        "in_progress": in_progress,
        "issues": available,
    })
}

/// Filter analyzed issues by readiness criteria.
///
/// Valid filter names: "ready", "blocked", "decomposed", "quick-start".
/// Returns filtered list. Returns error string for unknown filters.
pub fn filter_issues(issues: &[Value], filter_name: &str) -> Result<Vec<Value>, String> {
    let predicate: Box<dyn Fn(&Value) -> bool> = match filter_name {
        "ready" => Box::new(|i: &Value| !i["blocked"].as_bool().unwrap_or(false)),
        "blocked" => Box::new(|i: &Value| i["blocked"].as_bool().unwrap_or(false)),
        "decomposed" => Box::new(|i: &Value| i["decomposed"].as_bool().unwrap_or(false)),
        "quick-start" => Box::new(|i: &Value| {
            i["decomposed"].as_bool().unwrap_or(false) && !i["blocked"].as_bool().unwrap_or(false)
        }),
        _ => return Err(format!("Unknown filter: {}", filter_name)),
    };

    Ok(issues.iter().filter(|i| predicate(i)).cloned().collect())
}

/// CLI arguments for the analyze-issues subcommand.
#[derive(clap::Args)]
pub struct Args {
    /// Path to pre-fetched gh issue list JSON file (for testing)
    #[arg(long = "issues-json")]
    pub issues_json: Option<String>,

    /// Show only issues that are not blocked
    #[arg(long, group = "filter_group")]
    pub ready: bool,

    /// Show only issues that are blocked
    #[arg(long, group = "filter_group")]
    pub blocked: bool,

    /// Show only decomposed issues
    #[arg(long, group = "filter_group")]
    pub decomposed: bool,

    /// Show only decomposed issues without Blocked label
    #[arg(long = "quick-start", group = "filter_group")]
    pub quick_start: bool,
}

/// Run the analyze-issues CLI.
pub fn run(args: Args) {
    let issues_json = if let Some(path) = &args.issues_json {
        match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(e) => {
                crate::output::json_error(&format!("Could not read issues file: {}", e), &[]);
                std::process::exit(1);
            }
        }
    } else {
        // Call gh issue list
        let mut child = match std::process::Command::new("gh")
            .args([
                "issue",
                "list",
                "--state",
                "open",
                "--json",
                "number,title,labels,createdAt,body,url",
                "--limit",
                "100",
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                crate::output::json_error(&format!("gh issue list failed: {}", e), &[]);
                std::process::exit(1);
            }
        };

        // Drain stdout in a thread to prevent pipe buffer deadlock, then
        // poll for exit with a 30s timeout (matches Python timeout=30).
        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();
        let stdout_reader = std::thread::spawn(move || {
            let mut buf = String::new();
            if let Some(mut pipe) = stdout_handle {
                use std::io::Read;
                let _ = pipe.read_to_string(&mut buf);
            }
            buf
        });
        let stderr_reader = std::thread::spawn(move || {
            let mut buf = String::new();
            if let Some(mut pipe) = stderr_handle {
                use std::io::Read;
                let _ = pipe.read_to_string(&mut buf);
            }
            buf
        });

        let timeout = std::time::Duration::from_secs(30);
        let start = std::time::Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) => {
                    if start.elapsed() >= timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        crate::output::json_error("gh issue list timed out", &[]);
                        std::process::exit(1);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(e) => {
                    crate::output::json_error(&format!("gh issue list failed: {}", e), &[]);
                    std::process::exit(1);
                }
            }
        };

        let stdout_data = stdout_reader.join().unwrap_or_default();
        let stderr_data = stderr_reader.join().unwrap_or_default();

        match status {
            Some(s) if s.success() => stdout_data,
            _ => {
                crate::output::json_error(
                    &format!("gh issue list failed: {}", stderr_data.trim()),
                    &[],
                );
                std::process::exit(1);
            }
        }
    };

    let issues: Vec<Value> = match serde_json::from_str(&issues_json) {
        Ok(v) => v,
        Err(e) => {
            crate::output::json_error(&format!("Invalid JSON: {}", e), &[]);
            std::process::exit(1);
        }
    };

    // Fetch native blocker details via GraphQL (best-effort)
    let blocker_map = match crate::github::detect_repo(None) {
        Some(repo) => {
            let all_numbers: Vec<i64> =
                issues.iter().filter_map(|i| i["number"].as_i64()).collect();
            fetch_blockers(&repo, &all_numbers)
        }
        None => HashMap::new(),
    };

    let mut output = analyze_issues(&issues, &blocker_map);

    // Apply filter if requested
    let filter_name = if args.ready {
        Some("ready")
    } else if args.blocked {
        Some("blocked")
    } else if args.decomposed {
        Some("decomposed")
    } else if args.quick_start {
        Some("quick-start")
    } else {
        None
    };

    if let Some(name) = filter_name {
        if let Some(issues_arr) = output["issues"].as_array() {
            match filter_issues(issues_arr, name) {
                Ok(filtered) => {
                    let in_progress_count = output["in_progress"]
                        .as_array()
                        .map(|a| a.len())
                        .unwrap_or(0);
                    let count = in_progress_count + filtered.len();
                    output["issues"] = Value::Array(filtered);
                    output["total"] = serde_json::json!(count);
                }
                Err(e) => {
                    crate::output::json_error(&e, &[]);
                    std::process::exit(1);
                }
            }
        }
    }

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- extract_file_paths ---

    #[test]
    fn extracts_directory_prefixed_paths() {
        let body = "Check lib/foo.py and skills/bar/SKILL.md for details.";
        let result = extract_file_paths(body);
        assert!(result.contains(&"lib/foo.py".to_string()));
        assert!(result.contains(&"skills/bar/SKILL.md".to_string()));
    }

    #[test]
    fn extracts_paths_with_file_extensions() {
        let body = "See config/setup.json and src/main.sh";
        let result = extract_file_paths(body);
        assert!(result.contains(&"config/setup.json".to_string()));
        assert!(result.contains(&"src/main.sh".to_string()));
    }

    #[test]
    fn no_file_paths() {
        let result = extract_file_paths("This is a plain description.");
        assert!(result.is_empty());
    }

    #[test]
    fn deduplicates_file_paths() {
        let body = "Check lib/foo.py and also lib/foo.py again";
        let result = extract_file_paths(body);
        assert_eq!(result.iter().filter(|p| *p == "lib/foo.py").count(), 1);
    }

    #[test]
    fn extracts_dotprefix_paths() {
        let body = "Edit .claude/rules/testing.md";
        let result = extract_file_paths(body);
        assert!(result.contains(&".claude/rules/testing.md".to_string()));
    }

    // --- detect_labels ---

    #[test]
    fn detects_in_progress_label() {
        let labels = vec![
            serde_json::json!({"name": "Flow In-Progress"}),
            serde_json::json!({"name": "Bug"}),
        ];
        let result = detect_labels(&labels);
        assert!(result.in_progress);
        assert!(!result.decomposed);
        assert!(!result.blocked);
    }

    #[test]
    fn detects_decomposed_label() {
        let labels = vec![serde_json::json!({"name": "decomposed"})];
        let result = detect_labels(&labels);
        assert!(result.decomposed);
        assert!(!result.in_progress);
        assert!(!result.blocked);
    }

    #[test]
    fn detects_decomposed_label_case_insensitive() {
        let labels = vec![serde_json::json!({"name": "Decomposed"})];
        let result = detect_labels(&labels);
        assert!(result.decomposed);
        assert!(!result.blocked);
    }

    #[test]
    fn detects_blocked_label() {
        let labels = vec![
            serde_json::json!({"name": "Blocked"}),
            serde_json::json!({"name": "Bug"}),
        ];
        let result = detect_labels(&labels);
        assert!(result.blocked);
        assert!(!result.in_progress);
        assert!(!result.decomposed);
    }

    #[test]
    fn detects_blocked_label_case_insensitive() {
        let labels = vec![serde_json::json!({"name": "blocked"})];
        let result = detect_labels(&labels);
        assert!(result.blocked);
    }

    #[test]
    fn no_blocked_label() {
        let labels = vec![serde_json::json!({"name": "Enhancement"})];
        let result = detect_labels(&labels);
        assert!(!result.blocked);
    }

    #[test]
    fn no_special_labels() {
        let labels = vec![serde_json::json!({"name": "Bug"})];
        let result = detect_labels(&labels);
        assert!(!result.in_progress);
        assert!(!result.decomposed);
        assert!(!result.blocked);
    }

    #[test]
    fn empty_labels() {
        let result = detect_labels(&[]);
        assert!(!result.in_progress);
        assert!(!result.decomposed);
        assert!(!result.blocked);
    }

    // --- categorize ---

    #[test]
    fn categorize_by_label() {
        let labels: HashSet<String> = ["Flaky Test".to_string()].into();
        assert_eq!(categorize(&labels, "Some title", "body"), "Flaky Test");
    }

    #[test]
    fn categorize_rule_label() {
        let labels: HashSet<String> = ["Rule".to_string()].into();
        assert_eq!(categorize(&labels, "title", "body"), "Rule");
    }

    #[test]
    fn categorize_flow_label() {
        let labels: HashSet<String> = ["Flow".to_string()].into();
        assert_eq!(categorize(&labels, "title", "body"), "Flow");
    }

    #[test]
    fn categorize_tech_debt_label() {
        let labels: HashSet<String> = ["Tech Debt".to_string()].into();
        assert_eq!(categorize(&labels, "title", "body"), "Tech Debt");
    }

    #[test]
    fn categorize_documentation_drift_label() {
        let labels: HashSet<String> = ["Documentation Drift".to_string()].into();
        assert_eq!(categorize(&labels, "title", "body"), "Documentation Drift");
    }

    #[test]
    fn categorize_bug_by_content() {
        let labels: HashSet<String> = HashSet::new();
        assert_eq!(
            categorize(&labels, "Fix crash on login", "error when"),
            "Bug"
        );
    }

    #[test]
    fn categorize_enhancement_by_content() {
        let labels: HashSet<String> = HashSet::new();
        assert_eq!(
            categorize(&labels, "Add dark mode", "new feature"),
            "Enhancement"
        );
    }

    #[test]
    fn categorize_other_fallback() {
        let labels: HashSet<String> = HashSet::new();
        assert_eq!(categorize(&labels, "Misc cleanup", "tidy up"), "Other");
    }

    // --- check_stale ---

    #[test]
    fn stale_issue_with_missing_files() {
        // Use a path that definitely doesn't exist
        let paths = vec!["/nonexistent/path/lib/missing.py".to_string()];
        let result = check_stale(&paths, 90);
        assert!(result.stale);
        assert_eq!(result.stale_missing, 1);
    }

    #[test]
    fn not_stale_when_files_exist() {
        // Use Cargo.toml which exists in the repo
        let paths = vec!["Cargo.toml".to_string()];
        let result = check_stale(&paths, 90);
        assert!(!result.stale);
        assert_eq!(result.stale_missing, 0);
    }

    #[test]
    fn not_stale_when_recent() {
        let paths = vec!["/nonexistent/lib/missing.py".to_string()];
        let result = check_stale(&paths, 10);
        assert!(!result.stale);
    }

    #[test]
    fn not_stale_when_no_file_paths() {
        let result = check_stale(&[], 90);
        assert!(!result.stale);
    }

    // --- truncate_body ---

    #[test]
    fn truncate_body_short() {
        assert_eq!(truncate_body("short text", 200), "short text");
    }

    #[test]
    fn truncate_body_long() {
        let body: String = "x".repeat(300);
        let result = truncate_body(&body, 200);
        assert!(result.chars().count() <= 203); // 200 + "..."
        assert!(result.ends_with("..."));
    }

    // --- build_blocker_query ---

    #[test]
    fn build_blocker_query_single_issue() {
        let query = build_blocker_query(&[10]);
        assert!(query.contains("issue_10: issue(number: 10)"));
        assert!(query.contains("blockedBy(first: 10)"));
        assert!(query.contains("nodes"));
        assert!(query.contains("number"));
        assert!(query.contains("state"));
    }

    #[test]
    fn build_blocker_query_multiple_issues() {
        let query = build_blocker_query(&[10, 20, 30]);
        assert!(query.contains("issue_10: issue(number: 10)"));
        assert!(query.contains("issue_20: issue(number: 20)"));
        assert!(query.contains("issue_30: issue(number: 30)"));
    }

    #[test]
    fn build_blocker_query_has_variables() {
        let query = build_blocker_query(&[1]);
        assert!(query.contains("$owner: String!"));
        assert!(query.contains("$repo: String!"));
    }

    // --- parse_blocker_response ---

    /// Build a GraphQL response with blockedBy nodes for testing.
    /// Each entry is (issue_number, vec of (blocker_number, state)).
    fn graphql_response(issue_blockers: &[(i64, Vec<(i64, &str)>)]) -> String {
        let mut repo_data = serde_json::Map::new();
        for (number, blockers) in issue_blockers {
            let nodes: Vec<Value> = blockers
                .iter()
                .map(|(n, state)| serde_json::json!({"number": n, "state": state}))
                .collect();
            repo_data.insert(
                format!("issue_{}", number),
                serde_json::json!({"blockedBy": {"nodes": nodes}}),
            );
        }
        serde_json::json!({
            "data": {"repository": repo_data}
        })
        .to_string()
    }

    #[test]
    fn parse_blocker_response_happy_path() {
        let response = graphql_response(&[
            (10, vec![(100, "OPEN"), (101, "OPEN")]),
            (20, vec![]),
            (30, vec![(200, "OPEN")]),
        ]);
        let result = parse_blocker_response(&response, &[10, 20, 30]);
        assert_eq!(result[&10], vec![100, 101]);
        assert!(result[&20].is_empty());
        assert_eq!(result[&30], vec![200]);
    }

    #[test]
    fn parse_blocker_response_filters_closed() {
        let response = graphql_response(&[(10, vec![(100, "OPEN"), (101, "CLOSED")])]);
        let result = parse_blocker_response(&response, &[10]);
        assert_eq!(result[&10], vec![100]);
    }

    #[test]
    fn parse_blocker_response_all_closed_returns_empty() {
        let response = graphql_response(&[(10, vec![(100, "CLOSED"), (101, "CLOSED")])]);
        let result = parse_blocker_response(&response, &[10]);
        assert!(result[&10].is_empty());
    }

    #[test]
    fn parse_blocker_response_malformed_json() {
        let result = parse_blocker_response("{corrupt", &[10]);
        assert!(result.is_empty());
    }

    #[test]
    fn parse_blocker_response_null_repository() {
        let response = r#"{"data":{"repository":null}}"#;
        let result = parse_blocker_response(response, &[10]);
        assert!(result[&10].is_empty());
    }

    #[test]
    fn parse_blocker_response_null_blocked_by() {
        let response = r#"{"data":{"repository":{"issue_10":{"blockedBy":null}}}}"#;
        let result = parse_blocker_response(response, &[10]);
        assert!(result[&10].is_empty());
    }

    #[test]
    fn parse_blocker_response_null_nodes() {
        let response = r#"{"data":{"repository":{"issue_10":{"blockedBy":{"nodes":null}}}}}"#;
        let result = parse_blocker_response(response, &[10]);
        assert!(result[&10].is_empty());
    }

    // --- fetch_blockers ---

    #[test]
    fn fetch_blockers_empty_list() {
        let result = fetch_blockers("owner/repo", &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn fetch_blockers_malformed_repo() {
        let result = fetch_blockers("noslash", &[10]);
        assert!(result.is_empty());
    }

    // --- analyze_issues helpers ---

    fn make_issue(
        number: i64,
        title: &str,
        body: &str,
        labels: &[&str],
        created_at: &str,
    ) -> Value {
        let label_arr: Vec<Value> = labels
            .iter()
            .map(|n| serde_json::json!({"name": n}))
            .collect();
        serde_json::json!({
            "number": number,
            "title": title,
            "body": body,
            "labels": label_arr,
            "createdAt": created_at,
            "url": format!("https://github.com/test/repo/issues/{}", number),
        })
    }

    fn now_iso() -> String {
        chrono::Local::now().to_rfc3339()
    }

    // --- analyze_issues ---

    #[test]
    fn analyze_empty_list() {
        let result = analyze_issues(&[], &HashMap::new());
        assert_eq!(result["status"], "ok");
        assert_eq!(result["total"], 0);
        assert_eq!(result["in_progress"].as_array().unwrap().len(), 0);
        assert_eq!(result["issues"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn analyze_separates_in_progress() {
        let issues = vec![
            make_issue(1, "Active", "", &["Flow In-Progress"], &now_iso()),
            make_issue(2, "Available", "", &[], &now_iso()),
        ];
        let result = analyze_issues(&issues, &HashMap::new());
        assert_eq!(result["in_progress"].as_array().unwrap().len(), 1);
        assert_eq!(result["in_progress"][0]["number"], 1);
        assert_eq!(result["issues"].as_array().unwrap().len(), 1);
        assert_eq!(result["issues"][0]["number"], 2);
    }

    #[test]
    fn analyze_issue_fields() {
        let issues = vec![make_issue(
            1,
            "Test",
            "Check lib/foo.py",
            &["decomposed"],
            &now_iso(),
        )];
        let result = analyze_issues(&issues, &HashMap::new());
        let issue = &result["issues"][0];
        assert_eq!(issue["number"], 1);
        assert_eq!(issue["title"], "Test");
        assert!(issue.get("url").is_some());
        assert!(issue["decomposed"].as_bool().unwrap());
        assert!(issue.get("age_days").is_some());
        assert!(issue.get("file_paths").is_some());
        assert!(issue.get("blocked").is_some());
        assert!(issue.get("brief").is_some());
        assert!(issue.get("category").is_some());
        assert!(issue.get("stale").is_some());
        assert!(issue.get("stale_missing").is_some());
    }

    #[test]
    fn analyze_blocked_label() {
        let issues = vec![
            make_issue(1, "Ready issue", "", &[], &now_iso()),
            make_issue(2, "Blocked issue", "", &["Blocked"], &now_iso()),
        ];
        let result = analyze_issues(&issues, &HashMap::new());
        let arr = result["issues"].as_array().unwrap();
        let issue_1 = arr.iter().find(|i| i["number"] == 1).unwrap();
        let issue_2 = arr.iter().find(|i| i["number"] == 2).unwrap();
        assert!(!issue_1["blocked"].as_bool().unwrap());
        assert!(issue_2["blocked"].as_bool().unwrap());
        // Label-only blocked: native_blocked is false, blocked_by is empty
        assert!(!issue_2["native_blocked"].as_bool().unwrap());
        assert!(issue_2["blocked_by"].as_array().unwrap().is_empty());
    }

    #[test]
    fn analyze_total_includes_all() {
        let issues = vec![
            make_issue(1, "A", "", &["Flow In-Progress"], &now_iso()),
            make_issue(2, "B", "", &[], &now_iso()),
            make_issue(3, "C", "", &[], &now_iso()),
        ];
        let result = analyze_issues(&issues, &HashMap::new());
        assert_eq!(result["total"], 3);
    }

    #[test]
    fn analyze_stale_detection() {
        let old_date = (chrono::Utc::now() - chrono::Duration::days(90)).to_rfc3339();
        let issues = vec![make_issue(
            1,
            "Old issue",
            "Check /nonexistent/gone.py",
            &[],
            &old_date,
        )];
        let result = analyze_issues(&issues, &HashMap::new());
        let issue = &result["issues"][0];
        assert!(issue["stale"].as_bool().unwrap());
        assert!(issue["stale_missing"].as_i64().unwrap() >= 1);
    }

    #[test]
    fn analyze_native_blocked_without_label() {
        let issues = vec![make_issue(10, "Has native blocker", "", &[], &now_iso())];
        let mut blocker_map: HashMap<i64, Vec<i64>> = HashMap::new();
        blocker_map.insert(10, vec![100, 200]);
        let result = analyze_issues(&issues, &blocker_map);
        let issue = &result["issues"][0];
        assert!(issue["blocked"].as_bool().unwrap());
        assert!(issue["native_blocked"].as_bool().unwrap());
        let blocked_by: Vec<i64> = issue["blocked_by"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect();
        assert_eq!(blocked_by, vec![100, 200]);
    }

    #[test]
    fn analyze_no_blocker_counts_default() {
        let issues = vec![make_issue(10, "No counts", "", &[], &now_iso())];
        let result = analyze_issues(&issues, &HashMap::new());
        let issue = &result["issues"][0];
        assert!(!issue["blocked"].as_bool().unwrap());
        assert!(!issue["native_blocked"].as_bool().unwrap());
        assert!(issue["blocked_by"].as_array().unwrap().is_empty());
    }

    // --- filter_issues ---

    #[test]
    fn filter_ready_returns_not_blocked() {
        let issues = vec![
            serde_json::json!({"number": 1, "blocked": false, "decomposed": false}),
            serde_json::json!({"number": 2, "blocked": true, "decomposed": false}),
            serde_json::json!({"number": 3, "blocked": false, "decomposed": true}),
        ];
        let result = filter_issues(&issues, "ready").unwrap();
        let numbers: Vec<i64> = result
            .iter()
            .map(|i| i["number"].as_i64().unwrap())
            .collect();
        assert_eq!(numbers, vec![1, 3]);
    }

    #[test]
    fn filter_blocked_returns_blocked() {
        let issues = vec![
            serde_json::json!({"number": 1, "blocked": false, "decomposed": false}),
            serde_json::json!({"number": 2, "blocked": true, "decomposed": false}),
            serde_json::json!({"number": 3, "blocked": true, "decomposed": true}),
        ];
        let result = filter_issues(&issues, "blocked").unwrap();
        let numbers: Vec<i64> = result
            .iter()
            .map(|i| i["number"].as_i64().unwrap())
            .collect();
        assert_eq!(numbers, vec![2, 3]);
    }

    #[test]
    fn filter_decomposed_returns_decomposed() {
        let issues = vec![
            serde_json::json!({"number": 1, "blocked": false, "decomposed": false}),
            serde_json::json!({"number": 2, "blocked": true, "decomposed": true}),
            serde_json::json!({"number": 3, "blocked": false, "decomposed": true}),
        ];
        let result = filter_issues(&issues, "decomposed").unwrap();
        let numbers: Vec<i64> = result
            .iter()
            .map(|i| i["number"].as_i64().unwrap())
            .collect();
        assert_eq!(numbers, vec![2, 3]);
    }

    #[test]
    fn filter_quick_start() {
        let issues = vec![
            serde_json::json!({"number": 1, "blocked": false, "decomposed": false}),
            serde_json::json!({"number": 2, "blocked": true, "decomposed": true}),
            serde_json::json!({"number": 3, "blocked": false, "decomposed": true}),
        ];
        let result = filter_issues(&issues, "quick-start").unwrap();
        let numbers: Vec<i64> = result
            .iter()
            .map(|i| i["number"].as_i64().unwrap())
            .collect();
        assert_eq!(numbers, vec![3]);
    }

    #[test]
    fn filter_unknown_raises() {
        let result = filter_issues(&[], "invalid");
        assert!(result.is_err());
    }

    // --- CLI (run via Args) ---

    /// Helper: write issues JSON to a temp file and run analyze via Args.
    /// Returns (exit_code, stdout). Uses a subprocess to capture exit behavior.
    fn run_with_file(issues_json: &str, extra_args: &[&str]) -> (i32, String) {
        let dir = tempfile::tempdir().unwrap();
        let json_file = dir.path().join("issues.json");
        std::fs::write(&json_file, issues_json).unwrap();

        let bin = std::env::current_exe().unwrap();
        // Find the flow-rs binary in the same target directory
        let target_dir = bin.parent().unwrap().parent().unwrap();
        let flow_rs = target_dir.join("flow-rs");

        let mut cmd_args = vec![
            "analyze-issues".to_string(),
            "--issues-json".to_string(),
            json_file.to_str().unwrap().to_string(),
        ];
        for arg in extra_args {
            cmd_args.push(arg.to_string());
        }

        let output = std::process::Command::new(&flow_rs)
            .args(&cmd_args)
            .output()
            .expect("Failed to run flow-rs");

        let code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        (code, stdout)
    }

    #[test]
    fn cli_with_issues_json_file() {
        let issues = serde_json::to_string(&vec![make_issue(
            1,
            "Test issue",
            "Check lib/foo.py",
            &[],
            &now_iso(),
        )])
        .unwrap();
        let (code, stdout) = run_with_file(&issues, &[]);
        assert_eq!(code, 0, "stdout: {}", stdout);
        let output: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(output["status"], "ok");
        assert_eq!(output["total"], 1);
    }

    #[test]
    fn cli_empty_json_file() {
        let (code, stdout) = run_with_file("[]", &[]);
        assert_eq!(code, 0, "stdout: {}", stdout);
        let output: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(output["total"], 0);
    }

    #[test]
    fn cli_malformed_json() {
        let (code, stdout) = run_with_file("{corrupt", &[]);
        assert_eq!(code, 1);
        let output: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(output["status"], "error");
    }

    #[test]
    fn cli_ready_flag() {
        let issues = serde_json::to_string(&vec![
            make_issue(1, "Ready", "", &[], &now_iso()),
            make_issue(2, "Blocked", "", &["Blocked"], &now_iso()),
        ])
        .unwrap();
        let (code, stdout) = run_with_file(&issues, &["--ready"]);
        assert_eq!(code, 0, "stdout: {}", stdout);
        let output: Value = serde_json::from_str(&stdout).unwrap();
        let numbers: Vec<i64> = output["issues"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["number"].as_i64().unwrap())
            .collect();
        assert!(numbers.contains(&1));
        assert!(!numbers.contains(&2));
    }

    #[test]
    fn cli_missing_file() {
        let bin = std::env::current_exe().unwrap();
        let target_dir = bin.parent().unwrap().parent().unwrap();
        let flow_rs = target_dir.join("flow-rs");

        let output = std::process::Command::new(&flow_rs)
            .args(["analyze-issues", "--issues-json", "/nonexistent/file.json"])
            .output()
            .expect("Failed to run flow-rs");

        assert_eq!(output.status.code().unwrap_or(-1), 1);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let parsed: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(parsed["status"], "error");
    }
}
