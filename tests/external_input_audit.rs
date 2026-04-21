//! Integration tests for the external-input-audit scanner.
//!
//! Two roles:
//!
//! 1. Contract tests scan the committed prose corpus (`CLAUDE.md`,
//!    `.claude/rules/*.md`, `skills/**/SKILL.md`,
//!    `.claude/skills/**/SKILL.md`) for panic/assert tightening
//!    prose that lacks an accompanying callsite
//!    source-classification audit table. These are the authoring
//!    surfaces covered by the external-input-validation rule; plan
//!    files (per-branch, ephemeral) are covered at runtime by
//!    `bin/flow plan-check`.
//! 2. Unit-like tests drive the public `scan` surface with
//!    hand-crafted content to exercise every private branch of the
//!    scanner (trigger vocabulary, direct-token detection, fenced
//!    mask, opt-outs, negation discipline, window bounds, section
//!    boundaries, table detection, header aliases, separator shape).
//!
//! The rule itself exists at
//! `.claude/rules/external-input-audit-gate.md` and is the primary
//! instrument — this scanner is the merge-conflict trip-wire that
//! locks in the clean state once and fails CI on future regressions.

use std::fs;
use std::path::{Path, PathBuf};

use flow_rs::external_input_audit::{scan, Violation};

mod common;

fn dummy_path() -> PathBuf {
    PathBuf::from("dummy.md")
}

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

fn read_md_files(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    walk(dir, &mut out);
    out
}

fn walk(dir: &Path, out: &mut Vec<(PathBuf, String)>) {
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

// --- corpus contract tests ---

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

// --- trigger vocabulary ---
// These tests cover every alternation branch of TRIGGER_PATTERN by
// driving content without an accompanying audit table and asserting
// the line flags.

#[test]
fn trigger_matches_tighten_to_panic() {
    let v = scan("tighten FlowPaths::new to panic on empty\n", &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_add_assert() {
    let v = scan(
        "add an assert! that the branch is non-empty\n",
        &dummy_path(),
    );
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_introduce_new_panic() {
    let v = scan("introduce a new panic! in the constructor\n", &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_panic_on_empty() {
    let v = scan("panic on empty input\n", &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_reject_empty() {
    let v = scan("reject empty branches\n", &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_reject_invalid() {
    let v = scan("reject invalid names\n", &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_reject_malformed() {
    let v = scan("reject malformed inputs\n", &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_reject_unsupported() {
    let v = scan("reject unsupported configurations\n", &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_assert_that() {
    let v = scan("assert that the value is non-null\n", &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_enforce_validation_assertion() {
    let v = scan("enforce a validation assertion on input\n", &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_require_invariant_check() {
    let v = scan(
        "require an invariant check in the constructor\n",
        &dummy_path(),
    );
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_introduce_constructor_invariant() {
    let v = scan("introduce a new constructor invariant\n", &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_impose_assert_eq() {
    let v = scan("impose an assert_eq! on the field\n", &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_add_assert_ne() {
    let v = scan("add an assert_ne! to the body\n", &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_is_case_insensitive() {
    let v = scan("Tighten the PANIC on empty input\n", &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_rejects_unrelated_prose() {
    let v = scan("the CI pipeline runs quickly\n", &dummy_path());
    assert!(v.is_empty(), "{:?}", v);
}

#[test]
fn trigger_rejects_panic_without_trigger_verb_or_phrase() {
    // "panic in production is bad" has no verb+noun match and no
    // direct-action phrase.
    let v = scan("panic in production is bad\n", &dummy_path());
    assert!(v.is_empty(), "{:?}", v);
}

// --- direct token pattern ---

#[test]
fn direct_token_matches_assert_macro() {
    let v = scan(
        "the body calls assert!(branch.len() > 0) directly\n",
        &dummy_path(),
    );
    assert!(!v.is_empty(), "{:?}", v);
}

#[test]
fn direct_token_matches_panic_macro() {
    let v = scan(
        "the constructor uses panic!(\"bad input\") here\n",
        &dummy_path(),
    );
    assert!(!v.is_empty(), "{:?}", v);
}

#[test]
fn direct_token_matches_assert_eq_macro() {
    let v = scan("the test body has assert_eq!(a, b) inline\n", &dummy_path());
    assert!(!v.is_empty(), "{:?}", v);
}

#[test]
fn direct_token_matches_assert_ne_macro() {
    let v = scan(
        "we add assert_ne!(a, b) to prevent collisions\n",
        &dummy_path(),
    );
    assert!(!v.is_empty(), "{:?}", v);
}

#[test]
fn direct_token_rejects_bare_assert_word() {
    // "assert" without macro parenthesis is too noisy, must not flag.
    let v = scan("we assert this is correct\n", &dummy_path());
    assert!(v.is_empty(), "{:?}", v);
}

// --- scan: enumeration present (audit table nearby) ---

#[test]
fn scan_passes_forward_audit_table() {
    let content = "tighten FlowPaths::new to panic on empty\n\n| Caller | Source | Classification | Handling |\n|--------|--------|----------------|----------|\n| `current_branch()` | git subprocess | Trusted-but-external | `try_new` |\n";
    let v = scan(content, &dummy_path());
    assert!(
        v.is_empty(),
        "expected no violations with audit table, got {:?}",
        v
    );
}

#[test]
fn scan_passes_backward_audit_table() {
    let content = "| Caller | Source | Classification | Handling |\n|--------|--------|----------------|----------|\n| `x::y` | git | Trusted-but-external | `try_new` |\n\nSo we tighten FlowPaths::new to panic on empty.\n";
    let v = scan(content, &dummy_path());
    assert!(
        v.is_empty(),
        "expected no violations with backward audit table, got {:?}",
        v
    );
}

#[test]
fn scan_passes_without_leading_pipes() {
    let content = "tighten to panic on empty\n\nCaller | Source | Classification | Handling\n-------|--------|----------------|---------\n`x` | git | Trusted-but-external | `try_new`\n";
    let v = scan(content, &dummy_path());
    assert!(
        v.is_empty(),
        "tables without leading pipes must be accepted, got {:?}",
        v
    );
}

#[test]
fn scan_passes_with_alignment_markers() {
    let content = "tighten to panic on empty\n\n| Caller | Source | Classification | Handling |\n|:-------|:------:|:--------------:|---------:|\n| `x` | git | Trusted-but-external | `try_new` |\n";
    let v = scan(content, &dummy_path());
    assert!(
        v.is_empty(),
        "alignment markers must be accepted, got {:?}",
        v
    );
}

#[test]
fn scan_passes_with_callsite_header_alias() {
    let content = "tighten to panic on empty\n\n| Callsite | Source | Classification | Handling |\n|----------|--------|----------------|----------|\n| `x` | git | Trusted-but-external | `try_new` |\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty(), "callsite alias must be accepted, got {:?}", v);
}

#[test]
fn scan_passes_with_class_header_alias() {
    let content = "tighten to panic on empty\n\n| Caller | Source | Class | Handling |\n|--------|--------|-------|----------|\n| `x` | git | Trusted-but-external | `try_new` |\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty(), "class alias must be accepted, got {:?}", v);
}

#[test]
fn scan_passes_with_disposition_header_alias() {
    let content = "tighten to panic on empty\n\n| Caller | Source | Classification | Disposition |\n|--------|--------|----------------|-------------|\n| `x` | git | Trusted-but-external | `try_new` |\n";
    let v = scan(content, &dummy_path());
    assert!(
        v.is_empty(),
        "disposition alias must be accepted, got {:?}",
        v
    );
}

#[test]
fn scan_accepts_tbd_row_content() {
    // Design choice: the gate validates table presence, not row
    // content. A TBD table signals author irresponsibility to
    // reviewers — the gate forces the conversation to happen.
    let content = "tighten to panic on empty\n\n| Caller | Source | Classification | Handling |\n|--------|--------|----------------|----------|\n| TBD | TBD | TBD | TBD |\n";
    let v = scan(content, &dummy_path());
    assert!(
        v.is_empty(),
        "TBD-content table must be accepted by design, got {:?}",
        v
    );
}

// --- scan: violation surfaced ---

#[test]
fn scan_fails_tightening_without_table() {
    let content = "tighten FlowPaths::new to panic on empty branches.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn scan_fails_panic_on_without_table() {
    let content = "panic on empty strings.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn scan_fails_reject_empty_without_table() {
    let content = "reject empty inputs at the constructor boundary.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn scan_fails_direct_assert_macro_mention() {
    let content = "we add assert!(!branch.is_empty()) to the constructor body.\n";
    let v = scan(content, &dummy_path());
    assert!(
        !v.is_empty(),
        "direct assert! mention outside code must flag, got {:?}",
        v
    );
}

#[test]
fn scan_fails_incomplete_table_columns() {
    // Only 3 columns — not a valid audit table.
    let content = "tighten to panic on empty\n\n| Caller | Source | Handling |\n|--------|--------|----------|\n| `x` | git | `try_new` |\n";
    let v = scan(content, &dummy_path());
    assert_eq!(
        v.len(),
        1,
        "incomplete-column table must not satisfy the gate, got {:?}",
        v
    );
}

#[test]
fn scan_fails_header_only_no_data_row() {
    // Header + separator but no data row — not a real audit.
    let content = "tighten to panic on empty\n\n| Caller | Source | Classification | Handling |\n|--------|--------|----------------|----------|\n";
    let v = scan(content, &dummy_path());
    assert_eq!(
        v.len(),
        1,
        "header-only table must not satisfy the gate, got {:?}",
        v
    );
}

#[test]
fn scan_fails_header_row_missing_source_column() {
    // Four cells but Source is missing — fails the has_source check.
    let content = "tighten to panic on empty\n\n| Caller | Origin | Classification | Handling |\n|--------|--------|----------------|----------|\n| `x` | git | Trusted | `try_new` |\n";
    let v = scan(content, &dummy_path());
    assert_eq!(
        v.len(),
        1,
        "header without Source column must not satisfy gate, got {:?}",
        v
    );
}

#[test]
fn scan_fails_separator_row_without_dashes() {
    // Header row, then a row of only pipes and spaces (no dashes).
    // That is not a valid separator — the audit is incomplete.
    let content = "tighten to panic on empty\n\n| Caller | Source | Classification | Handling |\n| | | | |\n| `x` | git | Trusted | `try_new` |\n";
    let v = scan(content, &dummy_path());
    assert_eq!(
        v.len(),
        1,
        "pipes-only separator row must not satisfy gate, got {:?}",
        v
    );
}

#[test]
fn scan_fails_data_row_without_pipes() {
    // Header + separator, but the data row is a plain sentence.
    // Not a valid audit — the data row must contain at least one `|`.
    let content = "tighten to panic on empty\n\n| Caller | Source | Classification | Handling |\n|--------|--------|----------------|----------|\nno pipes in this data row\n";
    let v = scan(content, &dummy_path());
    assert_eq!(
        v.len(),
        1,
        "pipe-less data row must not satisfy gate, got {:?}",
        v
    );
}

// --- scan: fenced block skip ---

#[test]
fn scan_skips_trigger_inside_fenced_block() {
    let content = "## Heading\n\n```text\ntighten FlowPaths::new to panic on empty\n```\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty(), "fenced content must skip, got {:?}", v);
}

#[test]
fn scan_skips_direct_token_inside_fenced_block() {
    let content = "## Heading\n\n```rust\nassert!(x > 0);\n```\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty(), "fenced direct-token must skip, got {:?}", v);
}

#[test]
fn scan_unterminated_fence_fails_open() {
    // An unclosed fence must not silence every subsequent trigger.
    let content = "```\ncode here\n\ntighten FlowPaths::new to panic on empty.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(
        v.len(),
        1,
        "unterminated fence must not suppress downstream triggers, got {:?}",
        v
    );
}

#[test]
fn scan_trigger_after_unclosed_fence_detected() {
    // An unclosed fence must fail open — content after the fence
    // is NOT masked, so triggers are still detected.
    let content = "```\nfenced content\ntighten FlowPaths::new to panic on empty.\n";
    let v = scan(content, &dummy_path());
    assert!(
        !v.is_empty(),
        "trigger after unclosed fence must be detected (fail-open)"
    );
}

#[test]
fn scan_fenced_window_skip_inside_collect_window() {
    // A fenced block sits between a trigger and a would-be table so
    // the window-collector's fenced-skip path is exercised.
    let content = "tighten to panic on empty\n\n```rust\nfn mock() {}\n```\n\n| Caller | Source | Classification | Handling |\n|--------|--------|----------------|----------|\n| `x` | git | Trusted | `try_new` |\n";
    let v = scan(content, &dummy_path());
    assert!(
        v.is_empty(),
        "table past a fenced block must still satisfy gate, got {:?}",
        v
    );
}

// --- scan: opt-out ---

#[test]
fn scan_skips_opt_out_preceding_line() {
    let content = "<!-- external-input-audit: not-a-tightening -->\ntighten FlowPaths::new to panic on empty in the discussion context.\n";
    let v = scan(content, &dummy_path());
    assert!(
        v.is_empty(),
        "preceding-line opt-out must skip, got {:?}",
        v
    );
}

#[test]
fn scan_skips_opt_out_same_line() {
    let content = "<!-- external-input-audit: not-a-tightening --> tighten to panic on empty\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty(), "same-line opt-out must skip, got {:?}", v);
}

#[test]
fn scan_opt_out_allows_single_blank_line_gap() {
    let content = "<!-- external-input-audit: not-a-tightening -->\n\ntighten FlowPaths::new to panic on empty\n";
    let v = scan(content, &dummy_path());
    assert!(
        v.is_empty(),
        "single-blank-line gap must allow opt-out, got {:?}",
        v
    );
}

#[test]
fn scan_opt_out_rejects_multi_blank_gap() {
    let content =
        "<!-- external-input-audit: not-a-tightening -->\n\n\n\ntighten to panic on empty\n";
    let v = scan(content, &dummy_path());
    assert_eq!(
        v.len(),
        1,
        "multi-blank gap must not carry opt-out, got {:?}",
        v
    );
}

#[test]
fn scan_opt_out_tolerates_extra_internal_whitespace() {
    let content = "<!--  external-input-audit: not-a-tightening  -->\ntighten to panic on empty\n";
    let v = scan(content, &dummy_path());
    assert!(
        v.is_empty(),
        "opt-out with extra whitespace must still match, got {:?}",
        v
    );
}

#[test]
fn scan_opt_out_tolerates_missing_space_after_colon() {
    let content = "<!-- external-input-audit:not-a-tightening -->\ntighten to panic on empty\n";
    let v = scan(content, &dummy_path());
    assert!(
        v.is_empty(),
        "opt-out without space after colon must match, got {:?}",
        v
    );
}

#[test]
fn scan_opt_out_does_not_leak_to_later_lines() {
    let content = "<!-- external-input-audit: not-a-tightening -->\ntighten to panic on empty (covered by opt-out)\n\ntighten FlowPaths::new to panic on empty (not covered)\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1, "later trigger must still flag, got {:?}", v);
}

// --- scan: section boundary ---

#[test]
fn scan_does_not_cross_h2_forward() {
    let content = "## A\n\ntighten to panic on empty.\n\n## B\n\n| Caller | Source | Classification | Handling |\n|--------|--------|----------------|----------|\n| `x` | git | Trusted | `try_new` |\n";
    let v = scan(content, &dummy_path());
    assert_eq!(
        v.len(),
        1,
        "forward table across ## must not count, got {:?}",
        v
    );
}

#[test]
fn scan_does_not_cross_h2_backward() {
    let content = "## A\n\n| Caller | Source | Classification | Handling |\n|--------|--------|----------------|----------|\n| `x` | git | Trusted | `try_new` |\n\n## B\n\ntighten to panic on empty.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(
        v.len(),
        1,
        "backward table across ## must not count, got {:?}",
        v
    );
}

#[test]
fn scan_does_not_cross_h1_forward() {
    let content = "# Top\n\ntighten to panic on empty.\n\n# Next\n\n| Caller | Source | Classification | Handling |\n|--------|--------|----------------|----------|\n| `x` | git | Trusted | `try_new` |\n";
    let v = scan(content, &dummy_path());
    assert_eq!(
        v.len(),
        1,
        "h1 boundary must also bound the window, got {:?}",
        v
    );
}

#[test]
fn scan_crosses_h3_heading() {
    let content = "## Section\n\ntighten to panic on empty\n\n### Subsection\n\n| Caller | Source | Classification | Handling |\n|--------|--------|----------------|----------|\n| `x` | git | Trusted | `try_new` |\n";
    let v = scan(content, &dummy_path());
    assert!(
        v.is_empty(),
        "### subsection must not block the window, got {:?}",
        v
    );
}

// --- scan: negation skip ---

#[test]
fn scan_skips_do_not_tighten() {
    let content = "do not tighten FlowPaths::new to panic on slash-containing branches.\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty(), "negation must skip, got {:?}", v);
}

#[test]
fn scan_skips_never_add_panic() {
    let content = "never add a panic! to hook entry points.\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty(), "negation must skip, got {:?}", v);
}

#[test]
fn scan_does_not_skip_unrelated_not_earlier_in_line() {
    // A "not" in an unrelated earlier sentence must NOT suppress a
    // downstream trigger. The regression guarded here is a prior
    // naive `prefix.contains("not ")` that silently bypassed the
    // gate whenever an unrelated "not" appeared earlier.
    let content = "This is not the only rule. We will tighten to panic on empty input.\n";
    let v = scan(content, &dummy_path());
    assert!(
        !v.is_empty(),
        "unrelated 'not' earlier in the line must not suppress the trigger, got {:?}",
        v
    );
}

#[test]
fn scan_does_not_skip_unrelated_without_earlier_in_line() {
    // "without" is not in the negation set because it is not a
    // clean grammatical negator. Opt-out comments are the correct
    // mechanism for discussion prose.
    let content = "We proceed without intermediate checks; we then tighten to panic on empty.\n";
    let v = scan(content, &dummy_path());
    assert!(
        !v.is_empty(),
        "unrelated 'without' earlier in the line must not suppress the trigger, got {:?}",
        v
    );
}

// --- header-row edge cases ---

#[test]
fn scan_accepts_header_with_leading_empty_cells() {
    // Leading pipe + extra pipe creates empty cells that the filter
    // must discard while still matching the four required columns.
    let content = "tighten to panic on empty\n\n| | Caller | Source | Classification | Handling |\n|---|--------|--------|----------------|----------|\n| | `x` | git | Trusted | `try_new` |\n";
    let v = scan(content, &dummy_path());
    assert!(
        v.is_empty(),
        "leading empty cells must still resolve header, got {:?}",
        v
    );
}

// --- window exhaustion ---

#[test]
fn scan_fails_when_forward_window_exhausts_before_table() {
    // Fill the forward window with 15 non-blank non-table lines —
    // more than WINDOW_NON_BLANK_LINES (8) — so the forward scan
    // terminates before reaching the valid table. Also push the
    // table past WINDOW_LINES (30) so collect_window excludes it
    // from the window entirely. The section boundary above the
    // trigger prevents the backward scan from reaching any table
    // above.
    let mut content = String::from("## Section\n\ntighten to panic on empty.\n");
    for i in 0..32 {
        content.push_str(&format!("filler line {}\n", i));
    }
    content.push_str("| Caller | Source | Classification | Handling |\n");
    content.push_str("|--------|--------|----------------|----------|\n");
    content.push_str("| a | b | c | d |\n");
    let v = scan(&content, &dummy_path());
    assert_eq!(
        v.len(),
        1,
        "table past window must not satisfy gate, got {:?}",
        v
    );
}

#[test]
fn scan_fails_when_backward_window_exhausts_before_table() {
    // Table at top, then 32 filler lines (past WINDOW_LINES = 30),
    // then trigger. No section boundary anywhere so the window walks
    // freely. The window contains 9+ non-blank lines above the
    // trigger, exercising the WINDOW_NON_BLANK_LINES break in the
    // backward scan (line 454 of `external_input_audit.rs`).
    let mut content = String::new();
    content.push_str("| Caller | Source | Classification | Handling |\n");
    content.push_str("|--------|--------|----------------|----------|\n");
    content.push_str("| a | b | c | d |\n");
    for i in 0..32 {
        content.push_str(&format!("filler line {}\n", i));
    }
    content.push_str("tighten to panic on empty.\n");
    let v = scan(&content, &dummy_path());
    assert_eq!(
        v.len(),
        1,
        "table above distant trigger must not satisfy gate, got {:?}",
        v
    );
}

#[test]
fn scan_passes_blank_line_between_separator_and_data() {
    // `find_separator_then_data` calls `next_non_blank` twice — once
    // to locate the separator, once to locate the data row. When a
    // blank line sits between the separator and the data row, the
    // second call must skip the blank to find the data row. This
    // exercises `next_non_blank`'s `i += 1` blank-skip path.
    let content = "tighten to panic on empty\n\n| Caller | Source | Classification | Handling |\n|--------|--------|----------------|----------|\n\n| `x` | git | Trusted | `try_new` |\n";
    let v = scan(content, &dummy_path());
    assert!(
        v.is_empty(),
        "blank line between separator and data must still resolve table, got {:?}",
        v
    );
}

#[test]
fn scan_passes_separator_with_internal_whitespace() {
    // A separator row like `| --- | --- | --- | --- |` has spaces
    // between pipes and dashes. `is_separator_row`'s char-scan must
    // accept whitespace (the `c.is_whitespace()` alternative). This
    // test exercises the whitespace branch of the separator's
    // allowed-char closure.
    let content = "tighten to panic on empty\n\n| Caller | Source | Classification | Handling |\n| --- | --- | --- | --- |\n| `x` | git | Trusted | `try_new` |\n";
    let v = scan(content, &dummy_path());
    assert!(
        v.is_empty(),
        "whitespace-padded separator must be accepted, got {:?}",
        v
    );
}

#[test]
fn scan_fails_header_at_end_of_window_no_separator() {
    // The header row is the LAST non-blank line in the window —
    // no separator below it. `find_separator_then_data`'s first
    // `next_non_blank(header_idx + 1)?` must return None and the
    // function returns None early. This is distinct from the
    // header-plus-separator-but-no-data case (exercised by
    // `scan_fails_header_only_no_data_row`), which hits the second
    // `next_non_blank` call.
    let content = "tighten to panic on empty\n\n| Caller | Source | Classification | Handling |\n";
    let v = scan(content, &dummy_path());
    assert_eq!(
        v.len(),
        1,
        "header at window end must not satisfy gate, got {:?}",
        v
    );
}

#[test]
fn scan_fails_malformed_separator_with_non_allowed_char() {
    // A separator row like `|---X---|---X---|---X---|---X---|`
    // contains `-` (so passes the contains-dash early check) but
    // has an `X` character that is not pipe, dash, colon, or
    // whitespace. `is_separator_row`'s char-scan must return false
    // in this case, so the gate sees no valid table and flags a
    // violation. This test exercises the all()-returns-false path
    // of the closure.
    let content = "tighten to panic on empty\n\n| Caller | Source | Classification | Handling |\n|---X---|---X---|---X---|---X---|\n| `x` | git | Trusted | `try_new` |\n";
    let v = scan(content, &dummy_path());
    assert_eq!(
        v.len(),
        1,
        "malformed separator with non-allowed char must not satisfy gate, got {:?}",
        v
    );
}

#[test]
fn scan_passes_when_trigger_at_document_start_finds_forward_table() {
    // Trigger at line 0 exercises the `trigger_rel_idx == 0`
    // short-circuit in the backward scan (no walk) while the
    // forward scan finds the table.
    let content = "tighten to panic on empty\n\n| Caller | Source | Classification | Handling |\n|--------|--------|----------------|----------|\n| `x` | git | Trusted | `try_new` |\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty(), "{:?}", v);
}

#[test]
fn scan_fails_when_trigger_at_document_start_has_no_forward_table() {
    // Trigger at line 0, no table anywhere — backward scan returns
    // immediately and forward scan finds nothing.
    let content = "tighten to panic on empty.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

// --- integration: Violation fields ---

#[test]
fn scan_populates_violation_fields() {
    let content = "line one\ntighten FlowPaths::new to panic on empty.\nline three\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].line, 2);
    // The direct-action pattern `\bpanic\s+on\s+\w+` wins on
    // the example line because "FlowPaths::new" is not in the
    // verb+noun alternation's allowed noun set. Either trigger
    // is sufficient for the gate's purpose — flag the line.
    assert!(v[0].phrase.to_lowercase().contains("panic on"));
    assert!(v[0].context.contains("tighten FlowPaths::new"));
    assert_eq!(v[0].file, PathBuf::from("dummy.md"));
}
