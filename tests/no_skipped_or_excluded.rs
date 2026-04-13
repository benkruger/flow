//! Zero-tolerance contract test: no skipped tests, no coverage exclusions,
//! no blanket lint silencers anywhere in `src/*.rs` or `tests/*.rs`.
//!
//! Banned patterns:
//! - Skip markers: `#[ignore]`, `#[ignore =`, `#[ignore(`
//! - Coverage exclusions: `coverage(off)`, `coverage_nightly`,
//!   `GRCOV_EXCL_{LINE,START,STOP,BR_LINE}`, `LCOV_EXCL_{LINE,START,STOP}`
//! - Blanket lint silencers: `#![allow(warnings)]`, `#[allow(warnings)]`,
//!   `#![allow(clippy::all)]`, `#[allow(clippy::all)]`
//!
//! Targeted `#[allow(...)]` (e.g., `#[allow(dead_code)]`) is permitted —
//! surgical exceptions for legitimate cases like shared test helpers are
//! distinct from blanket "shut up the linter" silencers.
//!
//! This test file itself contains every banned pattern as a search string.
//! Self-exclusion uses canonicalized path comparison so the file scans
//! every other Rust source without flagging itself.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN: &[&str] = &[
    "#[ignore]",
    "#[ignore =",
    "#[ignore(",
    "coverage(off)",
    "coverage_nightly",
    "GRCOV_EXCL_LINE",
    "GRCOV_EXCL_START",
    "GRCOV_EXCL_STOP",
    "GRCOV_EXCL_BR_LINE",
    "LCOV_EXCL_LINE",
    "LCOV_EXCL_START",
    "LCOV_EXCL_STOP",
    "#![allow(warnings)]",
    "#[allow(warnings)]",
    "#![allow(clippy::all)]",
    "#[allow(clippy::all)]",
];

fn scan_dir(dir: &Path, violations: &mut Vec<String>, self_canon: &Path) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir(&path, violations, self_canon);
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        if let Ok(canon) = path.canonicalize() {
            if canon == self_canon {
                continue;
            }
        }
        scan_file(&path, violations);
    }
}

fn scan_file(path: &Path, violations: &mut Vec<String>) {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };
    for (lineno, line) in content.lines().enumerate() {
        for pattern in FORBIDDEN {
            if line.contains(pattern) {
                violations.push(format!(
                    "{}:{}: contains forbidden pattern `{}`",
                    path.display(),
                    lineno + 1,
                    pattern
                ));
            }
        }
    }
}

#[test]
fn no_skipped_tests_or_coverage_exclusions() {
    let root = common::repo_root();
    let self_rel = PathBuf::from(file!());
    let self_abs = if self_rel.is_absolute() {
        self_rel
    } else {
        root.join(&self_rel)
    };
    let self_canon = self_abs
        .canonicalize()
        .expect("contract test file must exist on disk");

    let mut violations = Vec::new();
    scan_dir(&root.join("src"), &mut violations, &self_canon);
    scan_dir(&root.join("tests"), &mut violations, &self_canon);

    assert!(
        violations.is_empty(),
        "Zero-tolerance contract violated — no #[ignore], no coverage exclusions, \
         no blanket lint silencers permitted. Delete the offending construct, fix \
         the underlying problem, or change the rule itself if it genuinely needs \
         to change.\n\nViolations ({}):\n{}",
        violations.len(),
        violations.join("\n")
    );
}
