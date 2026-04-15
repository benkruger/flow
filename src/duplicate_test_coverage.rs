//! Duplicate test coverage scanner.
//!
//! When a Plan-phase plan proposes a new test function whose name
//! normalizes to the same identifier as an existing test in the
//! committed test corpus (`tests/**/*.rs` integration tests plus
//! `src/**/*.rs` inline `#[test]`-annotated functions), the
//! Plan-phase gate (`bin/flow plan-check`) flags the proposal. See
//! `.claude/rules/duplicate-test-coverage.md` for the rule, the
//! opt-out grammar, and the motivating PR #1173 incident.
//!
//! This module is the shared scanner used by three callers:
//!
//! - `bin/flow plan-check` — gates Plan-phase completion on
//!   `.flow-states/<branch>-plan.md`. The standard plan path
//!   invokes it from `skills/flow-plan/SKILL.md` Step 4.
//! - `src/plan_extract.rs` extracted path — runs the same scanner
//!   against the promoted plan content for pre-decomposed issues.
//! - `src/plan_extract.rs` resume path — runs the scanner against
//!   an existing plan file on re-invocation.
//!
//! A contract test in `tests/duplicate_test_coverage.rs` also uses
//! `scan` against the committed prose corpus (`CLAUDE.md`,
//! `.claude/rules/*.md`, `skills/**/SKILL.md`,
//! `.claude/skills/**/SKILL.md`) to catch drift in authoritative
//! documentation that would accidentally document a pre-existing
//! test name.
//!
//! ## Normalization
//!
//! `normalize(name)` strips a leading `test_` prefix and lowercases
//! the remainder. Matching is symmetric: `test_foo_bar_quux` and
//! `foo_bar_quux` both normalize to `foo_bar_quux`.
//!
//! ## Candidate extraction
//!
//! Candidate names come from two sources in plan prose:
//!
//! 1. `fn <snake_name>(` declarations inside fenced code blocks.
//! 2. Backtick-quoted identifiers matching
//!    `^[a-z_][a-z0-9_]{9,}$` (length ≥ 10) outside fenced blocks.
//!    The length filter prevents common-word false positives.
//!
//! ## Opt-outs
//!
//! Two line-level opt-out comments are recognized:
//!
//! - `<!-- duplicate-test-coverage: not-a-new-test -->` — prose
//!   discusses an existing test rather than proposing one.
//! - `<!-- duplicate-test-coverage: intentional-duplicate -->` —
//!   author is knowingly adding a parallel test (Named-Tests-
//!   After-Refactor pattern).
//!
//! The comment applies to its own line, the next non-blank line,
//! or two lines below with a single blank line between. No chaining
//! beyond one blank line — the same walk-back rule as the sibling
//! scanners in `src/scope_enumeration.rs` and
//! `src/external_input_audit.rs`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

/// A violation: a candidate test name in the plan that collides
/// with an existing test in the corpus.
///
/// `line` is 1-indexed. `phrase` is the literal candidate name as it
/// appeared in the plan. `context` is the full line containing the
/// match (untrimmed). `existing_test` is the full name of the
/// pre-existing test (with or without the `test_` prefix — preserved
/// as written in source). `existing_file` is the file path and line
/// where the existing test is defined.
#[derive(Debug, Clone)]
pub struct Violation {
    pub file: PathBuf,
    pub line: usize,
    pub phrase: String,
    pub context: String,
    pub existing_test: String,
    pub existing_file: String,
}

/// Index of existing test functions by normalized name.
///
/// Built once per `plan-check` invocation by walking `tests/` and
/// `src/` under the repo root. Indexed functions are those directly
/// annotated `#[test]`. Collisions across multiple files are
/// preserved (a normalized key maps to every `(full_name, path,
/// line)` triple).
pub struct TestCorpus {
    index: HashMap<String, Vec<(String, PathBuf, usize)>>,
}

impl TestCorpus {
    /// Walk `root/tests` and `root/src`, indexing every
    /// `#[test]`-annotated function by normalized name.
    ///
    /// The walk is scoped to the passed `root` — never the host
    /// repo. Missing directories (e.g. a fresh repo with no
    /// `tests/`) are silently skipped so the function never fails
    /// on a valid-but-empty worktree layout.
    pub fn from_repo(root: &Path) -> TestCorpus {
        let mut index: HashMap<String, Vec<(String, PathBuf, usize)>> = HashMap::new();
        for subdir in &["tests", "src"] {
            let dir = root.join(subdir);
            if dir.is_dir() {
                index_dir(&dir, root, &mut index);
            }
        }
        TestCorpus { index }
    }

    /// Look up a normalized name in the corpus. Returns `None` when
    /// no existing test matches.
    pub fn lookup(&self, normalized: &str) -> Option<&[(String, PathBuf, usize)]> {
        self.index.get(normalized).map(|v| v.as_slice())
    }

    /// Number of indexed entries — useful for diagnostics.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.index.values().map(|v| v.len()).sum()
    }

    /// True when no entries are indexed. Paired with `len` per clippy
    /// `len_without_is_empty`; test-only because the production
    /// callers only need `lookup`.
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Normalize a test name for matching: strip a leading `test_`
/// prefix and lowercase the remainder. Case-insensitive matching is
/// intentional — a plan author writing `TEST_Foo` vs source
/// containing `test_foo` still collides.
pub fn normalize(name: &str) -> String {
    let trimmed = name.strip_prefix("test_").unwrap_or(name);
    // Strip a second `test_` prefix only if the remainder starts
    // with it — keeps the function idempotent on normalized input
    // while still matching `test_Test_foo` style names. Current
    // matching is case-sensitive for the prefix strip because Rust
    // convention is lowercase `test_`; only the remainder is
    // lowercased.
    trimmed.to_ascii_lowercase()
}

/// Scan `content` for candidate test names that collide with an
/// existing test in `corpus`. `source` is the file path used to
/// populate `Violation::file`.
pub fn scan(content: &str, source: &Path, corpus: &TestCorpus) -> Vec<Violation> {
    let lines: Vec<&str> = content.lines().collect();
    let fenced = compute_fenced_mask(&lines);
    let mut violations = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        if is_optout_line(&lines, idx) {
            continue;
        }
        // Inside fenced blocks, only `fn <name>(` declarations are
        // extracted — prose-style backtick identifiers don't appear
        // there. Outside fenced blocks, only backtick identifiers
        // are extracted — raw `fn <name>(` in prose is rare enough
        // to skip without loss.
        let candidates: Vec<&str> = if fenced[idx] {
            extract_fn_declarations(line)
        } else {
            extract_backtick_identifiers(line)
        };

        for candidate in candidates {
            let normalized = normalize(candidate);
            if let Some(hits) = corpus.lookup(&normalized) {
                // Self-match guard: if the candidate string itself
                // is identical to an indexed name AND the indexed
                // entry points to the scanned file, skip — the
                // corpus is indexing the plan file itself (unlikely
                // in practice, but defensive).
                if hits
                    .iter()
                    .any(|(name, path, _)| name == candidate && path.as_path() == source)
                {
                    continue;
                }
                // Emit one violation per hit so the user sees every
                // collision location.
                for (existing_name, existing_path, existing_line) in hits {
                    violations.push(Violation {
                        file: source.to_path_buf(),
                        line: idx + 1,
                        phrase: candidate.to_string(),
                        context: (*line).to_string(),
                        existing_test: existing_name.clone(),
                        existing_file: format!("{}:{}", existing_path.display(), existing_line),
                    });
                }
            }
        }
    }
    violations
}

/// Regex matching a snake_case identifier of length ≥ 10
/// characters (the minimum length that prevents common-word
/// identifiers like `foo`, `config`, `helper` from false-positive
/// matching). The `^...$` anchors make this a whole-token match —
/// partial matches inside longer strings are rejected.
fn ident_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[a-z_][a-z0-9_]{9,}$").expect("identifier regex must compile"))
}

/// Regex matching backtick-quoted content: `` `...` `` with at
/// least one non-backtick character inside.
fn backtick_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"`([^`\n]+)`").expect("backtick regex must compile"))
}

/// Regex matching `fn <name>(` declarations.
fn fn_decl_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\bfn\s+([a-z_][a-z0-9_]*)\s*\(").expect("fn declaration regex must compile")
    })
}

/// Regex matching `#[test]` attribute followed by an `fn <name>(`
/// declaration, with any whitespace (including newlines) between.
/// Used by `TestCorpus::from_repo` to find test function names in
/// `.rs` files.
fn test_fn_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?m)^\s*#\[test\]\s*\n\s*(?:async\s+)?fn\s+([a-z_][a-z0-9_]*)\s*\(")
            .expect("test fn regex must compile")
    })
}

/// Extract backtick-quoted identifiers from a single line, filtered
/// by the length/shape requirement. Returns candidates in order of
/// appearance.
fn extract_backtick_identifiers(line: &str) -> Vec<&str> {
    backtick_regex()
        .captures_iter(line)
        .filter_map(|cap| cap.get(1))
        .map(|m| m.as_str())
        .filter(|s| ident_regex().is_match(s))
        .collect()
}

/// Extract `fn <name>(` declarations from a line. Used inside
/// fenced code blocks where Rust declarations are the canonical
/// test-name source.
fn extract_fn_declarations(line: &str) -> Vec<&str> {
    fn_decl_regex()
        .captures_iter(line)
        .filter_map(|cap| cap.get(1))
        .map(|m| m.as_str())
        .collect()
}

/// Recursively walk a directory and index every `#[test]`-annotated
/// function found in `.rs` files.
fn index_dir(dir: &Path, root: &Path, index: &mut HashMap<String, Vec<(String, PathBuf, usize)>>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        // Skip target/ and hidden dirs.
        if name == "target" || name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            index_dir(&path, root, index);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        // Map byte offsets to 1-indexed line numbers lazily.
        let mut line_starts: Vec<usize> = vec![0];
        for (i, b) in content.as_bytes().iter().enumerate() {
            if *b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        // Relative path from root for nicer error messages.
        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        for cap in test_fn_regex().captures_iter(&content) {
            let m = match cap.get(1) {
                Some(m) => m,
                None => continue,
            };
            let fn_name = m.as_str().to_string();
            let normalized = normalize(&fn_name);
            let byte_offset = m.start();
            let line_num = line_starts
                .binary_search(&byte_offset)
                .unwrap_or_else(|i| i.saturating_sub(1))
                + 1;
            index
                .entry(normalized)
                .or_default()
                .push((fn_name, rel.clone(), line_num));
        }
    }
}

/// Returns `true` for every line index that sits inside (or on) a
/// fenced code block. Same semantics as the sibling scanners: an
/// unclosed fence fails open (scan continues) so a typo does not
/// silently suppress every violation past the stray fence.
fn compute_fenced_mask(lines: &[&str]) -> Vec<bool> {
    let mut mask = vec![false; lines.len()];
    let mut in_block = false;
    let mut last_open_idx: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("```") {
            if in_block {
                in_block = false;
                last_open_idx = None;
            } else {
                in_block = true;
                last_open_idx = Some(i);
            }
            mask[i] = true;
            continue;
        }
        mask[i] = in_block;
    }
    if let Some(start) = last_open_idx {
        for m in &mut mask[start..] {
            *m = false;
        }
    }
    mask
}

/// Returns `true` when line `idx` is covered by an opt-out comment.
/// Same walk-back-one-blank-line rule as the sibling scanners.
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

fn line_has_optout_comment(line: &str) -> bool {
    line.contains("<!-- duplicate-test-coverage: not-a-new-test -->")
        || line.contains("<!-- duplicate-test-coverage: intentional-duplicate -->")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    // --- normalize ---

    #[test]
    fn normalize_strips_test_prefix() {
        assert_eq!(normalize("test_foo_bar"), "foo_bar");
    }

    #[test]
    fn normalize_is_idempotent_when_no_prefix() {
        assert_eq!(normalize("foo_bar"), "foo_bar");
    }

    #[test]
    fn normalize_is_case_insensitive() {
        assert_eq!(normalize("test_FooBar"), "foobar");
    }

    #[test]
    fn normalize_keeps_test_suffix() {
        // Only the leading `test_` is stripped; a suffix is kept.
        assert_eq!(normalize("foo_test"), "foo_test");
    }

    // --- TestCorpus::from_repo ---

    #[test]
    fn test_corpus_scopes_to_root_parameter() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonicalize");
        let tests = root.join("tests");
        fs::create_dir_all(&tests).expect("create tests dir");
        fs::write(tests.join("foo.rs"), "#[test]\nfn test_foo_bar_baz() {}\n")
            .expect("write test file");

        let corpus = TestCorpus::from_repo(&root);
        assert!(
            corpus.lookup("foo_bar_baz").is_some(),
            "corpus should index test_foo_bar_baz"
        );
        assert_eq!(corpus.len(), 1, "corpus should contain exactly one entry");
    }

    #[test]
    fn test_corpus_indexes_tests_dir_and_src_inline_tests() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonicalize");
        fs::create_dir_all(root.join("tests")).expect("tests dir");
        fs::create_dir_all(root.join("src")).expect("src dir");
        fs::write(
            root.join("tests/integration.rs"),
            "#[test]\nfn test_integration_path() {}\n",
        )
        .expect("write tests file");
        fs::write(
            root.join("src/lib.rs"),
            "#[cfg(test)]\nmod tests {\n    #[test]\n    fn test_inline_case() {}\n}\n",
        )
        .expect("write src file");

        let corpus = TestCorpus::from_repo(&root);
        assert!(corpus.lookup("integration_path").is_some());
        assert!(corpus.lookup("inline_case").is_some());
    }

    #[test]
    fn test_corpus_skips_functions_without_test_attribute() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonicalize");
        fs::create_dir_all(root.join("src")).expect("src dir");
        fs::write(
            root.join("src/lib.rs"),
            "fn plain_function_not_a_test() {}\n",
        )
        .expect("write src file");

        let corpus = TestCorpus::from_repo(&root);
        assert!(corpus.lookup("plain_function_not_a_test").is_none());
    }

    #[test]
    fn test_corpus_skips_target_directory() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonicalize");
        fs::create_dir_all(root.join("target/debug")).expect("target dir");
        fs::write(
            root.join("target/debug/build_artifact.rs"),
            "#[test]\nfn test_in_target_dir() {}\n",
        )
        .expect("write artifact");

        let corpus = TestCorpus::from_repo(&root);
        assert!(
            corpus.lookup("in_target_dir").is_none(),
            "target/ artifacts must not be indexed"
        );
    }

    #[test]
    fn test_corpus_missing_directories_skipped() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonicalize");
        // No tests/ or src/ dirs created — corpus must still build.
        let corpus = TestCorpus::from_repo(&root);
        assert_eq!(corpus.len(), 0);
    }

    // --- scan ---

    fn make_corpus_with(existing: &[(&str, &str, usize)]) -> TestCorpus {
        let mut index: HashMap<String, Vec<(String, PathBuf, usize)>> = HashMap::new();
        for (name, path, line) in existing {
            let normalized = normalize(name);
            index.entry(normalized).or_default().push((
                (*name).to_string(),
                PathBuf::from(path),
                *line,
            ));
        }
        TestCorpus { index }
    }

    #[test]
    fn scan_flags_duplicate_normalized_name() {
        let corpus = make_corpus_with(&[("test_foo_bar_baz_quux", "tests/foo.rs", 42)]);
        let plan = "The plan adds `foo_bar_baz_quux` as a new test.";
        let violations = scan(plan, Path::new("/plan.md"), &corpus);
        assert_eq!(violations.len(), 1, "one collision expected");
        assert_eq!(violations[0].phrase, "foo_bar_baz_quux");
        assert_eq!(violations[0].existing_test, "test_foo_bar_baz_quux");
        assert_eq!(violations[0].existing_file, "tests/foo.rs:42");
    }

    #[test]
    fn scan_flags_pr_1173_incident_name() {
        let corpus = make_corpus_with(&[(
            "test_stop_continue_qa_pending_fallback_blocks",
            "tests/hooks.rs",
            1499,
        )]);
        let plan =
            "Task 7 adds `stop_continue_qa_pending_fallback_blocks` as a new subprocess test.";
        let violations = scan(plan, Path::new("/plan.md"), &corpus);
        assert_eq!(violations.len(), 1, "PR #1173 incident must flag");
        assert_eq!(
            violations[0].phrase,
            "stop_continue_qa_pending_fallback_blocks"
        );
    }

    #[test]
    fn scan_no_match_for_short_identifier() {
        // `foo` is below the 10-char minimum length; even if the
        // corpus contains `test_foo`, the plan's `foo` is not a
        // candidate because common-word backtick IDs don't count.
        let corpus = make_corpus_with(&[("test_foo", "tests/a.rs", 1)]);
        let plan = "The plan mentions `foo` but it's not a test name.";
        let violations = scan(plan, Path::new("/plan.md"), &corpus);
        assert!(
            violations.is_empty(),
            "short identifiers must not false-positive"
        );
    }

    #[test]
    fn scan_no_match_for_unrelated_name() {
        let corpus = make_corpus_with(&[("test_existing_long_name", "tests/a.rs", 1)]);
        let plan = "The plan names `totally_unrelated_function` as a new test.";
        let violations = scan(plan, Path::new("/plan.md"), &corpus);
        assert!(violations.is_empty());
    }

    #[test]
    fn scan_opt_out_not_a_new_test_suppresses() {
        let corpus = make_corpus_with(&[("test_foo_bar_baz_quux", "tests/foo.rs", 42)]);
        let plan = "Existing `foo_bar_baz_quux` already covers this. <!-- duplicate-test-coverage: not-a-new-test -->";
        let violations = scan(plan, Path::new("/plan.md"), &corpus);
        assert!(violations.is_empty(), "same-line opt-out must suppress");
    }

    #[test]
    fn scan_opt_out_intentional_duplicate_suppresses() {
        let corpus = make_corpus_with(&[("test_foo_bar_baz_quux", "tests/foo.rs", 42)]);
        let plan = "<!-- duplicate-test-coverage: intentional-duplicate -->\nAdd `foo_bar_baz_quux` as a parallel test.";
        let violations = scan(plan, Path::new("/plan.md"), &corpus);
        assert!(
            violations.is_empty(),
            "preceding-line opt-out must suppress"
        );
    }

    #[test]
    fn scan_opt_out_walk_back_one_blank_line() {
        let corpus = make_corpus_with(&[("test_foo_bar_baz_quux", "tests/foo.rs", 42)]);
        let plan = "<!-- duplicate-test-coverage: not-a-new-test -->\n\nThe existing `foo_bar_baz_quux` test is referenced here.";
        let violations = scan(plan, Path::new("/plan.md"), &corpus);
        assert!(
            violations.is_empty(),
            "opt-out two lines above with one blank must suppress"
        );
    }

    #[test]
    fn scan_opt_out_does_not_chain_across_two_blank_lines() {
        let corpus = make_corpus_with(&[("test_foo_bar_baz_quux", "tests/foo.rs", 42)]);
        let plan = "<!-- duplicate-test-coverage: not-a-new-test -->\n\n\nThe `foo_bar_baz_quux` test is referenced here.";
        let violations = scan(plan, Path::new("/plan.md"), &corpus);
        assert_eq!(
            violations.len(),
            1,
            "opt-out must not chain across two blank lines"
        );
    }

    #[test]
    fn scan_fn_declaration_in_fenced_block_triggers() {
        let corpus = make_corpus_with(&[("test_foo_bar_baz_quux", "tests/foo.rs", 42)]);
        let plan = "```rust\nfn foo_bar_baz_quux() {}\n```";
        let violations = scan(plan, Path::new("/plan.md"), &corpus);
        assert_eq!(violations.len(), 1, "fn declaration in fence should flag");
        assert_eq!(violations[0].phrase, "foo_bar_baz_quux");
    }

    #[test]
    fn scan_backtick_identifier_inside_fenced_block_is_skipped() {
        let corpus = make_corpus_with(&[("test_foo_bar_baz_quux", "tests/foo.rs", 42)]);
        // Inside a fenced block the scanner only looks for `fn ...(`
        // declarations, not backtick-quoted identifiers. A prose-
        // style mention `foo_bar_baz_quux` (wrapped in backticks)
        // inside a fence is not a candidate.
        let plan = "```\nSome narrative that mentions `foo_bar_baz_quux` in backticks.\n```";
        let violations = scan(plan, Path::new("/plan.md"), &corpus);
        assert!(violations.is_empty());
    }

    #[test]
    fn scan_reports_existing_test_location() {
        let corpus = make_corpus_with(&[("test_the_existing_one", "tests/hooks.rs", 1499)]);
        let plan = "Plan names `the_existing_one` as new.";
        let violations = scan(plan, Path::new("/plan.md"), &corpus);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].existing_test, "test_the_existing_one");
        assert_eq!(violations[0].existing_file, "tests/hooks.rs:1499");
    }

    #[test]
    fn scan_multiple_existing_hits_produce_multiple_violations() {
        let corpus = make_corpus_with(&[
            ("test_duplicate_named", "tests/a.rs", 10),
            ("test_duplicate_named", "tests/b.rs", 20),
        ]);
        let plan = "Plan names `duplicate_named` as a new test.";
        let violations = scan(plan, Path::new("/plan.md"), &corpus);
        assert_eq!(
            violations.len(),
            2,
            "both existing hits should produce violations"
        );
    }

    #[test]
    fn scan_symmetric_match_when_plan_uses_test_prefix() {
        let corpus = make_corpus_with(&[("plain_unprefixed_name", "tests/a.rs", 1)]);
        let plan = "Plan names `test_plain_unprefixed_name` as new.";
        let violations = scan(plan, Path::new("/plan.md"), &corpus);
        assert_eq!(violations.len(), 1, "normalized match works symmetrically");
    }

    #[test]
    fn scan_handles_empty_corpus_without_false_positives() {
        let corpus = make_corpus_with(&[]);
        let plan = "The plan freely names `any_identifier_goes_here` as a new test.";
        let violations = scan(plan, Path::new("/plan.md"), &corpus);
        assert!(violations.is_empty());
    }

    // --- fence mask behavior ---

    #[test]
    fn scan_unclosed_fence_fails_open() {
        let corpus = make_corpus_with(&[("test_after_unclosed_fence", "tests/a.rs", 1)]);
        // An unclosed fence at the top must NOT mask the violation
        // below — the mask is rewound per the sibling scanner's
        // contract. Otherwise a typo silently suppresses violations.
        let plan = "```\n(no closing fence)\n\n`after_unclosed_fence` is a candidate.";
        let violations = scan(plan, Path::new("/plan.md"), &corpus);
        assert_eq!(
            violations.len(),
            1,
            "unclosed fence must fail open per sibling scanner contract"
        );
    }
}
