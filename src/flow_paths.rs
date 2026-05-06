//! Centralized construction for FLOW-managed paths.
//!
//! Two types cover the `.flow-states/` access patterns, plus a free
//! function for `.worktrees/<branch>/` boundary computation:
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
//! - `compute_worktree_paths` / `compute_worktree_root` — derive the
//!   project root and worktree root from any cwd inside the worktree.
//!   `compute_worktree_paths` returns `Option<(project_root, worktree_root)>`
//!   for callers (the worktree-paths hook) that need both. The thin
//!   wrapper `compute_worktree_root` returns just the worktree root
//!   for callers (`ci::run_impl`) that only need to normalize cwd to
//!   the worktree boundary. Both are anchored on a leading `/` and
//!   match the rightmost occurrence so a project path containing
//!   `.worktrees/` as a non-marker component does not produce a false
//!   match.
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
    /// Returns true iff `branch` is a valid FLOW branch name. The
    /// branch is joined onto `.flow-states/` to construct the
    /// branch-scoped subdirectory `<flow_states_dir>/<branch>/`, so
    /// any value that would resolve outside that subdirectory must be
    /// rejected here. Cleanup runs `fs::remove_dir_all(branch_dir())`,
    /// so a path-traversal slip turns into arbitrary-directory
    /// deletion (`--branch ..` would target the project root,
    /// `--branch .` would target every sibling flow's subdirectory).
    ///
    /// Rejects:
    /// - empty string (cannot identify a flow)
    /// - `.` or `..` (path-traversal — `.flow-states/.` and
    ///   `.flow-states/..` resolve to `.flow-states/` and the project
    ///   root respectively, which cleanup would then `remove_dir_all`)
    /// - any string containing `/` (would create a subdirectory under
    ///   `.flow-states/<top>/...` that the discovery scanners cannot
    ///   find)
    /// - any string containing `\0` (NUL bytes round-trip through
    ///   filesystem syscalls in implementation-defined ways)
    pub fn is_valid_branch(branch: &str) -> bool {
        !branch.is_empty()
            && branch != "."
            && branch != ".."
            && !branch.contains('/')
            && !branch.contains('\0')
    }

    /// Construct a new `FlowPaths` rooted at `<project_root>/.flow-states`
    /// for the given branch.
    ///
    /// Panics if `branch` fails `is_valid_branch`. Use this when you
    /// know the branch is valid (e.g., it came from state file keyspace
    /// or was already checked). Use `try_new` for branches sourced from
    /// git (`current_branch()`, `resolve_branch()`) — those can carry
    /// slashes (`feature/foo`, `dependabot/*`) that must not panic.
    /// Use `FlowStatesDir` when an operation is genuinely branch-free.
    pub fn new(project_root: impl AsRef<Path>, branch: impl Into<String>) -> Self {
        let branch = branch.into();
        assert!(
            Self::is_valid_branch(&branch),
            "FlowPaths::new: invalid branch: {branch:?} (must be non-empty, \
             not '.' or '..', no '/' or NUL)"
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

    /// `<branch_dir>/state.json` — authoritative state file.
    pub fn state_file(&self) -> PathBuf {
        self.branch_dir().join("state.json")
    }

    /// `<branch_dir>/log` — session log appended by skills and Rust
    /// modules via `append_log`.
    pub fn log_file(&self) -> PathBuf {
        self.branch_dir().join("log")
    }

    /// `<branch_dir>/plan.md` — Plan phase output.
    pub fn plan_file(&self) -> PathBuf {
        self.branch_dir().join("plan.md")
    }

    /// `<branch_dir>/dag.md` — DAG decomposition output.
    pub fn dag_file(&self) -> PathBuf {
        self.branch_dir().join("dag.md")
    }

    /// `<branch_dir>/phases.json` — frozen phase config captured at
    /// flow-start time.
    pub fn frozen_phases(&self) -> PathBuf {
        self.branch_dir().join("phases.json")
    }

    /// `<branch_dir>/ci-passed` — CI sentinel; presence means the last
    /// `bin/flow ci` invocation passed for the current working tree.
    pub fn ci_sentinel(&self) -> PathBuf {
        self.branch_dir().join("ci-passed")
    }

    /// `<branch_dir>/timings.md` — phase timing report.
    pub fn timings_file(&self) -> PathBuf {
        self.branch_dir().join("timings.md")
    }

    /// `<branch_dir>/closed-issues.json` — issues closed during the
    /// flow, persisted for the post-merge close step.
    pub fn closed_issues(&self) -> PathBuf {
        self.branch_dir().join("closed-issues.json")
    }

    /// `<branch_dir>/issues.md` — issues summary rendered for PR body
    /// inclusion.
    pub fn issues_file(&self) -> PathBuf {
        self.branch_dir().join("issues.md")
    }

    /// `<branch_dir>/rule-content.md` — scratch file for rule-file
    /// edits routed through `bin/flow write-rule`.
    pub fn rule_content(&self) -> PathBuf {
        self.branch_dir().join("rule-content.md")
    }

    /// `<branch_dir>/commit-msg.txt` — final commit message file
    /// consumed by `bin/flow finalize-commit`. Branch-scoped so
    /// concurrent flows in different worktrees of the same repo never
    /// share a single file, and so abort/complete cleanup removes it
    /// deterministically alongside other branch-scoped state via the
    /// single `remove_dir_all` over `branch_dir()`.
    pub fn commit_msg(&self) -> PathBuf {
        self.branch_dir().join("commit-msg.txt")
    }

    /// `<branch_dir>/commit-msg-content.txt` — scratch file the commit
    /// skill writes via the Write tool, then `bin/flow write-rule`
    /// reads and routes to [`commit_msg`].
    pub fn commit_msg_content(&self) -> PathBuf {
        self.branch_dir().join("commit-msg-content.txt")
    }

    /// `<branch_dir>/start-prompt` — verbatim start prompt captured
    /// by `/flow:flow-start` for downstream phases.
    pub fn start_prompt(&self) -> PathBuf {
        self.branch_dir().join("start-prompt")
    }
}

/// Compute the project root and worktree root from a cwd that lives
/// somewhere inside the worktree.
///
/// Returns `Some((project_root, worktree_root))` where:
/// - `project_root` is the prefix of `cwd` before `/.worktrees/`
/// - `worktree_root` is the prefix of `cwd` ending at
///   `<project_root>/.worktrees/<branch>` (no trailing slash)
///
/// Returns `None` when `cwd` is not inside a `.worktrees/<branch>/`
/// subdirectory.
///
/// The match is **anchored to a leading slash** (`/.worktrees/`) so
/// substring shapes like `xx.worktrees/yy` (no leading `/`) do NOT
/// match. The match is **rightmost-occurrence** via `rfind` so a
/// project_root path that itself contains a `.worktrees/` directory
/// (e.g. `/home/dev/my.worktrees/myproject/.worktrees/feat`) resolves
/// to the FLOW worktree boundary, not the spurious match inside the
/// project_root.
///
/// Used by the worktree-paths hook (to derive both project_root and
/// worktree_root) and by `ci::run_impl` (which only needs worktree_root
/// — see `compute_worktree_root` thin wrapper).
///
/// Branches:
/// - cwd lacks `/.worktrees/` → `None`
/// - cwd ends with `.worktrees/` (no branch segment) → `None`
/// - cwd at worktree root (no subdir) → `Some((root, cwd))` (borrows
///   from input)
/// - cwd at worktree root with trailing slash → `Some((root, <root>/.worktrees/<branch>))`
///   (slash stripped)
/// - cwd at any subdir under the worktree → strips every segment after
///   the branch
///
/// Worktree directory names are git-branch-name-shaped via
/// `branch_name()` sanitization at flow-start, so the first `/` after
/// `.worktrees/` is the branch terminator.
pub fn compute_worktree_paths(cwd: &str) -> Option<(&str, &str)> {
    const WORKTREE_ANCHOR: &str = "/.worktrees/";
    let slash_pos = cwd.rfind(WORKTREE_ANCHOR)?;
    let project_root = &cwd[..slash_pos];
    let after_anchor = slash_pos + WORKTREE_ANCHOR.len();
    let after_worktrees = &cwd[after_anchor..];
    if after_worktrees.is_empty() {
        return None;
    }
    let branch_end = after_worktrees.find('/').unwrap_or(after_worktrees.len());
    let worktree_root = &cwd[..after_anchor + branch_end];
    Some((project_root, worktree_root))
}

/// Compute the worktree root from a cwd that lives somewhere inside it.
///
/// Thin wrapper over `compute_worktree_paths` for callers that only
/// need the worktree root (e.g. `ci::run_impl` cwd normalization).
/// See `compute_worktree_paths` for full semantics including the
/// leading-slash anchor and rightmost-occurrence behavior.
pub fn compute_worktree_root(cwd: &str) -> Option<&str> {
    compute_worktree_paths(cwd).map(|(_, worktree_root)| worktree_root)
}
