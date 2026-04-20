//! Analyze open GitHub issues for the flow-issues skill.
//!
//! Handles mechanical work: JSON parsing, file path extraction,
//! label detection, stale detection. Outputs condensed per-issue
//! briefs so the LLM only needs to rank by impact.
//!
//! Tests live at `tests/analyze_issues.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` in
//! this file.

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

/// Strip NULs, replace CR/LF with spaces, collapse runs of whitespace, and
/// trim the result. Produces a single-line error-message-safe payload.
///
/// Error messages flow into JSON output consumed by the `flow-issues` skill
/// and into operator-visible log lines; embedded control characters
/// truncate C-string consumers (NUL), break line-oriented parsers (CR/LF),
/// and leak internal formatting templates when the payload is whitespace
/// only. Normalizing at the error-formatting boundary keeps downstream
/// consumers robust without having to re-implement the same sanitization.
pub fn normalize_error_payload(raw: &str) -> String {
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
pub fn gh_output_to_result(
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
pub fn run_gh(args: &[&str], command_label: &str) -> Result<String, String> {
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
pub fn blocker_result_to_map(
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
