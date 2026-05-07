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

#[test]
fn unclosed_fence_at_eof_fails_open() {
    let (_d, index) = fixture_index(&[]);
    let content = "## Tasks\n\n```\n`nonexistent_test_name`\n";
    let v = scan(content, &fixture_path(), &index);
    assert_eq!(v.len(), 1, "got: {:?}", v);
}
