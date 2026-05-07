//! Deletion-sweep scanner — Plan-phase Gate 2.
//!
//! When a plan proposes deleting, removing, or renaming a named
//! identifier, the plan must enumerate the per-identifier grep
//! sweep — see `.claude/rules/docs-with-behavior.md` "Scope
//! Enumeration (Rename Side)". The sweep names every file that
//! contains the old identifier so the Code phase has a checklist
//! to update.
//!
//! ## Trigger vocabulary
//!
//! Closed and curated. The trigger fires on a single line
//! containing a delete/remove/rename/replace verb paired with a
//! backtick-quoted identifier of length ≥ 10 characters. The
//! length filter mirrors `duplicate_test_coverage`'s identifier
//! filter so common-word identifiers like `config`, `foo` do not
//! produce false positives.
//!
//! ## Compliance proof
//!
//! Within `WINDOW_NON_BLANK_LINES` non-blank lines forward of the
//! trigger, the scanner looks for either:
//!
//! - A bullet list naming files (lines starting with `-` or `*`
//!   that contain at least one backtick-quoted token).
//! - An "Exploration" table or heading.
//!
//! Either piece of evidence proves the author has documented the
//! sweep result. Absence produces a violation whose `phrase`
//! field names the proposed-for-deletion identifier.
//!
//! ## Identifier cap
//!
//! The scanner stops after `MAX_IDENTIFIERS_PER_PLAN` (20) unique
//! identifiers in a single plan to bound walk cost on
//! pathologically large plans.
//!
//! ## Opt-outs
//!
//! `<!-- deletion-sweep: not-a-deletion -->` on the trigger line,
//! the line directly above, or two lines above with a single
//! blank line in between. Same walk-back grammar as the sibling
//! scanners.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

/// Number of non-blank lines scanned forward when searching for
/// the sweep evidence.
const WINDOW_NON_BLANK_LINES: usize = 12;

/// Maximum unique identifiers extracted from a single plan to
/// bound walk cost.
const MAX_IDENTIFIERS_PER_PLAN: usize = 20;

/// Trigger pattern — delete/remove/rename/replace verb followed
/// by a backtick-quoted identifier on the same line.
///
/// **Case-sensitive**. Plan tasks use capitalized imperatives
/// ("Remove `foo`", "Delete `bar`"); lowercase forms in prose
/// ("removes the directory", "deleting these entries describes
/// the behavior") describe existing behavior rather than
/// proposing a new deletion. The case-sensitive pattern keeps
/// the trigger conservative so the corpus contract test does
/// not balloon with false positives from prose discussing
/// existing system behavior.
///
/// The verbs are the closed set `(Remove|Removes|Removing|
/// Delete|Deletes|Deleting|Rename|Renames|Renaming|Replace|
/// Replaces|Replacing)`. The identifier is matched by
/// `ident_regex` (≥ 10 chars) inside backticks later in the
/// line.
pub const TRIGGER_PATTERN: &str = r"(?x)
    \b(?:Removes?|Removing|Deletes?|Deleting|Renames?|Renaming|Replaces?|Replacing)
    \b
";

fn trigger_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(TRIGGER_PATTERN).expect("deletion-sweep trigger regex"))
}

/// Identifier filter — backtick-quoted identifiers of length
/// ≥ 10. Mirrors `duplicate_test_coverage` shape.
fn ident_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"`([^`\n]+)`").expect("deletion-sweep ident regex"))
}

fn ident_shape_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^[a-z_][a-z0-9_:./]{9,}$").expect("deletion-sweep ident shape regex")
    })
}

/// A violation: a delete/rename trigger without nearby sweep
/// evidence.
#[derive(Debug, Clone)]
pub struct Violation {
    pub file: PathBuf,
    pub line: usize,
    pub phrase: String,
    pub context: String,
    /// The proposed-for-deletion identifier extracted from
    /// backticks on the trigger line.
    pub identifier: String,
}

/// Scan `content` for delete/rename triggers without nearby sweep
/// evidence.
pub fn scan(content: &str, source: &Path) -> Vec<Violation> {
    let lines: Vec<&str> = content.lines().collect();
    let fenced = compute_fenced_mask(&lines);
    let mut violations = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for (idx, line) in lines.iter().enumerate() {
        if seen.len() >= MAX_IDENTIFIERS_PER_PLAN {
            break;
        }
        if fenced[idx] {
            continue;
        }
        if is_optout_line(&lines, idx) {
            continue;
        }
        let trig = match trigger_regex().find(line) {
            Some(m) => m,
            None => continue,
        };
        if has_negation_prefix(line, trig.start()) {
            continue;
        }
        // Find a candidate identifier in the line — backtick-quoted
        // token of length ≥ 10.
        let candidate = match extract_candidate(line) {
            Some(c) => c,
            None => continue,
        };
        if !seen.insert(candidate.clone()) {
            continue;
        }
        if !sweep_evidence_present(&lines, idx, &fenced) {
            violations.push(Violation {
                file: source.to_path_buf(),
                line: idx + 1,
                phrase: trig.as_str().to_string(),
                context: (*line).to_string(),
                identifier: candidate,
            });
        }
    }

    violations
}

/// Extract the first backtick-quoted identifier on the line
/// matching the shape regex.
fn extract_candidate(line: &str) -> Option<String> {
    for cap in ident_regex().captures_iter(line) {
        // `ident_regex()` is `r"`([^`\n]+)`"` — capture group 1
        // always present on a successful match, so `cap.get(1)`
        // cannot return None. `.expect` does not create a
        // testable branch per
        // `.claude/rules/testability-means-simplicity.md`.
        let inside = cap
            .get(1)
            .expect("ident regex has capture group 1")
            .as_str()
            .trim();
        if ident_shape_regex().is_match(inside) {
            return Some(inside.to_string());
        }
    }
    None
}

/// Check whether sweep evidence appears within the window forward
/// of the trigger. Evidence is either a bullet list with
/// backtick-quoted items OR an Exploration heading.
fn sweep_evidence_present(lines: &[&str], trigger_idx: usize, fenced: &[bool]) -> bool {
    let mut non_blank = 0;
    let mut bullet_count = 0;
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
        if lower.contains("exploration") {
            return true;
        }
        let leftmost = line.trim_start();
        if (leftmost.starts_with('-') || leftmost.starts_with('*')) && line.contains('`') {
            bullet_count += 1;
            if bullet_count >= 2 {
                return true;
            }
        }
        // Inline table row with backticks also counts as evidence.
        if trimmed.starts_with('|') && line.contains('`') {
            return true;
        }
    }
    false
}

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
        Regex::new(r"(?i)<!--\s*deletion-sweep\s*:\s*not-a-deletion\s*-->")
            .expect("deletion-sweep opt-out regex")
    })
}

fn line_has_optout_comment(line: &str) -> bool {
    optout_regex().is_match(line)
}

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
