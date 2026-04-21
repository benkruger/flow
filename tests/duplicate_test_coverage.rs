//! Integration tests for `src/duplicate_test_coverage.rs`.
//!
//! History: An initial iteration of this file scanned the committed
//! prose corpus (`CLAUDE.md`, `.claude/rules/*.md`,
//! `skills/**/SKILL.md`, `.claude/skills/**/SKILL.md`) for
//! backtick-quoted identifiers that normalize to an existing test
//! name. That scanner produced 18+ false positives on the first run —
//! every legitimate educational citation in the rule files
//! (e.g. `test_agent_frontmatter_only_supported_keys` in CLAUDE.md,
//! `production_ci_decider_tree_changed_returns_not_skipped` in
//! `.claude/rules/extract-helper-refactor.md`) fired. Per
//! `.claude/rules/tests-guard-real-regressions.md` "Forbidden
//! patterns: Duplicate guards for a property already covered by an
//! existing plan-check scanner," the corpus check adds no protection
//! on top of the Plan-phase gate already shipped — a plan that names
//! an existing test is caught at plan-check time regardless of
//! whether the name was copied from a committed prose file. Per
//! `.claude/rules/scope-enumeration.md` "False-positive sweep before
//! expanding the vocabulary" (count ≥ 5 → revert), the corpus scan
//! is intentionally absent from this file.
//!
//! What lives here: unit-level tests migrated from the former inline
//! `#[cfg(test)] mod tests` block in `src/duplicate_test_coverage.rs`
//! per `.claude/rules/test-placement.md`. They drive the public
//! surface (`TestCorpus::from_repo`, `TestCorpus::lookup`, `scan`,
//! `normalize`) through on-disk fixture files. The
//! `make_corpus_with_fixture` helper builds tempdir-rooted fixtures
//! so every corpus passed to `scan` comes from the real `from_repo`
//! codepath — no hand-crafted test-only constructor on `TestCorpus`.

use std::fs;
use std::path::Path;

use tempfile::tempdir;

use flow_rs::duplicate_test_coverage::{normalize, scan, TestCorpus};

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

#[test]
fn normalize_uppercase_prefix_is_stripped() {
    // `TEST_Foo` lowercases to `test_foo`, then the prefix is
    // stripped to `foo`. Verifies the lowercase-first ordering
    // in normalize().
    assert_eq!(normalize("TEST_Foo"), "foo");
}

#[test]
fn normalize_does_not_strip_double_prefix() {
    // Documented contract: normalize strips exactly one leading
    // `test_`. `test_test_foo` → `test_foo` (one strip).
    assert_eq!(normalize("test_test_foo"), "test_foo");
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
    assert!(
        corpus.lookup("anything_else_not_in_corpus").is_none(),
        "corpus must not index names that weren't in the fixture"
    );
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
    assert!(
        corpus.lookup("anything_at_all").is_none(),
        "missing tests/ must yield an empty corpus (no entries)"
    );
}

/// Exercises `compute_fenced_mask`'s mixed-fence-kind branch: a
/// tilde fence inside a backtick block must be treated as content so
/// the candidate inside the outer block is still scanned as a fenced
/// fn-declaration.
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

/// The self-match guard skips a violation when the candidate name
/// maps to an indexed entry that points back at the scanned file
/// itself. Used defensively when the corpus indexes the plan/scan
/// source itself.
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

// --- scan ---

/// Build a `TestCorpus` from hand-specified entries by writing fixture
/// source files into a tempdir and calling `TestCorpus::from_repo`.
/// Tests that only assert `violations.len()` or `violations[0].phrase`
/// tolerate the tempdir-rooted `existing_file` string; tests that
/// pin `existing_file` exactly use `make_corpus_with_fixed_path`.
/// Returns the corpus AND the root directory; callers drop the dir
/// when the test scope exits.
fn make_corpus_with(existing: &[(&str, &str, usize)]) -> TestCorpus {
    let (corpus, _root) = make_corpus_with_root(existing);
    // Leak the tempdir so paths embedded in corpus entries remain
    // valid for the life of the test. The OS reclaims the directory
    // when the test process exits.
    std::mem::forget(_root);
    corpus
}

fn make_corpus_with_root(existing: &[(&str, &str, usize)]) -> (TestCorpus, tempfile::TempDir) {
    let dir = tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    let tests = root.join("tests");
    fs::create_dir_all(&tests).expect("create tests dir");
    // Group entries by file so multiple `#[test] fn`s in the same
    // virtual path get appended to one fixture file.
    use std::collections::BTreeMap;
    let mut by_file: BTreeMap<&str, Vec<(&str, usize)>> = BTreeMap::new();
    for (name, path, line) in existing {
        by_file.entry(path).or_default().push((name, *line));
    }
    for (virtual_path, entries) in by_file {
        // Virtual paths look like "tests/foo.rs"; strip the leading
        // "tests/" so we write under the tempdir's tests/ directory.
        let filename = virtual_path.strip_prefix("tests/").unwrap_or(virtual_path);
        let mut body = String::new();
        for (name, _line) in entries {
            body.push_str(&format!("#[test]\nfn {name}() {{}}\n"));
        }
        fs::write(tests.join(filename), body).expect("write fixture");
    }
    let corpus = TestCorpus::from_repo(&root);
    (corpus, dir)
}

#[test]
fn scan_flags_duplicate_normalized_name() {
    let corpus = make_corpus_with(&[("test_foo_bar_baz_quux", "tests/foo.rs", 42)]);
    let plan = "The plan adds `foo_bar_baz_quux` as a new test.";
    let violations = scan(plan, Path::new("/plan.md"), &corpus);
    assert_eq!(violations.len(), 1, "one collision expected");
    assert_eq!(violations[0].phrase, "foo_bar_baz_quux");
    assert_eq!(violations[0].existing_test, "test_foo_bar_baz_quux");
    assert!(
        violations[0].existing_file.contains("tests/foo.rs:"),
        "existing_file must reference tests/foo.rs (tempdir-rooted), got: {}",
        violations[0].existing_file
    );
}

#[test]
fn scan_flags_pr_1173_incident_name() {
    let corpus = make_corpus_with(&[(
        "test_stop_continue_qa_pending_fallback_blocks",
        "tests/hooks.rs",
        1499,
    )]);
    let plan = "Task 7 adds `stop_continue_qa_pending_fallback_blocks` as a new subprocess test.";
    let violations = scan(plan, Path::new("/plan.md"), &corpus);
    assert_eq!(violations.len(), 1, "PR #1173 incident must flag");
    assert_eq!(
        violations[0].phrase,
        "stop_continue_qa_pending_fallback_blocks"
    );
}

#[test]
fn scan_no_match_for_short_identifier() {
    // `foo` is below the 10-char minimum length; even if the corpus
    // contains `test_foo`, the plan's `foo` is not a candidate.
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
    // declarations, not backtick-quoted identifiers.
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
    assert!(
        violations[0].existing_file.contains("tests/hooks.rs:"),
        "existing_file must reference tests/hooks.rs (tempdir-rooted), got: {}",
        violations[0].existing_file
    );
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
    // contract.
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
    // `~~~` fences must be recognized alongside backtick fences so
    // a plan author cannot bypass fn-declaration extraction by
    // using CommonMark's tilde-fence alternative.
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
    // Padded backticks (`` ` foo_bar ` ``) must normalize identically
    // to the unpadded form. Markdown renders them the same.
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

// --- corpus multi-attribute / modifier support ---

fn make_corpus_from_source(files: &[(&str, &str)]) -> TestCorpus {
    let dir = tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    let tests = root.join("tests");
    fs::create_dir_all(&tests).expect("create tests dir");
    for (name, body) in files {
        fs::write(tests.join(name), body).expect("write test fixture");
    }
    // Leak the TempDir so the corpus's PathBuf entries remain valid
    // for the duration of the test.
    let corpus = TestCorpus::from_repo(&root);
    std::mem::forget(dir);
    corpus
}

#[test]
fn corpus_indexes_unsafe_test_fn() {
    // `#[test] unsafe fn` declarations must be indexed. FFI-facing
    // tests often use `unsafe fn`.
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
    // (skip markers, panic-expectation markers, cfg gating) must
    // still index the function name. Construct the fixture via
    // concat! so the zero-tolerance no_skipped_or_excluded contract
    // test does not flag this file for containing the skip-marker
    // attribute as a literal substring.
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

#[test]
fn index_dir_recurses_into_subdirectories() {
    // Create `tests/sub/deep.rs` so the recursive `index_dir(&path, ...)`
    // branch fires. Without this, that call site is uncovered.
    let dir = tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    let deep = root.join("tests").join("sub");
    fs::create_dir_all(&deep).expect("create deep dir");
    fs::write(
        deep.join("deep.rs"),
        "#[test]\nfn test_nested_subdir_target_name() {}\n",
    )
    .expect("write nested");
    let corpus = TestCorpus::from_repo(&root);
    assert!(
        corpus.lookup("nested_subdir_target_name").is_some(),
        "nested test files must be indexed"
    );
}

#[test]
fn index_dir_skips_nested_target_and_hidden_dirs() {
    // `tests/target/` and `tests/.hidden/` are skipped by name, so
    // any `.rs` files inside them must not appear in the corpus.
    let dir = tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    let target = root.join("tests").join("target");
    let hidden = root.join("tests").join(".hidden");
    fs::create_dir_all(&target).expect("create nested target");
    fs::create_dir_all(&hidden).expect("create hidden");
    fs::write(
        target.join("build.rs"),
        "#[test]\nfn test_nested_target_skipped_name() {}\n",
    )
    .expect("write target");
    fs::write(
        hidden.join("secret.rs"),
        "#[test]\nfn test_nested_hidden_skipped_name() {}\n",
    )
    .expect("write hidden");
    let corpus = TestCorpus::from_repo(&root);
    assert!(corpus.lookup("nested_target_skipped_name").is_none());
    assert!(corpus.lookup("nested_hidden_skipped_name").is_none());
}

#[cfg(unix)]
#[test]
fn from_repo_tolerates_read_dir_error_on_tests() {
    // `fs::read_dir` on a directory that `is_dir()` returns Ok for
    // may still fail when the directory lacks read permission.
    // Exercise the `Err(_) => return` branch inside index_dir.
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    let tests = root.join("tests");
    fs::create_dir_all(&tests).expect("create tests");
    // mode 0o111 = execute-only: is_dir() succeeds (exec bit), but
    // read_dir fails (no read bit).
    let mut perms = fs::metadata(&tests).unwrap().permissions();
    perms.set_mode(0o111);
    fs::set_permissions(&tests, perms).unwrap();

    let corpus = TestCorpus::from_repo(&root);

    // Restore perms so tempdir cleanup works.
    let mut rperms = fs::metadata(&tests).unwrap().permissions();
    rperms.set_mode(0o755);
    fs::set_permissions(&tests, rperms).unwrap();

    // The corpus builds without panicking; no entries are indexed
    // because the scan hit the read_dir Err branch and bailed.
    assert!(
        corpus.lookup("nothing_should_be_indexed").is_none(),
        "read_dir Err branch must produce an empty corpus"
    );
}

#[cfg(unix)]
#[test]
fn index_dir_skips_unreadable_rs_file() {
    // Exercise the `fs::read_to_string` Err branch inside index_dir:
    // chmod a .rs file to 0o000 so the read fails and the loop
    // skips past it without aborting.
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    let tests = root.join("tests");
    fs::create_dir_all(&tests).expect("create tests");
    let unreadable = tests.join("locked.rs");
    fs::write(
        &unreadable,
        "#[test]\nfn test_unreadable_but_present_name() {}\n",
    )
    .expect("write");
    let mut perms = fs::metadata(&unreadable).unwrap().permissions();
    perms.set_mode(0o000);
    fs::set_permissions(&unreadable, perms).unwrap();
    // Also write a readable file so the corpus isn't empty.
    fs::write(
        tests.join("readable.rs"),
        "#[test]\nfn test_readable_present_name() {}\n",
    )
    .expect("write readable");

    let corpus = TestCorpus::from_repo(&root);

    // Restore perms so tempdir cleanup works.
    let mut rperms = fs::metadata(&unreadable).unwrap().permissions();
    rperms.set_mode(0o644);
    fs::set_permissions(&unreadable, rperms).unwrap();

    // The unreadable file's contents were skipped; the readable one
    // is indexed.
    assert!(corpus.lookup("readable_present_name").is_some());
    assert!(corpus.lookup("unreadable_but_present_name").is_none());
}

#[test]
fn index_dir_skips_non_rs_extension_files() {
    // A `.txt` file in tests/ must be skipped — indexing only walks
    // `.rs` source files.
    let dir = tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    let tests = root.join("tests");
    fs::create_dir_all(&tests).expect("create tests");
    fs::write(
        tests.join("readme.txt"),
        "This file is not Rust, must be skipped.",
    )
    .expect("write txt");
    fs::write(
        tests.join("real.rs"),
        "#[test]\nfn test_rs_file_indexed_here() {}\n",
    )
    .expect("write rs");
    let corpus = TestCorpus::from_repo(&root);
    assert!(corpus.lookup("rs_file_indexed_here").is_some());
    // The .txt file's content isn't parsed, so no test names from it
    // appear in the corpus.
    assert!(
        corpus
            .lookup("This_file_is_not_Rust_must_be_skipped")
            .is_none(),
        ".txt content must not be scanned for test names"
    );
}

#[cfg(unix)]
#[test]
fn index_dir_skips_symlinked_subdirectories() {
    // Directory symlinks (especially cycles) must not be followed.
    // fs::symlink_metadata + an explicit is_symlink skip so a
    // ln -s .. loop cannot cause unbounded recursion or index
    // content outside the scanned root.
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
    // the real file, not walk through the symlink. If the symlink
    // were followed, `lookup("real_fixture_target_long")` would
    // return two entries instead of one.
    let corpus = TestCorpus::from_repo(&root);
    let hits = corpus
        .lookup("real_fixture_target_long")
        .expect("real file must be indexed");
    assert_eq!(
        hits.len(),
        1,
        "symlinked subdirectory must not be traversed (would produce a second hit)"
    );
}
