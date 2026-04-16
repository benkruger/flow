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

// --- Coverage waiver loophole closure ---
//
// Coverage waivers are forbidden. The `test_coverage.md` file, the
// Waiver Discipline section in `.claude/rules/docs-with-behavior.md`,
// and any reference to `test_coverage.md` from `CLAUDE.md` are the
// three surfaces that, taken together, authorized future sessions to
// classify inconvenient code as "uncoverable" and ship a justification
// instead of a refactor. All three are removed; these tombstones fail
// CI if a merge resolution or a future edit re-introduces any of them.

#[test]
fn test_coverage_md_must_not_exist() {
    let root = common::repo_root();
    let path = root.join("test_coverage.md");
    assert!(
        !path.exists(),
        "test_coverage.md must not exist — coverage waivers are forbidden. \
         Refactor the uncovered code instead (extract `process::exit` into \
         a return-code wrapper, inject subprocess callers as `&dyn Fn` \
         seams, split helpers until each branch is independently testable)."
    );
}

#[test]
fn docs_with_behavior_no_waiver_discipline_section() {
    let root = common::repo_root();
    let path = root.join(".claude/rules/docs-with-behavior.md");
    let content = fs::read_to_string(&path).expect("docs-with-behavior.md must exist");
    assert!(
        !content.contains("Waiver Discipline"),
        ".claude/rules/docs-with-behavior.md must not contain a 'Waiver Discipline' \
         section — coverage waivers are forbidden. Refactor the code instead."
    );
    assert!(
        !content.contains("test_coverage.md"),
        ".claude/rules/docs-with-behavior.md must not reference test_coverage.md — \
         the file is gone and waivers are forbidden."
    );
}

#[test]
fn claude_md_no_test_coverage_references() {
    let root = common::repo_root();
    let path = root.join("CLAUDE.md");
    let content = fs::read_to_string(&path).expect("CLAUDE.md must exist");
    assert!(
        !content.contains("test_coverage.md"),
        "CLAUDE.md must not reference test_coverage.md — coverage waivers are forbidden."
    );
    assert!(
        !content.contains("architecturally-unreachable code"),
        "CLAUDE.md must not contain the 'architecturally-unreachable code' waiver \
         bullet — coverage waivers are forbidden."
    );
}

// Stale tombstones for PR #1176 and PR #1154 removed — both PRs merged
// before the oldest open PR was created, so no active branch can
// resurrect the deleted code via merge conflict. The structural scanner
// `source_contains_pub_fn_run_with_process_exit` and its unit test
// module `source_scanner_tests` were also removed as orphaned helpers.
//
// PR #1176: format_complete_summary, format_issues_summary,
//   format_pr_timings — pub fn run wrappers replaced by run_impl_main
// PR #1154: TUI refactor — run_terminal, activate_iterm_tab, open_url,
//   find_bin_flow, module-level run, atty_check removed

// --- Coverage supersession tombstones (issue #1197) ---
//
// Two guards inside `start_finalize::run_impl` and `start_gate::run_impl`
// are unreachable dead code. The deletions close a class of uncoverable
// branches so the 100% coverage target is achievable. These structural
// tombstones scan the enclosing `run_impl` function body for the
// forbidden patterns plus enumerated bypasses.

#[test]
fn test_start_finalize_no_phase_complete_error_guard() {
    // Tombstone: removed in PR #1206. phase_complete() is infallible —
    // status is always "ok". The guard was unreachable dead code.
    let root = common::repo_root();
    let path = root.join("src/start_finalize.rs");
    let content = fs::read_to_string(&path).expect("src/start_finalize.rs must exist");
    let tail = content
        .split_once("fn run_impl(")
        .map(|(_, t)| t)
        .expect("run_impl must exist");
    let body = tail
        .split_once("\npub fn run(")
        .map(|(b, _)| b)
        .unwrap_or(tail);
    for forbidden in &[
        r#"phase_result["status"] == "error""#,
        r#"phase_result["status"] != "ok""#,
    ] {
        assert!(
            !body.contains(forbidden),
            "start_finalize::run_impl must not contain `{}` — phase_complete is infallible",
            forbidden
        );
    }
}

#[test]
fn test_start_gate_no_unexpected_deps_status_guard() {
    // Tombstone: removed in PR #1206. Earlier branches return early on
    // every non-deps_changed case; reaching this line requires
    // deps_changed == true, so the guard was unreachable.
    let root = common::repo_root();
    let path = root.join("src/start_gate.rs");
    let content = fs::read_to_string(&path).expect("src/start_gate.rs must exist");
    let tail = content
        .split_once("fn run_impl(")
        .map(|(_, t)| t)
        .expect("run_impl must exist");
    let body = tail
        .split_once("\nfn commit_deps(")
        .map(|(b, _)| b)
        .unwrap_or(tail);
    for forbidden in &["!deps_changed", "deps_changed == false"] {
        assert!(
            !body.contains(forbidden),
            "start_gate::run_impl must not contain `{}` — dead guard, deps_changed is true on reach",
            forbidden
        );
    }
}
