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
// (`--fail-under-*` gate) and `.claude/rules/no-waivers.md` are the
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
/// shipping below 100%, defeating the `--fail-under-*` gate in
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

/// Normalize prose for the weak-coverage scan: ASCII-lowercase plus
/// whitespace collapse (any run of whitespace — spaces, tabs, newlines,
/// non-breaking spaces — becomes a single ASCII space). This catches
/// case variants ("Adequate test coverage"), interior whitespace
/// variants ("adequate  test coverage", tab-separated, non-breaking
/// space), and line-spanning matches where Markdown word-wrap puts
/// the forbidden phrase on two lines. Per
/// `.claude/rules/tombstone-tests.md` "Assertion Strength" and
/// `.claude/rules/security-gates.md` "Normalize Before Comparing",
/// both sides of the comparison must be normalized.
fn normalize_for_weak_coverage_scan(s: &str) -> String {
    s.to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn test_no_weak_coverage_language_in_prose_corpus() {
    let root = common::repo_root();
    let normalized_phrases: Vec<String> = WEAK_COVERAGE_PHRASES
        .iter()
        .map(|p| normalize_for_weak_coverage_scan(p))
        .collect();
    let mut violations: Vec<String> = Vec::new();
    for dir in WEAK_COVERAGE_SCAN_DIRS {
        let dir_path = root.join(dir);
        for (rel, content) in common::collect_md_files(&dir_path) {
            let normalized = normalize_for_weak_coverage_scan(&content);
            for (orig, normalized_phrase) in
                WEAK_COVERAGE_PHRASES.iter().zip(normalized_phrases.iter())
            {
                if normalized.contains(normalized_phrase.as_str()) {
                    violations.push(format!("{}/{} — {}", dir, rel, orig));
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

// Stale tombstones for PR #1176, PR #1154, PR #1258, PR #1344, and
// PR #1375 removed — each PR merged before the oldest open PR was
// created, so no active branch can resurrect the deleted code via
// merge conflict. The structural scanner
// `source_contains_pub_fn_run_with_process_exit` and its unit test
// module `source_scanner_tests` were also removed as orphaned
// helpers.
//
// PR #1176: format_complete_summary, format_issues_summary,
//   format_pr_timings — pub fn run wrappers replaced by run_impl_main
// PR #1154: TUI refactor — run_terminal, activate_iterm_tab, open_url,
//   find_bin_flow, module-level run, atty_check removed
// PR #1258: branch-scoped state-file layout moved from
//   `.flow-states/<branch>-<purpose>.<ext>` to
//   `.flow-states/<branch>/<purpose>.<ext>`; the
//   `test_no_flat_layout_format_in_rust_source` scanner is no
//   longer needed once the branch-cutoff window passed
// PR #1344: flow-qa maintainer skill, the four backing Rust modules
//   (qa_mode, qa_reset, qa_verify, scaffold_qa), the qa/templates/
//   directory, the Commands::Qa* clap variants, and the
//   `Bash(rm -rf *.qa-repos*)` allow-list entry. The maintainer QAs
//   locally via --plugin-dir instead.
// PR #1383: Phase 2 (Plan) lifecycle phase, the flow-plan skill,
//   plan_extract / plan_check / scanner sources, plan_step state
//   fields, FlowPlan enum variant, scanner rule files, and the
//   Phase 2 docs page. Plans now travel inside issue bodies via
//   the FLOW-PLAN-BEGIN / FLOW-PLAN-END sentinels extracted by
//   bin/flow plan-from-issue at flow-start.

// --- flow-status skill removal (PR #1389) ---

/// Tombstone: removed in PR #1389. Must not return.
///
/// File-existence guard for the skill's SKILL.md. Pairs with the
/// byte-substring tombstone below per `.claude/rules/tombstone-tests.md`
/// "Two kinds of tombstone" — file-resurrection threats are caught here
/// regardless of how a future commit imports the file (e.g., via a
/// `#[path = "..."]` rename), and the substring scan catches any
/// reintroduction of the skill's slash-command surface.
#[test]
fn flow_status_skill_directory_must_not_exist() {
    let root = common::repo_root();
    let path = root.join("skills").join("flow-status").join("SKILL.md");
    assert!(
        fs::symlink_metadata(&path).is_err(),
        "skills/flow-status/SKILL.md must not exist — the skill was \
         replaced by the `bin/flow status` Rust subcommand. Consumer \
         skills (phase transition gates) now invoke that binary \
         directly."
    );
}

/// Tombstone: removed in PR #1389. Must not return.
///
/// File-existence guard for the published documentation page. Pairs
/// with the substring scan in
/// `flow_status_skill_invocation_must_not_appear_in_rules_or_docs`.
#[test]
fn flow_status_docs_page_must_not_exist() {
    let root = common::repo_root();
    let path = root.join("docs").join("skills").join("flow-status.md");
    assert!(
        fs::symlink_metadata(&path).is_err(),
        "docs/skills/flow-status.md must not exist — the skill was \
         replaced by the `bin/flow status` Rust subcommand."
    );
}

/// Tombstone: removed in PR #1389. Must not return.
///
/// Substring scan over every SKILL.md in `skills/` for the literal
/// `flow:flow-status`. The literal is stable per the four-question
/// checklist in `.claude/rules/tombstone-tests.md`:
///
/// 1. concat!: cannot be assembled — Claude Code's Skill resolver reads
///    the literal string from the user's invocation; a `concat!`-built
///    surrogate exists only in source and never reaches the resolver.
/// 2. format!: same reason — runtime format reassembly cannot resolve
///    through the Skill tool.
/// 3. split constants: same.
/// 4. method-chain `.arg()`: same — the Skill resolver names the skill
///    via a fixed identifier.
///
/// A reintroduction of the skill would have to spell `flow:flow-status`
/// literally in a SKILL.md (or any markdown file under `skills/`) for
/// the model to invoke it. The byte-substring check catches every such
/// shape.
#[test]
fn flow_status_skill_invocation_must_not_appear_in_skills() {
    let root = common::repo_root();
    let skills_dir = root.join("skills");
    let mut violations: Vec<String> = Vec::new();
    for (rel, content) in common::collect_md_files(&skills_dir) {
        if content.contains("flow:flow-status") {
            violations.push(format!("skills/{}", rel));
        }
    }
    assert!(
        violations.is_empty(),
        "`flow:flow-status` must not appear in any skills/**/SKILL.md \
         — the skill was replaced by `bin/flow status`. Consumer \
         skills invoke the binary directly. Violations:\n{}",
        violations.join("\n")
    );
}

/// Tombstone: removed in PR #1389. Must not return.
///
/// Substring scan over `.claude/rules/*.md`, `docs/skills/index.md`,
/// `docs/skills/flow-skills.md`, `docs/index.html`, `docs/reference/`,
/// and `README.md` for the precise tokens `flow:flow-status`,
/// `/flow-status`, and `flow-status.md`. Bare `flow-status` is NOT
/// scanned because it is a substring of `format-status` — a search
/// for bare `flow-status` would false-positive on every legitimate
/// `format-status` reference.
#[test]
fn flow_status_skill_invocation_must_not_appear_in_rules_or_docs() {
    let root = common::repo_root();
    const TOKENS: &[&str] = &["flow:flow-status", "/flow-status", "flow-status.md"];
    let mut targets: Vec<PathBuf> = Vec::new();

    let rules_dir = root.join(".claude").join("rules");
    if rules_dir.is_dir() {
        for entry in fs::read_dir(&rules_dir).expect("rules dir") {
            let entry = entry.expect("read entry");
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("md") {
                targets.push(path);
            }
        }
    }

    targets.push(root.join("docs").join("skills").join("index.md"));
    targets.push(root.join("docs").join("skills").join("flow-skills.md"));
    targets.push(root.join("docs").join("index.html"));
    targets.push(root.join("README.md"));

    let docs_ref = root.join("docs").join("reference");
    if docs_ref.is_dir() {
        for entry in fs::read_dir(&docs_ref).expect("docs/reference dir") {
            let entry = entry.expect("read entry");
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("md") {
                targets.push(path);
            }
        }
    }

    let mut violations: Vec<String> = Vec::new();
    for target in &targets {
        if let Ok(content) = fs::read_to_string(target) {
            for token in TOKENS {
                if content.contains(token) {
                    violations.push(format!(
                        "{}: contains `{}`",
                        target.strip_prefix(&root).unwrap_or(target).display(),
                        token
                    ));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "flow-status references must not appear in rules or docs — \
         the skill was replaced by `bin/flow status`. Bare \
         `flow-status` is intentionally not scanned (substring of \
         `format-status`). Violations:\n{}",
        violations.join("\n")
    );
}
