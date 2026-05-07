//! Tests for `src/verify_references_scanner.rs`.
//!
//! Plan-phase Gate 6: backtick-quoted identifiers (≥10 chars,
//! snake_case) inside the `## Tasks` section must exist as
//! `fn <name>(` definitions in `tests/` or `src/`.

use std::fs;
use std::path::PathBuf;

use flow_rs::verify_references_scanner::{scan, DefinitionIndex, Violation};
use tempfile::tempdir;

fn fixture_path() -> PathBuf {
    PathBuf::from(".flow-states/test/plan.md")
}

/// Build a fixture index by writing fake source files to a tempdir
/// containing the named functions.
fn fixture_index(funcs: &[(&str, &str)]) -> (tempfile::TempDir, DefinitionIndex) {
    let dir = tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    for (name, sub) in funcs {
        let file = root.join(sub).join(format!("{}.rs", name));
        fs::write(&file, format!("fn {}() {{}}\n", name)).unwrap();
    }
    let index = DefinitionIndex::from_repo(&root);
    (dir, index)
}

fn assert_clean(content: &str, index: &DefinitionIndex) {
    let v = scan(content, &fixture_path(), index);
    assert!(v.is_empty(), "expected no violations, got: {:?}", v);
}

fn assert_violations(content: &str, index: &DefinitionIndex) -> Vec<Violation> {
    scan(content, &fixture_path(), index)
}

#[test]
fn identifier_with_definition_passes() {
    let (_d, index) = fixture_index(&[("test_my_function_name", "tests")]);
    let content = "## Tasks\n\nWrite `test_my_function_name`.\n";
    assert_clean(content, &index);
}

#[test]
fn identifier_with_no_definition_fires() {
    let (_d, index) = fixture_index(&[]);
    let content = "## Tasks\n\nWrite `test_my_function_name`.\n";
    let v = assert_violations(content, &index);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].identifier, "test_my_function_name");
}

#[test]
fn definition_in_src_satisfies() {
    let (_d, index) = fixture_index(&[("my_helper_function", "src")]);
    let content = "## Tasks\n\nCall `my_helper_function`.\n";
    assert_clean(content, &index);
}

#[test]
fn identifier_outside_tasks_section_ignored() {
    // Identifiers in Context, Risks, Approach, etc. should NOT
    // trigger — those sections cite existing code.
    let (_d, index) = fixture_index(&[]);
    let content =
        "## Context\n\nThe existing `nonexistent_helper_fn` is broken.\n\n## Approach\n\nFix it.\n";
    assert_clean(content, &index);
}

#[test]
fn identifier_in_nested_subheading_under_tasks_scanned() {
    let (_d, index) = fixture_index(&[]);
    let content = "## Tasks\n\n### Task 1: write test\n\nThe test is `nonexistent_test_name`.\n";
    let v = assert_violations(content, &index);
    assert_eq!(v.len(), 1);
}

#[test]
fn fenced_block_identifiers_ignored() {
    let (_d, index) = fixture_index(&[]);
    let content = "## Tasks\n\n```\n`nonexistent_test_name`\n```\n";
    assert_clean(content, &index);
}

#[test]
fn short_identifier_below_length_filter_ignored() {
    // 9 chars or fewer is below the threshold (≥10).
    let (_d, index) = fixture_index(&[]);
    let content = "## Tasks\n\nUse `short_fn`.\n";
    assert_clean(content, &index);
}

#[test]
fn non_snake_case_ignored() {
    let (_d, index) = fixture_index(&[]);
    let content = "## Tasks\n\nSee `Some::Path::Type` and `kebab-case-name`.\n";
    // Neither matches the snake_case filter.
    assert_clean(content, &index);
}

#[test]
fn opt_out_on_trigger_line_suppresses() {
    let (_d, index) = fixture_index(&[]);
    let content = "## Tasks\n\nExisting test `nonexistent_test_name`. <!-- verify-references: prose-citation -->\n";
    assert_clean(content, &index);
}

#[test]
fn opt_out_directly_above_suppresses() {
    let (_d, index) = fixture_index(&[]);
    let content = "## Tasks\n\n<!-- verify-references: prose-citation -->\nExisting test `nonexistent_test_name`.\n";
    assert_clean(content, &index);
}

#[test]
fn opt_out_two_lines_above_with_blank_suppresses() {
    let (_d, index) = fixture_index(&[]);
    let content = "## Tasks\n\n<!-- verify-references: prose-citation -->\n\nExisting test `nonexistent_test_name`.\n";
    assert_clean(content, &index);
}

#[test]
fn opt_out_three_lines_above_does_not_suppress() {
    let (_d, index) = fixture_index(&[]);
    let content = "## Tasks\n\n<!-- verify-references: prose-citation -->\n\n\nExisting test `nonexistent_test_name`.\n";
    assert_eq!(scan(content, &fixture_path(), &index).len(), 1);
}

#[test]
fn duplicate_identifier_dedup() {
    let (_d, index) = fixture_index(&[]);
    let content = "## Tasks\n\nUse `nonexistent_test_name`.\nAlso use `nonexistent_test_name`.\n";
    let v = assert_violations(content, &index);
    assert_eq!(v.len(), 1);
}

#[test]
fn cap_limits_identifiers() {
    // 35 unique nonexistent identifiers; cap is 30.
    let (_d, index) = fixture_index(&[]);
    let mut content = String::from("## Tasks\n\n");
    for i in 0..35 {
        content.push_str(&format!("`nonexistent_test_name_{:02}`\n", i));
    }
    let v = scan(&content, &fixture_path(), &index);
    assert!(v.len() <= 30, "got {} violations", v.len());
}

#[test]
fn empty_content_clean() {
    let (_d, index) = fixture_index(&[]);
    assert_clean("", &index);
}

#[test]
fn no_tasks_section_clean() {
    let (_d, index) = fixture_index(&[]);
    let content = "## Context\n\nUse `nonexistent_helper_fn`.\n";
    assert_clean(content, &index);
}

#[test]
fn definition_index_walks_subdirectories() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let nested = root.join("tests").join("subdir");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("nested.rs"), "fn deeply_nested_function() {}\n").unwrap();
    let index = DefinitionIndex::from_repo(root);
    assert!(index.contains("deeply_nested_function"));
}

#[test]
fn definition_index_skips_target() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let target = root.join("tests").join("target");
    fs::create_dir_all(&target).unwrap();
    fs::write(
        target.join("excluded.rs"),
        "fn excluded_fn_name_here() {}\n",
    )
    .unwrap();
    let index = DefinitionIndex::from_repo(root);
    assert!(!index.contains("excluded_fn_name_here"));
}

#[test]
fn definition_index_skips_dotted_dirs() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let dotted = root.join("tests").join(".hidden");
    fs::create_dir_all(&dotted).unwrap();
    fs::write(dotted.join("excluded.rs"), "fn hidden_fn_name_here() {}\n").unwrap();
    let index = DefinitionIndex::from_repo(root);
    assert!(!index.contains("hidden_fn_name_here"));
}

#[test]
fn definition_index_skips_symlinks() {
    use std::os::unix::fs::symlink;
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    let real = root.join("real.rs");
    fs::write(&real, "fn real_function_name() {}\n").unwrap();
    let link = root.join("tests").join("link_to_real.rs");
    let _ = symlink(&real, &link);
    let index = DefinitionIndex::from_repo(root);
    // Only the original `real.rs` (under root, not in tests/)
    // would be indexed if it were in src/. With it at the root,
    // it's not indexed at all.
    assert!(!index.contains("real_function_name"));
}

#[test]
fn definition_index_ignores_non_rs_files() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src").join("ignored.txt"),
        "fn would_be_indexed() {}\n",
    )
    .unwrap();
    let index = DefinitionIndex::from_repo(root);
    assert!(!index.contains("would_be_indexed"));
}

#[test]
fn definition_index_handles_missing_dirs() {
    let dir = tempdir().unwrap();
    let index = DefinitionIndex::from_repo(dir.path());
    assert!(!index.contains("anything"));
}

// --- ### Tasks subsection support ---

#[test]
fn level_three_tasks_section_honored() {
    let (_d, index) = fixture_index(&[]);
    let content = "## Plan\n\n### Tasks\n\nWrite `nonexistent_test_function_name`.\n";
    let v = scan(content, &fixture_path(), &index);
    assert!(!v.is_empty(), "level-3 Tasks heading must be honored");
}

#[test]
fn level_four_tasks_section_honored() {
    let (_d, index) = fixture_index(&[]);
    let content =
        "## Plan\n\n### Subplan\n\n#### Tasks\n\nWrite `nonexistent_test_function_name`.\n";
    let v = scan(content, &fixture_path(), &index);
    assert!(!v.is_empty(), "level-4 Tasks heading must be honored");
}

#[test]
fn nested_subheading_inside_tasks_does_not_end_section() {
    let (_d, index) = fixture_index(&[]);
    let content = "## Tasks\n\n### Task 1: thing\n\nWrite `nonexistent_test_function_name`.\n";
    let v = scan(content, &fixture_path(), &index);
    assert!(
        !v.is_empty(),
        "deeper subheading inside Tasks must keep the section open"
    );
}

#[test]
fn equal_level_heading_after_level_three_tasks_ends_section() {
    let (_d, index) = fixture_index(&[]);
    let content =
        "## Plan\n\n### Tasks\n\nFoo.\n\n### Other\n\nWrite `nonexistent_test_function_name`.\n";
    let v = scan(content, &fixture_path(), &index);
    assert!(
        v.is_empty(),
        "equal-level heading must end the Tasks section: {:?}",
        v
    );
}

// --- Tilde-fence support ---

#[test]
fn tilde_fenced_block_identifiers_ignored() {
    let (_d, index) = fixture_index(&[]);
    let content = "## Tasks\n\n~~~rust\nfn `nonexistent_test_function_name`() {}\n~~~\n";
    let v = scan(content, &fixture_path(), &index);
    assert!(
        v.is_empty(),
        "identifiers inside ~~~ fence must be masked: {:?}",
        v
    );
}

#[test]
fn backtick_inside_tilde_fence_does_not_close_outer() {
    let (_d, index) = fixture_index(&[]);
    let content = "## Tasks\n\n~~~text\n```\n`nonexistent_test_function_name`\n```\n~~~\n";
    let v = scan(content, &fixture_path(), &index);
    assert!(
        v.is_empty(),
        "backtick fence inside tilde fence is content, not a closer: {:?}",
        v
    );
}

// --- Path-prefixed identifiers ---

#[test]
fn path_prefixed_identifier_in_named_file_passes() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("tests/foo.rs"), "fn bar_baz_quux_helper() {}\n").unwrap();
    let index = DefinitionIndex::from_repo(root);
    let content = "## Tasks\n\nWrite `tests/foo.rs::bar_baz_quux_helper`.\n";
    let v = scan(content, &fixture_path(), &index);
    assert!(
        v.is_empty(),
        "path-prefixed lookup must succeed in named file: {:?}",
        v
    );
}

#[test]
fn path_prefixed_identifier_in_different_file_fires() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("tests/other.rs"), "fn bar_baz_quux_helper() {}\n").unwrap();
    let index = DefinitionIndex::from_repo(root);
    let content = "## Tasks\n\nWrite `tests/foo.rs::bar_baz_quux_helper`.\n";
    let v = scan(content, &fixture_path(), &index);
    assert_eq!(
        v.len(),
        1,
        "path-prefixed lookup must fail when defined in different file"
    );
}

#[test]
fn path_prefixed_src_form_works() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/foo.rs"), "fn process_input_helper() {}\n").unwrap();
    let index = DefinitionIndex::from_repo(root);
    let content = "## Tasks\n\nWrite `src/foo.rs::process_input_helper`.\n";
    let v = scan(content, &fixture_path(), &index);
    assert!(v.is_empty(), "src/-prefixed path must work: {:?}", v);
}

#[test]
fn path_prefixed_short_name_below_length_filter_skipped() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    let index = DefinitionIndex::from_repo(root);
    let content = "## Tasks\n\nWrite `tests/foo.rs::bar`.\n";
    let v = scan(content, &fixture_path(), &index);
    assert!(
        v.is_empty(),
        "short name in path-prefixed form must be skipped: {:?}",
        v
    );
}

#[test]
fn path_prefixed_empty_name_falls_back() {
    // `tests/foo.rs::` with empty name part — parse_path_prefixed_ident
    // returns None, falls back to bare-identifier path. The whole token
    // contains `::` so doesn't match the snake_case shape, gets skipped.
    let (_d, index) = fixture_index(&[]);
    let content = "## Tasks\n\nWrite `tests/foo.rs::`.\n";
    let v = scan(content, &fixture_path(), &index);
    assert!(
        v.is_empty(),
        "empty name in path-prefixed form must skip: {:?}",
        v
    );
}

#[test]
fn path_prefixed_invalid_path_prefix_falls_back() {
    // A `::` that isn't preceded by tests/ or src/ falls back to
    // the bare identifier path. The whole token is treated as a
    // shape candidate. Since "foo::bar_baz_quux_helper" doesn't
    // match the snake_case-only shape regex, it's skipped.
    let (_d, index) = fixture_index(&[]);
    let content = "## Tasks\n\nWrite `foo::bar_baz_quux_helper`.\n";
    let v = scan(content, &fixture_path(), &index);
    assert!(
        v.is_empty(),
        "non-tests/non-src path-prefixed form must skip: {:?}",
        v
    );
}

// --- contains_in_file public API ---

#[test]
fn definition_index_contains_in_file_matches_path() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("tests/foo.rs"), "fn my_helper_function() {}\n").unwrap();
    let index = DefinitionIndex::from_repo(root);
    assert!(index.contains_in_file("my_helper_function", &PathBuf::from("tests/foo.rs")));
    assert!(!index.contains_in_file("my_helper_function", &PathBuf::from("tests/bar.rs")));
    assert!(!index.contains_in_file("nonexistent_name", &PathBuf::from("tests/foo.rs")));
}

// --- index_dir error handling (does not panic) ---

#[test]
fn definition_index_skips_unreadable_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    let readable = root.join("tests/readable.rs");
    fs::write(&readable, "fn readable_helper_one() {}\n").unwrap();
    let unreadable = root.join("tests/unreadable.rs");
    fs::write(&unreadable, "fn would_be_indexed_two() {}\n").unwrap();
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();
    // Must not panic; readable file should still index.
    let index = DefinitionIndex::from_repo(root);
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(index.contains("readable_helper_one"));
    assert!(!index.contains("would_be_indexed_two"));
}

#[test]
fn definition_index_skips_unreadable_directory() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::create_dir_all(root.join("tests/locked")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("tests/locked/x.rs"),
        "fn locked_helper_one() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("tests/sibling.rs"),
        "fn sibling_helper_two() {}\n",
    )
    .unwrap();
    // chmod 000 on the locked directory — fs::read_dir on it will
    // fail with EACCES, exercising the read_dir Err branch.
    fs::set_permissions(root.join("tests/locked"), fs::Permissions::from_mode(0o000)).unwrap();
    let index = DefinitionIndex::from_repo(root);
    fs::set_permissions(root.join("tests/locked"), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        index.contains("sibling_helper_two"),
        "siblings still indexed"
    );
    assert!(
        !index.contains("locked_helper_one"),
        "unreadable dir contents skipped"
    );
}

#[test]
fn definition_index_skips_invalid_utf8_file() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    // A .rs file containing invalid UTF-8 bytes. read_to_string returns
    // InvalidData Err, exercising the read_to_string Err branch.
    let path = root.join("tests/badbytes.rs");
    fs::write(&path, [0xFF, 0xFE, 0x00, 0x80, 0xC3, 0x28]).unwrap();
    fs::write(root.join("tests/good.rs"), "fn good_neighbor_helper() {}\n").unwrap();
    let index = DefinitionIndex::from_repo(root);
    assert!(index.contains("good_neighbor_helper"));
}

// --- Byte cap on file reads ---

#[test]
fn definition_index_caps_oversized_file() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    // Build a file > 4 MB; the function `late_file_late_helper`
    // appears past the cap and should NOT be indexed.
    let path = root.join("tests/big.rs");
    let pad = "//".to_string() + &"x".repeat(5 * 1024 * 1024);
    let content = format!(
        "fn early_indexed_helper() {{}}\n{}\nfn late_file_late_helper() {{}}\n",
        pad
    );
    fs::write(&path, content).unwrap();
    let index = DefinitionIndex::from_repo(root);
    assert!(index.contains("early_indexed_helper"));
    assert!(!index.contains("late_file_late_helper"));
}

#[test]
fn path_prefixed_duplicate_seen_skipped() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    let index = DefinitionIndex::from_repo(root);
    // Same path-prefixed identifier cited twice — second occurrence
    // hits the seen-duplicate branch in the path-prefix arm.
    let content = "## Tasks\n\nWrite `tests/foo.rs::bar_baz_quux_helper` then `tests/foo.rs::bar_baz_quux_helper` again.\n";
    let v = scan(content, &fixture_path(), &index);
    assert_eq!(
        v.len(),
        1,
        "duplicate path-prefixed identifier must produce one violation"
    );
}

#[test]
fn heading_level_hash_only_not_a_heading() {
    // A line that is purely `#` (no trailing space) is not a Markdown
    // heading. Drive this through the public scan() API: a `#`-only
    // line inside Tasks must NOT close the section, and a `#`-only
    // line outside Tasks must NOT open one.
    let (_d, index) = fixture_index(&[]);
    let content = "## Tasks\n\n#\nWrite `nonexistent_test_function_name`.\n";
    let v = scan(content, &fixture_path(), &index);
    assert_eq!(v.len(), 1, "lone # must not close the Tasks section");
}

#[test]
fn unclosed_fence_at_eof_fails_open() {
    let (_d, index) = fixture_index(&[]);
    let content = "## Tasks\n\n```\n`nonexistent_test_name`\n";
    let v = scan(content, &fixture_path(), &index);
    assert_eq!(v.len(), 1, "got: {:?}", v);
}
