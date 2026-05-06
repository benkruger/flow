//! Write content to a target file path.
//!
//! Usage:
//!   bin/flow write-rule --path <target> --content-file <temp>
//!
//! Output (JSON to stdout):
//!   Success: {"status": "ok", "path": "<target_path>"}
//!   Error:   {"status": "error", "message": "..."}             — content-file read failure or fs::write failure
//!   Error:   {"status": "error", "step": "path_canonicalization",
//!             "message": "...", "provided": "...",
//!             "canonical": "...", "artifact_kind": "..."}      — managed-artifact path mismatch (see canonicalization gate)
//!
//! When `--path` names a FLOW-managed artifact (`plan.md`, `dag.md`,
//! `commit-msg.txt`, `.flow-issue-body`, `orchestrate-queue.json`),
//! `run_impl_main` rejects any value that doesn't normalize to the
//! `(project_root, branch)`-derived canonical destination. The gate
//! runs BEFORE `read_content_file` so a rejection does not destroy
//! the caller's input file. When the gate fires and accepts, the
//! actual `fs::write` call uses the resolved absolute path so a
//! relative `--path` cannot silently re-resolve against the process
//! cwd at write time. See `.claude/rules/file-tool-preflights.md`
//! "Managed-Artifact Canonicalization Gate (CLI Layer)".
//!
//! Tests live at tests/write_rule.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use std::fs;
use std::path::{Component, Path, PathBuf};

use clap::Parser;
use serde_json::json;

use crate::flow_paths::{FlowPaths, FlowStatesDir};
use crate::git;
use crate::hooks::{detect_branch_from_path, is_flow_active};
use crate::protected_paths::is_protected_path;

/// FLOW-managed artifacts whose on-disk location is computed by
/// `FlowPaths` rather than chosen by the caller. When `--path` names
/// one of these, write-rule canonicalizes the target — see
/// `canonical_path` and the `run_impl_main` gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedArtifact {
    /// `<branch_dir>/plan.md`
    PlanMd,
    /// `<branch_dir>/dag.md`
    DagMd,
    /// `<branch_dir>/commit-msg.txt`
    CommitMsgTxt,
    /// `<project_root>/.flow-issue-body`
    FlowIssueBody,
    /// `<project_root>/.flow-states/orchestrate-queue.json`
    OrchestrateQueue,
}

/// Classify `path` by basename. Returns `Some(variant)` when the
/// basename matches a FLOW-managed artifact, `None` otherwise.
///
/// Pure function — does not touch the filesystem and does not
/// validate parent directories. The caller (`run_impl_main`)
/// computes the canonical destination from `(project_root, branch)`
/// and rejects when the canonicalized provided path differs.
pub fn classify_path(path: &Path) -> Option<ManagedArtifact> {
    let name = path.file_name()?.to_str()?;
    match name {
        "plan.md" => Some(ManagedArtifact::PlanMd),
        "dag.md" => Some(ManagedArtifact::DagMd),
        "commit-msg.txt" => Some(ManagedArtifact::CommitMsgTxt),
        ".flow-issue-body" => Some(ManagedArtifact::FlowIssueBody),
        "orchestrate-queue.json" => Some(ManagedArtifact::OrchestrateQueue),
        _ => None,
    }
}

/// Compute the canonical destination for a managed artifact.
///
/// Branch-scoped artifacts (`PlanMd`, `DagMd`, `CommitMsgTxt`) live at
/// `<project_root>/.flow-states/<branch>/<filename>` and require a
/// valid branch — `None` is returned when `branch_opt` is absent or
/// fails `FlowPaths::is_valid_branch` (e.g., contains `/`). Returning
/// `None` lets `run_impl_main` fall back to pass-through behavior in
/// detached-HEAD or invalid-branch contexts rather than panicking.
///
/// `FlowIssueBody` lives at `<project_root>/.flow-issue-body` and
/// `OrchestrateQueue` lives at
/// `<project_root>/.flow-states/orchestrate-queue.json`. Neither is
/// branch-scoped, so both always return `Some(_)` regardless of
/// `branch_opt`.
pub fn canonical_path(
    art: ManagedArtifact,
    root: &Path,
    branch_opt: Option<&str>,
) -> Option<PathBuf> {
    match art {
        ManagedArtifact::PlanMd => FlowPaths::try_new(root, branch_opt?).map(|p| p.plan_file()),
        ManagedArtifact::DagMd => FlowPaths::try_new(root, branch_opt?).map(|p| p.dag_file()),
        ManagedArtifact::CommitMsgTxt => {
            FlowPaths::try_new(root, branch_opt?).map(|p| p.commit_msg())
        }
        ManagedArtifact::FlowIssueBody => Some(root.join(".flow-issue-body")),
        ManagedArtifact::OrchestrateQueue => Some(
            FlowStatesDir::new(root)
                .path()
                .join("orchestrate-queue.json"),
        ),
    }
}

#[derive(Parser, Debug)]
#[command(name = "write-rule", about = "Write content to a target file")]
pub struct Args {
    /// Target file path
    #[arg(long)]
    pub path: String,
    /// Path to file containing content (file is deleted after reading)
    #[arg(long = "content-file")]
    pub content_file: String,
}

/// Read content from a file and delete it.
/// Returns Ok(content) or Err(message).
pub fn read_content_file(path: &str) -> Result<String, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Could not read content file '{}': {}", path, e))?;

    // Delete the content file after reading, ignore errors
    let _ = fs::remove_file(path);

    Ok(content)
}

/// Write content to the target path, creating parent dirs as needed.
/// Returns Ok(()) or Err(message).
pub fn write_rule(target_path: &str, content: &str) -> Result<(), String> {
    let path = Path::new(target_path);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create directories for '{}': {}", target_path, e))?;
    }

    fs::write(path, content).map_err(|e| format!("Could not write to '{}': {}", target_path, e))?;

    Ok(())
}

/// Lexically normalize a path: resolve `..` components without
/// touching the filesystem. Used by the canonicalization gate to
/// compare `--path` against the canonical destination without
/// requiring either to exist on disk. `Path::components()` already
/// drops mid-path `.` segments, so only `..` (`Component::ParentDir`)
/// needs explicit handling.
fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Walk up from `cwd` looking for an ancestor that contains
/// `.flow-states/`. Returns the first match — the main repo root.
///
/// Mirrors `find_project_root_in` in
/// `src/hooks/validate_claude_paths.rs`. Duplicated rather than
/// promoted to a shared helper because the loop is five lines and a
/// new pub surface would require its own consumer + test row per
/// `.claude/rules/test-placement.md` "Bright-line test for `pub`
/// additions".
fn find_main_root_from(cwd: &Path) -> Option<PathBuf> {
    let mut current = cwd.to_path_buf();
    loop {
        if FlowStatesDir::new(&current).path().is_dir() {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

/// Worktree-path guard for protected basenames during an active flow.
///
/// Closes the subprocess-layer hole the `validate-claude-paths`
/// PreToolUse hook leaves open: the hook blocks Edit/Write tool calls
/// on `CLAUDE.md`, `.claude/rules/*`, and `.claude/skills/*` and
/// redirects the model to `bin/flow write-rule`. Without this guard,
/// a `bin/flow write-rule --path <main_repo>/CLAUDE.md` invocation
/// during an active flow would write to the main-repo copy, bypassing
/// the worktree-only invariant the hook enforces.
///
/// Returns `Some(error_value)` when the gate fires (caller exits 1
/// with that JSON envelope on stdout). Returns `None` when the gate
/// is silent — the path isn't protected, no flow is active, the
/// target already lands inside the worktree, or `cwd` cannot be
/// resolved.
///
/// The gate uses path-based detection (no git subprocess) so it
/// preserves the existing pass-through guarantee for non-managed
/// paths: `git::project_root()` is only invoked for managed
/// artifacts in the canonicalization gate above. Detection here:
///
/// 1. Walk up `cwd` for `.flow-states/` → `main_root`.
/// 2. `detect_branch_from_path(cwd)` reads the `.worktrees/<branch>/`
///    marker if present, falling back to `git branch --show-current`.
/// 3. `is_flow_active(branch, main_root)` checks for
///    `<main_root>/.flow-states/<branch>/state.json`.
/// 4. The expected worktree is `<main_root>/.worktrees/<branch>/`.
///    Reject when the resolved target normalizes outside it.
///
/// Per `.claude/rules/security-gates.md` "Gate-Action Atomicity for
/// Validated Paths", the caller must use the resolved absolute path
/// (not the raw `args.path`) when this guard is silent — otherwise
/// `fs::write` re-resolves the relative input against the process
/// cwd at write time and lands at a different file than the one the
/// gate inspected.
fn worktree_path_guard(provided: &Path, cwd: &Path) -> Option<serde_json::Value> {
    if !is_protected_path(provided) {
        return None;
    }
    let main_root = find_main_root_from(cwd)?;
    let branch = detect_branch_from_path(cwd)?;
    if !is_flow_active(&branch, &main_root) {
        return None;
    }
    let target_abs = if provided.is_absolute() {
        provided.to_path_buf()
    } else {
        cwd.join(provided)
    };
    let worktree = main_root.join(".worktrees").join(&branch);
    let target_norm = normalize_lexical(&target_abs);
    let worktree_norm = normalize_lexical(&worktree);
    if target_norm.starts_with(&worktree_norm) {
        return None;
    }
    Some(json!({
        "status": "error",
        "step": "worktree_path_validation",
        "message": format!(
            "write-rule rejects --path {} for protected basename: \
             active flow on branch '{}' requires writes to land inside \
             the worktree at {}",
            target_abs.display(),
            branch,
            worktree.display()
        ),
        "provided": provided.display().to_string(),
        "branch": branch,
        "worktree": worktree.display().to_string(),
    }))
}

pub fn run_impl_main(args: &Args) -> (serde_json::Value, i32) {
    let provided = Path::new(&args.path);

    // Canonicalization gate per .claude/rules/file-tool-preflights.md
    // "Managed-Artifact Canonicalization Gate (CLI Layer)". When --path
    // names a managed artifact by basename, the canonical destination
    // is computed from (project_root, current_branch) via FlowPaths and
    // any provided path that doesn't normalize to that destination is
    // rejected. Branch-unavailable cases (detached HEAD, slash branch)
    // produce a None canonical and the gate stays silent — that's
    // pass-through behavior, not a reject.
    //
    // Two ordering invariants the gate must honor:
    //   1. The gate runs BEFORE `read_content_file` so a rejection does
    //      not destroy the caller's input — `read_content_file` deletes
    //      the source as part of its normal contract.
    //   2. When the gate accepts, the actual `fs::write` call uses the
    //      resolved absolute path, NOT `args.path`. A relative
    //      `--path` resolved against `project_root` for the gate would
    //      otherwise be re-resolved by `fs::write` against the process
    //      cwd — and from a mono-repo subdirectory the two are
    //      different paths, so the file would land at a misplaced
    //      location while the gate had already approved.
    let target_path: String = if let Some(art) = classify_path(provided) {
        let root = git::project_root();
        let branch = git::current_branch();
        if let Some(canonical) = canonical_path(art, &root, branch.as_deref()) {
            let provided_abs = if provided.is_absolute() {
                provided.to_path_buf()
            } else {
                root.join(provided)
            };
            if normalize_lexical(&provided_abs) != normalize_lexical(&canonical) {
                return (
                    json!({
                        "status": "error",
                        "step": "path_canonicalization",
                        "message": format!(
                            "write-rule rejects --path {} for managed \
                             artifact {:?}: canonical destination is {}",
                            args.path,
                            art,
                            canonical.display()
                        ),
                        "provided": &args.path,
                        "canonical": canonical.display().to_string(),
                        "artifact_kind": format!("{:?}", art),
                    }),
                    1,
                );
            }
            // Gate accepted: write to the resolved absolute path so
            // fs::write cannot silently re-resolve against the process cwd.
            provided_abs.to_string_lossy().into_owned()
        } else {
            // canonical_path returned None (branch-unavailable):
            // pass-through, write to the caller-provided path verbatim.
            args.path.clone()
        }
    } else {
        // Non-managed basename: pass-through, write to the caller-provided path.
        args.path.clone()
    };

    // Worktree-path guard for protected basenames during an active
    // flow. Runs BEFORE `read_content_file` (same ordering invariant
    // as the canonicalization gate above) so a rejection does not
    // destroy the caller's input file. See
    // `.claude/rules/file-tool-preflights.md` "Managed-Artifact
    // Canonicalization Gate (CLI Layer)" for the gate-before-read
    // discipline this guard inherits.
    //
    // `.expect()` is intentional: a subprocess that cannot resolve its
    // own cwd is broken in ways the upstream `git::project_root()`
    // call already would have failed on (the canonicalization gate
    // above shells `git rev-parse` from the same cwd). Collapsing the
    // branch keeps the gate testable without contriving a deleted-cwd
    // fixture per `.claude/rules/testability-means-simplicity.md`.
    let cwd = std::env::current_dir().expect("subprocess cwd must be resolvable for write-rule");
    if let Some(err) = worktree_path_guard(provided, &cwd) {
        return (err, 1);
    }

    let content = match read_content_file(&args.content_file) {
        Ok(c) => c,
        Err(e) => return (json!({"status": "error", "message": e}), 1),
    };

    if let Err(e) = write_rule(&target_path, &content) {
        return (json!({"status": "error", "message": e}), 1);
    }
    (json!({"status": "ok", "path": target_path}), 0)
}
