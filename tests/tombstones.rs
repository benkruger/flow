//! Consolidated tombstone tests.
//!
//! Tombstone tests assert that intentionally removed features, files,
//! and code patterns do not return. If a merge conflict resolution
//! re-introduces deleted content, the corresponding test fails.
//!
//! Standalone tombstones (file-existence, source-content) live here.
//! Topical tombstones that are integral to a test domain (e.g.
//! skill_contracts, structural) stay in their respective test files.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

/// Substring patterns whose presence in a `.rs` source line indicates a
/// backward-facing comment per `.claude/rules/comment-quality.md`. Each
/// entry is checked case-sensitively against every line in `src/**/*.rs`
/// and `tests/**/*.rs` (except `tests/tombstones.rs` itself, which must
/// contain these strings as search input).
///
/// Lines protected by the tombstone exception (lines that match
/// `Tombstone:.*?PR #`) are skipped before this list is consulted, so
/// tombstone fixtures, tombstone assertion messages, and the
/// `tombstone-audit` source remain valid even when they reference the
/// `removed in PR` substring as fixture or documentation content.
///
/// The list is curated rather than regex-based: it captures every
/// phrasing the rule explicitly prohibits, plus the phrasings observed
/// in this repo at the time the rule was enforced. New phrasings
/// introduced by future commits will not be caught automatically — the
/// rule itself is the primary instrument, and this scanner is the
/// merge-conflict trip-wire that locks in the cleanup.
const PROHIBITED: &[&str] = &[
    // Parity references to a deleted Python codebase.
    "Python parity",
    "Python-parity",
    "TypeError parity",
    "matches Python",
    "match Python",
    "matching Python",
    "matching the Python",
    "the Python original",
    "Python original",
    "the Python script",
    "Python script",
    "the Python implementation",
    "Python implementation",
    "the Python source",
    "Python source",
    "Python's",
    "Python-era",
    "Python integration tests",
    "Python test suite",
    "Python `",
    "Python:",
    "Python Path",
    "Python timeout",
    "Python behavior",
    "Python truthy",
    "Python falsy",
    "Python semantics",
    "Python writes",
    "Python ignores",
    "Python matches",
    "Python takes",
    "Python used",
    "Python prints",
    "Python swallows",
    "Python fallback",
    "Python key ordering",
    "Python output",
    "Python-only",
    "older Python",
    "Older Python",
    // Origin / port references.
    "ported to Rust",
    "was ported",
    "Ports Python",
    "Port Python",
    "Port of ",
    "Rust port",
    "mirror Python",
    "based on the old",
    // Historical PR / before-the-fix narratives.
    "Adversarial regression (PR",
    "Before the fix",
    "Before this fix",
    "Rust since PR",
    "Fixed in PR #",
    "Removed in PR #",
    "removed in PR ",
];

/// Walk a directory recursively, appending every `.rs` file path to `out`.
/// Skips `target/` build artifact directories.
fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if name == "target" {
                    continue;
                }
                collect_rs_files(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
}

/// Source-content scanner enforcing `.claude/rules/comment-quality.md`.
///
/// Walks every `*.rs` file under `src/` and `tests/` and asserts that no
/// line contains a backward-facing parity reference, historical-PR
/// provenance, or "Before the fix" narrative. Lines that match the
/// tombstone exception (`Tombstone:.*?PR #`) are skipped — they are
/// intentional per the rule. The exception regex matches any line where
/// `Tombstone:` is followed (lazily) by `PR #`, regardless of whether
/// the next characters are literal digits, a `{}` format placeholder,
/// or the regex literal `(\d+)` itself. This keeps tombstone fixture
/// generators in `tests/tombstone_audit.rs` and the parsing source in
/// `src/tombstone_audit.rs` valid without requiring per-file
/// exclusions.
///
/// The scanner self-excludes `tests/tombstones.rs` (this file) by
/// canonicalized-path comparison, because the prohibited pattern strings
/// must appear here as search input.
///
/// On any violation, the test panics with a single message listing every
/// `path:line — phrase` triple discovered in one scan, so a developer
/// gets the full inventory in one CI run instead of fixing one violation
/// at a time.
#[test]
fn test_no_backward_facing_comments_in_rust_source() {
    let root = common::repo_root();
    let scanner_path = root
        .join("tests")
        .join("tombstones.rs")
        .canonicalize()
        .expect("scanner path must canonicalize");

    let tombstone_re = Regex::new(r"Tombstone:.*?PR #").unwrap();

    let mut files: Vec<PathBuf> = Vec::new();
    collect_rs_files(&root.join("src"), &mut files);
    collect_rs_files(&root.join("tests"), &mut files);

    let mut violations: Vec<String> = Vec::new();

    for file in &files {
        // Self-exclude the scanner file (it must contain the search patterns).
        if file
            .canonicalize()
            .map(|p| p == scanner_path)
            .unwrap_or(false)
        {
            continue;
        }

        let content = match fs::read_to_string(file) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let rel = file.strip_prefix(&root).unwrap_or(file);

        for (idx, line) in content.lines().enumerate() {
            // Tombstone exception: skip lines that intentionally reference a PR.
            if tombstone_re.is_match(line) {
                continue;
            }
            for phrase in PROHIBITED {
                if line.contains(phrase) {
                    violations.push(format!("{}:{} — {}", rel.display(), idx + 1, phrase));
                }
            }
            // Paired check: "Mirrors the" + "Python" on the same line.
            // The single-pattern list cannot capture this safely because
            // "Mirrors the" appears in legitimate same-codebase parity
            // references (e.g. mirroring a guard in a sibling function).
            if line.contains("Mirrors the") && line.contains("Python") {
                violations.push(format!(
                    "{}:{} — Mirrors the .. Python",
                    rel.display(),
                    idx + 1
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Backward-facing comments found (see .claude/rules/comment-quality.md):\n\n{}",
        violations.join("\n")
    );
}

// --- validate-pretool quote-aware scanner removal tombstones (PR #1035) ---
//
// PR #1035 replaced two byte-level scanners in
// src/hooks/validate_pretool.rs with a single quote-aware state
// machine (scan_unquoted) plus two predicates (compound_op_predicate,
// redirect_predicate). The old functions were quote-unaware and
// produced false positives whenever operator characters appeared
// inside quoted arguments. These source-content tombstones assert the
// old function names do not reappear in the source file — a merge
// conflict that reintroduces them alongside the new scanner would
// silently revert the fix.

/// Tombstone: has_unescaped_semicolon removed in PR #1035. Must not return.
#[test]
fn test_no_has_unescaped_semicolon_function() {
    let path = common::repo_root()
        .join("src")
        .join("hooks")
        .join("validate_pretool.rs");
    let content = fs::read_to_string(&path).expect("validate_pretool.rs must exist");
    assert!(
        !content.contains("fn has_unescaped_semicolon"),
        "fn has_unescaped_semicolon was deleted in PR #1035 and must not return. \
         Semicolon detection now goes through compound_op_predicate + scan_unquoted \
         which tracks bash quote state."
    );
}

/// Tombstone: has_redirect removed in PR #1035. Must not return.
#[test]
fn test_no_has_redirect_function() {
    let path = common::repo_root()
        .join("src")
        .join("hooks")
        .join("validate_pretool.rs");
    let content = fs::read_to_string(&path).expect("validate_pretool.rs must exist");
    assert!(
        !content.contains("fn has_redirect"),
        "fn has_redirect was deleted in PR #1035 and must not return. \
         Redirection detection now goes through redirect_predicate + scan_unquoted \
         which tracks bash quote state."
    );
}

/// Tombstone: mid-phase `rm <temp_test_file>` in flow-code-review SKILL.md
/// removed in PR #1040. Must not return.
#[test]
fn test_code_review_no_mid_phase_adversarial_rm() {
    let path = common::repo_root()
        .join("skills")
        .join("flow-code-review")
        .join("SKILL.md");
    let content = fs::read_to_string(&path).expect("flow-code-review SKILL.md must exist");
    assert!(
        !content.contains("rm <temp_test_file>"),
        "Mid-phase `rm <temp_test_file>` block was deleted in PR #1040 and must not return. \
         Phase 6 cleanup (src/cleanup.rs::try_delete_adversarial_test_files) is the \
         authoritative cleanup — mid-phase rm targets an extensionless path that never \
         exists and silently no-ops."
    );
}

/// Tombstone: mid-phase `rm <temp_test_file>` in adversarial.md Round 2
/// removed in PR #1040. Must not return.
#[test]
fn test_adversarial_agent_no_mid_phase_rm() {
    let path = common::repo_root().join("agents").join("adversarial.md");
    let content = fs::read_to_string(&path).expect("agents/adversarial.md must exist");
    assert!(
        !content.contains("rm <temp_test_file>"),
        "Mid-phase `rm <temp_test_file>` block in Round 2 was deleted in PR #1040 and must \
         not return. Phase 6 cleanup (src/cleanup.rs::try_delete_adversarial_test_files) is \
         the authoritative cleanup — the agent's mid-phase rm targets an extensionless path \
         that never exists and silently no-ops."
    );
}
