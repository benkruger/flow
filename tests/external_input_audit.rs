//! Contract tests for the external-input-audit rule.
//!
//! Scans the committed prose corpus (`CLAUDE.md`, `.claude/rules/*.md`,
//! `skills/**/SKILL.md`, `.claude/skills/**/SKILL.md`) for panic/assert
//! tightening prose that lacks an accompanying callsite
//! source-classification audit table. These are the authoring surfaces
//! covered by the external-input-validation rule; plan files
//! (per-branch, ephemeral) are covered at runtime by
//! `bin/flow plan-check`.
//!
//! The rule itself exists at
//! `.claude/rules/external-input-audit-gate.md` and is the primary
//! instrument — this scanner is the merge-conflict trip-wire that
//! locks in the clean state once and fails CI on future regressions.

use std::fs;
use std::path::PathBuf;

use flow_rs::external_input_audit::{scan, Violation};

mod common;

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
fn claude_md_has_no_unaudited_tightenings() {
    let path = common::repo_root().join("CLAUDE.md");
    let content = fs::read_to_string(&path).expect("CLAUDE.md must exist");
    let violations = scan(&content, &path);
    assert!(
        violations.is_empty(),
        "CLAUDE.md has panic/assert tightening prose without an audit table:\n{}",
        format_violations(&violations)
    );
}

// --- scan .claude/rules/*.md ---

#[test]
fn rules_have_no_unaudited_tightenings() {
    let rules_dir = common::repo_root().join(".claude").join("rules");
    let files = read_md_files(&rules_dir);
    assert!(!files.is_empty(), "expected .claude/rules/*.md files");
    let mut all: Vec<Violation> = Vec::new();
    for (path, content) in &files {
        let vs = scan(content, path);
        all.extend(vs);
    }
    assert!(
        all.is_empty(),
        ".claude/rules/ has panic/assert tightening prose without an audit table:\n{}",
        format_violations(&all)
    );
}

// --- scan skills/**/SKILL.md ---

#[test]
fn plugin_skills_have_no_unaudited_tightenings() {
    let skills_dir = common::repo_root().join("skills");
    let files = read_md_files(&skills_dir);
    assert!(!files.is_empty(), "expected skills/**/SKILL.md files");
    let mut all: Vec<Violation> = Vec::new();
    for (path, content) in &files {
        let vs = scan(content, path);
        all.extend(vs);
    }
    assert!(
        all.is_empty(),
        "skills/ has panic/assert tightening prose without an audit table:\n{}",
        format_violations(&all)
    );
}

// --- scan .claude/skills/**/SKILL.md ---

#[test]
fn private_skills_have_no_unaudited_tightenings() {
    let skills_dir = common::repo_root().join(".claude").join("skills");
    let files = read_md_files(&skills_dir);
    // Private skills directory may not exist in all repos, but in
    // this one it contains maintainer-only skills. Skip if empty.
    if files.is_empty() {
        return;
    }
    let mut all: Vec<Violation> = Vec::new();
    for (path, content) in &files {
        let vs = scan(content, path);
        all.extend(vs);
    }
    assert!(
        all.is_empty(),
        ".claude/skills/ has panic/assert tightening prose without an audit table:\n{}",
        format_violations(&all)
    );
}
