//! Integration tests for `src/flow_paths.rs`. Covers `FlowPaths`
//! construction, filename suffixes, branch validation, and the
//! `FlowStatesDir` helper. All tests live here per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` in
//! `src/flow_paths.rs`.

use std::path::{Path, PathBuf};

use flow_rs::flow_paths::{FlowPaths, FlowStatesDir};

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
fn state_file_has_json_suffix() {
    assert_eq!(
        paths().state_file(),
        PathBuf::from("/tmp/project/.flow-states/my-feature.json")
    );
}

#[test]
fn log_file_has_log_suffix() {
    assert_eq!(
        paths().log_file(),
        PathBuf::from("/tmp/project/.flow-states/my-feature.log")
    );
}

#[test]
fn plan_file_has_plan_md_suffix() {
    assert_eq!(
        paths().plan_file(),
        PathBuf::from("/tmp/project/.flow-states/my-feature-plan.md")
    );
}

#[test]
fn dag_file_has_dag_md_suffix() {
    assert_eq!(
        paths().dag_file(),
        PathBuf::from("/tmp/project/.flow-states/my-feature-dag.md")
    );
}

#[test]
fn frozen_phases_has_phases_json_suffix() {
    assert_eq!(
        paths().frozen_phases(),
        PathBuf::from("/tmp/project/.flow-states/my-feature-phases.json")
    );
}

#[test]
fn ci_sentinel_has_ci_passed_suffix() {
    assert_eq!(
        paths().ci_sentinel(),
        PathBuf::from("/tmp/project/.flow-states/my-feature-ci-passed")
    );
}

#[test]
fn timings_file_has_timings_md_suffix() {
    assert_eq!(
        paths().timings_file(),
        PathBuf::from("/tmp/project/.flow-states/my-feature-timings.md")
    );
}

#[test]
fn closed_issues_has_closed_issues_json_suffix() {
    assert_eq!(
        paths().closed_issues(),
        PathBuf::from("/tmp/project/.flow-states/my-feature-closed-issues.json")
    );
}

#[test]
fn issues_file_has_issues_md_suffix() {
    assert_eq!(
        paths().issues_file(),
        PathBuf::from("/tmp/project/.flow-states/my-feature-issues.md")
    );
}

#[test]
fn rule_content_has_rule_content_md_suffix() {
    assert_eq!(
        paths().rule_content(),
        PathBuf::from("/tmp/project/.flow-states/my-feature-rule-content.md")
    );
}

#[test]
fn commit_msg_has_commit_msg_txt_suffix() {
    assert_eq!(
        paths().commit_msg(),
        PathBuf::from("/tmp/project/.flow-states/my-feature-commit-msg.txt")
    );
}

#[test]
fn commit_msg_content_has_commit_msg_content_txt_suffix() {
    assert_eq!(
        paths().commit_msg_content(),
        PathBuf::from("/tmp/project/.flow-states/my-feature-commit-msg-content.txt")
    );
}

#[test]
fn start_prompt_has_start_prompt_suffix() {
    assert_eq!(
        paths().start_prompt(),
        PathBuf::from("/tmp/project/.flow-states/my-feature-start-prompt")
    );
}

#[test]
fn adversarial_test_prefix_ends_in_dot() {
    assert_eq!(
        paths().adversarial_test_prefix(),
        "my-feature-adversarial_test."
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
#[should_panic(expected = "non-empty")]
fn new_panics_on_empty_branch() {
    let _ = FlowPaths::new("/p", "");
}

#[test]
#[should_panic(expected = "must not contain")]
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
        PathBuf::from("/p/.flow-states/my-feature.json")
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
