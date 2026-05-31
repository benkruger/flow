//! Centralized construction for FLOW-managed paths.
//!
//! Two types cover the `.flow-states/` access patterns, plus free
//! functions for `.worktrees/<branch>/` boundary computation and
//! finalize-commit destination routing:
//!
//! - `FlowStatesDir` — directory-only. Use for cross-branch operations
//!   (discovery scans, hook prefix checks, pre-lock queue paths) that
//!   need the `.flow-states/` directory without a specific branch.
//! - `FlowPaths` — branch-scoped. Use when addressing a per-branch
//!   file (`state_file`, `log_file`, `plan_file`, etc.). The only
//!   constructor is `try_new`, an `Option`-returning variant that
//!   filters branches via `is_valid_branch` so empty,
//!   slash-containing, NUL-bearing, and `.`/`..` traversal segments
//!   never reach path construction. Callers that hold a branch known
//!   to be valid upstream call `try_new(...).expect("<boundary>")`
//!   with a doc-comment naming the upstream sanitizer; callers that
//!   receive a branch from git or a CLI override pattern-match on
//!   the `Option` and treat `None` as "no active flow".
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
//! - `finalize_commit_destination` — the Layer 10 commit gate's pure
//!   `branch == integration` predicate: returns the project root for
//!   an integration-branch / bootstrap commit and the per-branch
//!   worktree for everything else. This is the hook's block decision
//!   only — it is not the finalize-commit binary's physical router,
//!   which resolves the commit cwd from git's actual checkout location
//!   via `crate::git::resolve_worktree_for_branch`. The two agree on
//!   the route-to-root case because a trunk commit is, per git,
//!   checked out at the project root.
//!
//! `FlowPaths` also exposes `flow_states_dir()` for callers that
//! already hold a branch-scoped instance and incidentally need the
//! directory — standalone directory access belongs in `FlowStatesDir`.
//! Filename suffixes live here so the on-disk layout can change by
//! editing this module alone.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::git::default_branch_in;
use crate::hooks::transcript_walker::normalize_gate_input;

/// Byte cap for `<root>/.flow-states/<branch>/state.json` reads in
/// `is_autonomous_flow_active`. 8 MB matches the cap in
/// `src/hooks/validate_pretool.rs` per
/// `.claude/rules/external-input-path-construction.md` so a
/// corrupted or maliciously-large state file cannot OOM the hook
/// path. Reads above the cap are silently truncated;
/// `serde_json::from_str` downstream rejects mid-token truncations
/// and the predicate falls through to its `false` result.
const STATE_FILE_BYTE_CAP: u64 = 8 * 1024 * 1024;

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
///
/// Holds the project root and a validated branch name. The project
/// root is absolutized at construction so `worktree()` and every
/// other branch-scoped path returns an absolute `PathBuf` regardless
/// of whether the caller passed an empty, relative, or absolute
/// project root. The absolutization guarantee is what
/// `src/finalize_commit.rs::run_impl` relies on: running git in a
/// relative worktree path would resolve against the process cwd and
/// defeat branch-derived routing entirely.
#[derive(Debug, Clone)]
pub struct FlowPaths {
    project_root: PathBuf,
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

    /// Positive validator for `relative_cwd` values read from the
    /// state file. Per
    /// `.claude/rules/external-input-path-construction.md`, every
    /// state-derived string that flows into `Path::join` or a shell-
    /// bearing literal must pass a positive validator before
    /// construction. Accepts:
    ///
    /// - the empty string (the root-flow sentinel)
    /// - non-empty paths whose every `/`-separated component is
    ///   non-empty and is not `.` or `..`
    ///
    /// Rejects:
    ///
    /// - leading `/` or `\` (absolute paths — `Path::join` would
    ///   replace `worktree_path` entirely)
    /// - any `..` or `.` component (path traversal)
    /// - empty components from leading or duplicate slashes
    /// - `\0` (NUL bytes truncate paths in implementation-defined
    ///   ways)
    /// - `"` (double quotes break shell-bearing interpolation in
    ///   `cd "<worktree_cwd>"` and the cwd_scope error message)
    pub fn is_safe_relative_cwd(s: &str) -> bool {
        if s.is_empty() {
            return true;
        }
        if s.starts_with('/') || s.starts_with('\\') {
            return false;
        }
        if s.contains('\0') || s.contains('"') {
            return false;
        }
        for component in s.split('/') {
            if component.is_empty() || component == "." || component == ".." {
                return false;
            }
        }
        true
    }

    /// Sole constructor — returns `Some(FlowPaths)` when `branch`
    /// passes `is_valid_branch`, `None` otherwise. Callers that hold
    /// a branch already validated upstream (state-file keyspace,
    /// `branch_name()` output) chain `.expect("<boundary>")` with a
    /// doc-comment naming the sanitizer. Callers that receive a
    /// branch from git (`current_branch()`, `resolve_branch()`) or
    /// from a CLI override pattern-match the `Option` and treat
    /// `None` as "no active flow on this branch" — the same posture
    /// as the detached-HEAD branch in those callers. Use
    /// `FlowStatesDir` when an operation is genuinely branch-free.
    pub fn try_new(project_root: impl AsRef<Path>, branch: impl Into<String>) -> Option<Self> {
        let branch = branch.into();
        if !Self::is_valid_branch(&branch) {
            return None;
        }
        // Treat empty project_root as `/` so worktree() always
        // produces an absolute path. A degenerate empty input
        // would otherwise produce relative paths that resolve
        // against the process cwd at use time — a silent routing
        // defect for callers that change directory (the
        // finalize-commit path changes cwd to the worktree for
        // every git operation). Production callers pass canonical
        // absolute roots from `project_root()`; this branch
        // defends test fixtures and degenerate inputs.
        let root_ref = project_root.as_ref();
        let project_root = if root_ref.as_os_str().is_empty() {
            PathBuf::from("/")
        } else {
            root_ref.to_path_buf()
        };
        let flow_states_dir = project_root.join(".flow-states");
        Some(Self {
            project_root,
            flow_states_dir,
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
    /// houses every per-branch artifact (state file, log, plan,
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

    /// `<project_root>/.worktrees/<branch>/` — the git worktree
    /// directory FLOW creates for this branch at flow-start. Derived
    /// from the project root and branch name held by this instance,
    /// so callers that already validated the branch via `try_new`
    /// inherit a `/`-free, `\0`-free, non-empty component. The
    /// returned path is always absolute because `try_new`
    /// absolutizes `project_root` at construction time.
    ///
    /// Named production consumer: `src/finalize_commit.rs::run_impl`
    /// uses this to route every git operation, the CI sub-invocation,
    /// and the tree snapshot through the worktree path derived from
    /// the explicit `<branch>` argument — independent of the caller's
    /// cwd.
    pub fn worktree(&self) -> PathBuf {
        self.project_root.join(".worktrees").join(&self.branch)
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

/// Predicate: does the state file at
/// `<project_root>/.flow-states/<branch>/state.json` describe an
/// active autonomous phase?
///
/// Returns `true` iff ALL of:
///
/// - `branch` is `Some` and passes `FlowPaths::is_valid_branch` (the
///   path-construction boundary required by
///   `.claude/rules/branch-path-safety.md`).
/// - The state file exists and parses as JSON within the
///   `STATE_FILE_BYTE_CAP` byte cap.
/// - `current_phase` is non-empty AND
///   `phases.<current_phase>.status` equals `"in_progress"` (normalized
///   per `.claude/rules/security-gates.md` "Normalize Before
///   Comparing").
/// - `skills.<current_phase>` resolves to `auto` — accepted in two
///   shapes per `src/state.rs::SkillConfig`: the bare string form
///   (`"auto"`) and the object form (`{"continue": "auto", ...}`).
///   Comparisons run on normalized strings.
///
/// Fail-open on every error class — missing file, IO failure,
/// non-JSON content, missing or wrong-type field — returns `false`
/// per `.claude/rules/state-files.md` "Corruption Resilience" and
/// `.claude/rules/hook-state-timing.md`. The `in_progress` status
/// check is the stability guard for the transition-boundary window
/// where `current_phase` has advanced but `phase_enter` has not yet
/// flipped the new phase's status.
///
/// Sibling predicates compute the same "autonomous + in-progress"
/// condition for their own gates:
/// `validate_ask_user::validate` (the AskUserQuestion block) and
/// `stop_continue::check_autonomous_stop` (the Stop refusal), both
/// documented in `.claude/rules/autonomous-phase-discipline.md`
/// "Enforcement". This copy adds `normalize_gate_input` + the byte
/// cap because it reads the state file fresh from a path-construction
/// boundary; a future change to the autonomous-in-progress definition
/// must be applied to all three.
pub fn is_autonomous_flow_active(project_root: &Path, branch: Option<&str>) -> bool {
    let branch = match branch {
        Some(b) => b,
        None => return false,
    };
    let paths = match FlowPaths::try_new(project_root, branch) {
        Some(p) => p,
        None => return false,
    };
    let content = match read_state_file_capped(&paths.state_file()) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let state: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let current_phase = match state.get("current_phase").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return false,
    };
    let status = state
        .get("phases")
        .and_then(|p| p.get(&current_phase))
        .and_then(|p| p.get("status"))
        .and_then(|v| v.as_str())
        .map(normalize_gate_input)
        .unwrap_or_default();
    if status != "in_progress" {
        return false;
    }
    let skill_entry = state.get("skills").and_then(|s| s.get(&current_phase));
    match skill_entry {
        Some(v) if v.as_str().map(normalize_gate_input).as_deref() == Some("auto") => true,
        Some(v) => {
            v.get("continue")
                .and_then(|c| c.as_str())
                .map(normalize_gate_input)
                .as_deref()
                == Some("auto")
        }
        None => false,
    }
}

/// Read a state file with the documented `STATE_FILE_BYTE_CAP`.
fn read_state_file_capped(path: &Path) -> io::Result<String> {
    use std::io::Read;
    let file = fs::File::open(path)?;
    let mut buf = String::new();
    file.take(STATE_FILE_BYTE_CAP).read_to_string(&mut buf)?;
    Ok(buf)
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

/// The Layer 10 commit gate's block predicate: would a finalize-commit
/// on `branch` land on the integration trunk (the project `root`), or
/// on a per-branch worktree?
///
/// The integration branch (the trunk the bootstrap skills —
/// flow-start, flow-prime, flow-release — commit on) is checked out
/// at the project root itself; every other branch's work lives in
/// its per-branch `<root>/.worktrees/<branch>/` worktree. This
/// helper returns the project root for an integration-branch commit
/// and the per-branch worktree for everything else.
///
/// This answers a question for the hook only — it is NOT the
/// finalize-commit binary's physical router. `finalize_commit::run_impl`
/// resolves its commit cwd from git's actual checkout location via
/// `crate::git::resolve_worktree_for_branch`. This predicate exists so
/// `match_finalize_commit_destination` can decide whether a commit on
/// `branch` would target the trunk and therefore needs gating.
///
/// The ONLY route-to-root case is: `crate::git::default_branch_in(root)`
/// is `Ok(integration)` AND `branch` normalizes equal to
/// `integration` (`normalize_gate_input` on both sides per
/// `.claude/rules/security-gates.md` "Normalize Before Comparing").
/// Every other input — a feature branch with a resolvable
/// integration branch, OR `default_branch_in` erring because git
/// cannot name the integration branch (no `origin` remote, fresh
/// clone, non-git dir) — routes to the per-branch worktree. The
/// `Err` case deliberately does NOT route to the project root: when
/// git cannot detect the integration branch there is no basis to
/// treat the commit as an integration-branch destination.
///
/// Hook/binary agreement (the "cannot drift" invariant) holds even
/// though only the hook calls this helper:
///
/// - **Trunk commit.** When `branch == integration`, git has the
///   trunk checked out at the project root, so
///   `resolve_worktree_for_branch` routes the binary's commit to the
///   root — exactly the destination this predicate flags for the
///   hook's block decision.
/// - **Feature commit.** A feature branch is never the integration
///   branch, so this predicate never flags it as a trunk destination;
///   the binary commits wherever git has the feature branch checked
///   out (the repo root or a worktree), which can never be a disguised
///   trunk commit.
///
/// `FlowPaths::is_valid_branch` is checked first as the
/// path-construction boundary required by
/// `.claude/rules/branch-path-safety.md`: an empty / `.` / `..` /
/// `/`- or `\0`-bearing branch must never be joined onto
/// `.worktrees/`. The hook's caller validates upstream
/// (`extract_finalize_commit_branch_arg` via `is_valid_branch`), so
/// an invalid branch is unreachable in production; the guard returns
/// the project root (a non-escaping path the helper already returns
/// for the integration case) rather than constructing a
/// traversal-shaped worktree path.
///
/// Named production consumer (per
/// `.claude/rules/docs-with-behavior.md`):
/// `src/hooks/validate_pretool.rs::match_finalize_commit_destination`
/// (the Layer 10 integration-branch gate).
pub fn finalize_commit_destination(root: &Path, branch: &str) -> PathBuf {
    if !FlowPaths::is_valid_branch(branch) {
        return root.to_path_buf();
    }
    match default_branch_in(root) {
        Ok(integration) if normalize_gate_input(branch) == normalize_gate_input(&integration) => {
            root.to_path_buf()
        }
        _ => root.join(".worktrees").join(branch),
    }
}
