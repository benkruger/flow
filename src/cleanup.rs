//! Per-branch cleanup orchestrator for FLOW features.
//!
//! Used by `/flow:flow-complete` (Phase 4) and `/flow:flow-abort`.
//! Closes the PR, removes the worktree, deletes the local and remote
//! branches, removes the branch directory under `.flow-states/`, and
//! sweeps the start-lock queue entry for the named branch.
//!
//! Usage:
//!   bin/flow cleanup <project_root> --branch <name> --worktree <path> [--pr <number>] [--pull]
//!
//! Output (JSON to stdout):
//!   {"status": "ok", "steps": {"pr_close": ..., "adversarial_probe": ...,
//!                              "worktree": ..., "remote_branch": ...,
//!                              "local_branch": ..., "phase_anchor_marker": ...,
//!                              "branch_dir": ..., "queue_entry": ...,
//!                              "git_pull": ...}}
//!
//! Each step reports one of: "removed"/"deleted"/"closed"/"pulled",
//! "skipped", or "failed: <reason>".
//!
//! Machine-wide `.flow-states/` wipes belong to `/flow:flow-reset`,
//! which invokes `bin/reset` directly (resolved via the plugin root
//! prefix) — that script (and not this module) owns the wholesale
//! reset path.
//!
//! Tests live at tests/cleanup.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Parser;
use indexmap::IndexMap;
use serde_json::Value;

use crate::commands::log::append_log;
use crate::commands::start_lock::QUEUE_DIRNAME;
use crate::flow_paths::FlowPaths;

#[derive(Parser, Debug)]
#[command(name = "cleanup", about = "FLOW per-branch cleanup orchestrator")]
pub struct Args {
    /// Path to project root
    pub project_root: String,

    /// Branch name (required)
    #[arg(long)]
    pub branch: Option<String>,

    /// Worktree path relative to project_root (required)
    #[arg(long)]
    pub worktree: Option<String>,

    /// PR number to close
    #[arg(long = "pr")]
    pub pr: Option<i64>,

    /// Run git pull origin <base_branch> after per-branch cleanup
    #[arg(long)]
    pub pull: bool,
}

/// Run a command in `cwd` via `Command::output()` without a timeout.
/// Returns `(success, trimmed-output)` where output is stderr on
/// failure (or stdout when stderr is empty).
fn run_cmd(args: &[&str], cwd: &Path) -> (bool, String) {
    match Command::new(args[0])
        .args(&args[1..])
        .current_dir(cwd)
        .output()
    {
        Ok(output) => {
            if output.status.success() {
                (
                    true,
                    String::from_utf8_lossy(&output.stdout).trim().to_string(),
                )
            } else {
                let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
                if err.is_empty() {
                    (
                        false,
                        String::from_utf8_lossy(&output.stdout).trim().to_string(),
                    )
                } else {
                    (false, err)
                }
            }
        }
        Err(e) => (false, e.to_string()),
    }
}

fn label_result(ok: bool, ok_label: &str, output: &str) -> String {
    if ok {
        ok_label.to_string()
    } else {
        format!("failed: {}", output)
    }
}

/// Resolve `probe_path` against the filesystem and return the
/// canonical form contained inside `wt_canon`, or `None` when the
/// path falls outside the worktree or cannot be resolved.
///
/// `bin/test --adversarial-path` is project-owned bash whose stdout
/// flows directly into `fs::remove_file`. Per
/// `.claude/rules/external-input-path-construction.md`, every
/// state-derived/external-input string flowing into a filesystem
/// path must pass a positive validator before the syscall.
///
/// Strategy: walk up `probe_path` until an ancestor exists on disk,
/// canonicalize that ancestor (which collapses any `..` segments in
/// the existing prefix and resolves any symlinks), then re-append
/// the stripped suffix. The final path is checked for
/// `starts_with(wt_canon)`. Three cases collapse to the same code:
///
/// - Probe exists (file or symlink) — `exists()` is true on first
///   iteration; canonicalize collapses traversals and resolves
///   symlinks; the final containment check decides.
/// - Probe doesn't exist but its parent does — first iteration
///   pushes the basename; second iteration finds the parent exists,
///   canonicalizes, re-appends, and the final containment check
///   decides. The probe is then "missing" (resolution succeeded;
///   the file is just absent).
/// - Probe and several ancestors don't exist — the loop walks
///   further up before finding an existing ancestor.
///
/// `Path::file_name()` returns `None` for paths terminating in
/// `..`, `.`, `/`, or empty — the `?` operator bails out with
/// `None` for any such pathological input.
fn resolve_probe_inside_worktree(probe_path: &Path, wt_canon: &Path) -> Option<PathBuf> {
    let mut anchor = probe_path.to_path_buf();
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    while !anchor.exists() {
        let name = anchor.file_name()?.to_owned();
        suffix.push(name);
        // `file_name()` returned `Some(Normal-component)`, so the
        // path has a proper trailing component and `parent()` is
        // guaranteed `Some` here. `.expect()` documents the
        // unreachable arm rather than papering it with a runtime
        // check that the type system proves cannot fire.
        anchor = anchor
            .parent()
            .expect("file_name() Some implies parent() Some")
            .to_path_buf();
    }
    // `anchor.exists()` returned true above (loop exited), so the
    // path resolves to an inode the kernel can see. canonicalize()
    // on the same path uses the same syscall semantics; the only
    // way it returns Err is a TOCTOU race where permissions or the
    // path itself change between two adjacent syscalls, which no
    // production caller can trigger. `.expect()` per the
    // unreachable-arm carve-out in
    // `.claude/rules/testability-means-simplicity.md`.
    let canonical_anchor = anchor
        .canonicalize()
        .expect("anchor.exists() returned true; canonicalize is unreachable absent a TOCTOU race");
    let mut full = canonical_anchor;
    for seg in suffix.iter().rev() {
        full.push(seg);
    }
    if full.starts_with(wt_canon) {
        Some(full)
    } else {
        None
    }
}

/// Remove the Review adversarial probe file from the worktree
/// before `git worktree remove` disposes of the worktree directory.
/// Per `.claude/rules/ephemeral-file-cleanup.md`, running this step
/// BEFORE worktree removal makes the disposal explicit in the JSON
/// `steps` output rather than a silent side-effect of the
/// `git worktree remove --force` later in the same cleanup pass.
///
/// The probe path is resolved by spawning the worktree's
/// `bin/test --adversarial-path`. The file is removed via
/// `fs::remove_file` (which works regardless of any caller's
/// permission allow-list and tolerates `NotFound` as `"missing"`).
///
/// `bin/test` is project-owned bash whose stdout flows into
/// `fs::remove_file`; its stdout is treated as untrusted input per
/// `.claude/rules/external-input-path-construction.md`. The resolved
/// probe path is canonicalized and verified to be contained inside
/// the canonicalized worktree directory before any deletion. A path
/// that resolves outside the worktree (`../../etc/passwd`,
/// `/Users/.../authorized_keys`) returns `"skipped"` and the file
/// is not touched.
///
/// Outcomes:
///
/// - `"deleted"` — probe present, contained inside the worktree, and
///   removed.
/// - `"missing"` — path resolved to a worktree-internal location but
///   no file is there (the adversarial agent never wrote one, or
///   Step 4 already reconciled the probe per
///   `.claude/rules/adversarial-probe-lifecycle.md`).
/// - `"skipped"` — worktree directory missing, `bin/test` missing,
///   `bin/test` exited non-zero (unconfigured stub), its stdout is
///   empty, the worktree path cannot be canonicalized, or the
///   resolved probe path falls outside the worktree.
/// - `"failed: <reason>"` — `fs::remove_file` failed with a reason
///   other than `NotFound` (permissions, filesystem error,
///   `EISDIR`).
fn delete_adversarial_probe(project_root: &Path, worktree: &str) -> String {
    let wt_path = project_root.join(worktree);
    if !wt_path.is_dir() {
        return "skipped".to_string();
    }
    let bin_test = wt_path.join("bin").join("test");
    if !bin_test.is_file() {
        return "skipped".to_string();
    }
    let bin_test_str = bin_test.to_string_lossy().to_string();
    let (ok, output) = run_cmd(&[&bin_test_str, "--adversarial-path"], &wt_path);
    if !ok {
        return "skipped".to_string();
    }
    let probe_rel = output.trim();
    if probe_rel.is_empty() {
        return "skipped".to_string();
    }
    let candidate = if Path::new(probe_rel).is_absolute() {
        PathBuf::from(probe_rel)
    } else {
        wt_path.join(probe_rel)
    };
    // `wt_path.is_dir()` returned true above, so the directory
    // exists and the kernel can stat it. canonicalize() on an
    // existing directory only fails via TOCTOU permission
    // revocation between adjacent syscalls — unreachable in
    // production. `.expect()` per the unreachable-arm carve-out in
    // `.claude/rules/testability-means-simplicity.md`.
    let wt_canon = wt_path
        .canonicalize()
        .expect("wt_path.is_dir() returned true; canonicalize is unreachable absent a TOCTOU race");
    let probe_path = match resolve_probe_inside_worktree(&candidate, &wt_canon) {
        Some(p) => p,
        None => return "skipped".to_string(),
    };
    match fs::remove_file(&probe_path) {
        Ok(()) => "deleted".to_string(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "missing".to_string(),
        Err(e) => format!("failed: {}", e),
    }
}

/// Read the `session_id` string from the branch's state file, returning
/// `None` when the file is absent, unparseable, or carries no
/// `session_id` string. Used to resolve which phase-anchor marker to
/// delete before the state file itself is removed. Reads via
/// `fs::read_to_string` — the state file is a FLOW-managed artifact
/// written only by FLOW's own subcommands, not external input, so the
/// read carries no byte cap.
fn read_state_session_id(state_file: &Path) -> Option<String> {
    let content = fs::read_to_string(state_file).ok()?;
    let parsed: Value = serde_json::from_str(&content).ok()?;
    parsed
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// Remove the session-keyed phase-anchor marker written by
/// `phase-enter` (`src/phase_anchor.rs`). The marker lives under
/// `<home>/.claude/flow/`, NOT inside the worktree, so `git worktree
/// remove` does not dispose of it — it must be removed explicitly.
/// Idempotent: `NotFound` is tolerated as `"missing"` because cleanup
/// may run twice (abort-then-complete, or a retry). Returns `"skipped"`
/// when no session id resolves or the marker path cannot be built
/// (unsafe/empty home).
fn remove_phase_anchor_marker(home: &Path, session_id: Option<&str>) -> String {
    let sid = match session_id {
        Some(s) => s,
        None => return "skipped".to_string(),
    };
    let path = match crate::phase_anchor::marker_path(home, sid) {
        Some(p) => p,
        None => return "skipped".to_string(),
    };
    match fs::remove_file(&path) {
        Ok(()) => "deleted".to_string(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "missing".to_string(),
        Err(e) => format!("failed: {}", e),
    }
}

/// Recursively remove `<.flow-states>/<branch>/` and everything inside
/// it. The branch directory holds every per-branch artifact (state
/// file, log, plan, frozen phases, CI sentinel, timings,
/// closed-issues record, issues summary, scratch rule content, commit
/// message, start prompt, adversarial test files of any extension), so
/// a single recursive remove replaces the previous per-suffix
/// enumeration and the bespoke adversarial-test glob. Idempotent —
/// `NotFound` is treated as success because cleanup may run twice
/// (abort-then-complete in adjacent sessions, or a retry after a
/// partial failure).
fn try_remove_branch_dir(branch_dir: &Path) -> String {
    match fs::remove_dir_all(branch_dir) {
        Ok(()) => "deleted".to_string(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "skipped".to_string(),
        Err(e) => format!("failed: {}", e),
    }
}

/// Perform cleanup steps. Returns an ordered map of step results.
/// Called cross-module from `complete_finalize::run_impl_with_deps` as
/// well as from `run_impl_main` below.
///
/// `base_branch` is the integration branch the optional `--pull`
/// step targets via `git pull origin <base_branch>`; the caller
/// resolves it from the state file (or falls back to `"main"` for
/// legacy state files / the abort path with no state file).
pub fn cleanup(
    project_root: &Path,
    branch: &str,
    worktree: &str,
    pr_number: Option<i64>,
    pull: bool,
    base_branch: &str,
) -> IndexMap<String, String> {
    let mut steps = IndexMap::new();

    // Close PR (abort only)
    if let Some(pr) = pr_number {
        let (ok, output) = run_cmd(&["gh", "pr", "close", &pr.to_string()], project_root);
        steps.insert("pr_close".to_string(), label_result(ok, "closed", &output));
    } else {
        steps.insert("pr_close".to_string(), "skipped".to_string());
    }

    // Dispose of the Review adversarial probe explicitly before
    // worktree removal so the disposal lands in the steps JSON as an
    // audit trail entry rather than a silent side-effect of
    // `git worktree remove`. Must run BEFORE the worktree-removal
    // step per `.claude/rules/skill-authoring.md` "Cleanup Script
    // Step Ordering" — once the worktree is removed,
    // `bin/test --adversarial-path` no longer resolves.
    steps.insert(
        "adversarial_probe".to_string(),
        delete_adversarial_probe(project_root, worktree),
    );

    // Remove worktree (the subsequent `git worktree remove --force`
    // also disposes of any worktree-internal scratch like `tmp/`, so a
    // separate per-tmp step is unnecessary).
    let wt_path = project_root.join(worktree);
    if wt_path.exists() {
        let wt_str = wt_path.to_string_lossy().to_string();
        let (ok, output) = run_cmd(
            &["git", "worktree", "remove", &wt_str, "--force"],
            project_root,
        );
        steps.insert("worktree".to_string(), label_result(ok, "removed", &output));
    } else {
        steps.insert("worktree".to_string(), "skipped".to_string());
    }

    // Delete remote branch (abort only — GitHub auto-deletes after merge)
    if pr_number.is_some() {
        let (ok, output) = run_cmd(&["git", "push", "origin", "--delete", branch], project_root);
        steps.insert(
            "remote_branch".to_string(),
            label_result(ok, "deleted", &output),
        );
    } else {
        steps.insert("remote_branch".to_string(), "skipped".to_string());
    }

    // Delete local branch
    let (ok, output) = run_cmd(&["git", "branch", "-D", branch], project_root);
    steps.insert(
        "local_branch".to_string(),
        label_result(ok, "deleted", &output),
    );

    // External-input audit: `branch` reaches cleanup directly from
    // complete-finalize's `--branch` CLI arg per
    // `.claude/rules/external-input-validation.md`. Slash-containing
    // or empty branches cannot address `.flow-states/<branch>/` —
    // use `try_new` and skip the branch-dir removal when the branch
    // is invalid. `--pull` still runs because it does not depend on
    // FlowPaths.
    let paths = match FlowPaths::try_new(project_root, branch) {
        Some(p) => p,
        None => {
            steps.insert(
                "branch_dir".to_string(),
                "skipped: invalid branch".to_string(),
            );
            steps.insert(
                "queue_entry".to_string(),
                "skipped: invalid branch".to_string(),
            );
            if pull {
                let (ok, output) = run_cmd(&["git", "pull", "origin", base_branch], project_root);
                steps.insert("git_pull".to_string(), label_result(ok, "pulled", &output));
            }
            return steps;
        }
    };

    // Log cleanup progress before the branch directory (and therefore
    // the log file inside it) is removed. Only log if the log file
    // already exists — `append_log` creates the file if missing, which
    // would otherwise cause `try_remove_branch_dir` to remove a freshly
    // created file instead of a missing one and produce surprising
    // results in test fixtures that intentionally omit the log. This
    // entry is written mid-cleanup (before the dir removal), so it
    // cannot report a total step count — the JSON output has the full
    // step results.
    let log_path = paths.log_file();
    if log_path.exists() {
        let _ = append_log(
            project_root,
            branch,
            "[Phase 4] cleanup — in progress (branch directory will be removed next)",
        );
    }

    // Delete the session-keyed phase-anchor marker
    // (`src/phase_anchor.rs`). The marker lives under
    // `<home>/.claude/flow/`, NOT inside the worktree, so it survives
    // `git worktree remove` and must be removed explicitly. The
    // session id is read from the state file HERE, before the
    // branch_dir removal below disposes of `state.json`. Best-effort:
    // tolerates a missing marker and skips when no session id resolves.
    let anchor_session_id = read_state_session_id(&paths.state_file());
    let anchor_home = crate::session_metrics::home_dir_or_empty();
    steps.insert(
        "phase_anchor_marker".to_string(),
        remove_phase_anchor_marker(&anchor_home, anchor_session_id.as_deref()),
    );

    // Every per-branch artifact (`state.json`, `log`, `plan.md`,
    // `phases.json`, `ci-passed`, `timings.md`,
    // `closed-issues.json`, `issues.md`, `rule-content.md`,
    // `start-prompt`) lives under `branch_dir()`, so one
    // `remove_dir_all` covers the
    // full set and naturally handles future per-branch additions
    // without code changes. Review's adversarial probe lives
    // inside the worktree's test tree (declared per-project via
    // `bin/test --adversarial-path`) and is disposed of by
    // `git worktree remove` later in this same cleanup pass — no
    // per-suffix glob is required here.
    steps.insert(
        "branch_dir".to_string(),
        try_remove_branch_dir(&paths.branch_dir()),
    );

    // Remove the start-lock queue entry for this branch, if present.
    // `start_init` writes `.flow-states/<QUEUE_DIRNAME>/<branch>` while
    // holding the start lock and `start_finalize` releases it on the
    // happy path; this step is defense-in-depth for the abort path and
    // any unusual case where Complete runs without a clean Start. The
    // queue_dir itself is left in place — `start_lock::queue_path`
    // recreates it on demand for subsequent flows.
    let queue_entry_path = paths.flow_states_dir().join(QUEUE_DIRNAME).join(branch);
    let queue_result = match fs::remove_file(&queue_entry_path) {
        Ok(()) => "removed".to_string(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "skipped".to_string(),
        Err(e) => format!("failed: {}", e),
    };
    steps.insert("queue_entry".to_string(), queue_result);

    // Pull latest origin/<base_branch> (after worktree removal —
    // ordering matters). `base_branch` flows in from the caller's
    // state-file read (defaulting to "main" for legacy state files).
    if pull {
        let (ok, output) = run_cmd(&["git", "pull", "origin", base_branch], project_root);
        steps.insert("git_pull".to_string(), label_result(ok, "pulled", &output));
    }

    steps
}

/// Main-arm dispatch: validate args.project_root and run per-branch cleanup.
/// Returns (JSON value, exit code).
///
/// `base_branch` is resolved via `git::default_branch_in(root)` ONLY
/// when `args.pull` is true (the only consumer of `base_branch`
/// inside `cleanup` is `git pull origin <base_branch>`). When
/// `args.pull` is false, `base_branch` is irrelevant and cleanup
/// proceeds with an empty placeholder. When `args.pull` is true and
/// git cannot resolve `origin/HEAD`, the resolve failure is surfaced
/// as `git_pull: "failed: <reason>"` in the steps map rather than
/// aborting all cleanup — worktree removal, branch deletion, PR
/// close, queue-entry sweep, and branch-dir removal proceed
/// independently of integration-branch resolution.
pub fn run_impl_main(args: &Args) -> (Value, i32) {
    let root = Path::new(&args.project_root);
    if !root.is_dir() {
        let msg = format!("Project root not found: {}", args.project_root);
        return (crate::output::json_error_value(&msg, &[]), 1);
    }

    // Per-branch mode: --branch and --worktree are required.
    let branch = match args.branch.as_deref() {
        Some(b) => b,
        None => {
            return (
                crate::output::json_error_value("--branch (with --worktree) is required", &[]),
                1,
            );
        }
    };
    let worktree = match args.worktree.as_deref() {
        Some(w) => w,
        None => {
            return (
                crate::output::json_error_value("--worktree is required when --branch is set", &[]),
                1,
            );
        }
    };

    let (base_branch, base_branch_resolve_err): (String, Option<String>) = if args.pull {
        match crate::git::default_branch_in(root) {
            Ok(b) => (b, None),
            Err(msg) => (String::new(), Some(msg)),
        }
    } else {
        (String::new(), None)
    };

    let pull_for_cleanup = args.pull && base_branch_resolve_err.is_none();
    let mut steps = cleanup(
        root,
        branch,
        worktree,
        args.pr,
        pull_for_cleanup,
        &base_branch,
    );
    if let Some(msg) = base_branch_resolve_err {
        steps.insert(
            "git_pull".to_string(),
            format!("failed: cannot resolve integration branch — {}", msg),
        );
    }
    // Build the steps object directly as a `serde_json::Map`
    // (`preserve_order` keeps the insertion order from the
    // order-preserving `steps` IndexMap), so the success arm returns
    // the Value with no serialize/reparse round-trip.
    let steps_value = Value::Object(
        steps
            .into_iter()
            .map(|(k, v)| (k, Value::String(v)))
            .collect(),
    );
    (crate::output::json_ok_value(&[("steps", steps_value)]), 0)
}
