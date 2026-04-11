//! Universal-coverage enumeration scanner.
//!
//! When a plan task or committed prose claims a guard/check/constraint
//! applies to "every subcommand" or "all runners" (a universal
//! quantifier plus a code-family noun), the claim must be accompanied
//! by a named list of the concrete siblings. Without the list, the
//! Code phase has no checklist and downstream reviewers catch
//! uncovered siblings. See `.claude/rules/scope-enumeration.md` for
//! the rule and the rationale.
//!
//! This module is the shared scanner used by two callers:
//!
//! - `bin/flow plan-check` — gates Plan-phase completion on
//!   `.flow-states/<branch>-plan.md`. Consumed by the standard path
//!   via `skills/flow-plan/SKILL.md` Step 4.
//! - `src/plan_extract.rs` — same gate, applied directly in the
//!   extracted path for pre-decomposed issues (bypasses the skill).
//!
//! A contract test in `tests/scope_enumeration.rs` also uses `scan`
//! against the committed prose corpus (CLAUDE.md, `.claude/rules/*.md`,
//! `skills/**/SKILL.md`, `.claude/skills/**/SKILL.md`) to catch drift
//! in authoritative documentation.
//!
//! ## Vocabulary
//!
//! The trigger vocabulary is closed and curated — novel phrasings
//! that slip past the regex are handled by expanding the vocabulary
//! in follow-up commits, mirroring the curated-pattern discipline
//! documented for the backward-facing comment scanner in
//! `.claude/rules/comment-quality.md`. The rule file is the primary
//! instrument; this scanner is the merge-conflict trip-wire.
//!
//! ## Opt-outs
//!
//! Two line-level opt-out comments are recognized:
//!
//! - `<!-- scope-enumeration: open-ended -->` — for genuinely
//!   unbounded families (e.g. "every supported git version").
//! - `<!-- scope-enumeration: imperative -->` — for instructional
//!   phrasing the heuristic cannot distinguish from a coverage claim
//!   (e.g. "grep for every caller").
//!
//! The comment applies to itself and to the next non-blank line.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

/// Look-around window when searching for a nearby enumeration.
///
/// The value was chosen to span the longest observed gap between a
/// universal phrase and its adjacent bullet list in the current tree
/// (`tool-dispatch.md:26` — 5 lines backward) with generous headroom.
const WINDOW_LINES: usize = 15;

/// The trigger pattern: a universal quantifier followed by a
/// code-family noun. An optional single-token adjective is allowed
/// between the quantifier and the noun (e.g. "every sibling entry
/// point"). The noun set is closed and curated. `\b` word boundaries
/// on both ends prevent substring matches.
pub const SCOPE_TRIGGER_PATTERN: &str = r"(?i)\b(?:every|all|each)\s+(?:[\w\-]+\s+)?(?:state[\s\-]+mutator|CLI\s+(?:variant|entry)|entry\s+point|dispatch\s+path|subcommand|runner|callsite|caller|mutator)s?\b";

fn trigger_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(SCOPE_TRIGGER_PATTERN).expect("scope trigger regex must compile"))
}

/// Matches inline backtick-quoted spans like `` `foo` ``. Used by the
/// enumeration heuristic to count identifiers in the lookaround window.
fn backtick_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"`[^`\n]+`").expect("backtick regex must compile"))
}

/// A violation of the scope-enumeration rule.
///
/// `line` is 1-indexed. `phrase` is the literal match from the trigger
/// regex. `context` is the full line containing the match (untrimmed).
#[derive(Debug, Clone)]
pub struct Violation {
    pub file: PathBuf,
    pub line: usize,
    pub phrase: String,
    pub context: String,
}

/// Scan `content` for universal-coverage prose without a nearby
/// named enumeration. `source` is the file path used to populate
/// `Violation::file`; when scanning an in-memory plan fragment, pass
/// the plan file's absolute path.
///
/// Returns an empty Vec when the content is clean, contains no
/// triggers, or every trigger is accompanied by an enumeration,
/// negated, inside a fenced block, or opted out.
pub fn scan(content: &str, source: &Path) -> Vec<Violation> {
    let lines: Vec<&str> = content.lines().collect();
    let fenced = compute_fenced_mask(&lines);
    let mut violations = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        if fenced[idx] {
            continue;
        }
        if is_optout_line(&lines, idx) {
            continue;
        }
        for m in trigger_regex().find_iter(line) {
            if has_negation_prefix(line, m.start()) {
                continue;
            }
            let window = collect_window(&lines, idx, &fenced);
            if !enumeration_present(&window) {
                violations.push(Violation {
                    file: source.to_path_buf(),
                    line: idx + 1,
                    phrase: m.as_str().to_string(),
                    context: (*line).to_string(),
                });
            }
        }
    }
    violations
}

/// Returns `true` for every line index that sits inside (or on) a
/// fenced code block. The fence lines themselves are marked `true`
/// so triggers on the fence markers are ignored.
fn compute_fenced_mask(lines: &[&str]) -> Vec<bool> {
    let mut mask = vec![false; lines.len()];
    let mut in_block = false;
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("```") {
            in_block = !in_block;
            mask[i] = true;
            continue;
        }
        mask[i] = in_block;
    }
    mask
}

/// Returns `true` when line `idx` is covered by an opt-out comment —
/// either on the same line, or on the immediately preceding non-blank
/// line.
fn is_optout_line(lines: &[&str], idx: usize) -> bool {
    if line_has_optout_comment(lines[idx]) {
        return true;
    }
    let mut j = idx;
    while j > 0 {
        j -= 1;
        let trimmed = lines[j].trim();
        if trimmed.is_empty() {
            continue;
        }
        return line_has_optout_comment(trimmed);
    }
    false
}

fn line_has_optout_comment(line: &str) -> bool {
    line.contains("<!-- scope-enumeration: open-ended -->")
        || line.contains("<!-- scope-enumeration: imperative -->")
}

/// Returns `true` when the prefix of `line` before `match_start`
/// contains a negation word. Used to skip phrases like
/// "do not trace every caller" where "every caller" is explicitly
/// not a coverage claim.
fn has_negation_prefix(line: &str, match_start: usize) -> bool {
    let prefix = &line[..match_start].to_lowercase();
    const NEGATIONS: &[&str] = &[
        "not ",
        "never ",
        "avoid ",
        "don't ",
        "won't ",
        "cannot ",
        "doesn't ",
        "shouldn't ",
        "mustn't ",
        "without ",
    ];
    NEGATIONS.iter().any(|n| prefix.contains(n))
}

/// Returns the content of the enumeration search window around
/// line `idx`. The window spans up to `WINDOW_LINES` lines in each
/// direction, stopping at any `##` or higher Markdown heading, and
/// skips lines inside fenced code blocks.
fn collect_window(lines: &[&str], idx: usize, fenced: &[bool]) -> Vec<String> {
    let start = idx.saturating_sub(WINDOW_LINES);
    let end = (idx + WINDOW_LINES + 1).min(lines.len());

    // Walk backward from idx-1 toward start, stopping at the line
    // AFTER a section boundary (so the boundary itself is excluded).
    let mut up_stop = start;
    if idx > 0 {
        for i in (start..idx).rev() {
            if is_section_boundary(lines[i]) {
                up_stop = i + 1;
                break;
            }
        }
    }

    // Walk forward from idx+1 toward end, stopping at a section
    // boundary (so the boundary itself is excluded).
    let mut down_stop = end;
    for (i, line) in lines.iter().enumerate().take(end).skip(idx + 1) {
        if is_section_boundary(line) {
            down_stop = i;
            break;
        }
    }

    let mut window = Vec::with_capacity(down_stop - up_stop);
    for i in up_stop..down_stop {
        if !fenced[i] {
            window.push(lines[i].to_string());
        }
    }
    window
}

/// Returns `true` for `# ` or `## ` headings. `### ` and deeper are
/// NOT boundaries — the scanner's window must span multi-step skill
/// sections without truncation.
fn is_section_boundary(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("# ") || trimmed.starts_with("## ")
}

/// The enumeration heuristic: ≥ 2 inline backtick-quoted spans
/// anywhere in the window count as a named enumeration. This passes
/// every observed legitimate enumeration form in the current tree
/// (inline parenthetical, forward bullet list, backward bullet list)
/// and rejects the observed failure forms (prose claims with no
/// identifiers listed nearby).
pub fn enumeration_present(window: &[String]) -> bool {
    let mut count = 0;
    for line in window {
        count += backtick_regex().find_iter(line).count();
        if count >= 2 {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn dummy_path() -> PathBuf {
        PathBuf::from("dummy.md")
    }

    // --- scan ---

    #[test]
    fn trigger_matches_every_subcommand() {
        assert!(trigger_regex().is_match("add the guard to every subcommand"));
    }

    #[test]
    fn trigger_matches_all_runners() {
        assert!(trigger_regex().is_match("apply to all runners"));
    }

    #[test]
    fn trigger_matches_each_cli_entry_point() {
        assert!(trigger_regex().is_match("gate each CLI entry point"));
    }

    #[test]
    fn trigger_matches_state_mutator() {
        assert!(trigger_regex().is_match("every state mutator must enforce"));
    }

    #[test]
    fn trigger_matches_state_mutator_hyphenated() {
        assert!(trigger_regex().is_match("every state-mutator must enforce"));
    }

    #[test]
    fn trigger_matches_bare_mutator() {
        assert!(trigger_regex().is_match("every mutator"));
    }

    #[test]
    fn trigger_matches_dispatch_path() {
        assert!(trigger_regex().is_match("every dispatch path"));
    }

    #[test]
    fn trigger_matches_every_sibling_entry_point() {
        // Adjective between quantifier and noun.
        assert!(trigger_regex().is_match("every sibling entry point in the family"));
    }

    #[test]
    fn trigger_matches_plural_subcommands() {
        assert!(trigger_regex().is_match("all subcommands are gated"));
    }

    #[test]
    fn trigger_matches_plural_callers() {
        assert!(trigger_regex().is_match("grep for all callers"));
    }

    #[test]
    fn trigger_is_case_insensitive() {
        assert!(trigger_regex().is_match("Every Subcommand"));
        assert!(trigger_regex().is_match("ALL RUNNERS"));
    }

    #[test]
    fn trigger_rejects_non_code_nouns() {
        assert!(!trigger_regex().is_match("every commit must pass CI"));
        assert!(!trigger_regex().is_match("all developers should read this"));
        assert!(!trigger_regex().is_match("each release note"));
    }

    // --- scan: positive (enumeration present) ---

    #[test]
    fn scan_passes_inline_parenthetical() {
        let content = "`guard` runs on every subcommand (`foo`, `bar`, `baz`).\n";
        let v = scan(content, &dummy_path());
        assert!(v.is_empty(), "expected no violations, got {:?}", v);
    }

    #[test]
    fn scan_passes_forward_bullet_window() {
        let content = "Add the guard to every subcommand:\n\n- `foo` — does X\n- `bar` — does Y\n";
        let v = scan(content, &dummy_path());
        assert!(v.is_empty(), "expected no violations, got {:?}", v);
    }

    #[test]
    fn scan_passes_backward_bullet_window() {
        let content = "The two callsites:\n\n- `foo`\n- `bar`\n\nA test at each callsite should exercise X.\n";
        let v = scan(content, &dummy_path());
        assert!(v.is_empty(), "expected no violations, got {:?}", v);
    }

    #[test]
    fn scan_passes_sibling_entry_point_with_bullets() {
        let content = "The same guard must be added to every sibling entry point in the family:\n\n- `ci::run`\n- `build::run`\n- `lint::run`\n";
        let v = scan(content, &dummy_path());
        assert!(v.is_empty(), "expected no violations, got {:?}", v);
    }

    // --- scan: negative (violation surfaced) ---

    #[test]
    fn scan_fails_unenumerated_universal_claim() {
        let content = "Add the drift guard to every state mutator.\n";
        let v = scan(content, &dummy_path());
        assert_eq!(v.len(), 1, "{:?}", v);
        assert!(v[0].phrase.to_lowercase().contains("every"));
        assert_eq!(v[0].line, 1);
    }

    #[test]
    fn scan_fails_all_runners_without_list() {
        let content = "Apply FLOW_CI_RUNNING to all runners.\n";
        let v = scan(content, &dummy_path());
        assert_eq!(v.len(), 1, "{:?}", v);
    }

    #[test]
    fn scan_fails_each_entry_point_without_list() {
        let content = "Gate each CLI entry point with the permission check.\n";
        let v = scan(content, &dummy_path());
        assert_eq!(v.len(), 1, "{:?}", v);
    }

    // --- scan: negation skip ---

    #[test]
    fn scan_skips_do_not_trace_every() {
        let content = "Do not trace every caller of the function.\n";
        let v = scan(content, &dummy_path());
        assert!(v.is_empty(), "negation should skip, got {:?}", v);
    }

    #[test]
    fn scan_skips_never_enumerate() {
        let content = "Never enumerate every subcommand manually.\n";
        let v = scan(content, &dummy_path());
        assert!(v.is_empty());
    }

    // --- scan: fenced block skip ---

    #[test]
    fn scan_skips_trigger_inside_fenced_block() {
        let content = "## Heading\n\n```text\nevery state mutator enforces the guard\n```\n";
        let v = scan(content, &dummy_path());
        assert!(v.is_empty(), "fenced content should skip, got {:?}", v);
    }

    #[test]
    fn scan_skips_trigger_inside_fenced_block_with_language() {
        let content = "## Heading\n\n```rust\n// every subcommand is tested\nfn main() {}\n```\n";
        let v = scan(content, &dummy_path());
        assert!(v.is_empty(), "fenced content should skip, got {:?}", v);
    }

    // --- scan: opt-out comment skip ---

    #[test]
    fn scan_skips_open_ended_optout_same_line() {
        let content = "<!-- scope-enumeration: open-ended --> every supported version\n";
        let v = scan(content, &dummy_path());
        assert!(v.is_empty(), "same-line opt-out should skip, got {:?}", v);
    }

    #[test]
    fn scan_skips_open_ended_optout_preceding_line() {
        let content =
            "<!-- scope-enumeration: open-ended -->\nTest against every supported git version.\n";
        let v = scan(content, &dummy_path());
        assert!(
            v.is_empty(),
            "preceding-line opt-out should skip, got {:?}",
            v
        );
    }

    #[test]
    fn scan_skips_imperative_optout() {
        let content = "<!-- scope-enumeration: imperative -->\nGrep for every caller.\n";
        let v = scan(content, &dummy_path());
        assert!(v.is_empty());
    }

    #[test]
    fn scan_optout_does_not_leak_to_later_lines() {
        // The opt-out applies to the next non-blank line only.
        // A later unenumerated claim should still be flagged.
        let content = "<!-- scope-enumeration: open-ended -->\nevery supported git version\n\nAdd guard to every state mutator.\n";
        let v = scan(content, &dummy_path());
        assert_eq!(
            v.len(),
            1,
            "later unenumerated claim should still be flagged, got {:?}",
            v
        );
    }

    // --- scan: section boundary ---

    #[test]
    fn scan_does_not_cross_h2_heading_forward() {
        // Enumeration is in a DIFFERENT section — should not count.
        let content = "## A\n\nevery subcommand must enforce this.\n\n## B\n\n- `foo`\n- `bar`\n";
        let v = scan(content, &dummy_path());
        assert_eq!(
            v.len(),
            1,
            "forward enumeration across ## should not count, got {:?}",
            v
        );
    }

    #[test]
    fn scan_does_not_cross_h2_heading_backward() {
        let content = "## A\n\n- `foo`\n- `bar`\n\n## B\n\nAdd to every subcommand.\n";
        let v = scan(content, &dummy_path());
        assert_eq!(
            v.len(),
            1,
            "backward enumeration across ## should not count, got {:?}",
            v
        );
    }

    #[test]
    fn scan_crosses_h3_heading() {
        // ### is NOT a boundary — window spans subsections.
        let content =
            "## Section\n\nevery subcommand must do X\n\n### Subsection\n\n- `foo`\n- `bar`\n";
        let v = scan(content, &dummy_path());
        assert!(v.is_empty(), "### should not block the window, got {:?}", v);
    }

    // --- enumeration_present ---

    #[test]
    fn enumeration_present_requires_two_backticks() {
        let window = vec!["single `one` identifier".to_string()];
        assert!(!enumeration_present(&window));
    }

    #[test]
    fn enumeration_present_passes_on_two() {
        let window = vec!["two `one` and `two` identifiers".to_string()];
        assert!(enumeration_present(&window));
    }

    #[test]
    fn enumeration_present_counts_across_lines() {
        let window = vec!["`one`".to_string(), "`two`".to_string()];
        assert!(enumeration_present(&window));
    }
}
