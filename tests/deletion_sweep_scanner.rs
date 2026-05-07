//! Tests for `src/deletion_sweep_scanner.rs`.
//!
//! Plan-phase Gate 2: when a plan proposes deleting/removing/
//! renaming a backtick-quoted identifier (≥ 10 chars), the plan
//! must include sweep evidence (file bullets or an Exploration
//! heading) within `WINDOW_NON_BLANK_LINES` forward.

use std::path::PathBuf;

use flow_rs::deletion_sweep_scanner::{scan, Violation, TRIGGER_PATTERN};

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

#[test]
fn trigger_with_sweep_bullets_passes() {
    let content = "Remove `old_legacy_function`.\n\n\
        - `src/foo.rs`\n\
        - `tests/foo.rs`\n";
    assert_clean(content);
}

#[test]
fn trigger_with_exploration_heading_passes() {
    let content = "Remove `old_legacy_function`.\n\n\
        ## Exploration\n\n\
        Files affected: see below.\n";
    assert_clean(content);
}

#[test]
fn trigger_without_sweep_fires() {
    let content = "Remove `old_legacy_function`. No sweep follows.\n";
    let v = assert_violation(content);
    assert_eq!(v.line, 1);
    assert_eq!(v.identifier, "old_legacy_function");
    assert!(v.phrase.to_lowercase().contains("remov"));
}

#[test]
fn delete_verb_triggers() {
    let content = "Delete `obsolete_handler_v1`. Nothing else.\n";
    let v = assert_violation(content);
    assert_eq!(v.identifier, "obsolete_handler_v1");
}

#[test]
fn rename_verb_triggers() {
    let content = "Rename `old_long_helper_name` to something shorter.\n";
    let v = assert_violation(content);
    assert_eq!(v.identifier, "old_long_helper_name");
}

#[test]
fn replace_verb_triggers() {
    let content = "Replace `legacy_validator_fn` with the new helper.\n";
    let v = assert_violation(content);
    assert_eq!(v.identifier, "legacy_validator_fn");
}

#[test]
fn verb_variants_trigger() {
    let cases = [
        "Remove `old_long_function_name`. End.",
        "Removes `old_long_function_name`. End.",
        "Removing `old_long_function_name`. End.",
        "Delete `old_long_function_name`. End.",
        "Deletes `old_long_function_name`. End.",
        "Deleting `old_long_function_name`. End.",
        "Rename `old_long_function_name` to bar. End.",
        "Renames `old_long_function_name` to bar. End.",
        "Renaming `old_long_function_name` to bar. End.",
        "Replace `old_long_function_name` with bar. End.",
        "Replaces `old_long_function_name` with bar. End.",
        "Replacing `old_long_function_name` with bar. End.",
    ];
    for c in cases.iter() {
        let v = scan(c, &fixture_path());
        assert_eq!(v.len(), 1, "expected violation for: {}", c);
    }
}

#[test]
fn short_identifier_below_length_filter_ignored() {
    // `foo_bar` is < 10 chars so it does not qualify as an
    // identifier; the trigger has no candidate to attach to.
    let content = "Remove `foo_bar`. No sweep.\n";
    assert_clean(content);
}

#[test]
fn fenced_block_trigger_ignored() {
    let content = "```\nRemove `old_long_function_name`. No sweep.\n```\n";
    assert_clean(content);
}

#[test]
fn opt_out_on_trigger_line_suppresses() {
    let content = "Remove `old_long_function_name`. <!-- deletion-sweep: not-a-deletion --> end.\n";
    assert_clean(content);
}

#[test]
fn opt_out_directly_above_suppresses() {
    let content = "<!-- deletion-sweep: not-a-deletion -->\nRemove `old_long_function_name`.\n";
    assert_clean(content);
}

#[test]
fn opt_out_two_lines_above_with_blank_suppresses() {
    let content = "<!-- deletion-sweep: not-a-deletion -->\n\nRemove `old_long_function_name`.\n";
    assert_clean(content);
}

#[test]
fn opt_out_three_lines_above_does_not_suppress() {
    let content = "<!-- deletion-sweep: not-a-deletion -->\n\n\nRemove `old_long_function_name`.\n";
    assert_eq!(scan(content, &fixture_path()).len(), 1);
}

#[test]
fn negated_trigger_ignored() {
    // Capital "Remove" triggers (case-sensitive verb pattern);
    // "not" earlier in the same sentence suppresses via the
    // negation-prefix check.
    let content = "Do not Remove `old_long_function_name` in this PR.\n";
    assert_clean(content);
}

#[test]
fn lowercase_verb_does_not_trigger() {
    // "removes" / "deletes" / "renaming" mid-sentence describe
    // existing behavior; the case-sensitive pattern intentionally
    // skips them.
    let cases = [
        "The script removes `old_long_function_name` at runtime.",
        "Each step deletes `old_long_function_name` from the queue.",
        "The handler is renaming `old_long_function_name` internally.",
    ];
    for c in cases.iter() {
        let v = scan(c, &fixture_path());
        assert!(v.is_empty(), "expected no violation for: {} got {:?}", c, v);
    }
}

#[test]
fn duplicate_identifier_only_reported_once() {
    // Two trigger lines naming the SAME identifier produce one
    // violation (dedup on identifier).
    let content = "Remove `old_long_function_name`.\nDelete `old_long_function_name`.\n";
    let v = scan(content, &fixture_path());
    assert_eq!(v.len(), 1);
}

#[test]
fn cap_limits_violations() {
    // Generate 25 trigger+identifier pairs without sweep evidence.
    // The cap pins violations at MAX_IDENTIFIERS_PER_PLAN = 20.
    let mut content = String::new();
    for i in 0..25 {
        content.push_str(&format!("Remove `old_long_function_name_{}`.\n", i));
    }
    let v = scan(&content, &fixture_path());
    assert!(v.len() <= 20, "got {} violations", v.len());
}

#[test]
fn empty_content_clean() {
    assert_clean("");
}

#[test]
fn no_trigger_clean() {
    assert_clean("## Plan\n\nThis adds a new feature with no removals.\n");
}

#[test]
fn trigger_without_identifier_clean() {
    // "Remove the duplicate" — no backtick-quoted identifier.
    let content = "Remove the duplicate from the corpus.\n";
    assert_clean(content);
}

#[test]
fn inline_table_evidence_passes() {
    let content = "Remove `old_long_function_name`.\n\n\
        | File | Reason |\n|---|---|\n| `src/old.rs` | source |\n";
    assert_clean(content);
}

#[test]
fn trigger_pattern_constant_exposed() {
    assert!(!TRIGGER_PATTERN.is_empty());
}

#[test]
fn negation_in_earlier_sentence_does_not_suppress() {
    // A "not" in an earlier sentence on the same line must not
    // suppress a trigger in a later sentence.
    let content = "This is not refactor work. Remove `old_long_function_name` here.\n";
    let v = scan(content, &fixture_path());
    assert_eq!(v.len(), 1);
}

#[test]
fn unclosed_fence_at_eof_fails_open() {
    // An unclosed fence reverts the mask so triggers below the
    // stray opener remain visible.
    let content = "```\nRemove `old_long_function_name`. No sweep.\n";
    let v = scan(content, &fixture_path());
    assert_eq!(v.len(), 1, "got: {:?}", v);
}

#[test]
fn sweep_evidence_skips_fenced_block_lines() {
    // A fenced block between trigger and a real bullet list does
    // not count its own lines against the window. The bullets
    // outside the fence should still satisfy compliance.
    let content = "Remove `old_long_function_name`.\n\n\
        ```\nfn foo() {}\n```\n\n\
        - `src/foo.rs`\n- `tests/foo.rs`\n";
    assert_clean(content);
}
