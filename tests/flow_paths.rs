//! Integration tests for `src/flow_paths.rs`. Covers `FlowPaths`
//! construction, filename suffixes, branch validation, and the
//! `FlowStatesDir` helper. All tests live here per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` in
//! `src/flow_paths.rs`.

use std::path::{Path, PathBuf};
use std::process::Command;

use flow_rs::flow_paths::{
    compute_worktree_paths, compute_worktree_root, finalize_commit_destination,
    is_autonomous_flow_active, FlowPaths, FlowStatesDir,
};

mod common;

// --- FlowPaths ---

fn paths() -> FlowPaths {
    FlowPaths::try_new("/tmp/project", "my-feature").expect("test fixture branch is valid")
}

#[test]
fn branch_returns_configured_branch() {
    assert_eq!(paths().branch(), "my-feature");
}

#[test]
fn flow_states_dir_is_project_root_dot_flow_states() {
    assert_eq!(
        paths().flow_states_dir(),
        PathBuf::from("/tmp/project/.flow-states")
    );
}

#[test]
fn state_file_lives_under_branch_dir() {
    assert_eq!(
        paths().state_file(),
        PathBuf::from("/tmp/project/.flow-states/my-feature/state.json")
    );
}

#[test]
fn log_file_lives_under_branch_dir() {
    assert_eq!(
        paths().log_file(),
        PathBuf::from("/tmp/project/.flow-states/my-feature/log")
    );
}

#[test]
fn plan_file_lives_under_branch_dir() {
    assert_eq!(
        paths().plan_file(),
        PathBuf::from("/tmp/project/.flow-states/my-feature/plan.md")
    );
}

#[test]
fn frozen_phases_lives_under_branch_dir() {
    assert_eq!(
        paths().frozen_phases(),
        PathBuf::from("/tmp/project/.flow-states/my-feature/phases.json")
    );
}

#[test]
fn ci_sentinel_lives_under_branch_dir() {
    assert_eq!(
        paths().ci_sentinel(),
        PathBuf::from("/tmp/project/.flow-states/my-feature/ci-passed")
    );
}

#[test]
fn timings_file_lives_under_branch_dir() {
    assert_eq!(
        paths().timings_file(),
        PathBuf::from("/tmp/project/.flow-states/my-feature/timings.md")
    );
}

#[test]
fn closed_issues_lives_under_branch_dir() {
    assert_eq!(
        paths().closed_issues(),
        PathBuf::from("/tmp/project/.flow-states/my-feature/closed-issues.json")
    );
}

#[test]
fn issues_file_lives_under_branch_dir() {
    assert_eq!(
        paths().issues_file(),
        PathBuf::from("/tmp/project/.flow-states/my-feature/issues.md")
    );
}

#[test]
fn rule_content_lives_under_branch_dir() {
    assert_eq!(
        paths().rule_content(),
        PathBuf::from("/tmp/project/.flow-states/my-feature/rule-content.md")
    );
}

#[test]
fn start_prompt_lives_under_branch_dir() {
    assert_eq!(
        paths().start_prompt(),
        PathBuf::from("/tmp/project/.flow-states/my-feature/start-prompt")
    );
}

#[test]
fn flow_paths_worktree_returns_main_root_dot_worktrees_branch() {
    let p = FlowPaths::try_new("/tmp/project", "my-feature").expect("valid branch");
    assert_eq!(
        p.worktree(),
        PathBuf::from("/tmp/project/.worktrees/my-feature")
    );
}

#[test]
fn accepts_pathbuf_and_path_for_project_root() {
    let p1 = FlowPaths::try_new(PathBuf::from("/p"), "b").expect("valid branch");
    let p2 = FlowPaths::try_new(Path::new("/p"), "b").expect("valid branch");
    assert_eq!(p1.state_file(), p2.state_file());
}

#[test]
fn accepts_owned_and_borrowed_branch() {
    let b = String::from("branch-x");
    let p1 = FlowPaths::try_new("/p", b.clone()).expect("valid branch");
    let p2 = FlowPaths::try_new("/p", b.as_str()).expect("valid branch");
    assert_eq!(p1.state_file(), p2.state_file());
}

#[test]
fn clone_preserves_fields() {
    let original = paths();
    let cloned = original.clone();
    assert_eq!(original.state_file(), cloned.state_file());
    assert_eq!(original.branch(), cloned.branch());
}

#[test]
fn debug_format_contains_branch() {
    // Exercises the derived Debug impl on FlowPaths.
    let p = paths();
    let dbg = format!("{:?}", p);
    assert!(dbg.contains("my-feature"));
}

// --- branch_dir + ensure_branch_dir ---

#[test]
fn branch_dir_returns_branch_subdirectory_under_flow_states_dir() {
    let p = FlowPaths::try_new("/tmp/project", "feature-foo").expect("valid branch");
    assert_eq!(
        p.branch_dir(),
        PathBuf::from("/tmp/project/.flow-states/feature-foo")
    );
}

#[test]
fn ensure_branch_dir_creates_directory_when_missing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let p = FlowPaths::try_new(tmp.path(), "feature-foo").expect("valid branch");
    assert!(!p.branch_dir().exists());
    p.ensure_branch_dir().expect("ensure_branch_dir succeeds");
    assert!(p.branch_dir().is_dir());
}

#[test]
fn ensure_branch_dir_idempotent_on_existing_directory() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let p = FlowPaths::try_new(tmp.path(), "feature-foo").expect("valid branch");
    p.ensure_branch_dir().expect("first call succeeds");
    p.ensure_branch_dir().expect("second call is idempotent");
    assert!(p.branch_dir().is_dir());
}

#[test]
fn ensure_branch_dir_propagates_io_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let flow_states = tmp.path().join(".flow-states");
    std::fs::create_dir_all(&flow_states).expect("create .flow-states");
    let collision = flow_states.join("feature-foo");
    std::fs::write(&collision, b"blocking file").expect("write blocking file");
    let p = FlowPaths::try_new(tmp.path(), "feature-foo").expect("valid branch");
    let err = p
        .ensure_branch_dir()
        .expect_err("ensure_branch_dir must fail when path is a regular file");
    let _ = err.kind();
}

// --- is_valid_branch + try_new ---

#[test]
fn is_valid_branch_accepts_plain_name() {
    assert!(FlowPaths::is_valid_branch("my-feature"));
}

#[test]
fn is_valid_branch_rejects_empty_string() {
    assert!(!FlowPaths::is_valid_branch(""));
}

#[test]
fn is_valid_branch_rejects_single_slash() {
    assert!(!FlowPaths::is_valid_branch("feature/foo"));
}

#[test]
fn is_valid_branch_rejects_multi_slash() {
    assert!(!FlowPaths::is_valid_branch("dependabot/npm/acme-1.2"));
}

#[test]
fn is_valid_branch_rejects_leading_and_trailing_slash() {
    assert!(!FlowPaths::is_valid_branch("/a"));
    assert!(!FlowPaths::is_valid_branch("a/"));
    assert!(!FlowPaths::is_valid_branch("/"));
}

#[test]
fn try_new_returns_some_for_valid_branch() {
    let p = FlowPaths::try_new("/p", "my-feature");
    assert!(p.is_some());
    assert_eq!(
        p.unwrap().state_file(),
        PathBuf::from("/p/.flow-states/my-feature/state.json")
    );
}

#[test]
fn try_new_returns_none_for_empty_branch() {
    assert!(FlowPaths::try_new("/p", "").is_none());
}

#[test]
fn try_new_returns_none_for_slash_branch() {
    assert!(FlowPaths::try_new("/p", "feature/foo").is_none());
}

#[test]
fn try_new_returns_none_for_multi_slash_branch() {
    assert!(FlowPaths::try_new("/p", "a/b/c").is_none());
}

#[test]
fn try_new_with_empty_project_root_defaults_to_slash() {
    // Empty project_root triggers the defensive `PathBuf::from("/")`
    // fallback so worktree() / state_file() etc. produce absolute
    // paths instead of cwd-relative paths. Production callers pass
    // canonical absolute roots; this branch guards test fixtures and
    // degenerate inputs from silent routing defects.
    let paths = FlowPaths::try_new("", "feature-foo").expect("valid branch");
    assert!(
        paths.state_file().is_absolute(),
        "empty project_root must produce absolute paths, got: {:?}",
        paths.state_file()
    );
    assert!(
        paths.state_file().starts_with("/"),
        "empty project_root must default to `/`, got: {:?}",
        paths.state_file()
    );
}

// --- Path-traversal rejection (PR #1258 security gate) ---
//
// `.` and `..` segments would resolve outside the per-branch
// subdirectory once joined onto `.flow-states/`, turning
// `cleanup`'s `remove_dir_all(branch_dir())` into arbitrary
// directory deletion. NUL bytes survive into filesystem syscalls
// in implementation-defined ways. All four rejections must hit
// `is_valid_branch` so `try_new` returns None.

#[test]
fn is_valid_branch_rejects_dot() {
    assert!(!FlowPaths::is_valid_branch("."));
}

#[test]
fn is_valid_branch_rejects_dot_dot() {
    assert!(!FlowPaths::is_valid_branch(".."));
}

#[test]
fn is_valid_branch_rejects_nul_byte() {
    assert!(!FlowPaths::is_valid_branch("foo\0bar"));
}

#[test]
fn try_new_returns_none_for_dot_branch() {
    assert!(FlowPaths::try_new("/p", ".").is_none());
}

#[test]
fn try_new_returns_none_for_dot_dot_branch() {
    assert!(FlowPaths::try_new("/p", "..").is_none());
}

#[test]
fn try_new_returns_none_for_nul_branch() {
    assert!(FlowPaths::try_new("/p", "branch\0name").is_none());
}

// --- is_safe_relative_cwd ---
//
// Per `.claude/rules/external-input-path-construction.md`, a state-
// derived `relative_cwd` value flowing into `Path::join` and into the
// `cd "<worktree_cwd>"` shell-bearing instruction must pass a positive
// validator. These tests pin the validator's accept/reject surface for
// every rejection class the rule names: empty (allowed — root sentinel),
// single and nested non-empty paths (allowed), absolute paths (rejected),
// `..` and `.` segments (rejected), NUL bytes (rejected), `"`
// (rejected). Consumers: `cwd_scope::enforce`, `phase_enter::run_impl`.

#[test]
fn is_safe_relative_cwd_accepts_empty() {
    assert!(FlowPaths::is_safe_relative_cwd(""));
}

#[test]
fn is_safe_relative_cwd_accepts_single_component() {
    assert!(FlowPaths::is_safe_relative_cwd("api"));
}

#[test]
fn is_safe_relative_cwd_accepts_nested_components() {
    assert!(FlowPaths::is_safe_relative_cwd("packages/api"));
}

#[test]
fn is_safe_relative_cwd_rejects_absolute_path() {
    assert!(!FlowPaths::is_safe_relative_cwd("/etc"));
}

#[test]
fn is_safe_relative_cwd_rejects_backslash_absolute() {
    assert!(!FlowPaths::is_safe_relative_cwd("\\windows"));
}

#[test]
fn is_safe_relative_cwd_rejects_parent_traversal() {
    assert!(!FlowPaths::is_safe_relative_cwd(".."));
}

#[test]
fn is_safe_relative_cwd_rejects_parent_in_middle() {
    assert!(!FlowPaths::is_safe_relative_cwd("api/../etc"));
}

#[test]
fn is_safe_relative_cwd_rejects_dot_segment() {
    assert!(!FlowPaths::is_safe_relative_cwd("."));
}

#[test]
fn is_safe_relative_cwd_rejects_dot_in_middle() {
    assert!(!FlowPaths::is_safe_relative_cwd("api/./b"));
}

#[test]
fn is_safe_relative_cwd_rejects_nul_byte() {
    assert!(!FlowPaths::is_safe_relative_cwd("api\0b"));
}

#[test]
fn is_safe_relative_cwd_rejects_double_quote() {
    assert!(!FlowPaths::is_safe_relative_cwd("api\"b"));
}

#[test]
fn is_safe_relative_cwd_rejects_trailing_slash() {
    assert!(!FlowPaths::is_safe_relative_cwd("api/"));
}

#[test]
fn is_safe_relative_cwd_rejects_double_slash() {
    assert!(!FlowPaths::is_safe_relative_cwd("api//b"));
}

// --- FlowStatesDir ---

#[test]
fn flow_states_dir_new_returns_dot_flow_states_under_root() {
    let d = FlowStatesDir::new("/tmp/project");
    assert_eq!(d.path(), Path::new("/tmp/project/.flow-states"));
}

#[test]
fn flow_states_dir_accepts_path_and_pathbuf_for_root() {
    let d1 = FlowStatesDir::new(PathBuf::from("/p"));
    let d2 = FlowStatesDir::new(Path::new("/p"));
    assert_eq!(d1.path(), d2.path());
}

#[test]
fn flow_states_dir_path_returns_borrowed_path() {
    let d = FlowStatesDir::new("/p");
    let p1: &Path = d.path();
    let p2: &Path = d.path();
    assert_eq!(p1, p2);
}

#[test]
fn flow_states_dir_clone_preserves_path() {
    let original = FlowStatesDir::new("/tmp/project");
    let cloned = original.clone();
    assert_eq!(original.path(), cloned.path());
}

#[test]
fn flow_states_dir_debug_format_contains_path() {
    let d = FlowStatesDir::new("/tmp/project");
    let dbg = format!("{:?}", d);
    assert!(dbg.contains("flow-states"));
}

// --- compute_worktree_root ---

#[test]
fn compute_worktree_root_returns_none_when_no_marker() {
    assert_eq!(compute_worktree_root("/Users/ben/code/flow"), None);
}

#[test]
fn compute_worktree_root_returns_none_when_no_branch_segment() {
    assert_eq!(
        compute_worktree_root("/Users/ben/code/flow/.worktrees/"),
        None
    );
}

#[test]
fn compute_worktree_root_at_worktree_root_no_slash() {
    let cwd = "/Users/ben/code/flow/.worktrees/my-feature";
    assert_eq!(compute_worktree_root(cwd), Some(cwd));
}

#[test]
fn compute_worktree_root_at_worktree_root_trailing_slash() {
    let cwd = "/Users/ben/code/flow/.worktrees/my-feature/";
    assert_eq!(
        compute_worktree_root(cwd),
        Some("/Users/ben/code/flow/.worktrees/my-feature")
    );
}

#[test]
fn compute_worktree_root_strips_single_subdir() {
    assert_eq!(
        compute_worktree_root("/Users/ben/code/flow/.worktrees/my-feature/synapse"),
        Some("/Users/ben/code/flow/.worktrees/my-feature")
    );
}

#[test]
fn compute_worktree_root_strips_multi_subdir() {
    assert_eq!(
        compute_worktree_root("/Users/ben/code/flow/.worktrees/my-feature/packages/api"),
        Some("/Users/ben/code/flow/.worktrees/my-feature")
    );
}

// Regression: project paths that contain `.worktrees/` as a non-marker
// substring (no leading slash) must NOT be matched. Previous `find` /
// first-occurrence semantics could resolve `xx.worktrees/yy` against
// the `.worktrees/` substring; the leading-slash anchor on rfind
// prevents this so the helper returns None for non-anchored shapes.
#[test]
fn compute_worktree_root_rejects_unanchored_substring_match() {
    assert_eq!(compute_worktree_root("/foo/xx.worktrees/yy"), None);
    assert_eq!(compute_worktree_root("/abc.worktrees/feat"), None);
}

// Regression for PM-F2: project_root path that itself contains a
// `.worktrees/` directory before the FLOW worktree must resolve to
// the outermost (rightmost) `/.worktrees/` boundary, not the spurious
// match in the project_root. With first-occurrence (find) semantics,
// the helper would truncate at the wrong boundary; rfind anchors on
// the FLOW worktree.
#[test]
fn compute_worktree_root_uses_rightmost_worktrees_segment() {
    assert_eq!(
        compute_worktree_root("/home/dev/my.worktrees/myproject/.worktrees/feat/cortex"),
        Some("/home/dev/my.worktrees/myproject/.worktrees/feat")
    );
}

// --- compute_worktree_paths ---

#[test]
fn compute_worktree_paths_returns_project_and_worktree_roots() {
    let cwd = "/Users/ben/code/flow/.worktrees/my-feature/cortex";
    assert_eq!(
        compute_worktree_paths(cwd),
        Some((
            "/Users/ben/code/flow",
            "/Users/ben/code/flow/.worktrees/my-feature"
        ))
    );
}

#[test]
fn compute_worktree_paths_returns_none_for_unanchored_substring() {
    assert_eq!(compute_worktree_paths("/foo/xx.worktrees/yy"), None);
}

#[test]
fn compute_worktree_paths_uses_rightmost_anchor() {
    let cwd = "/home/dev/my.worktrees/myproject/.worktrees/feat";
    assert_eq!(
        compute_worktree_paths(cwd),
        Some((
            "/home/dev/my.worktrees/myproject",
            "/home/dev/my.worktrees/myproject/.worktrees/feat"
        ))
    );
}

#[test]
fn compute_worktree_paths_returns_none_when_no_branch_segment() {
    assert_eq!(
        compute_worktree_paths("/Users/ben/code/flow/.worktrees/"),
        None
    );
}

// --- finalize_commit_destination ---
//
// `finalize_commit_destination` decides where a finalize-commit git
// operation runs. Integration-branch commits (the bootstrap skills
// that commit on the trunk) route to the project root, where the
// integration branch is checked out. Feature-branch commits route to
// the per-branch worktree. When git cannot resolve the integration
// branch, the worktree-existence fallback distinguishes a real
// feature flow (worktree dir present -> worktree) from a fresh-clone
// bootstrap (no worktree dir -> project root). The four tests below
// pin each arm of that decision, including the normalize-both-sides
// contract on the branch comparison.

/// Create a plain `git init` repo with no `origin` remote at
/// `<parent>/no-origin`. `git symbolic-ref refs/remotes/origin/HEAD`
/// has no target in such a repo, so `default_branch_in` returns
/// `Err` — exercising the worktree-existence fallback arms.
fn create_git_repo_no_origin(parent: &Path) -> PathBuf {
    let repo = parent.join("no-origin");
    std::fs::create_dir_all(&repo).expect("create repo dir");
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(&repo)
        .output()
        .expect("git init");
    repo
}

#[test]
fn finalize_commit_destination_integration_branch_routes_to_root() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let parent = tmp.path().canonicalize().expect("canonicalize");
    let root = common::create_git_repo_with_remote(&parent);
    assert_eq!(finalize_commit_destination(&root, "main"), root);
}

#[test]
fn finalize_commit_destination_integration_branch_case_variant_routes_to_root() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let parent = tmp.path().canonicalize().expect("canonicalize");
    let root = common::create_git_repo_with_remote(&parent);
    assert_eq!(finalize_commit_destination(&root, "MAIN"), root);
}

#[test]
fn finalize_commit_destination_integration_branch_whitespace_routes_to_root() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let parent = tmp.path().canonicalize().expect("canonicalize");
    let root = common::create_git_repo_with_remote(&parent);
    assert_eq!(finalize_commit_destination(&root, " main "), root);
}

#[test]
fn finalize_commit_destination_feature_branch_routes_to_worktree() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let parent = tmp.path().canonicalize().expect("canonicalize");
    let root = common::create_git_repo_with_remote(&parent);
    assert_eq!(
        finalize_commit_destination(&root, "feat-x"),
        root.join(".worktrees").join("feat-x")
    );
}

#[test]
fn finalize_commit_destination_no_origin_with_worktree_routes_to_worktree() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let parent = tmp.path().canonicalize().expect("canonicalize");
    let root = create_git_repo_no_origin(&parent);
    std::fs::create_dir_all(root.join(".worktrees").join("feat-x")).expect("create worktree dir");
    assert_eq!(
        finalize_commit_destination(&root, "feat-x"),
        root.join(".worktrees").join("feat-x")
    );
}

// Divergence regression: when git cannot name the integration
// branch (no `origin`), the helper must NEVER route to the project
// root — even with no worktree directory present. Routing to root
// here was the hook/binary drift the Review adversarial agent
// proved: the binary committed feature work onto the trunk while
// the hook's `default_branch_in().ok()?` returned no-block. The
// `Err` path now routes to the per-branch worktree on both sides.
#[test]
fn finalize_commit_destination_no_origin_no_worktree_routes_to_worktree() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let parent = tmp.path().canonicalize().expect("canonicalize");
    let root = create_git_repo_no_origin(&parent);
    assert_eq!(
        finalize_commit_destination(&root, "feat-x"),
        root.join(".worktrees").join("feat-x")
    );
}

// Branch-path-safety boundary (per
// `.claude/rules/branch-path-safety.md`): `finalize_commit_destination`
// is a pub surface that joins `branch` onto `.worktrees/`. An
// invalid branch must never reach that join — the `is_valid_branch`
// guard returns the project root (a non-escaping path) instead of
// constructing a traversal-shaped `<root>/.worktrees/<bad>`. Both
// production callers validate upstream, so this guard is
// defense-in-depth; these tests lock the rejection classes the
// adversarial harness could not reach (no crate link).

#[test]
fn finalize_commit_destination_rejects_empty_branch() {
    let root = Path::new("/tmp/proj");
    assert_eq!(finalize_commit_destination(root, ""), root);
}

#[test]
fn finalize_commit_destination_rejects_dot_branch() {
    let root = Path::new("/tmp/proj");
    assert_eq!(finalize_commit_destination(root, "."), root);
}

#[test]
fn finalize_commit_destination_rejects_dot_dot_branch() {
    let root = Path::new("/tmp/proj");
    assert_eq!(finalize_commit_destination(root, ".."), root);
}

#[test]
fn finalize_commit_destination_rejects_slash_branch() {
    let root = Path::new("/tmp/proj");
    assert_eq!(finalize_commit_destination(root, "a/b"), root);
}

#[test]
fn finalize_commit_destination_rejects_nul_branch() {
    let root = Path::new("/tmp/proj");
    assert_eq!(finalize_commit_destination(root, "a\0b"), root);
}

// --- is_autonomous_flow_active ---

/// Write a state file at `<root>/.flow-states/<branch>/state.json`
/// with the given JSON content.
fn write_state(root: &Path, branch: &str, content: &str) {
    let dir = root.join(".flow-states").join(branch);
    std::fs::create_dir_all(&dir).expect("create flow-states dir");
    std::fs::write(dir.join("state.json"), content).expect("write state.json");
}

#[test]
fn autonomous_flow_active_returns_false_for_missing_state_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    // No state file written.
    assert!(!is_autonomous_flow_active(&root, Some("feat")));
}

#[test]
fn autonomous_flow_active_returns_true_for_auto_in_progress() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    write_state(
        &root,
        "feat",
        r#"{
            "current_phase": "flow-code",
            "phases": {"flow-code": {"status": "in_progress"}},
            "skills": {"flow-code": {"continue": "auto"}}
        }"#,
    );
    assert!(is_autonomous_flow_active(&root, Some("feat")));
}

#[test]
fn autonomous_flow_active_returns_false_for_manual_phase() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    write_state(
        &root,
        "feat",
        r#"{
            "current_phase": "flow-code",
            "phases": {"flow-code": {"status": "in_progress"}},
            "skills": {"flow-code": {"continue": "manual"}}
        }"#,
    );
    assert!(!is_autonomous_flow_active(&root, Some("feat")));
}

#[test]
fn autonomous_flow_active_returns_false_for_completed_phase() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    write_state(
        &root,
        "feat",
        r#"{
            "current_phase": "flow-code",
            "phases": {"flow-code": {"status": "complete"}},
            "skills": {"flow-code": {"continue": "auto"}}
        }"#,
    );
    assert!(!is_autonomous_flow_active(&root, Some("feat")));
}

#[test]
fn autonomous_flow_active_fails_open_on_corrupt_state_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    write_state(&root, "feat", "{not valid json");
    assert!(!is_autonomous_flow_active(&root, Some("feat")));
}

#[test]
fn autonomous_flow_active_fails_open_on_non_utf8_state_file() {
    // Open succeeds, read_to_string fails on invalid UTF-8 — exercises
    // the second `?` operator in read_state_file_capped.
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let dir = root.join(".flow-states").join("feat");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("state.json"), [0xFF, 0xFE, 0xFD]).unwrap();
    assert!(!is_autonomous_flow_active(&root, Some("feat")));
}

#[test]
fn autonomous_flow_active_returns_false_for_none_branch() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    assert!(!is_autonomous_flow_active(&root, None));
}

#[test]
fn autonomous_flow_active_returns_false_for_invalid_branch() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    // Slash-containing branch fails FlowPaths::is_valid_branch.
    assert!(!is_autonomous_flow_active(&root, Some("foo/bar")));
}

#[test]
fn autonomous_flow_active_returns_false_when_current_phase_missing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    write_state(&root, "feat", "{}");
    assert!(!is_autonomous_flow_active(&root, Some("feat")));
}

#[test]
fn autonomous_flow_active_returns_false_when_current_phase_empty_string() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    write_state(&root, "feat", r#"{"current_phase": ""}"#);
    assert!(!is_autonomous_flow_active(&root, Some("feat")));
}

#[test]
fn autonomous_flow_active_returns_false_when_skills_entry_missing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    write_state(
        &root,
        "feat",
        r#"{
            "current_phase": "flow-code",
            "phases": {"flow-code": {"status": "in_progress"}}
        }"#,
    );
    assert!(!is_autonomous_flow_active(&root, Some("feat")));
}

#[test]
fn autonomous_flow_active_returns_false_when_skill_object_lacks_continue() {
    // Detailed-shape skills.<phase> object that's missing the
    // `continue` key — exercises the second match arm's `None`
    // result inside the v.get("continue") chain.
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    write_state(
        &root,
        "feat",
        r#"{
            "current_phase": "flow-code",
            "phases": {"flow-code": {"status": "in_progress"}},
            "skills": {"flow-code": {}}
        }"#,
    );
    assert!(!is_autonomous_flow_active(&root, Some("feat")));
}

#[test]
fn autonomous_flow_active_accepts_bare_string_auto_skill_config() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    write_state(
        &root,
        "feat",
        r#"{
            "current_phase": "flow-code",
            "phases": {"flow-code": {"status": "in_progress"}},
            "skills": {"flow-code": "auto"}
        }"#,
    );
    assert!(is_autonomous_flow_active(&root, Some("feat")));
}
