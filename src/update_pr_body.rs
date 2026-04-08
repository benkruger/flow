use std::path::Path;

use clap::{ArgGroup, Parser};
use regex::Regex;
use serde_json::json;

use crate::output::{json_error, json_ok};

/// Build a markdown artifact line: - **Label**: `value`.
pub fn build_artifact_line(label: &str, value: &str) -> String {
    format!("- **{}**: `{}`", label, value)
}

/// Insert ## Artifacts section after ## What paragraph if not present.
pub fn ensure_artifacts_section(body: &str) -> String {
    if body.contains("## Artifacts") {
        return body.to_string();
    }

    let re = Regex::new(r"(## What\n\n[^\n]+)").unwrap();
    if let Some(m) = re.find(body) {
        let insert_at = m.end();
        return format!(
            "{}\n\n## Artifacts\n{}",
            &body[..insert_at],
            &body[insert_at..]
        );
    }

    format!("{}\n\n## Artifacts\n", body)
}

/// Add or replace an artifact line in the ## Artifacts section.
pub fn add_artifact_to_body(body: &str, label: &str, value: &str) -> String {
    let body = ensure_artifacts_section(body);
    let new_line = build_artifact_line(label, value);

    let pattern = Regex::new(&format!(r"(?m)^- \*\*{}\*\*:.*$", regex::escape(label))).unwrap();
    if pattern.is_match(&body) {
        return pattern.replace(&body, new_line.as_str()).to_string();
    }

    let artifacts_idx = body.find("## Artifacts").unwrap();
    let section_end = body[artifacts_idx + 1..]
        .find("\n## ")
        .map(|i| i + artifacts_idx + 1)
        .unwrap_or(body.len());

    let body_before = body[..section_end].trim_end_matches('\n');
    let body_after = &body[section_end..];
    format!("{}\n\n{}{}", body_before, new_line, body_after)
}

/// Build a plain markdown section with heading and end sentinel.
pub fn build_plain_section(heading: &str, content: &str) -> String {
    format!("## {}\n\n{}\n\n<!-- end:{} -->", heading, content, heading)
}

/// Append or replace a plain (non-collapsible) section in the body.
pub fn append_plain_section_to_body(body: &str, heading: &str, content: &str) -> String {
    let block = build_plain_section(heading, content);

    let pattern = Regex::new(&format!(
        r"(?s)## {}\n\n.*?<!-- end:{} -->",
        regex::escape(heading),
        regex::escape(heading)
    ))
    .unwrap();
    if pattern.is_match(body) {
        return pattern.replace(body, block.as_str()).to_string();
    }

    format!("{}\n\n{}", body.trim_end_matches('\n'), block)
}

/// Return a backtick fence long enough to safely wrap content.
///
/// Scans for the longest consecutive run of backticks in the content
/// and returns a fence that is at least one backtick longer (minimum 3).
pub fn fence_for_content(content: &str) -> String {
    if !content.contains('`') {
        return "```".to_string();
    }
    let re = Regex::new(r"`+").unwrap();
    let max_len = re
        .find_iter(content)
        .map(|m| m.as_str().len())
        .max()
        .unwrap_or(0);
    "`".repeat(std::cmp::max(3, max_len + 1))
}

/// Build a collapsible details block with heading and fenced code.
pub fn build_details_block(heading: &str, summary: &str, content: &str, fmt: &str) -> String {
    let fence = fence_for_content(content);
    format!(
        "## {}\n\n<details>\n<summary>{}</summary>\n\n{}{}\n{}\n{}\n\n</details>",
        heading, summary, fence, fmt, content, fence
    )
}

/// Append or replace a collapsible section in the body.
pub fn append_section_to_body(
    body: &str,
    heading: &str,
    summary: &str,
    content: &str,
    fmt: &str,
) -> String {
    let block = build_details_block(heading, summary, content, fmt);

    let pattern = Regex::new(&format!(
        r"(?s)## {}\n\n<details>.*?</details>",
        regex::escape(heading)
    ))
    .unwrap();
    if pattern.is_match(body) {
        return pattern.replace(body, block.as_str()).to_string();
    }

    format!("{}\n\n{}", body.trim_end_matches('\n'), block)
}

/// Read current PR body via gh.
pub fn gh_get_body(pr_number: i64) -> Result<String, String> {
    let output = std::process::Command::new("gh")
        .args([
            "pr",
            "view",
            &pr_number.to_string(),
            "--json",
            "body",
            "--jq",
            ".body",
        ])
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Err(if !stderr.is_empty() { stderr } else { stdout });
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end_matches('\n')
        .to_string())
}

/// Write PR body via gh.
pub fn gh_set_body(pr_number: i64, body: &str) -> Result<(), String> {
    let output = std::process::Command::new("gh")
        .args(["pr", "edit", &pr_number.to_string(), "--body", body])
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Err(if !stderr.is_empty() { stderr } else { stdout });
    }

    Ok(())
}

#[derive(Parser, Debug)]
#[command(name = "update-pr-body", about = "Update PR body with artifacts")]
#[command(group(ArgGroup::new("mode").required(true).args(["add_artifact", "append_section"])))]
pub struct Args {
    /// PR number
    #[arg(long = "pr")]
    pub pr: i64,

    /// Add artifact line to ## Artifacts section
    #[arg(long = "add-artifact")]
    pub add_artifact: bool,

    /// Append collapsible details section
    #[arg(long = "append-section")]
    pub append_section: bool,

    /// Artifact label (for --add-artifact, repeatable)
    #[arg(long)]
    pub label: Vec<String>,

    /// Artifact value (for --add-artifact, repeatable)
    #[arg(long)]
    pub value: Vec<String>,

    /// Section heading (for --append-section)
    #[arg(long)]
    pub heading: Option<String>,

    /// Details summary (for --append-section)
    #[arg(long)]
    pub summary: Option<String>,

    /// Path to content file (for --append-section)
    #[arg(long = "content-file")]
    pub content_file: Option<String>,

    /// Code block format (for --append-section)
    #[arg(long = "format", default_value = "text")]
    pub fmt: String,

    /// Render plain section instead of collapsible details
    #[arg(long = "no-collapse")]
    pub no_collapse: bool,
}

pub fn run(args: Args) {
    if args.add_artifact {
        if args.label.len() != args.value.len() {
            json_error(
                &format!(
                    "Mismatched --label/--value count: {} labels, {} values",
                    args.label.len(),
                    args.value.len()
                ),
                &[],
            );
            return;
        }

        let body = match gh_get_body(args.pr) {
            Ok(b) => b,
            Err(e) => {
                json_error(&e, &[]);
                return;
            }
        };

        let mut body = body;
        for (label, value) in args.label.iter().zip(args.value.iter()) {
            body = add_artifact_to_body(&body, label, value);
        }

        if let Err(e) = gh_set_body(args.pr, &body) {
            json_error(&e, &[]);
            return;
        }

        json_ok(&[("action", json!("add_artifact"))]);
    } else {
        // --append-section
        let content_file = match &args.content_file {
            Some(f) => f,
            None => {
                json_error("Missing --content-file", &[]);
                return;
            }
        };

        let path = Path::new(content_file);
        if !path.exists() {
            json_error(&format!("File not found: {}", content_file), &[]);
            return;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                json_error(&format!("Failed to read file: {}", e), &[]);
                return;
            }
        };

        let body = match gh_get_body(args.pr) {
            Ok(b) => b,
            Err(e) => {
                json_error(&e, &[]);
                return;
            }
        };

        let heading = args.heading.as_deref().unwrap_or("");

        let new_body = if args.no_collapse {
            append_plain_section_to_body(&body, heading, &content)
        } else {
            let summary = args.summary.as_deref().unwrap_or("");
            append_section_to_body(&body, heading, summary, &content, &args.fmt)
        };

        if let Err(e) = gh_set_body(args.pr, &new_body) {
            json_error(&e, &[]);
            return;
        }

        json_ok(&[("action", json!("append_section"))]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- build_artifact_line ---

    #[test]
    fn build_artifact_line_returns_formatted_markdown() {
        let result = build_artifact_line("Plan file", "/path/to/plan.md");
        assert_eq!(result, "- **Plan file**: `/path/to/plan.md`");
    }

    // --- ensure_artifacts_section ---

    #[test]
    fn ensure_artifacts_section_inserts_after_what() {
        let body = "## What\n\nFeature Title.";
        let result = ensure_artifacts_section(body);
        assert!(result.contains("## Artifacts"));
        assert!(result.find("## What").unwrap() < result.find("## Artifacts").unwrap());
    }

    #[test]
    fn ensure_artifacts_section_no_what_heading() {
        let body = "Some other content.";
        let result = ensure_artifacts_section(body);
        assert!(result.contains("## Artifacts"));
        assert!(result.starts_with("Some other content."));
    }

    #[test]
    fn ensure_artifacts_section_idempotent() {
        let body = "## What\n\nFeature Title.\n\n## Artifacts\n\n- **Session log**: `/path`";
        let result = ensure_artifacts_section(body);
        assert_eq!(result.matches("## Artifacts").count(), 1);
    }

    // --- add_artifact_to_body ---

    #[test]
    fn add_artifact_to_body_adds_new_line() {
        let body = "## What\n\nFeature Title.\n\n## Artifacts\n";
        let result = add_artifact_to_body(body, "Plan file", "/plans/x.md");
        assert!(result.contains("- **Plan file**: `/plans/x.md`"));
    }

    #[test]
    fn add_artifact_to_body_replaces_existing_same_label() {
        let body = "## What\n\nFeature Title.\n\n## Artifacts\n\n- **Plan file**: `/old/path.md`";
        let result = add_artifact_to_body(body, "Plan file", "/new/path.md");
        assert!(result.contains("- **Plan file**: `/new/path.md`"));
        assert!(!result.contains("/old/path.md"));
        assert_eq!(result.matches("Plan file").count(), 1);
    }

    #[test]
    fn add_artifact_to_body_creates_section_if_missing() {
        let body = "## What\n\nFeature Title.";
        let result = add_artifact_to_body(body, "Session log", "/path/log.jsonl");
        assert!(result.contains("## Artifacts"));
        assert!(result.contains("- **Session log**: `/path/log.jsonl`"));
    }

    #[test]
    fn add_artifact_multiple_pairs() {
        let body = "## What\n\nFeature Title.\n\n## Artifacts\n";
        let body = add_artifact_to_body(body, "Plan file", "/plans/x.md");
        let body = add_artifact_to_body(&body, "Session log", "/logs/y.jsonl");
        assert!(body.contains("- **Plan file**: `/plans/x.md`"));
        assert!(body.contains("- **Session log**: `/logs/y.jsonl`"));
    }

    // --- build_details_block ---

    #[test]
    fn build_details_block_returns_collapsible_html() {
        let result = build_details_block(
            "State File",
            ".flow-states/b.json",
            r#"{"key": "value"}"#,
            "json",
        );
        assert!(result.contains("## State File"));
        assert!(result.contains("<details>"));
        assert!(result.contains("<summary>.flow-states/b.json</summary>"));
        assert!(result.contains("```json"));
        assert!(result.contains(r#"{"key": "value"}"#));
        assert!(result.contains("</details>"));
    }

    #[test]
    fn build_details_block_text_format() {
        let result = build_details_block(
            "Session Log",
            ".flow-states/b.log",
            "line 1\nline 2",
            "text",
        );
        assert!(result.contains("```text"));
        assert!(result.contains("line 1\nline 2"));
    }

    // --- fence_for_content ---

    #[test]
    fn fence_for_content_no_backticks() {
        let result = fence_for_content("plain text without any fences");
        assert_eq!(result, "```");
    }

    #[test]
    fn fence_for_content_triple_backticks() {
        let result = fence_for_content("before\n```python\ncode\n```\nafter");
        assert_eq!(result, "````");
    }

    #[test]
    fn fence_for_content_quad_backticks() {
        let result = fence_for_content("before\n````text\ncontent\n````\nafter");
        assert_eq!(result, "`````");
    }

    #[test]
    fn fence_for_content_mixed_lengths() {
        let result = fence_for_content("```python\ncode\n```\n\n````xml\ndata\n````");
        assert_eq!(result, "`````");
    }

    // --- build_details_block with nested fences ---

    #[test]
    fn build_details_block_nested_fences() {
        let content = "# DAG\n\n```xml\n<dag/>\n```\n\n```python\nprint('hi')\n```";
        let result = build_details_block("DAG Analysis", "dag.md", content, "text");
        let lines: Vec<&str> = result.split('\n').collect();
        let fence_lines: Vec<&&str> = lines.iter().filter(|l| l.starts_with("````")).collect();
        assert_eq!(
            fence_lines.len(),
            2,
            "Expected open and close 4+ backtick fences"
        );
        assert!(result.contains("```xml"));
        assert!(result.contains("```python"));
        assert!(result.starts_with("## DAG Analysis"));
        assert!(result.ends_with("</details>"));
    }

    // --- build_plain_section ---

    #[test]
    fn build_plain_section_returns_heading_and_content() {
        let result = build_plain_section("Phase Timings", "| Phase | Duration |");
        assert!(result.contains("## Phase Timings"));
        assert!(result.contains("| Phase | Duration |"));
        assert!(result.contains("<!-- end:Phase Timings -->"));
        assert!(!result.contains("<details>"));
    }

    // --- append_section_to_body ---

    #[test]
    fn append_section_to_body_appends() {
        let body = "## What\n\nFeature Title.";
        let result = append_section_to_body(
            body,
            "State File",
            ".flow-states/b.json",
            r#"{"k": "v"}"#,
            "json",
        );
        assert!(result.contains(body));
        assert!(result.contains("## State File"));
        assert!(result.contains("<details>"));
    }

    #[test]
    fn append_section_replaces_if_heading_exists() {
        let body = "## What\n\nFeature Title.\n\n## State File\n\n<details>\n<summary>old</summary>\n\n```json\nold content\n```\n\n</details>";
        let result =
            append_section_to_body(body, "State File", "new-summary", "new content", "json");
        assert!(!result.contains("old content"));
        assert!(result.contains("new content"));
        assert_eq!(result.matches("## State File").count(), 1);
    }

    // --- append_plain_section_to_body ---

    #[test]
    fn append_plain_section_appends_to_body() {
        let body = "## What\n\nFeature Title.";
        let result = append_plain_section_to_body(body, "Phase Timings", "| Phase | Duration |");
        assert!(result.contains(body));
        assert!(result.contains("## Phase Timings"));
        assert!(result.contains("<!-- end:Phase Timings -->"));
    }

    #[test]
    fn append_plain_section_replaces_existing() {
        let body = "## What\n\nFeature Title.\n\n## Phase Timings\n\nold content\n\n<!-- end:Phase Timings -->";
        let result = append_plain_section_to_body(body, "Phase Timings", "new content");
        assert!(!result.contains("old content"));
        assert!(result.contains("new content"));
        assert_eq!(result.matches("## Phase Timings").count(), 1);
    }

    #[test]
    fn append_plain_section_idempotent() {
        let body = "## What\n\nFeature Title.";
        let first = append_plain_section_to_body(body, "Phase Timings", "| Phase | Duration |");
        let second = append_plain_section_to_body(&first, "Phase Timings", "| Phase | Duration |");
        assert_eq!(first, second);
        assert_eq!(second.matches("## Phase Timings").count(), 1);
    }
}
