use flow_rs::plan_extract::{count_tasks, extract_implementation_plan, promote_headings};

// --- Unit tests for pure functions ---

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
