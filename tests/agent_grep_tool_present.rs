//! Frontmatter contract test: every Review-tier agent that consumes
//! a diff via file handoff (`DIFF_FILE` / `SUBSTANTIVE_DIFF_FILE`)
//! must declare `Grep` in its `tools:` allow-list so the agent can
//! search the codebase for context.
//!
//! Runtime verification of the actual Grep tool dispatch from inside
//! a sub-agent is deferred per
//! `.claude/rules/verify-automation-e2e.md` "bootstrapping carve-out":
//! the test would require running `/flow:flow-review` against a target
//! project from inside an active FLOW session, which conflicts with
//! the current session's state file, lock, and worktree. The deferral
//! is recorded via `bin/flow log` and in the commit message body so a
//! cold-start session can pick up the post-merge verification step.

mod common;

use common::read_agent;

/// The four Review-phase sub-agents that receive the diff via file
/// handoff. Each must keep `Grep` in its `tools:` allow-list so the
/// Read-the-file-then-investigate workflow described in the agent's
/// Input section can actually run.
const REVIEW_AGENTS: &[&str] = &[
    "reviewer.md",
    "pre-mortem.md",
    "adversarial.md",
    "documentation.md",
];

/// Extract the `tools:` value from a markdown file's YAML frontmatter.
///
/// Returns `Some(value)` with the trimmed string after `tools:` when
/// the file's frontmatter contains the field, `None` otherwise. The
/// helper does NOT parse the comma-separated list — callers test for
/// the literal `Grep` token so a reordering or a switch from
/// comma-separated string to YAML list both pass as long as the token
/// is present.
fn frontmatter_tools(content: &str) -> Option<String> {
    // Walk to the second `---` delimiter before deciding. Returning
    // early on the first `tools:` line would let a body line that
    // happens to start with `tools:` masquerade as a frontmatter
    // value when the frontmatter has no closing delimiter
    // (malformed file). The contract: only return Some when the
    // file has a well-formed frontmatter block AND a `tools:` line
    // inside it.
    let mut in_frontmatter = false;
    let mut saw_close = false;
    let mut found: Option<String> = None;
    for line in content.lines() {
        if line.trim() == "---" {
            if !in_frontmatter {
                in_frontmatter = true;
            } else {
                saw_close = true;
                break;
            }
            continue;
        }
        if in_frontmatter && found.is_none() {
            if let Some(rest) = line.strip_prefix("tools:") {
                found = Some(rest.trim().to_string());
            }
        }
    }
    if !saw_close {
        return None;
    }
    found
}

#[test]
fn review_agents_frontmatter_declares_grep() {
    for agent_file in REVIEW_AGENTS {
        let content = read_agent(agent_file);
        let tools = frontmatter_tools(&content).unwrap_or_else(|| {
            panic!(
                "agents/{} must declare a `tools:` field in its YAML frontmatter",
                agent_file
            )
        });
        // The presence test is intentionally a substring check rather
        // than a tokenization: `Grep` is a fixed proper-noun tool name
        // (cannot legitimately appear as a substring of another tool)
        // so the substring check is precise.
        assert!(
            tools.contains("Grep"),
            "agents/{} must declare Grep in `tools:` so the diff-file workflow can search the codebase; got: {:?}",
            agent_file,
            tools
        );
    }
}

// --- frontmatter_tools helper coverage ---

#[test]
fn frontmatter_tools_returns_none_when_no_frontmatter() {
    // Files without a leading `---` block must not surface a phantom
    // tools value — the helper distinguishes "frontmatter absent" from
    // "frontmatter present without tools".
    let content = "# Heading\n\nBody text without YAML frontmatter.\n";
    assert!(frontmatter_tools(content).is_none());
}

#[test]
fn frontmatter_tools_returns_none_when_tools_absent_from_frontmatter() {
    let content = "---\nname: agent\nmodel: sonnet\n---\n\nBody.";
    assert!(frontmatter_tools(content).is_none());
}

#[test]
fn frontmatter_tools_does_not_match_tools_outside_frontmatter() {
    // A `tools:` line in the body must not be promoted to the
    // frontmatter value; the helper closes the scan at the second
    // `---` delimiter.
    let content = "---\nname: agent\n---\n\nProse body that happens to say `tools: Read` later.\n";
    assert!(frontmatter_tools(content).is_none());
}

#[test]
fn frontmatter_tools_returns_value_when_field_present() {
    let content = "---\nname: agent\ntools: Read, Grep, Bash\nmodel: sonnet\n---\n\nBody.";
    assert_eq!(
        frontmatter_tools(content),
        Some("Read, Grep, Bash".to_string())
    );
}

#[test]
fn frontmatter_tools_returns_none_when_closing_delimiter_missing() {
    // Malformed file: opening `---` but no closing `---`. A
    // body-line `tools: Read` must not be promoted to a frontmatter
    // value just because the file scanner found it while still in
    // `in_frontmatter` state. The closing delimiter is the structural
    // boundary the contract depends on.
    let content = "---\nname: agent\ntools: Read, Grep, Bash\nstill in pseudo-frontmatter\n";
    assert_eq!(frontmatter_tools(content), None);
}
