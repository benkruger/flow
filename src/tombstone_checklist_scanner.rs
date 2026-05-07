//! Tombstone checklist scanner — Plan-phase Gate 3.
//!
//! When a plan proposes adding a tombstone test, the plan must
//! include a five-item checklist enumerating: (1) protection
//! target, (2) assertion kind (literal/structural), (3) stability
//! argument, (4) bypass list, (5) file-resurrection pair. See
//! `.claude/rules/tombstone-tests.md` "Plan-phase responsibility".
//!
//! ## Trigger vocabulary
//!
//! The trigger fires when plan prose proposes a tombstone — a
//! line containing a tombstone noun phrase ("tombstone test",
//! "tombstone-test", "tombstone tests", "tombstone-tests",
//! "tombstone for") AND a propose-verb (`add`, `ship`,
//! `introduce`, `include`) on the same line. The propose-verb
//! requirement keeps the trigger conservative — prose discussing
//! tombstones generally ("tombstones live in `tests/`") does not
//! fire.
//!
//! ## Compliance proof
//!
//! Within `WINDOW_NON_BLANK_LINES` non-blank lines forward of the
//! trigger, the scanner looks for prose mentions of all five
//! checklist items. A trigger with one or more missing items
//! produces a violation whose `missing_items` field names the
//! absent ones.
//!
//! ## Opt-outs
//!
//! `<!-- tombstone-checklist: not-a-tombstone -->` on the trigger
//! line, the line directly above, or two lines above with one
//! blank line between. Same walk-back grammar as the sibling
//! scanners.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

const WINDOW_NON_BLANK_LINES: usize = 20;

/// Trigger pattern — a line containing both "tombstone"
/// (case-insensitive) and a propose-verb (`add`, `ship`,
/// `introduce`, `include`).
pub const TRIGGER_PATTERN: &str = r"(?ix)
    \b(?:add|ship|introduce|include)\b
";

fn trigger_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(TRIGGER_PATTERN).expect("tombstone-checklist trigger regex"))
}

/// The five checklist items. Each entry is (key, prose-keyword
/// patterns). The scanner counts an item as "present" when ANY
/// pattern in its list appears in nearby prose.
const CHECKLIST_ITEMS: &[(&str, &[&str])] = &[
    (
        "protection_target",
        &["protection target", "protect against", "protected target"],
    ),
    (
        "assertion_kind",
        &["literal", "structural", "assertion kind", "byte-substring"],
    ),
    (
        "stability",
        &["stability", "stable", "concat!", "stability argument"],
    ),
    ("bypass_list", &["bypass", "bypasses", "bypass list"]),
    (
        "file_resurrection",
        &[
            "file-resurrection",
            "file resurrection",
            "file-existence",
            "file existence",
            "resurrection pair",
        ],
    ),
];

#[derive(Debug, Clone)]
pub struct Violation {
    pub file: PathBuf,
    pub line: usize,
    pub phrase: String,
    pub context: String,
    pub missing_items: Vec<String>,
}

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
        let lower = line.to_lowercase();
        // Require a tombstone noun phrase. Both space and hyphen
        // variants ("tombstone test", "tombstone-test", "tombstone
        // tests", "tombstone-tests", "tombstone for") are
        // recognized — plan prose uses both. Bare "tombstone"
        // mid-prose (e.g. "tombstones live in tests/",
        // "tombstone-audit subcommand") describes existing
        // infrastructure rather than proposing a new one.
        if !(lower.contains("tombstone test")
            || lower.contains("tombstone-test")
            || lower.contains("tombstone for"))
        {
            continue;
        }
        let trig = match trigger_regex().find(&lower) {
            Some(m) => m,
            None => continue,
        };
        if has_negation_prefix(&lower, trig.start()) {
            continue;
        }
        let missing = missing_checklist_items(&lines, idx, &fenced);
        if !missing.is_empty() {
            violations.push(Violation {
                file: source.to_path_buf(),
                line: idx + 1,
                phrase: trig.as_str().to_string(),
                context: (*line).to_string(),
                missing_items: missing,
            });
        }
    }

    violations
}

fn missing_checklist_items(lines: &[&str], trigger_idx: usize, fenced: &[bool]) -> Vec<String> {
    let mut found = vec![false; CHECKLIST_ITEMS.len()];
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
        for (item_idx, (_key, keywords)) in CHECKLIST_ITEMS.iter().enumerate() {
            if found[item_idx] {
                continue;
            }
            if keywords.iter().any(|kw| lower.contains(kw)) {
                found[item_idx] = true;
            }
        }
    }
    CHECKLIST_ITEMS
        .iter()
        .zip(found.iter())
        .filter(|(_, &f)| !f)
        .map(|((key, _), _)| (*key).to_string())
        .collect()
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
        Regex::new(r"(?i)<!--\s*tombstone-checklist\s*:\s*not-a-tombstone\s*-->")
            .expect("tombstone-checklist opt-out regex")
    })
}

fn line_has_optout_comment(line: &str) -> bool {
    optout_regex().is_match(line)
}

fn has_negation_prefix(line: &str, match_start: usize) -> bool {
    let prefix = &line[..match_start];
    let current_sentence = match prefix.rfind(". ") {
        Some(i) => &prefix[i + 2..],
        None => prefix,
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
