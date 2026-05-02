//! Centralized construction for `.flow-states/` paths.
//!
//! Two types cover the two access patterns:
//!
//! - `FlowStatesDir` — directory-only. Use for cross-branch operations
//!   (discovery scans, hook prefix checks, pre-lock queue paths) that
//!   need the `.flow-states/` directory without a specific branch.
//! - `FlowPaths` — branch-scoped. Use when addressing a per-branch
//!   file (`state_file`, `log_file`, `plan_file`, etc.). The
//!   constructor panics on empty or slash-containing branches because
//!   those shapes produce malformed paths; `try_new` is the fallible
//!   variant for callers that receive branches from git and cannot
//!   guarantee validity.
//!
//! `FlowPaths` also exposes `flow_states_dir()` for callers that
//! already hold a branch-scoped instance and incidentally need the
//! directory — standalone directory access belongs in `FlowStatesDir`.
//! Filename suffixes live here so the on-disk layout can change by
//! editing this module alone.

use std::fs;
use std::io;
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
    /// Returns true iff `branch` is a valid FLOW branch name — non-empty
    /// and contains no '/'. FLOW branch-scoped files are flat filenames
    /// under `.flow-states/`, so slashes would produce subdirectory paths
    /// that discovery scanners iterating direct children cannot find.
    pub fn is_valid_branch(branch: &str) -> bool {
        !branch.is_empty() && !branch.contains('/')
    }

    /// Construct a new `FlowPaths` rooted at `<project_root>/.flow-states`
    /// for the given branch.
    ///
    /// Panics if `branch` is empty or contains '/'. Use this when you
    /// know the branch is valid (e.g., it came from state file keyspace
    /// or was already checked). Use `try_new` for branches sourced from
    /// git (`current_branch()`, `resolve_branch()`) — those can carry
    /// slashes (`feature/foo`, `dependabot/*`) that must not panic.
    /// Use `FlowStatesDir` when an operation is genuinely branch-free.
    pub fn new(project_root: impl AsRef<Path>, branch: impl Into<String>) -> Self {
        let branch = branch.into();
        assert!(
            !branch.is_empty(),
            "FlowPaths::new: branch must be non-empty"
        );
        assert!(
            !branch.contains('/'),
            "FlowPaths::new: branch must not contain '/': {branch}"
        );
        Self {
            flow_states_dir: project_root.as_ref().join(".flow-states"),
            branch,
        }
    }

    /// Fallible constructor — returns `None` when `branch` fails
    /// `is_valid_branch`. Callers that receive branches from external
    /// sources (git, user input) should use this instead of `new` to
    /// treat invalid branches as "no active flow" rather than panicking.
    pub fn try_new(project_root: impl AsRef<Path>, branch: impl Into<String>) -> Option<Self> {
        let branch = branch.into();
        if !Self::is_valid_branch(&branch) {
            return None;
        }
        Some(Self {
            flow_states_dir: project_root.as_ref().join(".flow-states"),
            branch,
        })
    }

    /// The branch this instance is scoped to.
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// The `.flow-states/` directory at the project root. Retained for
    /// callers that already hold a `FlowPaths` instance and need the
    /// directory incidentally (directory creation before writing a
    /// branch-scoped file, directory listing alongside branch-scoped
    /// cleanup). For standalone cross-branch directory access, use
    /// `FlowStatesDir` directly — it avoids the need to pick a branch
    /// just to reach the directory.
    pub fn flow_states_dir(&self) -> PathBuf {
        self.flow_states_dir.clone()
    }

    /// `<.flow-states>/<branch>/` — branch-scoped subdirectory that
    /// houses every per-branch artifact (state file, log, plan, DAG,
    /// commit message, etc.). Cleanup walks this directory, and flow
    /// discovery scans the `.flow-states/` directory for subdirectories
    /// containing a `state.json` rather than enumerating per-suffix
    /// filenames.
    pub fn branch_dir(&self) -> PathBuf {
        self.flow_states_dir.join(&self.branch)
    }

    /// Create `<.flow-states>/<branch>/` if it does not already exist.
    /// Idempotent — wraps `fs::create_dir_all`. Callers that write
    /// branch-scoped files (init_state, start_init writing
    /// `start_prompt`) must call this before the first `fs::write` so
    /// the parent directory exists. Errors propagate so callers can
    /// surface filesystem failures (e.g., a regular file blocking the
    /// branch path) instead of silently swallowing them.
    pub fn ensure_branch_dir(&self) -> io::Result<()> {
        fs::create_dir_all(self.branch_dir())
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

    /// `<.flow-states>/<branch>-commit-msg.txt` — final commit message
    /// file consumed by `bin/flow finalize-commit`. Branch-scoped under
    /// `.flow-states/` so concurrent flows in different worktrees of the
    /// same repo never share a single file, and so abort/complete cleanup
    /// removes it deterministically alongside other branch-scoped state.
    pub fn commit_msg(&self) -> PathBuf {
        self.flow_states_dir
            .join(format!("{}-commit-msg.txt", self.branch))
    }

    /// `<.flow-states>/<branch>-commit-msg-content.txt` — scratch file
    /// the commit skill writes via the Write tool, then `bin/flow
    /// write-rule` reads and routes to [`commit_msg`].
    pub fn commit_msg_content(&self) -> PathBuf {
        self.flow_states_dir
            .join(format!("{}-commit-msg-content.txt", self.branch))
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
