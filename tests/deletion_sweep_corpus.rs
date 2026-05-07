//! Corpus contract test for `src/deletion_sweep_scanner.rs`.
//!
//! Scans the committed prose corpus and asserts no Gate 2
//! violations. Per the
//! `.claude/rules/tests-guard-real-regressions.md` "Corpus-scan
//! viability check," the initial run is graded by false-positive
//! count — under 5 → corpus contract test viable; over 5 → defer
//! with documented marker. This file SHIPS as the contract test;
//! if the count grows past 5 in the future, swap with a
//! documented-marker file.
//!
//! Named regression: a future commit adds prose proposing a
//! delete/rename of a backtick-quoted identifier without nearby
//! sweep evidence (file bullets, Exploration heading, or table).
//! Named consumer: `.claude/rules/docs-with-behavior.md` "Scope
//! Enumeration (Rename Side)".

use std::fs;
use std::path::{Path, PathBuf};

use flow_rs::deletion_sweep_scanner::scan;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn collect_corpus(root: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    let claude_md = root.join("CLAUDE.md");
    if claude_md.is_file() {
        paths.push(claude_md);
    }
    walk_md(&root.join(".claude/rules"), &mut paths);
    walk_md(&root.join("skills"), &mut paths);
    walk_md(&root.join(".claude/skills"), &mut paths);
    paths
}

fn walk_md(dir: &Path, paths: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_md(&path, paths);
        } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
            paths.push(path);
        }
    }
}

#[test]
fn deletion_sweep_corpus_is_clean() {
    let root = repo_root();
    let corpus = collect_corpus(&root);
    let mut total_violations = 0;
    let mut details: Vec<String> = Vec::new();
    for path in &corpus {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let violations = scan(&content, path);
        for v in violations {
            total_violations += 1;
            details.push(format!(
                "{}:{} — `{}` ({})",
                v.file.display(),
                v.line,
                v.identifier,
                v.context.trim()
            ));
        }
    }
    assert_eq!(
        total_violations,
        0,
        "deletion-sweep violations in committed corpus:\n{}",
        details.join("\n")
    );
}
