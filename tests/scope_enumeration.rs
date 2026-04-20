//! Tests for the scope-enumeration scanner.
//!
//! Two kinds of tests live here:
//!
//! 1. **Corpus contract tests** — scan the committed prose corpus
//!    (`CLAUDE.md`, `.claude/rules/*.md`, `skills/**/SKILL.md`,
//!    `.claude/skills/**/SKILL.md`) for universal-coverage language
//!    without a named enumeration nearby. Plan files (per-branch,
//!    ephemeral) are covered at runtime by `bin/flow plan-check`.
//! 2. **Unit tests** — exercise `scan` behavior across trigger
//!    matching, enumeration detection, fenced-block skipping,
//!    opt-out comments, negation prefixes, and window boundaries.
//!    Migrated from `src/scope_enumeration.rs` per
//!    `.claude/rules/test-placement.md` (no inline `#[cfg(test)]`
//!    in src). Trigger-regex behavior is tested through `scan` —
//!    the regex helper is module-private and there's no
//!    justification for making it public solely to test it.

use std::fs;
use std::path::PathBuf;

use flow_rs::scope_enumeration::{scan, Violation};

mod common;

/// Pretty-print a list of violations for assertion failure messages.
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

/// Read every `.md` file under a directory recursively and return
/// `(absolute_path, content)` pairs.
fn read_md_files(dir: &PathBuf) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    walk(dir, &mut out);
    out
}

fn walk(dir: &PathBuf, out: &mut Vec<(PathBuf, String)>) {
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

/// Path placeholder used in unit-test assertions where the file
/// identity doesn't matter — only the `scan` result does.
fn dummy_path() -> PathBuf {
    PathBuf::from("dummy.md")
}

// === Corpus contract tests ===

// --- scan CLAUDE.md ---

#[test]
fn claude_md_has_no_unenumerated_universal_claims() {
    let path = common::repo_root().join("CLAUDE.md");
    let content = fs::read_to_string(&path).expect("CLAUDE.md must exist");
    let violations = scan(&content, &path);
    assert!(
        violations.is_empty(),
        "CLAUDE.md has unenumerated universal-coverage prose:\n{}",
        format_violations(&violations)
    );
}

// --- scan .claude/rules/*.md ---

#[test]
fn rules_have_no_unenumerated_universal_claims() {
    let rules_dir = common::repo_root().join(".claude").join("rules");
    let files = read_md_files(&rules_dir);
    assert!(!files.is_empty(), "expected .claude/rules/*.md files");

    let mut all_violations = Vec::new();
    for (path, content) in &files {
        let violations = scan(content, path);
        all_violations.extend(violations);
    }

    assert!(
        all_violations.is_empty(),
        ".claude/rules/ has unenumerated universal-coverage prose:\n{}",
        format_violations(&all_violations)
    );
}

// --- scan skills/**/SKILL.md (phase + utility skills) ---

#[test]
fn skills_have_no_unenumerated_universal_claims() {
    let skills_dir = common::skills_dir();
    let files = read_md_files(&skills_dir);
    assert!(!files.is_empty(), "expected skills/**/SKILL.md files");

    let mut all_violations = Vec::new();
    for (path, content) in &files {
        if path.file_name().and_then(|f| f.to_str()) != Some("SKILL.md") {
            continue;
        }
        let violations = scan(content, path);
        all_violations.extend(violations);
    }

    assert!(
        all_violations.is_empty(),
        "skills/**/SKILL.md has unenumerated universal-coverage prose:\n{}",
        format_violations(&all_violations)
    );
}

// --- scan .claude/skills/**/SKILL.md (maintainer skills) ---

#[test]
fn maintainer_skills_have_no_unenumerated_universal_claims() {
    let dot_skills_dir = common::repo_root().join(".claude").join("skills");
    if !dot_skills_dir.exists() {
        return;
    }
    let files = read_md_files(&dot_skills_dir);

    let mut all_violations = Vec::new();
    for (path, content) in &files {
        if path.file_name().and_then(|f| f.to_str()) != Some("SKILL.md") {
            continue;
        }
        let violations = scan(content, path);
        all_violations.extend(violations);
    }

    assert!(
        all_violations.is_empty(),
        ".claude/skills/**/SKILL.md has unenumerated universal-coverage prose:\n{}",
        format_violations(&all_violations)
    );
}

// === Unit tests (migrated from src/scope_enumeration.rs) ===
//
// Trigger detection was previously asserted against the private
// `trigger_regex()` helper. The public surface is `scan`, which
// compiles and uses the same regex; every trigger-matching claim
// below is verified by constructing content that would be flagged
// iff the trigger matched, then asserting `scan` produces the
// expected violation count.

// --- trigger vocabulary: matches ---

#[test]
fn trigger_matches_every_subcommand() {
    let content = "add the guard to every subcommand.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_all_runners() {
    let content = "apply to all runners.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_each_cli_entry_point() {
    let content = "gate each CLI entry point.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_state_mutator() {
    let content = "every state mutator must enforce the guard.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_state_mutator_hyphenated() {
    let content = "every state-mutator must enforce the guard.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_bare_mutator() {
    let content = "every mutator must log.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_dispatch_path() {
    let content = "every dispatch path carries the recursion guard.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_every_sibling_entry_point() {
    let content = "every sibling entry point in the family must enforce.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_every_single_sibling_entry_point() {
    // Two-adjective form: "single" and "sibling" both precede "entry point".
    let content = "every single sibling entry point enforces the guard.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_every_handler() {
    // Adversarial finding A2 — plausible noun for hook/request/event
    // handler families across the FLOW codebase.
    let content = "apply the drift check to every handler we register.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_plural_subcommands() {
    let content = "all subcommands are gated.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_matches_plural_callers() {
    let content = "grep for all callers of the helper.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn trigger_is_case_insensitive() {
    let v1 = scan("Every Subcommand must enforce.\n", &dummy_path());
    assert_eq!(v1.len(), 1, "{:?}", v1);
    let v2 = scan("ALL RUNNERS apply the check.\n", &dummy_path());
    assert_eq!(v2.len(), 1, "{:?}", v2);
}

// --- trigger vocabulary: rejections ---

#[test]
fn trigger_rejects_bare_command_intentionally() {
    // Adversarial finding A1 ("every command") — acknowledged
    // vocabulary gap. Bare `command` produces too many false
    // positives in the current tree. See the Vocabulary section of
    // `.claude/rules/scope-enumeration.md`.
    let content = "every command must enforce the check.\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty(), "bare 'command' must not trigger, got {:?}", v);
}

#[test]
fn trigger_rejects_bare_module_intentionally() {
    // Adversarial finding A3 ("every module") — acknowledged
    // vocabulary gap for the same reason as `command`.
    let content = "wire the recursion guard into every module.\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty(), "bare 'module' must not trigger, got {:?}", v);
}

#[test]
fn trigger_rejects_non_code_nouns() {
    assert!(scan("every commit must pass CI.\n", &dummy_path()).is_empty());
    assert!(scan("all developers should read this.\n", &dummy_path()).is_empty());
    assert!(scan("each release note is reviewed.\n", &dummy_path()).is_empty());
}

// --- scan: positive (enumeration present) ---

#[test]
fn scan_passes_inline_parenthetical() {
    let content = "`guard` runs on every subcommand (`foo`, `bar`, `baz`).\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty(), "expected no violations, got {:?}", v);
}

#[test]
fn scan_passes_forward_bullet_window() {
    let content = "Add the guard to every subcommand:\n\n- `foo` — does X\n- `bar` — does Y\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty(), "expected no violations, got {:?}", v);
}

#[test]
fn scan_passes_backward_bullet_window() {
    let content =
        "The two callsites:\n\n- `foo`\n- `bar`\n\nA test at each callsite should exercise X.\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty(), "expected no violations, got {:?}", v);
}

#[test]
fn scan_passes_sibling_entry_point_with_bullets() {
    let content = "The same guard must be added to every sibling entry point in the family:\n\n- `ci::run`\n- `build::run`\n- `lint::run`\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty(), "expected no violations, got {:?}", v);
}

#[test]
fn scan_passes_inline_colon_list() {
    // Mirrors CLAUDE.md:112 — colon-delimited list inline with
    // the trigger. ≥ 3 backticks after the trigger match.
    let content = "`guard` runs on every subcommand that mutates state: `ci`, `build`, `lint`, `format`, `test`.\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty(), "expected no violations, got {:?}", v);
}

// --- scan: negative (violation surfaced) ---

#[test]
fn scan_fails_unenumerated_universal_claim() {
    let content = "Add the drift guard to every state mutator.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
    assert!(v[0].phrase.to_lowercase().contains("every"));
    assert_eq!(v[0].line, 1);
}

#[test]
fn scan_fails_all_runners_without_list() {
    let content = "Apply FLOW_CI_RUNNING to all runners.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn scan_fails_each_entry_point_without_list() {
    let content = "Gate each CLI entry point with the permission check.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn scan_fails_unrelated_backticks_without_structured_list() {
    // Adversarial finding A4 — two unrelated identifiers near the
    // trigger must NOT satisfy the enumeration heuristic.
    let content = "Make sure `FOO` is set and `BAR` is also respected. Then add the guard to every subcommand.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(
        v.len(),
        1,
        "unrelated backticks should not count as enumeration, got {:?}",
        v
    );
}

#[test]
fn scan_fails_prose_with_backticks_on_previous_line() {
    // Backticks exist on a previous line but NOT in a bullet list.
    // Must still be flagged — prose with inline code is not an
    // enumeration.
    let content =
        "`mutate_state` and `phase_enter` are related.\n\nAdd the guard to every subcommand.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

// --- scan: negation skip ---

#[test]
fn scan_skips_do_not_trace_every() {
    let content = "Do not trace every caller of the function.\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty(), "negation should skip, got {:?}", v);
}

#[test]
fn scan_skips_never_enumerate() {
    let content = "Never enumerate every subcommand manually.\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty());
}

// --- scan: fenced block skip ---

#[test]
fn scan_skips_trigger_inside_fenced_block() {
    let content = "## Heading\n\n```text\nevery state mutator enforces the guard\n```\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty(), "fenced content should skip, got {:?}", v);
}

#[test]
fn scan_skips_trigger_inside_fenced_block_with_language() {
    let content = "## Heading\n\n```rust\n// every subcommand is tested\nfn main() {}\n```\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty(), "fenced content should skip, got {:?}", v);
}

#[test]
fn scan_unterminated_fence_does_not_suppress_violations_below() {
    // Pre-mortem finding F4 — a typo'd open fence with no close used
    // to mask every subsequent line. The fix reverts the mask for
    // unclosed fences so the scan continues.
    let content = "```\nsome code\n\nAdd the guard to every state mutator.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(
        v.len(),
        1,
        "unterminated fence must not silence later violations, got {:?}",
        v
    );
}

// --- scan: opt-out comment skip ---

#[test]
fn scan_skips_open_ended_optout_same_line() {
    let content = "<!-- scope-enumeration: open-ended --> every supported version\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty(), "same-line opt-out should skip, got {:?}", v);
}

#[test]
fn scan_skips_open_ended_optout_preceding_line() {
    let content =
        "<!-- scope-enumeration: open-ended -->\nTest against every supported git version.\n";
    let v = scan(content, &dummy_path());
    assert!(
        v.is_empty(),
        "preceding-line opt-out should skip, got {:?}",
        v
    );
}

#[test]
fn scan_skips_imperative_optout() {
    let content = "<!-- scope-enumeration: imperative -->\nGrep for every caller.\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty());
}

#[test]
fn scan_optout_allows_single_blank_line_gap() {
    // One blank line between the opt-out and the trigger is allowed
    // per the "at most one blank line separating them" contract.
    let content = "<!-- scope-enumeration: imperative -->\n\ngrep for every caller\n";
    let v = scan(content, &dummy_path());
    assert!(
        v.is_empty(),
        "single blank gap should allow opt-out, got {:?}",
        v
    );
}

#[test]
fn scan_optout_rejects_multi_blank_line_gap() {
    // Adversarial finding A5 / Pre-mortem F1 — opt-out followed by
    // multiple blank lines must NOT silence a distant trigger.
    let content =
        "<!-- scope-enumeration: imperative -->\n\n\n\n\nevery subcommand must be gated\n";
    let v = scan(content, &dummy_path());
    assert_eq!(
        v.len(),
        1,
        "opt-out must not chain through multiple blanks, got {:?}",
        v
    );
}

#[test]
fn scan_optout_does_not_leak_to_later_lines() {
    // The opt-out applies to its own line and the next non-blank
    // line only. A later unenumerated claim must still be flagged.
    let content = "<!-- scope-enumeration: open-ended -->\nevery supported git version\n\nAdd guard to every state mutator.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(
        v.len(),
        1,
        "later unenumerated claim should still be flagged, got {:?}",
        v
    );
}

// --- scan: section boundary ---

#[test]
fn scan_does_not_cross_h2_heading_forward() {
    let content = "## A\n\nevery subcommand must enforce this.\n\n## B\n\n- `foo`\n- `bar`\n";
    let v = scan(content, &dummy_path());
    assert_eq!(
        v.len(),
        1,
        "forward enumeration across ## should not count, got {:?}",
        v
    );
}

#[test]
fn scan_does_not_cross_h2_heading_backward() {
    let content = "## A\n\n- `foo`\n- `bar`\n\n## B\n\nAdd to every subcommand.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(
        v.len(),
        1,
        "backward enumeration across ## should not count, got {:?}",
        v
    );
}

#[test]
fn scan_crosses_h3_heading() {
    let content =
        "## Section\n\nevery subcommand must do X\n\n### Subsection\n\n- `foo`\n- `bar`\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty(), "### should not block the window, got {:?}", v);
}

// --- enumeration_present (via scan) ---

#[test]
fn enumeration_present_rejects_single_backtick_nearby() {
    let content = "Add guard to every subcommand in the `foo` module.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1);
}

#[test]
fn enumeration_present_passes_on_inline_three_backticks() {
    let content = "every subcommand has: `foo`, `bar`, `baz`.\n";
    let v = scan(content, &dummy_path());
    assert!(v.is_empty(), "{:?}", v);
}

#[test]
fn enumeration_present_rejects_forward_without_bullet() {
    let content = "Add guard to every subcommand.\n\nThe `foo` and `bar` are callers.\n";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

// --- Window-limit and fenced-inclusion coverage ---

#[test]
fn has_structured_list_forward_stops_at_non_blank_limit() {
    // Twelve non-blank prose lines after the trigger, no bullet — the
    // forward scanner must break at `WINDOW_NON_BLANK_LINES` without
    // finding a bullet, confirming the limit branch fires.
    let mut content = String::from("Add guard to every subcommand.\n");
    for i in 0..12 {
        content.push_str(&format!("plain prose line {}\n", i));
    }
    let v = scan(&content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn has_structured_list_backward_stops_at_non_blank_limit() {
    // Twelve non-blank prose lines BEFORE the trigger, no bullet —
    // the backward scanner must break at `WINDOW_NON_BLANK_LINES`
    // without finding a bullet.
    let mut content = String::new();
    for i in 0..12 {
        content.push_str(&format!("earlier prose line {}\n", i));
    }
    content.push_str("Add guard to every subcommand.\n");
    let v = scan(&content, &dummy_path());
    assert_eq!(v.len(), 1, "{:?}", v);
}

#[test]
fn collect_window_skips_fenced_lines_in_middle_of_window() {
    // Trigger is AFTER a fenced code block. The fenced block lines
    // sit inside the collected window and must be skipped.
    let content = "\
Intro paragraph explaining context.

```bash
# This fenced block sits in the scan window and must be skipped.
echo hello
```

Add guard to every subcommand: `a`, `b`, `c`.
";
    let v = scan(content, &dummy_path());
    assert_eq!(v.len(), 0, "{:?}", v);
}
