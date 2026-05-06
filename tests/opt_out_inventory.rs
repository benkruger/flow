//! Frozen inventory of opt-out comments in the rule corpus.
//!
//! Opt-out comments (`<!-- scope-enumeration: imperative -->`,
//! `<!-- external-input-audit: not-a-tightening -->`, etc.) disable
//! specific lint scanners on adjacent prose. Each one is a sanctioned
//! bypass — but it is still a bypass, and the project's discipline is
//! that bypasses should be rare and visible.
//!
//! This test pins the exact set of opt-out occurrences in the rule
//! corpus. Adding a new opt-out fails the test until the EXPECTED
//! constant is updated in the same diff — making every new bypass a
//! deliberate, reviewable change rather than something a session can
//! sneak in to make CI green.
//!
//! When the test fails:
//! - Removed opt-out: confirm the prose was reworded to drop the
//!   trigger (not just the comment) and remove the entry from EXPECTED.
//! - Added opt-out: prefer rewording the prose to avoid the trigger
//!   instead. Adding an opt-out is the wrong fix for most cases.
//!   When an opt-out is genuinely warranted, add it to EXPECTED and
//!   justify the bypass in the commit message.

use std::fs;
use std::path::{Path, PathBuf};

mod common;

/// One entry per active opt-out comment in the corpus, sorted.
/// Format: `<relative_path>::<kind>::<variant>`.
///
/// "Active" means the comment is not inside a fenced code block (those
/// are documentation examples) and not wrapped in backticks (those are
/// prose references to the comment, not active bypasses).
const EXPECTED: &[&str] = &[
    "CLAUDE.md::duplicate-test-coverage::not-a-new-test",
    "CLAUDE.md::duplicate-test-coverage::not-a-new-test",
    ".claude/rules/branch-path-safety.md::scope-enumeration::imperative",
    ".claude/rules/branch-path-safety.md::scope-enumeration::imperative",
    ".claude/rules/rust-patterns.md::scope-enumeration::imperative",
    ".claude/rules/verify-runtime-path.md::scope-enumeration::imperative",
    ".claude/rules/verify-runtime-path.md::scope-enumeration::imperative",
    ".claude/rules/verify-runtime-path.md::scope-enumeration::imperative",
    ".claude/rules/verify-runtime-path.md::scope-enumeration::imperative",
    ".claude/rules/verify-runtime-path.md::scope-enumeration::imperative",
    ".claude/rules/verify-runtime-path.md::scope-enumeration::imperative",
];

#[test]
fn opt_out_inventory_is_frozen() {
    let repo_root = common::repo_root();
    let mut actual = collect(&repo_root);
    actual.sort();
    let mut expected: Vec<String> = EXPECTED.iter().map(|s| s.to_string()).collect();
    expected.sort();

    if actual == expected {
        return;
    }

    let mut msg = String::from("Opt-out inventory drift in CLAUDE.md / .claude/rules/:\n\n");
    for entry in &expected {
        if count(&actual, entry) < count(&expected, entry)
            && !msg.contains(&format!("REMOVED: {}", entry))
        {
            msg.push_str(&format!("  REMOVED: {}\n", entry));
        }
    }
    for entry in &actual {
        if count(&expected, entry) < count(&actual, entry)
            && !msg.contains(&format!("ADDED:   {}", entry))
        {
            msg.push_str(&format!("  ADDED:   {}\n", entry));
        }
    }
    msg.push_str(
        "\nOpt-out comments are sanctioned bypasses for prose that would otherwise\n\
         trip a corpus scanner (scope-enumeration, external-input-audit,\n\
         duplicate-test-coverage, extract-helper-refactor). Every entry in this\n\
         inventory is a deliberate exception. To resolve drift:\n\n\
         - REMOVED entries: verify the underlying prose was actually reworded to\n\
           drop the scanner trigger (not just the bypass comment). If yes, remove\n\
           the entry from EXPECTED in tests/opt_out_inventory.rs.\n\n\
         - ADDED entries: prefer rewording the prose to avoid the scanner trigger.\n\
           Adding an opt-out is the wrong fix for most cases. When an opt-out is\n\
           genuinely warranted (the prose is imperative or the family is\n\
           open-ended), add the entry to EXPECTED and justify the bypass in the\n\
           commit message body.\n",
    );
    panic!("{}", msg);
}

fn count<S: AsRef<str> + PartialEq>(haystack: &[S], needle: &S) -> usize {
    haystack.iter().filter(|x| *x == needle).count()
}

fn collect(repo_root: &Path) -> Vec<String> {
    let mut out = Vec::new();

    let mut paths: Vec<PathBuf> = vec![repo_root.join("CLAUDE.md")];
    let rules_dir = repo_root.join(".claude").join("rules");
    let mut rule_files: Vec<PathBuf> = fs::read_dir(&rules_dir)
        .expect("read .claude/rules/")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension().and_then(|s| s.to_str()) == Some("md") {
                Some(path)
            } else {
                None
            }
        })
        .collect();
    rule_files.sort();
    paths.extend(rule_files);

    for path in paths {
        let rel = path
            .strip_prefix(repo_root)
            .expect("path under repo_root")
            .to_string_lossy()
            .to_string();
        let content = fs::read_to_string(&path).expect("read corpus file");
        scan_file(&rel, &content, &mut out);
    }

    out
}

fn scan_file(rel_path: &str, content: &str, out: &mut Vec<String>) {
    let mut in_fence = false;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some((kind, variant)) = extract_optout(line) {
            out.push(format!("{}::{}::{}", rel_path, kind, variant));
        }
    }
}

/// Extract the first active opt-out comment on a line.
/// Returns `None` for lines with no opt-out, or where the opt-out is
/// wrapped in backticks (a prose reference, not an active bypass).
fn extract_optout(line: &str) -> Option<(String, String)> {
    const KINDS: &[&str] = &[
        "scope-enumeration",
        "external-input-audit",
        "duplicate-test-coverage",
        "extract-helper-refactor",
    ];
    let mut best: Option<(usize, String, String)> = None;
    for kind in KINDS {
        let needle = format!("<!-- {}: ", kind);
        if let Some(start) = line.find(&needle) {
            // Skip if inside a backtick span (odd count of backticks before).
            let backticks_before = line[..start].chars().filter(|c| *c == '`').count();
            if backticks_before % 2 == 1 {
                continue;
            }
            let after_kind = &line[start + needle.len()..];
            if let Some(end) = after_kind.find(" -->") {
                let variant = after_kind[..end].to_string();
                if best.as_ref().is_none_or(|(s, _, _)| start < *s) {
                    best = Some((start, kind.to_string(), variant));
                }
            }
        }
    }
    best.map(|(_, k, v)| (k, v))
}
