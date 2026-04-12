//! Centralized construction for branch-scoped `.flow-states/*` paths.
//!
//! Every branch-scoped file under `.flow-states/` is addressed through a
//! single `FlowPaths` instance constructed from the project root and the
//! active branch name. Callers stay agnostic to the filename suffixes, so
//! the on-disk layout can change by editing this module alone.
//!
//! The struct also exposes `flow_states_dir()` for consumers that need
//! the parent directory (e.g. flow-discovery globs) and `branch()` for
//! code that still needs the branch name after constructing paths.

use std::path::{Path, PathBuf};

/// Directory-only handle for the `.flow-states/` directory. Use this
/// for cross-branch operations (discovery scans, hook prefix checks,
/// pre-lock queue paths) that need the directory without a specific
/// branch. Pairs with `FlowPaths` for branch-scoped access.
#[derive(Debug, Clone)]
pub struct FlowStatesDir {
    path: PathBuf,
}

impl FlowStatesDir {
    /// Construct a handle to `<project_root>/.flow-states/`.
    pub fn new(project_root: impl AsRef<Path>) -> Self {
        Self {
            path: project_root.as_ref().join(".flow-states"),
        }
    }

    /// Borrow the `.flow-states/` path. Callers that need an owned
    /// `PathBuf` can `.to_path_buf()` it.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Branch-scoped `.flow-states/*` path builder.
#[derive(Debug, Clone)]
pub struct FlowPaths {
    flow_states_dir: PathBuf,
    branch: String,
}

impl FlowPaths {
    /// Construct a new `FlowPaths` rooted at `<project_root>/.flow-states`
    /// for the given branch.
    pub fn new(project_root: impl AsRef<Path>, branch: impl Into<String>) -> Self {
        Self {
            flow_states_dir: project_root.as_ref().join(".flow-states"),
            branch: branch.into(),
        }
    }

    /// The branch this instance is scoped to.
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// The `.flow-states/` directory at the project root. Use this for
    /// cross-branch discovery (e.g. globbing `*.json`) — prefer the
    /// named file accessors for branch-scoped reads and writes.
    pub fn flow_states_dir(&self) -> PathBuf {
        self.flow_states_dir.clone()
    }

    /// `<.flow-states>/<branch>.json` — authoritative state file.
    pub fn state_file(&self) -> PathBuf {
        self.flow_states_dir.join(format!("{}.json", self.branch))
    }

    /// `<.flow-states>/<branch>.log` — session log appended by skills
    /// and Rust modules via `append_log`.
    pub fn log_file(&self) -> PathBuf {
        self.flow_states_dir.join(format!("{}.log", self.branch))
    }

    /// `<.flow-states>/<branch>-plan.md` — Plan phase output.
    pub fn plan_file(&self) -> PathBuf {
        self.flow_states_dir
            .join(format!("{}-plan.md", self.branch))
    }

    /// `<.flow-states>/<branch>-dag.md` — DAG decomposition output.
    pub fn dag_file(&self) -> PathBuf {
        self.flow_states_dir.join(format!("{}-dag.md", self.branch))
    }

    /// `<.flow-states>/<branch>-phases.json` — frozen phase config
    /// captured at flow-start time.
    pub fn frozen_phases(&self) -> PathBuf {
        self.flow_states_dir
            .join(format!("{}-phases.json", self.branch))
    }

    /// `<.flow-states>/<branch>-ci-passed` — CI sentinel; presence
    /// means the last `bin/flow ci` invocation passed for the current
    /// working tree.
    pub fn ci_sentinel(&self) -> PathBuf {
        self.flow_states_dir
            .join(format!("{}-ci-passed", self.branch))
    }

    /// `<.flow-states>/<branch>-timings.md` — phase timing report.
    pub fn timings_file(&self) -> PathBuf {
        self.flow_states_dir
            .join(format!("{}-timings.md", self.branch))
    }

    /// `<.flow-states>/<branch>-closed-issues.json` — issues closed
    /// during the flow, persisted for the post-merge close step.
    pub fn closed_issues(&self) -> PathBuf {
        self.flow_states_dir
            .join(format!("{}-closed-issues.json", self.branch))
    }

    /// `<.flow-states>/<branch>-issues.md` — issues summary rendered
    /// for PR body inclusion.
    pub fn issues_file(&self) -> PathBuf {
        self.flow_states_dir
            .join(format!("{}-issues.md", self.branch))
    }

    /// `<.flow-states>/<branch>-rule-content.md` — scratch file for
    /// rule-file edits routed through `bin/flow write-rule`.
    pub fn rule_content(&self) -> PathBuf {
        self.flow_states_dir
            .join(format!("{}-rule-content.md", self.branch))
    }

    /// `<.flow-states>/<branch>-start-prompt` — verbatim start prompt
    /// captured by `/flow:flow-start` for downstream phases.
    pub fn start_prompt(&self) -> PathBuf {
        self.flow_states_dir
            .join(format!("{}-start-prompt", self.branch))
    }

    /// Bare prefix `<branch>-adversarial_test.` used to glob Phase 4
    /// adversarial test files. The agent chooses the extension at
    /// runtime so callers filter `fs::read_dir` entries by this
    /// prefix rather than addressing a fixed filename.
    pub fn adversarial_test_prefix(&self) -> String {
        format!("{}-adversarial_test.", self.branch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
    fn branch_with_slashes_is_preserved_literally() {
        // Branch names with slashes (e.g. "user/fix") would produce
        // subdirectory-shaped filenames. FlowPaths passes the branch
        // through unchanged; sanitization belongs upstream.
        let p = FlowPaths::new("/p", "user/fix");
        assert_eq!(
            p.state_file(),
            PathBuf::from("/p/.flow-states/user/fix.json")
        );
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
        // path() returns &Path — borrow the same instance twice.
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
}
