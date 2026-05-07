//! Corpus contract test for `src/cli_output_contract_scanner.rs`.
//!
//! Scans the committed prose corpus (`CLAUDE.md`,
//! `.claude/rules/*.md`, `skills/**/SKILL.md`,
//! `.claude/skills/**/SKILL.md`) and asserts that no Gate 1
//! violations exist. Per the
//! `.claude/rules/tests-guard-real-regressions.md` "Corpus-scan
//! viability check" — the initial run produced 1 false positive
//! (the rule file's own example sentence), which is below the
//! ≥5 high-false-positive threshold. The single match was opted
//! out via `<!-- cli-output-contracts: not-a-new-flag -->` on the
//! trigger line; the corpus is now clean and this test guards
//! against future drift.
//!
//! Named regression: a future commit adds prose proposing a new
//! flag/subcommand with consumed output but omits the four-item
//! contract block (output format, exit codes, error messages,
//! fallback). Named consumer: `.claude/rules/cli-output-contracts.md`
//! and the Plan-phase gate that depends on the rule's own corpus
//! being clean.

use std::fs;
use std::path::{Path, PathBuf};

use flow_rs::cli_output_contract_scanner::scan;

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
fn cli_output_contracts_corpus_is_clean() {
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
                "{}:{} — {} (missing: {})",
                v.file.display(),
                v.line,
                v.context.trim(),
                v.missing_items.join(", ")
            ));
        }
    }
    assert_eq!(
        total_violations,
        0,
        "cli-output-contracts violations in committed corpus:\n{}",
        details.join("\n")
    );
}
