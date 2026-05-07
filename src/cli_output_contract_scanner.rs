//! CLI output contract scanner — Plan-phase Gate 1.
//!
//! When a plan proposes a new `bin/flow` subcommand or a new flag on a
//! `bin/<tool>` stub whose stdout, exit code, or stderr is consumed by
//! skills, agents, or other subcommands, the plan must include a
//! four-item contract block within the trigger's window. The four
//! items are output format, exit codes, error messages, and fallback
//! behavior — see `.claude/rules/cli-output-contracts.md`.
//!
//! This module is the shared scanner used by three callers:
//!
//! - `bin/flow plan-check` — gates Plan-phase completion.
//! - `src/plan_extract.rs` extracted path — runs the same scanner
//!   against the promoted plan content for pre-decomposed issues.
//! - `src/plan_extract.rs` resume path — re-runs against the existing
//!   plan file on re-invocation.
//!
//! ## Trigger vocabulary
//!
//! Closed and curated. The trigger fires on a single line containing
//! BOTH a "new contract surface" verb-target ("new flag", "add a
//! subcommand", "introduce ... flag", etc.) AND an output-kind keyword
//! ("output", "stdout", "stderr", "exit code", "consumed", "json",
//! "parses"). Co-occurrence on the same line scopes the trigger to
//! prose that proposes a consumed-output surface, not generic mentions
//! of either word.
//!
//! ## Compliance proof
//!
//! Within `WINDOW_NON_BLANK_LINES` non-blank lines forward of the
//! trigger, the scanner looks for prose mentions of all four contract
//! items: output format, exit codes, error messages (or stderr), and
//! fallback behavior. A trigger with one or more missing items
//! produces a violation whose `missing_items` field names the absent
//! items.
//!
//! ## Opt-outs
//!
//! `<!-- cli-output-contracts: not-a-new-flag -->` on the trigger
//! line, the line directly above, or two lines above with a single
//! blank line in between. Same walk-back grammar as the sibling
//! scanners.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

/// Number of non-blank lines scanned forward when searching for the
/// four-item contract block. Matches the rule's stated proximity for
/// the contract block.
const WINDOW_NON_BLANK_LINES: usize = 12;

/// Trigger pattern — a co-occurrence on the same line of a
/// "new contract surface" verb-target and an output-kind keyword.
///
/// Verb-targets: `(?:new|add|adds|adding|introduce|introduces|
/// introducing|extend|extends|extending|implement|implements)\s+
/// (?:a\s+|an\s+)?(?:new\s+)?(?:flag|subcommand)`.
/// Output kinds: `output`, `stdout`, `stderr`, `exit code`,
/// `exit codes`, `consumed`, `json`, `parses`, `branches on`.
///
/// The regex matches the verb-target. Output-kind co-occurrence is
/// validated separately by `line_has_output_kind` so the regex stays
/// readable and the negation/fenced-block logic operates on the
/// verb-target span.
pub const TRIGGER_PATTERN: &str = r"(?ix)
    \b(?:new|adds?|adding|introduces?|introducing|extends?|extending|implements?)\s+
    (?:a\s+|an\s+)?(?:new\s+)?
    (?:flag|subcommand)
";

fn trigger_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(TRIGGER_PATTERN).expect("cli-output-contracts trigger regex"))
}

/// Output-kind keywords. Matched case-insensitively.
const OUTPUT_KIND_KEYWORDS: &[&str] = &[
    "output",
    "stdout",
    "stderr",
    "exit code",
    "exit codes",
    "consumed",
    "json",
    "parses",
    "branches on",
];

/// Returns true when the line contains an output-kind keyword.
fn line_has_output_kind(line: &str) -> bool {
    let lower = line.to_lowercase();
    OUTPUT_KIND_KEYWORDS.iter().any(|kw| lower.contains(kw))
}

/// Contract item identifiers. Order is the canonical order used in
/// violation responses.
const CONTRACT_ITEMS: &[(&str, &[&str])] = &[
    (
        "output_format",
        &["output format", "json output", "stdout format", "format:"],
    ),
    ("exit_codes", &["exit code", "exit codes", "exit status"]),
    (
        "error_messages",
        &["error message", "error messages", "stderr", "error class"],
    ),
    ("fallback", &["fallback", "default value", "fail closed"]),
];

/// A violation: a trigger without nearby coverage of all four contract
/// items.
#[derive(Debug, Clone)]
pub struct Violation {
    pub file: PathBuf,
    pub line: usize,
    pub phrase: String,
    pub context: String,
    /// Subset of CONTRACT_ITEMS keys that were not found within the
    /// trigger's forward window.
    pub missing_items: Vec<String>,
}

/// Scan `content` for new-flag / new-subcommand triggers without a
/// nearby four-item contract block. `source` is the path used to
/// populate `Violation::file`.
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
        if !line_has_output_kind(line) {
            continue;
        }

        let mut matches: Vec<(usize, usize, String)> = Vec::new();
        for m in trigger_regex().find_iter(line) {
            matches.push((m.start(), m.end(), m.as_str().to_string()));
        }
        matches.sort_by_key(|(start, _, _)| *start);
        matches.dedup_by_key(|(start, _, _)| *start);

        for (start, _end, phrase) in matches {
            if has_negation_prefix(line, start) {
                continue;
            }
            let missing = missing_contract_items(&lines, idx, &fenced);
            if !missing.is_empty() {
                violations.push(Violation {
                    file: source.to_path_buf(),
                    line: idx + 1,
                    phrase,
                    context: (*line).to_string(),
                    missing_items: missing,
                });
                // Only one violation per line — multiple verb-target
                // matches on a single trigger line are still one
                // missing-contract claim.
                break;
            }
        }
    }

    violations
}

/// Returns the list of contract item keys that are NOT covered by
/// prose within the next `WINDOW_NON_BLANK_LINES` non-blank lines
/// forward of the trigger. An empty Vec means all four items were
/// found and the trigger is compliant.
fn missing_contract_items(lines: &[&str], trigger_idx: usize, fenced: &[bool]) -> Vec<String> {
    let mut found = vec![false; CONTRACT_ITEMS.len()];
    let mut non_blank = 0;
    for (i, line) in lines.iter().enumerate().skip(trigger_idx + 1) {
        if fenced[i] {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if non_blank >= WINDOW_NON_BLANK_LINES {
            break;
        }
        non_blank += 1;
        let lower = line.to_lowercase();
        for (item_idx, (_key, keywords)) in CONTRACT_ITEMS.iter().enumerate() {
            if found[item_idx] {
                continue;
            }
            if keywords.iter().any(|kw| lower.contains(kw)) {
                found[item_idx] = true;
            }
        }
    }
    CONTRACT_ITEMS
        .iter()
        .zip(found.iter())
        .filter(|(_, &f)| !f)
        .map(|((key, _), _)| (*key).to_string())
        .collect()
}

/// Returns `true` for every line index inside or on a fenced code
/// block (` ``` `). Unclosed fences fail open per the sibling scanners.
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

/// Returns `true` when the trigger at `idx` is covered by an opt-out
/// comment on its own line, the line directly above, or two lines
/// above with a single blank intermediate line.
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

fn optout_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)<!--\s*cli-output-contracts\s*:\s*not-a-new-flag\s*-->")
            .expect("cli-output-contracts opt-out regex")
    })
}

fn line_has_optout_comment(line: &str) -> bool {
    optout_regex().is_match(line)
}

/// Returns `true` when the current sentence before `match_start`
/// contains a negation token, suppressing the trigger. Sentence scope
/// matches `external_input_audit.rs::has_negation_prefix`.
fn has_negation_prefix(line: &str, match_start: usize) -> bool {
    let prefix = line[..match_start].to_lowercase();
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
