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
