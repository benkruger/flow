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

    /// Number of indexed entries — useful for diagnostics and tests.
    pub fn len(&self) -> usize {
        self.index.values().map(|v| v.len()).sum()
    }

    /// True when no entries are indexed. Paired with `len` per clippy
    /// `len_without_is_empty`.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Construct a corpus from explicit `(full_name, path, line)`
    /// entries. Normalizes each name and inserts into the index.
    /// Used by integration tests that exercise the scanner with
    /// hand-built corpora instead of on-disk fixture files.
    pub fn from_entries<I, P>(entries: I) -> TestCorpus
    where
        I: IntoIterator<Item = (String, P, usize)>,
        P: Into<PathBuf>,
    {
        let mut index: HashMap<String, Vec<(String, PathBuf, usize)>> = HashMap::new();
        for (name, path, line) in entries {
            let normalized = normalize(&name);
            index
                .entry(normalized)
                .or_default()
                .push((name, path.into(), line));
        }
        TestCorpus { index }
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
        //
        // The path was just yielded by the iterator — a metadata
        // lookup on an entry the kernel just handed us succeeds in
        // every practical case. The Err arm is a TOCTOU race window
        // (entry removed between read_dir and metadata) so narrow
        // that we treat it as a programmer-visible panic instead of
        // silently skipping an untestable branch.
        let meta =
            fs::symlink_metadata(&path).expect("symlink_metadata on a freshly-iterated entry");
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
            // The regex always has capture group 1 (the function name).
            // `cap.get(1)` on a successful match cannot return None —
            // a failure here is a programmer bug in the regex.
            let m = cap.get(1).expect("regex has capture group 1");
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
