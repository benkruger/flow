//! Integration tests for `src/hooks/validate_worktree_paths.rs`.

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use flow_rs::hooks::validate_worktree_paths::{
    build_rewrite_envelope, detect_misplaced_flow_states, get_file_path,
    is_approved_out_of_project_path, is_shared_config, validate, validate_shared_config,
    APPROVED_TMP_EXTENSIONS,
};
use serde_json::json;

// --- validate tests ---

#[test]
fn test_allows_when_not_in_worktree() {
    let (allowed, msg) = validate(
        "/Users/ben/code/flow/lib/foo.py",
        "/Users/ben/code/flow",
        "/Users/ben",
    );
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_file_inside_worktree() {
    let (allowed, msg) = validate(
        "/Users/ben/code/flow/.worktrees/my-feature/lib/foo.py",
        "/Users/ben/code/flow/.worktrees/my-feature",
        "/Users/ben",
    );
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_blocks_main_repo_path_from_worktree() {
    let cwd = "/Users/ben/code/flow/.worktrees/my-feature";
    let (allowed, msg) = validate("/Users/ben/code/flow/lib/foo.py", cwd, "/Users/ben");
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains(cwd));
}

#[test]
fn test_allows_flow_states_path() {
    let (allowed, msg) = validate(
        "/Users/ben/code/flow/.flow-states/my-feature.json",
        "/Users/ben/code/flow/.worktrees/my-feature",
        "/Users/ben",
    );
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_error_message_includes_corrected_path() {
    let cwd = "/Users/ben/code/flow/.worktrees/my-feature";
    let file_path = "/Users/ben/code/flow/skills/flow-prime/SKILL.md";
    let (allowed, msg) = validate(file_path, cwd, "/Users/ben");
    assert!(!allowed);
    let corrected = "/Users/ben/code/flow/.worktrees/my-feature/skills/flow-prime/SKILL.md";
    assert!(msg.contains(corrected));
    assert!(msg.contains(file_path));
}

#[test]
fn test_allows_empty_file_path() {
    let (allowed, _) = validate(
        "",
        "/Users/ben/code/flow/.worktrees/my-feature",
        "/Users/ben",
    );
    assert!(allowed);
}

#[test]
fn test_allows_worktree_root_path_exactly() {
    let cwd = "/Users/ben/code/flow/.worktrees/my-feature";
    let (allowed, _) = validate(cwd, cwd, "/Users/ben");
    assert!(allowed);
}

// Regression for #1269: cwd in a service subdir of the worktree;
// file_path at the worktree root must be allowed.
#[test]
fn validate_allows_worktree_root_path_from_subdir_cwd() {
    let cwd = "/Users/ben/code/flow/.worktrees/my-feature/synapse";
    let file_path = "/Users/ben/code/flow/.worktrees/my-feature/CLAUDE.md";
    let (allowed, msg) = validate(file_path, cwd, "/Users/ben");
    assert!(allowed);
    assert!(msg.is_empty());
}

// Regression for #1260: cwd in a subdir, file_path under .claude/rules/
// must be allowed.
#[test]
fn validate_allows_claude_rules_path_from_subdir_cwd() {
    let cwd = "/Users/ben/code/flow/.worktrees/my-feature/cortex";
    let file_path = "/Users/ben/code/flow/.worktrees/my-feature/.claude/rules/testing-gotchas.md";
    let (allowed, _) = validate(file_path, cwd, "/Users/ben");
    assert!(allowed);
}

// Regression for #1291 / #1249: redirect message must name the worktree
// root, never produce a doubly-nested path.
#[test]
fn validate_redirect_uses_worktree_root_not_cwd_subdir() {
    let cwd = "/Users/ben/code/flow/.worktrees/my-feature/synapse";
    let file_path = "/Users/ben/code/flow/lib/foo.py";
    let (allowed, msg) = validate(file_path, cwd, "/Users/ben");
    assert!(!allowed);
    assert!(msg.contains("/Users/ben/code/flow/.worktrees/my-feature/lib/foo.py"));
    assert!(!msg.contains("/synapse/.worktrees/"));
}

// Regression: multi-level subdir cwd produces worktree-root-prefixed
// redirect, exercises Branch F of compute_worktree_root.
#[test]
fn validate_redirect_uses_worktree_root_multi_level_subdir() {
    let cwd = "/Users/ben/code/flow/.worktrees/my-feature/packages/api";
    let file_path = "/Users/ben/code/flow/lib/foo.py";
    let (_, msg) = validate(file_path, cwd, "/Users/ben");
    assert!(msg.contains("/Users/ben/code/flow/.worktrees/my-feature/lib/foo.py"));
    assert!(!msg.contains("/packages/api/.worktrees/"));
}

// Regression: when cwd ends exactly in `.worktrees/` (no branch
// segment), the hook treats it as "not in a worktree" and allows the
// file_path. Previously, an inline reimplementation in the hook
// produced a malformed redirect containing `//` (e.g.,
// "Use /proj/.worktrees//lib/foo.py"). The shared helper returns
// None for this shape; the hook now follows.
#[test]
fn validate_allows_when_cwd_ends_in_marker_no_branch() {
    let cwd = "/proj/.worktrees/";
    let file_path = "/proj/lib/foo.py";
    let (allowed, msg) = validate(file_path, cwd, "/Users/ben");
    assert!(allowed);
    assert!(msg.is_empty());
}

// Regression for the rightmost-occurrence anchor: when cwd is inside
// a worktree whose project_root path contains `.worktrees/` as a
// non-marker directory component, the hook resolves the FLOW worktree
// boundary at the deepest `/.worktrees/` segment, not the spurious
// project-root match.
#[test]
fn validate_uses_rightmost_worktrees_segment_in_redirect() {
    let cwd = "/home/dev/my.worktrees/proj/.worktrees/feat/cortex";
    let file_path = "/home/dev/my.worktrees/proj/lib/foo.py";
    let (allowed, msg) = validate(file_path, cwd, "/Users/ben");
    assert!(!allowed);
    assert!(msg.contains("/home/dev/my.worktrees/proj/.worktrees/feat/lib/foo.py"));
}

// --- validate() autonomous-strict branch (#1704 branch C) ---

/// Plant a state file at `<root>/.flow-states/<branch>/state.json`.
fn write_state_for(root: &std::path::Path, branch: &str, content: &str) {
    let dir = root.join(".flow-states").join(branch);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("state.json"), content).unwrap();
}

const AUTO_IN_PROGRESS: &str = r#"{
    "current_phase": "flow-code",
    "phases": {"flow-code": {"status": "in_progress"}},
    "skills": {"flow-code": {"continue": "auto"}}
}"#;

const MANUAL_IN_PROGRESS: &str = r#"{
    "current_phase": "flow-code",
    "phases": {"flow-code": {"status": "in_progress"}},
    "skills": {"flow-code": {"continue": "manual"}}
}"#;

#[test]
fn autonomous_strict_emits_json_envelope_on_existing_block_path() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    write_state_for(&root, "feat", AUTO_IN_PROGRESS);
    let worktree = format!("{}/.worktrees/feat", root.display());
    let file_path = format!("{}/lib/foo.py", root.display());
    let (allowed, msg) = validate(&file_path, &worktree, "/Users/ben");
    assert!(!allowed);
    assert!(
        msg.contains("\"reason\":\"out_of_worktree_in_autonomous\""),
        "expected JSON envelope; got: {}",
        msg
    );
    assert!(msg.contains("\"autonomous\":true"));
    assert!(msg.contains(&file_path));
}

#[test]
fn non_autonomous_preserves_existing_block_message_shape() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    write_state_for(&root, "feat", MANUAL_IN_PROGRESS);
    let worktree = format!("{}/.worktrees/feat", root.display());
    let file_path = format!("{}/lib/foo.py", root.display());
    let (allowed, msg) = validate(&file_path, &worktree, "/Users/ben");
    assert!(!allowed);
    assert!(
        msg.starts_with("BLOCKED:"),
        "non-autonomous flow should preserve human-readable BLOCKED message; got: {}",
        msg
    );
    assert!(!msg.contains("out_of_worktree_in_autonomous"));
}

#[test]
fn autonomous_strict_allows_in_worktree_path() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    write_state_for(&root, "feat", AUTO_IN_PROGRESS);
    let worktree = format!("{}/.worktrees/feat", root.display());
    let file_path = format!("{}/.worktrees/feat/src/lib.rs", root.display());
    let (allowed, msg) = validate(&file_path, &worktree, "/Users/ben");
    assert!(allowed, "in-worktree path must be allowed; got msg={}", msg);
}

#[test]
fn non_active_flow_preserves_existing_block_message_shape() {
    // No state file written — is_autonomous_flow_active returns false.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let worktree = format!("{}/.worktrees/feat", root.display());
    let file_path = format!("{}/lib/foo.py", root.display());
    let (allowed, msg) = validate(&file_path, &worktree, "/Users/ben");
    assert!(!allowed);
    assert!(
        msg.starts_with("BLOCKED:"),
        "no active flow should preserve human-readable BLOCKED message; got: {}",
        msg
    );
}

#[test]
fn autonomous_strict_preserves_flow_states_redirect_message() {
    // Misplaced .flow-states/ write under the worktree — should
    // route through the .flow-states/ redirect branch BEFORE the
    // autonomous-strict check, preserving the existing redirect
    // message regardless of autonomous mode.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    write_state_for(&root, "feat", AUTO_IN_PROGRESS);
    let worktree = format!("{}/.worktrees/feat", root.display());
    let file_path = format!("{}/.worktrees/feat/.flow-states/feat/log", root.display());
    let (allowed, msg) = validate(&file_path, &worktree, "/Users/ben");
    assert!(!allowed);
    assert!(
        msg.contains(".flow-states/"),
        "misplaced .flow-states/ should still get its redirect message: {}",
        msg
    );
    assert!(!msg.contains("out_of_worktree_in_autonomous"));
}

// --- validate() out-of-project gate matrix ---
//
// Out-of-project paths (not under project_root) are fail-closed during
// an active flow (cwd inside a worktree). Only the approved surface
// (auto-memory dir + /tmp scratch) is allowed; everything else is
// blocked — autonomous flows get the `out_of_bounds_in_autonomous`
// JSON envelope, manual flows get the prose BLOCKED message. Fixtures
// canonicalize the tempdir root per
// `.claude/rules/testing-gotchas.md` "macOS Subprocess Path
// Canonicalization" so the project_root prefix comparison inside
// validate() matches.

#[test]
fn validate_out_of_project_plugin_read_autonomous_emits_envelope() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    write_state_for(&root, "feat", AUTO_IN_PROGRESS);
    let worktree = format!("{}/.worktrees/feat", root.display());
    // home is OUTSIDE the project root so the path reaches the
    // out-of-project branch (a home under root would be in-project).
    let home = "/Users/testuser";
    let plugin_path = "/Users/testuser/.claude/plugins/cache/flow/0.1/SKILL.md";
    let (allowed, msg) = validate(plugin_path, &worktree, home);
    assert!(!allowed);
    assert!(
        msg.contains("\"reason\":\"out_of_bounds_in_autonomous\""),
        "expected out_of_bounds envelope; got: {}",
        msg
    );
    assert!(msg.contains("\"autonomous\":true"));
    assert!(msg.contains(plugin_path));
}

#[test]
fn validate_out_of_project_plugin_read_manual_emits_prose() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    write_state_for(&root, "feat", MANUAL_IN_PROGRESS);
    let worktree = format!("{}/.worktrees/feat", root.display());
    let home = "/Users/testuser";
    let plugin_path = "/Users/testuser/.claude/plugins/cache/flow/0.1/SKILL.md";
    let (allowed, msg) = validate(plugin_path, &worktree, home);
    assert!(!allowed);
    assert!(
        msg.starts_with("BLOCKED:"),
        "manual flow should emit prose BLOCKED; got: {}",
        msg
    );
    assert!(!msg.contains("out_of_bounds_in_autonomous"));
}

#[test]
fn validate_out_of_project_memory_read_is_allowed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    write_state_for(&root, "feat", AUTO_IN_PROGRESS);
    let worktree = format!("{}/.worktrees/feat", root.display());
    let home = "/Users/testuser";
    let memory_path = "/Users/testuser/.claude/projects/abc/memory/MEMORY.md";
    let (allowed, msg) = validate(memory_path, &worktree, home);
    assert!(
        allowed,
        "approved memory path must be allowed; got: {}",
        msg
    );
}

#[test]
fn validate_out_of_project_tmp_scratch_is_allowed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    write_state_for(&root, "feat", AUTO_IN_PROGRESS);
    let worktree = format!("{}/.worktrees/feat", root.display());
    // /tmp/x.md is out-of-project relative to the tempdir root and in
    // the approved extension set.
    let (allowed, msg) = validate("/tmp/flow-scratch.md", &worktree, "/Users/ben");
    assert!(
        allowed,
        "approved /tmp scratch must be allowed; got: {}",
        msg
    );
}

#[test]
fn validate_out_of_project_arbitrary_path_is_blocked() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    // No state file → not autonomous → prose BLOCKED.
    let worktree = format!("{}/.worktrees/feat", root.display());
    let (allowed, msg) = validate("/etc/hosts", &worktree, "/Users/ben");
    assert!(!allowed, "arbitrary out-of-project path must be blocked");
    assert!(msg.starts_with("BLOCKED:"));
}

#[test]
fn validate_cwd_not_in_worktree_allows_out_of_project_path() {
    // The fail-closed gate is scoped to cwd-inside-a-worktree (the
    // active-flow proxy). On a plain repo cwd, out-of-project behavior
    // is unchanged — the "not in a worktree" early return allows.
    let (allowed, msg) = validate("/etc/hosts", "/Users/ben/code/flow", "/Users/ben");
    assert!(allowed, "non-worktree cwd must not block; got: {}", msg);
    assert!(msg.is_empty());
}

#[test]
fn validate_out_of_project_block_is_tool_agnostic() {
    // validate() does not branch on tool name — an out-of-project path
    // blocks identically whether the caller is a Read or an Edit/Write
    // (run_impl_main reaches validate() before any tool-name check).
    // The regression guard is against a future tool-name carve-out
    // inside validate() that would let an out-of-project Edit slip.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let worktree = format!("{}/.worktrees/feat", root.display());
    // Out-of-project (not under the tempdir root), not memory, not /tmp.
    let edit_target = "/Users/someone/other-repo/config.toml";
    let (allowed, msg) = validate(edit_target, &worktree, "/Users/ben");
    assert!(!allowed, "out-of-project Edit/Write target must block");
    assert!(msg.starts_with("BLOCKED:"));
}

// --- is_approved_out_of_project_path tests ---

// Class 1: home-anchored auto-memory directory.

#[test]
fn approved_oop_memory_path_under_valid_home_is_allowed() {
    assert!(is_approved_out_of_project_path(
        "/Users/ben/.claude/projects/abc123/memory/MEMORY.md",
        "/Users/ben"
    ));
}

#[test]
fn approved_oop_memory_path_case_variant_is_allowed() {
    // macOS APFS is case-insensitive — the .claude/projects/.../memory
    // structure matches regardless of case so a case-variant path
    // resolves to the same inode native already allows.
    assert!(is_approved_out_of_project_path(
        "/Users/ben/.CLAUDE/Projects/abc123/Memory/notes.md",
        "/Users/ben"
    ));
}

#[test]
fn approved_oop_memory_path_under_empty_home_is_blocked() {
    // Env-var validator: an empty $HOME fails closed before any memory
    // prefix is built.
    assert!(!is_approved_out_of_project_path(
        "/Users/ben/.claude/projects/abc123/memory/MEMORY.md",
        ""
    ));
}

#[test]
fn approved_oop_memory_path_under_relative_home_is_blocked() {
    // A relative $HOME (no leading slash) fails closed — otherwise it
    // would resolve cwd-relative against the worktree root.
    assert!(!is_approved_out_of_project_path(
        "relative/dir/.claude/projects/abc123/memory/MEMORY.md",
        "relative/dir"
    ));
}

#[test]
fn approved_oop_memory_shaped_path_not_under_home_is_blocked() {
    // A /tmp path shaped like the memory dir but NOT rooted at <home>
    // must not be allowed (tight allow — home-anchored).
    assert!(!is_approved_out_of_project_path(
        "/tmp/.claude/projects/x/memory/y",
        "/Users/ben"
    ));
}

#[test]
fn approved_oop_memory_prefix_substring_of_longer_home_is_blocked() {
    // home="/Users/ben" must not match "/Users/benjamin/..." via a
    // bare string prefix — the segment boundary after home is required.
    assert!(!is_approved_out_of_project_path(
        "/Users/benjamin/.claude/projects/abc/memory/MEMORY.md",
        "/Users/ben"
    ));
}

#[test]
fn approved_oop_memory_empty_id_segment_is_blocked() {
    assert!(!is_approved_out_of_project_path(
        "/Users/ben/.claude/projects//memory/MEMORY.md",
        "/Users/ben"
    ));
}

#[test]
fn approved_oop_memory_no_file_after_memory_is_blocked() {
    // A directory-only memory path (trailing slash, no file) is not an
    // approved file target.
    assert!(!is_approved_out_of_project_path(
        "/Users/ben/.claude/projects/abc/memory/",
        "/Users/ben"
    ));
}

// Class 2: /tmp/ scratch with an approved extension (one assertion per
// extension to satisfy per-branch coverage of the extension set).

#[test]
fn approved_oop_tmp_txt_is_allowed() {
    assert!(is_approved_out_of_project_path(
        "/tmp/scratch.txt",
        "/Users/ben"
    ));
}

#[test]
fn approved_oop_tmp_diff_is_allowed() {
    assert!(is_approved_out_of_project_path(
        "/tmp/scratch.diff",
        "/Users/ben"
    ));
}

#[test]
fn approved_oop_tmp_patch_is_allowed() {
    assert!(is_approved_out_of_project_path(
        "/tmp/scratch.patch",
        "/Users/ben"
    ));
}

#[test]
fn approved_oop_tmp_md_is_allowed() {
    assert!(is_approved_out_of_project_path(
        "/tmp/scratch.md",
        "/Users/ben"
    ));
}

#[test]
fn approved_oop_tmp_json_is_allowed() {
    assert!(is_approved_out_of_project_path(
        "/tmp/scratch.json",
        "/Users/ben"
    ));
}

#[test]
fn approved_oop_tmp_jsonl_is_allowed() {
    assert!(is_approved_out_of_project_path(
        "/tmp/scratch.jsonl",
        "/Users/ben"
    ));
}

#[test]
fn approved_oop_tmp_case_variant_is_allowed() {
    // /TMP/X.MD resolves to the same inode as /tmp/x.md on a
    // case-insensitive filesystem.
    assert!(is_approved_out_of_project_path("/TMP/X.MD", "/Users/ben"));
}

#[test]
fn approved_oop_tmp_unapproved_extension_is_blocked() {
    assert!(!is_approved_out_of_project_path(
        "/tmp/scratch.csv",
        "/Users/ben"
    ));
}

#[test]
fn approved_oop_tmp_no_extension_is_blocked() {
    assert!(!is_approved_out_of_project_path(
        "/tmp/scratch",
        "/Users/ben"
    ));
}

#[test]
fn approved_oop_tmp_directory_only_is_blocked() {
    // The bare "/tmp/" prefix with no file segment has no extension.
    assert!(!is_approved_out_of_project_path("/tmp/", "/Users/ben"));
}

#[test]
fn approved_oop_tmp_doubled_slash_is_allowed() {
    // /tmp//x.md normalizes to /tmp/x.md at the FS layer; allowing it
    // matches the native same-inode resolution.
    assert!(is_approved_out_of_project_path(
        "/tmp//scratch.md",
        "/Users/ben"
    ));
}

// Everything else out-of-project is fail-closed.

#[test]
fn approved_oop_plugin_cache_is_blocked() {
    // The plugin cache is deliberately NOT in the approved surface —
    // an active flow has no reason to reach plugin source.
    assert!(!is_approved_out_of_project_path(
        "/Users/ben/.claude/plugins/cache/flow/0.28.5/skills/flow-code/SKILL.md",
        "/Users/ben"
    ));
}

#[test]
fn approved_oop_arbitrary_path_is_blocked() {
    assert!(!is_approved_out_of_project_path(
        "/Users/x/other/file.rs",
        "/Users/ben"
    ));
}

#[test]
fn approved_oop_nul_byte_path_is_blocked() {
    // Embedded NUL fails closed — a NUL truncates the path in syscalls.
    assert!(!is_approved_out_of_project_path(
        "/tmp/x\0.md",
        "/Users/ben"
    ));
}

// --- out-of-project edge cases (A1/A2/A3/R2 regression guards) ---

/// A1: the /tmp class is home-independent, so an empty $HOME must not
/// suppress an approved /tmp scratch path. Regression: ordering the
/// home guard ahead of the /tmp class blocks /tmp under an unset $HOME
/// (CI/cron/launchd), forcing a native-prompt-equivalent block on a
/// path UNIVERSAL_ALLOW grants regardless of $HOME.
#[test]
fn approved_oop_tmp_allowed_with_empty_home() {
    assert!(is_approved_out_of_project_path("/tmp/scratch.md", ""));
}

/// A1: a relative $HOME likewise has no bearing on the /tmp class.
#[test]
fn approved_oop_tmp_allowed_with_relative_home() {
    assert!(is_approved_out_of_project_path(
        "/tmp/scratch.json",
        "relative/home"
    ));
}

/// A2: a doubled-slash memory path resolves to the same inode as the
/// single-slash form, so it must be approved the way the /tmp class
/// accepts /tmp//x.md. Regression: dropping the up-front slash collapse
/// rejects the doubled-slash memory shape while approving the
/// equivalent /tmp shape.
#[test]
fn approved_oop_memory_doubled_slash_is_allowed() {
    assert!(is_approved_out_of_project_path(
        "/Users/ben//.claude/projects/abc/memory/MEMORY.md",
        "/Users/ben"
    ));
}

/// A3: a trailing-slash $HOME names the same home directory; the memory
/// class must still approve the path rather than failing the
/// segment-boundary check. Regression: removing the home trailing-slash
/// strip blocks a legitimate MEMORY.md under a trailing-slash $HOME.
#[test]
fn approved_oop_memory_trailing_slash_home_is_allowed() {
    assert!(is_approved_out_of_project_path(
        "/Users/ben/.claude/projects/abc/memory/MEMORY.md",
        "/Users/ben/"
    ));
}

/// R2: a `..` segment past memory/ escapes the memory dir and is not a
/// native-allowed single-segment match, so it must fail closed.
/// Regression: dropping the traversal-segment rejection approves a
/// memory-rooted path that escapes via `..` (and defers to a native
/// prompt that would hang an unattended flow).
#[test]
fn approved_oop_memory_parent_traversal_segment_is_blocked() {
    assert!(!is_approved_out_of_project_path(
        "/Users/ben/.claude/projects/abc/memory/../../../etc/passwd",
        "/Users/ben"
    ));
}

/// R2: a `.` segment likewise fails closed.
#[test]
fn approved_oop_memory_dot_segment_is_blocked() {
    assert!(!is_approved_out_of_project_path(
        "/Users/ben/.claude/projects/abc/memory/./notes.md",
        "/Users/ben"
    ));
}

/// A1 through the public validate() surface: an approved /tmp scratch
/// path is allowed even when $HOME is empty (the /tmp class is
/// home-independent). Drives the home-independence through validate()
/// rather than the helper alone — no state file written, so the flow
/// is non-autonomous and an over-block would surface as prose BLOCKED.
#[test]
fn validate_out_of_project_tmp_allowed_with_empty_home() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let worktree = format!("{}/.worktrees/feat", root.display());
    let (allowed, msg) = validate("/tmp/flow-scratch.md", &worktree, "");
    assert!(
        allowed,
        "approved /tmp scratch must be allowed with empty $HOME; got: {}",
        msg
    );
    assert!(msg.is_empty());
}

// --- APPROVED_TMP_EXTENSIONS drift contract ---

/// Named regression: a `/tmp` extension added to `UNIVERSAL_ALLOW`
/// (the native-permission allow-list in `src/prime_check.rs`) but NOT
/// to `APPROVED_TMP_EXTENSIONS` would re-open a native permission
/// prompt for that extension during an active flow — exactly the hang
/// the out-of-project gate exists to prevent. The named consumer is
/// `is_approved_out_of_project_path`, whose `/tmp` class is only safe
/// while its extension set equals the native allow-list's.
///
/// The two sources cannot share a single `const` (UNIVERSAL_ALLOW
/// entries are full permission strings like `Read(//tmp/*.md)`), so
/// this test extracts the `/tmp` extensions from UNIVERSAL_ALLOW at
/// test time and asserts set-equality with APPROVED_TMP_EXTENSIONS.
#[test]
fn approved_tmp_extensions_match_universal_allow() {
    use flow_rs::prime_check::UNIVERSAL_ALLOW;
    use std::collections::BTreeSet;

    // Single-slash marker matches both the double-slash entries
    // (`Read(//tmp/*.md)`) and a single-slash shape (`Read(/tmp/*.md)`),
    // so a future /tmp entry in either shape is still extracted rather
    // than silently missed by a shape-specific marker.
    let marker = "/tmp/*.";
    let mut from_allow: BTreeSet<String> = BTreeSet::new();
    for entry in UNIVERSAL_ALLOW {
        if let Some(idx) = entry.find(marker) {
            let after = &entry[idx + marker.len()..];
            if let Some(close) = after.find(')') {
                let ext = &after[..close];
                if !ext.is_empty() {
                    from_allow.insert(ext.to_ascii_lowercase());
                }
            }
        }
    }

    let from_const: BTreeSet<String> = APPROVED_TMP_EXTENSIONS
        .iter()
        .map(|e| e.to_ascii_lowercase())
        .collect();

    assert_eq!(
        from_allow, from_const,
        "APPROVED_TMP_EXTENSIONS must equal the /tmp extensions in \
         UNIVERSAL_ALLOW (src/prime_check.rs). Drift re-opens a native \
         permission prompt for the missing extension during an active flow."
    );
}

// --- get_file_path tests ---

#[test]
fn test_get_file_path_prefers_file_path() {
    let tool_input = json!({"file_path": "/some/path.py", "path": "/other/path"});
    assert_eq!(get_file_path(&tool_input), "/some/path.py");
}

#[test]
fn test_get_file_path_falls_back_to_path() {
    let tool_input = json!({"path": "/some/dir"});
    assert_eq!(get_file_path(&tool_input), "/some/dir");
}

#[test]
fn test_get_file_path_returns_empty_for_neither() {
    let tool_input = json!({"command": "something"});
    assert_eq!(get_file_path(&tool_input), "");
}

// --- is_shared_config tests ---

#[test]
fn test_shared_config_gitignore() {
    assert!(is_shared_config("/project/.worktrees/feat/.gitignore"));
}

#[test]
fn test_shared_config_gitattributes() {
    assert!(is_shared_config("/project/.worktrees/feat/.gitattributes"));
}

#[test]
fn test_shared_config_makefile() {
    assert!(is_shared_config("/project/.worktrees/feat/Makefile"));
}

#[test]
fn test_shared_config_rakefile() {
    assert!(is_shared_config("/project/.worktrees/feat/Rakefile"));
}

#[test]
fn test_shared_config_justfile() {
    assert!(is_shared_config("/project/.worktrees/feat/justfile"));
}

#[test]
fn test_shared_config_package_json() {
    assert!(is_shared_config("/project/.worktrees/feat/package.json"));
}

#[test]
fn test_shared_config_requirements_txt() {
    assert!(is_shared_config(
        "/project/.worktrees/feat/requirements.txt"
    ));
}

#[test]
fn test_shared_config_go_mod() {
    assert!(is_shared_config("/project/.worktrees/feat/go.mod"));
}

#[test]
fn test_shared_config_cargo_toml() {
    assert!(is_shared_config("/project/.worktrees/feat/Cargo.toml"));
}

#[test]
fn test_shared_config_github_directory() {
    assert!(is_shared_config(
        "/project/.worktrees/feat/.github/workflows/ci.yml"
    ));
}

#[test]
fn test_shared_config_github_codeowners() {
    assert!(is_shared_config(
        "/project/.worktrees/feat/.github/CODEOWNERS"
    ));
}

#[test]
fn test_shared_config_not_regular_file() {
    assert!(!is_shared_config("/project/.worktrees/feat/src/lib.rs"));
}

#[test]
fn test_shared_config_not_readme() {
    assert!(!is_shared_config("/project/.worktrees/feat/README.md"));
}

#[test]
fn test_shared_config_empty_path() {
    assert!(!is_shared_config(""));
}

#[test]
fn test_shared_config_case_sensitive_makefile() {
    assert!(!is_shared_config("/project/.worktrees/feat/makefile"));
}

#[test]
fn test_shared_config_github_directory_itself() {
    assert!(!is_shared_config("/project/.worktrees/feat/.github"));
}

// --- validate_shared_config tests ---

#[test]
fn test_shared_config_edit_gitignore_blocked() {
    let cwd = "/project/.worktrees/feat";
    let file_path = "/project/.worktrees/feat/.gitignore";
    let (allowed, msg) = validate_shared_config(file_path, cwd, "Edit");
    assert!(!allowed);
    assert!(msg.contains("shared configuration"));
    assert!(msg.contains("permissions.md"));
}

#[test]
fn test_shared_config_write_cargo_toml_blocked() {
    let cwd = "/project/.worktrees/feat";
    let file_path = "/project/.worktrees/feat/Cargo.toml";
    let (allowed, msg) = validate_shared_config(file_path, cwd, "Write");
    assert!(!allowed);
    assert!(msg.contains("shared configuration"));
}

#[test]
fn test_shared_config_read_gitignore_allowed() {
    let cwd = "/project/.worktrees/feat";
    let file_path = "/project/.worktrees/feat/.gitignore";
    let (allowed, msg) = validate_shared_config(file_path, cwd, "Read");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_shared_config_grep_github_allowed() {
    let cwd = "/project/.worktrees/feat";
    let file_path = "/project/.worktrees/feat/.github/workflows/ci.yml";
    let (allowed, msg) = validate_shared_config(file_path, cwd, "Grep");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_shared_config_edit_outside_worktree_allowed() {
    let cwd = "/project";
    let file_path = "/project/.gitignore";
    let (allowed, msg) = validate_shared_config(file_path, cwd, "Edit");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_shared_config_edit_regular_file_allowed() {
    let cwd = "/project/.worktrees/feat";
    let file_path = "/project/.worktrees/feat/src/lib.rs";
    let (allowed, msg) = validate_shared_config(file_path, cwd, "Edit");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_shared_config_empty_path_allowed() {
    let cwd = "/project/.worktrees/feat";
    let (allowed, msg) = validate_shared_config("", cwd, "Edit");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_shared_config_edit_main_repo_shared_allowed() {
    let cwd = "/project/.worktrees/feat";
    let file_path = "/project/.gitignore";
    let (allowed, msg) = validate_shared_config(file_path, cwd, "Edit");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_shared_config_edit_github_workflow_blocked() {
    let cwd = "/project/.worktrees/feat";
    let file_path = "/project/.worktrees/feat/.github/workflows/ci.yml";
    let (allowed, msg) = validate_shared_config(file_path, cwd, "Edit");
    assert!(!allowed);
    assert!(msg.contains("shared configuration"));
}

#[test]
fn test_shared_config_empty_tool_name_allowed() {
    let cwd = "/project/.worktrees/feat";
    let file_path = "/project/.worktrees/feat/.gitignore";
    let (allowed, msg) = validate_shared_config(file_path, cwd, "");
    assert!(allowed);
    assert!(msg.is_empty());
}

// --- validate_shared_config approval consult ---
//
// The "proceed" half: before the block return, validate_shared_config
// consults+consumes a single-use shared-config approval marker. A
// valid unconsumed marker for the target allows the edit exactly
// once; absence, corruption, per-file mismatch, or a second edit all
// keep blocking (fail-closed).

fn sc_fixture() -> (tempfile::TempDir, std::path::PathBuf, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let cwd = root.join(".worktrees").join("feat");
    fs::create_dir_all(&cwd).unwrap();
    let file_path = format!("{}/Cargo.toml", cwd.display());
    let cwd_s = cwd.to_string_lossy().to_string();
    (dir, root, cwd_s, file_path)
}

#[test]
fn shared_config_allows_with_valid_marker() {
    let (_d, root, cwd, file_path) = sc_fixture();
    flow_rs::shared_config_approval::write_approval(&root, "feat", &file_path).unwrap();
    let (allowed, msg) = validate_shared_config(&file_path, &cwd, "Edit");
    assert!(allowed, "valid marker must allow the edit");
    assert!(msg.is_empty());
}

#[test]
fn shared_config_blocks_without_marker() {
    let (_d, _root, cwd, file_path) = sc_fixture();
    let (allowed, msg) = validate_shared_config(&file_path, &cwd, "Edit");
    assert!(!allowed);
    assert!(msg.contains("is a shared configuration file that affects every engineer"));
    assert!(msg.contains("approve-shared-config"));
}

#[test]
fn shared_config_marker_is_single_use() {
    let (_d, root, cwd, file_path) = sc_fixture();
    flow_rs::shared_config_approval::write_approval(&root, "feat", &file_path).unwrap();
    let (a1, _) = validate_shared_config(&file_path, &cwd, "Edit");
    assert!(a1, "first edit consumes the marker");
    let (a2, msg2) = validate_shared_config(&file_path, &cwd, "Edit");
    assert!(!a2, "second edit re-blocks (marker consumed)");
    assert!(msg2.contains("approve-shared-config"));
}

#[test]
fn shared_config_blocks_on_corrupt_marker() {
    let (_d, root, cwd, file_path) = sc_fixture();
    let mpath = flow_rs::shared_config_approval::marker_path(&root, "feat", &file_path).unwrap();
    fs::create_dir_all(mpath.parent().unwrap()).unwrap();
    fs::write(&mpath, "{ not json").unwrap();
    let (allowed, _) = validate_shared_config(&file_path, &cwd, "Edit");
    assert!(!allowed, "corrupt marker must fail closed (still block)");
}

#[test]
fn shared_config_marker_is_per_file() {
    let (_d, root, cwd, file_path) = sc_fixture();
    // Marker granted for Cargo.toml; an Edit of .gitignore must
    // still block.
    flow_rs::shared_config_approval::write_approval(&root, "feat", &file_path).unwrap();
    let other = format!("{}/.gitignore", std::path::Path::new(&cwd).display());
    let (allowed, _) = validate_shared_config(&other, &cwd, "Edit");
    assert!(!allowed, "marker for Cargo.toml must not allow .gitignore");
    // Cargo.toml's marker is untouched and still usable.
    let (a, _) = validate_shared_config(&file_path, &cwd, "Edit");
    assert!(a);
}

#[test]
fn shared_config_marker_ignored_for_read_tool() {
    // Read is never blocked regardless of marker state — the
    // tool-name guard returns allow before the consult, so the
    // marker is NOT consumed by a Read.
    let (_d, root, cwd, file_path) = sc_fixture();
    flow_rs::shared_config_approval::write_approval(&root, "feat", &file_path).unwrap();
    let (allowed, _) = validate_shared_config(&file_path, &cwd, "Read");
    assert!(allowed);
    // Marker survives (was not consumed): an Edit can still use it.
    let (a, _) = validate_shared_config(&file_path, &cwd, "Edit");
    assert!(a, "Read must not consume the marker");
}

#[test]
fn shared_config_blocks_when_worktree_unresolvable() {
    // cwd contains ".worktrees/" (passes the flow-active proxy
    // check) but NOT "/.worktrees/" (no leading slash before the
    // marker), so compute_worktree_paths returns None and the
    // approval consult is skipped — fail-closed, still blocks.
    let cwd = "x.worktrees/feat";
    let file_path = "x.worktrees/feat/Cargo.toml";
    let (allowed, msg) = validate_shared_config(file_path, cwd, "Edit");
    assert!(!allowed);
    assert!(msg.contains("is a shared configuration file that affects every engineer"));
}

// Direct `run_impl_main` tests removed — decision core is now
// private. Its branches are driven through the subprocess tests
// below that spawn `bin/flow hook validate-worktree-paths`.

// --- run() subprocess smoke test ---

fn run_hook(stdin_input: &str) -> (i32, String, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = crate::common::spawn_hook(
        "validate-worktree-paths",
        dir.path(),
        stdin_input.as_bytes(),
        &[],
    );
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn run_subprocess_exits_0_with_no_hook_input() {
    // Empty stdin → read_hook_input returns None → exit 0.
    let (code, _, _) = run_hook("");
    assert_eq!(code, 0);
}

#[test]
fn run_subprocess_exits_0_with_empty_file_path() {
    let (code, _, _) = run_hook("{\"tool_input\": {}}");
    assert_eq!(code, 0);
}

/// Covers the `file_path == cwd` short-circuit path in
/// `validate_shared_config`: when the file_path is exactly the worktree
/// root (no `/` suffix), it should NOT be treated as a main-repo path
/// nor as a shared config file.
#[test]
fn test_shared_config_file_path_equals_cwd_allowed() {
    let cwd = "/project/.worktrees/feat";
    // file_path equals cwd — not starts_with cwd_prefix but equals cwd.
    let (allowed, _) = validate_shared_config(cwd, cwd, "Edit");
    // The cwd path itself is not a shared-config filename (it's a dir),
    // so allowed = true.
    assert!(allowed);
}

/// Drives the `None => return (0, None)` branch in `run_impl_main`'s
/// cwd match. The production wrapper reads cwd from
/// `std::env::current_dir().ok().map(...)`; forcing the subprocess's
/// cwd inode to be unlinked via `pre_exec` + `rmdir` makes
/// `getcwd(3)` return `ENOENT`, so the hook sees `cwd = None`.
#[cfg(unix)]
#[test]
fn run_subprocess_exits_0_when_current_dir_fails() {
    use std::os::unix::process::CommandExt;

    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let cwd = root.join("doomed");
    std::fs::create_dir(&cwd).expect("mkdir doomed");

    let preexec_path = std::ffi::CString::new(cwd.to_str().expect("utf8").as_bytes())
        .expect("CString from cwd path");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.args(["hook", "validate-worktree-paths"])
        .env_remove("FLOW_CI_RUNNING")
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // SAFETY: `libc::rmdir` is POSIX async-signal-safe; the closure
    // allocates no memory and does not panic.
    unsafe {
        cmd.pre_exec(move || {
            libc::rmdir(preexec_path.as_ptr());
            Ok(())
        });
    }

    let mut child = cmd.spawn().expect("spawn flow-rs");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(br#"{"tool_input":{"file_path":"/tmp/x"}}"#)
        .unwrap();
    let output = child.wait_with_output().expect("wait");
    // With cwd unresolvable, the hook exits 0 (no-op) because it
    // cannot validate a path it can't contextualize against a
    // worktree root.
    assert_eq!(output.status.code(), Some(0));
}

/// Drives the `validate_shared_config returns blocked` path of
/// `run_impl_main` (line 354 of src/hooks/validate_worktree_paths.rs:
/// `return (2, Some(sc_message));`). Spawns the hook from inside a
/// `.worktrees/<branch>/` cwd with stdin describing an Edit on a
/// shared config file (Cargo.toml) inside that worktree. validate()
/// passes (file is inside the worktree) so control reaches
/// validate_shared_config, which blocks because Edit on Cargo.toml
/// inside a `.worktrees/` cwd matches the shared-config gate.
#[test]
fn run_subprocess_exits_2_when_editing_shared_config_in_worktree() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    // Build a fake worktree: <root>/.worktrees/feat/
    let worktree = root.join(".worktrees").join("feat");
    std::fs::create_dir_all(&worktree).expect("mkdir worktree");
    // Create the shared-config file inside the worktree so file_path
    // resolves to a real existing path (defends against any future
    // path-existence check the hook might add).
    let cargo_toml = worktree.join("Cargo.toml");
    std::fs::write(&cargo_toml, b"[package]\nname=\"x\"\n").unwrap();

    let payload = format!(
        r#"{{"tool_name":"Edit","tool_input":{{"file_path":"{}"}}}}"#,
        cargo_toml.display()
    );

    let output = crate::common::spawn_hook(
        "validate-worktree-paths",
        &worktree,
        payload.as_bytes(),
        &[],
    );
    assert_eq!(
        output.status.code(),
        Some(2),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("is a shared configuration file"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// --- detect_misplaced_flow_states tests ---

#[test]
fn detect_misplaced_returns_none_for_canonical_path() {
    let result = detect_misplaced_flow_states(
        "/Users/ben/code/flow/.flow-states/foo.md",
        "/Users/ben/code/flow",
    );
    assert!(result.is_none());
}

#[test]
fn detect_misplaced_returns_canonical_for_worktree_root_flow_states() {
    let result = detect_misplaced_flow_states(
        "/Users/ben/code/flow/.worktrees/feat/.flow-states/foo.md",
        "/Users/ben/code/flow",
    );
    assert_eq!(
        result,
        Some("/Users/ben/code/flow/.flow-states/foo.md".to_string())
    );
}

#[test]
fn detect_misplaced_returns_canonical_for_service_subdir_flow_states() {
    let result = detect_misplaced_flow_states(
        "/Users/ben/code/flow/.worktrees/feat/api/.flow-states/foo.md",
        "/Users/ben/code/flow",
    );
    assert_eq!(
        result,
        Some("/Users/ben/code/flow/.flow-states/foo.md".to_string())
    );
}

#[test]
fn detect_misplaced_returns_canonical_for_deep_nested_flow_states() {
    let result = detect_misplaced_flow_states(
        "/Users/ben/code/flow/.worktrees/feat/api/sub/.flow-states/foo.md",
        "/Users/ben/code/flow",
    );
    assert_eq!(
        result,
        Some("/Users/ben/code/flow/.flow-states/foo.md".to_string())
    );
}

#[test]
fn detect_misplaced_returns_none_for_paths_without_flow_states() {
    let result = detect_misplaced_flow_states(
        "/Users/ben/code/flow/.worktrees/feat/src/main.rs",
        "/Users/ben/code/flow",
    );
    assert!(result.is_none());
}

#[test]
fn detect_misplaced_returns_none_for_paths_outside_project() {
    let result = detect_misplaced_flow_states("/home/user/.claude/foo", "/Users/ben/code/flow");
    assert!(result.is_none());
}

#[test]
fn detect_misplaced_returns_none_for_substring_match() {
    let result = detect_misplaced_flow_states(
        "/Users/ben/code/flow/.worktrees/feat/foo-flow-states-bar",
        "/Users/ben/code/flow",
    );
    assert!(result.is_none());
}

#[test]
fn detect_misplaced_returns_none_when_no_slash_after_worktrees_prefix() {
    // Path matches `<root>/.worktrees/` literally but has no `/` after
    // the branch token, so `after_worktrees.find('/')` returns None
    // and the helper short-circuits without trying to detect the
    // `.flow-states/` segment.
    let result = detect_misplaced_flow_states(
        "/Users/ben/code/flow/.worktrees/lonely-branch",
        "/Users/ben/code/flow",
    );
    assert!(result.is_none());
}

#[test]
fn detect_misplaced_matches_mixed_case_flow_states() {
    // macOS APFS is case-insensitive — `.Flow-States/foo.md` resolves
    // to the same inode as `.flow-states/foo.md`, so the helper must
    // match case-insensitively to uphold the canonical-only invariant.
    let result = detect_misplaced_flow_states(
        "/Users/ben/code/flow/.worktrees/feat/.Flow-States/plan.md",
        "/Users/ben/code/flow",
    );
    assert_eq!(
        result,
        Some("/Users/ben/code/flow/.flow-states/plan.md".to_string())
    );
}

#[test]
fn detect_misplaced_matches_uppercase_flow_states() {
    let result = detect_misplaced_flow_states(
        "/Users/ben/code/flow/.worktrees/feat/.FLOW-STATES/plan.md",
        "/Users/ben/code/flow",
    );
    assert_eq!(
        result,
        Some("/Users/ben/code/flow/.flow-states/plan.md".to_string())
    );
}

#[test]
fn detect_misplaced_collapses_doubled_slashes_in_input() {
    // A doubled slash between the project root and `.worktrees/` would
    // otherwise slip past the worktrees-prefix probe and fall through
    // to the generic main-repo block (which produces a recursive
    // worktree path in its redirect message).
    let result = detect_misplaced_flow_states(
        "/Users/ben/code/flow//.worktrees/feat/.flow-states/plan.md",
        "/Users/ben/code/flow",
    );
    assert_eq!(
        result,
        Some("/Users/ben/code/flow/.flow-states/plan.md".to_string())
    );
}

#[test]
fn detect_misplaced_sanitizes_traversal_segments_in_canonical() {
    // The block fires correctly, but the redirect message must not name
    // a path containing `..` segments — that would mislead the caller
    // toward path-traversal usage.
    let result = detect_misplaced_flow_states(
        "/Users/ben/code/flow/.worktrees/feat/.flow-states/../../etc/passwd",
        "/Users/ben/code/flow",
    );
    assert_eq!(
        result,
        Some("/Users/ben/code/flow/.flow-states/etc/passwd".to_string())
    );
}

#[test]
fn detect_misplaced_sanitizes_dot_segments_in_canonical() {
    let result = detect_misplaced_flow_states(
        "/Users/ben/code/flow/.worktrees/feat/.flow-states/./foo/./bar.md",
        "/Users/ben/code/flow",
    );
    assert_eq!(
        result,
        Some("/Users/ben/code/flow/.flow-states/foo/bar.md".to_string())
    );
}

#[test]
fn detect_misplaced_collapses_doubled_slashes_inside_suffix() {
    let result = detect_misplaced_flow_states(
        "/Users/ben/code/flow/.worktrees/feat/.flow-states//foo//bar.md",
        "/Users/ben/code/flow",
    );
    assert_eq!(
        result,
        Some("/Users/ben/code/flow/.flow-states/foo/bar.md".to_string())
    );
}

// --- validate() .flow-states/ canonicalization tests ---

#[test]
fn validate_rejects_worktree_flow_states_write() {
    let cwd = "/Users/ben/code/flow/.worktrees/feat/api";
    let file_path = "/Users/ben/code/flow/.worktrees/feat/.flow-states/plan.md";
    let (allowed, msg) = validate(file_path, cwd, "/Users/ben");
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains(".flow-states/"));
    assert!(msg.contains("/Users/ben/code/flow/.flow-states/plan.md"));
    assert!(msg.contains(file_path));
}

#[test]
fn validate_rejects_service_subdir_flow_states_write() {
    let cwd = "/Users/ben/code/flow/.worktrees/feat/api";
    let file_path = "/Users/ben/code/flow/.worktrees/feat/api/.flow-states/plan.md";
    let (allowed, msg) = validate(file_path, cwd, "/Users/ben");
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("/Users/ben/code/flow/.flow-states/plan.md"));
}

#[test]
fn validate_accepts_main_repo_flow_states_write_from_subdir_cwd() {
    let cwd = "/Users/ben/code/flow/.worktrees/feat/api";
    let file_path = "/Users/ben/code/flow/.flow-states/feat/plan.md";
    let (allowed, msg) = validate(file_path, cwd, "/Users/ben");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn validate_accepts_worktree_non_flow_states_write() {
    let cwd = "/Users/ben/code/flow/.worktrees/feat/api";
    let file_path = "/Users/ben/code/flow/.worktrees/feat/api/src/main.rs";
    let (allowed, msg) = validate(file_path, cwd, "/Users/ben");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn validate_plain_repo_no_op() {
    // No .worktrees/ in cwd — the new check is bypassed via the
    // pre-existing "not in a worktree" early-return.
    let cwd = "/Users/ben/code/flow";
    let file_path = "/Users/ben/code/flow/.flow-states/feat/plan.md";
    let (allowed, msg) = validate(file_path, cwd, "/Users/ben");
    assert!(allowed);
    assert!(msg.is_empty());
}

// --- subprocess matrix: .flow-states/ canonicalization per tool ---

/// Spawn the hook with stdin matching `tool_name` + `path_field`,
/// targeted at `file_path`, with cwd set to `worktree_cwd`.
///
/// Per `.claude/rules/subprocess-test-hygiene.md`: removes
/// `FLOW_CI_RUNNING` (the `bin/flow hook` family inherits the parent's
/// CI guard) and pins `HOME` to a tempdir so the child reads no user
/// dotfiles. Returns `(exit_code, stdout, stderr)`.
fn spawn_hook_with_cwd(
    worktree_cwd: &std::path::Path,
    home: &std::path::Path,
    tool_name: &str,
    path_field: &str,
    file_path: &str,
) -> (i32, String, String) {
    let stdin_input = format!(
        r#"{{"tool_name":"{}","tool_input":{{"{}":"{}"}}}}"#,
        tool_name, path_field, file_path
    );
    let output = crate::common::spawn_hook(
        "validate-worktree-paths",
        worktree_cwd,
        stdin_input.as_bytes(),
        &[("HOME", home.to_str().unwrap())],
    );
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

/// Build a fixture worktree at `<canonical_tmp>/.worktrees/feat` and
/// return `(root, worktree_cwd)`. Per
/// `.claude/rules/testing-gotchas.md` "macOS Subprocess Path
/// Canonicalization": canonicalize the tempdir root before any
/// descendant `join()` so the child's `current_dir()` and the
/// production `starts_with` prefix check agree.
fn worktree_fixture(tmp: &tempfile::TempDir) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = tmp.path().canonicalize().expect("canonicalize");
    let worktree_cwd = root.join(".worktrees").join("feat");
    std::fs::create_dir_all(&worktree_cwd).expect("mkdir worktree");
    (root, worktree_cwd)
}

// --- payload-cwd gate engagement (Task 6) ---

/// Spawn validate-worktree-paths with an explicit payload `cwd` field
/// while the process's real cwd (`.current_dir`) is a DIFFERENT
/// directory. Proves `run()` resolves the worktree from the payload
/// `cwd`, not `std::env::current_dir()`. Returns `(exit_code, stderr)`.
///
/// Env hygiene per `.claude/rules/subprocess-test-hygiene.md`: removes
/// `FLOW_CI_RUNNING` and pins `HOME` to `real_cwd` so the child reads
/// no user dotfiles.
fn spawn_with_payload_cwd(
    real_cwd: &std::path::Path,
    payload_cwd: &str,
    file_path: &str,
) -> (i32, String) {
    let stdin_input = format!(
        r#"{{"cwd":"{}","tool_name":"Read","tool_input":{{"file_path":"{}"}}}}"#,
        payload_cwd, file_path
    );
    let output = crate::common::spawn_hook(
        "validate-worktree-paths",
        real_cwd,
        stdin_input.as_bytes(),
        &[("HOME", real_cwd.to_str().unwrap())],
    );
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn validate_worktree_paths_reads_payload_cwd_engages_gate() {
    // The worktree gate must resolve the worktree from the payload
    // `cwd` field, not the hook's env::current_dir(). Real cwd is the
    // (non-worktree) project root where compute_worktree_paths returns
    // None and the gate self-disables; the payload cwd points at the
    // worktree. With the payload cwd honored, an out-of-worktree
    // file_path engages the gate (exit 2). If the hook read
    // env::current_dir() instead, the gate would self-disable and
    // allow (exit 0) — the Review-phase halt this fix targets.
    let tmp = tempfile::tempdir().expect("tempdir");
    let (root, worktree_cwd) = worktree_fixture(&tmp);
    let file_path = root.join("Cargo.toml");
    let (code, stderr) = spawn_with_payload_cwd(
        &root,                           // real cwd: non-worktree project root
        &worktree_cwd.to_string_lossy(), // payload cwd: the worktree
        &file_path.to_string_lossy(),    // out-of-worktree, in-project target
    );
    assert_eq!(
        code, 2,
        "payload cwd must engage the worktree gate on an out-of-worktree \
         path; stderr={stderr}"
    );
}

/// R3: drive run_impl_main's $HOME resolution into the memory-allow
/// path. The helper unit tests pass `home` directly; only a subprocess
/// test proves run_impl_main reads $HOME from the environment and
/// threads it into validate() so an out-of-project memory Read under
/// that $HOME is allowed (exit 0). Regression: a change that drops or
/// mis-resolves $HOME in run_impl_main would block the approved memory
/// surface even under a correct $HOME. The memory dir lives under a
/// separate tempdir from the worktree root so it is genuinely
/// out-of-project.
#[test]
fn validate_subprocess_memory_read_allowed_via_resolved_home() {
    let home_tmp = tempfile::tempdir().expect("home tempdir");
    let home = home_tmp.path().canonicalize().expect("canonicalize home");
    let wt_tmp = tempfile::tempdir().expect("worktree tempdir");
    let (_root, worktree_cwd) = worktree_fixture(&wt_tmp);
    let memory_path = format!("{}/.claude/projects/abc/memory/MEMORY.md", home.display());
    let (code, _stdout, stderr) =
        spawn_hook_with_cwd(&worktree_cwd, &home, "Read", "file_path", &memory_path);
    assert_eq!(
        code, 0,
        "an out-of-project memory Read under the resolved $HOME must be \
         allowed (exit 0); stderr: {}",
        stderr
    );
}

#[test]
fn validate_subprocess_rejects_worktree_flow_states_write() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (root, worktree_cwd) = worktree_fixture(&tmp);
    let target = worktree_cwd.join(".flow-states/plan.md");
    let canonical = root.join(".flow-states/plan.md");
    let (code, _, stderr) = spawn_hook_with_cwd(
        &worktree_cwd,
        &root,
        "Write",
        "file_path",
        target.to_str().unwrap(),
    );
    assert_eq!(code, 2, "stderr: {}", stderr);
    assert!(stderr.contains("BLOCKED"), "stderr: {}", stderr);
    assert!(stderr.contains(".flow-states/"), "stderr: {}", stderr);
    assert!(
        stderr.contains(canonical.to_str().unwrap()),
        "stderr: {}",
        stderr
    );
}

#[test]
fn validate_subprocess_rejects_worktree_flow_states_read() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (root, worktree_cwd) = worktree_fixture(&tmp);
    let target = worktree_cwd.join(".flow-states/plan.md");
    let canonical = root.join(".flow-states/plan.md");
    let (code, _, stderr) = spawn_hook_with_cwd(
        &worktree_cwd,
        &root,
        "Read",
        "file_path",
        target.to_str().unwrap(),
    );
    assert_eq!(code, 2, "stderr: {}", stderr);
    assert!(stderr.contains("BLOCKED"), "stderr: {}", stderr);
    assert!(
        stderr.contains(canonical.to_str().unwrap()),
        "stderr: {}",
        stderr
    );
}

#[test]
fn validate_subprocess_rejects_worktree_flow_states_edit() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (root, worktree_cwd) = worktree_fixture(&tmp);
    let target = worktree_cwd.join(".flow-states/plan.md");
    let canonical = root.join(".flow-states/plan.md");
    let (code, _, stderr) = spawn_hook_with_cwd(
        &worktree_cwd,
        &root,
        "Edit",
        "file_path",
        target.to_str().unwrap(),
    );
    assert_eq!(code, 2, "stderr: {}", stderr);
    assert!(stderr.contains("BLOCKED"), "stderr: {}", stderr);
    assert!(
        stderr.contains(canonical.to_str().unwrap()),
        "stderr: {}",
        stderr
    );
}

#[test]
fn validate_subprocess_rejects_worktree_flow_states_glob() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (root, worktree_cwd) = worktree_fixture(&tmp);
    // Glob/Grep use the `path` field rather than `file_path`.
    let target = worktree_cwd.join(".flow-states/plan.md");
    let canonical = root.join(".flow-states/plan.md");
    let (code, _, stderr) = spawn_hook_with_cwd(
        &worktree_cwd,
        &root,
        "Glob",
        "path",
        target.to_str().unwrap(),
    );
    assert_eq!(code, 2, "stderr: {}", stderr);
    assert!(stderr.contains("BLOCKED"), "stderr: {}", stderr);
    assert!(
        stderr.contains(canonical.to_str().unwrap()),
        "stderr: {}",
        stderr
    );
}

#[test]
fn validate_subprocess_rejects_worktree_flow_states_grep() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (root, worktree_cwd) = worktree_fixture(&tmp);
    let target = worktree_cwd.join(".flow-states/plan.md");
    let canonical = root.join(".flow-states/plan.md");
    let (code, _, stderr) = spawn_hook_with_cwd(
        &worktree_cwd,
        &root,
        "Grep",
        "path",
        target.to_str().unwrap(),
    );
    assert_eq!(code, 2, "stderr: {}", stderr);
    assert!(stderr.contains("BLOCKED"), "stderr: {}", stderr);
    assert!(
        stderr.contains(canonical.to_str().unwrap()),
        "stderr: {}",
        stderr
    );
}

#[test]
fn validate_subprocess_accepts_main_repo_flow_states_write() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (root, worktree_cwd) = worktree_fixture(&tmp);
    let target = root.join(".flow-states/feat/plan.md");
    let (code, _, stderr) = spawn_hook_with_cwd(
        &worktree_cwd,
        &root,
        "Write",
        "file_path",
        target.to_str().unwrap(),
    );
    assert_eq!(code, 0, "stderr: {}", stderr);
}

#[test]
fn validate_subprocess_accepts_main_repo_flow_states_read() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (root, worktree_cwd) = worktree_fixture(&tmp);
    let target = root.join(".flow-states/feat/plan.md");
    let (code, _, stderr) = spawn_hook_with_cwd(
        &worktree_cwd,
        &root,
        "Read",
        "file_path",
        target.to_str().unwrap(),
    );
    assert_eq!(code, 0, "stderr: {}", stderr);
}

#[test]
fn validate_subprocess_accepts_main_repo_flow_states_edit() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (root, worktree_cwd) = worktree_fixture(&tmp);
    let target = root.join(".flow-states/feat/plan.md");
    let (code, _, stderr) = spawn_hook_with_cwd(
        &worktree_cwd,
        &root,
        "Edit",
        "file_path",
        target.to_str().unwrap(),
    );
    assert_eq!(code, 0, "stderr: {}", stderr);
}

#[test]
fn validate_subprocess_accepts_main_repo_flow_states_glob() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (root, worktree_cwd) = worktree_fixture(&tmp);
    let target = root.join(".flow-states/feat/plan.md");
    let (code, _, stderr) = spawn_hook_with_cwd(
        &worktree_cwd,
        &root,
        "Glob",
        "path",
        target.to_str().unwrap(),
    );
    assert_eq!(code, 0, "stderr: {}", stderr);
}

#[test]
fn validate_subprocess_accepts_main_repo_flow_states_grep() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (root, worktree_cwd) = worktree_fixture(&tmp);
    let target = root.join(".flow-states/feat/plan.md");
    let (code, _, stderr) = spawn_hook_with_cwd(
        &worktree_cwd,
        &root,
        "Grep",
        "path",
        target.to_str().unwrap(),
    );
    assert_eq!(code, 0, "stderr: {}", stderr);
}

// --- build_rewrite_envelope tests ---

#[test]
fn build_rewrite_envelope_write_input_replaces_file_path_preserves_content() {
    let tool_input = json!({
        "file_path": "/proj/.worktrees/feat/.flow-states/plan.md",
        "content": "X"
    });
    let canonical = "/proj/.flow-states/plan.md";
    let env = build_rewrite_envelope(&tool_input, canonical).expect("object input yields envelope");
    let hso = &env["hookSpecificOutput"];
    assert_eq!(hso["hookEventName"], "PreToolUse");
    assert_eq!(hso["permissionDecision"], "allow");
    let updated = &hso["updatedInput"];
    assert_eq!(updated["file_path"], canonical);
    // Field-agnostic preservation: content survives the rewrite.
    assert_eq!(updated["content"], "X");
}

#[test]
fn build_rewrite_envelope_edit_input_preserves_all_non_path_fields() {
    let tool_input = json!({
        "file_path": "/proj/.worktrees/feat/.flow-states/plan.md",
        "old_string": "a",
        "new_string": "b",
        "replace_all": true
    });
    let canonical = "/proj/.flow-states/plan.md";
    let env = build_rewrite_envelope(&tool_input, canonical).expect("object input yields envelope");
    let updated = &env["hookSpecificOutput"]["updatedInput"];
    // Only file_path changes; every Edit field is preserved verbatim.
    assert_eq!(updated["file_path"], canonical);
    assert_eq!(updated["old_string"], "a");
    assert_eq!(updated["new_string"], "b");
    assert_eq!(updated["replace_all"], true);
}

#[test]
fn build_rewrite_envelope_non_object_input_returns_none() {
    // A non-object tool_input (a JSON string) cannot carry a file_path
    // key to overwrite, so the helper signals "no rewrite" and the
    // caller falls through to the block.
    let tool_input = json!("not-an-object");
    let result = build_rewrite_envelope(&tool_input, "/proj/.flow-states/plan.md");
    assert!(result.is_none());
}

// --- Presence contract: shared-config BLOCKED phrase ---

/// Presence-contract assertion that the literal substring
/// `"is a shared configuration file that affects every engineer"`
/// appears in the source of `src/hooks/validate_worktree_paths.rs`.
/// Named consumer:
/// `crate::hooks::transcript_walker::recent_edit_blocked_on_shared_config`,
/// which detects shared-config blocks by scanning tool_result content
/// for this exact substring. The phrase is intentionally long so the
/// detection signal cannot accidentally match unrelated error
/// messages (a permission-denied error, a generic "this file is
/// shared" warning) — the suffix "that affects every engineer"
/// scopes the match to the BLOCKED message format from
/// `validate_shared_config`. A refactor of that BLOCKED message that
/// drops the substring would silently break the validate-ask-user
/// shared-config carve-out — the helper would return false on every
/// transcript and the autonomous-phase block would deadlock the flow
/// when the model tries to confirm a shared-config edit. This test
/// is a presence contract (not a tombstone) because the assertion is
/// positive presence, not absence.
#[test]
fn validate_worktree_paths_emits_shared_config_phrase() {
    let content = std::fs::read_to_string("src/hooks/validate_worktree_paths.rs")
        .expect("validate_worktree_paths.rs source must be readable");
    assert!(
        content.contains("is a shared configuration file that affects every engineer"),
        "src/hooks/validate_worktree_paths.rs must emit the literal \
         substring \"is a shared configuration file that affects every engineer\" — \
         transcript_walker::recent_edit_blocked_on_shared_config \
         depends on this exact phrase as its detection signal"
    );
}
