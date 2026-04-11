//! Contract tests for the scope-enumeration rule.
//!
//! Scans the committed prose corpus (CLAUDE.md, `.claude/rules/*.md`,
//! `skills/**/SKILL.md`, `.claude/skills/**/SKILL.md`) for
//! universal-coverage language that lacks a named enumeration nearby.
//! These are the authoring surfaces covered by the scope-enumeration
//! rule; plan files (per-branch, ephemeral) are covered at runtime by
//! `bin/flow plan-check`.
//!
//! The rule itself exists at `.claude/rules/scope-enumeration.md`
//! (added in a follow-up commit) and is the primary instrument — this
//! scanner is the merge-conflict trip-wire that locks in the clean
//! state once and fails CI on future regressions.

use std::fs;
use std::path::PathBuf;

use flow_rs::scope_enumeration::{scan, Violation};

mod common;

/// Pretty-print a list of violations for assertion failure messages.
fn format_violations(violations: &[Violation]) -> String {
    let mut s = String::new();
    for v in violations {
        s.push_str(&format!(
            "  {}:{} — {}\n    context: {}\n",
            v.file.display(),
            v.line,
            v.phrase,
            v.context.trim()
        ));
    }
    s
}

/// Read every `.md` file under a directory recursively and return
/// `(absolute_path, content)` pairs.
fn read_md_files(dir: &PathBuf) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    walk(dir, &mut out);
    out
}

fn walk(dir: &PathBuf, out: &mut Vec<(PathBuf, String)>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                if let Ok(content) = fs::read_to_string(&path) {
                    out.push((path, content));
                }
            }
        }
    }
}

// --- scan CLAUDE.md ---

#[test]
fn claude_md_has_no_unenumerated_universal_claims() {
    let path = common::repo_root().join("CLAUDE.md");
    let content = fs::read_to_string(&path).expect("CLAUDE.md must exist");
    let violations = scan(&content, &path);
    assert!(
        violations.is_empty(),
        "CLAUDE.md has unenumerated universal-coverage prose:\n{}",
        format_violations(&violations)
    );
}

// --- scan .claude/rules/*.md ---

#[test]
fn rules_have_no_unenumerated_universal_claims() {
    let rules_dir = common::repo_root().join(".claude").join("rules");
    let files = read_md_files(&rules_dir);
    assert!(!files.is_empty(), "expected .claude/rules/*.md files");

    let mut all_violations = Vec::new();
    for (path, content) in &files {
        let violations = scan(content, path);
        all_violations.extend(violations);
    }

    assert!(
        all_violations.is_empty(),
        ".claude/rules/ has unenumerated universal-coverage prose:\n{}",
        format_violations(&all_violations)
    );
}

// --- scan skills/**/SKILL.md (phase + utility skills) ---

#[test]
fn skills_have_no_unenumerated_universal_claims() {
    let skills_dir = common::skills_dir();
    let files = read_md_files(&skills_dir);
    assert!(!files.is_empty(), "expected skills/**/SKILL.md files");

    let mut all_violations = Vec::new();
    for (path, content) in &files {
        if path.file_name().and_then(|f| f.to_str()) != Some("SKILL.md") {
            continue;
        }
        let violations = scan(content, path);
        all_violations.extend(violations);
    }

    assert!(
        all_violations.is_empty(),
        "skills/**/SKILL.md has unenumerated universal-coverage prose:\n{}",
        format_violations(&all_violations)
    );
}

// --- scan .claude/skills/**/SKILL.md (maintainer skills) ---

#[test]
fn maintainer_skills_have_no_unenumerated_universal_claims() {
    let dot_skills_dir = common::repo_root().join(".claude").join("skills");
    if !dot_skills_dir.exists() {
        return;
    }
    let files = read_md_files(&dot_skills_dir);

    let mut all_violations = Vec::new();
    for (path, content) in &files {
        if path.file_name().and_then(|f| f.to_str()) != Some("SKILL.md") {
            continue;
        }
        let violations = scan(content, path);
        all_violations.extend(violations);
    }

    assert!(
        all_violations.is_empty(),
        ".claude/skills/**/SKILL.md has unenumerated universal-coverage prose:\n{}",
        format_violations(&all_violations)
    );
}
