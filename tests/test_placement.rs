//! Contract test for `.claude/rules/test-placement.md`.
//!
//! Scans every `.rs` file under `src/` and flags any line that
//! contains the literal `#[cfg(test)]` outside a `//` line comment.
//! The check is context-free — it flags real attributes, block
//! comments, raw strings, and normal string literals alike. One
//! canonical escape exists for src files that genuinely need the
//! characters in a string literal: `concat!("#[cfg", "(test)]")`.
//! See the rule's Enforcement section for the full decision tree.
//!
//! This contract test is expected to fail until every src file has
//! been migrated. It's the red-from-day-one drift tripwire named in
//! the rule's Enforcement section.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name.starts_with('.') || name == "target" {
            continue;
        }
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            collect_rust_files(&path, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// The attribute we're banning from `src/`. Split via `concat!` so
/// this contract test file does not itself contain the exact
/// substring — keeps the scanner's logic self-consistent if it's
/// ever extended to scan `tests/` or similar.
const BANNED_ATTR: &str = concat!("#[cfg", "(test)]");

#[test]
fn src_contains_no_inline_cfg_test_blocks() {
    let src = common::repo_root().join("src");
    let mut files = Vec::new();
    collect_rust_files(&src, &mut files);
    assert!(
        !files.is_empty(),
        "expected src/ to contain .rs files — is the test running from the repo root?"
    );

    let mut violations: Vec<String> = Vec::new();
    for file in &files {
        let content = match fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for (idx, line) in content.lines().enumerate() {
            // Strip `//` line comments (including `///` and `//!`
            // doc comments) before scanning — those are the only
            // contexts where mentioning the attribute is allowed.
            // Block comments, raw strings, and normal string
            // literals are intentionally NOT stripped: the rule
            // flags the literal substring everywhere else, and the
            // one canonical escape is `concat!("#[cfg", "(test)]")`
            // per `.claude/rules/test-placement.md` Enforcement.
            let code = match line.find("//") {
                Some(cut) => &line[..cut],
                None => line,
            };
            if code.contains(BANNED_ATTR) {
                let rel = file.strip_prefix(common::repo_root()).unwrap_or(file);
                violations.push(format!("  {}:{}", rel.display(), idx + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "src/*.rs must not contain inline {attr} blocks. Migrate tests \
         to tests/<name>.rs per .claude/rules/test-placement.md.\n\
         Violations ({count}):\n{list}",
        attr = BANNED_ATTR,
        count = violations.len(),
        list = violations.join("\n"),
    );
}
