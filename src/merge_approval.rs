//! Branch-directory-keyed, single-use merge-approval marker store,
//! plus the `bin/flow confirm-merge` subcommand that writes the
//! marker.
//!
//! `flow-complete` is the Phase 4 terminal skill. Its autonomy mode
//! (`auto` = merge without asking, `manual` = confirm first) is
//! configured per-project under `skills.flow-complete` and resolved
//! at runtime via `resolve_skill_mode::resolve`. When the mode is
//! `manual`, the squash-merge is gated: both merge surfaces
//! (`complete_merge`, `complete_fast::freshness_and_merge`) consume a
//! merge-approval marker before the freshness check that precedes the
//! squash-merge, and refuse with
//! `{"status":"error","reason":"merge_not_confirmed"}` when it is
//! absent. Consuming before the freshness check means every merge
//! attempt consumes the marker — a freshness outcome that loops back
//! without merging (`ci_rerun`/`ci_stale`) still requires a fresh
//! confirmation on the next attempt. `confirm-merge` is the "proceed"
//! half — the flow-complete skill invokes it on the user's "Yes,
//! merge" answer.
//!
//! The marker is keyed on the **branch directory** —
//! `<project_root>/.flow-states/<branch>/` — because the two consumer
//! families hold different identifiers: `confirm-merge` has the
//! external `--branch` string and validates it through
//! `FlowPaths::try_new` before deriving the branch directory, while
//! the merge surfaces hold the `--state-file` path and take its
//! parent directory. Both resolve to the same branch directory, so a
//! single directory key serves both without a fragile root/branch
//! re-derivation.
//!
//! Three invariants the store enforces:
//!
//! - **Single-use.** Consumption deletes the marker. A merge that
//!   loops back through the confirmation prompt (a `ci_rerun`
//!   re-verification) finds no marker and requires a fresh
//!   confirmation. There is no "consumed" flag — file presence IS
//!   the unconsumed state.
//! - **Per-branch scope (defense-in-depth).** The marker lives in
//!   the per-branch state directory, so a marker written for one
//!   branch is never visible to a check against another. The marker
//!   body ALSO carries the branch name and
//!   `check_and_consume_approval` re-verifies it against the branch
//!   directory's own name, so a marker hand-moved between branch
//!   directories cannot satisfy the check.
//! - **Fail-closed corruption resilience.** Any unreadable,
//!   oversized, unparseable, wrong-root-type, `approved != true`, or
//!   branch-mismatched marker yields no approval. The merge then
//!   stays refused — a corrupt marker can never become an escape
//!   hatch.
//!
//! The external `--branch` string reaches filesystem path
//! construction only through `confirm-merge`'s `FlowPaths::try_new`
//! call, which rejects empty / `.` / `..` / `/`-bearing / NUL-bearing
//! branches per `.claude/rules/branch-path-safety.md`. The marker
//! functions themselves take a ready branch-directory path and join
//! a fixed filename onto it — no branch interpolation.
//!
//! Tests live at `tests/merge_approval.rs` per
//! `.claude/rules/test-placement.md`.

use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};

use clap::Parser;
use serde_json::{json, Value};

use crate::flow_paths::FlowPaths;
use crate::git::resolve_branch_in;

/// Maximum bytes read from a marker file. Markers this module writes
/// are a few dozen bytes of JSON; the cap bounds I/O when the marker
/// path holds a corrupted or hostile oversized file (a hand-edit, an
/// interrupted unrelated write, a symlink to a large file). Per
/// `.claude/rules/external-input-path-construction.md` every external
/// read enforces a documented byte cap.
const MARKER_BYTE_CAP: u64 = 64 * 1024;

/// Marker filename under the per-branch state directory.
const MARKER_FILENAME: &str = "merge-approval";

/// The marker file path inside `branch_dir` — a fixed filename joined
/// onto the caller-supplied branch directory.
pub fn marker_path(branch_dir: &Path) -> PathBuf {
    branch_dir.join(MARKER_FILENAME)
}

/// Write a merge-approval marker authorizing exactly one subsequent
/// squash-merge of the flow in `branch_dir`. `branch` is recorded in
/// the marker body so `check_and_consume_approval` can re-verify it
/// against the directory name. Creates `branch_dir` if absent.
/// Returns `Err` on any filesystem failure — the caller surfaces a
/// structured error rather than silently approving.
pub fn write_approval(branch_dir: &Path, branch: &str) -> io::Result<()> {
    fs::create_dir_all(branch_dir)?;
    let body = json!({ "approved": true, "branch": branch });
    fs::write(marker_path(branch_dir), body.to_string())
}

/// Consult and consume the merge-approval marker in `branch_dir`.
///
/// Returns `true` iff a valid, unconsumed marker existed AND was
/// successfully deleted (single-use consume-on-allow). Every other
/// outcome returns `false` so the merge gate keeps refusing:
///
/// - missing / unreadable marker
/// - marker larger than `MARKER_BYTE_CAP`
/// - non-JSON or wrong-root-type content
/// - `approved` not boolean `true`
/// - `branch` body field absent, non-string, or not equal to
///   `branch_dir`'s own directory name
/// - the marker existed and validated but `fs::remove_file` failed
///   (fail-closed: if it cannot be consumed it must not authorize,
///   so a subsequent merge cannot reuse the same marker)
pub fn check_and_consume_approval(branch_dir: &Path) -> bool {
    let path = marker_path(branch_dir);
    let file = match File::open(&path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut buf = String::new();
    if BufReader::new(file.take(MARKER_BYTE_CAP))
        .read_to_string(&mut buf)
        .is_err()
    {
        return false;
    }
    let parsed: Value = match serde_json::from_str(&buf) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let obj = match parsed.as_object() {
        Some(o) => o,
        None => return false,
    };
    if obj.get("approved").and_then(Value::as_bool) != Some(true) {
        return false;
    }
    // The marker body's branch must match the directory it sits in,
    // so a marker hand-moved between branch directories fails the
    // check. `file_name()` is `None` only for a root / `..`-ending
    // directory; that folds into the mismatch arm — an absent
    // expected name never equals a present body string.
    let expected = branch_dir.file_name().and_then(|n| n.to_str());
    if obj.get("branch").and_then(Value::as_str) != expected {
        return false;
    }
    // Valid + unconsumed: deleting the marker IS the consume. Only
    // report approval when the delete succeeds, so a failed remove
    // cannot leave a reusable marker behind.
    fs::remove_file(&path).is_ok()
}

#[derive(Parser, Debug)]
#[command(
    name = "confirm-merge",
    about = "Record a single-use user confirmation to squash-merge the flow's PR"
)]
pub struct Args {
    /// Branch whose `.flow-states/<branch>/` holds the marker.
    /// Optional; resolved from the worktree cwd when absent.
    #[arg(long)]
    pub branch: Option<String>,
}

fn err(reason: &str, message: impl Into<String>) -> (Value, i32) {
    (
        json!({"status": "error", "reason": reason, "message": message.into()}),
        1,
    )
}

/// Main-arm dispatcher that accepts `cwd` as a `Result` so the
/// `current_dir()`-failure fallback (deleted-cwd / chroot) lives in
/// the module where a unit test can drive it — keeping the
/// `src/main.rs` arm a closure-free one-liner. Mirrors
/// `approve_shared_config::run_impl_main_with_cwd_result`.
pub fn run_impl_main_with_cwd_result(
    args: &Args,
    root: &Path,
    cwd_result: std::io::Result<PathBuf>,
) -> (Value, i32) {
    let cwd = cwd_result.unwrap_or(PathBuf::from("."));
    run_impl_main(args, root, &cwd)
}

/// Main-arm dispatcher. `cwd` is the subcommand's working directory
/// (inside the flow worktree). Exit code is `1` on every rejection so
/// a non-confirmation can never silently produce an approval marker;
/// `0` with `{"status":"ok"}` when the marker is written.
pub fn run_impl_main(args: &Args, root: &Path, cwd: &Path) -> (Value, i32) {
    // State-mutator cwd guard (rust-patterns "Guard Universality
    // Across CLI Entry Points"): this subcommand writes a marker, so
    // it enforces the same drift guard as other state mutators.
    if let Err(message) = crate::cwd_scope::enforce(cwd, root) {
        return err("cwd_drift", message);
    }

    let branch = match resolve_branch_in(args.branch.as_deref(), cwd, root) {
        Some(b) => b,
        None => return err("invalid_branch", "could not determine branch"),
    };
    // Branch-path-safety: the external `--branch` string reaches a
    // `.flow-states/` path only through `FlowPaths::try_new`, which
    // rejects `/`-bearing and other escape shapes.
    let paths = match FlowPaths::try_new(root, &branch) {
        Some(p) => p,
        None => return err("invalid_branch", format!("invalid branch: {branch:?}")),
    };

    match write_approval(&paths.branch_dir(), &branch) {
        Ok(()) => (json!({"status": "ok", "branch": branch}), 0),
        Err(e) => err("write_failed", format!("failed to write approval: {e}")),
    }
}
