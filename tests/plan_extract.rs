mod common;

use common::flow_states_dir;
use flow_rs::duplicate_test_coverage::Violation as DupViolation;
use flow_rs::external_input_audit::Violation as AuditViolation;
use flow_rs::plan_extract::{
    complete_plan_phase, count_tasks, count_tasks_any_level, extract_implementation_plan,
    find_heading, gate_check, is_decomposed, is_heading_terminated, load_frozen_config,
    promote_headings, read_dag_mode, violations_response,
};
use flow_rs::scope_enumeration::Violation;
use serde_json::json;
use std::path::PathBuf;

// --- Unit tests for pure functions ---

// (Unit tests are below, integration tests at end of file)

#[test]
fn extract_plan_basic() {
    let body = "## Problem\n\nSomething.\n\n## Implementation Plan\n\n### Context\n\nStuff.\n\n### Tasks\n\n#### Task 1: Do thing\n\n## Files to Investigate\n\n- foo.rs\n";
    let result = extract_implementation_plan(body).unwrap();
    assert!(result.contains("### Context"));
    assert!(result.contains("#### Task 1: Do thing"));
    assert!(!result.contains("## Files to Investigate"));
    assert!(!result.contains("## Problem"));
}

#[test]
fn extract_plan_at_end_of_body() {
    let body = "## Problem\n\nFoo.\n\n## Implementation Plan\n\n### Context\n\nLast section.";
    let result = extract_implementation_plan(body).unwrap();
    assert!(result.contains("### Context"));
    assert!(result.contains("Last section."));
}

#[test]
fn extract_plan_missing() {
    let body = "## Problem\n\nNo plan here.\n\n## Files to Investigate\n\n- bar.rs\n";
    assert!(extract_implementation_plan(body).is_none());
}

#[test]
fn extract_plan_empty_section() {
    let body = "## Implementation Plan\n\n## Files to Investigate\n";
    assert!(extract_implementation_plan(body).is_none());
}

#[test]
fn promote_headings_basic() {
    let content = "### Context\n\nText.\n\n#### Task 1: Do thing\n\nMore text.\n";
    let result = promote_headings(content);
    assert!(result.contains("## Context"));
    assert!(!result.contains("### Context"));
    assert!(result.contains("### Task 1: Do thing"));
    assert!(!result.contains("#### Task 1"));
}

#[test]
fn promote_headings_skips_code_blocks() {
    let content = "### Before\n\n```\n### Inside code block\n#### Also inside\n```\n\n### After\n";
    let result = promote_headings(content);
    assert!(result.contains("## Before"));
    assert!(result.contains("### Inside code block"));
    assert!(result.contains("#### Also inside"));
    assert!(result.contains("## After"));
}

#[test]
fn promote_headings_preserves_h2() {
    // ## should NOT be promoted to # — only ### and #### are promoted
    let content = "## Already H2\n\n### Should become H2\n";
    let result = promote_headings(content);
    assert!(result.contains("## Already H2"));
    // The ### becomes ## too, so we have two ## lines
    let h2_count = result.lines().filter(|l| l.starts_with("## ")).count();
    assert_eq!(h2_count, 2);
}

#[test]
fn promote_headings_fenced_with_language() {
    let content = "### Heading\n\n```rust\n### not a heading\n```\n\n### Another\n";
    let result = promote_headings(content);
    assert!(result.contains("## Heading"));
    assert!(result.contains("### not a heading"));
    assert!(result.contains("## Another"));
}

#[test]
fn count_tasks_basic() {
    let content = "#### Task 1: First\n\nStuff.\n\n#### Task 2: Second\n\nMore.\n";
    assert_eq!(count_tasks(content), 2);
}

#[test]
fn count_tasks_skips_code_blocks() {
    let content = "#### Task 1: Real\n\n```\n#### Task 2: Fake\n```\n\n#### Task 3: Also real\n";
    assert_eq!(count_tasks(content), 2);
}

#[test]
fn count_tasks_zero_when_none() {
    let content = "### Context\n\nNo tasks here.\n";
    assert_eq!(count_tasks(content), 0);
}

#[test]
fn count_tasks_requires_task_prefix() {
    // #### without "Task " should not count
    let content = "#### Something else\n\n#### Task 1: Real\n";
    assert_eq!(count_tasks(content), 1);
}

#[test]
fn extract_plan_ends_at_first_h2() {
    // extract_implementation_plan uses simple find("\n## ") — not code-block-aware.
    // A ## inside a code block within the plan section ends extraction early.
    // This is acceptable because flow-create-issue controls the issue format
    // and does not produce ## headings inside code blocks.
    let body = "## Implementation Plan\n\n### Context\n\n```\n## This is not a heading\n```\n\n### Tasks\n\n## Out of Scope\n";
    let result = extract_implementation_plan(body).unwrap();
    assert!(result.contains("### Context"));
    // Extraction ends at the ## inside the code block (first \n## match)
    assert!(!result.contains("### Tasks"));
}

#[test]
fn extract_plan_rejects_heading_suffix() {
    // "## Implementation Planning" should NOT match — it's a different heading
    let body = "## Implementation Planning\n\n### Context\n\nStuff.\n";
    assert!(extract_implementation_plan(body).is_none());
}

#[test]
fn extract_plan_rejects_heading_extra_words() {
    // "## Implementation Plan Details" should NOT match
    let body = "## Problem\n\nFoo.\n\n## Implementation Plan Details\n\n### Context\n\nStuff.\n";
    assert!(extract_implementation_plan(body).is_none());
}

#[test]
fn extract_plan_rejects_mid_line_heading() {
    // "## Implementation Plan" not at line start should NOT match
    let body = "some text ## Implementation Plan\n\n### Context\n\nStuff.\n";
    assert!(extract_implementation_plan(body).is_none());
}

#[test]
fn extract_plan_matches_at_start_of_body() {
    // Body starting with the heading should match
    let body = "## Implementation Plan\n\n### Context\n\nStuff.\n";
    let result = extract_implementation_plan(body).unwrap();
    assert!(result.contains("### Context"));
}

#[test]
fn extract_plan_matches_after_other_sections() {
    // Heading preceded by \n (after other content) should match
    let body = "## Problem\n\nFoo.\n\n## Implementation Plan\n\n### Context\n\nContent.\n";
    let result = extract_implementation_plan(body).unwrap();
    assert!(result.contains("### Context"));
    assert!(result.contains("Content."));
}

#[test]
fn extract_plan_matches_windows_line_endings() {
    // Heading followed by \r\n should match
    let body =
        "## Problem\r\n\r\nFoo.\r\n\r\n## Implementation Plan\r\n\r\n### Context\r\n\r\nStuff.\r\n";
    let result = extract_implementation_plan(body).unwrap();
    assert!(result.contains("### Context"));
}

#[test]
fn extract_plan_tolerates_trailing_space() {
    // Heading with trailing spaces should still match
    let body = "## Problem\n\nFoo.\n\n## Implementation Plan  \n\n### Context\n\nContent.\n";
    let result = extract_implementation_plan(body).unwrap();
    assert!(result.contains("### Context"));
}

#[test]
fn extract_plan_tolerates_trailing_tab() {
    // Heading with trailing tab should still match
    let body = "## Implementation Plan\t\n\n### Context\n\nStuff.\n";
    let result = extract_implementation_plan(body).unwrap();
    assert!(result.contains("### Context"));
}

#[test]
fn extract_plan_skips_suffix_finds_exact() {
    // First heading has suffix (rejected), second is exact (accepted)
    let body = "## Implementation Planning\n\nIgnore.\n\n## Implementation Plan\n\nReal content.\n";
    let result = extract_implementation_plan(body).unwrap();
    assert!(result.contains("Real content."));
    assert!(!result.contains("Ignore."));
}

#[test]
fn promote_headings_five_hashes_unchanged() {
    // ##### should not be promoted (only ### and #### are)
    let content = "##### Five hashes\n### Three hashes\n";
    let result = promote_headings(content);
    // ##### starts with #### so it gets promoted to ####
    assert!(result.contains("#### Five hashes"));
    assert!(result.contains("## Three hashes"));
}

#[test]
fn count_tasks_ten() {
    let mut content = String::new();
    for i in 1..=10 {
        content.push_str(&format!("#### Task {}: Description {}\n\nBody.\n\n", i, i));
    }
    assert_eq!(count_tasks(&content), 10);
}

// --- violations_response ---

#[test]
fn violations_response_aggregates_all_three_scanners_with_rule_tags() {
    let scope = vec![Violation {
        file: PathBuf::from("/tmp/plan.md"),
        line: 10,
        phrase: "every subcommand".to_string(),
        context: "Add guard to every subcommand.".to_string(),
    }];
    let audit = vec![AuditViolation {
        file: PathBuf::from("/tmp/plan.md"),
        line: 20,
        phrase: "panic on empty".to_string(),
        context: "tighten to panic on empty".to_string(),
    }];
    let dup = vec![DupViolation {
        file: PathBuf::from("/tmp/plan.md"),
        line: 30,
        phrase: "duplicate_name_here".to_string(),
        context: "Plan names `duplicate_name_here` as new.".to_string(),
        existing_test: "test_duplicate_name_here".to_string(),
        existing_file: "tests/hooks.rs:1499".to_string(),
    }];
    let resp = violations_response(&scope, &audit, &dup, "extracted");
    assert_eq!(resp["status"], "error");
    assert_eq!(resp["path"], "extracted");

    let violations = resp["violations"].as_array().expect("array");
    assert_eq!(violations.len(), 3);
    let rules: Vec<String> = violations
        .iter()
        .map(|v| v["rule"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(rules.contains(&"scope-enumeration".to_string()));
    assert!(rules.contains(&"external-input-audit".to_string()));
    assert!(rules.contains(&"duplicate-test-coverage".to_string()));

    let dup_entry = violations
        .iter()
        .find(|v| v["rule"].as_str() == Some("duplicate-test-coverage"))
        .expect("dup entry present");
    assert_eq!(
        dup_entry["existing_test"].as_str(),
        Some("test_duplicate_name_here")
    );
    assert_eq!(
        dup_entry["existing_file"].as_str(),
        Some("tests/hooks.rs:1499")
    );

    let msg = resp["message"].as_str().unwrap_or("");
    assert!(msg.contains("3 plan-check violation"));
    assert!(msg.contains("scope-enumeration.md"));
    assert!(msg.contains("external-input-audit-gate.md"));
    assert!(msg.contains("duplicate-test-coverage.md"));
}

#[test]
fn violations_response_audit_only_omits_other_rule_messages() {
    let scope: Vec<Violation> = vec![];
    let audit = vec![AuditViolation {
        file: PathBuf::from("/tmp/plan.md"),
        line: 5,
        phrase: "panic on empty".to_string(),
        context: "tighten to panic on empty".to_string(),
    }];
    let dup: Vec<DupViolation> = vec![];
    let resp = violations_response(&scope, &audit, &dup, "resumed");
    let msg = resp["message"].as_str().unwrap_or("");
    assert!(msg.contains("external-input-audit-gate.md"));
    assert!(!msg.contains("scope-enumeration.md"));
    assert!(!msg.contains("duplicate-test-coverage.md"));
    assert_eq!(resp["path"], "resumed");
}

#[test]
fn violations_response_duplicate_only_names_only_duplicate_rule() {
    let scope: Vec<Violation> = vec![];
    let audit: Vec<AuditViolation> = vec![];
    let dup = vec![DupViolation {
        file: PathBuf::from("/tmp/plan.md"),
        line: 42,
        phrase: "proposed_dup_name".to_string(),
        context: "Add `proposed_dup_name` as a new test.".to_string(),
        existing_test: "test_proposed_dup_name".to_string(),
        existing_file: "tests/foo.rs:100".to_string(),
    }];
    let resp = violations_response(&scope, &audit, &dup, "extracted");
    let msg = resp["message"].as_str().unwrap_or("");
    assert!(msg.contains("duplicate-test-coverage.md"));
    assert!(!msg.contains("scope-enumeration.md"));
    assert!(!msg.contains("external-input-audit-gate.md"));
}

// --- load_frozen_config ---

#[test]
fn load_frozen_config_with_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let branch = "test-frozen";
    let flow_states = root.join(".flow-states");
    std::fs::create_dir_all(&flow_states).unwrap();
    let frozen_path = flow_states.join(format!("{}-phases.json", branch));
    let frozen_json = json!({
        "order": ["flow-start", "flow-plan"],
        "phases": {
            "flow-start": {"name": "Start", "command": "/flow:flow-start"},
            "flow-plan": {"name": "Plan", "command": "/flow:flow-plan"}
        }
    });
    std::fs::write(&frozen_path, frozen_json.to_string()).unwrap();

    let (order, commands) = load_frozen_config(root, branch);
    assert!(
        order.is_some(),
        "order should be Some when frozen file exists"
    );
    assert!(commands.is_some());
    let order = order.unwrap();
    assert_eq!(order.len(), 2);
    assert_eq!(order[0], "flow-start");
}

#[test]
fn load_frozen_config_without_file_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (order, commands) = load_frozen_config(root, "no-such-branch");
    assert!(order.is_none());
    assert!(commands.is_none());
}

// --- count_tasks_any_level ---

#[test]
fn count_tasks_any_level_skips_code_blocks() {
    let content = "### Task 1: Real task\n\n\
        ```\n\
        ### Task 2: Inside code block\n\
        ```\n\n\
        ### Task 3: Another real task\n";
    assert_eq!(count_tasks_any_level(content), 2);
}

#[test]
fn count_tasks_any_level_counts_both_hash_levels() {
    let content = "### Task 1: h3\n#### Task 2: h4\n### Task 3: h3\n";
    assert_eq!(count_tasks_any_level(content), 3);
}

#[test]
fn count_tasks_any_level_tilde_fence() {
    let content = "### Task 1: Real\n~~~\n### Task 2: Fake\n~~~\n### Task 3: Real\n";
    assert_eq!(count_tasks_any_level(content), 2);
}

// --- is_decomposed ---

#[test]
fn is_decomposed_matches_case_insensitive_lower() {
    let issue = json!({"labels": [{"name": "decomposed"}]});
    assert!(is_decomposed(&issue));
}

#[test]
fn is_decomposed_matches_case_insensitive_mixed() {
    let issue = json!({"labels": [{"name": "Decomposed"}]});
    assert!(is_decomposed(&issue));
}

#[test]
fn is_decomposed_matches_case_insensitive_upper() {
    let issue = json!({"labels": [{"name": "DECOMPOSED"}]});
    assert!(is_decomposed(&issue));
}

#[test]
fn is_decomposed_false_without_label() {
    let issue = json!({"labels": [{"name": "Bug"}, {"name": "Tech Debt"}]});
    assert!(!is_decomposed(&issue));
}

#[test]
fn is_decomposed_false_on_missing_labels_key() {
    let issue = json!({"title": "x"});
    assert!(!is_decomposed(&issue));
}

#[test]
fn is_decomposed_false_on_empty_labels() {
    let issue = json!({"labels": []});
    assert!(!is_decomposed(&issue));
}

#[test]
fn is_decomposed_false_when_labels_not_array() {
    let issue = json!({"labels": "not an array"});
    assert!(!is_decomposed(&issue));
}

#[test]
fn is_decomposed_false_when_label_name_missing() {
    let issue = json!({"labels": [{"color": "red"}]});
    assert!(!is_decomposed(&issue));
}

// --- read_dag_mode ---

#[test]
fn read_dag_mode_default_when_missing() {
    let state = json!({});
    assert_eq!(read_dag_mode(&state), "auto");
}

#[test]
fn read_dag_mode_explicit_never() {
    let state = json!({"skills": {"flow-plan": {"dag": "never"}}});
    assert_eq!(read_dag_mode(&state), "never");
}

#[test]
fn read_dag_mode_explicit_always() {
    let state = json!({"skills": {"flow-plan": {"dag": "always"}}});
    assert_eq!(read_dag_mode(&state), "always");
}

#[test]
fn read_dag_mode_empty_string_falls_back_to_default() {
    let state = json!({"skills": {"flow-plan": {"dag": ""}}});
    assert_eq!(read_dag_mode(&state), "auto");
}

#[test]
fn read_dag_mode_non_string_falls_back_to_default() {
    let state = json!({"skills": {"flow-plan": {"dag": 42}}});
    assert_eq!(read_dag_mode(&state), "auto");
}

// --- is_heading_terminated ---

#[test]
fn is_heading_terminated_accepts_empty() {
    assert!(is_heading_terminated(""));
}

#[test]
fn is_heading_terminated_accepts_lf() {
    assert!(is_heading_terminated("\n"));
}

#[test]
fn is_heading_terminated_accepts_cr() {
    assert!(is_heading_terminated("\r"));
}

#[test]
fn is_heading_terminated_accepts_trailing_space_lf() {
    assert!(is_heading_terminated("   \n"));
}

#[test]
fn is_heading_terminated_accepts_trailing_tab_lf() {
    assert!(is_heading_terminated("\t\n"));
}

#[test]
fn is_heading_terminated_accepts_only_whitespace() {
    assert!(is_heading_terminated("   "));
}

#[test]
fn is_heading_terminated_rejects_text_with_leading_space() {
    assert!(!is_heading_terminated(" foo"));
}

#[test]
fn is_heading_terminated_rejects_inline_text() {
    assert!(!is_heading_terminated("x"));
}

// --- find_heading ---

#[test]
fn find_heading_at_start() {
    let body = "## Implementation Plan\n\ncontent";
    assert_eq!(find_heading(body, "## Implementation Plan"), Some(0));
}

#[test]
fn find_heading_after_prose() {
    let body = "# Title\n\n## Implementation Plan\n\nbody";
    assert_eq!(find_heading(body, "## Implementation Plan"), Some(9));
}

#[test]
fn find_heading_not_found_when_inline() {
    let body = "Some text with ## Implementation Plan inline.";
    assert_eq!(find_heading(body, "## Implementation Plan"), None);
}

#[test]
fn find_heading_not_found_when_absent() {
    let body = "# Title\n\n## Context\n\n## Tasks\n";
    assert_eq!(find_heading(body, "## Implementation Plan"), None);
}

#[test]
fn find_heading_rejects_start_prefix_then_finds_exact() {
    // Body starts with "## Implementation Planning" (not exact); the
    // real match appears after a newline. strip_prefix matches but
    // `is_heading_terminated` rejects the suffix, so the search
    // continues via the `\n<heading>` loop.
    let body = "## Implementation Planning\n\n## Implementation Plan\n\nbody";
    let pos = find_heading(body, "## Implementation Plan").unwrap();
    assert!(pos > 0);
}

#[test]
fn find_heading_iterates_past_inline_match_to_exact() {
    // First candidate after \n is "## Implementation Planx" (not
    // terminated). The loop advances and finds the next candidate
    // which is a real match.
    let body = "## Context\n## Implementation Planx\n## Implementation Plan\n\nbody";
    let pos = find_heading(body, "## Implementation Plan").unwrap();
    assert!(pos > 20);
}

// --- gate_check ---

#[test]
fn gate_check_passes_when_start_complete() {
    let state = json!({"phases": {"flow-start": {"status": "complete"}}});
    assert!(gate_check(&state).is_ok());
}

#[test]
fn gate_check_fails_when_start_incomplete() {
    let state = json!({"phases": {"flow-start": {"status": "in_progress"}}});
    let err = gate_check(&state).unwrap_err();
    assert_eq!(err["status"], "error");
    assert!(err["message"].as_str().unwrap().contains("flow-start"));
}

#[test]
fn gate_check_fails_when_status_missing() {
    let state = json!({"phases": {"flow-start": {}}});
    assert!(gate_check(&state).is_err());
}

#[test]
fn gate_check_fails_when_phases_missing() {
    let state = json!({});
    assert!(gate_check(&state).is_err());
}

// --- complete_plan_phase ---

#[test]
fn complete_plan_phase_returns_err_on_missing_state() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let state_path = root.join(".flow-states").join("nonexistent.json");
    let result = complete_plan_phase(&state_path, root, "nonexistent");
    assert!(result.is_err(), "expected Err when state file is missing");
    let err = result.unwrap_err();
    assert!(
        err.contains("Failed to complete phase"),
        "expected map_err message, got: {}",
        err
    );
}

// --- extract_implementation_plan: empty-section branch ---

#[test]
fn extract_implementation_plan_none_when_empty_section_between_h2() {
    let body = "## Implementation Plan\n\n## Next section\n";
    assert_eq!(extract_implementation_plan(body), None);
}

#[test]
fn extract_implementation_plan_runs_to_eof_when_no_next_heading() {
    let body = "## Implementation Plan\n\ntail content only\n";
    let extracted = extract_implementation_plan(body).unwrap();
    assert!(extracted.contains("tail content only"));
}

// --- Integration tests for run_impl (via subprocess) ---

mod integration {
    use std::fs;
    use std::process::Command;

    use super::flow_states_dir;

    fn setup_git_repo(dir: &std::path::Path, branch: &str) {
        Command::new("git")
            .args(["-c", "init.defaultBranch=main", "init"])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "commit.gpgsign", "false"])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["checkout", "-b", branch])
            .current_dir(dir)
            .output()
            .unwrap();
    }

    fn setup_state(dir: &std::path::Path, branch: &str, state_json: &str) {
        let state_dir = flow_states_dir(dir);
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(state_dir.join(format!("{}.json", branch)), state_json).unwrap();
    }

    /// Build a plan-extract-ready state JSON with flow-start complete.
    /// `prompt` controls the prompt field (determines issue detection).
    /// `extra` is a closure that can mutate the state Value before serialization.
    fn make_plan_state(prompt: &str, extra: impl FnOnce(&mut serde_json::Value)) -> String {
        let mut state = serde_json::json!({
            "branch": "test-feature",
            "current_phase": "flow-start",
            "prompt": prompt,
            "files": {
                "plan": serde_json::Value::Null,
                "dag": serde_json::Value::Null,
            },
            "skills": {
                "flow-plan": {
                    "continue": "auto",
                    "dag": "auto",
                }
            },
            "phases": {
                "flow-start": {
                    "name": "Start",
                    "status": "complete",
                    "started_at": "2026-01-01T00:00:00-08:00",
                    "completed_at": "2026-01-01T00:01:00-08:00",
                    "session_started_at": serde_json::Value::Null,
                    "cumulative_seconds": 60,
                    "visit_count": 1
                },
                "flow-plan": {
                    "name": "Plan",
                    "status": "pending",
                    "started_at": serde_json::Value::Null,
                    "completed_at": serde_json::Value::Null,
                    "session_started_at": serde_json::Value::Null,
                    "cumulative_seconds": 0,
                    "visit_count": 0
                },
                "flow-code": {
                    "name": "Code",
                    "status": "pending",
                    "started_at": serde_json::Value::Null,
                    "completed_at": serde_json::Value::Null,
                    "session_started_at": serde_json::Value::Null,
                    "cumulative_seconds": 0,
                    "visit_count": 0
                },
                "flow-code-review": {
                    "name": "Code Review",
                    "status": "pending",
                    "started_at": serde_json::Value::Null,
                    "completed_at": serde_json::Value::Null,
                    "session_started_at": serde_json::Value::Null,
                    "cumulative_seconds": 0,
                    "visit_count": 0
                },
                "flow-learn": {
                    "name": "Learn",
                    "status": "pending",
                    "started_at": serde_json::Value::Null,
                    "completed_at": serde_json::Value::Null,
                    "session_started_at": serde_json::Value::Null,
                    "cumulative_seconds": 0,
                    "visit_count": 0
                },
                "flow-complete": {
                    "name": "Complete",
                    "status": "pending",
                    "started_at": serde_json::Value::Null,
                    "completed_at": serde_json::Value::Null,
                    "session_started_at": serde_json::Value::Null,
                    "cumulative_seconds": 0,
                    "visit_count": 0
                }
            },
            "phase_transitions": []
        });
        extra(&mut state);
        state.to_string()
    }

    /// Run `flow-rs plan-extract` in the given directory.
    /// Returns (exit_code, parsed_json).
    fn run_plan_extract(dir: &std::path::Path, extra_args: &[&str]) -> (i32, serde_json::Value) {
        run_plan_extract_inner(dir, extra_args, None)
    }

    /// Run `flow-rs plan-extract` with a gh stub on PATH.
    fn run_plan_extract_with_gh(
        dir: &std::path::Path,
        extra_args: &[&str],
        stub_dir: &std::path::Path,
    ) -> (i32, serde_json::Value) {
        run_plan_extract_inner(dir, extra_args, Some(stub_dir))
    }

    fn run_plan_extract_inner(
        dir: &std::path::Path,
        extra_args: &[&str],
        stub_dir: Option<&std::path::Path>,
    ) -> (i32, serde_json::Value) {
        let mut args = vec!["plan-extract"];
        args.extend_from_slice(extra_args);

        let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
        cmd.args(&args).current_dir(dir);

        if let Some(sd) = stub_dir {
            let path_env = format!(
                "{}:{}",
                sd.to_string_lossy(),
                std::env::var("PATH").unwrap_or_default()
            );
            cmd.env("PATH", &path_env);
        }

        let output = cmd.output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let code = output.status.code().unwrap_or(-1);
        let json: serde_json::Value = serde_json::from_str(stdout.trim())
            .unwrap_or(serde_json::json!({"raw": stdout.trim()}));
        (code, json)
    }

    /// Create a gh stub script. Returns the stub directory.
    fn create_gh_stub(dir: &std::path::Path, script: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let stub_dir = dir.join(".stub-bin");
        fs::create_dir_all(&stub_dir).unwrap();
        let gh_stub = stub_dir.join("gh");
        fs::write(&gh_stub, script).unwrap();
        fs::set_permissions(&gh_stub, fs::Permissions::from_mode(0o755)).unwrap();
        stub_dir
    }

    // --- Error path tests ---

    #[test]
    fn test_error_no_state_file() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");
        // No state file created

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "error");
        assert!(
            json["message"].as_str().unwrap().contains("No state file"),
            "Expected 'No state file' error, got: {}",
            json["message"]
        );
    }

    #[test]
    fn test_error_corrupt_json() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state_dir = flow_states_dir(dir.path());
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(state_dir.join("test-feature.json"), "{bad json").unwrap();

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 1);
        assert_eq!(json["status"], "error");
        assert!(
            json["message"].as_str().unwrap().contains("Invalid JSON"),
            "Expected 'Invalid JSON' error, got: {}",
            json["message"]
        );
    }

    #[test]
    fn test_error_gate_start_not_complete() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        // State with flow-start still pending
        let state = make_plan_state("build a thing", |s| {
            s["phases"]["flow-start"]["status"] = serde_json::json!("pending");
        });
        setup_state(dir.path(), "test-feature", &state);

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "error");
        assert!(
            json["message"]
                .as_str()
                .unwrap()
                .contains("Phase 1: Start must be complete"),
            "Expected gate failure message, got: {}",
            json["message"]
        );
    }

    // --- Standard path tests ---

    #[test]
    fn test_standard_no_issue_refs() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("build a feature", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["path"], "standard");
        assert!(
            json["issue_body"].is_null(),
            "issue_body should be null for no issue refs"
        );
        assert!(
            json["issue_number"].is_null(),
            "issue_number should be null for no issue refs"
        );
        assert_eq!(json["dag_mode"], "auto");
    }

    #[test]
    fn test_standard_dag_mode_from_state() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("build a feature", |s| {
            s["skills"]["flow-plan"]["dag"] = serde_json::json!("never");
        });
        setup_state(dir.path(), "test-feature", &state);

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["path"], "standard");
        assert_eq!(
            json["dag_mode"], "never",
            "dag_mode should reflect the state file's skills.flow-plan.dag value"
        );
    }

    // --- Resumed path test ---

    #[test]
    fn test_resumed_plan_exists() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let plan_content = "## Context\n\nTest plan.\n\n## Tasks\n\n### Task 1: Do something\n";
        let plan_rel = ".flow-states/test-feature-plan.md";

        // State with files.plan set (creates .flow-states/ directory)
        let state = make_plan_state("build a feature", |s| {
            s["files"]["plan"] = serde_json::json!(plan_rel);
        });
        setup_state(dir.path(), "test-feature", &state);

        // Write the plan file (after .flow-states/ exists)
        let plan_abs = dir.path().join(plan_rel);
        fs::write(&plan_abs, plan_content).unwrap();

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["path"], "resumed");
        assert_eq!(
            json["plan_content"].as_str().unwrap(),
            plan_content,
            "plan_content should match the file on disk"
        );
        assert_eq!(json["plan_file"], plan_rel);
        assert!(
            json["formatted_time"].is_string(),
            "formatted_time must be present"
        );
        assert!(
            json["continue_action"].is_string(),
            "continue_action must be present"
        );

        // Verify state file was updated: flow-plan should be complete
        let updated_state: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(flow_states_dir(dir.path()).join("test-feature.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            updated_state["phases"]["flow-plan"]["status"], "complete",
            "flow-plan should be marked complete after resumed path"
        );
    }

    #[test]
    fn test_resumed_missing_plan_file_does_not_corrupt_state() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let plan_rel = ".flow-states/test-feature-plan.md";

        // State with files.plan set but NO plan file on disk
        let state = make_plan_state("build a feature", |s| {
            s["files"]["plan"] = serde_json::json!(plan_rel);
        });
        setup_state(dir.path(), "test-feature", &state);

        // Deliberately do NOT create the plan file

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 1, "Should exit 1 when plan file is missing");
        assert_eq!(
            json["status"], "error",
            "error path should set status=error"
        );
        assert!(
            json["message"]
                .as_str()
                .unwrap_or("")
                .contains("Could not read plan file"),
            "Expected 'Could not read plan file' error, got: {}",
            json
        );

        // Critical: state file must NOT be corrupted with "complete"
        let updated_state: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(flow_states_dir(dir.path()).join("test-feature.json")).unwrap(),
        )
        .unwrap();
        assert_ne!(
            updated_state["phases"]["flow-plan"]["status"], "complete",
            "flow-plan must NOT be marked complete when plan file is missing"
        );
    }

    // --- gh-dependent tests ---

    #[test]
    fn test_standard_issue_not_decomposed() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("fix issue #42", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        // gh stub returns issue without Decomposed label
        let stub_dir = create_gh_stub(
            dir.path(),
            r#"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":42,"title":"Fix the bug","body":"Something is broken.","labels":[]}'
    exit 0
fi
exit 1
"#,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["path"], "standard");
        assert_eq!(json["issue_number"], 42);
        assert_eq!(json["issue_body"].as_str().unwrap(), "Something is broken.");
    }

    #[test]
    fn test_standard_decomposed_no_plan_section() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("work on #99", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        // gh stub returns decomposed issue WITHOUT ## Implementation Plan
        // Uses echo (not printf) so \n stays literal in JSON output
        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":99,"title":"Refactor auth","body":"## Problem\n\nAuth is slow.","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
exit 1
"###,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(
            json["path"], "standard",
            "Decomposed issue without Implementation Plan should return standard path"
        );
        assert_eq!(json["issue_number"], 99);

        // DAG file should have been created
        let dag_path = flow_states_dir(dir.path()).join("test-feature-dag.md");
        assert!(
            dag_path.exists(),
            "DAG file should be created for decomposed issues"
        );
        let dag_content = fs::read_to_string(&dag_path).unwrap();
        assert!(dag_content.contains("# Pre-Decomposed Analysis: Refactor auth"));
    }

    #[test]
    fn test_extracted_decomposed_with_plan() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("work on #100", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        // gh stub returns decomposed issue WITH ## Implementation Plan and tasks
        // Uses echo (not printf) so \n stays literal in JSON output
        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":100,"title":"Add tests","body":"## Problem\n\nNeed tests.\n\n## Implementation Plan\n\n### Context\n\nWe need integration tests.\n\n### Tasks\n\n#### Task 1: Write helpers\n\nAdd test helpers.\n\n#### Task 2: Write tests\n\nAdd actual tests.\n\n## Files to Investigate\n\n- src/plan_extract.rs","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
exit 1
"###,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["path"], "extracted");
        assert!(
            json["plan_content"].as_str().unwrap().contains("Context"),
            "plan_content should contain promoted headings"
        );
        assert_eq!(json["task_count"], 2);
        assert!(json["formatted_time"].is_string());
        assert!(json["continue_action"].is_string());

        // Verify DAG and plan files created on disk
        let dag_path = flow_states_dir(dir.path()).join("test-feature-dag.md");
        assert!(dag_path.exists(), "DAG file should exist");

        let plan_path = flow_states_dir(dir.path()).join("test-feature-plan.md");
        assert!(plan_path.exists(), "Plan file should exist");

        // Verify state file shows flow-plan complete
        let updated_state: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(flow_states_dir(dir.path()).join("test-feature.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            updated_state["phases"]["flow-plan"]["status"], "complete",
            "flow-plan should be complete after extracted path"
        );
    }

    // --- scope-enumeration gate (issue #1033) ---

    #[test]
    fn plan_extract_returns_error_on_unenumerated_plan() {
        // A pre-planned issue with universal-coverage prose but no
        // named enumeration must fail the extracted path before
        // `complete_plan_phase` runs. The plan file is still written
        // to disk so the model can edit it and re-run.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("work on #101", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        // Implementation Plan contains "every state mutator" with no
        // adjacent named list — the scope-enumeration scanner flags it.
        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":101,"title":"Add drift guard","body":"## Problem\n\nGuard is missing.\n\n## Implementation Plan\n\n### Context\n\nAdd the drift guard to every state mutator.\n\n### Tasks\n\n#### Task 1: Add guard\n\nImplement.\n\n## Files to Investigate\n\n- src/lib.rs","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
exit 1
"###,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0, "business errors exit 0, got {}", json);
        assert_eq!(json["status"], "error");
        assert_eq!(json["path"], "extracted");
        let violations = json["violations"]
            .as_array()
            .expect("violations[] expected");
        assert!(!violations.is_empty(), "expected at least one violation");
        assert!(
            violations[0]["phrase"]
                .as_str()
                .unwrap()
                .to_lowercase()
                .contains("every"),
            "phrase should contain the trigger, got {}",
            violations[0]
        );

        // Plan file MUST exist on disk so the user can edit it in place.
        let plan_path = flow_states_dir(dir.path()).join("test-feature-plan.md");
        assert!(
            plan_path.exists(),
            "plan file must be written to disk even on violation"
        );

        // Phase must NOT be marked complete.
        let updated_state: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(flow_states_dir(dir.path()).join("test-feature.json")).unwrap(),
        )
        .unwrap();
        assert_ne!(
            updated_state["phases"]["flow-plan"]["status"], "complete",
            "flow-plan must not be marked complete when violations are found"
        );

        // files.plan must be set so the next invocation takes the
        // resume path (which re-scans the file the user edited).
        assert_eq!(
            updated_state["files"]["plan"].as_str().unwrap(),
            ".flow-states/test-feature-plan.md",
            "files.plan must be set so resume path can pick up the edited file"
        );
    }

    #[test]
    fn plan_extract_resume_gates_on_scope_enumeration() {
        // When a plan file already exists on disk with a violation,
        // the resume path must return an error without completing
        // the phase. Mirrors the "user fixed the extracted-path
        // error, re-ran plan-extract" path.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let plan_content = "## Context\n\nAdd the drift guard to every state mutator.\n";
        let plan_rel = ".flow-states/test-feature-plan.md";

        let state = make_plan_state("build a feature", |s| {
            s["files"]["plan"] = serde_json::json!(plan_rel);
        });
        setup_state(dir.path(), "test-feature", &state);

        let plan_abs = dir.path().join(plan_rel);
        fs::write(&plan_abs, plan_content).unwrap();

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "error");
        assert_eq!(json["path"], "resumed");
        assert!(
            json["violations"].is_array(),
            "violations[] expected, got {}",
            json
        );

        // Phase must not be marked complete.
        let updated_state: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(flow_states_dir(dir.path()).join("test-feature.json")).unwrap(),
        )
        .unwrap();
        assert_ne!(
            updated_state["phases"]["flow-plan"]["status"], "complete",
            "flow-plan must not be marked complete on resume-path violation"
        );
    }

    #[test]
    fn plan_extract_resume_gates_on_external_input_audit() {
        // A plan file on disk with a panic-tightening proposal but no
        // paired callsite source-classification table must fail the
        // resume-path external-input-audit scanner.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let plan_content = "## Approach\n\n\
            Tighten the existing FlowPaths::new to panic on empty branches.\n";
        let plan_rel = ".flow-states/test-feature-plan.md";

        let state = make_plan_state("build a feature", |s| {
            s["files"]["plan"] = serde_json::json!(plan_rel);
        });
        setup_state(dir.path(), "test-feature", &state);

        let plan_abs = dir.path().join(plan_rel);
        fs::write(&plan_abs, plan_content).unwrap();

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "error");
        assert_eq!(json["path"], "resumed");
        let violations = json["violations"]
            .as_array()
            .expect("violations[] expected");
        assert!(!violations.is_empty(), "expected audit violation");
        let has_audit = violations
            .iter()
            .any(|v| v["rule"].as_str() == Some("external-input-audit"));
        assert!(
            has_audit,
            "resume-path audit scanner should flag tighten+panic without table, got: {}",
            json
        );
    }

    #[test]
    fn plan_extract_resume_runs_to_eof_when_no_next_heading_in_promoted() {
        // Resume-path variant covering the case where the plan file
        // ends immediately after the promoted tasks without a trailing
        // ## heading. Exercises the "run to end of string" branch in
        // the extraction parsing pipeline when invoked from resume.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let plan_content = "## Context\n\n\
            Apply the guard at the five specific sites\n\
            (`site_a`, `site_b`, `site_c`, `site_d`, `site_e`).\n\n\
            ## Tasks\n\n\
            ### Task 1: Add guard at site_a\n";
        let plan_rel = ".flow-states/test-feature-plan.md";

        let state = make_plan_state("build a feature", |s| {
            s["files"]["plan"] = serde_json::json!(plan_rel);
        });
        setup_state(dir.path(), "test-feature", &state);

        let plan_abs = dir.path().join(plan_rel);
        fs::write(&plan_abs, plan_content).unwrap();

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["path"], "resumed");
        assert_eq!(
            json["plan_content"].as_str().unwrap(),
            plan_content,
            "plan_content on resume must match the file exactly"
        );
    }

    #[test]
    fn plan_extract_resume_passes_enumerated_plan() {
        // The resume path must allow completion when the plan has
        // been fixed to include a named enumeration. Simulates the
        // user editing the plan file after seeing a violation on
        // the prior run.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let plan_content = "## Context\n\n\
            Add the drift guard to every state mutator \
            (`phase-enter`, `phase-finalize`, `phase-transition`, \
            `set-timestamp`, `add-finding`).\n";
        let plan_rel = ".flow-states/test-feature-plan.md";

        let state = make_plan_state("build a feature", |s| {
            s["files"]["plan"] = serde_json::json!(plan_rel);
        });
        setup_state(dir.path(), "test-feature", &state);

        let plan_abs = dir.path().join(plan_rel);
        fs::write(&plan_abs, plan_content).unwrap();

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["path"], "resumed");

        let updated_state: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(flow_states_dir(dir.path()).join("test-feature.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            updated_state["phases"]["flow-plan"]["status"], "complete",
            "flow-plan should complete when enumerated plan passes the gate"
        );
    }
}
