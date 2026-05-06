//! Integration tests for `src/protected_paths.rs`.
//!
//! Per .claude/rules/test-placement.md, tests live at
//! tests/<name>.rs mirroring src/<name>.rs and drive the subject
//! through the public interface.

use std::path::Path;

use flow_rs::protected_paths::is_protected_path;

// --- is_protected_path ---

#[test]
fn empty_path_is_not_protected() {
    assert!(!is_protected_path(Path::new("")));
}

#[test]
fn claude_rules_path_is_protected() {
    assert!(is_protected_path(Path::new(
        "/project/.claude/rules/foo.md"
    )));
}

#[test]
fn claude_md_at_root_is_protected() {
    assert!(is_protected_path(Path::new("/project/CLAUDE.md")));
}

#[test]
fn claude_skills_path_is_protected() {
    assert!(is_protected_path(Path::new(
        "/project/.claude/skills/foo/SKILL.md"
    )));
}

#[test]
fn claude_settings_json_is_not_protected() {
    assert!(!is_protected_path(Path::new(
        "/project/.claude/settings.json"
    )));
}

#[test]
fn claude_settings_local_json_is_not_protected() {
    assert!(!is_protected_path(Path::new(
        "/project/.claude/settings.local.json"
    )));
}

#[test]
fn nested_claude_rules_path_is_protected() {
    assert!(is_protected_path(Path::new(
        "/project/.claude/rules/subdir/deep.md"
    )));
}

#[test]
fn nested_claude_skills_path_is_protected() {
    assert!(is_protected_path(Path::new(
        "/project/.claude/skills/subdir/deep/SKILL.md"
    )));
}

#[test]
fn worktree_claude_rules_path_is_protected() {
    assert!(is_protected_path(Path::new(
        "/project/.worktrees/feat/.claude/rules/foo.md"
    )));
}

#[test]
fn worktree_claude_md_is_protected() {
    assert!(is_protected_path(Path::new(
        "/project/.worktrees/feat/CLAUDE.md"
    )));
}

#[test]
fn worktree_claude_skills_path_is_protected() {
    assert!(is_protected_path(Path::new(
        "/project/.worktrees/feat/.claude/skills/foo/SKILL.md"
    )));
}

#[test]
fn mixed_case_claude_md_basename_is_protected() {
    assert!(is_protected_path(Path::new("/project/Claude.md")));
    assert!(is_protected_path(Path::new("/project/claude.md")));
}

#[test]
fn mixed_case_claude_dir_is_protected() {
    assert!(is_protected_path(Path::new(
        "/project/.CLAUDE/rules/foo.md"
    )));
    assert!(is_protected_path(Path::new(
        "/project/.Claude/rules/foo.md"
    )));
}

#[test]
fn mixed_case_rules_and_skills_dirs_are_protected() {
    assert!(is_protected_path(Path::new(
        "/project/.claude/Rules/foo.md"
    )));
    assert!(is_protected_path(Path::new(
        "/project/.claude/SKILLS/foo/SKILL.md"
    )));
}

#[test]
fn unrelated_source_file_is_not_protected() {
    assert!(!is_protected_path(Path::new("/project/src/lib.rs")));
}

#[test]
fn relative_and_absolute_paths_match_identically() {
    // Relative `.claude/rules/foo.md` and absolute `/project/.claude/rules/foo.md`
    // both resolve to a `.claude/rules/...` component sequence, so the
    // classifier returns `true` for both. This guards the (currently
    // unused) callsite that may pass a relative path computed from a
    // git-relative file_path field.
    assert!(is_protected_path(Path::new(".claude/rules/foo.md")));
    assert!(is_protected_path(Path::new(
        "/project/.claude/rules/foo.md"
    )));

    assert!(is_protected_path(Path::new(".claude/skills/foo/SKILL.md")));
    assert!(is_protected_path(Path::new(
        "/project/.claude/skills/foo/SKILL.md"
    )));

    assert!(is_protected_path(Path::new("CLAUDE.md")));
    assert!(is_protected_path(Path::new("/project/CLAUDE.md")));

    assert!(!is_protected_path(Path::new("src/lib.rs")));
    assert!(!is_protected_path(Path::new("/project/src/lib.rs")));
}

#[test]
fn settings_basename_only_is_not_protected() {
    // CLAUDE.md basename is the trigger; settings.json basename is not.
    // Driving with a bare basename (no directory component) covers the
    // edge case where the path is a single file with no parent.
    assert!(!is_protected_path(Path::new("settings.json")));
    assert!(is_protected_path(Path::new("CLAUDE.md")));
}
