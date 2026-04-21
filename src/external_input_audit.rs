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
    let mut open_at: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("```") {
            open_at = match open_at {
                Some(_) => None,
                None => Some(i),
            };
            mask[i] = true;
            continue;
        }
        mask[i] = open_at.is_some();
    }
    if let Some(start) = open_at {
        for m in &mut mask[start..] {
            *m = false;
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
/// because `:` is in the allowed set. Callers pass the trimmed
/// cell of a row already known to be non-blank (via `next_non_blank`),
/// so no empty-string guard is needed — a row without `-` fails
/// the dash-presence check below.
fn is_separator_row(line: &str) -> bool {
    // Must contain at least one `-` to be a separator (rules out
    // empty-pipe lines and any other separator-shaped content that
    // lacks dashes).
    if !line.contains('-') {
        return false;
    }
    line.chars()
        .all(|c| c == '|' || c == '-' || c == ':' || c.is_whitespace())
}
