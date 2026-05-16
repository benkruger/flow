//! Integration tests for `src/hooks/validate_claude_paths.rs`.
//!
//! `is_protected_path` tests live at tests/protected_paths.rs (mirroring
//! src/protected_paths.rs) — only the hook-specific `validate` and
//! `run_impl_main` surface is exercised here.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use flow_rs::hooks::validate_claude_paths::{run_impl_main, validate};

// --- validate tests ---

#[test]
fn test_blocks_claude_rules_when_flow_active() {
    let (allowed, msg) = validate("/project/.claude/rules/foo.md", true, "Edit");
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("write-rule"));
}

#[test]
fn test_blocks_claude_md_when_flow_active() {
    let (allowed, msg) = validate("/project/CLAUDE.md", true, "Edit");
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("write-rule"));
}

#[test]
fn test_allows_claude_rules_when_no_flow() {
    let (allowed, msg) = validate("/project/.claude/rules/foo.md", false, "Edit");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_claude_md_when_no_flow() {
    let (allowed, msg) = validate("/project/CLAUDE.md", false, "Edit");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_unrelated_path_when_flow_active() {
    let (allowed, msg) = validate("/project/lib/foo.py", true, "Edit");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_claude_settings_when_flow_active() {
    let (allowed, msg) = validate("/project/.claude/settings.json", true, "Edit");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_flow_states_path() {
    let (allowed, msg) = validate("/project/.flow-states/branch-rule-content.md", true, "Edit");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_empty_path() {
    let (allowed, msg) = validate("", true, "Edit");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_blocks_nested_claude_rules() {
    let (allowed, msg) = validate("/project/.claude/rules/subdir/deep.md", true, "Edit");
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_worktree_claude_rules() {
    let (allowed, msg) = validate(
        "/project/.worktrees/feat/.claude/rules/foo.md",
        true,
        "Edit",
    );
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_worktree_claude_md() {
    let (allowed, msg) = validate("/project/.worktrees/feat/CLAUDE.md", true, "Edit");
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_claude_skills_when_flow_active() {
    let (allowed, msg) = validate("/project/.claude/skills/foo/SKILL.md", true, "Edit");
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("write-rule"));
}

#[test]
fn test_blocks_nested_claude_skills() {
    let (allowed, msg) = validate("/project/.claude/skills/subdir/deep/SKILL.md", true, "Edit");
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_worktree_claude_skills() {
    let (allowed, msg) = validate(
        "/project/.worktrees/feat/.claude/skills/foo/SKILL.md",
        true,
        "Edit",
    );
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_allows_claude_skills_when_no_flow() {
    let (allowed, msg) = validate("/project/.claude/skills/foo/SKILL.md", false, "Edit");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_claude_settings_local() {
    let (allowed, _) = validate("/project/.claude/settings.local.json", true, "Edit");
    assert!(allowed);
}

#[test]
fn test_error_message_mentions_write_rule() {
    let (_, msg) = validate("/project/.claude/rules/foo.md", true, "Edit");
    assert!(msg.contains("write-rule"));
    assert!(msg.contains("--path"));
    assert!(msg.contains("--content-file"));
}

// --- ~/.claude/projects/ transcript path block ---
//
// The block fires regardless of flow_active because transcript
// tampering can subvert validate-skill's user-only block.

#[test]
fn validate_claude_paths_blocks_edit_in_claude_projects() {
    let (allowed, msg) = validate(
        "/Users/ben/.claude/projects/abc/session.jsonl",
        true,
        "Edit",
    );
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("transcript"));
}

#[test]
fn validate_claude_paths_blocks_write_in_claude_projects() {
    // The transcript-root block fires for any tool that can address
    // the path. Exercise Write explicitly so the test's name matches
    // what it covers; per `.claude/rules/testing-gotchas.md` "Message
    // Content Assertions — Per Variant, Not Just Presence."
    let (allowed, msg) = validate(
        "/Users/ben/.claude/projects/abc/session.jsonl",
        true,
        "Write",
    );
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn validate_claude_paths_blocks_read_in_claude_projects() {
    // Read of the transcript root is blocked (post-PR semantics).
    // Internal walkers use fs::read_to_string from Rust, not the
    // Read tool, so they remain unaffected.
    let (allowed, msg) = validate(
        "/Users/ben/.claude/projects/abc/session.jsonl",
        true,
        "Read",
    );
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("transcript"));
}

#[test]
fn validate_claude_paths_blocks_glob_in_claude_projects() {
    // Glob on the transcript root is blocked — same rationale as
    // Read. Without this gate, a model could enumerate transcript
    // file paths even though it can't Read them.
    let (allowed, msg) = validate(
        "/Users/ben/.claude/projects/abc/session.jsonl",
        true,
        "Glob",
    );
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn validate_claude_paths_blocks_grep_in_claude_projects() {
    // Grep on the transcript root is blocked — Grep with
    // output_mode=content reads file bytes, equivalent to Read for
    // the threat model.
    let (allowed, msg) = validate(
        "/Users/ben/.claude/projects/abc/session.jsonl",
        true,
        "Grep",
    );
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn validate_claude_paths_allows_memory_subdirectory_read() {
    // The auto-memory carve-out preserves Read access to
    // `~/.claude/projects/<project-id>/memory/MEMORY.md`. The
    // UNIVERSAL_ALLOW pattern Read(~/.claude/projects/*/memory/*)
    // documents this boundary at the settings layer; the hook
    // honors the same boundary so user memory continues to work.
    let (allowed, msg) = validate(
        "/Users/ben/.claude/projects/proj-id/memory/MEMORY.md",
        true,
        "Read",
    );
    assert!(allowed, "memory subdirectory must allow Read; msg: {}", msg);
}

#[test]
fn validate_claude_paths_blocks_memory_subdirectory_edit_when_flow_active() {
    // Memory carve-out preserves Read but the path is still under
    // `.claude/`. Edit on memory files during an active flow falls
    // through transcript-root check (carved out) and protected-path
    // check (the path isn't .claude/rules/ or .claude/skills/ — it's
    // .claude/projects/.../memory/). So Edit on memory is allowed
    // when no flow is active, and falls through cleanly during a
    // flow because is_protected_path doesn't match memory paths.
    let (allowed, _) = validate(
        "/Users/ben/.claude/projects/proj-id/memory/MEMORY.md",
        true,
        "Edit",
    );
    assert!(
        allowed,
        "memory paths fall outside the protected-path classifier"
    );
}

#[test]
fn validate_unrecognized_tool_name_blocks_protected_path() {
    // Per the doc comment's fail-closed contract: an unrecognized
    // tool_name falls into the mutating-tool block class. Empty
    // tool_name and future tool names that aren't Read/Glob/Grep
    // must trigger the protected-path block.
    let (allowed, msg) = validate("/project/.claude/rules/foo.md", true, "");
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn validate_future_tool_name_blocks_protected_path() {
    // A future tool name like "WriteAtomic" should fall-closed.
    let (allowed, msg) = validate("/project/.claude/rules/foo.md", true, "WriteAtomic");
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn validate_lowercase_read_allows_protected_path() {
    // Normalization: lowercase "read" must allow protected paths
    // (case-insensitive Read recognition). Per
    // `.claude/rules/security-gates.md` "Normalize Before Comparing."
    let (allowed, _) = validate("/project/.claude/rules/foo.md", true, "read");
    assert!(allowed);
}

#[test]
fn validate_whitespace_padded_read_allows_protected_path() {
    // Normalization: trailing whitespace on tool_name must not
    // defeat the Read allow branch.
    let (allowed, _) = validate("/project/.claude/rules/foo.md", true, "Read ");
    assert!(allowed);
}

#[test]
fn validate_glob_allows_protected_path() {
    let (allowed, _) = validate("/project/.claude/rules/foo.md", true, "Glob");
    assert!(allowed);
}

#[test]
fn validate_grep_allows_protected_path() {
    let (allowed, _) = validate("/project/.claude/rules/foo.md", true, "Grep");
    assert!(allowed);
}

#[test]
fn validate_claude_paths_blocks_in_claude_projects_when_no_flow_active() {
    // Distinguishing property: unlike .claude/rules, the transcript
    // block fires even when no flow is active. Pre-flow and
    // post-flow tampering must be blocked too.
    let (allowed, msg) = validate(
        "/Users/ben/.claude/projects/abc/session.jsonl",
        false,
        "Edit",
    );
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("transcript"));
}

#[test]
fn validate_claude_paths_allows_edit_in_other_paths_under_home() {
    // .claude/rules pre-existing behavior preserved — without an
    // active flow, .claude/rules edits pass through.
    let (allowed, msg) = validate("/Users/ben/.claude/rules/foo.md", false, "Edit");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn validate_claude_paths_blocks_nested_claude_projects() {
    let (allowed, msg) = validate(
        "/Users/ben/.claude/projects/abc/subdir/deep.jsonl",
        false,
        "Edit",
    );
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn validate_claude_paths_case_insensitive_claude_projects_match() {
    // macOS APFS is case-insensitive — `.CLAUDE/Projects/` resolves
    // to the same inode as `.claude/projects/`. The block matches
    // both casings.
    let (allowed, msg) = validate(
        "/Users/ben/.CLAUDE/Projects/abc/session.jsonl",
        false,
        "Edit",
    );
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn validate_claude_paths_allows_claude_projects_substring_in_filename() {
    // `.claude_projects` (no separator) is not the transcript
    // family — must not match.
    let (allowed, _msg) = validate("/Users/ben/foo/.claude_projects/x", true, "Edit");
    assert!(allowed);
}

#[test]
fn validate_claude_paths_block_message_includes_write_rule_redirect() {
    // The transcript-root block message must lead with the redirect
    // to bin/flow write-rule so the model has a concrete path to
    // route a behavioral constraint into a project rule instead of
    // silently dropping the persistence target.
    let (_, msg) = validate(
        "/Users/testuser/.claude/projects/abc/session.jsonl",
        true,
        "Edit",
    );
    assert!(msg.contains("write-rule"), "msg: {}", msg);
}

#[test]
fn validate_claude_paths_block_message_points_at_persistence_routing_rule() {
    // The transcript-root block message must reference
    // persistence-routing.md so the model can consult the routing
    // decision tree when the block fires.
    let (_, msg) = validate(
        "/Users/testuser/.claude/projects/abc/session.jsonl",
        true,
        "Edit",
    );
    assert!(msg.contains("persistence-routing.md"), "msg: {}", msg);
}

// --- run_impl_main tests (drive find_project_root_in branches) ---

fn seed_active_flow_fixture(root: &Path, branch: &str) -> std::path::PathBuf {
    let branch_dir = root.join(".flow-states").join(branch);
    std::fs::create_dir_all(&branch_dir).unwrap();
    std::fs::write(branch_dir.join("state.json"), "{}").unwrap();
    let worktree = root.join(".worktrees").join(branch);
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(worktree.join(".git"), "gitdir: fake\n").unwrap();
    worktree
}

#[test]
fn run_impl_main_returns_zero_when_cwd_none() {
    let cwd: Option<&Path> = None;
    let (code, msg) = run_impl_main(
        Some(serde_json::json!({
            "tool_input": {"file_path": "/anything/.claude/rules/foo.md"}
        })),
        cwd,
    );
    assert_eq!(code, 0);
    assert!(msg.is_none());
}

#[test]
fn run_impl_main_returns_zero_when_hook_input_missing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (code, msg) = run_impl_main(None, Some(&root));
    assert_eq!(code, 0);
    assert!(msg.is_none());
}

#[test]
fn run_impl_main_returns_zero_when_file_path_empty() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let input = serde_json::json!({"tool_input": {}});
    let (code, msg) = run_impl_main(Some(input), Some(&root));
    assert_eq!(code, 0);
    assert!(msg.is_none());
}

#[test]
fn run_impl_main_returns_zero_when_no_project_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let input = serde_json::json!({
        "tool_input": {"file_path": "/anything/.claude/rules/foo.md"}
    });
    let (code, msg) = run_impl_main(Some(input), Some(&root));
    assert_eq!(code, 0);
    assert!(msg.is_none());
}

#[test]
fn run_impl_main_returns_block_when_flow_active_and_protected_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let worktree = seed_active_flow_fixture(&root, "feat");
    let target = worktree.join(".claude/rules/foo.md");
    let input = serde_json::json!({
        "tool_input": {"file_path": target.to_string_lossy()}
    });
    let (code, msg) = run_impl_main(Some(input), Some(&worktree));
    assert_eq!(code, 2);
    let msg = msg.expect("block returns Some(message)");
    assert!(msg.contains("BLOCKED"), "message: {}", msg);
}

#[test]
fn run_impl_main_returns_zero_when_flow_active_and_unprotected_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let worktree = seed_active_flow_fixture(&root, "feat");
    let target = worktree.join("src/lib.rs");
    let input = serde_json::json!({
        "tool_input": {"file_path": target.to_string_lossy()}
    });
    let (code, msg) = run_impl_main(Some(input), Some(&worktree));
    assert_eq!(code, 0);
    assert!(msg.is_none());
}

#[test]
fn run_impl_main_returns_zero_when_branch_none() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join(".flow-states")).unwrap();
    let sub = root.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let input = serde_json::json!({
        "tool_input": {"file_path": "/anything/.claude/rules/foo.md"}
    });
    let (code, msg) = run_impl_main(Some(input), Some(&sub));
    assert_eq!(code, 0);
    assert!(msg.is_none());
}

/// Covers the direct-match branch of `find_project_root_in`: cwd
/// itself has `.flow-states/`, so the loop returns on the first
/// iteration. Complements the ancestor-match case above.
#[test]
fn run_impl_main_cwd_with_flow_states_directly_resolves_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join(".flow-states")).unwrap();
    let input = serde_json::json!({
        "tool_input": {"file_path": "/anything/.claude/rules/foo.md"}
    });
    let (code, msg) = run_impl_main(Some(input), Some(&root));
    // `detect_branch_from_path` returns None because the cwd is the
    // project root (not under `.worktrees/`), so flow_active is false
    // and the hook silently allows.
    assert_eq!(code, 0);
    assert!(msg.is_none());
}

// --- run() subprocess tests ---

fn run_hook_subprocess(cwd: &Path, stdin_input: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["hook", "validate-claude-paths"])
        .current_dir(cwd)
        .env_remove("FLOW_CI_RUNNING")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn flow-rs");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(stdin_input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().expect("wait");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

/// `run()` with no active flow silently allows (exit 0, no stderr).
#[test]
fn run_subprocess_exits_0_when_no_flow_active() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let input = serde_json::json!({
        "tool_input": {"file_path": "/project/.claude/rules/foo.md"}
    });
    let (code, _stdout, _stderr) = run_hook_subprocess(&root, &input.to_string());
    assert_eq!(code, 0);
}

/// `run()` with an active flow and protected path blocks (exit 2,
/// stderr carries the BLOCKED message).
#[test]
fn run_subprocess_exits_2_when_flow_active_and_protected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let worktree = seed_active_flow_fixture(&root, "feat");
    let target = worktree.join(".claude/rules/foo.md");
    let input = serde_json::json!({
        "tool_input": {"file_path": target.to_string_lossy()}
    });
    let (code, _stdout, stderr) = run_hook_subprocess(&worktree, &input.to_string());
    assert_eq!(code, 2);
    assert!(stderr.contains("BLOCKED"), "stderr: {}", stderr);
}

// --- Read tool on transcript paths blocks regardless of flow state ---

/// Read tool input on a transcript-root path blocks (exit 2) regardless
/// of flow-active state. Registering `validate-claude-paths` on the Read
/// matcher mechanically blocks a model that attempts to read
/// `~/.claude/projects/` before the path leaves the harness — the
/// sanctioned recovery surface is `compact_summary` in the state file,
/// not the persisted transcript JSONL. The walkers in `validate-skill`
/// and `validate-ask-user` use `fs::read_to_string` from inside Rust
/// subcommands, not the Read tool, so blocking the Read tool does not
/// affect them.
#[test]
fn read_on_transcript_root_blocks_regardless_of_flow_state() {
    // Case 1: no active flow.
    let dir1 = tempfile::tempdir().unwrap();
    let root1 = dir1.path().canonicalize().unwrap();
    let input_no_flow = serde_json::json!({
        "tool_name": "Read",
        "tool_input": {
            "file_path": "/Users/example/.claude/projects/abc/session.jsonl"
        }
    });
    let (code, _stdout, stderr) = run_hook_subprocess(&root1, &input_no_flow.to_string());
    assert_eq!(code, 2, "no-flow Read on transcript path must block");
    assert!(
        stderr.contains("BLOCKED") && stderr.contains("transcript"),
        "stderr: {}",
        stderr
    );

    // Case 2: active flow in a worktree.
    let dir2 = tempfile::tempdir().unwrap();
    let root2 = dir2.path().canonicalize().unwrap();
    let worktree = seed_active_flow_fixture(&root2, "feat");
    let input_active = serde_json::json!({
        "tool_name": "Read",
        "tool_input": {
            "file_path": "/Users/example/.claude/projects/abc/session.jsonl"
        }
    });
    let (code, _stdout, stderr) = run_hook_subprocess(&worktree, &input_active.to_string());
    assert_eq!(code, 2, "active-flow Read on transcript path must block");
    assert!(
        stderr.contains("BLOCKED") && stderr.contains("transcript"),
        "stderr: {}",
        stderr
    );
}

/// Read of `.claude/rules/`, `.claude/skills/`, and `CLAUDE.md` is
/// allowed during active flows; only Edit and Write to those paths
/// are redirected to `bin/flow write-rule`. The transcript root block
/// added in this PR applies only to `~/.claude/projects/` paths, not
/// to plugin-managed rule paths.
#[test]
fn read_on_protected_non_transcript_path_allows_during_active_flow() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let worktree = seed_active_flow_fixture(&root, "feat");
    let target = worktree.join(".claude/rules/foo.md");
    let input = serde_json::json!({
        "tool_name": "Read",
        "tool_input": {"file_path": target.to_string_lossy()}
    });
    let (code, _stdout, stderr) = run_hook_subprocess(&worktree, &input.to_string());
    assert_eq!(
        code, 0,
        "Read on .claude/rules/ must allow during flow; stderr: {}",
        stderr
    );
}

// --- Glob and Grep field handling (regression: tool_input.path /
// tool_input.pattern bypass) ---
//
// The hook is registered on the Read|Glob|Grep matcher; for Glob and
// Grep, the target path lives in `tool_input.path` or
// `tool_input.pattern` rather than `tool_input.file_path`. Without
// extracting both fields, a Glob/Grep call on the transcript root
// silently bypassed the block. These tests lock in that the extraction
// covers all three field shapes.

#[test]
fn glob_on_transcript_root_blocks_via_path_field() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let input = serde_json::json!({
        "tool_name": "Glob",
        "tool_input": {
            "path": "/Users/example/.claude/projects/abc/session.jsonl"
        }
    });
    let (code, _stdout, stderr) = run_hook_subprocess(&root, &input.to_string());
    assert_eq!(
        code, 2,
        "Glob with tool_input.path on transcript root must block; stderr: {}",
        stderr
    );
    assert!(stderr.contains("BLOCKED"));
}

#[test]
fn glob_on_transcript_root_blocks_via_pattern_field() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let input = serde_json::json!({
        "tool_name": "Glob",
        "tool_input": {
            "pattern": "/Users/example/.claude/projects/**/*.jsonl"
        }
    });
    let (code, _stdout, stderr) = run_hook_subprocess(&root, &input.to_string());
    assert_eq!(
        code, 2,
        "Glob with tool_input.pattern on transcript root must block; stderr: {}",
        stderr
    );
    assert!(stderr.contains("BLOCKED"));
}

#[test]
fn grep_on_transcript_root_blocks_via_path_field() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let input = serde_json::json!({
        "tool_name": "Grep",
        "tool_input": {
            "path": "/Users/example/.claude/projects/abc"
        }
    });
    let (code, _stdout, stderr) = run_hook_subprocess(&root, &input.to_string());
    assert_eq!(
        code, 2,
        "Grep with tool_input.path on transcript root must block; stderr: {}",
        stderr
    );
    assert!(stderr.contains("BLOCKED"));
}

#[test]
fn glob_on_protected_non_transcript_path_allows_during_active_flow() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let worktree = seed_active_flow_fixture(&root, "feat");
    let target = worktree.join(".claude/rules");
    let input = serde_json::json!({
        "tool_name": "Glob",
        "tool_input": {"path": target.to_string_lossy()}
    });
    let (code, _stdout, stderr) = run_hook_subprocess(&worktree, &input.to_string());
    assert_eq!(
        code, 0,
        "Glob on .claude/rules/ must allow during flow; stderr: {}",
        stderr
    );
}

#[test]
fn memory_subdirectory_read_allows_in_subprocess() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let input = serde_json::json!({
        "tool_name": "Read",
        "tool_input": {
            "file_path": "/Users/example/.claude/projects/proj-id/memory/MEMORY.md"
        }
    });
    let (code, _stdout, stderr) = run_hook_subprocess(&root, &input.to_string());
    assert_eq!(
        code, 0,
        "Read of memory subdirectory must allow; stderr: {}",
        stderr
    );
}
