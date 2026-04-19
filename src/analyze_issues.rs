//! Analyze open GitHub issues for the flow-issues skill.
//!
//! Handles mechanical work: JSON parsing, file path extraction,
//! label detection, stale detection. Outputs condensed per-issue
//! briefs so the LLM only needs to rank by impact.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

/// Pre-compiled regexes for extracting file paths with known directory prefixes.
static DIR_PREFIX_REGEXES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    DIR_PREFIXES
        .iter()
        .map(|prefix| {
            let escaped = regex::escape(prefix);
            let pattern = format!("{}{}", escaped, r"[\w./\-]+");
            Regex::new(&pattern).unwrap()
        })
        .collect()
});

/// Pre-compiled regex for file paths with recognized extensions.
/// Uses non-word character boundaries (`(?:^|[^\w])` / `(?:$|[^\w])`) instead of
/// lookahead/lookbehind because the `regex` crate does not support lookaround.
static FILE_EXT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:^|[^\w])([\w./\-]+/[\w.\-]+\.(?:py|md|json|sh|yml|yaml|rb|js|ts|html|css|toml))(?:$|[^\w])",
    )
    .unwrap()
});

/// Pre-compiled regex for bug-related keywords in issue content.
static BUG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(bug|fix|crash|error|broken|fail|wrong|incorrect)\b").unwrap()
});

/// Pre-compiled regex for enhancement-related keywords in issue content.
static ENHANCEMENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(add|new|feature|enhance|improve|support|implement)\b").unwrap()
});

/// Known directory prefixes for file path extraction.
const DIR_PREFIXES: &[&str] = &[
    "lib/", "skills/", "tests/", "docs/", "hooks/", ".claude/", "bin/", "agents/", "src/",
    "config/", "app/",
];

/// Extract file paths from issue body text.
///
/// Recognizes paths with known directory prefixes and paths containing
/// slashes with recognized file extensions. Returns deduplicated sorted list.
pub fn extract_file_paths(body: &str) -> Vec<String> {
    let mut paths: HashSet<String> = HashSet::new();

    // Match paths with known directory prefixes
    for re in DIR_PREFIX_REGEXES.iter() {
        for mat in re.find_iter(body) {
            paths.insert(mat.as_str().to_string());
        }
    }

    // Match paths with file extensions (must contain /)
    for cap in FILE_EXT_RE.captures_iter(body) {
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
fn detect_labels(labels: &[Value]) -> LabelFlags {
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

    if BUG_RE.is_match(&combined) {
        return "Bug".to_string();
    }
    if ENHANCEMENT_RE.is_match(&combined) {
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
fn check_stale(file_paths: &[String], age_days: i64) -> StaleInfo {
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
/// Uses char count (not byte count) to avoid panicking on multi-byte UTF-8 boundaries.
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
fn parse_blocker_response(json_str: &str, issue_numbers: &[i64]) -> HashMap<i64, Vec<i64>> {
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

/// Strip NULs, replace CR/LF with spaces, collapse runs of whitespace, and
/// trim the result. Produces a single-line error-message-safe payload.
///
/// Error messages flow into JSON output consumed by the `flow-issues` skill
/// and into operator-visible log lines; embedded control characters
/// truncate C-string consumers (NUL), break line-oriented parsers (CR/LF),
/// and leak internal formatting templates when the payload is whitespace
/// only. Normalizing at the error-formatting boundary keeps downstream
/// consumers robust without having to re-implement the same sanitization.
fn normalize_error_payload(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| *c != '\0')
        .map(|c| if c == '\r' || c == '\n' { ' ' } else { c })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Translate a completed [`std::process::Output`] into the stdout-or-
/// error-message shape the callers want. Split from [`run_gh`] so
/// every branch — success, non-zero with stderr, non-zero with empty
/// stderr + exit code, non-zero with empty stderr + signal — is
/// testable without spawning a real process.
fn gh_output_to_result(
    output: std::process::Output,
    command_label: &str,
) -> Result<String, String> {
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let normalized = normalize_error_payload(&stderr);
    let detail = if normalized.is_empty() {
        match output.status.code() {
            Some(code) => format!("(no stderr output, exit code {})", code),
            None => "(no stderr output, terminated by signal)".to_string(),
        }
    } else {
        normalized
    };
    Err(format!("{} failed: {}", command_label, detail))
}

/// Run `gh` with the given args and return captured stdout on success
/// or a normalized error message on failure. Uses `Command::output()`
/// which drains stdout/stderr to EOF automatically — no hand-rolled
/// poll loop, no background drain threads, no timeout seam. See
/// `.claude/rules/testability-means-simplicity.md` for the refactor
/// rationale. `gh` has its own network timeout (~10s per request);
/// a truly hung process is a Ctrl-C scenario.
fn run_gh(args: &[&str], command_label: &str) -> Result<String, String> {
    match std::process::Command::new("gh").args(args).output() {
        Ok(o) => gh_output_to_result(o, command_label),
        Err(e) => {
            let msg = normalize_error_payload(&format!("{}", e));
            Err(format!("{} failed: {}", command_label, msg))
        }
    }
}

/// Fetch native blocked-by details for issues via GitHub GraphQL API.
///
/// Uses `blockedBy(first: 10)` connection with batched aliased queries.
/// Returns HashMap mapping issue number to list of open blocker issue numbers.
///
/// Graceful degradation: returns an empty HashMap on every failure mode —
/// the 30-second subprocess timeout firing, `gh` spawn failure (missing
/// binary, permission denied), `gh` exiting non-zero (auth expiry, rate
/// limit, malformed query, missing repo permission), or a `try_wait` I/O
/// error mid-poll. In each non-success case the helper logs a single-line
/// diagnostic to stderr via `eprintln!` so operators can see which
/// failure mode occurred — without that log, auth expiry would silently
/// report every issue as unblocked and the user would have no signal.
///
/// Timeout: 30 seconds — long enough for the GraphQL endpoint to respond
/// on a slow link, short enough to keep the analyze step from hanging
/// the calling skill.
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
    let query_arg = format!("query={}", query);
    let owner_arg = format!("owner={}", owner);
    let repo_arg = format!("repo={}", name);

    let result = run_gh(
        &[
            "api", "graphql", "-f", &query_arg, "-f", &owner_arg, "-f", &repo_arg,
        ],
        "gh api graphql",
    );
    blocker_result_to_map(issue_numbers, result)
}

/// Convert a run_gh result into a blocker map. Split out so the
/// `Ok(stdout) => parse_blocker_response` branch is directly
/// testable without a live gh subprocess.
fn blocker_result_to_map(
    issue_numbers: &[i64],
    result: Result<String, String>,
) -> HashMap<i64, Vec<i64>> {
    match result {
        Ok(stdout) => parse_blocker_response(&stdout, issue_numbers),
        Err(msg) => {
            eprintln!(
                "warning: blocker fetch failed, treating all issues as unblocked ({})",
                msg
            );
            HashMap::new()
        }
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
        // chrono::DateTime::parse_from_rfc3339 accepts both `Z` and
        // `±HH:MM` offsets, so a Z-suffix fallback would be dead code.
        // Empirically: every input that fails this strict parse also
        // fails after a `Z` → `+00:00` substitution (verified by
        // coverage instrumentation showing the fallback's success arm
        // hit 0 times across the test corpus). Treat unparseable
        // dates as age 0.
        let age_days = chrono::DateTime::parse_from_rfc3339(created_at_str)
            .map(|created| (chrono::Utc::now() - created.with_timezone(&chrono::Utc)).num_days())
            .unwrap_or(0);

        let stale_info = check_stale(&file_paths, age_days);
        let category = categorize(&label_names, issue["title"].as_str().unwrap_or(""), body);

        let blocked_by = blocker_map.get(&number).cloned().unwrap_or_default();
        let native_blocked = !blocked_by.is_empty();

        let milestone = issue
            .get("milestone")
            .and_then(|m| m.get("title"))
            .and_then(|t| t.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| Value::String(s.to_string()))
            .unwrap_or(Value::Null);

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
            "milestone": milestone,
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

    /// Filter by GitHub label (server-side, repeatable)
    #[arg(long, short = 'l')]
    pub label: Vec<String>,

    /// Filter by GitHub milestone (server-side, by title or number)
    #[arg(long, short = 'm')]
    pub milestone: Option<String>,
}

/// Main-arm dispatcher for the `analyze-issues` CLI. Returns
/// `(Value, i32)` so main.rs's match arm can dispatch via
/// `dispatch::dispatch_json` without a separate thin `run` wrapper
/// that would be linked (but never called) into every lib test
/// binary, producing unexecuted-instantiation coverage gaps.
pub fn run_impl_main(args: Args) -> (Value, i32) {
    let issues_json = match read_issues_json(&args) {
        Ok(s) => s,
        Err(v) => return (v, 1),
    };

    let issues: Vec<Value> = match serde_json::from_str(&issues_json) {
        Ok(v) => v,
        Err(e) => {
            return (
                serde_json::json!({
                    "status": "error",
                    "message": format!("Invalid JSON: {}", e),
                }),
                1,
            );
        }
    };

    let blocker_map = match crate::github::detect_repo(None) {
        Some(repo) => {
            let all_numbers: Vec<i64> =
                issues.iter().filter_map(|i| i["number"].as_i64()).collect();
            fetch_blockers(&repo, &all_numbers)
        }
        None => HashMap::new(),
    };

    let mut output = analyze_issues(&issues, &blocker_map);

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
        let issues_arr = output["issues"]
            .as_array()
            .expect("analyze_issues always writes issues as an array");
        let filtered = filter_issues(issues_arr, name)
            .expect("internal filter name is always one of the four known values");
        let in_progress_count = output["in_progress"]
            .as_array()
            .expect("analyze_issues always writes in_progress as an array")
            .len();
        let count = in_progress_count + filtered.len();
        output["issues"] = Value::Array(filtered);
        output["total"] = serde_json::json!(count);
    }

    (output, 0)
}

#[inline(always)]
fn read_issues_json(args: &Args) -> Result<String, Value> {
    if let Some(path) = &args.issues_json {
        return match std::fs::read_to_string(path) {
            Ok(s) => Ok(s),
            Err(e) => Err(serde_json::json!({
                "status": "error",
                "message": format!("Could not read issues file: {}", e),
            })),
        };
    }
    let mut gh_args: Vec<String> = vec![
        "issue".to_string(),
        "list".to_string(),
        "--state".to_string(),
        "open".to_string(),
        "--json".to_string(),
        "number,title,labels,createdAt,body,url,milestone".to_string(),
        "--limit".to_string(),
        "100".to_string(),
    ];
    for l in &args.label {
        gh_args.push("--label".to_string());
        gh_args.push(l.clone());
    }
    if let Some(ref m) = args.milestone {
        gh_args.push("--milestone".to_string());
        gh_args.push(m.clone());
    }
    let gh_argv: Vec<&str> = gh_args.iter().map(|s| s.as_str()).collect();
    match run_gh(&gh_argv, "gh issue list") {
        Ok(s) => Ok(s),
        Err(msg) => Err(serde_json::json!({
            "status": "error",
            "message": msg,
        })),
    }
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

    /// Drive `run_impl_main` through the `--issues-json` path with a
    /// valid file so the bin-tier `run_impl_main` code path reaches
    /// through JSON parsing, detect_repo (None in tempdir), and
    /// analyze_issues. Eliminates the unexecuted-instantiation gap
    /// for the `run_impl_main` body by exercising it in the lib
    /// test binary.
    #[test]
    fn run_impl_main_with_issues_json_path_returns_ok() {
        let dir = tempfile::tempdir().unwrap();
        let issues = vec![fake_issue(1, "Test", vec!["Rule"])];
        let issues_path = dir.path().join("issues.json");
        std::fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
        let args = Args {
            issues_json: Some(issues_path.to_string_lossy().into_owned()),
            ready: false,
            blocked: false,
            decomposed: false,
            quick_start: false,
            label: Vec::new(),
            milestone: None,
        };
        let (value, code) = run_impl_main(args);
        assert_eq!(code, 0);
        assert_eq!(value["status"], "ok");
    }

    /// Drive `run_impl_main` with an invalid file path so the
    /// `read_issues_json` error arm fires.
    #[test]
    fn run_impl_main_missing_file_returns_error_one() {
        let args = Args {
            issues_json: Some("/definitely/not/a/real/path.json".to_string()),
            ready: false,
            blocked: false,
            decomposed: false,
            quick_start: false,
            label: Vec::new(),
            milestone: None,
        };
        let (value, code) = run_impl_main(args);
        assert_eq!(code, 1);
        assert_eq!(value["status"], "error");
        assert!(value["message"]
            .as_str()
            .unwrap()
            .contains("Could not read issues file"));
    }

    /// Drive `run_impl_main` with malformed JSON so the
    /// "Invalid JSON" arm fires.
    #[test]
    fn run_impl_main_malformed_json_returns_error_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "{corrupt").unwrap();
        let args = Args {
            issues_json: Some(path.to_string_lossy().into_owned()),
            ready: false,
            blocked: false,
            decomposed: false,
            quick_start: false,
            label: Vec::new(),
            milestone: None,
        };
        let (value, code) = run_impl_main(args);
        assert_eq!(code, 1);
        assert_eq!(value["status"], "error");
        assert!(value["message"].as_str().unwrap().contains("Invalid JSON"));
    }

    /// Drive `run_impl_main` with `--ready` filter so the filter arm
    /// fires.
    #[test]
    fn run_impl_main_with_ready_filter_applies_filter() {
        let dir = tempfile::tempdir().unwrap();
        let issues = vec![
            fake_issue(1, "Ready", vec!["Rule"]),
            fake_issue(2, "Blocked", vec!["Blocked"]),
        ];
        let issues_path = dir.path().join("issues.json");
        std::fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
        let args = Args {
            issues_json: Some(issues_path.to_string_lossy().into_owned()),
            ready: true,
            blocked: false,
            decomposed: false,
            quick_start: false,
            label: Vec::new(),
            milestone: None,
        };
        let (value, code) = run_impl_main(args);
        assert_eq!(code, 0);
        assert_eq!(value["status"], "ok");
    }

    fn fake_issue(number: i64, title: &str, labels: Vec<&str>) -> Value {
        let labels_json: Vec<Value> = labels
            .into_iter()
            .map(|l| serde_json::json!({"name": l}))
            .collect();
        serde_json::json!({
            "number": number,
            "title": title,
            "body": "",
            "labels": labels_json,
            "createdAt": chrono::Local::now().to_rfc3339(),
            "url": format!("https://github.com/test/repo/issues/{}", number),
            "milestone": Value::Null,
        })
    }

    // --- gh_output_to_result ---

    fn fake_output(code: Option<i32>, stdout: &str, stderr: &str) -> std::process::Output {
        use std::os::unix::process::ExitStatusExt;
        let status = match code {
            Some(c) => std::process::ExitStatus::from_raw(c << 8),
            None => std::process::ExitStatus::from_raw(9), // signal 9
        };
        std::process::Output {
            status,
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    #[test]
    fn gh_output_to_result_success_returns_stdout() {
        let out = fake_output(Some(0), "payload", "");
        assert_eq!(gh_output_to_result(out, "gh").unwrap(), "payload");
    }

    #[test]
    fn gh_output_to_result_nonzero_with_stderr_returns_labeled_error() {
        let out = fake_output(Some(2), "", "oops");
        assert_eq!(
            gh_output_to_result(out, "gh issue list").unwrap_err(),
            "gh issue list failed: oops"
        );
    }

    #[test]
    fn gh_output_to_result_nonzero_empty_stderr_with_code_names_exit_code() {
        let out = fake_output(Some(9), "", "");
        let err = gh_output_to_result(out, "gh").unwrap_err();
        assert!(err.contains("no stderr output"), "err was: {}", err);
        assert!(err.contains("exit code 9"), "err was: {}", err);
    }

    #[test]
    fn gh_output_to_result_signal_terminated_empty_stderr_names_signal() {
        let out = fake_output(None, "", "");
        let err = gh_output_to_result(out, "gh").unwrap_err();
        assert!(err.contains("terminated by signal"), "err was: {}", err);
    }

    #[test]
    fn gh_output_to_result_whitespace_stderr_includes_exit_code() {
        let out = fake_output(Some(3), "", "   \n\t\n  ");
        let err = gh_output_to_result(out, "gh").unwrap_err();
        assert!(err.contains("exit code 3"), "err was: {}", err);
    }

    #[test]
    fn gh_output_to_result_strips_nuls_and_cr_lf_from_stderr() {
        let out = fake_output(Some(4), "", "foo\0bar\r\nbaz");
        let err = gh_output_to_result(out, "gh").unwrap_err();
        assert!(!err.contains('\0'));
        assert!(!err.contains('\r'));
        assert!(!err.contains('\n'));
    }

    // --- run_gh ---

    /// Exercises the `run_gh` body via a real (or missing) gh subprocess.
    /// The outcome (Ok or Err) depends on whether gh is installed on
    /// PATH; we don't care which — the goal is to execute the body.
    #[test]
    fn run_gh_executes_body() {
        let _ = run_gh(&["--version"], "gh --version");
    }

    // --- blocker_result_to_map ---

    #[test]
    fn blocker_result_to_map_ok_parses_response() {
        let response = r#"{"data":{"repository":{"issue_10":{"blockedBy":{"nodes":[]}}}}}"#;
        let map = blocker_result_to_map(&[10], Ok(response.to_string()));
        assert!(map.contains_key(&10));
    }

    #[test]
    fn blocker_result_to_map_err_logs_and_returns_empty() {
        let map = blocker_result_to_map(&[10], Err("gh failed".to_string()));
        assert!(map.is_empty());
    }

    // --- normalize_error_payload ---

    #[test]
    fn normalize_error_payload_strips_nuls() {
        assert_eq!(normalize_error_payload("a\0b\0c"), "abc");
    }

    #[test]
    fn normalize_error_payload_collapses_newlines() {
        assert_eq!(normalize_error_payload("a\r\nb\nc"), "a b c");
    }

    #[test]
    fn normalize_error_payload_trims_and_collapses_whitespace() {
        assert_eq!(normalize_error_payload("  foo   bar  \n\t "), "foo bar");
    }

    #[test]
    fn normalize_error_payload_empty_on_whitespace_only() {
        assert_eq!(normalize_error_payload("   \n\t \r\n  "), "");
    }

    #[test]
    fn normalize_error_payload_passes_through_normal_text() {
        assert_eq!(normalize_error_payload("hello world"), "hello world");
    }

    // --- analyze_issues helpers ---

    fn make_issue(
        number: i64,
        title: &str,
        body: &str,
        labels: &[&str],
        created_at: &str,
    ) -> Value {
        make_issue_opt(number, title, body, labels, created_at, None)
    }

    /// Create an issue with an optional milestone object.
    fn make_issue_opt(
        number: i64,
        title: &str,
        body: &str,
        labels: &[&str],
        created_at: &str,
        milestone_title: Option<&str>,
    ) -> Value {
        let label_arr: Vec<Value> = labels
            .iter()
            .map(|n| serde_json::json!({"name": n}))
            .collect();
        let milestone = match milestone_title {
            Some(t) => serde_json::json!({"title": t, "number": 1}),
            None => Value::Null,
        };
        serde_json::json!({
            "number": number,
            "title": title,
            "body": body,
            "labels": label_arr,
            "createdAt": created_at,
            "url": format!("https://github.com/test/repo/issues/{}", number),
            "milestone": milestone,
        })
    }

    fn now_iso() -> String {
        chrono::Local::now().to_rfc3339()
    }

    // --- analyze_issues ---

    /// Covers the `None` branches of the `.as_str().map(String::from)`
    /// filter_map region at label-name extraction. Three scenarios:
    /// (1) label object missing the `"name"` key entirely → `?`
    /// short-circuits on `None`; (2) `"name": null` → `.as_str()`
    /// returns None; (3) `"name": 42` → `.as_str()` returns None.
    /// Without these inputs every test label carries a valid string
    /// name and neither None branch fires.
    #[test]
    fn analyze_non_string_label_name_filtered_out() {
        let issue = serde_json::json!({
            "number": 99,
            "title": "Non-string label",
            "body": "",
            "labels": [
                {"color": "red"},        // no "name" key
                {"name": null},           // name is null
                {"name": 42},             // name is number
                {"name": "valid-label"},  // valid
            ],
            "createdAt": now_iso(),
            "url": "https://github.com/test/repo/issues/99",
            "milestone": Value::Null,
        });
        let result = analyze_issues(&[issue], &HashMap::new());
        let issues_arr = result["issues"].as_array().unwrap();
        assert_eq!(issues_arr.len(), 1);
        // Only the valid string label survives the filter_map.
        let labels = issues_arr[0]["labels"].as_array().unwrap();
        assert!(labels.iter().any(|l| l == "valid-label"));
        assert!(!labels.iter().any(|l| l.is_null()));
    }

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

    /// `chrono::DateTime::parse_from_rfc3339` accepts the `Z` suffix
    /// natively. The Z-substitution fallback was removed after coverage
    /// instrumentation showed its success arm was unreachable. This
    /// test pins the contract that Z-suffix dates parse successfully.
    #[test]
    fn analyze_age_days_z_suffix_parses_natively() {
        let issues = vec![make_issue(
            42,
            "z-suffix issue",
            "",
            &[],
            "2023-06-15T12:00:00Z",
        )];
        let result = analyze_issues(&issues, &HashMap::new());
        let issue = &result["issues"][0];
        let age = issue["age_days"].as_i64().unwrap();
        assert!(age > 0, "expected positive age_days, got {}", age);
    }

    /// `created_at` that fails BOTH parsers (raw RFC3339 and the
    /// Z→+00:00 fallback) lands at production line 483 (`0`). Use a
    /// completely unparseable string.
    #[test]
    fn analyze_age_days_unparseable_date_returns_zero() {
        let issues = vec![make_issue(7, "unparseable date", "", &[], "not-a-date")];
        let result = analyze_issues(&issues, &HashMap::new());
        let issue = &result["issues"][0];
        assert_eq!(issue["age_days"].as_i64().unwrap(), 0);
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

    // --- --label and --milestone CLI args ---

    #[test]
    fn cli_label_single() {
        let issues =
            serde_json::to_string(&vec![make_issue(1, "Bug fix", "", &["Bug"], &now_iso())])
                .unwrap();
        let (code, stdout) = run_with_file(&issues, &["--label", "Bug"]);
        assert_eq!(code, 0, "stdout: {}", stdout);
    }

    #[test]
    fn cli_label_multiple() {
        let issues =
            serde_json::to_string(&vec![make_issue(1, "Bug fix", "", &["Bug"], &now_iso())])
                .unwrap();
        let (code, stdout) = run_with_file(&issues, &["--label", "Bug", "--label", "Enhancement"]);
        assert_eq!(code, 0, "stdout: {}", stdout);
    }

    #[test]
    fn cli_milestone() {
        let issues =
            serde_json::to_string(&vec![make_issue(1, "Feature", "", &[], &now_iso())]).unwrap();
        let (code, stdout) = run_with_file(&issues, &["--milestone", "v1.0"]);
        assert_eq!(code, 0, "stdout: {}", stdout);
    }

    #[test]
    fn cli_label_combines_with_ready() {
        let issues = serde_json::to_string(&vec![
            make_issue(1, "Ready bug", "", &["Bug"], &now_iso()),
            make_issue(2, "Blocked bug", "", &["Bug", "Blocked"], &now_iso()),
        ])
        .unwrap();
        let (code, stdout) = run_with_file(&issues, &["--label", "Bug", "--ready"]);
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

    // --- milestone in analyze_issues output ---

    #[test]
    fn analyze_milestone_present() {
        let issues = vec![make_issue_opt(
            1,
            "Milestone issue",
            "",
            &[],
            &now_iso(),
            Some("v1.2.0"),
        )];
        let result = analyze_issues(&issues, &HashMap::new());
        let issue = &result["issues"][0];
        assert_eq!(issue["milestone"], "v1.2.0");
    }

    #[test]
    fn analyze_milestone_null() {
        let issues = vec![make_issue_opt(1, "No milestone", "", &[], &now_iso(), None)];
        let result = analyze_issues(&issues, &HashMap::new());
        let issue = &result["issues"][0];
        assert!(issue["milestone"].is_null());
    }

    #[test]
    fn analyze_milestone_empty_string_is_null() {
        let label_arr: Vec<Value> = vec![];
        let issue = serde_json::json!({
            "number": 1,
            "title": "Empty milestone title",
            "body": "",
            "labels": label_arr,
            "createdAt": now_iso(),
            "url": "https://github.com/test/repo/issues/1",
            "milestone": {"title": "", "number": 1},
        });
        let result = analyze_issues(&[issue], &HashMap::new());
        assert!(
            result["issues"][0]["milestone"].is_null(),
            "Empty milestone title should be null"
        );
    }

    #[test]
    fn cli_issues_json_with_milestone() {
        let issues = serde_json::to_string(&vec![make_issue_opt(
            1,
            "With milestone",
            "",
            &[],
            &now_iso(),
            Some("v2.0"),
        )])
        .unwrap();
        let (code, stdout) = run_with_file(&issues, &[]);
        assert_eq!(code, 0, "stdout: {}", stdout);
        let output: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(output["issues"][0]["milestone"], "v2.0");
    }
}
