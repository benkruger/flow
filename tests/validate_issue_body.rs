//! Tests for the `validate_issue_body` subcommand — the pre-filing
//! validator the role-based planning skills invoke before writing
//! the issue to GitHub.
//!
//! The contract: every decomposed-mode body that passes the
//! validator must also pass `bin/flow plan-from-issue`'s extraction
//! logic at flow-start; vanilla-mode bodies must satisfy the
//! `## What` / `## Why` / `## Acceptance Criteria` triad without
//! sentinels. The validator's named consumers are the
//! `### Validate + File` step in `skills/flow-explore/SKILL.md`
//! (vanilla) and the `### Validate + File + Link` step in
//! `skills/flow-plan/SKILL.md` (decomposed); both route a non-`ok`
//! envelope back to the Revise loop with the validator's `message`
//! field shown.

mod common;

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use flow_rs::plan_from_issue::PLAN_BODY_BYTE_CAP;
use flow_rs::validate_issue_body::{run_impl_main, Args};

const BEGIN: &str = "<!-- FLOW-PLAN-BEGIN -->";
const END: &str = "<!-- FLOW-PLAN-END -->";

fn well_formed_body() -> String {
    format!(
        "## Problem\n\nProse.\n\n{}\n## Implementation Plan\n\n### Context\n\nContext prose.\n\n### Tasks\n\n#### Task 1: Do the thing\n\n- Description\n{}\n\n## Files\n\nMore prose.\n",
        BEGIN, END
    )
}

fn run(path: &Path) -> (serde_json::Value, i32) {
    run_with_mode(path, "decomposed")
}

fn run_vanilla(path: &Path) -> (serde_json::Value, i32) {
    run_with_mode(path, "vanilla")
}

fn run_with_mode(path: &Path, mode: &str) -> (serde_json::Value, i32) {
    let args = Args {
        body_file: path.to_path_buf(),
        mode: mode.to_string(),
    };
    run_impl_main(&args, path.parent().unwrap_or_else(|| Path::new(".")))
}

fn well_formed_vanilla_body() -> String {
    "## What\n\nObservable problem statement.\n\n## Why\n\nWhy it matters.\n\n## Acceptance Criteria\n\n- Criterion 1\n- Criterion 2\n".to_string()
}

// --- happy path ---

#[test]
fn happy_path_returns_ok_with_tasks_total() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, well_formed_body()).unwrap();
    let (value, code) = run(&path);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["tasks_total"], 1);
}

#[test]
fn happy_path_counts_multiple_tasks() {
    let body = format!(
        "{}\n## Implementation Plan\n\n#### Task 1: a\n\n#### Task 2: b\n\n#### Task 3: c\n{}",
        BEGIN, END
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, body).unwrap();
    let (value, code) = run(&path);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["tasks_total"], 3);
}

// --- marker_count_wrong ---

#[test]
fn marker_count_wrong_when_zero_begin_markers() {
    let body = format!("## Implementation Plan\n#### Task 1: a\n{}", END);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, body).unwrap();
    let (value, code) = run(&path);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "marker_count_wrong");
}

#[test]
fn marker_count_wrong_when_two_begin_markers() {
    let body = format!(
        "{}\n{}\n## Implementation Plan\n#### Task 1: a\n{}",
        BEGIN, BEGIN, END
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, body).unwrap();
    let (value, code) = run(&path);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "marker_count_wrong");
}

#[test]
fn marker_count_wrong_when_three_end_markers() {
    let body = format!(
        "{}\n## Implementation Plan\n#### Task 1: a\n{}\n{}\n{}",
        BEGIN, END, END, END
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, body).unwrap();
    let (value, code) = run(&path);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "marker_count_wrong");
}

// --- plan_extraction_failed ---

#[test]
fn plan_extraction_failed_when_plan_content_is_empty() {
    // BEGIN and END are adjacent — extract_plan returns Empty.
    let body = format!("{}\n{}", BEGIN, END);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, body).unwrap();
    let (value, code) = run(&path);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "plan_extraction_failed");
}

// --- plan_missing_heading ---

#[test]
fn plan_missing_heading_when_content_starts_with_prose() {
    let body = format!(
        "{}\nProse content with no heading.\n\n#### Task 1: a\n{}",
        BEGIN, END
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, body).unwrap();
    let (value, code) = run(&path);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "plan_missing_heading");
}

#[test]
fn plan_missing_heading_when_content_starts_with_different_heading() {
    let body = format!("{}\n## Wrong Heading\n\n#### Task 1: a\n{}", BEGIN, END);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, body).unwrap();
    let (value, code) = run(&path);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "plan_missing_heading");
}

#[test]
fn plan_missing_heading_when_content_is_only_whitespace_after_extraction() {
    // The plan content is "## " followed by a newline, which trims
    // to "##" — extract_plan returns Ok (non-empty after trim), but
    // the heading check fails because "##" != "## Implementation Plan".
    let body = format!("{}\n## \n{}", BEGIN, END);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, body).unwrap();
    let (value, code) = run(&path);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "plan_missing_heading");
}

// --- no_tasks ---

#[test]
fn no_tasks_when_zero_task_headings_in_plan() {
    let body = format!(
        "{}\n## Implementation Plan\n\n### Context\n\nNo tasks here.\n{}",
        BEGIN, END
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, body).unwrap();
    let (value, code) = run(&path);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "no_tasks");
}

// --- body_read_failed ---

#[test]
fn body_read_failed_when_path_does_not_exist() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.md");
    let (value, code) = run(&path);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "body_read_failed");
}

#[test]
fn body_read_failed_when_path_is_a_directory() {
    let dir = tempfile::tempdir().unwrap();
    let subdir = dir.path().join("subdir");
    fs::create_dir(&subdir).unwrap();
    let (value, code) = run(&subdir);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "body_read_failed");
}

#[test]
fn body_read_failed_when_path_is_a_dangling_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("missing.md");
    let link = dir.path().join("link.md");
    symlink(&target, &link).unwrap();
    let (value, code) = run(&link);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "body_read_failed");
}

#[test]
fn body_read_failed_when_path_is_a_live_symlink_to_a_regular_file() {
    // Symlinks are rejected without following, even when the target
    // is a valid regular file — `symlink_metadata` reports the
    // symlink's own file_type, which is not `is_file()`.
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("real.md");
    fs::write(&target, well_formed_body()).unwrap();
    let link = dir.path().join("link.md");
    symlink(&target, &link).unwrap();
    let (value, code) = run(&link);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "body_read_failed");
}

// --- body_too_large ---

#[test]
fn body_too_large_when_file_exceeds_plan_body_byte_cap() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("huge.md");
    // PLAN_BODY_BYTE_CAP + 1 bytes — exactly one byte over the cap.
    let huge = "x".repeat(PLAN_BODY_BYTE_CAP + 1);
    fs::write(&path, huge).unwrap();
    let (value, code) = run(&path);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "body_too_large");
}

#[test]
fn body_read_failed_when_file_is_chmod_000() {
    // After `symlink_metadata` reports the entry is a regular file,
    // `File::open` can still fail with EACCES when the file's mode
    // is `000`. Per `.claude/rules/reachable-is-testable.md`
    // "Fixture recipes for the common hard cases" — chmod 000 is
    // the canonical way to drive the Err arm of File::open while
    // keeping the path's metadata visible. Restoring mode is
    // unnecessary because the TempDir Drop cleans up.
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("locked.md");
    fs::write(&path, well_formed_body()).unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o000);
    fs::set_permissions(&path, perms).unwrap();
    let (value, code) = run(&path);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "body_read_failed");
    // Restore mode so TempDir Drop can clean up on every platform.
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o644);
    fs::set_permissions(&path, perms).unwrap();
}

#[test]
fn body_read_failed_when_file_contains_non_utf8_bytes() {
    // `BufReader::read_to_string` returns Err when the underlying
    // bytes are not valid UTF-8. This drives the Err arm of the
    // read_to_string call without depending on filesystem-level
    // failure (permissions, missing-file, etc.). The body uses an
    // invalid UTF-8 sequence (an isolated 0xFF byte) at the head
    // of the buffer so the decode fails before any of the
    // marker-counting logic runs.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("non-utf8.md");
    fs::write(&path, b"\xFFabc").unwrap();
    let (value, code) = run(&path);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "body_read_failed");
}

#[test]
fn body_at_cap_passes_size_check_and_routes_to_marker_check() {
    // Exactly PLAN_BODY_BYTE_CAP bytes — the size check passes but
    // the body has no FLOW-PLAN markers, so the next check
    // (marker_count_wrong) is the one that fires. Proves the
    // off-by-one boundary on PLAN_BODY_BYTE_CAP is correct: equal-
    // to-cap is accepted, cap+1 is rejected.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("at-cap.md");
    let at_cap = "x".repeat(PLAN_BODY_BYTE_CAP);
    fs::write(&path, at_cap).unwrap();
    let (value, code) = run(&path);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "marker_count_wrong");
}

// --- mode dispatch ---

#[test]
fn decomposed_mode_default_preserves_existing_behavior() {
    // Explicit `--mode decomposed` matches the default-mode path
    // exercised by every test above: a well-formed body returns
    // status=ok with the task count.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, well_formed_body()).unwrap();
    let (value, code) = run_with_mode(&path, "decomposed");
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["tasks_total"], 1);
}

#[test]
fn invalid_mode_returns_structured_error() {
    // A `--mode <other>` invocation must fail closed with a JSON
    // envelope so the calling skill can route through its Revise
    // loop, not see a clap exit-2 surface.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, well_formed_vanilla_body()).unwrap();
    let (value, code) = run_with_mode(&path, "fancy");
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "invalid_mode");
}

// --- mode normalization (security-gates "Normalize Before Comparing") ---

#[test]
fn vanilla_mode_normalizes_uppercase() {
    // `--mode VANILLA` must dispatch to the vanilla branch — the
    // normalization step lowercases ASCII before comparison.
    // Without normalization, uppercase variants fall through to the
    // invalid_mode arm and the calling skill enters a Revise loop
    // it cannot escape.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, well_formed_vanilla_body()).unwrap();
    let (value, code) = run_with_mode(&path, "VANILLA");
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
}

#[test]
fn vanilla_mode_normalizes_mixed_case() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, well_formed_vanilla_body()).unwrap();
    let (value, code) = run_with_mode(&path, "Vanilla");
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
}

#[test]
fn vanilla_mode_normalizes_leading_whitespace() {
    // Leading whitespace is trimmed per security-gates.md so
    // shell-quoting accidents (e.g. ` vanilla` from a stray space)
    // do not defeat the gate.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, well_formed_vanilla_body()).unwrap();
    let (value, code) = run_with_mode(&path, " vanilla");
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
}

#[test]
fn vanilla_mode_normalizes_trailing_newline() {
    // Trailing newlines from config-file substitution must be
    // trimmed before comparison.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, well_formed_vanilla_body()).unwrap();
    let (value, code) = run_with_mode(&path, "vanilla\n");
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
}

#[test]
fn vanilla_mode_normalizes_embedded_nul() {
    // Per security-gates.md "Normalize Before Comparing", embedded
    // NULs from truncated writes or editor artifacts must be
    // stripped before byte-equality comparison.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, well_formed_vanilla_body()).unwrap();
    let (value, code) = run_with_mode(&path, "vanilla\0");
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
}

#[test]
fn decomposed_mode_normalizes_uppercase() {
    // The case-insensitivity guard is symmetric across both
    // branches of the mode dispatch — `DECOMPOSED` must route to
    // validate_decomposed exactly as `VANILLA` routes to
    // validate_vanilla.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, well_formed_body()).unwrap();
    let (value, code) = run_with_mode(&path, "DECOMPOSED");
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["tasks_total"], 1);
}

#[test]
fn invalid_mode_error_preserves_raw_value_in_message() {
    // The error envelope's `message` field must show the raw
    // (un-normalized) value the caller passed so the user can spot
    // typos. The normalized form is the dispatch key, not the
    // diagnostic surface.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, well_formed_vanilla_body()).unwrap();
    let (value, code) = run_with_mode(&path, "fAnCy");
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "invalid_mode");
    assert!(
        value["message"].as_str().unwrap().contains("'fAnCy'"),
        "error message should preserve the raw value, got: {value}"
    );
}

// --- vanilla mode happy path ---

#[test]
fn vanilla_mode_accepts_body_with_what_why_acceptance_headings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, well_formed_vanilla_body()).unwrap();
    let (value, code) = run_vanilla(&path);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
}

// --- vanilla mode missing-section rejections ---

#[test]
fn vanilla_missing_section_what() {
    let body = "## Why\n\nProse.\n\n## Acceptance Criteria\n\n- one\n";
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, body).unwrap();
    let (value, code) = run_vanilla(&path);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "vanilla_missing_section_what");
}

#[test]
fn vanilla_missing_section_why() {
    let body = "## What\n\nProse.\n\n## Acceptance Criteria\n\n- one\n";
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, body).unwrap();
    let (value, code) = run_vanilla(&path);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "vanilla_missing_section_why");
}

#[test]
fn vanilla_missing_section_acceptance() {
    let body = "## What\n\nProse.\n\n## Why\n\nProse.\n";
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, body).unwrap();
    let (value, code) = run_vanilla(&path);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "vanilla_missing_section_acceptance");
}

// --- vanilla mode forbidden-construct rejections ---

#[test]
fn vanilla_has_sentinels_when_begin_marker_present() {
    let body = format!(
        "## What\n\nProse.\n\n## Why\n\nProse.\n\n## Acceptance Criteria\n\n- one\n\n{}\n## Implementation Plan\n\n#### Task 1: x\n{}",
        BEGIN, END
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, body).unwrap();
    let (value, code) = run_vanilla(&path);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "vanilla_has_sentinels");
}

#[test]
fn vanilla_has_sentinels_when_end_marker_present_alone() {
    // Even a stray END marker without a BEGIN trips the sentinel
    // check — vanilla bodies must contain neither.
    let body = format!(
        "## What\n\nProse.\n\n## Why\n\nProse.\n\n## Acceptance Criteria\n\n- one\n\nstray {} marker\n",
        END
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, body).unwrap();
    let (value, code) = run_vanilla(&path);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "vanilla_has_sentinels");
}

#[test]
fn vanilla_has_implementation_plan_heading() {
    let body = "## What\n\nProse.\n\n## Why\n\nProse.\n\n## Acceptance Criteria\n\n- one\n\n## Implementation Plan\n\nplan content\n";
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("body.md");
    fs::write(&path, body).unwrap();
    let (value, code) = run_vanilla(&path);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "vanilla_has_implementation_plan");
}
