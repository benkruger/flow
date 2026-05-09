//! Tests for the `plan_from_issue` subcommand — the sentinel-based plan
//! extractor that replaces the heuristic `plan-extract` path.
//!
//! The contract: scan an issue body for the literal sentinel pair
//! `<!-- FLOW-PLAN-BEGIN -->` and `<!-- FLOW-PLAN-END -->`, and return
//! the bytes between verbatim. No heading promotion, no truncation
//! detection, no scanner gates — the issue is the plan, the markers
//! delimit it, end of contract.

use flow_rs::plan_from_issue::{extract_plan, ExtractError, PLAN_BODY_BYTE_CAP};

const BEGIN: &str = "<!-- FLOW-PLAN-BEGIN -->";
const END: &str = "<!-- FLOW-PLAN-END -->";

// --- extract_plan ---

#[test]
fn extract_plan_happy_path_returns_content_between_markers() {
    let body = format!(
        "Issue prelude.\n\n{}\n## Plan\n\nContent here.\n{}\n\nIssue postlude.",
        BEGIN, END
    );
    let result = extract_plan(&body).expect("extraction succeeds");
    assert!(result.contains("## Plan"));
    assert!(result.contains("Content here."));
    assert!(!result.contains("Issue prelude"));
    assert!(!result.contains("Issue postlude"));
}

#[test]
fn extract_plan_rejects_when_both_markers_missing() {
    let body = "Some issue body with no sentinels at all.";
    let err = extract_plan(body).expect_err("extraction must fail");
    assert!(matches!(err, ExtractError::MarkersMissing));
}

#[test]
fn extract_plan_rejects_when_only_begin_present() {
    let body = format!("Prelude.\n{}\n## Plan\nContent.\n", BEGIN);
    let err = extract_plan(&body).expect_err("extraction must fail");
    assert!(matches!(err, ExtractError::MarkersMalformed));
}

#[test]
fn extract_plan_rejects_when_only_end_present() {
    let body = format!("Prelude.\n## Plan\nContent.\n{}\n", END);
    let err = extract_plan(&body).expect_err("extraction must fail");
    assert!(matches!(err, ExtractError::MarkersMalformed));
}

#[test]
fn extract_plan_uses_first_begin_when_multiple_begins_present() {
    let body = format!(
        "{}\nFirst plan.\n{}\nMiddle.\n{}\nSecond plan.\n{}",
        BEGIN, END, BEGIN, END
    );
    let result = extract_plan(&body).expect("extraction succeeds");
    assert!(result.contains("First plan."));
    assert!(!result.contains("Second plan."));
}

#[test]
fn extract_plan_uses_first_end_after_begin_when_multiple_ends_present() {
    let body = format!("{}\nReal plan content.\n{}\nNoise.\n{}", BEGIN, END, END);
    let result = extract_plan(&body).expect("extraction succeeds");
    assert!(result.contains("Real plan content."));
    assert!(!result.contains("Noise."));
}

#[test]
fn extract_plan_rejects_empty_content_between_markers() {
    let body = format!("{}{}", BEGIN, END);
    let err = extract_plan(&body).expect_err("extraction must fail");
    assert!(matches!(err, ExtractError::Empty));
}

#[test]
fn extract_plan_rejects_whitespace_only_content_between_markers() {
    let body = format!("{}\n   \n\t\n{}", BEGIN, END);
    let err = extract_plan(&body).expect_err("extraction must fail");
    assert!(matches!(err, ExtractError::Empty));
}

#[test]
fn extract_plan_rejects_when_end_appears_before_begin() {
    let body = format!("{}\nbackwards\n{}", END, BEGIN);
    let err = extract_plan(&body).expect_err("extraction must fail");
    assert!(matches!(err, ExtractError::MarkersMalformed));
}

#[test]
fn extract_plan_handles_crlf_line_endings() {
    let body = format!(
        "Prelude.\r\n{}\r\n## Plan\r\n\r\nContent.\r\n{}\r\nPostlude.",
        BEGIN, END
    );
    let result = extract_plan(&body).expect("extraction succeeds");
    assert!(result.contains("## Plan"));
    assert!(result.contains("Content."));
}

#[test]
fn extract_plan_rejects_body_larger_than_byte_cap() {
    let mut body = String::with_capacity(PLAN_BODY_BYTE_CAP + 1024);
    body.push_str(BEGIN);
    body.push('\n');
    while body.len() < PLAN_BODY_BYTE_CAP + 100 {
        body.push_str("padding line that consumes bytes\n");
    }
    body.push_str(END);
    let err = extract_plan(&body).expect_err("extraction must fail");
    assert!(matches!(err, ExtractError::TooLarge));
}

#[test]
fn extract_plan_accepts_body_at_byte_cap_boundary() {
    let mut body = String::new();
    body.push_str(BEGIN);
    body.push('\n');
    body.push_str("plan content\n");
    body.push_str(END);
    assert!(body.len() <= PLAN_BODY_BYTE_CAP);
    let result = extract_plan(&body).expect("under-cap body extracts cleanly");
    assert!(result.contains("plan content"));
}

#[test]
fn extract_plan_byte_cap_constant_is_one_megabyte() {
    assert_eq!(PLAN_BODY_BYTE_CAP, 1_048_576);
}

// --- ExtractError Display ---

#[test]
fn extract_error_display_markers_missing() {
    let msg = format!("{}", ExtractError::MarkersMissing);
    assert!(msg.contains("FLOW-PLAN-BEGIN"));
    assert!(msg.contains("FLOW-PLAN-END"));
}

#[test]
fn extract_error_display_markers_malformed() {
    let msg = format!("{}", ExtractError::MarkersMalformed);
    assert!(msg.contains("FLOW-PLAN"));
    assert!(msg.contains("unmatched") || msg.contains("out-of-order"));
}

#[test]
fn extract_error_display_empty() {
    let msg = format!("{}", ExtractError::Empty);
    assert!(msg.contains("empty"));
    assert!(msg.contains("FLOW-PLAN"));
}

#[test]
fn extract_error_display_too_large() {
    let msg = format!("{}", ExtractError::TooLarge);
    assert!(msg.contains("MiB") || msg.contains("cap"));
}
