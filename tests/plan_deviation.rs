//! Library-level unit tests for the plan-deviation scanner and
//! `run_impl` gate. Subprocess integration tests that exercise
//! the same gate through the compiled binary live in
//! `tests/plan_deviation_integration.rs`.
//!
//! The scanner is pure-function — it accepts plan content and a
//! staged diff as strings, returns a `Vec<Deviation>`. The tests
//! here drive it directly with crafted strings to exercise every
//! branch of the plan-parser and diff-parser, plus the
//! acknowledgment substring match and the `run_impl` orchestration
//! branches.

use std::fs;
use std::path::Path;

use flow_rs::plan_deviation::{acknowledged, run_impl, scan, Deviation};

mod common;

// --- scan: Tasks-section boundaries ---

#[test]
fn scan_plan_without_tasks_section_returns_empty() {
    let plan = "# Plan\n\n## Context\n\nSome context.\n\n## Risks\n\nNone.\n";
    let diff = "";
    assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
}

#[test]
fn scan_plan_discussion_prose_in_risks_section_ignored() {
    // Plan's Risks section contains a code fence with a value
    // that would drift against the diff. Parser must scope to
    // `## Tasks` only and ignore Risks prose.
    let plan = concat!(
        "## Risks\n\n",
        "```rust\n",
        "fn test_foo() {\n",
        "    let key = \"ignored\";\n",
        "}\n",
        "```\n\n",
        "## Tasks\n\n",
        "Task 1 — plain prose, no code blocks.\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+fn test_foo() {\n",
        "+    let key = \"actual\";\n",
        "+}\n",
    );
    assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
}

#[test]
fn scan_plan_with_h2_after_tasks_stops_at_boundary() {
    // `## Tasks` followed by `## Other` — the scanner must stop
    // at `## Other`. A divergent fixture below `## Other` must
    // not produce a deviation.
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1 — plain prose.\n\n",
        "## Other\n\n",
        "```rust\n",
        "fn test_foo() {\n",
        "    let key = \"ignored\";\n",
        "}\n",
        "```\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+fn test_foo() {\n",
        "+    let key = \"actual\";\n",
        "+}\n",
    );
    assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
}

#[test]
fn scan_plan_tasks_heading_inside_preceding_fence_ignored() {
    // A `## Tasks` literal inside a fenced code block in a
    // preceding section must NOT be treated as the Tasks
    // heading. The real `## Tasks` appears later; the scanner
    // must use that one.
    let plan = concat!(
        "## Context\n\n",
        "```text\n",
        "## Tasks\n",
        "not the real tasks heading\n",
        "```\n\n",
        "## Tasks\n\n",
        "Real tasks begin here.\n\n",
        "```rust\n",
        "fn test_foo() {\n",
        "    let key = \"expected\";\n",
        "}\n",
        "```\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+fn test_foo() {\n",
        "+    let key = \"actual\";\n",
        "+}\n",
    );
    let result = scan(plan, diff);
    assert_eq!(result.len(), 1, "real tasks heading must drive detection");
    assert_eq!(result[0].test_name, "test_foo");
    assert_eq!(result[0].plan_value, "expected");
}

#[test]
fn scan_plan_tasks_heading_with_trailing_content_recognized() {
    // `## Tasks foo` must be recognized as the Tasks heading
    // (the `starts_with("## Tasks ")` branch).
    let plan = concat!(
        "## Tasks and subsections\n\n",
        "Task 1.\n\n",
        "```rust\n",
        "fn test_foo() {\n",
        "    let key = \"expected\";\n",
        "}\n",
        "```\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+fn test_foo() {\n",
        "+    let key = \"actual\";\n",
        "+}\n",
    );
    let result = scan(plan, diff);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].plan_value, "expected");
}

// --- scan: fence-eligibility ---

#[test]
fn scan_plan_pseudocode_fence_skipped() {
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1 — test foo.\n\n",
        "```pseudocode\n",
        "fn test_foo() {\n",
        "    let key = \"expected\";\n",
        "}\n",
        "```\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+fn test_foo() {\n",
        "+    let key = \"actual\";\n",
        "+}\n",
    );
    assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
}

#[test]
fn scan_plan_tilde_fence_accepted() {
    // Tilde-delimited fences (`~~~`) are recognized per
    // CommonMark so an author using tilde fencing is not
    // silently bypassed.
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1 — test foo.\n\n",
        "~~~rust\n",
        "fn test_foo() {\n",
        "    let key = \"expected\";\n",
        "}\n",
        "~~~\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+fn test_foo() {\n",
        "+    let key = \"actual\";\n",
        "+}\n",
    );
    let result = scan(plan, diff);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].plan_value, "expected");
}

#[test]
fn scan_plan_untagged_fence_accepted() {
    // An untagged ``` fence is in the eligible set (empty lang
    // string) — fixtures must still be collected.
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1.\n\n",
        "```\n",
        "fn test_foo() {\n",
        "    let key = \"expected\";\n",
        "}\n",
        "```\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+fn test_foo() {\n",
        "+    let key = \"actual\";\n",
        "+}\n",
    );
    let result = scan(plan, diff);
    assert_eq!(result.len(), 1);
}

// --- scan: fixture extraction ---

#[test]
fn scan_plan_matching_diff_returns_empty() {
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1 — test foo.\n\n",
        "```rust\n",
        "fn test_foo() {\n",
        "    let key = \"expected\";\n",
        "}\n",
        "```\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+fn test_foo() {\n",
        "+    let key = \"expected\";\n",
        "+}\n",
    );
    assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
}

#[test]
fn scan_plan_diverging_diff_returns_one_deviation() {
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1 — test foo.\n\n",
        "```rust\n",
        "fn test_foo() {\n",
        "    let key = \"expected\";\n",
        "}\n",
        "```\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+fn test_foo() {\n",
        "+    let key = \"actual\";\n",
        "+}\n",
    );
    let result = scan(plan, diff);
    assert_eq!(result.len(), 1, "expected exactly one deviation");
    assert_eq!(result[0].test_name, "test_foo");
    assert_eq!(result[0].fixture_key, "key");
    assert_eq!(result[0].plan_value, "expected");
}

#[test]
fn scan_plan_assignment_without_fn_context_skipped() {
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1 — bare assignment with no preceding fn.\n\n",
        "```rust\n",
        "let bare = \"orphan\";\n",
        "```\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,1 @@\n",
        "+let bare = \"different\";\n",
    );
    assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
}

#[test]
fn scan_plan_multiple_tests_one_drifts_returns_one_deviation() {
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1 — two tests.\n\n",
        "```rust\n",
        "fn test_alpha() {\n",
        "    let key = \"alpha_ok\";\n",
        "}\n",
        "\n",
        "fn test_beta() {\n",
        "    let key = \"beta_ok\";\n",
        "}\n",
        "```\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,6 @@\n",
        "+fn test_alpha() {\n",
        "+    let key = \"alpha_ok\";\n",
        "+}\n",
        "+fn test_beta() {\n",
        "+    let key = \"beta_different\";\n",
        "+}\n",
    );
    let result = scan(plan, diff);
    assert_eq!(result.len(), 1, "only test_beta should drift");
    assert_eq!(result[0].test_name, "test_beta");
    assert_eq!(result[0].plan_value, "beta_ok");
}

// --- scan: reserved keys ---

#[test]
fn scan_plan_double_quoted_reserved_key_let_skipped() {
    // `let = "x"` (reserved key) must be skipped. A following
    // non-reserved `key = "expected"` still drives detection.
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1 — test foo.\n\n",
        "```rust\n",
        "fn test_foo() {\n",
        "    let = \"skipped\";\n",
        "    key = \"expected\";\n",
        "}\n",
        "```\n",
    );
    // Diff fixture keeps `key = "expected"` — no drift. If the
    // `let` fixture were collected, "skipped" would be absent
    // from the diff's literals and produce a spurious deviation.
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+fn test_foo() {\n",
        "+    key = \"expected\";\n",
        "+}\n",
    );
    assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
}

#[test]
fn scan_plan_double_quoted_reserved_key_const_skipped() {
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1.\n\n",
        "```rust\n",
        "fn test_foo() {\n",
        "    const = \"skipped\";\n",
        "    key = \"expected\";\n",
        "}\n",
        "```\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+fn test_foo() {\n",
        "+    key = \"expected\";\n",
        "+}\n",
    );
    assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
}

#[test]
fn scan_plan_double_quoted_reserved_key_static_skipped() {
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1.\n\n",
        "```rust\n",
        "fn test_foo() {\n",
        "    static = \"skipped\";\n",
        "    key = \"expected\";\n",
        "}\n",
        "```\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+fn test_foo() {\n",
        "+    key = \"expected\";\n",
        "+}\n",
    );
    assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
}

#[test]
fn scan_plan_double_quoted_reserved_key_mut_skipped() {
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1.\n\n",
        "```rust\n",
        "fn test_foo() {\n",
        "    mut = \"skipped\";\n",
        "    key = \"expected\";\n",
        "}\n",
        "```\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+fn test_foo() {\n",
        "+    key = \"expected\";\n",
        "+}\n",
    );
    assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
}

#[test]
fn scan_plan_reserved_keys_are_case_sensitive() {
    // Upper-case `LET` is a user identifier, not a reserved
    // keyword — the triple must be collected. A matching diff
    // produces no drift; a divergent diff would. Confirm
    // collection via divergent diff.
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1.\n\n",
        "```rust\n",
        "fn test_foo() {\n",
        "    LET = \"expected\";\n",
        "}\n",
        "```\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+fn test_foo() {\n",
        "+    LET = \"actual\";\n",
        "+}\n",
    );
    let result = scan(plan, diff);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].fixture_key, "LET");
    assert_eq!(result[0].plan_value, "expected");
}

// --- scan: single-quoted assignments ---

#[test]
fn scan_plan_single_quoted_assign_collected() {
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1 — test bar.\n\n",
        "```rust\n",
        "fn test_bar() {\n",
        "    key = 'y';\n",
        "}\n",
        "```\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+fn test_bar() {\n",
        "+    key = 'z';\n",
        "+}\n",
    );
    let result = scan(plan, diff);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].test_name, "test_bar");
    assert_eq!(result[0].plan_value, "y");
}

#[test]
fn scan_plan_single_quoted_reserved_key_skipped() {
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1 — test baz.\n\n",
        "```rust\n",
        "fn test_baz() {\n",
        "    let = 'skipped';\n",
        "    key = 'y';\n",
        "}\n",
        "```\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+fn test_baz() {\n",
        "+    key = 'y';\n",
        "+}\n",
    );
    assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
}

// --- scan: unclosed fences ---

#[test]
fn scan_plan_unclosed_fence_rewinds_triples() {
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1 — unclosed fence.\n\n",
        "```rust\n",
        "fn test_unclosed() {\n",
        "    let key = \"stray\";\n",
        "}\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+fn test_unclosed() {\n",
        "+    let key = \"different\";\n",
        "+}\n",
    );
    assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
}

// --- scan: diff-side filters ---

#[test]
fn scan_diff_non_rust_file_ignored() {
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1 — test foo.\n\n",
        "```rust\n",
        "fn test_foo() {\n",
        "    let key = \"expected\";\n",
        "}\n",
        "```\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.py b/tests/foo.py\n",
        "--- a/tests/foo.py\n",
        "+++ b/tests/foo.py\n",
        "@@ -0,0 +1,2 @@\n",
        "+def test_foo():\n",
        "+    key = 'actual'\n",
    );
    assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
}

#[test]
fn scan_diff_test_not_in_plan_ignored() {
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1 — test foo.\n\n",
        "```rust\n",
        "fn test_foo() {\n",
        "    let key = \"expected\";\n",
        "}\n",
        "```\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+fn test_unrelated() {\n",
        "+    let key = \"anything\";\n",
        "+}\n",
    );
    assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
}

#[test]
fn scan_diff_prefix_renamed_test_does_not_match_intentionally() {
    // Contract: v1 uses exact `fn <name>(` match. A renamed test
    // (even a prefix-extended one) is invisible to the detector.
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1 — test foo.\n\n",
        "```rust\n",
        "fn test_foo() {\n",
        "    let key = \"expected\";\n",
        "}\n",
        "```\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+fn test_foo_happy_path() {\n",
        "+    let key = \"actual\";\n",
        "+}\n",
    );
    assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
}

#[test]
fn scan_diff_context_line_not_counted_as_added_fn() {
    // A diff that lacks `+` prefix on the fn boundary must not
    // register `test_foo` as an added test. Since `test_foo` is
    // absent from the diff map, the gate should skip.
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1 — test foo.\n\n",
        "```rust\n",
        "fn test_foo() {\n",
        "    let key = \"expected\";\n",
        "}\n",
        "```\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -1,5 +1,5 @@\n",
        " fn test_foo() {\n",
        "-    let key = \"expected\";\n",
        "+    let other = \"unrelated\";\n",
        " }\n",
    );
    // `other = "unrelated"` is in the diff as an added line, but
    // no `+fn test_foo` boundary was seen, so current_test never
    // becomes Some("test_foo"). The plan-named test_foo has no
    // entry in the diff map and the gate skips.
    assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
}

#[test]
fn scan_diff_attribute_before_fn_recognized() {
    // `+#[test] fn test_foo(` should be recognized as an added
    // test boundary — attribute-prefixed `fn` is tolerated.
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1.\n\n",
        "```rust\n",
        "fn test_foo() {\n",
        "    let key = \"expected\";\n",
        "}\n",
        "```\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+#[test] fn test_foo() {\n",
        "+    let key = \"actual\";\n",
        "+}\n",
    );
    let result = scan(plan, diff);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].plan_value, "expected");
}

#[test]
fn scan_diff_pub_prefix_before_fn_recognized() {
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1.\n\n",
        "```rust\n",
        "fn test_foo() {\n",
        "    let key = \"expected\";\n",
        "}\n",
        "```\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,3 @@\n",
        "+pub fn test_foo() {\n",
        "+    let key = \"actual\";\n",
        "+}\n",
    );
    let result = scan(plan, diff);
    assert_eq!(result.len(), 1);
}

#[test]
fn scan_diff_hunk_header_does_not_mutate_scope() {
    // `@@` hunk headers must not mutate test scope. After a hunk
    // header inside a plan-named test body, literals on the next
    // added lines must still be attributed to that test.
    let plan = concat!(
        "## Tasks\n\n",
        "Task 1.\n\n",
        "```rust\n",
        "fn test_foo() {\n",
        "    let key = \"expected\";\n",
        "}\n",
        "```\n",
    );
    let diff = concat!(
        "diff --git a/tests/foo.rs b/tests/foo.rs\n",
        "--- a/tests/foo.rs\n",
        "+++ b/tests/foo.rs\n",
        "@@ -0,0 +1,2 @@\n",
        "+fn test_foo() {\n",
        "@@ -5,0 +5,1 @@\n",
        "+    let key = \"expected\";\n",
    );
    // "expected" is present in test_foo's added lines, so no
    // deviation despite the hunk-header interruption.
    assert_eq!(scan(plan, diff), Vec::<Deviation>::new());
}

// --- acknowledged ---

fn make_deviation(test_name: &str, plan_value: &str) -> Deviation {
    Deviation {
        test_name: test_name.to_string(),
        fixture_key: "key".to_string(),
        plan_value: plan_value.to_string(),
        plan_line: 1,
    }
}

#[test]
fn acknowledged_log_line_contains_both_returns_true() {
    let dev = make_deviation("test_foo", "/flow:flow-plan");
    let log = "2026-04-15T10:00:00-08:00 [Phase 3] Plan signature deviation: test_foo drifted from /flow:flow-plan to /flow:flow-code-review. Reason: X.\n";
    assert!(acknowledged(&dev, log));
}

#[test]
fn acknowledged_log_line_missing_plan_value_returns_false() {
    let dev = make_deviation("test_foo", "/flow:flow-plan");
    let log = "2026-04-15T10:00:00-08:00 [Phase 3] test_foo is under development.\n";
    assert!(!acknowledged(&dev, log));
}

#[test]
fn acknowledged_missing_log_returns_false() {
    let dev = make_deviation("test_foo", "/flow:flow-plan");
    let log = "";
    assert!(!acknowledged(&dev, log));
}

#[test]
fn acknowledged_log_split_lines_returns_false() {
    // Test name on one line, plan value on another. Acknowledgment
    // requires both on the same line.
    let dev = make_deviation("test_foo", "/flow:flow-plan");
    let log = concat!(
        "2026-04-15T10:00:00-08:00 [Phase 3] test_foo is the test under review.\n",
        "2026-04-15T10:01:00-08:00 [Phase 3] The plan named /flow:flow-plan earlier.\n",
    );
    assert!(!acknowledged(&dev, log));
}

#[test]
fn acknowledged_log_contains_both_on_single_line_case_sensitive() {
    // Plan value has different case than what appears in the log —
    // substring comparison is case-sensitive, so this does NOT
    // acknowledge the deviation.
    let dev = make_deviation("test_foo", "/flow:flow-plan");
    let log =
        "2026-04-15T10:00:00-08:00 [Phase 3] test_foo drifted from /FLOW:FLOW-PLAN to new value.\n";
    assert!(!acknowledged(&dev, log));
}

#[test]
fn acknowledged_empty_plan_value_returns_false() {
    // Empty plan_value would match any line via `contains`.
    // Guard against trivial acknowledgment.
    let dev = make_deviation("test_foo", "");
    let log = "2026-04-15T10:00:00-08:00 [Phase 3] test_foo entry.\n";
    assert!(!acknowledged(&dev, log));
}

#[test]
fn acknowledged_plan_value_only_as_substring_of_test_name_returns_false() {
    // If the plan value happens to be a substring of the test
    // name itself — e.g. test_name "test_foo", plan_value "foo" —
    // acknowledgment must verify plan_value appears independently
    // of the test name on the same line.
    let dev = make_deviation("test_foo", "foo");
    let log = "[Phase 3] test_foo entered the review phase.\n";
    assert!(
        !acknowledged(&dev, log),
        "plan_value that is only a substring of test_name must not acknowledge"
    );
}

// --- run_impl ---

const RUN_IMPL_BRANCH: &str = "devtest";

const DRIFTING_PLAN: &str = concat!(
    "## Tasks\n\n",
    "Task 1 — test foo.\n\n",
    "```rust\n",
    "fn test_foo() {\n",
    "    let key = \"expected\";\n",
    "}\n",
    "```\n",
);

const DRIFTING_DIFF: &str = concat!(
    "diff --git a/tests/foo.rs b/tests/foo.rs\n",
    "--- a/tests/foo.rs\n",
    "+++ b/tests/foo.rs\n",
    "@@ -0,0 +1,3 @@\n",
    "+fn test_foo() {\n",
    "+    let key = \"actual\";\n",
    "+}\n",
);

const MATCHING_DIFF: &str = concat!(
    "diff --git a/tests/foo.rs b/tests/foo.rs\n",
    "--- a/tests/foo.rs\n",
    "+++ b/tests/foo.rs\n",
    "@@ -0,0 +1,3 @@\n",
    "+fn test_foo() {\n",
    "+    let key = \"expected\";\n",
    "+}\n",
);

/// Canonicalized tempdir plus an empty `.flow-states/` directory.
/// Held by the caller for filesystem lifetime.
fn run_impl_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir
        .path()
        .canonicalize()
        .expect("tempdir path must canonicalize");
    fs::create_dir_all(root.join(".flow-states")).expect("create .flow-states dir");
    (dir, root)
}

fn write_state(root: &Path, branch: &str, contents: &str) {
    let state_path = root.join(".flow-states").join(format!("{}.json", branch));
    fs::write(&state_path, contents).expect("write state file");
}

fn write_plan(root: &Path, branch: &str, contents: &str) {
    let plan_path = root
        .join(".flow-states")
        .join(format!("{}-plan.md", branch));
    fs::write(&plan_path, contents).expect("write plan file");
}

fn write_log(root: &Path, branch: &str, contents: &str) {
    let log_path = root.join(".flow-states").join(format!("{}.log", branch));
    fs::write(&log_path, contents).expect("write log file");
}

#[test]
fn run_impl_invalid_branch_returns_ok() {
    // A slash-containing branch fails `FlowPaths::try_new` and
    // returns Ok(()) — no active flow on this branch.
    let (_dir, root) = run_impl_fixture();
    let result = run_impl(&root, "feature/foo", DRIFTING_DIFF);
    assert_eq!(result, Ok(()));
}

#[test]
fn run_impl_empty_branch_returns_ok() {
    let (_dir, root) = run_impl_fixture();
    let result = run_impl(&root, "", DRIFTING_DIFF);
    assert_eq!(result, Ok(()));
}

#[test]
fn run_impl_missing_state_file_returns_ok() {
    let (_dir, root) = run_impl_fixture();
    let result = run_impl(&root, RUN_IMPL_BRANCH, DRIFTING_DIFF);
    assert_eq!(result, Ok(()));
}

#[test]
fn run_impl_empty_state_file_returns_ok() {
    let (_dir, root) = run_impl_fixture();
    write_state(&root, RUN_IMPL_BRANCH, "");
    let result = run_impl(&root, RUN_IMPL_BRANCH, DRIFTING_DIFF);
    assert_eq!(result, Ok(()));
}

#[test]
fn run_impl_non_json_state_file_returns_ok() {
    let (_dir, root) = run_impl_fixture();
    write_state(&root, RUN_IMPL_BRANCH, "not json {{");
    let result = run_impl(&root, RUN_IMPL_BRANCH, DRIFTING_DIFF);
    assert_eq!(result, Ok(()));
}

#[test]
fn run_impl_non_object_state_root_returns_ok() {
    let (_dir, root) = run_impl_fixture();
    write_state(&root, RUN_IMPL_BRANCH, r#"["array","not","object"]"#);
    let result = run_impl(&root, RUN_IMPL_BRANCH, DRIFTING_DIFF);
    assert_eq!(result, Ok(()));
}

#[test]
fn run_impl_state_without_plan_path_returns_ok() {
    let (_dir, root) = run_impl_fixture();
    write_state(&root, RUN_IMPL_BRANCH, r#"{"branch":"devtest"}"#);
    let result = run_impl(&root, RUN_IMPL_BRANCH, DRIFTING_DIFF);
    assert_eq!(result, Ok(()));
}

#[test]
fn run_impl_state_with_empty_plan_path_returns_ok() {
    let (_dir, root) = run_impl_fixture();
    write_state(
        &root,
        RUN_IMPL_BRANCH,
        r#"{"branch":"devtest","files":{"plan":""}}"#,
    );
    let result = run_impl(&root, RUN_IMPL_BRANCH, DRIFTING_DIFF);
    assert_eq!(result, Ok(()));
}

#[test]
fn run_impl_plan_path_set_but_file_missing_returns_ok() {
    let (_dir, root) = run_impl_fixture();
    write_state(
        &root,
        RUN_IMPL_BRANCH,
        r#"{"branch":"devtest","files":{"plan":".flow-states/devtest-plan.md"}}"#,
    );
    let result = run_impl(&root, RUN_IMPL_BRANCH, DRIFTING_DIFF);
    assert_eq!(result, Ok(()));
}

#[test]
fn run_impl_legacy_plan_file_key_honored() {
    // Older state files store the plan path at top-level
    // `plan_file` instead of `files.plan`. The fallback must
    // honor the legacy key.
    let (_dir, root) = run_impl_fixture();
    write_state(
        &root,
        RUN_IMPL_BRANCH,
        r#"{"branch":"devtest","plan_file":".flow-states/devtest-plan.md"}"#,
    );
    write_plan(&root, RUN_IMPL_BRANCH, DRIFTING_PLAN);
    let result = run_impl(&root, RUN_IMPL_BRANCH, DRIFTING_DIFF);
    match result {
        Err(devs) => assert_eq!(devs.len(), 1),
        Ok(_) => panic!("legacy plan_file must still drive detection"),
    }
}

#[test]
fn run_impl_no_deviations_returns_ok() {
    let (_dir, root) = run_impl_fixture();
    write_state(
        &root,
        RUN_IMPL_BRANCH,
        r#"{"branch":"devtest","files":{"plan":".flow-states/devtest-plan.md"}}"#,
    );
    write_plan(&root, RUN_IMPL_BRANCH, DRIFTING_PLAN);
    let result = run_impl(&root, RUN_IMPL_BRANCH, MATCHING_DIFF);
    assert_eq!(result, Ok(()));
}

#[test]
fn run_impl_deviations_all_acknowledged_returns_ok() {
    let (_dir, root) = run_impl_fixture();
    write_state(
        &root,
        RUN_IMPL_BRANCH,
        r#"{"branch":"devtest","files":{"plan":".flow-states/devtest-plan.md"}}"#,
    );
    write_plan(&root, RUN_IMPL_BRANCH, DRIFTING_PLAN);
    write_log(
        &root,
        RUN_IMPL_BRANCH,
        "2026-04-15T10:00:00-08:00 [Phase 3] Plan signature deviation: test_foo drifted from expected to actual.\n",
    );
    let result = run_impl(&root, RUN_IMPL_BRANCH, DRIFTING_DIFF);
    assert_eq!(result, Ok(()));
}

#[test]
fn run_impl_unacknowledged_deviations_returns_err() {
    let (_dir, root) = run_impl_fixture();
    write_state(
        &root,
        RUN_IMPL_BRANCH,
        r#"{"branch":"devtest","files":{"plan":".flow-states/devtest-plan.md"}}"#,
    );
    write_plan(&root, RUN_IMPL_BRANCH, DRIFTING_PLAN);
    let result = run_impl(&root, RUN_IMPL_BRANCH, DRIFTING_DIFF);
    match result {
        Err(devs) => {
            assert_eq!(
                devs.len(),
                1,
                "expected exactly one unacknowledged deviation"
            );
            assert_eq!(devs[0].test_name, "test_foo");
            assert_eq!(devs[0].plan_value, "expected");
        }
        Ok(_) => panic!("expected Err with unacknowledged deviation"),
    }
}

#[test]
fn run_impl_unreadable_log_treated_as_empty() {
    // No log file on disk — `fs::read_to_string` fails and
    // `unwrap_or_default` yields an empty string, so the drift
    // remains unacknowledged.
    let (_dir, root) = run_impl_fixture();
    write_state(
        &root,
        RUN_IMPL_BRANCH,
        r#"{"branch":"devtest","files":{"plan":".flow-states/devtest-plan.md"}}"#,
    );
    write_plan(&root, RUN_IMPL_BRANCH, DRIFTING_PLAN);
    let result = run_impl(&root, RUN_IMPL_BRANCH, DRIFTING_DIFF);
    assert!(
        result.is_err(),
        "missing log file must leave drift unacknowledged"
    );
}
