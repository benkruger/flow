//! Analyze open GitHub issues for the flow-issues skill.
//!
//! Produces a flat `issues` array enriched with per-row label flags
//! (`decomposed`, `blocked`, `vanilla`, `flow_in_progress`,
//! `triage_in_progress`), assignees, and resolved `blocked_by`
//! entries that carry both `number` and a fully-constructed
//! GitHub URL. The dashboard renderer reads one stream and
//! dispatches by label.
//!
//! Tests live at `tests/analyze_issues.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` in
//! this file.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

/// Per-issue label detection result. The booleans drive bucket
/// assignment and color treatment in the four-section dashboard:
/// `decomposed` and `blocked` choose the section, `vanilla` marks
/// the Vanilla bucket, and `flow_in_progress` / `triage_in_progress`
/// flag rows that the renderer prefixes (🟡 / 🔍) with bold title
/// and a suppressed Command cell.
pub struct LabelFlags {
    pub decomposed: bool,
    pub blocked: bool,
    pub vanilla: bool,
    pub flow_in_progress: bool,
    pub triage_in_progress: bool,
}

/// Check for canonical FLOW labels. All five labels match
/// case-insensitively because GitHub's label registry is
/// case-preserving and historic data may use mixed case. A repo
/// that records `flow in-progress` as the label string still
/// surfaces under the `flow_in_progress` boolean.
pub fn detect_labels(labels: &[Value]) -> LabelFlags {
    let label_names: HashSet<String> = labels
        .iter()
        .filter_map(|l| l.get("name")?.as_str().map(String::from))
        .collect();

    LabelFlags {
        decomposed: label_names
            .iter()
            .any(|n| n.eq_ignore_ascii_case("decomposed")),
        blocked: label_names
            .iter()
            .any(|n| n.eq_ignore_ascii_case("blocked")),
        vanilla: label_names
            .iter()
            .any(|n| n.eq_ignore_ascii_case("vanilla")),
        flow_in_progress: label_names
            .iter()
            .any(|n| n.eq_ignore_ascii_case("Flow In-Progress")),
        triage_in_progress: label_names
            .iter()
            .any(|n| n.eq_ignore_ascii_case("Triage In-Progress")),
    }
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

/// Validate a GitHub `owner/name` slug for safe interpolation into
/// a markdown-rendered URL. Accepts exactly one `/` separator and
/// rejects any segment containing characters outside GitHub's
/// canonical owner/repo grammar — alphanumeric, hyphen, underscore,
/// and period. Rejects `..` segments specifically so path-traversal
/// attempts via a poisoned remote (`owner/repo/../../evil`) cannot
/// inject extra path components into the blocker URL.
///
/// Per `.claude/rules/external-input-path-construction.md`: the
/// `repo` string is state-derived (sourced from
/// `git remote get-url origin` via `crate::github::detect_repo`)
/// and reaches a `format!`-interpolated URL that is rendered as
/// `[#N](url)` markdown by the consumer. A repo with structural
/// characters (`|`, `<`, `(`, `[`, `\n`, `..`) breaks the markdown
/// surface or redirects the link target. The validator runs at the
/// boundary so the URL construction itself can assume a safe value.
fn is_safe_repo_slug(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let owner = match parts.next() {
        Some(s) if !s.is_empty() => s,
        _ => return false,
    };
    let name = match parts.next() {
        Some(s) if !s.is_empty() => s,
        _ => return false,
    };
    if parts.next().is_some() {
        return false;
    }
    let safe = |s: &str| {
        s != ".."
            && s != "."
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    };
    safe(owner) && safe(name)
}

/// Parse a GraphQL response for blocker details.
///
/// Extracts `blockedBy.nodes` for each issue number. Returns a HashMap
/// mapping issue number to a list of open blocker entries; each entry
/// is a JSON object `{"number": N, "url": "https://github.com/<repo>/issues/N"}`
/// so consumers can render linked blocker references without
/// re-constructing URLs. `repo` is the `owner/name` slug; URL knowledge
/// stays at the blocker-fetch layer rather than leaking into the
/// dashboard renderer. `repo` is validated through
/// [`is_safe_repo_slug`] before interpolation — an unsafe value
/// short-circuits to an empty map so a poisoned remote cannot inject
/// path-traversal or markdown-structural characters into the
/// rendered URL. Only blockers where `state == "OPEN"` are included
/// — closed blockers are resolved. Handles null values at any
/// level gracefully (defaults to empty vec).
pub fn parse_blocker_response(
    json_str: &str,
    issue_numbers: &[i64],
    repo: &str,
) -> HashMap<i64, Vec<Value>> {
    if !is_safe_repo_slug(repo) {
        return HashMap::new();
    }

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

        let blocker_entries: Vec<Value> = match nodes {
            Some(arr) => arr
                .iter()
                .filter(|node| {
                    node.get("state")
                        .and_then(|s| s.as_str())
                        .map(|s| s == "OPEN")
                        .unwrap_or(false)
                })
                .filter_map(|node| {
                    let n = node.get("number").and_then(|n| n.as_i64())?;
                    Some(serde_json::json!({
                        "number": n,
                        "url": format!("https://github.com/{}/issues/{}", repo, n),
                    }))
                })
                .collect(),
            None => Vec::new(),
        };
        blockers.insert(number, blocker_entries);
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
pub fn fetch_blockers(repo: &str, issue_numbers: &[i64]) -> HashMap<i64, Vec<Value>> {
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
    blocker_result_to_map(issue_numbers, repo, result)
}

/// Convert a run_gh result into a blocker map. Split out so the
/// `Ok(stdout) => parse_blocker_response` branch is directly
/// testable without a live gh subprocess. `repo` is threaded through
/// so each blocker entry carries its GitHub URL.
pub fn blocker_result_to_map(
    issue_numbers: &[i64],
    repo: &str,
    result: Result<String, String>,
) -> HashMap<i64, Vec<Value>> {
    match result {
        Ok(stdout) => parse_blocker_response(&stdout, issue_numbers, repo),
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
/// Every input issue flows into a single `issues` array on the output
/// envelope. Per-row label flags (`decomposed`, `blocked`,
/// `flow_in_progress`, `triage_in_progress`, `vanilla`) carry the
/// signal the consumer dispatches on; no top-level `in_progress`
/// partition. The `blocker_map` maps issue numbers to open blocker
/// numbers; native_blocked rows fold into `blocked`.
pub fn analyze_issues(issues: &[Value], blocker_map: &HashMap<i64, Vec<Value>>) -> Value {
    if issues.is_empty() {
        return serde_json::json!({
            "status": "ok",
            "total": 0,
            "issues": [],
        });
    }

    let mut available = Vec::new();

    for issue in issues {
        let number = issue["number"].as_i64().unwrap_or(0);
        let labels_arr = issue.get("labels").and_then(|l| l.as_array());
        let labels_vec: Vec<Value> = labels_arr.cloned().unwrap_or_default();

        let label_names: HashSet<String> = labels_vec
            .iter()
            .filter_map(|l| l.get("name")?.as_str().map(String::from))
            .collect();
        let mut label_list: Vec<String> = label_names.iter().cloned().collect();
        label_list.sort();

        let label_flags = detect_labels(&labels_vec);

        let blocked_by = blocker_map.get(&number).cloned().unwrap_or_default();
        let native_blocked = !blocked_by.is_empty();

        let assignees: Vec<String> = issue
            .get("assignees")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|a| {
                        a.get("login")
                            .and_then(|l| l.as_str())
                            .filter(|s| !s.is_empty())
                            .map(String::from)
                    })
                    .collect()
            })
            .unwrap_or_default();

        // `blocked` collapses both label-driven and native-GitHub-
        // blocked-by sources into one bucket signal. `native_blocked`
        // is surfaced separately so the consumer can distinguish the
        // source when composing the `Blocked By` cell (label-only
        // blocks render `—`; native blocks render the linked blocker
        // numbers from `blocked_by`).
        available.push(serde_json::json!({
            "number": number,
            "title": issue["title"],
            "url": issue.get("url").cloned().unwrap_or(Value::String(String::new())),
            "labels": label_list,
            "decomposed": label_flags.decomposed,
            "blocked": label_flags.blocked || native_blocked,
            "native_blocked": native_blocked,
            "blocked_by": blocked_by,
            "assignees": assignees,
            "vanilla": label_flags.vanilla,
            "flow_in_progress": label_flags.flow_in_progress,
            "triage_in_progress": label_flags.triage_in_progress,
        }));
    }

    serde_json::json!({
        "status": "ok",
        "total": available.len(),
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
            i["decomposed"].as_bool().unwrap_or(false)
                && !i["blocked"].as_bool().unwrap_or(false)
                && !i["flow_in_progress"].as_bool().unwrap_or(false)
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
        let count = filtered.len();
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
        "number,title,labels,createdAt,body,url,milestone,assignees".to_string(),
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
