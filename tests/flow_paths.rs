//! Integration tests for `src/flow_paths.rs`. Covers `FlowPaths`
//! construction, filename suffixes, branch validation, and the
//! `FlowStatesDir` helper. All tests live here per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` in
//! `src/flow_paths.rs`.

use std::path::{Path, PathBuf};

use flow_rs::flow_paths::{
    compute_worktree_paths, compute_worktree_root, FlowPaths, FlowStatesDir,
};

// --- FlowPaths ---

fn paths() -> FlowPaths {
    FlowPaths::new("/tmp/project", "my-feature")
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
fn dag_file_lives_under_branch_dir() {
    assert_eq!(
        paths().dag_file(),
        PathBuf::from("/tmp/project/.flow-states/my-feature/dag.md")
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
fn commit_msg_lives_under_branch_dir() {
    assert_eq!(
        paths().commit_msg(),
        PathBuf::from("/tmp/project/.flow-states/my-feature/commit-msg.txt")
    );
}

#[test]
fn commit_msg_content_lives_under_branch_dir() {
    assert_eq!(
        paths().commit_msg_content(),
        PathBuf::from("/tmp/project/.flow-states/my-feature/commit-msg-content.txt")
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
fn accepts_pathbuf_and_path_for_project_root() {
    let p1 = FlowPaths::new(PathBuf::from("/p"), "b");
    let p2 = FlowPaths::new(Path::new("/p"), "b");
    assert_eq!(p1.state_file(), p2.state_file());
}

#[test]
fn accepts_owned_and_borrowed_branch() {
    let b = String::from("branch-x");
    let p1 = FlowPaths::new("/p", b.clone());
    let p2 = FlowPaths::new("/p", b.as_str());
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

#[test]
#[should_panic(expected = "invalid branch")]
fn new_panics_on_empty_branch() {
    let _ = FlowPaths::new("/p", "");
}

#[test]
#[should_panic(expected = "invalid branch")]
fn new_panics_on_branch_with_single_slash() {
    let _ = FlowPaths::new("/p", "user/fix");
}

#[test]
#[should_panic]
fn new_panics_on_branch_with_multiple_slashes() {
    let _ = FlowPaths::new("/p", "a/b/c");
}

#[test]
#[should_panic]
fn new_panics_on_branch_that_is_just_a_slash() {
    let _ = FlowPaths::new("/p", "/");
}

#[test]
#[should_panic]
fn new_panics_on_trailing_slash() {
    let _ = FlowPaths::new("/p", "a/");
}

#[test]
#[should_panic]
fn new_panics_on_leading_slash() {
    let _ = FlowPaths::new("/p", "/a");
}

// --- branch_dir + ensure_branch_dir ---

#[test]
fn branch_dir_returns_branch_subdirectory_under_flow_states_dir() {
    let p = FlowPaths::new("/tmp/project", "feature-foo");
    assert_eq!(
        p.branch_dir(),
        PathBuf::from("/tmp/project/.flow-states/feature-foo")
    );
}

#[test]
fn ensure_branch_dir_creates_directory_when_missing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let p = FlowPaths::new(tmp.path(), "feature-foo");
    assert!(!p.branch_dir().exists());
    p.ensure_branch_dir().expect("ensure_branch_dir succeeds");
    assert!(p.branch_dir().is_dir());
}

#[test]
fn ensure_branch_dir_idempotent_on_existing_directory() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let p = FlowPaths::new(tmp.path(), "feature-foo");
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
    let p = FlowPaths::new(tmp.path(), "feature-foo");
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

// --- Path-traversal rejection (PR #1258 security gate) ---
//
// `.` and `..` segments would resolve outside the per-branch
// subdirectory once joined onto `.flow-states/`, turning
// `cleanup`'s `remove_dir_all(branch_dir())` into arbitrary
// directory deletion. NUL bytes survive into filesystem syscalls
// in implementation-defined ways. All four rejections must hit
// `is_valid_branch` so `try_new` returns None and the panicking
// `new` constructor's assertion fires.

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

#[test]
#[should_panic(expected = "invalid branch")]
fn new_panics_on_dot_branch() {
    let _ = FlowPaths::new("/p", ".");
}

#[test]
#[should_panic(expected = "invalid branch")]
fn new_panics_on_dot_dot_branch() {
    let _ = FlowPaths::new("/p", "..");
}

#[test]
#[should_panic(expected = "invalid branch")]
fn new_panics_on_nul_branch() {
    let _ = FlowPaths::new("/p", "with\0nul");
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
