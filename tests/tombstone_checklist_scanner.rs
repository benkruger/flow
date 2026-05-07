//! Tests for `src/tombstone_checklist_scanner.rs`.
//!
//! Plan-phase Gate 3: when a plan proposes adding a tombstone
//! test, the plan must include the five-item checklist
//! (protection target, assertion kind, stability argument,
//! bypass list, file-resurrection pair) within
//! `WINDOW_NON_BLANK_LINES` forward of the trigger.

use std::path::PathBuf;

use flow_rs::tombstone_checklist_scanner::{scan, Violation, TRIGGER_PATTERN};

fn fixture_path() -> PathBuf {
    PathBuf::from(".flow-states/test/plan.md")
}

fn assert_clean(content: &str) {
    let v = scan(content, &fixture_path());
    assert!(v.is_empty(), "expected no violations, got: {:?}", v);
}

fn assert_violation(content: &str) -> Violation {
    let v = scan(content, &fixture_path());
    assert_eq!(v.len(), 1, "expected exactly one violation, got: {:?}", v);
    v.into_iter().next().unwrap()
}

fn full_checklist() -> &'static str {
    "Protection target: the deleted function.\n\
     Assertion kind: literal byte-substring.\n\
     Stability argument: cannot be assembled by concat!.\n\
     Bypass list: format!, slice join, split constants.\n\
     File-resurrection pair: paired file-existence check.\n"
}

#[test]
fn trigger_with_full_checklist_passes() {
    let content = format!(
        "## Tasks\n\nAdd a tombstone test for `removed_fn`.\n\n{}\n",
        full_checklist()
    );
    assert_clean(&content);
}

#[test]
fn trigger_without_checklist_fires() {
    let content = "## Tasks\n\nAdd a tombstone test. No checklist.\n";
    let v = assert_violation(content);
    assert_eq!(v.line, 3);
    assert_eq!(v.missing_items.len(), 5);
}

#[test]
fn partial_checklist_reports_missing() {
    let content = "Add a tombstone test.\n\n\
        Protection target: foo.\n\
        Assertion kind: literal.\n";
    let v = assert_violation(content);
    assert!(!v.missing_items.contains(&"protection_target".to_string()));
    assert!(!v.missing_items.contains(&"assertion_kind".to_string()));
    assert!(v.missing_items.contains(&"stability".to_string()));
    assert!(v.missing_items.contains(&"bypass_list".to_string()));
    assert!(v.missing_items.contains(&"file_resurrection".to_string()));
}

#[test]
fn discussion_without_propose_verb_clean() {
    // "tombstones live in tests/" — tombstone mentioned without
    // a propose-verb (add/ship/introduce/include) and without
    // the "tombstone test" / "tombstone for" noun phrase.
    let content = "Tombstones live in tests/. Pre-existing infrastructure.\n";
    assert_clean(content);
}

#[test]
fn bare_tombstone_word_without_noun_phrase_clean() {
    // "tombstone-audit" subcommand reference + "add-finding"
    // verb on same line — no "tombstone test" or "tombstone for"
    // noun phrase, so trigger does not fire.
    let content = "Read-only subcommands (`tombstone-audit`) and `add-finding` mutator.\n";
    assert_clean(content);
}

#[test]
fn tombstone_test_phrase_without_propose_verb_clean() {
    // The noun phrase "tombstone test" appears, but no
    // propose-verb (add/ship/introduce/include) is on the line.
    // Plan prose discussing tombstones generally rather than
    // proposing one.
    let content = "The tombstone test asserts the removed identifier is absent.\n";
    assert_clean(content);
}

#[test]
fn fenced_block_trigger_ignored() {
    let content = "```\nAdd a tombstone test for X.\n```\n";
    assert_clean(content);
}

#[test]
fn opt_out_on_trigger_line_suppresses() {
    let content = "Add a tombstone test. <!-- tombstone-checklist: not-a-tombstone --> end.\n";
    assert_clean(content);
}

#[test]
fn opt_out_directly_above_suppresses() {
    let content = "<!-- tombstone-checklist: not-a-tombstone -->\nAdd a tombstone test.\n";
    assert_clean(content);
}

#[test]
fn opt_out_two_lines_above_with_blank_suppresses() {
    let content = "<!-- tombstone-checklist: not-a-tombstone -->\n\nAdd a tombstone test.\n";
    assert_clean(content);
}

#[test]
fn opt_out_three_lines_above_does_not_suppress() {
    let content = "<!-- tombstone-checklist: not-a-tombstone -->\n\n\nAdd a tombstone test.\n";
    assert_eq!(scan(content, &fixture_path()).len(), 1);
}

#[test]
fn negated_trigger_ignored() {
    let content = "Do not add a tombstone here.\n";
    assert_clean(content);
}

#[test]
fn negation_in_earlier_sentence_does_not_suppress() {
    let content = "This is not refactor work. Add a tombstone test for `foo_bar`.\n";
    assert_eq!(scan(content, &fixture_path()).len(), 1);
}

#[test]
fn ship_verb_triggers() {
    let content = "Ship a tombstone test. No checklist.\n";
    let v = assert_violation(content);
    assert!(v.phrase.to_lowercase().contains("ship"));
}

#[test]
fn introduce_verb_triggers() {
    let content = "Introduce a tombstone for the removal.\n";
    assert_violation(content);
}

#[test]
fn include_verb_triggers() {
    let content = "Include a tombstone test in this PR.\n";
    assert_violation(content);
}

#[test]
fn negated_trigger_with_noun_phrase_ignored() {
    // Capital trigger with noun phrase but negated: not a proposal.
    let content = "Do not add a tombstone test here.\n";
    assert_clean(content);
}

#[test]
fn empty_content_clean() {
    assert_clean("");
}

#[test]
fn no_tombstone_word_clean() {
    assert_clean("## Tasks\n\nAdd a regression test for `foo`.\n");
}

#[test]
fn unclosed_fence_at_eof_fails_open() {
    let content = "```\nAdd a tombstone test.\n";
    assert_eq!(scan(content, &fixture_path()).len(), 1);
}

#[test]
fn trigger_pattern_constant_exposed() {
    assert!(!TRIGGER_PATTERN.is_empty());
}

#[test]
fn case_insensitive_tombstone_word() {
    let content = "Add a TOMBSTONE test.\n";
    let v = assert_violation(content);
    assert_eq!(v.missing_items.len(), 5);
}

#[test]
fn checklist_lines_inside_fenced_block_do_not_count() {
    // A fenced block between the trigger and the real checklist
    // is skipped; checklist lines inside the fence don't count.
    let content = "Add a tombstone test.\n\n\
        ```\nProtection target: foo. Assertion kind: literal. Stability: ok. Bypass: none. File-resurrection: paired.\n```\n\n\
        End.\n";
    let v = assert_violation(content);
    assert_eq!(v.missing_items.len(), 5);
}

#[test]
fn checklist_outside_window_does_not_count() {
    let mut content = String::from("Add a tombstone test.\n");
    for i in 0..20 {
        content.push_str(&format!("Distractor {}.\n", i));
    }
    content.push_str(full_checklist());
    let v = assert_violation(&content);
    assert_eq!(v.missing_items.len(), 5);
}
