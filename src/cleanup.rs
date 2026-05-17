//! Cleanup orchestrator for FLOW features.
//!
//! Two entry shapes:
//!
//! - **Per-branch (`--branch`)** — used by `/flow:flow-complete` (Phase 5)
//!   and `/flow:flow-abort`. Closes the PR, removes the worktree, deletes
//!   branches, removes the branch directory, and sweeps the start-lock
//!   queue entry for the named branch.
//! - **All-flows (`--all`)** — used by `/flow:flow-reset`. Builds a
//!   categorized inventory of `.flow-states/` contents in one walk,
//!   then (unless `--dry-run`) removes the entire `.flow-states/`
//!   directory via `fs::remove_dir_all`. The directory shell is
//!   recreated on demand by subsequent `flow-start` invocations.
//!   PRs, worktrees, and branches are NOT touched by `--all` —
//!   those are handled per-flow via the `--branch <name> --worktree
//!   <path>` mode invoked by `/flow:flow-abort` and
//!   `/flow:flow-complete`.
//!
//! Per-branch usage:
//!   bin/flow cleanup <project_root> --branch <name> --worktree <path> [--pr <number>] [--pull]
//!
//! All-flows usage:
//!   bin/flow cleanup <project_root> --all [--dry-run]
//!
//! Per-branch output (JSON to stdout):
//!   {"status": "ok", "steps": {"pr_close": ..., "worktree": ..., "remote_branch": ...,
//!                              "local_branch": ..., "branch_dir": ..., "queue_entry": ...,
//!                              "git_pull": ...}}
//!
//! Each per-branch step reports one of:
//! "removed"/"deleted"/"closed"/"pulled", "skipped", or
//! "failed: <reason>".
//!
//! All-flows output (JSON to stdout):
//!   {"status": "ok", "dry_run": <bool>,
//!    "inventory": {"flows_with_state": [...], "orphan_dirs": [...],
//!                  "top_level_files": [...], "singletons": [...],
//!                  "sentinel_dirs": [...]},
//!    "flow_states_dir": "deleted" | "would_remove" | "skipped" | "failed: <reason>"}
//!
//! Each `inventory` array names the entries the walker found in
//! `.flow-states/` at decision time (BEFORE the wipe). `flow_states_dir`
//! reports the outcome of the wipe attempt: `"deleted"` after a
//! successful `fs::remove_dir_all`, `"would_remove"` when `--dry-run`
//! preserves the directory, `"skipped"` when the directory was missing
//! at invocation, or `"failed: <reason>"` when the recursive removal
//! returned an `io::Error`.
//!
//! Tests live at tests/cleanup.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Parser;
use indexmap::IndexMap;
use serde_json::{json, Value};

use crate::commands::log::append_log;
use crate::commands::start_lock::QUEUE_DIRNAME;
use crate::flow_paths::{FlowPaths, FlowStatesDir};

#[derive(Parser, Debug)]
#[command(name = "cleanup", about = "FLOW cleanup orchestrator")]
pub struct Args {
    /// Path to project root
    pub project_root: String,

    /// Branch name (required unless --all)
    #[arg(long)]
    pub branch: Option<String>,

    /// Worktree path relative to project_root (required unless --all)
    #[arg(long)]
    pub worktree: Option<String>,

    /// PR number to close (per-branch mode only)
    #[arg(long = "pr")]
    pub pr: Option<i64>,

    /// Run git pull origin <base_branch> after per-branch cleanup
    #[arg(long)]
    pub pull: bool,

    /// Build a categorized inventory of `.flow-states/` contents
    /// and (unless `--dry-run`) remove the entire `.flow-states/`
    /// directory via `fs::remove_dir_all`. The directory shell is
    /// recreated on demand by subsequent `flow-start` invocations.
    /// PRs, worktrees, and branches are NOT touched — those are
    /// handled per-flow via `/flow:flow-abort` and
    /// `/flow:flow-complete`. Mutually exclusive with `--branch`.
    #[arg(long)]
    pub all: bool,

    /// With `--all`: print the inventory of what would be removed
    /// without modifying disk.
    #[arg(long = "dry-run")]
    pub dry_run: bool,
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

/// Recursively remove `<.flow-states>/<branch>/` and everything inside
/// it. The branch directory holds every per-branch artifact (state
/// file, log, plan, DAG, frozen phases, CI sentinel, timings,
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
            "[Phase 5] cleanup — in progress (branch directory will be removed next)",
        );
    }

    // Every per-branch artifact (`state.json`, `log`, `plan.md`,
    // `dag.md`, `phases.json`, `ci-passed`, `timings.md`,
    // `closed-issues.json`, `issues.md`, `rule-content.md`,
    // `commit-msg.txt`, `commit-msg-content.txt`, `start-prompt`)
    // lives under `branch_dir()`, so one `remove_dir_all` covers the
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

/// Build a categorized inventory of `.flow-states/` contents.
///
/// Walks `states_dir` once and classifies every entry into one of five
/// buckets:
///
/// - `flows_with_state` — a subdirectory containing `state.json` (an
///   active or stranded per-branch flow).
/// - `sentinel_dirs` — a subdirectory whose name equals the resolved
///   integration branch (the base-branch CI sentinel directory
///   `.flow-states/<base_branch>/` written by `start-gate`). The
///   resolved branch comes from `git::default_branch_in(project_root)`
///   which falls back to `"main"` when origin/HEAD is unset, so this
///   bucket covers both main-trunk and `staging`/`develop`/etc. repos.
/// - `orphan_dirs` — any other subdirectory (state-less remnants from
///   killed Complete passes, partially-cleaned-up flows, etc.). The
///   `start-queue/` subdirectory falls into this bucket; it is wiped
///   by the wholesale `remove_dir_all` rather than being enumerated
///   per-entry.
/// - `singletons` — top-level `orchestrate.json` (the machine-level
///   orchestration queue).
/// - `top_level_files` — any other top-level file (e.g. an
///   `*-start-prompt` file left by an interrupted flow-start).
///
/// An empty directory produces five empty arrays. A missing directory
/// produces five empty arrays as well — callers distinguish via the
/// separate `flow_states_dir` outcome string.
///
/// The walker swallows per-entry filesystem errors (unreadable
/// `file_type`, etc.) and treats them as "skip this entry" rather
/// than propagating panics. The wholesale wipe that follows surfaces
/// the structural failure mode if the directory is truly unreadable.
fn build_inventory(states_dir: &Path, project_root: &Path) -> Value {
    let mut flows_with_state: Vec<String> = Vec::new();
    let mut orphan_dirs: Vec<String> = Vec::new();
    let mut sentinel_dirs: Vec<String> = Vec::new();
    let mut top_level_files: Vec<String> = Vec::new();
    let mut singletons: Vec<String> = Vec::new();

    let base_branch = crate::git::default_branch_in(project_root);

    if let Ok(entries) = fs::read_dir(states_dir) {
        let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        sorted.sort_by_key(|e| e.file_name());
        for entry in sorted {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            // Use symlink_metadata so symlinks classify as their actual
            // filesystem type (not the type of their target). A symlink
            // pointing to a foreign directory containing state.json
            // would otherwise be misreported as a real flow. The wipe
            // via fs::remove_dir_all unlinks the symlink entry rather
            // than following it, so symlinks fall through both arms
            // and are categorized into top_level_files (preserving
            // visibility in the inventory).
            let metadata = fs::symlink_metadata(&path).ok();
            let is_dir = metadata.as_ref().is_some_and(|m| m.is_dir());
            let is_file = metadata.as_ref().is_some_and(|m| m.is_file());
            if is_dir {
                if path.join("state.json").is_file() {
                    flows_with_state.push(name);
                } else if name == base_branch {
                    sentinel_dirs.push(name);
                } else {
                    orphan_dirs.push(name);
                }
            } else if is_file {
                if name == "orchestrate.json" {
                    singletons.push(name);
                } else {
                    top_level_files.push(name);
                }
            } else {
                // Symlinks (regardless of target) and other special
                // file types (sockets, fifos) fall into top_level_files
                // so the inventory remains audit-complete.
                top_level_files.push(name);
            }
        }
    }

    json!({
        "flows_with_state": flows_with_state,
        "orphan_dirs": orphan_dirs,
        "top_level_files": top_level_files,
        "singletons": singletons,
        "sentinel_dirs": sentinel_dirs,
    })
}

/// Reset every flow on this machine. Builds a categorized inventory
/// of `.flow-states/` contents in one walk, then (unless `dry_run`)
/// removes the entire `.flow-states/` directory via
/// `fs::remove_dir_all`. The directory shell is recreated on demand
/// by subsequent `flow-start` invocations.
///
/// PRs, worktrees, and branches are NOT touched by this entry shape —
/// those are handled per-flow via [`cleanup`] invoked by
/// `/flow:flow-abort` and `/flow:flow-complete`. The collapsed scope
/// keeps `--all` purely local: no `gh` calls, no `git worktree`
/// operations, no per-flow subprocess fan-out. Users who want
/// GitHub-side cleanup of a specific flow run `/flow:flow-abort`
/// against that branch before invoking `/flow:flow-reset`.
///
/// `dry_run = true` returns the inventory with `flow_states_dir`
/// reporting `"would_remove"` (or `"skipped"` if the directory is
/// missing) without touching disk. Live mode returns the same
/// inventory plus `flow_states_dir == "deleted"` on a successful
/// removal, `"failed: <reason>"` when `fs::remove_dir_all` returns
/// an `io::Error`, or `"skipped"` when the directory is missing at
/// invocation.
///
/// The inventory is built BEFORE the wipe so the JSON output reflects
/// the entries that existed at decision time — a future audit can
/// reconcile a successful reset against what was on disk.
///
/// **Concurrency.** Invoked exclusively from `/flow:flow-reset`,
/// whose Guard gates entry on the user being on the repository's
/// integration branch at the project root. The reset is destructive
/// by design: it wipes `.flow-states/` wholesale, which includes any
/// `start-queue/` subdirectory and the base-branch CI sentinel
/// directory `.flow-states/<base_branch>/`. Any concurrent
/// `flow-start` running on the same machine during the wipe will be
/// disrupted (the next start may re-run base-branch CI because the
/// sentinel was removed; an in-flight start may have its queue
/// entry deleted mid-acquire). The 30-minute stale timeout on queue
/// entries protects against permanent block; recovery from
/// sentinel-loss is automatic on the next CI run.
pub fn cleanup_all(project_root: &Path, dry_run: bool) -> Value {
    let states_dir = FlowStatesDir::new(project_root).path().to_path_buf();
    let inventory = build_inventory(&states_dir, project_root);

    let flow_states_dir = if !states_dir.is_dir() {
        "skipped".to_string()
    } else if dry_run {
        "would_remove".to_string()
    } else {
        match fs::remove_dir_all(&states_dir) {
            Ok(()) => "deleted".to_string(),
            Err(e) => format!("failed: {}", e),
        }
    };

    json!({
        "status": "ok",
        "dry_run": dry_run,
        "inventory": inventory,
        "flow_states_dir": flow_states_dir,
    })
}

/// Main-arm dispatch: validate args.project_root and run cleanup.
/// Returns (JSON value, exit code).
///
/// Two modes:
/// - `--all`: invoke [`cleanup_all`] over every flow on disk.
/// - `--branch <name> --worktree <path>`: invoke [`cleanup`] for the
///   single flow.
///
/// Per-branch `base_branch` is resolved from the per-branch state file
/// via `git::read_base_branch` and falls back to git's integration
/// branch (`origin/HEAD`) when the state file is missing, malformed,
/// or omits the field — both the abort path (state file may be
/// partially initialized) and pre-`base_branch`-field state files are
/// covered by the same fallback. `--all` resolves `base_branch`
/// per-flow inside [`cleanup_all`].
pub fn run_impl_main(args: &Args) -> (Value, i32) {
    let root = Path::new(&args.project_root);
    if !root.is_dir() {
        let msg = format!("Project root not found: {}", args.project_root);
        let err_str = crate::output::json_error_string(&msg, &[]);
        return (serde_json::from_str(&err_str).unwrap(), 1);
    }

    // Mutual exclusion: --all is the destructive machine-wide reset
    // path and ignores per-branch flags. Silently dropping --branch /
    // --worktree / --pr / --pull when --all is also set would mask
    // user intent (e.g., a misconstructed automation script that
    // sets both). Reject the combination with a structured error so
    // the user sees which flag was unexpected.
    if args.all {
        if args.branch.is_some() {
            let err_str =
                crate::output::json_error_string("--all is mutually exclusive with --branch", &[]);
            return (serde_json::from_str(&err_str).unwrap(), 1);
        }
        if args.worktree.is_some() {
            let err_str = crate::output::json_error_string(
                "--all is mutually exclusive with --worktree",
                &[],
            );
            return (serde_json::from_str(&err_str).unwrap(), 1);
        }
        if args.pr.is_some() {
            let err_str =
                crate::output::json_error_string("--all is mutually exclusive with --pr", &[]);
            return (serde_json::from_str(&err_str).unwrap(), 1);
        }
        if args.pull {
            let err_str =
                crate::output::json_error_string("--all is mutually exclusive with --pull", &[]);
            return (serde_json::from_str(&err_str).unwrap(), 1);
        }
        return (cleanup_all(root, args.dry_run), 0);
    }

    // --dry-run only applies to --all; reject when --all is absent.
    if args.dry_run {
        let err_str = crate::output::json_error_string("--dry-run requires --all", &[]);
        return (serde_json::from_str(&err_str).unwrap(), 1);
    }

    // Per-branch mode: --branch and --worktree are required.
    let branch = match args.branch.as_deref() {
        Some(b) => b,
        None => {
            let err_str = crate::output::json_error_string(
                "Either --branch (with --worktree) or --all is required",
                &[],
            );
            return (serde_json::from_str(&err_str).unwrap(), 1);
        }
    };
    let worktree = match args.worktree.as_deref() {
        Some(w) => w,
        None => {
            let err_str = crate::output::json_error_string(
                "--worktree is required when --branch is set",
                &[],
            );
            return (serde_json::from_str(&err_str).unwrap(), 1);
        }
    };

    let base_branch = FlowPaths::try_new(root, branch)
        .and_then(|paths| crate::git::read_base_branch(&paths.state_file()).ok())
        .unwrap_or_else(|| crate::git::default_branch_in(root));

    let steps = cleanup(root, branch, worktree, args.pr, args.pull, &base_branch);
    let steps_map: IndexMap<String, Value> = steps
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();
    let steps_value = serde_json::to_value(steps_map).unwrap();
    let ok_str = crate::output::json_ok_string(&[("steps", steps_value)]);
    (serde_json::from_str(&ok_str).unwrap(), 0)
}
