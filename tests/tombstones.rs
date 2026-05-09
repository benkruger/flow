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
fn test_rust_source_no_backward_facing_comments() {
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
fn test_root_no_test_coverage_md_file() {
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
fn test_docs_with_behavior_no_waiver_discipline_section() {
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
fn test_claude_md_no_test_coverage_references() {
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
fn test_prose_corpus_no_weak_coverage_language() {
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
// PR #1389: flow-status skill replaced by `bin/flow status` Rust
//   subcommand. The four byte-substring + file-existence
//   tombstones guarding `flow:flow-status` invocation surface and
//   the SKILL.md / docs/skills/flow-status.md files are stale —
//   no active branch can resurrect them.

// --- scan_naming_violations ---
//
// Pure helper used by `test_tombstones_no_naming_violations` to enforce
// the tombstone naming convention from `.claude/rules/tombstone-tests.md`
// "Naming Convention". Walks `#[test] fn <name>(` declarations in the
// supplied content and flags any name that does not match the regex
// `^test_[a-z][a-z0-9_]*_no_[a-z][a-z0-9_]*$` (the literal form of the
// `test_<scope>_no_<removed_thing>` pattern). Names listed in the
// `exclusions` slice are skipped — used by the contract test for the
// two contract tests themselves whose names are part of the rule's
// own implementation rather than tombstones.
//
// The walk regex tolerates zero or more intervening attributes
// between `#[test]` and `fn` so an author cannot bypass naming
// enforcement by stacking a second attribute (such as
// should_panic or other test-runner directives). The walk does
// NOT distinguish `#[test]` inside raw string literals from a
// real attribute — fixtures that need to emit the literal in
// source must use the `concat!` escape (see
// `tests/test_placement.rs::src_contains_no_inline_cfg_test_blocks`
// for the canonical pattern).

fn scan_naming_violations(content: &str, exclusions: &[&str]) -> Vec<String> {
    let test_fn_re =
        Regex::new(r"#\[test\](?:\s+#\[\w+(?:\([^)]*\))?\])*\s+fn\s+(\w+)\s*\(").unwrap();
    let name_re = Regex::new(r"^test_[a-z][a-z0-9_]*_no_[a-z][a-z0-9_]*$").unwrap();
    let mut violations = Vec::new();
    for cap in test_fn_re.captures_iter(content) {
        let m = cap.get(1).unwrap();
        let name = m.as_str();
        if exclusions.contains(&name) {
            continue;
        }
        if !name_re.is_match(name) {
            let offset = m.start();
            let line = content[..offset].matches('\n').count() + 1;
            violations.push(format!(
                "line {}: {} — must match `^test_[a-z][a-z0-9_]*_no_[a-z][a-z0-9_]*$`",
                line, name
            ));
        }
    }
    violations
}

#[test]
fn test_scanner_no_violations_for_conformant_names() {
    let fixture = concat!(
        "#[",
        "test",
        "]\nfn test_foo_no_bar() {}\n",
        "#[",
        "test",
        "]\nfn test_root_no_test_coverage_md_file() {}\n",
    );
    let violations = scan_naming_violations(fixture, &[]);
    assert!(
        violations.is_empty(),
        "expected no violations for conformant names: {:?}",
        violations
    );
}

#[test]
fn test_scanner_no_false_negative_for_missing_test_prefix() {
    let fixture = concat!("#[", "test", "]\nfn missing_prefix_no_test() {}\n",);
    let violations = scan_naming_violations(fixture, &[]);
    assert_eq!(
        violations.len(),
        1,
        "expected 1 violation: {:?}",
        violations
    );
    assert!(
        violations[0].contains("missing_prefix_no_test"),
        "violation should name the offender: {}",
        violations[0]
    );
}

#[test]
fn test_scanner_no_false_negative_for_missing_no_segment() {
    let fixture = concat!("#[", "test", "]\nfn test_something_must_not_exist() {}\n",);
    let violations = scan_naming_violations(fixture, &[]);
    assert_eq!(
        violations.len(),
        1,
        "expected 1 violation: {:?}",
        violations
    );
    assert!(violations[0].contains("test_something_must_not_exist"));
}

#[test]
fn test_scanner_no_false_negative_for_test_no_prefix_only() {
    let fixture = concat!("#[", "test", "]\nfn test_no_scope_segment() {}\n",);
    let violations = scan_naming_violations(fixture, &[]);
    assert_eq!(
        violations.len(),
        1,
        "expected 1 violation: {:?}",
        violations
    );
    assert!(violations[0].contains("test_no_scope_segment"));
}

#[test]
fn test_scanner_no_violations_for_excluded_names() {
    let fixture = concat!("#[", "test", "]\nfn nonconformant_excluded_name() {}\n",);
    let violations = scan_naming_violations(fixture, &["nonconformant_excluded_name"]);
    assert!(
        violations.is_empty(),
        "excluded name should be skipped: {:?}",
        violations
    );
}

#[test]
fn test_scanner_no_violations_for_non_test_fns() {
    let fixture = "fn helper_function() {}\nfn another_plain_fn() {}\n";
    let violations = scan_naming_violations(fixture, &[]);
    assert!(
        violations.is_empty(),
        "plain fn declarations without #[test] should be ignored: {:?}",
        violations
    );
}

// --- test_tombstones_no_naming_violations ---
//
// Contract test enforcing the tombstone naming convention against
// the live `tests/tombstones.rs` source. Reads the file at runtime
// and asserts every `#[test] fn` declaration matches
// `^test_[a-z][a-z0-9_]*_no_[a-z][a-z0-9_]*$`. The two contract
// tests themselves are excluded because their names are part of the
// rule's own implementation rather than tombstones — they enforce
// the conventions but do not assert a removal.

#[test]
fn test_tombstones_no_naming_violations() {
    let root = common::repo_root();
    let path = root.join("tests").join("tombstones.rs");
    let content = fs::read_to_string(&path).expect("tests/tombstones.rs must exist");
    let exclusions: &[&str] = &[
        "test_tombstones_no_naming_violations",
        "test_tombstones_no_stability_docs_violations",
    ];
    let violations = scan_naming_violations(&content, exclusions);
    assert!(
        violations.is_empty(),
        "Tombstone naming convention violations \
         (see .claude/rules/tombstone-tests.md `Naming Convention`):\n\n{}",
        violations.join("\n")
    );
}

// --- scan_stability_docs_violations ---
//
// Pure helper used by `test_tombstones_no_stability_docs_violations` to
// enforce the literal-tombstone stability checklist from
// `.claude/rules/tombstone-tests.md` "Literal tombstones — stability
// checklist". For every `#[test] fn` whose body (between the function's
// matching braces) contains a `.contains(` call (the byte-substring
// shape) AND whose preceding `///` doc block carries one or more
// `Tombstone:.*?PR #N` markers with the highest N at or above the
// sentinel PR, the helper checks the doc block for a stability
// argument — case-insensitive match on the macro forms `concat!` or
// `format!`, or the substring `constant`. A tombstone above the
// sentinel that uses a byte-substring assertion without at least one
// of those keywords in its doc block is a violation.
//
// Edge cases handled:
//
// - Body extraction tracks brace depth so the `.contains(` check
//   only sees the function's actual body, not interstitial helpers
//   or the next test's preceding doc block.
// - The walk regex tolerates zero or more intervening attributes
//   between `#[test]` and `fn` so a stacked second attribute
//   cannot bypass enforcement.
// - The doc-block walker tolerates one or more blank lines between
//   the `///` block and the `#[test]` attribute; rustdoc still
//   attaches the doc block across one blank line.
// - When multiple `Tombstone:.*?PR #N` markers appear in the same
//   doc block, the highest PR number wins (so an in-scope marker
//   stacked second cannot be hidden by a stale below-sentinel
//   marker stacked first).
// - PR-number parse failure (overflow beyond `u32::MAX`) fails
//   closed: the marker is treated as in-scope per
//   `.claude/rules/security-gates.md` "Fail Closed When State Is
//   Unreliable".
//
// Known-fuzzy keyword: the `constant` substring may match prose
// containing words like `constant-time` or `constants`. Authors who
// invoke `concat!` and `format!` in their stability argument
// (the canonical first two checklist items) trigger the more-
// specific macro-form keywords and avoid the fuzzy substring
// surface entirely.
//
// The sentinel scopes enforcement to tombstones at or above
// `STABILITY_DOCS_SENTINEL_PR`. Tombstones below the sentinel are
// out of scope — retrofitting `///` blocks onto every existing
// byte-substring tombstone would expand the diff far past the
// rule the contract test enforces forward.

/// Sentinel PR number for `test_tombstones_no_stability_docs_violations`.
///
/// Tombstones whose `Tombstone:.*?PR #N` marker has N at or above this
/// value MUST carry a `///` doc block that mentions `concat!`,
/// `format!`, or `constant` (case-insensitive). Tombstones below the
/// sentinel predate the stability-docs requirement and are out of
/// scope; the contract test does not retroactively flag them.
///
/// When raising the sentinel — typically after a campaign that
/// retrofits `///` blocks onto older byte-substring tombstones —
/// update the value here and verify every newly-in-scope tombstone
/// passes the contract test before committing.
const STABILITY_DOCS_SENTINEL_PR: u32 = 1397;

fn scan_stability_docs_violations(
    content: &str,
    sentinel_pr: u32,
    exclusions: &[&str],
) -> Vec<String> {
    let test_fn_re =
        Regex::new(r"#\[test\](?:\s+#\[\w+(?:\([^)]*\))?\])*\s+fn\s+(\w+)\s*\(").unwrap();
    let tombstone_re = Regex::new(r"Tombstone:.*?PR #(\d+)").unwrap();
    let mut violations = Vec::new();

    for cap in test_fn_re.captures_iter(content) {
        let name = cap.get(1).unwrap().as_str();
        if exclusions.contains(&name) {
            continue;
        }

        // Extract the function body by tracking brace depth from the
        // first `{` after the signature. This narrows the .contains(
        // check to the function's actual body rather than stretching
        // through interstitial code or the next test's preceding doc
        // block.
        let after_sig = cap.get(0).unwrap().end();
        let body = match extract_fn_body(content, after_sig) {
            Some(b) => b,
            None => continue,
        };
        if !body.contains(".contains(") {
            continue;
        }

        // Walk preceding lines for the `///` doc block. Tolerate one
        // or more blank lines between the doc block and `#[test]` —
        // rustdoc still attaches the doc block.
        let test_start = cap.get(0).unwrap().start();
        let preceding = &content[..test_start];
        let mut doc_lines: Vec<&str> = Vec::new();
        let mut iter = preceding.lines().rev().peekable();
        while iter.peek().map(|l| l.trim().is_empty()).unwrap_or(false) {
            iter.next();
        }
        for line in iter {
            let trimmed = line.trim_start();
            if trimmed.starts_with("///") {
                doc_lines.push(line);
            } else {
                break;
            }
        }
        doc_lines.reverse();
        let doc_block = doc_lines.join("\n");

        // Collect all PR # markers and use the highest. A stacked
        // below-sentinel marker cannot hide a co-located above-
        // sentinel marker. Parse failure (overflow beyond u32::MAX)
        // fails closed: treat as in-scope.
        let mut max_pr: Option<u32> = None;
        for c in tombstone_re.captures_iter(&doc_block) {
            let parsed: u32 = c.get(1).unwrap().as_str().parse().unwrap_or(u32::MAX);
            max_pr = Some(max_pr.map_or(parsed, |m| m.max(parsed)));
        }
        let pr_num = match max_pr {
            Some(n) => n,
            None => continue, // no Tombstone marker in doc block
        };
        if pr_num < sentinel_pr {
            continue;
        }

        let lower = doc_block.to_lowercase();
        let has_keyword =
            lower.contains("concat!") || lower.contains("format!") || lower.contains("constant");
        if !has_keyword {
            let line = content[..test_start].matches('\n').count() + 1;
            violations.push(format!(
                "line {}: {} (PR #{}) — `///` doc block missing stability keyword (concat!/format!/constant)",
                line, name, pr_num
            ));
        }
    }
    violations
}

/// Extract the body of a `fn` declaration starting at `after_sig`
/// (the byte offset right after the closing `)` of the function
/// signature). Returns the body slice including the outer braces, or
/// `None` if no opening brace follows or the braces are unbalanced.
///
/// Brace counting does not interpret string literals or comments —
/// curly braces inside `"..."` would skew the depth. For Rust test
/// bodies in `tests/tombstones.rs` (which contain regex literals,
/// concat! fixtures, and assert! calls but not raw `}` characters in
/// strings outside macros), the simple counter is accurate.
fn extract_fn_body(content: &str, after_sig: usize) -> Option<&str> {
    let opening = after_sig + content[after_sig..].find('{')?;
    let mut depth: i32 = 0;
    for (idx, byte) in content[opening..].bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&content[opening..=opening + idx]);
                }
            }
            _ => {}
        }
    }
    None
}

#[test]
fn test_stability_scanner_no_violations_for_doc_with_concat_keyword() {
    let fixture = concat!(
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "///\n",
        "/// The literal is stable per the concat! analysis: the\n",
        "/// runtime resolver reads a fixed identifier.\n",
        "#[",
        "test",
        "]\nfn test_x_no_y() {\n",
        "    let s = \"foo\";\n",
        "    assert!(s.contains(\"foo\"));\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture, 1397, &[]);
    assert!(
        violations.is_empty(),
        "doc block with `concat` keyword should pass: {:?}",
        violations
    );
}

#[test]
fn test_stability_scanner_no_false_negative_for_missing_keyword() {
    let fixture = concat!(
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "///\n",
        "/// File-existence guard with no stability argument.\n",
        "#[",
        "test",
        "]\nfn test_x_no_y() {\n",
        "    let s = \"foo\";\n",
        "    assert!(s.contains(\"foo\"));\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture, 1397, &[]);
    assert_eq!(
        violations.len(),
        1,
        "expected 1 violation for doc block missing keyword: {:?}",
        violations
    );
    assert!(violations[0].contains("test_x_no_y"));
    assert!(violations[0].contains("1500"));
}

#[test]
fn test_stability_scanner_no_violations_for_below_sentinel_pr() {
    let fixture = concat!(
        "/// ",
        "Tombstone: removed in PR #100. Must not return.\n",
        "///\n",
        "/// File-existence guard with no stability argument.\n",
        "#[",
        "test",
        "]\nfn test_x_no_y() {\n",
        "    let s = \"foo\";\n",
        "    assert!(s.contains(\"foo\"));\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture, 1397, &[]);
    assert!(
        violations.is_empty(),
        "PR below sentinel is out of scope: {:?}",
        violations
    );
}

#[test]
fn test_stability_scanner_no_violations_for_test_without_contains_call() {
    let fixture = concat!(
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "///\n",
        "/// File-existence guard.\n",
        "#[",
        "test",
        "]\nfn test_x_no_y() {\n",
        "    assert!(!path.exists());\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture, 1397, &[]);
    assert!(
        violations.is_empty(),
        "test without .contains( body should be ignored: {:?}",
        violations
    );
}

#[test]
fn test_stability_scanner_no_violations_for_excluded_names() {
    let fixture = concat!(
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "///\n",
        "/// No stability keyword here.\n",
        "#[",
        "test",
        "]\nfn test_excluded_no_check() {\n",
        "    let s = \"foo\";\n",
        "    s.contains(\"foo\");\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture, 1397, &["test_excluded_no_check"]);
    assert!(
        violations.is_empty(),
        "excluded name should be skipped: {:?}",
        violations
    );
}

#[test]
fn test_stability_scanner_no_false_negative_for_uppercase_keyword_variants() {
    let fixture_upper = concat!(
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "///\n",
        "/// Argued via FORMAT! reassembly check.\n",
        "#[",
        "test",
        "]\nfn test_uppercase_no_check() {\n",
        "    let s = \"foo\";\n",
        "    s.contains(\"foo\");\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture_upper, 1397, &[]);
    assert!(
        violations.is_empty(),
        "uppercase FORMAT should match case-insensitively: {:?}",
        violations
    );

    let fixture_mixed = concat!(
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "///\n",
        "/// Argued via Constant declaration check.\n",
        "#[",
        "test",
        "]\nfn test_mixed_case_no_check() {\n",
        "    let s = \"foo\";\n",
        "    s.contains(\"foo\");\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture_mixed, 1397, &[]);
    assert!(
        violations.is_empty(),
        "mixed-case Constant should match case-insensitively: {:?}",
        violations
    );
}

#[test]
fn test_naming_scanner_no_false_negative_for_intervening_attribute() {
    let fixture = concat!(
        "#[",
        "test",
        "]\n#[",
        "ignore",
        "]\n",
        "fn nonconformant_with_ignore_attr() {}\n",
    );
    let violations = scan_naming_violations(fixture, &[]);
    assert_eq!(
        violations.len(),
        1,
        concat!(
            "intervening #[",
            "ignore",
            "] should not bypass naming check: {:?}"
        ),
        violations
    );
    assert!(violations[0].contains("nonconformant_with_ignore_attr"));
}

#[test]
fn test_stability_scanner_no_false_negative_for_overflow_pr_number() {
    let fixture = concat!(
        "/// ",
        "Tombstone: removed in PR #99999999999999999999. Must not return.\n",
        "///\n",
        "/// File-existence guard with no stability argument.\n",
        "#[",
        "test",
        "]\nfn test_x_no_y() {\n",
        "    let s = \"foo\";\n",
        "    assert!(s.contains(\"foo\"));\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture, 1397, &[]);
    assert_eq!(
        violations.len(),
        1,
        "overflow PR # should fail closed (treated as in-scope): {:?}",
        violations
    );
}

#[test]
fn test_stability_scanner_no_false_negative_for_blank_line_before_test() {
    let fixture = concat!(
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "///\n",
        "/// File-existence guard with no stability argument.\n",
        "\n",
        "#[",
        "test",
        "]\nfn test_x_no_y() {\n",
        "    let s = \"foo\";\n",
        "    assert!(s.contains(\"foo\"));\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture, 1397, &[]);
    assert_eq!(
        violations.len(),
        1,
        "blank line between doc and #[test] should not bypass: {:?}",
        violations
    );
}

#[test]
fn test_stability_scanner_no_violations_for_path_existence_test_adjacent_to_substring_test() {
    // Two adjacent tests: the first is a path-existence tombstone
    // (no `.contains(` in its body); the second is a byte-substring
    // tombstone with a valid stability argument. The first should
    // not be misclassified by leakage of `.contains(` from the
    // second's preceding doc block.
    let fixture = concat!(
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "#[",
        "test",
        "]\nfn test_a_no_subdir() {\n",
        "    assert!(!path.exists());\n",
        "}\n",
        "\n",
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "/// Stable per concat! analysis.\n",
        "#[",
        "test",
        "]\nfn test_b_no_invocation() {\n",
        "    let content = read();\n",
        "    assert!(!content.contains(\"forbidden\"));\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture, 1397, &[]);
    assert!(
        violations.is_empty(),
        "path-existence tombstone should not be misclassified by adjacent test: {:?}",
        violations
    );
}

#[test]
fn test_stability_scanner_no_violations_for_doc_with_format_macro_keyword() {
    let fixture = concat!(
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "///\n",
        "/// The literal is stable per format! analysis.\n",
        "#[",
        "test",
        "]\nfn test_x_no_y() {\n",
        "    let s = \"foo\";\n",
        "    assert!(s.contains(\"foo\"));\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture, 1397, &[]);
    assert!(
        violations.is_empty(),
        "doc block with `format!` macro keyword should pass: {:?}",
        violations
    );
}

#[test]
fn test_stability_scanner_no_false_negative_for_doc_with_format_status_only() {
    let fixture = concat!(
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "///\n",
        "/// Bare `flow-status` is not scanned because it is a substring of `format-status`.\n",
        "#[",
        "test",
        "]\nfn test_x_no_y() {\n",
        "    let s = \"foo\";\n",
        "    assert!(s.contains(\"foo\"));\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture, 1397, &[]);
    assert_eq!(
        violations.len(),
        1,
        "incidental `format-status` substring without `format!` macro should violate: {:?}",
        violations
    );
}

#[test]
fn test_stability_scanner_no_false_negative_for_first_marker_below_when_second_above_sentinel() {
    let fixture = concat!(
        "/// ",
        "Tombstone: removed in PR #100. Must not return.\n",
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "///\n",
        "/// File-existence guard with no stability argument.\n",
        "#[",
        "test",
        "]\nfn test_x_no_y() {\n",
        "    let s = \"foo\";\n",
        "    assert!(s.contains(\"foo\"));\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture, 1397, &[]);
    assert_eq!(
        violations.len(),
        1,
        "highest PR # in doc block should determine scope (not first): {:?}",
        violations
    );
}

// --- test_tombstones_no_stability_docs_violations ---
//
// Contract test enforcing the literal-tombstone stability checklist
// against the live `tests/tombstones.rs` source. Reads the file at
// runtime, calls `scan_stability_docs_violations` with the sentinel
// PR (`STABILITY_DOCS_SENTINEL_PR`) and the contract-test exclusion
// list, and asserts the violations vector is empty. Existing
// tombstones with PR #N below the sentinel are out of scope; new
// tombstones at or above the sentinel must carry a `///` doc block
// with at least one of the stability keywords.

#[test]
fn test_tombstones_no_stability_docs_violations() {
    let root = common::repo_root();
    let path = root.join("tests").join("tombstones.rs");
    let content = fs::read_to_string(&path).expect("tests/tombstones.rs must exist");
    let exclusions: &[&str] = &[
        "test_tombstones_no_naming_violations",
        "test_tombstones_no_stability_docs_violations",
    ];
    let violations =
        scan_stability_docs_violations(&content, STABILITY_DOCS_SENTINEL_PR, exclusions);
    assert!(
        violations.is_empty(),
        "Literal-tombstone stability checklist violations \
         (see .claude/rules/tombstone-tests.md \
         `Literal tombstones — stability checklist`):\n\n{}",
        violations.join("\n")
    );
}
