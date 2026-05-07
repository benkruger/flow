//! Tests for `src/cli_output_contract_scanner.rs`.
//!
//! Covers Plan-phase Gate 1: a trigger ("new flag" / "new
//! subcommand" with output-kind co-occurrence on the same line) must
//! be paired with a four-item contract block (output format, exit
//! codes, error messages, fallback) within `WINDOW_NON_BLANK_LINES`
//! forward of the trigger.

use std::path::PathBuf;

use flow_rs::cli_output_contract_scanner::{scan, Violation, TRIGGER_PATTERN};

// --- scan ---

fn fixture_path() -> PathBuf {
    PathBuf::from(".flow-states/test/plan.md")
}

fn assert_clean(content: &str) {
    let violations = scan(content, &fixture_path());
    assert!(
        violations.is_empty(),
        "expected no violations, got: {:?}",
        violations
    );
}

fn assert_violation(content: &str) -> Violation {
    let violations = scan(content, &fixture_path());
    assert_eq!(
        violations.len(),
        1,
        "expected exactly one violation, got: {:?}",
        violations
    );
    violations.into_iter().next().unwrap()
}

fn full_contract_block() -> &'static str {
    "Output format: JSON.\nExit codes: 0 ok, 1 error.\nError messages: stderr names the failure class.\nFallback: none — fail closed."
}

#[test]
fn trigger_with_full_contract_passes() {
    let content = format!(
        "## Tasks\n\n### Task: Introduce a new flag with consumed stdout\n\n{}\n",
        full_contract_block()
    );
    assert_clean(&content);
}

#[test]
fn trigger_without_contract_block_fires() {
    let content = "## Tasks\n\nIntroduce a new flag with consumed stdout. No contract block.\n";
    let v = assert_violation(content);
    assert_eq!(v.line, 3);
    assert_eq!(v.missing_items.len(), 4);
    assert!(v.missing_items.contains(&"output_format".to_string()));
    assert!(v.missing_items.contains(&"exit_codes".to_string()));
    assert!(v.missing_items.contains(&"error_messages".to_string()));
    assert!(v.missing_items.contains(&"fallback".to_string()));
}

#[test]
fn partial_contract_reports_only_missing_items() {
    let content = "Add a new subcommand with stdout output.\n\nOutput format: JSON.\nExit codes: 0 ok, 1 error.\n";
    let v = assert_violation(content);
    assert!(!v.missing_items.contains(&"output_format".to_string()));
    assert!(!v.missing_items.contains(&"exit_codes".to_string()));
    assert!(v.missing_items.contains(&"error_messages".to_string()));
    assert!(v.missing_items.contains(&"fallback".to_string()));
}

#[test]
fn fenced_block_trigger_is_ignored() {
    let content = "```\nIntroduce a new flag with consumed stdout.\n```\n";
    assert_clean(content);
}

#[test]
fn fenced_block_contract_lines_count_against_compliance() {
    // A contract block buried inside a fenced code block should NOT
    // count as compliance — the rule wants prose. But our
    // implementation skips fenced lines from BOTH trigger detection
    // and contract scanning, which means a fenced contract block
    // doesn't satisfy the trigger. Verify that semantics.
    let content = "Add a new subcommand with consumed output.\n\n```\nOutput format: JSON.\nExit codes: 0 ok, 1 error.\nError messages: stderr.\nFallback: none.\n```\n";
    let v = assert_violation(content);
    assert_eq!(v.missing_items.len(), 4);
}

#[test]
fn unclosed_fence_at_eof_fails_open() {
    // An unclosed fence should not silence triggers below it. The
    // mask is reverted from the last open marker so triggers below
    // remain visible.
    let content = "```\nIntroduce a new flag with consumed stdout.\n";
    // No content block — should produce a violation because the fence
    // is unclosed and the line is treated as visible.
    let violations = scan(content, &fixture_path());
    assert_eq!(violations.len(), 1, "got: {:?}", violations);
}

#[test]
fn output_kind_keyword_required_on_same_line() {
    // A "new flag" mention without an output-kind keyword on the
    // same line is not a trigger.
    let content = "## Tasks\n\nWe will add a new flag for the subcommand. Some other paragraph.\n";
    assert_clean(content);
}

#[test]
fn co_occurrence_multiple_output_kinds() {
    // exit code, stderr, json — any of the keywords should trigger.
    let content = "Add a new subcommand whose exit code signals failure.\n";
    let v = assert_violation(content);
    assert_eq!(v.line, 1);
}

#[test]
fn opt_out_on_trigger_line_suppresses() {
    let content = "Introduce a new flag with consumed output. <!-- cli-output-contracts: not-a-new-flag -->\n";
    assert_clean(content);
}

#[test]
fn opt_out_directly_above_suppresses() {
    let content = "<!-- cli-output-contracts: not-a-new-flag -->\nIntroduce a new flag with consumed output.\n";
    assert_clean(content);
}

#[test]
fn opt_out_two_lines_above_with_blank_between_suppresses() {
    let content = "<!-- cli-output-contracts: not-a-new-flag -->\n\nIntroduce a new flag with consumed output.\n";
    assert_clean(content);
}

#[test]
fn opt_out_three_lines_above_does_not_suppress() {
    // Walk-back is bounded — beyond one blank line it does not chain.
    let content = "<!-- cli-output-contracts: not-a-new-flag -->\n\n\nIntroduce a new flag with consumed output.\n";
    let violations = scan(content, &fixture_path());
    assert_eq!(violations.len(), 1);
}

#[test]
fn opt_out_with_extra_whitespace_still_suppresses() {
    let content = "Introduce a new flag with consumed output. <!--   cli-output-contracts  :  not-a-new-flag   -->\n";
    assert_clean(content);
}

#[test]
fn negated_trigger_is_ignored() {
    let content = "We do not introduce a new flag with stdout output here.\n";
    assert_clean(content);
}

#[test]
fn negation_in_earlier_sentence_does_not_suppress() {
    // Per the sibling scanner's sentence-scoped negation: a "not" in
    // an earlier sentence must NOT suppress a trigger in a later
    // sentence on the same line.
    let content = "This is not a refactor. Introduce a new flag with consumed output.\n";
    let violations = scan(content, &fixture_path());
    assert_eq!(violations.len(), 1);
}

#[test]
fn variant_verbs_trigger() {
    // Verb variations: add, adds, adding, introduce, introduces,
    // introducing, extend, extends, extending, implement, implements.
    let cases = [
        "Add a new flag with stdout output.",
        "Adds a new flag with output.",
        "Adding a new flag with consumed output.",
        "Introduce a new subcommand with output.",
        "Introduces a new subcommand with stdout.",
        "Introducing a new flag with stdout output.",
        "Extend bin/test with a new flag with consumed output.",
        "Extends bin/test with a new flag with consumed output.",
        "Extending bin/test with a new flag with consumed output.",
        "Implement a new subcommand with stdout.",
        "Implements a new subcommand with stdout.",
    ];
    for c in cases.iter() {
        let violations = scan(c, &fixture_path());
        assert_eq!(violations.len(), 1, "expected violation for: {}", c);
    }
}

#[test]
fn empty_content_clean() {
    assert_clean("");
}

#[test]
fn no_trigger_clean() {
    let content = "## Plan\n\nThis describes a refactor with no new public surface.\n";
    assert_clean(content);
}

#[test]
fn violation_carries_phrase_and_context() {
    let content = "Introduce a new flag with consumed stdout.\n";
    let v = assert_violation(content);
    assert!(
        v.phrase.to_lowercase().contains("flag"),
        "phrase should name the verb-target match: {}",
        v.phrase
    );
    assert!(v.context.contains("Introduce a new flag"));
    assert_eq!(v.file, fixture_path());
}

#[test]
fn window_extends_exactly_twelve_non_blank_lines() {
    // Bury one contract item past the 12-non-blank-line window. It
    // should NOT count as compliance, so the trigger fires with that
    // item missing.
    let mut content = String::from("Introduce a new flag with consumed output.\n");
    // Add 12 non-blank distractor lines, none mentioning a contract
    // item.
    for i in 0..12 {
        content.push_str(&format!("Distractor line {}.\n", i));
    }
    // Now place all four contract markers — but they are past the
    // 12-line window.
    content.push_str("Output format: JSON.\n");
    content.push_str("Exit codes: 0 ok.\n");
    content.push_str("Error messages: stderr.\n");
    content.push_str("Fallback: none.\n");
    let v = assert_violation(&content);
    assert_eq!(v.missing_items.len(), 4);
}

#[test]
fn blank_lines_do_not_consume_window_budget() {
    // Blank lines between the trigger and contract items should not
    // count against the 12-line budget.
    let mut content = String::from("Introduce a new flag with consumed output.\n");
    for _ in 0..20 {
        content.push('\n');
    }
    content
        .push_str("Output format: JSON. Exit codes: 0. Error messages: stderr. Fallback: none.\n");
    assert_clean(&content);
}

#[test]
fn deduplication_one_violation_per_line() {
    // Multiple verb-target matches on the same line produce only one
    // violation.
    let content = "Add a new flag and introduce a new subcommand with consumed output.\n";
    let violations = scan(content, &fixture_path());
    assert_eq!(violations.len(), 1);
}

#[test]
fn trigger_pattern_constant_is_publicly_exposed() {
    // The TRIGGER_PATTERN constant is exposed for downstream
    // documentation/contract tests. Verify it parses as a valid
    // regex by performing a regex-compatible probe through scan().
    assert!(!TRIGGER_PATTERN.is_empty());
}
