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
//! This module is the shared scanner used by three callers:
//!
//! - `bin/flow plan-check` — gates Plan-phase completion on
//!   `.flow-states/<branch>-plan.md`. The standard plan path invokes
//!   it from `skills/flow-plan/SKILL.md` Step 4 before
//!   `phase-transition --action complete`.
//! - `src/plan_extract.rs` extracted path — runs the same scanner
//!   against the promoted plan content before `complete_plan_phase`
//!   for pre-decomposed issues (bypasses the skill entirely).
//! - `src/plan_extract.rs` resume path — runs the scanner against an
//!   existing plan file on re-invocation, so a plan the user edited
//!   after a prior violation must pass before the phase can complete.
//!
//! Tests live at `tests/scope_enumeration.rs` per
//! `.claude/rules/test-placement.md` — corpus contract tests over
//! the committed prose (`CLAUDE.md`, `.claude/rules/*.md`,
//! `skills/**/SKILL.md`, `.claude/skills/**/SKILL.md`) plus unit
//! tests exercising trigger matching, enumeration detection,
//! fenced-block skipping, opt-out comments, negation prefixes, and
//! window boundaries. All tests drive through the public `scan`
//! function; no inline `#[cfg(test)]` tests in this file.
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
//! The comment applies to its own line, to the line directly below
//! (no blank lines between), and to a line two positions below when
//! the intermediate line is blank. Any larger gap is considered
//! unrelated — the rule is "the line it sits on and the next
//! non-blank line, with at most one blank line separating them."

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

/// Number of NON-BLANK lines to scan in each direction when searching
/// for a nearby enumeration. Blank lines do not consume the budget
/// so ordinary Markdown paragraph breaks between a trigger and its
/// list do not narrow the window.
///
/// The value was chosen to span observed real-world enumerations in
/// the current tree (`tool-dispatch.md:26` — 4 non-blank lines back,
/// `rust-patterns.md:175` — 2 non-blank lines forward) with
/// generous headroom for multi-line bullet continuations.
///
/// Note that the search also respects Markdown section boundaries:
/// the window stops at `# ` or `## ` headings (which signal a new
/// top-level topic) but crosses `### ` and deeper (subsections within
/// the same topic). This rule lets scanning span multi-step skill
/// sections without truncation.
const WINDOW_NON_BLANK_LINES: usize = 8;

/// Legacy constant retained for the structural check inside
/// `collect_window` — caps the raw line distance the walker looks at
/// even when no enumeration is found, so pathologically long
/// sections don't pull the scanner into quadratic behavior.
const WINDOW_LINES: usize = 30;

/// The trigger pattern: a universal quantifier followed by a
/// code-family noun. Up to TWO optional single-token adjectives are
/// allowed between the quantifier and the noun, so phrasings like
/// "every sibling entry point" and "every single sibling entry
/// point" both match. The noun set is closed and curated. `\b` word
/// boundaries on both ends prevent substring matches, and the final
/// `s?` permits plural forms ("all runners", "every subcommands").
///
/// When adding a new noun:
///
/// 1. Add it to this alternation list.
/// 2. Add a `trigger_matches_<noun>` unit test in
///    `tests/scope_enumeration.rs` (driven through the public `scan`
///    function — the private `trigger_regex` helper is not exposed).
/// 3. Update the vocabulary list in `.claude/rules/scope-enumeration.md`
///    so the rule file and the scanner stay in sync.
pub const SCOPE_TRIGGER_PATTERN: &str = r"(?i)\b(?:every|all|each)\s+(?:[\w\-]+\s+){0,2}(?:state[\s\-]+mutator|CLI\s+(?:variant|entry)|entry\s+point|dispatch\s+path|subcommand|runner|callsite|caller|mutator|handler)s?\b";

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
            let (window, trigger_rel_idx) = collect_window(&lines, idx, &fenced);
            if !enumeration_present(&window, trigger_rel_idx, line, m.end()) {
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
///
/// When the source contains an unterminated fence (an opening
/// ```` ``` ```` with no matching close), the scanner cannot know
/// where the intended block ends. Treating everything below the
/// stray fence as "fenced" would silently suppress every violation
/// on every line past a typo. Instead, the mask is reverted from the
/// last unclosed fence onward: a missing close fence fails open
/// (scan continues) rather than fails closed (silent mask).
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
    // Unclosed fence at EOF: rewind the mask from the stray opener
    // onward so a typo doesn't silently suppress all violations past
    // it. By construction `last_open_idx.is_some() <=> in_block`, so
    // checking one condition is sufficient — gating on both just
    // added a dead branch for coverage.
    if let Some(start) = last_open_idx {
        for m in &mut mask[start..] {
            *m = false;
        }
    }
    mask
}

/// Returns `true` when line `idx` is covered by an opt-out comment.
/// The comment must be on one of three positions:
///
/// - the current line itself (same-line comment followed by prose)
/// - the line directly above (no blank lines between)
/// - two lines above, with a single blank intermediate line
///
/// Any larger gap is considered unrelated. This implements the
/// rule file's "next non-blank line, with at most one blank line
/// separating them" contract — a stray opt-out at the top of a
/// section cannot silence arbitrary triggers further down.
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

/// Returns the enumeration search window around line `idx` and the
/// 0-based index of the trigger line within the returned window.
///
/// The window spans up to `WINDOW_LINES` raw lines in each direction,
/// stopping at `#` or `##` Markdown headings (which signal a new
/// top-level topic) but crossing `###` and deeper (subsections within
/// the same topic). Fenced-code-block lines are skipped entirely —
/// they cannot contain valid enumeration bullets because Markdown
/// parsers treat them as literal text.
///
/// The returned `trigger_rel_idx` is the position of the trigger line
/// within the returned window slice. Callers use this to distinguish
/// "before trigger" lines from "after trigger" lines when searching
/// for forward and backward bullet-list enumerations.
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

    let mut window = Vec::with_capacity(down_stop - up_stop);
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

/// Returns `true` for `# ` or `## ` headings. `### ` and deeper are
/// NOT boundaries — the scanner's window must span multi-step skill
/// sections without truncation.
fn is_section_boundary(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("# ") || trimmed.starts_with("## ")
}

/// Checks whether a structured named enumeration accompanies the
/// trigger at `trigger_rel_idx` within the window. Returns `true`
/// when one of three patterns holds:
///
/// - **Inline list after the trigger** — the trigger line itself
///   contains ≥ 3 backtick-quoted spans after the trigger match
///   position. This catches colon-delimited lists and parenthetical
///   lists on the same line (e.g. `CLAUDE.md:112`'s "every subcommand
///   ... : `ci`, `build`, `lint`, `format`, `test` ...").
/// - **Forward bullet list** — within the next `WINDOW_NON_BLANK_LINES`
///   non-blank lines after the trigger, at least one line starts with
///   `-` or `*` (a bullet) AND the total backtick count in those
///   lines is ≥ 2. Multi-line bullet continuations count toward the
///   total. Catches `rust-patterns.md:175`-style lists.
/// - **Backward bullet list** — symmetric to the forward case, for
///   lists that precede the trigger (e.g. `tool-dispatch.md:26` where
///   the bullets appear above the "at each callsite" phrase).
///
/// This is strictly more restrictive than "count backticks anywhere
/// in the window." Two unrelated inline code references near a
/// universal claim no longer satisfy the heuristic — a real
/// structured enumeration (inline list OR bullet list) is required.
/// The rule file's motivating incidents all used structured lists,
/// so this heuristic matches the intended contract.
fn enumeration_present(
    window: &[String],
    trigger_rel_idx: usize,
    trigger_line: &str,
    trigger_match_end: usize,
) -> bool {
    // Pattern 1: inline list after the trigger on the same line.
    // A colon or parenthetical enumeration typically contains every
    // sibling inline, so ≥ 3 backticks after the match position is
    // a strong signal. `trigger_match_end` is a regex match end into
    // `trigger_line`, so it is always ≤ `trigger_line.len()` and the
    // slice is infallible — no defensive bounds check is needed.
    let after = &trigger_line[trigger_match_end..];
    if backtick_regex().find_iter(after).count() >= 3 {
        return true;
    }

    // Pattern 2: forward bullet list.
    if has_structured_list_forward(window, trigger_rel_idx) {
        return true;
    }

    // Pattern 3: backward bullet list.
    if has_structured_list_backward(window, trigger_rel_idx) {
        return true;
    }

    false
}

/// Searches forward from `trigger_rel_idx` for a bullet list with
/// enumerated backtick identifiers. Requires at least one bullet
/// line (`-` or `*` prefix) and a total backtick count ≥ 2 within
/// the next `WINDOW_NON_BLANK_LINES` non-blank lines.
fn has_structured_list_forward(window: &[String], trigger_rel_idx: usize) -> bool {
    let mut non_blank = 0;
    let mut bullet_seen = false;
    let mut backtick_count = 0;
    for line in window.iter().skip(trigger_rel_idx + 1) {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        if non_blank >= WINDOW_NON_BLANK_LINES {
            break;
        }
        non_blank += 1;
        if is_bullet_line(trimmed) {
            bullet_seen = true;
        }
        backtick_count += backtick_regex().find_iter(line.as_str()).count();
        if bullet_seen && backtick_count >= 2 {
            return true;
        }
    }
    false
}

/// Searches backward from `trigger_rel_idx` for a bullet list with
/// enumerated backtick identifiers. Same shape as the forward
/// search — the symmetry is intentional so both directions produce
/// consistent outcomes.
fn has_structured_list_backward(window: &[String], trigger_rel_idx: usize) -> bool {
    let mut non_blank = 0;
    let mut bullet_seen = false;
    let mut backtick_count = 0;
    for line in window.iter().take(trigger_rel_idx).rev() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        if non_blank >= WINDOW_NON_BLANK_LINES {
            break;
        }
        non_blank += 1;
        if is_bullet_line(trimmed) {
            bullet_seen = true;
        }
        backtick_count += backtick_regex().find_iter(line.as_str()).count();
        if bullet_seen && backtick_count >= 2 {
            return true;
        }
    }
    false
}

fn is_bullet_line(trimmed: &str) -> bool {
    trimmed.starts_with("- ") || trimmed.starts_with("* ")
}
