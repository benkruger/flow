//! Plan signature deviation detector.
//!
//! `.claude/rules/plan-commit-atomicity.md` "Plan Signature
//! Deviations Must Be Logged" requires the Code phase to log any
//! prototype divergence from the plan via `bin/flow log` before
//! the commit that delivers the divergence lands. Instructional
//! enforcement alone is insufficient — a Code-phase agent can
//! drift from the plan's named test fixtures and commit without
//! logging. This module is the mechanical enforcement.
//!
//! The detector runs as a post-CI, pre-commit gate inside
//! `src/finalize_commit.rs::run_impl`. On an unacknowledged
//! drift it blocks the commit with a structured stderr message
//! and a JSON error response on stdout; the error message names
//! the deviation and supplies the exact `bin/flow log` command
//! the user should run to acknowledge it.
//!
//! ## Detection scope
//!
//! The plan-side parser walks the `## Tasks` section of the plan
//! file. Inside each task description it scans fenced code
//! blocks whose info string is empty or in the code-hint set —
//! `rust`, `bash`, `json`, `python`. Within each eligible block
//! it collects:
//!
//! - `fn <name>(` declarations — the test-name candidates
//! - `<key>\s*[:=]\s*['"]([^'"]+)['"]` assignments — the
//!   fixture-value candidates (single-line literals only)
//!
//! Each assignment is associated with the nearest-preceding
//! `fn` declaration in the same code block. Assignments without
//! a preceding `fn` in the same block are discarded.
//!
//! The diff-side parser walks `git diff --cached` output. For
//! each `+++ b/<path>` header ending in `.rs`, it tracks added
//! lines and identifies test-function boundaries by
//! `+fn <name>(` declarations. For each boundary it collects
//! single-line string literals from the added body until the
//! next `+fn` boundary, the next file header, or EOF.
//!
//! A `Deviation` is reported when a plan-named
//! `(test_name, fixture_key, plan_value)` triple exists, the
//! diff map contains `test_name`, and `plan_value` is absent
//! from the literal set collected for that test in the diff.
//!
//! ## What is intentionally out of scope
//!
//! - Tests the Code phase adds that the plan never names — the
//!   Plan Test Verification check in `skills/flow-code/SKILL.md`
//!   owns that invariant, not this detector.
//! - Multi-line string literals — the v1 parser is single-line.
//! - Prefix-renamed tests (plan says `fn test_foo`, code writes
//!   `fn test_foo_happy_path`) — exact `fn <name>(` match is
//!   the documented v1 contract.
//! - Plan prose outside the `## Tasks` section — Context,
//!   Exploration, Risks, Approach sections are not scanned.
//!
//! ## Bypass grammar
//!
//! A deviation is considered acknowledged when any line of the
//! branch's `.flow-states/<branch>.log` file contains BOTH the
//! literal `test_name` AND the literal `plan_value` as
//! case-sensitive substrings on the same line. Acknowledgment
//! is per-deviation and non-transferable: logging one drift
//! value does not unblock a different drift value on the same
//! test.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

use crate::flow_paths::FlowPaths;

/// A plan signature deviation.
///
/// `test_name` is the test function the plan named. `fixture_key`
/// is the identifier on the left side of the plan's `=` or `:`
/// assignment. `plan_value` is the string literal the plan
/// assigned. `plan_line` is the 1-indexed line number in the
/// plan file where the assignment was discovered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deviation {
    pub test_name: String,
    pub fixture_key: String,
    pub plan_value: String,
    pub plan_line: usize,
}

/// Code-block info strings that plan-side parsing treats as
/// eligible for scanning. Empty string covers untagged fences.
const ELIGIBLE_FENCE_LANGS: &[&str] = &["", "rust", "bash", "json", "python"];

/// Cached regex for `fn <name>(` declarations inside plan code
/// blocks. Matches at any position on the line so test
/// declarations preceded by attributes or whitespace are
/// recognized.
fn plan_fn_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bfn\s+(\w+)\s*\(").expect("plan fn regex must compile"))
}

/// Cached regex for `key = "value"` and `key: "value"`
/// assignments with double-quoted string literals.
fn double_quoted_assign_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(\w+)\s*[:=]\s*"([^"]*)""#)
            .expect("double-quoted assignment regex must compile")
    })
}

/// Cached regex for `key = 'value'` and `key: 'value'`
/// assignments with single-quoted string literals.
fn single_quoted_assign_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(\w+)\s*[:=]\s*'([^']*)'"#)
            .expect("single-quoted assignment regex must compile")
    })
}

/// Cached regex for the diff-side `+fn <name>(` added-line
/// boundary. The `^\+` anchor ensures we only match lines the
/// diff marks as added. Attributes inline on the same line
/// (`+#[test] fn test_foo()`) are tolerated.
fn diff_fn_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^\+\s*(?:#\[[^\]]*\]\s*)*(?:pub\s+)?fn\s+(\w+)\s*\(")
            .expect("diff fn regex must compile")
    })
}

/// Cached regex for double-quoted string literals in any
/// context. Used on the diff side to harvest every literal
/// appearing on added lines inside a plan-named test body.
/// Escape-aware: `\"` inside the literal does not terminate
/// the match, symmetric with the plan-side assignment regex.
fn double_quoted_literal_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#""([^"\\]*(?:\\.[^"\\]*)*)""#)
            .expect("double-quoted literal regex must compile")
    })
}

/// Cached regex for single-quoted string literals in any
/// context. Escape-aware: `\'` inside the literal does not
/// terminate the match, symmetric with the double-quoted form.
fn single_quoted_literal_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"'([^'\\]*(?:\\.[^'\\]*)*)'"#)
            .expect("single-quoted literal regex must compile")
    })
}

/// Scan plan prose and a staged diff for plan signature
/// deviations.
///
/// Returns one `Deviation` for each
/// `(test_name, fixture_key, plan_value)` triple the plan names
/// whose `plan_value` does not appear as a string literal in the
/// body of a diff-added test function named `test_name`.
pub fn scan(plan_content: &str, staged_diff: &str) -> Vec<Deviation> {
    let triples = extract_plan_triples(plan_content);
    if triples.is_empty() {
        return Vec::new();
    }
    let diff_map = extract_diff_literals(staged_diff);

    let mut deviations = Vec::new();
    for (test_name, fixture_key, plan_value, plan_line) in triples {
        let Some(literals) = diff_map.get(&test_name) else {
            // The test is not in the staged diff. Another gate
            // (Plan Test Verification) owns the "plan named X
            // but X is missing" case.
            continue;
        };
        if !literals.contains(&plan_value) {
            deviations.push(Deviation {
                test_name,
                fixture_key,
                plan_value,
                plan_line,
            });
        }
    }
    deviations
}

/// Walk the plan's `## Tasks` section and collect every
/// `(test_name, fixture_key, plan_value, plan_line)` tuple from
/// eligible fenced code blocks.
///
/// Returns an empty Vec when the plan has no `## Tasks`
/// heading, when the Tasks section contains no eligible code
/// blocks, or when no assignments inside those blocks are
/// associated with a preceding `fn` declaration.
fn extract_plan_triples(plan_content: &str) -> Vec<(String, String, String, usize)> {
    let lines: Vec<&str> = plan_content.lines().collect();

    let Some(tasks_start) = find_tasks_section_start(&lines) else {
        return Vec::new();
    };
    let tasks_end = find_next_level_2_heading(&lines, tasks_start);

    let mut triples: Vec<(String, String, String, usize)> = Vec::new();
    let mut in_block = false;
    let mut block_lang = String::new();
    let mut current_fn: Option<String> = None;
    // Track the triple count at the last fence-open so an
    // unclosed fence can be rewound — triples collected after
    // the stray opener are discarded. Mirrors the rewind
    // discipline in `scope_enumeration::compute_fenced_mask`.
    let mut triples_at_fence_open: Option<usize> = None;

    for (rel_idx, line) in lines.iter().enumerate().take(tasks_end).skip(tasks_start) {
        let trimmed = line.trim_start();
        let one_indexed_line = rel_idx + 1;

        // Recognize both backtick (```) and tilde (~~~) fences per
        // CommonMark so a plan author's tilde-fenced Rust block does
        // not silently disable fixture extraction for that block.
        let fence_rest = trimmed
            .strip_prefix("```")
            .or_else(|| trimmed.strip_prefix("~~~"));
        if let Some(rest) = fence_rest {
            if in_block {
                in_block = false;
                block_lang.clear();
                current_fn = None;
                triples_at_fence_open = None;
            } else {
                in_block = true;
                block_lang = rest.trim().to_string();
                triples_at_fence_open = Some(triples.len());
            }
            continue;
        }

        if !in_block {
            continue;
        }
        if !ELIGIBLE_FENCE_LANGS.contains(&block_lang.as_str()) {
            continue;
        }

        if let Some(cap) = plan_fn_regex().captures(line) {
            current_fn = Some(cap[1].to_string());
        }

        let Some(test_name) = current_fn.as_ref() else {
            continue;
        };

        for cap in double_quoted_assign_regex().captures_iter(line) {
            let key = cap[1].to_string();
            let value = cap[2].to_string();
            if is_reserved_key(&key) {
                continue;
            }
            triples.push((test_name.clone(), key, value, one_indexed_line));
        }
        for cap in single_quoted_assign_regex().captures_iter(line) {
            let key = cap[1].to_string();
            let value = cap[2].to_string();
            if is_reserved_key(&key) {
                continue;
            }
            triples.push((test_name.clone(), key, value, one_indexed_line));
        }
    }

    // Unclosed fence at section end: discard triples collected
    // inside the stray opener so prose that follows an unclosed
    // fence does not produce false-positive deviations.
    if let Some(rewind_to) = triples_at_fence_open {
        triples.truncate(rewind_to);
    }

    triples
}

/// Returns the 0-indexed line number of the first line after a
/// `## Tasks` heading, or `None` if no such heading exists.
/// Tracks Markdown fence state so a `## Tasks` literal inside a
/// fenced code block in a preceding section is not matched.
fn find_tasks_section_start(lines: &[&str]) -> Option<usize> {
    let mut in_fence = false;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if trimmed == "## Tasks" || trimmed.starts_with("## Tasks ") {
            return Some(i + 1);
        }
    }
    None
}

/// Returns the 0-indexed line number of the next level-2
/// Markdown heading after `start`, or `lines.len()` if no such
/// heading exists before EOF. Tracks both backtick and tilde
/// fences per CommonMark so a `## ` inside a fenced example
/// block under the Tasks section does not silently truncate the
/// scan scope. `"### "` does not start with `"## "` (byte 2 is
/// `#` not ` `), so level-3+ headings are excluded by the
/// `starts_with` check alone.
fn find_next_level_2_heading(lines: &[&str], start: usize) -> usize {
    let mut in_fence = false;
    for (i, line) in lines.iter().enumerate().skip(start) {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if trimmed.starts_with("## ") {
            return i;
        }
    }
    lines.len()
}

/// Identifiers the assignment regex will capture but which are
/// not useful as fixture keys. The primary case is `let` (Rust
/// keyword preceding the real binding name).
fn is_reserved_key(key: &str) -> bool {
    matches!(key, "let" | "const" | "static" | "mut")
}

/// Walk a `git diff --cached` output and collect the set of
/// string literals that appear on added lines inside each
/// test-function body. Only `*.rs` files are considered.
///
/// Returns a map from test-function name to the set of literal
/// strings found inside that function's added body.
fn extract_diff_literals(staged_diff: &str) -> HashMap<String, HashSet<String>> {
    let mut result: HashMap<String, HashSet<String>> = HashMap::new();
    let mut current_file_is_rs = false;
    let mut current_test: Option<String> = None;

    for line in staged_diff.lines() {
        if let Some(rest) = line.strip_prefix("+++ ") {
            let path = rest.trim_start_matches("b/").trim();
            current_file_is_rs = path.ends_with(".rs");
            current_test = None;
            continue;
        }
        if line.starts_with("--- ") || line.starts_with("@@") {
            // Hunk headers and "old" file markers do not mutate
            // test-function scope; they do not add content.
            continue;
        }

        if !current_file_is_rs {
            continue;
        }

        if !line.starts_with('+') || line.starts_with("+++") {
            // Unchanged context lines and "+++" file markers
            // are ignored. Context lines inside a function body
            // are not added content.
            continue;
        }

        if let Some(cap) = diff_fn_regex().captures(line) {
            current_test = Some(cap[1].to_string());
        }

        let Some(test_name) = current_test.as_ref() else {
            continue;
        };
        let entry = result.entry(test_name.clone()).or_default();
        for cap in double_quoted_literal_regex().captures_iter(line) {
            entry.insert(cap[1].to_string());
        }
        for cap in single_quoted_literal_regex().captures_iter(line) {
            entry.insert(cap[1].to_string());
        }
    }

    result
}

/// Acknowledge a deviation via a matching `bin/flow log` entry.
///
/// Returns `true` when any line of `log_content` contains BOTH
/// the literal `deviation.test_name` AND the literal
/// `deviation.plan_value` as case-sensitive substrings on the
/// same line. Returns `false` otherwise, including when
/// `log_content` is empty or carries those tokens on separate
/// lines.
pub fn acknowledged(deviation: &Deviation, log_content: &str) -> bool {
    // Empty plan_value would match any line (`"".is_empty()` is
    // always true for `contains`). Guard against trivial
    // acknowledgment of empty-string fixture values.
    if deviation.plan_value.is_empty() {
        return false;
    }
    log_content.lines().any(|line| {
        if !line.contains(&deviation.test_name) {
            return false;
        }
        // Verify plan_value appears independently — not just as
        // a substring of test_name. Remove all occurrences of
        // test_name from the line, then check the remainder.
        let without_test_name = line.replace(&deviation.test_name, "");
        without_test_name.contains(&deviation.plan_value)
    })
}

/// Run the full plan-deviation detection gate for a branch.
///
/// Reads the plan file named by `files.plan` in the branch's
/// state, scans it against `staged_diff`, filters acknowledged
/// deviations via the branch log, and returns `Ok(())` when no
/// unacknowledged deviation remains. On any unacknowledged
/// deviation returns `Err(Vec<Deviation>)` listing the
/// unacknowledged set.
///
/// Tolerates a missing state file, a missing `files.plan`, a
/// missing plan file on disk, an invalid branch name (slash,
/// empty), and an unreadable log file — all five return
/// `Ok(())` so flows that predate this gate (or flows running
/// outside Phase 3) are not blocked.
pub fn run_impl(root: &Path, branch: &str, staged_diff: &str) -> Result<(), Vec<Deviation>> {
    // Invalid branch (e.g. slash-containing) → no active flow
    // on this branch. The `try_new` fallible constructor
    // matches the discipline in `external-input-validation.md`
    // for CLI branch arguments.
    let Some(paths) = FlowPaths::try_new(root, branch) else {
        return Ok(());
    };

    // State file — tolerate missing/empty/non-JSON/wrong root.
    let state_content = match fs::read_to_string(paths.state_file()) {
        Ok(content) if !content.is_empty() => content,
        _ => return Ok(()),
    };
    let state: Value = match serde_json::from_str(&state_content) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    if !state.is_object() {
        return Ok(());
    }

    // Plan path — check nested `files.plan` first, fall back
    // to legacy top-level `plan_file` for older state files.
    let plan_rel = state
        .get("files")
        .and_then(|f| f.get("plan"))
        .and_then(|p| p.as_str())
        .or_else(|| state.get("plan_file").and_then(|p| p.as_str()))
        .filter(|s| !s.is_empty());
    let Some(plan_rel) = plan_rel else {
        return Ok(());
    };

    // Resolve the plan path against the project root. The
    // state file always stores a project-relative path.
    let plan_path = root.join(plan_rel);
    let plan_content = match fs::read_to_string(&plan_path) {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    let deviations = scan(&plan_content, staged_diff);
    if deviations.is_empty() {
        return Ok(());
    }

    // Log content — tolerate missing or unreadable log. An
    // empty string simply acknowledges nothing, leaving every
    // deviation in the unacknowledged set.
    let log_content = fs::read_to_string(paths.log_file()).unwrap_or_default();

    let unacknowledged: Vec<Deviation> = deviations
        .into_iter()
        .filter(|d| !acknowledged(d, &log_content))
        .collect();

    if unacknowledged.is_empty() {
        Ok(())
    } else {
        Err(unacknowledged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- scan ---

    #[test]
    fn scan_plan_without_tasks_section_returns_empty() {
        let plan = "# Plan\n\n## Context\n\nSome context.\n\n## Risks\n\nNone.\n";
        let diff = "";
        assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
    }

    #[test]
    fn scan_plan_pseudocode_fence_skipped() {
        let plan = concat!(
            "## Tasks\n\n",
            "Task 1 — test foo.\n\n",
            "```pseudocode\n",
            "fn test_foo() {\n",
            "    let key = \"expected\";\n",
            "}\n",
            "```\n",
        );
        let diff = concat!(
            "diff --git a/tests/foo.rs b/tests/foo.rs\n",
            "--- a/tests/foo.rs\n",
            "+++ b/tests/foo.rs\n",
            "@@ -0,0 +1,3 @@\n",
            "+fn test_foo() {\n",
            "+    let key = \"actual\";\n",
            "+}\n",
        );
        assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
    }

    #[test]
    fn scan_plan_matching_diff_returns_empty() {
        let plan = concat!(
            "## Tasks\n\n",
            "Task 1 — test foo.\n\n",
            "```rust\n",
            "fn test_foo() {\n",
            "    let key = \"expected\";\n",
            "}\n",
            "```\n",
        );
        let diff = concat!(
            "diff --git a/tests/foo.rs b/tests/foo.rs\n",
            "--- a/tests/foo.rs\n",
            "+++ b/tests/foo.rs\n",
            "@@ -0,0 +1,3 @@\n",
            "+fn test_foo() {\n",
            "+    let key = \"expected\";\n",
            "+}\n",
        );
        assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
    }

    #[test]
    fn scan_plan_diverging_diff_returns_one_deviation() {
        let plan = concat!(
            "## Tasks\n\n",
            "Task 1 — test foo.\n\n",
            "```rust\n",
            "fn test_foo() {\n",
            "    let key = \"expected\";\n",
            "}\n",
            "```\n",
        );
        let diff = concat!(
            "diff --git a/tests/foo.rs b/tests/foo.rs\n",
            "--- a/tests/foo.rs\n",
            "+++ b/tests/foo.rs\n",
            "@@ -0,0 +1,3 @@\n",
            "+fn test_foo() {\n",
            "+    let key = \"actual\";\n",
            "+}\n",
        );
        let result = scan(plan, diff);
        assert_eq!(result.len(), 1, "expected exactly one deviation");
        assert_eq!(result[0].test_name, "test_foo");
        assert_eq!(result[0].fixture_key, "key");
        assert_eq!(result[0].plan_value, "expected");
    }

    #[test]
    fn scan_plan_assignment_without_fn_context_skipped() {
        let plan = concat!(
            "## Tasks\n\n",
            "Task 1 — bare assignment with no preceding fn.\n\n",
            "```rust\n",
            "let bare = \"orphan\";\n",
            "```\n",
        );
        let diff = concat!(
            "diff --git a/tests/foo.rs b/tests/foo.rs\n",
            "--- a/tests/foo.rs\n",
            "+++ b/tests/foo.rs\n",
            "@@ -0,0 +1,1 @@\n",
            "+let bare = \"different\";\n",
        );
        assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
    }

    #[test]
    fn scan_diff_non_rust_file_ignored() {
        let plan = concat!(
            "## Tasks\n\n",
            "Task 1 — test foo.\n\n",
            "```rust\n",
            "fn test_foo() {\n",
            "    let key = \"expected\";\n",
            "}\n",
            "```\n",
        );
        let diff = concat!(
            "diff --git a/tests/foo.py b/tests/foo.py\n",
            "--- a/tests/foo.py\n",
            "+++ b/tests/foo.py\n",
            "@@ -0,0 +1,2 @@\n",
            "+def test_foo():\n",
            "+    key = 'actual'\n",
        );
        assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
    }

    #[test]
    fn scan_diff_test_not_in_plan_ignored() {
        let plan = concat!(
            "## Tasks\n\n",
            "Task 1 — test foo.\n\n",
            "```rust\n",
            "fn test_foo() {\n",
            "    let key = \"expected\";\n",
            "}\n",
            "```\n",
        );
        let diff = concat!(
            "diff --git a/tests/foo.rs b/tests/foo.rs\n",
            "--- a/tests/foo.rs\n",
            "+++ b/tests/foo.rs\n",
            "@@ -0,0 +1,3 @@\n",
            "+fn test_unrelated() {\n",
            "+    let key = \"anything\";\n",
            "+}\n",
        );
        assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
    }

    #[test]
    fn scan_diff_prefix_renamed_test_does_not_match_intentionally() {
        // Contract: v1 uses exact `fn <name>(` match. A renamed
        // test (even a prefix-extended one) is invisible to the
        // detector. A future version may relax this.
        let plan = concat!(
            "## Tasks\n\n",
            "Task 1 — test foo.\n\n",
            "```rust\n",
            "fn test_foo() {\n",
            "    let key = \"expected\";\n",
            "}\n",
            "```\n",
        );
        let diff = concat!(
            "diff --git a/tests/foo.rs b/tests/foo.rs\n",
            "--- a/tests/foo.rs\n",
            "+++ b/tests/foo.rs\n",
            "@@ -0,0 +1,3 @@\n",
            "+fn test_foo_happy_path() {\n",
            "+    let key = \"actual\";\n",
            "+}\n",
        );
        assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
    }

    #[test]
    fn scan_plan_discussion_prose_in_risks_section_ignored() {
        // Plan's Risks section contains a code fence with a
        // value that would drift against the diff. Parser must
        // scope to `## Tasks` only and ignore Risks prose.
        let plan = concat!(
            "## Risks\n\n",
            "```rust\n",
            "fn test_foo() {\n",
            "    let key = \"ignored\";\n",
            "}\n",
            "```\n\n",
            "## Tasks\n\n",
            "Task 1 — plain prose, no code blocks.\n",
        );
        let diff = concat!(
            "diff --git a/tests/foo.rs b/tests/foo.rs\n",
            "--- a/tests/foo.rs\n",
            "+++ b/tests/foo.rs\n",
            "@@ -0,0 +1,3 @@\n",
            "+fn test_foo() {\n",
            "+    let key = \"actual\";\n",
            "+}\n",
        );
        assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
    }

    #[test]
    fn scan_plan_multiple_tests_one_drifts_returns_one_deviation() {
        let plan = concat!(
            "## Tasks\n\n",
            "Task 1 — two tests.\n\n",
            "```rust\n",
            "fn test_alpha() {\n",
            "    let key = \"alpha_ok\";\n",
            "}\n",
            "\n",
            "fn test_beta() {\n",
            "    let key = \"beta_ok\";\n",
            "}\n",
            "```\n",
        );
        let diff = concat!(
            "diff --git a/tests/foo.rs b/tests/foo.rs\n",
            "--- a/tests/foo.rs\n",
            "+++ b/tests/foo.rs\n",
            "@@ -0,0 +1,6 @@\n",
            "+fn test_alpha() {\n",
            "+    let key = \"alpha_ok\";\n",
            "+}\n",
            "+fn test_beta() {\n",
            "+    let key = \"beta_different\";\n",
            "+}\n",
        );
        let result = scan(plan, diff);
        assert_eq!(result.len(), 1, "only test_beta should drift");
        assert_eq!(result[0].test_name, "test_beta");
        assert_eq!(result[0].plan_value, "beta_ok");
    }

    // --- acknowledged ---

    fn make_deviation(test_name: &str, plan_value: &str) -> Deviation {
        Deviation {
            test_name: test_name.to_string(),
            fixture_key: "key".to_string(),
            plan_value: plan_value.to_string(),
            plan_line: 1,
        }
    }

    #[test]
    fn is_reserved_key_matches_all_four_keywords() {
        assert!(is_reserved_key("let"));
        assert!(is_reserved_key("const"));
        assert!(is_reserved_key("static"));
        assert!(is_reserved_key("mut"));
    }

    #[test]
    fn is_reserved_key_rejects_user_identifiers() {
        assert!(!is_reserved_key("foo"));
        assert!(!is_reserved_key("expected_value"));
        assert!(!is_reserved_key("LET")); // case-sensitive
        assert!(!is_reserved_key(""));
    }

    #[test]
    fn find_tasks_section_skips_tasks_heading_inside_code_fence() {
        let lines = vec!["```", "## Tasks", "```", "## Tasks", "content"];
        // The first "## Tasks" is inside a fence and must be skipped;
        // the second (post-fence) is the real start.
        assert_eq!(find_tasks_section_start(&lines), Some(4));
    }

    #[test]
    fn find_tasks_section_none_when_absent() {
        let lines = vec!["## Context", "## Approach", "content"];
        assert_eq!(find_tasks_section_start(&lines), None);
    }

    #[test]
    fn find_next_level_2_heading_returns_len_when_no_h2_after_start() {
        let lines = vec!["## Tasks", "content", "### sub-heading"];
        // Starting at index 1, no more ## headings — falls back to lines.len()
        assert_eq!(find_next_level_2_heading(&lines, 1), 3);
    }

    #[test]
    fn find_next_level_2_heading_stops_at_next_h2() {
        let lines = vec!["## Tasks", "body", "## Next", "more"];
        assert_eq!(find_next_level_2_heading(&lines, 1), 2);
    }

    #[test]
    fn acknowledged_log_line_contains_both_returns_true() {
        let dev = make_deviation("test_foo", "/flow:flow-plan");
        let log = "2026-04-15T10:00:00-08:00 [Phase 3] Plan signature deviation: test_foo drifted from /flow:flow-plan to /flow:flow-code-review. Reason: X.\n";
        assert!(acknowledged(&dev, log));
    }

    #[test]
    fn acknowledged_log_line_missing_plan_value_returns_false() {
        let dev = make_deviation("test_foo", "/flow:flow-plan");
        let log = "2026-04-15T10:00:00-08:00 [Phase 3] test_foo is under development.\n";
        assert!(!acknowledged(&dev, log));
    }

    #[test]
    fn acknowledged_missing_log_returns_false() {
        let dev = make_deviation("test_foo", "/flow:flow-plan");
        let log = "";
        assert!(!acknowledged(&dev, log));
    }

    #[test]
    fn acknowledged_log_split_lines_returns_false() {
        // Test name appears on one line, plan value on another.
        // Acknowledgment requires both on the same line.
        let dev = make_deviation("test_foo", "/flow:flow-plan");
        let log = concat!(
            "2026-04-15T10:00:00-08:00 [Phase 3] test_foo is the test under review.\n",
            "2026-04-15T10:01:00-08:00 [Phase 3] The plan named /flow:flow-plan earlier.\n",
        );
        assert!(!acknowledged(&dev, log));
    }

    #[test]
    fn acknowledged_log_contains_both_on_single_line_case_sensitive() {
        // Plan value has different case than what appears in the
        // log — substring comparison is case-sensitive, so this
        // does NOT acknowledge the deviation.
        let dev = make_deviation("test_foo", "/flow:flow-plan");
        let log = "2026-04-15T10:00:00-08:00 [Phase 3] test_foo drifted from /FLOW:FLOW-PLAN to new value.\n";
        assert!(!acknowledged(&dev, log));
    }

    // --- run_impl ---

    use std::fs;

    const RUN_IMPL_BRANCH: &str = "devtest";

    /// Builds a canonicalized tempdir plus an empty `.flow-states/`
    /// directory inside it, and returns `(TempDir, canonical_root)`.
    /// The TempDir must be held by the caller to keep the filesystem
    /// alive for the duration of the test. macOS path canonicalization
    /// is applied so subprocess-style path comparisons stay stable.
    fn run_impl_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir
            .path()
            .canonicalize()
            .expect("tempdir path must canonicalize");
        fs::create_dir_all(root.join(".flow-states")).expect("create .flow-states dir");
        (dir, root)
    }

    fn write_state(root: &Path, branch: &str, contents: &str) {
        let state_path = root.join(".flow-states").join(format!("{}.json", branch));
        fs::write(&state_path, contents).expect("write state file");
    }

    fn write_plan(root: &Path, branch: &str, contents: &str) {
        let plan_path = root
            .join(".flow-states")
            .join(format!("{}-plan.md", branch));
        fs::write(&plan_path, contents).expect("write plan file");
    }

    fn write_log(root: &Path, branch: &str, contents: &str) {
        let log_path = root.join(".flow-states").join(format!("{}.log", branch));
        fs::write(&log_path, contents).expect("write log file");
    }

    const DRIFTING_PLAN: &str = concat!(
        "## Tasks\n\n",
        "Task 1 — test foo.\n\n",
        "```rust\n",
        "fn test_foo() {\n",
        "    let key = \"expected\";\n",
        "}\n",
        "```\n",
    );

    const DRIFTING_DIFF: &str = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+fn test_foo() {\n",
        "+    let key = \"actual\";\n",
        "+}\n",
    );

    const MATCHING_DIFF: &str = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+fn test_foo() {\n",
        "+    let key = \"expected\";\n",
        "+}\n",
    );

    #[test]
    fn run_impl_missing_state_file_returns_ok() {
        let (_dir, root) = run_impl_fixture();
        let result = run_impl(&root, RUN_IMPL_BRANCH, DRIFTING_DIFF);
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn run_impl_state_without_plan_path_returns_ok() {
        let (_dir, root) = run_impl_fixture();
        write_state(&root, RUN_IMPL_BRANCH, r#"{"branch":"devtest"}"#);
        let result = run_impl(&root, RUN_IMPL_BRANCH, DRIFTING_DIFF);
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn run_impl_plan_path_set_but_file_missing_returns_ok() {
        let (_dir, root) = run_impl_fixture();
        write_state(
            &root,
            RUN_IMPL_BRANCH,
            r#"{"branch":"devtest","files":{"plan":".flow-states/devtest-plan.md"}}"#,
        );
        let result = run_impl(&root, RUN_IMPL_BRANCH, DRIFTING_DIFF);
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn run_impl_no_deviations_returns_ok() {
        let (_dir, root) = run_impl_fixture();
        write_state(
            &root,
            RUN_IMPL_BRANCH,
            r#"{"branch":"devtest","files":{"plan":".flow-states/devtest-plan.md"}}"#,
        );
        write_plan(&root, RUN_IMPL_BRANCH, DRIFTING_PLAN);
        let result = run_impl(&root, RUN_IMPL_BRANCH, MATCHING_DIFF);
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn run_impl_deviations_all_acknowledged_returns_ok() {
        let (_dir, root) = run_impl_fixture();
        write_state(
            &root,
            RUN_IMPL_BRANCH,
            r#"{"branch":"devtest","files":{"plan":".flow-states/devtest-plan.md"}}"#,
        );
        write_plan(&root, RUN_IMPL_BRANCH, DRIFTING_PLAN);
        write_log(
            &root,
            RUN_IMPL_BRANCH,
            "2026-04-15T10:00:00-08:00 [Phase 3] Plan signature deviation: test_foo drifted from expected to actual.\n",
        );
        let result = run_impl(&root, RUN_IMPL_BRANCH, DRIFTING_DIFF);
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn run_impl_unacknowledged_deviations_returns_err() {
        let (_dir, root) = run_impl_fixture();
        write_state(
            &root,
            RUN_IMPL_BRANCH,
            r#"{"branch":"devtest","files":{"plan":".flow-states/devtest-plan.md"}}"#,
        );
        write_plan(&root, RUN_IMPL_BRANCH, DRIFTING_PLAN);
        let result = run_impl(&root, RUN_IMPL_BRANCH, DRIFTING_DIFF);
        match result {
            Err(devs) => {
                assert_eq!(
                    devs.len(),
                    1,
                    "expected exactly one unacknowledged deviation"
                );
                assert_eq!(devs[0].test_name, "test_foo");
                assert_eq!(devs[0].plan_value, "expected");
            }
            Ok(_) => panic!("expected Err with unacknowledged deviation"),
        }
    }
}
