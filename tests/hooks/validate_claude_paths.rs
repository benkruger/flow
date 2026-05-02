//! Integration tests for `src/hooks/validate_claude_paths.rs`.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use flow_rs::hooks::validate_claude_paths::{is_protected_path, run_impl_main, validate};

// --- is_protected_path tests ---

#[test]
fn test_is_protected_path_empty() {
    assert!(!is_protected_path(""));
}

#[test]
fn test_is_protected_path_claude_rules() {
    assert!(is_protected_path("/project/.claude/rules/foo.md"));
}

#[test]
fn test_is_protected_path_claude_md() {
    assert!(is_protected_path("/project/CLAUDE.md"));
}

#[test]
fn test_is_protected_path_claude_skills() {
    assert!(is_protected_path("/project/.claude/skills/foo/SKILL.md"));
}

#[test]
fn test_is_protected_path_settings() {
    assert!(!is_protected_path("/project/.claude/settings.json"));
}

#[test]
fn test_is_protected_path_settings_local() {
    assert!(!is_protected_path("/project/.claude/settings.local.json"));
}

#[test]
fn test_is_protected_path_nested_rules() {
    assert!(is_protected_path("/project/.claude/rules/subdir/deep.md"));
}

#[test]
fn test_is_protected_path_nested_skills() {
    assert!(is_protected_path(
        "/project/.claude/skills/subdir/deep/SKILL.md"
    ));
}

#[test]
fn test_is_protected_path_worktree_rules() {
    assert!(is_protected_path(
        "/project/.worktrees/feat/.claude/rules/foo.md"
    ));
}

#[test]
fn test_is_protected_path_worktree_claude_md() {
    assert!(is_protected_path("/project/.worktrees/feat/CLAUDE.md"));
}

#[test]
fn test_is_protected_path_worktree_skills() {
    assert!(is_protected_path(
        "/project/.worktrees/feat/.claude/skills/foo/SKILL.md"
    ));
}

#[test]
fn test_is_protected_path_mixed_case_claude_md() {
    assert!(is_protected_path("/project/Claude.md"));
    assert!(is_protected_path("/project/claude.md"));
}

#[test]
fn test_is_protected_path_mixed_case_claude_dir() {
    assert!(is_protected_path("/project/.CLAUDE/rules/foo.md"));
    assert!(is_protected_path("/project/.Claude/rules/foo.md"));
}

#[test]
fn test_is_protected_path_mixed_case_rules_and_skills() {
    assert!(is_protected_path("/project/.claude/Rules/foo.md"));
    assert!(is_protected_path("/project/.claude/SKILLS/foo/SKILL.md"));
}

// --- validate tests ---

#[test]
fn test_blocks_claude_rules_when_flow_active() {
    let (allowed, msg) = validate("/project/.claude/rules/foo.md", true);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("write-rule"));
}

#[test]
fn test_blocks_claude_md_when_flow_active() {
    let (allowed, msg) = validate("/project/CLAUDE.md", true);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("write-rule"));
}

#[test]
fn test_allows_claude_rules_when_no_flow() {
    let (allowed, msg) = validate("/project/.claude/rules/foo.md", false);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_claude_md_when_no_flow() {
    let (allowed, msg) = validate("/project/CLAUDE.md", false);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_unrelated_path_when_flow_active() {
    let (allowed, msg) = validate("/project/lib/foo.py", true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_claude_settings_when_flow_active() {
    let (allowed, msg) = validate("/project/.claude/settings.json", true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_flow_states_path() {
    let (allowed, msg) = validate("/project/.flow-states/branch-rule-content.md", true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_empty_path() {
    let (allowed, msg) = validate("", true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_blocks_nested_claude_rules() {
    let (allowed, msg) = validate("/project/.claude/rules/subdir/deep.md", true);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_worktree_claude_rules() {
    let (allowed, msg) = validate("/project/.worktrees/feat/.claude/rules/foo.md", true);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_worktree_claude_md() {
    let (allowed, msg) = validate("/project/.worktrees/feat/CLAUDE.md", true);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_claude_skills_when_flow_active() {
    let (allowed, msg) = validate("/project/.claude/skills/foo/SKILL.md", true);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("write-rule"));
}

#[test]
fn test_blocks_nested_claude_skills() {
    let (allowed, msg) = validate("/project/.claude/skills/subdir/deep/SKILL.md", true);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_worktree_claude_skills() {
    let (allowed, msg) = validate("/project/.worktrees/feat/.claude/skills/foo/SKILL.md", true);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_allows_claude_skills_when_no_flow() {
    let (allowed, msg) = validate("/project/.claude/skills/foo/SKILL.md", false);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_claude_settings_local() {
    let (allowed, _) = validate("/project/.claude/settings.local.json", true);
    assert!(allowed);
}

#[test]
fn test_error_message_mentions_write_rule() {
    let (_, msg) = validate("/project/.claude/rules/foo.md", true);
    assert!(msg.contains("write-rule"));
    assert!(msg.contains("--path"));
    assert!(msg.contains("--content-file"));
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
