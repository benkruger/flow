//! Analyze open GitHub issues for the flow-issues skill.
//!
//! Handles mechanical work: JSON parsing, file path extraction,
//! label detection, stale detection. Outputs condensed per-issue
//! briefs so the LLM only needs to rank by impact.

use std::collections::HashSet;
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
        decomposed: label_names.iter().any(|n| n.eq_ignore_ascii_case("decomposed")),
        blocked: label_names.iter().any(|n| n.eq_ignore_ascii_case("blocked")),
    }
}

/// Label categories checked in order.
const LABEL_CATEGORIES: &[&str] = &["Rule", "Flow", "Flaky Test", "Tech Debt", "Documentation Drift"];

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

    let missing = file_paths.iter().filter(|fp| !Path::new(fp).exists()).count();
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
        assert_eq!(categorize(&labels, "Fix crash on login", "error when"), "Bug");
    }

    #[test]
    fn categorize_enhancement_by_content() {
        let labels: HashSet<String> = HashSet::new();
        assert_eq!(categorize(&labels, "Add dark mode", "new feature"), "Enhancement");
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
}
