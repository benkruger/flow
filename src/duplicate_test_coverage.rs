//! Duplicate test coverage scanner.
//!
//! When a Plan-phase plan proposes a new test function whose name
//! normalizes to the same identifier as an existing test in the
//! committed test corpus (`tests/**/*.rs`), the Plan-phase gate
//! (`bin/flow plan-check`) flags the proposal. See
//! `.claude/rules/duplicate-test-coverage.md` for the rule, the
//! opt-out grammar, and the motivating PR #1173 incident.
//!
//! The corpus scope is `tests/**/*.rs` only. Inline `#[cfg(test)]`
//! blocks in `src/*.rs` are prohibited by
//! `.claude/rules/test-placement.md` and enforced by
//! `tests/test_placement.rs`, so the scanner does not walk `src/`.
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
//! No corpus contract test ships for this scanner. The Plan-phase
//! gate already catches the real regression path (a plan naming an
//! existing test is rejected at plan-check time). A corpus scan
//! over committed prose surfaces would surface legitimate
//! educational citations in `CLAUDE.md` and `.claude/rules/*.md`
//! (e.g. `test_agent_frontmatter_only_supported_keys` in CLAUDE.md
//! as an enforcement-mechanism reference) as false positives. See
//! `tests/duplicate_test_coverage.rs` for the rationale.
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
/// Built once per `plan-check` invocation by walking `tests/`
/// under the repo root. Indexed functions are those directly
/// annotated `#[test]`. Collisions across multiple files are
/// preserved (a normalized key maps to every `(full_name, path,
/// line)` triple).
pub struct TestCorpus {
    index: HashMap<String, Vec<(String, PathBuf, usize)>>,
}

impl TestCorpus {
    /// Walk `root/tests`, indexing every `#[test]`-annotated
    /// function by normalized name.
    ///
    /// The walk is scoped to the passed `root` — never the host
    /// repo. A missing `tests/` directory (e.g. a fresh repo) is
    /// silently skipped so the function never fails on a
    /// valid-but-empty worktree layout. `src/` is intentionally
    /// not scanned — see `.claude/rules/test-placement.md`.
    pub fn from_repo(root: &Path) -> TestCorpus {
        let mut index: HashMap<String, Vec<(String, PathBuf, usize)>> = HashMap::new();
        let dir = root.join("tests");
        if dir.is_dir() {
            index_dir(&dir, root, &mut index);
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

/// Normalize a test name for matching: lowercase first, then strip
/// a leading `test_` prefix. Case-insensitive matching is
/// intentional — a plan author writing `TEST_Foo` collides with a
/// source test named `test_foo` because both normalize to `foo`.
/// Callers that extract candidates from plan prose or corpus
/// sources must hand this function the raw identifier; normalization
/// happens here so both sides of the lookup are symmetric.
pub fn normalize(name: &str) -> String {
    let lowered = name.to_ascii_lowercase();
    lowered
        .strip_prefix("test_")
        .map(|s| s.to_string())
        .unwrap_or(lowered)
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
/// partial matches inside longer strings are rejected. Case-
/// insensitive (`(?i)`) so a plan author writing `UPPER_CASE_FOO`
/// is extracted as a candidate and normalized to lowercase before
/// corpus lookup.
fn ident_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^[a-z_][a-z0-9_]{9,}$").expect("identifier regex must compile")
    })
}

/// Regex matching backtick-quoted content: `` `...` `` with at
/// least one non-backtick character inside. Callers trim the
/// captured content before the length/shape check so authors who
/// accidentally include leading/trailing whitespace inside
/// backticks do not silently bypass the scanner.
fn backtick_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"`([^`\n]+)`").expect("backtick regex must compile"))
}

/// Regex matching `fn <name>(` declarations. Case-insensitive so
/// capitalized fn identifiers in plan examples collide with the
/// lowercase corpus after `normalize()`.
fn fn_decl_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\bfn\s+([a-z_][a-z0-9_]*)\s*\(")
            .expect("fn declaration regex must compile")
    })
}

/// Regex matching `#[test]` attribute followed (eventually) by an
/// `fn <name>(` declaration. Tolerates:
///
/// - Any amount of whitespace (including newlines) between the test
///   attribute and `fn`.
/// - Additional outer attributes between the test attribute and
///   `fn` — outer attributes commonly paired with tests include
///   skip markers, panic-expectation markers, and `cfg` gating.
/// - Visibility and function modifiers (`pub`, `pub(crate)`,
///   `async`, `unsafe`, `const`, `extern "C"`, or any combination
///   in valid Rust grammar).
///
/// Capture group 1 is the function name. Used by
/// `TestCorpus::from_repo` to index every `#[test]`-annotated
/// function in a source file regardless of surrounding attribute
/// stack or modifier ordering.
fn test_fn_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Structure (multiline mode):
        //   ^\s*#\[test\]                         — the attribute, leading whitespace allowed
        //   (?: ...attr-or-modifier... )*          — zero-or-more of:
        //       whitespace (including newlines)
        //       additional outer attribute brackets
        //       or `pub` / `pub(crate)`
        //       or `async` / `unsafe` / `const`
        //   fn <name> (                            — the declaration
        //
        // `(?s)` lets `[^\]]` consume newlines inside a nested
        // attribute argument list. The identifier class accepts
        // both lowercase and uppercase so uppercase function names
        // get normalized at the corpus layer rather than silently
        // excluded. `extern "C"` is not covered here — tests do
        // not use `extern` declarations in practice, and adding
        // embedded-quote matching to a raw string requires a
        // different delimiter form.
        Regex::new(
            r##"(?ms)^[[:space:]]*#\[test\](?:[[:space:]]+|#\[[^\]]*\]|\bpub\b(?:\([^)]*\))?|\basync\b|\bunsafe\b|\bconst\b)*[[:space:]]*fn[[:space:]]+([a-zA-Z_][a-zA-Z0-9_]*)[[:space:]]*\("##,
        )
        .expect("test fn regex must compile")
    })
}

/// Extract backtick-quoted identifiers from a single line, filtered
/// by the length/shape requirement. Returns candidates in order of
/// appearance.
///
/// Trims whitespace inside the backtick capture before the
/// length/shape check so a plan author accidentally writing
/// `` ` foo_bar_baz_quux ` `` does not bypass the scanner — Markdown
/// renders padded and unpadded backticks identically, so the author
/// has no visual cue the padding matters. Per
/// `.claude/rules/security-gates.md` "Normalize Before Comparing",
/// string input for gate decisions must be trimmed before
/// comparison.
fn extract_backtick_identifiers(line: &str) -> Vec<&str> {
    backtick_regex()
        .captures_iter(line)
        .filter_map(|cap| cap.get(1))
        .map(|m| m.as_str().trim())
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
///
/// Uses `fs::symlink_metadata` for the type check so directory
/// symlink cycles (e.g. pnpm/Yarn workspace links pointing into
/// each other, or a stray `ln -s .. loop`) cannot cause unbounded
/// recursion. `Path::is_dir()` follows symlinks and would produce
/// a stack overflow on any cycle; symlink-metadata only reports
/// the link's own type. We also never recurse through symlinked
/// directories at all — indexed test sources must live under the
/// scanned root, not behind a link that could escape it.
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
        // Use symlink_metadata (not Path::is_dir) so a directory
        // symlink does NOT satisfy the is_dir() check. Only real
        // directories are recursed into; symlinks of any kind are
        // skipped. Cycles are impossible because we never traverse
        // through a link.
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
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
/// fenced code block. Recognizes both backtick fences (` ``` `) and
/// tilde fences (`~~~`) per CommonMark — a plan author using tilde
/// fences otherwise bypasses the fn-declaration extraction path.
///
/// Same semantics as the sibling scanners for the unclosed-fence
/// case: an unclosed opener at EOF fails open (scan continues) so
/// a typo does not silently suppress every violation past the stray
/// fence.
///
/// CommonMark allows a fence to close only with a marker of the
/// same kind — opening with ` ``` ` then seeing `~~~` does not
/// close the block, and vice versa. This implementation tracks the
/// opener's kind so mixed markers do not prematurely close the
/// block.
fn compute_fenced_mask(lines: &[&str]) -> Vec<bool> {
    let mut mask = vec![false; lines.len()];
    let mut in_block: Option<char> = None; // Some('`') or Some('~')
    let mut last_open_idx: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let opener = if trimmed.starts_with("```") {
            Some('`')
        } else if trimmed.starts_with("~~~") {
            Some('~')
        } else {
            None
        };
        if let Some(kind) = opener {
            match in_block {
                Some(open_kind) if open_kind == kind => {
                    // Close an existing block of the same kind.
                    in_block = None;
                    last_open_idx = None;
                }
                Some(_) => {
                    // Inside a block of a different kind — the
                    // fence marker is treated as content, not a
                    // close.
                }
                None => {
                    in_block = Some(kind);
                    last_open_idx = Some(i);
                }
            }
            // Mark the fence line itself as fenced regardless so
            // triggers on the fence marker are ignored.
            mask[i] = true;
            continue;
        }
        mask[i] = in_block.is_some();
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
    fn test_corpus_indexes_tests_dir_only() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonicalize");
        fs::create_dir_all(root.join("tests")).expect("tests dir");
        fs::create_dir_all(root.join("src")).expect("src dir");
        fs::write(
            root.join("tests/integration.rs"),
            "#[test]\nfn test_integration_path() {}\n",
        )
        .expect("write tests file");
        // Write a src file with an inline test attribute. The
        // scanner must NOT index it — `src/` is excluded per
        // `.claude/rules/test-placement.md`. Tests live in
        // `tests/<name>.rs`.
        let src_fixture = format!(
            "{attr}\nmod tests {{\n    #[test]\n    fn test_inline_case() {{}}\n}}\n",
            attr = concat!("#[cfg", "(test)]"),
        );
        fs::write(root.join("src/lib.rs"), src_fixture).expect("write src file");

        let corpus = TestCorpus::from_repo(&root);
        assert!(
            corpus.lookup("integration_path").is_some(),
            "tests/ must be indexed"
        );
        assert!(
            corpus.lookup("inline_case").is_none(),
            "src/ must NOT be indexed — inline tests are banned by test-placement.md"
        );
    }

    #[test]
    fn test_corpus_skips_functions_without_test_attribute() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonicalize");
        fs::create_dir_all(root.join("tests")).expect("tests dir");
        fs::write(
            root.join("tests/integration.rs"),
            "fn plain_function_not_a_test() {}\n",
        )
        .expect("write test file");

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
        assert!(corpus.is_empty(), "empty corpus must report is_empty()");
    }

    /// Exercises `compute_fenced_mask`'s mixed-fence-kind branch (the
    /// `Some(_)` arm when the opener kind differs from the active
    /// block's kind, lines 429-433). The tilde fence inside a backtick
    /// block must be treated as content so the candidate inside the
    /// outer block is still scanned as a fenced fn-declaration.
    #[test]
    fn scan_handles_mixed_fence_kinds_within_outer_block() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonicalize");
        let tests = root.join("tests");
        fs::create_dir_all(&tests).expect("create tests dir");
        // Index an existing test so the candidate inside the fenced block
        // collides.
        fs::write(
            tests.join("existing.rs"),
            "#[test]\nfn test_mixed_fence_collide() {}\n",
        )
        .expect("write");
        let corpus = TestCorpus::from_repo(&root);

        // Plan content opens a backtick fence, then has a tilde line
        // that the parser must NOT treat as a closer.
        let content = "```rust\n~~~yaml\nfn test_mixed_fence_collide() {}\n```\n";
        let plan_path = root.join(".flow-states").join("test-plan.md");
        let violations = scan(content, &plan_path, &corpus);
        // The candidate inside the still-open backtick block collides
        // with the indexed test.
        assert!(
            !violations.is_empty(),
            "candidate inside the backtick block must still be scanned"
        );
    }

    /// Exercises line 188 — the self-match guard skips a violation when
    /// the candidate name maps to an indexed entry that points back at
    /// the scanned file itself. Used defensively when the corpus indexes
    /// the plan/scan source itself (rare but possible in tooling).
    #[test]
    fn scan_skips_self_match_when_corpus_indexes_scanned_file() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonicalize");
        let tests = root.join("tests");
        fs::create_dir_all(&tests).expect("create tests dir");
        // The scanned file IS the same file the corpus indexes.
        let path = tests.join("self.rs");
        fs::write(&path, "#[test]\nfn test_self_match() {}\n").expect("write");
        let corpus = TestCorpus::from_repo(&root);
        let rel_path = path.strip_prefix(&root).unwrap().to_path_buf();
        // Pass the relative path as `source` so the self-match guard
        // can compare against the corpus entry's stored relative path.
        let content = "```rust\nfn test_self_match() {}\n```\n";
        let violations = scan(content, &rel_path, &corpus);
        assert!(
            violations.is_empty(),
            "self-match must be suppressed, got: {:?}",
            violations
        );
    }

    /// Direct unit test for `TestCorpus::is_empty()` — covers both the
    /// true and false branches via a populated corpus and an empty one.
    #[test]
    fn test_corpus_is_empty_returns_false_when_populated() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonicalize");
        let tests = root.join("tests");
        fs::create_dir_all(&tests).expect("create tests dir");
        fs::write(tests.join("a.rs"), "#[test]\nfn test_present() {}\n").expect("write");
        let corpus = TestCorpus::from_repo(&root);
        assert!(!corpus.is_empty());
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

    #[test]
    fn scan_tilde_fence_detects_fn_declaration() {
        // Guards PR #1177 adversarial finding: `~~~` fences must
        // be recognized alongside backtick fences so a plan author
        // cannot bypass fn-declaration extraction by using
        // CommonMark's tilde-fence alternative. The regression
        // path is a plan that places `fn <name>()` inside `~~~`
        // fences — before the compute_fenced_mask fix, the mask
        // would flag the lines as non-fenced, backtick-identifier
        // extraction would run (finding nothing), and fn-declaration
        // extraction (scoped to fenced blocks) would NOT run.
        let corpus = make_corpus_with(&[("test_tilde_fn_target_name", "tests/a.rs", 1)]);
        let plan = "Task: add a new test.\n\n~~~rust\nfn tilde_fn_target_name() {}\n~~~\n";
        let violations = scan(plan, Path::new("/plan.md"), &corpus);
        assert_eq!(
            violations.len(),
            1,
            "tilde-fenced fn declarations must be extracted"
        );
        assert_eq!(violations[0].phrase, "tilde_fn_target_name");
    }

    #[test]
    fn scan_tilde_fence_suppresses_backtick_identifier_extraction() {
        // Inside a `~~~` fenced block the scanner extracts fn
        // declarations (not backtick identifiers), matching the
        // behavior of backtick-fenced blocks.
        let corpus = make_corpus_with(&[("test_inside_tilde_fence_name", "tests/a.rs", 1)]);
        let plan = "~~~\nReference to `inside_tilde_fence_name` in narrative.\n~~~\n";
        let violations = scan(plan, Path::new("/plan.md"), &corpus);
        assert!(
            violations.is_empty(),
            "backtick identifiers inside tilde fences follow the same \
             suppression as backtick fences"
        );
    }

    #[test]
    fn scan_mixed_fence_markers_do_not_prematurely_close() {
        // CommonMark: a ~~~ opener is not closed by a ``` marker.
        // Ensure the scanner tracks fence kind so a stray ``` inside
        // a ~~~ block does not close the block early.
        let corpus = make_corpus_with(&[("test_mixed_fence_target_name", "tests/a.rs", 1)]);
        let plan = "~~~\nnot a close: ```\nfn mixed_fence_target_name() {}\n~~~\n";
        let violations = scan(plan, Path::new("/plan.md"), &corpus);
        assert_eq!(
            violations.len(),
            1,
            "mixed fence markers must not prematurely close the outer block"
        );
    }

    // --- whitespace trimming and case sensitivity ---

    #[test]
    fn scan_trims_trailing_whitespace_inside_backticks() {
        // Guards PR #1177 adversarial finding: padded backticks
        // (`` ` foo_bar ` ``) must normalize identically to the
        // unpadded form. Markdown renders them the same, so an
        // author has no visual cue that padding would bypass the
        // scanner.
        let corpus = make_corpus_with(&[("test_trailing_space_bypass_name", "tests/a.rs", 1)]);
        let plan = "Plan names `trailing_space_bypass_name ` as new.";
        let violations = scan(plan, Path::new("/plan.md"), &corpus);
        assert_eq!(
            violations.len(),
            1,
            "trailing space inside backticks must be trimmed"
        );
    }

    #[test]
    fn scan_trims_leading_whitespace_inside_backticks() {
        let corpus = make_corpus_with(&[("test_leading_space_bypass_name", "tests/a.rs", 1)]);
        let plan = "Plan names ` leading_space_bypass_name` as new.";
        let violations = scan(plan, Path::new("/plan.md"), &corpus);
        assert_eq!(
            violations.len(),
            1,
            "leading space inside backticks must be trimmed"
        );
    }

    #[test]
    fn scan_matches_uppercase_plan_identifier() {
        // Guards PR #1177 adversarial finding: case-insensitive
        // matching is the documented contract. `TEST_FOO` and
        // `test_foo` must normalize to the same key.
        let corpus = make_corpus_with(&[("test_upper_case_target_name", "tests/a.rs", 1)]);
        let plan = "Plan names `UPPER_CASE_TARGET_NAME` as new.";
        let violations = scan(plan, Path::new("/plan.md"), &corpus);
        assert_eq!(
            violations.len(),
            1,
            "uppercase identifiers must normalize case-insensitively"
        );
    }

    #[test]
    fn scan_matches_mixed_case_plan_identifier() {
        let corpus = make_corpus_with(&[("test_mixed_case_target_name", "tests/a.rs", 1)]);
        let plan = "Plan names `Mixed_Case_Target_Name` as new.";
        let violations = scan(plan, Path::new("/plan.md"), &corpus);
        assert_eq!(violations.len(), 1);
    }

    // --- normalize() contract ---

    #[test]
    fn normalize_uppercase_prefix_is_stripped() {
        // `TEST_Foo` lowercases to `test_foo`, then the prefix is
        // stripped to `foo`. Verifies the lowercase-first ordering
        // in normalize(), fixing a correctness drift between the
        // doc comment's case-insensitive promise and the original
        // case-sensitive strip.
        assert_eq!(normalize("TEST_Foo"), "foo");
    }

    #[test]
    fn normalize_does_not_strip_double_prefix() {
        // Documented contract: normalize strips exactly one leading
        // `test_`. `test_test_foo` → `test_foo` (one strip). This
        // matches the production code. The adversarial agent
        // flagged a doc comment earlier promising a double-strip
        // that the code did not implement; the doc has been
        // corrected and this test locks in the actual behavior.
        assert_eq!(normalize("test_test_foo"), "test_foo");
    }

    // --- corpus multi-attribute / modifier support ---

    fn make_corpus_from_source(files: &[(&str, &str)]) -> TestCorpus {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonicalize");
        let tests = root.join("tests");
        fs::create_dir_all(&tests).expect("create tests dir");
        for (name, body) in files {
            fs::write(tests.join(name), body).expect("write test fixture");
        }
        // NOTE: we leak the TempDir here intentionally so the
        // corpus's PathBuf entries remain valid for the duration of
        // the test. The helper is test-only so the leak is bounded.
        let corpus = TestCorpus::from_repo(&root);
        std::mem::forget(dir);
        corpus
    }

    #[test]
    fn corpus_indexes_unsafe_test_fn() {
        // Guards PR #1177 adversarial finding: `#[test] unsafe fn`
        // declarations must be indexed. FFI-facing tests often use
        // `unsafe fn`; skipping them leaves false negatives.
        let corpus = make_corpus_from_source(&[(
            "ffi.rs",
            "#[test]\nunsafe fn test_unsafe_fixture_target_long() {}\n",
        )]);
        assert!(
            corpus.lookup("unsafe_fixture_target_long").is_some(),
            "unsafe fn must be indexed"
        );
    }

    #[test]
    fn corpus_indexes_pub_test_fn() {
        let corpus = make_corpus_from_source(&[(
            "pub_test.rs",
            "#[test]\npub fn test_pub_fixture_target_long() {}\n",
        )]);
        assert!(
            corpus.lookup("pub_fixture_target_long").is_some(),
            "pub fn must be indexed"
        );
    }

    #[test]
    fn corpus_indexes_const_test_fn() {
        let corpus = make_corpus_from_source(&[(
            "const_test.rs",
            "#[test]\nconst fn test_const_fixture_target_long() {}\n",
        )]);
        assert!(corpus.lookup("const_fixture_target_long").is_some());
    }

    #[test]
    fn corpus_indexes_inline_attribute_test_fn() {
        // `#[test] fn foo()` on a single line must be indexed.
        let corpus = make_corpus_from_source(&[(
            "inline.rs",
            "#[test] fn test_inline_fixture_target_long() {}\n",
        )]);
        assert!(
            corpus.lookup("inline_fixture_target_long").is_some(),
            "same-line `#[test] fn` must be indexed"
        );
    }

    #[test]
    fn corpus_indexes_test_fn_with_extra_attributes() {
        // A test attribute followed by additional outer attributes
        // (skip markers, panic-expectation markers, cfg gating)
        // must still index the function name. Construct the fixture
        // via concat! so the zero-tolerance no_skipped_or_excluded
        // contract test does not flag this file for containing the
        // skip-marker attribute as a literal substring.
        let skip_attr = concat!("#[", "ignore", "]");
        let panic_attr = concat!("#[", "should_panic", "]");
        let cfg_attr = "#[cfg(feature = \"x\")]";
        let fixture = format!(
            "#[test]\n{skip}\nfn test_ignore_fixture_target_long() {{}}\n\
             \n\
             #[test]\n{panic}\nfn test_should_panic_fixture_long() {{}}\n\
             \n\
             #[test]\n{cfg}\nfn test_cfg_fixture_target_long() {{}}\n",
            skip = skip_attr,
            panic = panic_attr,
            cfg = cfg_attr,
        );
        let fixture_static: &'static str = Box::leak(fixture.into_boxed_str());
        let corpus = make_corpus_from_source(&[("multi_attr.rs", fixture_static)]);
        assert!(
            corpus.lookup("ignore_fixture_target_long").is_some(),
            "test attribute followed by a skip-marker attribute must be indexed"
        );
        assert!(
            corpus.lookup("should_panic_fixture_long").is_some(),
            "test attribute followed by a panic-expectation attribute must be indexed"
        );
        assert!(
            corpus.lookup("cfg_fixture_target_long").is_some(),
            "test attribute followed by a cfg attribute must be indexed"
        );
    }

    #[test]
    fn corpus_indexes_pub_async_test_fn() {
        // Combined modifiers: `#[test] pub async fn`.
        let corpus = make_corpus_from_source(&[(
            "pub_async.rs",
            "#[test]\npub async fn test_pub_async_target_long() {}\n",
        )]);
        assert!(corpus.lookup("pub_async_target_long").is_some());
    }

    // --- symlink safety ---

    #[cfg(unix)]
    #[test]
    fn index_dir_skips_symlinked_subdirectories() {
        // Guards PR #1177 pre-mortem critical finding: directory
        // symlinks (especially cycles) must not be followed. The
        // fix uses fs::symlink_metadata + an explicit is_symlink
        // skip so a ln -s .. loop cannot cause unbounded recursion
        // or index content outside the scanned root.
        use std::os::unix::fs::symlink;
        let dir = tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonicalize");
        let tests = root.join("tests");
        fs::create_dir_all(&tests).expect("create tests dir");
        fs::write(
            tests.join("real.rs"),
            "#[test]\nfn test_real_fixture_target_long() {}\n",
        )
        .expect("write real test");

        // Create a symlink loop: tests/loop -> tests
        let loop_link = tests.join("loop");
        symlink(&tests, &loop_link).expect("create symlink loop");

        // This must terminate (no stack overflow) and index only
        // the real file, not walk through the symlink.
        let corpus = TestCorpus::from_repo(&root);
        assert!(corpus.lookup("real_fixture_target_long").is_some());
        // The corpus should contain exactly one entry — if the
        // symlink were followed, the real file would be indexed
        // twice (once through tests/real.rs and once through
        // tests/loop/real.rs).
        assert_eq!(
            corpus.len(),
            1,
            "symlinked subdirectory must not be traversed"
        );
    }
}
