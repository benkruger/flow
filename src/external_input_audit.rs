//! External-input validation audit gate.
//!
//! When a plan proposes tightening a function parameter with a new
//! `assert!`/`panic!`/invariant check, the plan must pair the
//! tightening with a callsite source-classification table — an
//! enumerated audit of every caller showing where the value comes
//! from and which constructor variant (panicking or fallible) is
//! appropriate. Without the table, plans can and do assert
//! "upstream sanitization guarantees X" when the assumption is
//! wrong — the motivating incident is benkruger/flow#1054 where
//! `FlowPaths::new` was tightened to panic on slash-containing
//! branches under a false "`branch_name()` sanitizes all callers"
//! assumption, crashing five hooks and `format-status` on every
//! `feature/foo` or `dependabot/*` branch.
//!
//! See `.claude/rules/external-input-audit-gate.md` for the rule,
//! the required table format, the opt-out grammar, and the
//! motivating incident. See `.claude/rules/external-input-validation.md`
//! for the prose discipline this gate enforces mechanically.
//!
//! This module is the shared scanner used by three callers:
//!
//! - `bin/flow plan-check` — gates Plan-phase completion on
//!   `.flow-states/<branch>-plan.md`. The standard plan path invokes
//!   it from `skills/flow-plan/SKILL.md` Step 4.
//! - `src/plan_extract.rs` extracted path — runs the same scanner
//!   against the promoted plan content for pre-decomposed issues
//!   (bypasses the skill entirely).
//! - `src/plan_extract.rs` resume path — runs the scanner against
//!   an existing plan file on re-invocation, so a plan the user
//!   edited after a prior violation must pass before the phase
//!   can complete.
//!
//! ## Trigger vocabulary
//!
//! The vocabulary is closed and curated. It catches verb+noun
//! phrasings ("tighten to panic", "add an `assert!` that",
//! "introduce a new invariant check"), imperative action phrases
//! ("panic on empty", "assert that branch is valid", "reject empty
//! branches"), and direct mentions of `assert!(`, `panic!(`,
//! `assert_eq!(`, `assert_ne!(` in plan prose outside fenced code
//! blocks — those are almost always tightening proposals when they
//! appear in a plan body.
//!
//! When a reviewer finds a novel phrasing that slips past, add it
//! to `TRIGGER_PATTERN`, add a matching trigger unit test, update
//! the rule file's vocabulary list, and note the addition in the
//! commit message.
//!
//! ## Compliance proof
//!
//! The audit table has four required columns: Caller, Source,
//! Classification, Handling. The gate validates **table presence**
//! within `WINDOW_NON_BLANK_LINES` of the trigger, not table
//! content — the rule's authority validates rows. The gate is a
//! forcing function for the audit conversation.
//!
//! Header aliases accepted:
//!
//! - Column 1: `caller` or `callsite`
//! - Column 2: `source` (exact)
//! - Column 3: `classif...` (prefix; accepts `classification`,
//!   `class`)
//! - Column 4: `handling` or `disposition`
//!
//! ## Opt-outs
//!
//! `<!-- external-input-audit: not-a-tightening -->` on the
//! trigger's line, the line directly above, or two lines above
//! with a single blank line in between. Same walk-back rule as
//! the scope-enumeration opt-outs.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

/// Number of non-blank lines scanned in each direction when
/// searching for a nearby audit table. Matches the sibling scanner
/// in `scope_enumeration.rs` so authors see consistent reach from
/// either gate.
const WINDOW_NON_BLANK_LINES: usize = 8;

/// Raw-line cap around the trigger to prevent pathologically long
/// sections from pulling the scanner into quadratic walks.
const WINDOW_LINES: usize = 30;

/// Trigger pattern — a proposal to add a panic/assert validation
/// to a function parameter. Closed and curated vocabulary:
///
/// - Verb+noun: `(add|tighten|introduce|enforce|require|impose)`
///   applied to `panic|assert!|assert_eq!|assert_ne!|panic!|
///   invariant check|validation assertion|constructor invariant`.
/// - Direct action phrases: `panic on <word>`, `assert that
///   <word>`, `reject (empty|invalid|malformed|unsupported)
///   <word>`.
/// - Direct mentions of `assert!(`, `panic!(`, `assert_eq!(`,
///   `assert_ne!(` in plan prose outside fenced blocks — handled
///   by a second pattern (`DIRECT_TOKEN_PATTERN`) scanned in
///   parallel.
///
/// When adding a new phrasing:
///
/// 1. Add it to the alternation in `TRIGGER_PATTERN` or
///    `DIRECT_TOKEN_PATTERN`.
/// 2. Add a `trigger_matches_<name>` unit test below.
/// 3. Update the vocabulary section in
///    `.claude/rules/external-input-audit-gate.md`.
pub const TRIGGER_PATTERN: &str = r"(?ix)
    \b(?:add|tighten|introduce|enforce|require|impose)\s+
    (?:a\s+|an\s+)?(?:new\s+)?
    (?:panic|assert!|assert_eq!|assert_ne!|panic!|
       invariant\s+check|validation\s+assertion|
       constructor\s+invariant)
    |
    \bpanic\s+on\s+\w+
    |
    \bassert\s+that\s+\w+
    |
    \breject\s+(?:empty|invalid|malformed|unsupported)\s+\w+
";

/// Direct token pattern — bare mentions of panic/assert macros in
/// plan prose outside fenced blocks. When a plan body literally
/// includes `assert!(` or `panic!(` outside a code fence, it is
/// describing a new assertion being added, not discussing one in a
/// pre-existing context. Fenced code (code blocks) is already
/// filtered by the fenced-mask pre-pass.
pub const DIRECT_TOKEN_PATTERN: &str = r"(?i)\b(?:assert!\(|panic!\(|assert_eq!\(|assert_ne!\()";

fn trigger_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(TRIGGER_PATTERN).expect("external-input-audit trigger regex"))
}

fn direct_token_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(DIRECT_TOKEN_PATTERN).expect("external-input-audit direct-token regex")
    })
}

/// A violation: a tightening proposal without a nearby audit table.
#[derive(Debug, Clone)]
pub struct Violation {
    pub file: PathBuf,
    pub line: usize,
    pub phrase: String,
    pub context: String,
}

/// Scan `content` for panic/assert tightening proposals without a
/// nearby audit table. `source` is the file path used to populate
/// `Violation::file`.
///
/// Returns an empty Vec when the content is clean, contains no
/// triggers, every trigger is accompanied by an audit table, is
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

        // Collect trigger matches from both patterns with their start
        // positions so negation-prefix checks and duplicate
        // suppression work the same way for both.
        let mut matches: Vec<(usize, usize, String)> = Vec::new();
        for m in trigger_regex().find_iter(line) {
            matches.push((m.start(), m.end(), m.as_str().to_string()));
        }
        for m in direct_token_regex().find_iter(line) {
            matches.push((m.start(), m.end(), m.as_str().to_string()));
        }
        // Sort by start position; if multiple regexes matched the
        // same span, dedupe on start position so we report one
        // violation per position.
        matches.sort_by_key(|(start, _, _)| *start);
        matches.dedup_by_key(|(start, _, _)| *start);

        for (start, end, phrase) in matches {
            if has_negation_prefix(line, start) {
                continue;
            }
            let (window, trigger_rel_idx) = collect_window(&lines, idx, &fenced);
            if !audit_table_present(&window, trigger_rel_idx, line, end) {
                violations.push(Violation {
                    file: source.to_path_buf(),
                    line: idx + 1,
                    phrase,
                    context: (*line).to_string(),
                });
            }
        }
    }

    violations
}

/// Returns `true` for every line that is inside a fenced code
/// block (fence markers themselves are also masked). Unclosed
/// fences fail open — the mask is reverted from the last open
/// marker so a typo cannot silence every downstream trigger.
fn compute_fenced_mask(lines: &[&str]) -> Vec<bool> {
    let mut mask = vec![false; lines.len()];
    let mut in_block = false;
    let mut last_open_idx: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("```") {
            if in_block {
                in_block = false;
                last_open_idx = None;
            } else {
                in_block = true;
                last_open_idx = Some(i);
            }
            mask[i] = true;
            continue;
        }
        mask[i] = in_block;
    }
    if in_block {
        if let Some(start) = last_open_idx {
            for m in &mut mask[start..] {
                *m = false;
            }
        }
    }
    mask
}

/// Returns `true` when the trigger at `idx` is covered by an
/// opt-out comment on its own line, the line directly above, or
/// two lines above with a single blank intermediate line.
fn is_optout_line(lines: &[&str], idx: usize) -> bool {
    if line_has_optout_comment(lines[idx]) {
        return true;
    }
    if idx >= 1 && line_has_optout_comment(lines[idx - 1]) {
        return true;
    }
    if idx >= 2 && lines[idx - 1].trim().is_empty() && line_has_optout_comment(lines[idx - 2]) {
        return true;
    }
    false
}

/// Matches the opt-out comment tolerantly so author typos (extra
/// internal whitespace, missing space after colon, tabs) do not
/// silently disable the opt-out. The adversarial agent caught that
/// a strict literal-contains check made the opt-out brittle: an
/// author wrote the comment slightly differently, saw it in the
/// plan file, assumed it worked, but the trigger still flagged
/// because the byte sequence did not match exactly.
fn optout_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)<!--\s*external-input-audit\s*:\s*not-a-tightening\s*-->")
            .expect("external-input-audit opt-out regex")
    })
}

fn line_has_optout_comment(line: &str) -> bool {
    optout_regex().is_match(line)
}

/// Returns `true` when the current sentence before `match_start`
/// contains a negation word. Skips phrases like "do not tighten"
/// or "never add a panic!" where the trigger is explicitly
/// negated — including cases where the negation precedes an
/// inner trigger via an intermediate verb (e.g. "do not tighten
/// to panic on X" — the whole sentence is negated, so both the
/// "tighten to panic" trigger AND the "panic on X" trigger are
/// suppressed).
///
/// The adversarial agent (PR #1056 Code Review) caught that a naive
/// `prefix.contains("not ")` check matches ANY occurrence of "not"
/// earlier in the line — so "This is not the only rule. We will
/// tighten to panic on empty." would silently pass the gate
/// because "not " appears in an unrelated earlier sentence. The
/// tightened check scopes the negation search to the CURRENT
/// sentence (prefix after the last `. ` boundary), so unrelated
/// negations in earlier sentences cannot bypass the gate while
/// sentence-level negations like "do not tighten X to Y" still
/// suppress every trigger in that sentence.
///
/// `"without "` was also removed from the negation set — it is
/// not a clean grammatical negator and its presence here caused
/// the same class of false-negative bypass. Discussion prose
/// should use the opt-out comment instead.
fn has_negation_prefix(line: &str, match_start: usize) -> bool {
    if match_start > line.len() {
        return false;
    }
    let prefix = line[..match_start].to_lowercase();
    // Scope the search to the current sentence: truncate at the
    // last sentence boundary (`. ` — period followed by space).
    // Negations in earlier sentences do not suppress triggers in
    // later sentences. If no boundary is found, the entire prefix
    // is the current sentence.
    let current_sentence = match prefix.rfind(". ") {
        Some(i) => &prefix[i + 2..],
        None => prefix.as_str(),
    };
    const NEGATIONS: &[&str] = &[
        "not",
        "never",
        "avoid",
        "don't",
        "won't",
        "cannot",
        "doesn't",
        "shouldn't",
        "mustn't",
    ];
    current_sentence
        .split_whitespace()
        .any(|token| NEGATIONS.contains(&token))
}

/// Collect the lookaround window around line `idx`. Mirrors the
/// scope-enumeration scanner: up to `WINDOW_LINES` raw lines in
/// each direction, stopping at `# ` or `## ` section boundaries
/// but crossing `### ` and deeper. Fenced-code lines are skipped
/// entirely.
///
/// Returns the window as a `Vec<String>` and the 0-based position
/// of the trigger line within that vector.
fn collect_window(lines: &[&str], idx: usize, fenced: &[bool]) -> (Vec<String>, usize) {
    let start = idx.saturating_sub(WINDOW_LINES);
    let end = (idx + WINDOW_LINES + 1).min(lines.len());

    let mut up_stop = start;
    if idx > 0 {
        for i in (start..idx).rev() {
            if is_section_boundary(lines[i]) {
                up_stop = i + 1;
                break;
            }
        }
    }

    let mut down_stop = end;
    for (i, line) in lines.iter().enumerate().take(end).skip(idx + 1) {
        if is_section_boundary(line) {
            down_stop = i;
            break;
        }
    }

    let mut window = Vec::with_capacity(down_stop.saturating_sub(up_stop));
    let mut trigger_rel = 0;
    for i in up_stop..down_stop {
        if fenced[i] {
            continue;
        }
        if i == idx {
            trigger_rel = window.len();
        }
        window.push(lines[i].to_string());
    }
    (window, trigger_rel)
}

/// Returns `true` for `# ` or `## ` headings. `### ` and deeper
/// are subsection headings and do NOT bound the window.
fn is_section_boundary(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("# ") || trimmed.starts_with("## ")
}

/// Detect an audit table within the window around the trigger.
///
/// An audit table has:
///
/// 1. A header row naming all four required columns (Caller /
///    Source / Classification / Handling, case-insensitive, with
///    aliases per the rule file).
/// 2. A separator row immediately below (pipes, dashes, colons,
///    whitespace).
/// 3. At least one data row.
///
/// The search runs over the next `WINDOW_NON_BLANK_LINES` and the
/// previous `WINDOW_NON_BLANK_LINES` non-blank lines from the
/// trigger. `trigger_line` and `trigger_match_end` are unused
/// today but retained so future enhancements (e.g. inline
/// same-line table detection) can extend this function without
/// changing the call sites.
fn audit_table_present(
    window: &[String],
    trigger_rel_idx: usize,
    _trigger_line: &str,
    _trigger_match_end: usize,
) -> bool {
    // Forward search.
    if scan_for_table_forward(window, trigger_rel_idx) {
        return true;
    }
    // Backward search.
    if scan_for_table_backward(window, trigger_rel_idx) {
        return true;
    }
    false
}

/// Walk forward from `trigger_rel_idx + 1`, counting non-blank
/// lines up to `WINDOW_NON_BLANK_LINES`. When a header row is
/// encountered, verify a separator row follows and at least one
/// data row follows the separator. Return `true` on success.
fn scan_for_table_forward(window: &[String], trigger_rel_idx: usize) -> bool {
    let mut non_blank = 0;
    let mut i = trigger_rel_idx + 1;
    while i < window.len() {
        let trimmed = window[i].trim();
        if trimmed.is_empty() {
            i += 1;
            continue;
        }
        if non_blank >= WINDOW_NON_BLANK_LINES {
            break;
        }
        non_blank += 1;
        if is_audit_header_row(trimmed) && find_separator_then_data(window, i).is_some() {
            return true;
        }
        i += 1;
    }
    false
}

/// Walk backward from the trigger, scanning up to
/// `WINDOW_NON_BLANK_LINES` non-blank lines. When a header row is
/// found, verify a separator row and at least one data row follow
/// it forward (header -> separator -> data is always top-down
/// regardless of the window walk direction). Return `true` if a
/// complete table is found above the trigger.
fn scan_for_table_backward(window: &[String], trigger_rel_idx: usize) -> bool {
    if trigger_rel_idx == 0 {
        return false;
    }
    let mut non_blank = 0;
    let mut i = trigger_rel_idx;
    while i > 0 {
        i -= 1;
        let trimmed = window[i].trim();
        if trimmed.is_empty() {
            continue;
        }
        if non_blank >= WINDOW_NON_BLANK_LINES {
            break;
        }
        non_blank += 1;
        if is_audit_header_row(trimmed) && find_separator_then_data(window, i).is_some() {
            return true;
        }
    }
    false
}

/// Given a header-row line index, check that the next non-blank
/// line is a separator and the line after that (also non-blank) is
/// a data row. Returns the data-row index on success.
fn find_separator_then_data(window: &[String], header_idx: usize) -> Option<usize> {
    let sep_idx = next_non_blank(window, header_idx + 1)?;
    if !is_separator_row(window[sep_idx].trim()) {
        return None;
    }
    let data_idx = next_non_blank(window, sep_idx + 1)?;
    // Sanity: data row must contain pipes (or at least one `|`) to
    // qualify as a table row. Accept rows with or without leading
    // pipes.
    if !window[data_idx].contains('|') {
        return None;
    }
    Some(data_idx)
}

fn next_non_blank(window: &[String], from: usize) -> Option<usize> {
    let mut i = from;
    while i < window.len() {
        if !window[i].trim().is_empty() {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// A header row carries the four required columns — Caller (or
/// Callsite), Source, Classification (or Class or Classif...), and
/// Handling (or Disposition). Comparison is case-insensitive, and
/// the row may or may not have leading/trailing pipes.
fn is_audit_header_row(line: &str) -> bool {
    let lower = line.to_lowercase();
    // Split on pipes and trim cells; accept tables with or without
    // leading/trailing pipes by filtering empty cells at the ends.
    let cells: Vec<String> = lower
        .split('|')
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect();
    if cells.len() < 4 {
        return false;
    }
    let has_caller = cells.iter().any(|c| c == "caller" || c == "callsite");
    let has_source = cells.iter().any(|c| c == "source");
    let has_class = cells
        .iter()
        .any(|c| c == "class" || c.starts_with("classif"));
    let has_handling = cells.iter().any(|c| c == "handling" || c == "disposition");
    has_caller && has_source && has_class && has_handling
}

/// A separator row is composed entirely of `|`, `-`, `:`, and
/// whitespace — the standard Markdown table separator. Alignment
/// markers (`:---`, `:---:`, `---:`) are implicitly covered
/// because `:` is in the allowed set.
fn is_separator_row(line: &str) -> bool {
    if line.is_empty() {
        return false;
    }
    // Must contain at least one `-` to be a separator (rules out
    // empty-pipe lines).
    if !line.contains('-') {
        return false;
    }
    line.chars()
        .all(|c| c == '|' || c == '-' || c == ':' || c.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn dummy_path() -> PathBuf {
        PathBuf::from("dummy.md")
    }

    // --- trigger_regex ---

    #[test]
    fn trigger_matches_tighten_to_panic() {
        assert!(trigger_regex().is_match("tighten FlowPaths::new to panic on empty"));
    }

    #[test]
    fn trigger_matches_add_assert() {
        assert!(trigger_regex().is_match("add an assert! that the branch is non-empty"));
    }

    #[test]
    fn trigger_matches_introduce_new_panic() {
        assert!(trigger_regex().is_match("introduce a new panic! in the constructor"));
    }

    #[test]
    fn trigger_matches_panic_on_empty() {
        assert!(trigger_regex().is_match("panic on empty input"));
    }

    #[test]
    fn trigger_matches_reject_empty() {
        assert!(trigger_regex().is_match("reject empty branches"));
    }

    #[test]
    fn trigger_matches_reject_invalid() {
        assert!(trigger_regex().is_match("reject invalid names"));
    }

    #[test]
    fn trigger_matches_assert_that() {
        assert!(trigger_regex().is_match("assert that the value is non-null"));
    }

    #[test]
    fn trigger_matches_enforce_validation_assertion() {
        assert!(trigger_regex().is_match("enforce a validation assertion on input"));
    }

    #[test]
    fn trigger_matches_require_invariant_check() {
        assert!(trigger_regex().is_match("require an invariant check in the constructor"));
    }

    #[test]
    fn trigger_matches_introduce_constructor_invariant() {
        assert!(trigger_regex().is_match("introduce a new constructor invariant"));
    }

    #[test]
    fn trigger_is_case_insensitive() {
        assert!(trigger_regex().is_match("Tighten the PANIC on empty"));
    }

    #[test]
    fn trigger_rejects_unrelated_prose() {
        assert!(!trigger_regex().is_match("the CI pipeline runs quickly"));
        assert!(!trigger_regex().is_match("panic in production is bad"));
    }

    #[test]
    fn direct_token_matches_assert_macro() {
        assert!(direct_token_regex().is_match("the body calls assert!(branch.len() > 0)"));
    }

    #[test]
    fn direct_token_matches_panic_macro() {
        assert!(direct_token_regex().is_match("the constructor uses panic!(\"bad input\")"));
    }

    #[test]
    fn direct_token_rejects_bare_assert_word() {
        // "assert" without the macro parenthesis is too noisy.
        assert!(!direct_token_regex().is_match("we assert this is correct"));
    }

    // --- scan: enumeration present (audit table nearby) ---

    #[test]
    fn scan_passes_forward_audit_table() {
        let content = "tighten FlowPaths::new to panic on empty\n\n| Caller | Source | Classification | Handling |\n|--------|--------|----------------|----------|\n| `current_branch()` | git subprocess | Trusted-but-external | `try_new` |\n";
        let v = scan(content, &dummy_path());
        assert!(
            v.is_empty(),
            "expected no violations with audit table, got {:?}",
            v
        );
    }

    #[test]
    fn scan_passes_backward_audit_table() {
        let content = "| Caller | Source | Classification | Handling |\n|--------|--------|----------------|----------|\n| `x::y` | git | Trusted-but-external | `try_new` |\n\nSo we tighten FlowPaths::new to panic on empty.\n";
        let v = scan(content, &dummy_path());
        assert!(
            v.is_empty(),
            "expected no violations with backward audit table, got {:?}",
            v
        );
    }

    #[test]
    fn scan_passes_without_leading_pipes() {
        let content = "tighten to panic on empty\n\nCaller | Source | Classification | Handling\n-------|--------|----------------|---------\n`x` | git | Trusted-but-external | `try_new`\n";
        let v = scan(content, &dummy_path());
        assert!(
            v.is_empty(),
            "tables without leading pipes must be accepted, got {:?}",
            v
        );
    }

    #[test]
    fn scan_passes_with_alignment_markers() {
        let content = "tighten to panic on empty\n\n| Caller | Source | Classification | Handling |\n|:-------|:------:|:--------------:|---------:|\n| `x` | git | Trusted-but-external | `try_new` |\n";
        let v = scan(content, &dummy_path());
        assert!(
            v.is_empty(),
            "alignment markers must be accepted, got {:?}",
            v
        );
    }

    #[test]
    fn scan_passes_with_callsite_header_alias() {
        let content = "tighten to panic on empty\n\n| Callsite | Source | Classification | Handling |\n|----------|--------|----------------|----------|\n| `x` | git | Trusted-but-external | `try_new` |\n";
        let v = scan(content, &dummy_path());
        assert!(v.is_empty(), "callsite alias must be accepted, got {:?}", v);
    }

    #[test]
    fn scan_passes_with_class_header_alias() {
        let content = "tighten to panic on empty\n\n| Caller | Source | Class | Handling |\n|--------|--------|-------|----------|\n| `x` | git | Trusted-but-external | `try_new` |\n";
        let v = scan(content, &dummy_path());
        assert!(v.is_empty(), "class alias must be accepted, got {:?}", v);
    }

    #[test]
    fn scan_passes_with_disposition_header_alias() {
        let content = "tighten to panic on empty\n\n| Caller | Source | Classification | Disposition |\n|--------|--------|----------------|-------------|\n| `x` | git | Trusted-but-external | `try_new` |\n";
        let v = scan(content, &dummy_path());
        assert!(
            v.is_empty(),
            "disposition alias must be accepted, got {:?}",
            v
        );
    }

    #[test]
    fn scan_accepts_tbd_row_content() {
        // Design choice (Risk 4): the gate validates table presence,
        // not row content. A TBD table signals author
        // irresponsibility to reviewers — the gate forces the
        // conversation to happen.
        let content = "tighten to panic on empty\n\n| Caller | Source | Classification | Handling |\n|--------|--------|----------------|----------|\n| TBD | TBD | TBD | TBD |\n";
        let v = scan(content, &dummy_path());
        assert!(
            v.is_empty(),
            "TBD-content table must be accepted by design, got {:?}",
            v
        );
    }

    // --- scan: violation surfaced ---

    #[test]
    fn scan_fails_tightening_without_table() {
        let content = "tighten FlowPaths::new to panic on empty branches.\n";
        let v = scan(content, &dummy_path());
        assert_eq!(v.len(), 1, "{:?}", v);
    }

    #[test]
    fn scan_fails_panic_on_without_table() {
        let content = "panic on empty strings.\n";
        let v = scan(content, &dummy_path());
        assert_eq!(v.len(), 1, "{:?}", v);
    }

    #[test]
    fn scan_fails_reject_empty_without_table() {
        let content = "reject empty inputs at the constructor boundary.\n";
        let v = scan(content, &dummy_path());
        assert_eq!(v.len(), 1, "{:?}", v);
    }

    #[test]
    fn scan_fails_direct_assert_macro_mention() {
        let content = "we add assert!(!branch.is_empty()) to the constructor body.\n";
        let v = scan(content, &dummy_path());
        assert!(
            !v.is_empty(),
            "direct assert! mention outside code must flag, got {:?}",
            v
        );
    }

    #[test]
    fn scan_fails_incomplete_table_columns() {
        // Only 3 columns — not a valid audit table.
        let content = "tighten to panic on empty\n\n| Caller | Source | Handling |\n|--------|--------|----------|\n| `x` | git | `try_new` |\n";
        let v = scan(content, &dummy_path());
        assert_eq!(
            v.len(),
            1,
            "incomplete-column table must not satisfy the gate, got {:?}",
            v
        );
    }

    #[test]
    fn scan_fails_header_only_no_data_row() {
        // Header + separator but no data row — not a real audit.
        let content = "tighten to panic on empty\n\n| Caller | Source | Classification | Handling |\n|--------|--------|----------------|----------|\n";
        let v = scan(content, &dummy_path());
        assert_eq!(
            v.len(),
            1,
            "header-only table must not satisfy the gate, got {:?}",
            v
        );
    }

    // --- scan: fenced block skip ---

    #[test]
    fn scan_skips_trigger_inside_fenced_block() {
        let content = "## Heading\n\n```text\ntighten FlowPaths::new to panic on empty\n```\n";
        let v = scan(content, &dummy_path());
        assert!(v.is_empty(), "fenced content must skip, got {:?}", v);
    }

    #[test]
    fn scan_skips_direct_token_inside_fenced_block() {
        let content = "## Heading\n\n```rust\nassert!(x > 0);\n```\n";
        let v = scan(content, &dummy_path());
        assert!(v.is_empty(), "fenced direct-token must skip, got {:?}", v);
    }

    #[test]
    fn scan_unterminated_fence_fails_open() {
        // Mirror the scope_enumeration failure mode: an unclosed
        // fence must not silence every subsequent trigger.
        let content = "```\ncode here\n\ntighten FlowPaths::new to panic on empty.\n";
        let v = scan(content, &dummy_path());
        assert_eq!(
            v.len(),
            1,
            "unterminated fence must not suppress downstream triggers, got {:?}",
            v
        );
    }

    // --- scan: opt-out ---

    #[test]
    fn scan_skips_opt_out_preceding_line() {
        let content = "<!-- external-input-audit: not-a-tightening -->\ntighten FlowPaths::new to panic on empty in the discussion context.\n";
        let v = scan(content, &dummy_path());
        assert!(
            v.is_empty(),
            "preceding-line opt-out must skip, got {:?}",
            v
        );
    }

    #[test]
    fn scan_skips_opt_out_same_line() {
        let content = "<!-- external-input-audit: not-a-tightening --> tighten to panic on empty\n";
        let v = scan(content, &dummy_path());
        assert!(v.is_empty(), "same-line opt-out must skip, got {:?}", v);
    }

    #[test]
    fn scan_opt_out_allows_single_blank_line_gap() {
        let content = "<!-- external-input-audit: not-a-tightening -->\n\ntighten FlowPaths::new to panic on empty\n";
        let v = scan(content, &dummy_path());
        assert!(
            v.is_empty(),
            "single-blank-line gap must allow opt-out, got {:?}",
            v
        );
    }

    #[test]
    fn scan_opt_out_rejects_multi_blank_gap() {
        let content =
            "<!-- external-input-audit: not-a-tightening -->\n\n\n\ntighten to panic on empty\n";
        let v = scan(content, &dummy_path());
        assert_eq!(
            v.len(),
            1,
            "multi-blank gap must not carry opt-out, got {:?}",
            v
        );
    }

    /// Regression: the opt-out must tolerate author-typo whitespace
    /// variations (extra internal whitespace, missing space after
    /// colon). The adversarial agent caught that a strict literal
    /// match silently disabled the opt-out on typos, misleading
    /// authors who saw their comment in the plan file and assumed
    /// it worked.
    #[test]
    fn scan_opt_out_tolerates_extra_internal_whitespace() {
        let content =
            "<!--  external-input-audit: not-a-tightening  -->\ntighten to panic on empty\n";
        let v = scan(content, &dummy_path());
        assert!(
            v.is_empty(),
            "opt-out with extra whitespace must still match, got {:?}",
            v
        );
    }

    #[test]
    fn scan_opt_out_tolerates_missing_space_after_colon() {
        let content = "<!-- external-input-audit:not-a-tightening -->\ntighten to panic on empty\n";
        let v = scan(content, &dummy_path());
        assert!(
            v.is_empty(),
            "opt-out without space after colon must match, got {:?}",
            v
        );
    }

    #[test]
    fn scan_opt_out_does_not_leak_to_later_lines() {
        let content = "<!-- external-input-audit: not-a-tightening -->\ntighten to panic on empty (covered by opt-out)\n\ntighten FlowPaths::new to panic on empty (not covered)\n";
        let v = scan(content, &dummy_path());
        assert_eq!(v.len(), 1, "later trigger must still flag, got {:?}", v);
    }

    // --- scan: section boundary ---

    #[test]
    fn scan_does_not_cross_h2_forward() {
        let content = "## A\n\ntighten to panic on empty.\n\n## B\n\n| Caller | Source | Classification | Handling |\n|--------|--------|----------------|----------|\n| `x` | git | Trusted | `try_new` |\n";
        let v = scan(content, &dummy_path());
        assert_eq!(
            v.len(),
            1,
            "forward table across ## must not count, got {:?}",
            v
        );
    }

    #[test]
    fn scan_does_not_cross_h2_backward() {
        let content = "## A\n\n| Caller | Source | Classification | Handling |\n|--------|--------|----------------|----------|\n| `x` | git | Trusted | `try_new` |\n\n## B\n\ntighten to panic on empty.\n";
        let v = scan(content, &dummy_path());
        assert_eq!(
            v.len(),
            1,
            "backward table across ## must not count, got {:?}",
            v
        );
    }

    #[test]
    fn scan_crosses_h3_heading() {
        let content = "## Section\n\ntighten to panic on empty\n\n### Subsection\n\n| Caller | Source | Classification | Handling |\n|--------|--------|----------------|----------|\n| `x` | git | Trusted | `try_new` |\n";
        let v = scan(content, &dummy_path());
        assert!(
            v.is_empty(),
            "### subsection must not block the window, got {:?}",
            v
        );
    }

    // --- scan: negation skip ---

    #[test]
    fn scan_skips_do_not_tighten() {
        let content = "do not tighten FlowPaths::new to panic on slash-containing branches.\n";
        let v = scan(content, &dummy_path());
        assert!(v.is_empty(), "negation must skip, got {:?}", v);
    }

    #[test]
    fn scan_skips_never_add_panic() {
        let content = "never add a panic! to hook entry points.\n";
        let v = scan(content, &dummy_path());
        assert!(v.is_empty(), "negation must skip, got {:?}", v);
    }

    /// Regression: a "not" in an unrelated earlier clause must NOT
    /// suppress a downstream trigger. The adversarial agent caught
    /// that `prefix.contains("not ")` was matching unrelated earlier
    /// uses of "not", producing a silent gate bypass. The tightened
    /// check only accepts a negation as the word immediately before
    /// the trigger.
    #[test]
    fn scan_does_not_skip_unrelated_not_earlier_in_line() {
        let content = "This is not the only rule. We will tighten to panic on empty input.\n";
        let v = scan(content, &dummy_path());
        assert!(
            !v.is_empty(),
            "unrelated 'not' earlier in the line must not suppress the trigger, got {:?}",
            v
        );
    }

    /// Same class as above: "without" earlier in the line used to
    /// suppress the trigger. "without " was removed from the
    /// negation set entirely because it is not a clean grammatical
    /// negator — opt-out comments are the correct mechanism for
    /// discussion prose.
    #[test]
    fn scan_does_not_skip_unrelated_without_earlier_in_line() {
        let content =
            "We proceed without intermediate checks; we then tighten to panic on empty.\n";
        let v = scan(content, &dummy_path());
        assert!(
            !v.is_empty(),
            "unrelated 'without' earlier in the line must not suppress the trigger, got {:?}",
            v
        );
    }

    // --- compute_fenced_mask edge cases ---

    #[test]
    fn scan_trigger_after_unclosed_fence_detected() {
        // An unclosed fence must fail open — content after the fence
        // is NOT masked, so triggers are still detected.
        let content = "```\nfenced content\ntighten FlowPaths::new to panic on empty.\n";
        let v = scan(content, &dummy_path());
        assert!(
            !v.is_empty(),
            "trigger after unclosed fence must be detected (fail-open)"
        );
    }

    // --- is_audit_header_row edge cases ---

    #[test]
    fn audit_header_with_leading_empty_cells() {
        // Leading pipe creates an empty first cell — the filter in
        // is_audit_header_row must discard it and still match 4 columns.
        assert!(is_audit_header_row(
            "| | Caller | Source | Classification | Handling |"
        ));
    }

    // --- is_separator_row edge cases ---

    #[test]
    fn separator_row_rejects_pipes_only() {
        // A row of only pipes and spaces (no dashes) is not a separator.
        assert!(!is_separator_row("| | | | |"));
    }

    // --- next_non_blank edge cases ---

    #[test]
    fn next_non_blank_returns_none_at_eof() {
        let window: Vec<String> = vec![
            "trigger line".to_string(),
            "   ".to_string(),
            "".to_string(),
            "  ".to_string(),
        ];
        assert_eq!(next_non_blank(&window, 1), None);
    }

    // --- scan_for_table window exhaustion ---

    #[test]
    fn forward_scan_exhausts_window_no_table() {
        // Fill the window with enough non-blank non-table lines to
        // exceed WINDOW_NON_BLANK_LINES, then place a valid table
        // past the window. The forward scan should give up before
        // reaching the table.
        let mut lines: Vec<String> = Vec::new();
        lines.push("tighten to panic on empty.".to_string()); // trigger at index 0
        for i in 0..(WINDOW_NON_BLANK_LINES + 5) {
            lines.push(format!("filler line {}", i));
        }
        lines.push("| Caller | Source | Classification | Handling |".to_string());
        lines.push("|--------|--------|----------------|----------|".to_string());
        lines.push("| a | b | c | d |".to_string());

        assert!(!scan_for_table_forward(&lines, 0));
    }

    #[test]
    fn backward_scan_exhausts_window_no_table() {
        // Place a valid table at the top, then enough filler to
        // exceed WINDOW_NON_BLANK_LINES, then the trigger. The
        // backward scan should give up before reaching the table.
        let mut lines: Vec<String> = Vec::new();
        lines.push("| Caller | Source | Classification | Handling |".to_string());
        lines.push("|--------|--------|----------------|----------|".to_string());
        lines.push("| a | b | c | d |".to_string());
        for i in 0..(WINDOW_NON_BLANK_LINES + 5) {
            lines.push(format!("filler line {}", i));
        }
        let trigger_idx = lines.len();
        lines.push("tighten to panic on empty.".to_string());

        assert!(!scan_for_table_backward(&lines, trigger_idx));
    }

    // --- has_negation_prefix edge cases ---

    #[test]
    fn negation_prefix_out_of_bounds_returns_false() {
        // match_start exceeds line length — the bounds guard returns false.
        assert!(!has_negation_prefix("short", 100));
    }

    // --- integration: Violation fields ---

    #[test]
    fn scan_populates_violation_fields() {
        let content = "line one\ntighten FlowPaths::new to panic on empty.\nline three\n";
        let v = scan(content, &dummy_path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 2);
        // The direct-action pattern `\bpanic\s+on\s+\w+` wins on
        // the example line because "FlowPaths::new" is not in the
        // verb+noun alternation's allowed noun set. Either trigger
        // is sufficient for the gate's purpose — flag the line.
        assert!(v[0].phrase.to_lowercase().contains("panic on"));
        assert!(v[0].context.contains("tighten FlowPaths::new"));
        assert_eq!(v[0].file, PathBuf::from("dummy.md"));
    }
}
