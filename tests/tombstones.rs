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

// --- Weak-coverage prose loophole closure ---
//
// Weak-coverage language ("adequate test coverage", "adequately tested")
// is the prose surface through which a reviewer or reviewer agent could
// justify shipping below 100% coverage. The 100% gate in `bin/test`
// (`--fail-under-*` ratchet) and `.claude/rules/no-waivers.md` are the
// load-bearing mechanisms; this scanner prevents the prose from drifting
// back in via merge conflict or accidental edit. Scope is intentionally
// narrow: agent reports, skill instructions, and public docs — the
// surfaces where the phrases would license below-100% shipping. The
// `.claude/rules/` and `CLAUDE.md` corpus is excluded because those
// files discuss the coverage discipline and may legitimately cite the
// forbidden phrases. The `tests/` corpus is excluded because this
// scanner file contains the phrases as search input.

/// Weak-coverage phrases that must not reappear in the user-facing
/// prose corpus. Re-introducing either phrase would let a reviewer
/// agent cite "adequate"/"adequately" coverage as grounds for
/// shipping below 100%, defeating the `--fail-under-*` ratchet in
/// `bin/test` and the `.claude/rules/no-waivers.md` discipline.
const WEAK_COVERAGE_PHRASES: &[&str] = &["adequate test coverage", "adequately tested"];

/// Scan scope for the weak-coverage check. Only `agents/`, `skills/`,
/// and `docs/` are scanned — those are the prose surfaces where the
/// forbidden phrases would license below-100% shipping. `.claude/rules/`
/// and `CLAUDE.md` legitimately discuss the coverage discipline, and
/// `tests/tombstones.rs` contains the phrases as search input. None of
/// those paths fall under the scan directories, so the scanner cannot
/// reach its own literals.
const WEAK_COVERAGE_SCAN_DIRS: &[&str] = &["agents", "skills", "docs"];

#[test]
fn test_no_weak_coverage_language_in_prose_corpus() {
    let root = common::repo_root();
    let mut violations: Vec<String> = Vec::new();
    for dir in WEAK_COVERAGE_SCAN_DIRS {
        let dir_path = root.join(dir);
        for (rel, content) in common::collect_md_files(&dir_path) {
            for (idx, line) in content.lines().enumerate() {
                for phrase in WEAK_COVERAGE_PHRASES {
                    if line.contains(phrase) {
                        violations.push(format!("{}/{}:{} — {}", dir, rel, idx + 1, phrase));
                    }
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "Weak-coverage language found in prose corpus \
         (see .claude/rules/no-waivers.md and issue #1195):\n\n{}",
        violations.join("\n")
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

// --- TUI extraction to tui_terminal module ---
//
// `run_tui_terminal` and `TerminalGuard` live in `src/tui_terminal.rs`,
// not `src/main.rs`. Keeping the crossterm-coupled glue out of main.rs
// allows main.rs to reach 100% coverage — the new module's seam-injected
// `run_tui_arm_impl` is unit-testable in `cargo nextest`, while the
// crossterm-bound `run_terminal` only runs in production. A merge
// resolution that re-introduces either symbol into main.rs would
// duplicate the definitions and reintroduce uncovered crossterm code in
// a file expected to stay 100% covered.

#[test]
fn test_main_rs_no_run_tui_terminal_or_terminal_guard() {
    // Tombstone: removed in PR #1205. Must not return to main.rs.
    //
    // Multi-pronged scan because byte-substring assertions on
    // `fn run_tui_terminal(` and `struct TerminalGuard` alone are
    // bypassable via naming variation (`fn run_tui_terminal_inner`,
    // `struct TerminalGuardInner`) per `.claude/rules/tombstone-tests.md`
    // "Literal tombstones — stability checklist." The crossterm API
    // names (`enable_raw_mode`, `EnterAlternateScreen`,
    // `LeaveAlternateScreen`, `disable_raw_mode`) cannot be assembled
    // via `concat!`/`format!` because they must appear in `use
    // crossterm::terminal::{...}` import lines or as resolved
    // function call sites — they are not just string literals. Any
    // resurrection of TUI terminal glue under any naming variation
    // must reference at least one of these crossterm symbols, so
    // their absence from main.rs is the load-bearing structural
    // assertion.
    let root = common::repo_root();
    let path = root.join("src/main.rs");
    let content = fs::read_to_string(&path).expect("src/main.rs must exist");

    // Original literal-name assertions — caught the simple case.
    assert!(
        !content.contains("fn run_tui_terminal("),
        "src/main.rs must not contain `fn run_tui_terminal(` — \
         crossterm event-loop glue lives in `src/tui_terminal.rs` to \
         keep main.rs 100% covered."
    );
    assert!(
        !content.contains("struct TerminalGuard"),
        "src/main.rs must not contain `struct TerminalGuard` — the RAII \
         guard lives in `src/tui_terminal.rs` so its Drop is unit-testable \
         via an injected `release_fn` closure."
    );

    // Structural assertions — crossterm API symbol names that any
    // resurrected terminal glue would have to reference. Naming
    // variations on the wrapper function or guard struct do not
    // evade these because the underlying crossterm API names are
    // fixed by the upstream crate.
    const FORBIDDEN_CROSSTERM_SYMBOLS: &[&str] = &[
        "enable_raw_mode",
        "disable_raw_mode",
        "EnterAlternateScreen",
        "LeaveAlternateScreen",
        "CrosstermBackend",
    ];
    for symbol in FORBIDDEN_CROSSTERM_SYMBOLS {
        assert!(
            !content.contains(symbol),
            "src/main.rs must not reference crossterm symbol `{}` — \
             crossterm-coupled code lives in `src/tui_terminal.rs` so \
             main.rs stays 100% covered. If this symbol returned under a \
             new naming convention, move it to tui_terminal.rs.",
            symbol
        );
    }
}

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
    //
    // Scope is the run_impl_with_deps body (where the deleted guard
    // previously lived). The bounded-slice ends at pub fn run_impl,
    // which always follows run_impl_with_deps in the file.
    let root = common::repo_root();
    let path = root.join("src/start_finalize.rs");
    let content = fs::read_to_string(&path).expect("src/start_finalize.rs must exist");
    let tail = content
        .split_once("fn run_impl_with_deps(")
        .map(|(_, t)| t)
        .expect("run_impl_with_deps must exist");
    let body = tail
        .split_once("\npub fn run_impl(")
        .map(|(b, _)| b)
        .unwrap_or(tail);
    for forbidden in &[
        r#"phase_result["status"] == "error""#,
        r#"phase_result["status"] != "ok""#,
    ] {
        assert!(
            !body.contains(forbidden),
            "start_finalize::run_impl_with_deps must not contain `{}` — phase_complete is infallible",
            forbidden
        );
    }
}

#[test]
fn test_start_gate_no_unexpected_deps_status_guard() {
    // Tombstone: removed in PR #1206. Earlier branches return early on
    // every non-deps_changed case; reaching this line requires
    // deps_changed == true, so the guard was unreachable.
    //
    // Scope is the run_impl_with_deps body (where the deleted guard
    // previously lived). The bounded-slice ends at pub fn run_impl,
    // which always follows run_impl_with_deps in the file.
    let root = common::repo_root();
    let path = root.join("src/start_gate.rs");
    let content = fs::read_to_string(&path).expect("src/start_gate.rs must exist");
    let tail = content
        .split_once("fn run_impl_with_deps(")
        .map(|(_, t)| t)
        .expect("run_impl_with_deps must exist");
    let body = tail
        .split_once("\npub fn run_impl(")
        .map(|(b, _)| b)
        .unwrap_or(tail);
    for forbidden in &["!deps_changed", "deps_changed == false"] {
        assert!(
            !body.contains(forbidden),
            "start_gate::run_impl_with_deps must not contain `{}` — dead guard, deps_changed is true on reach",
            forbidden
        );
    }
}
