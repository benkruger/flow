//! Branch-scoped, per-file, single-use approval marker store for the
//! shared-config gate.
//!
//! The shared-config gate (`validate_worktree_paths::validate_shared_config`)
//! blocks Edit/Write on `.gitignore`/`Cargo.toml`/etc. inside a
//! worktree and instructs the user to reply with the exact line
//! `approve shared-config: <path>`. This module is the "proceed"
//! half: after that reply, `bin/flow approve-shared-config`
//! self-gates on the transcript (the user-typed phrase is the
//! unforgeable anchor — same trust model as `clear-halt`) and
//! writes a marker here; the gate then consults+consumes it
//! immediately before its block return — allowing exactly one edit
//! of exactly that file. The model never fires an `AskUserQuestion`
//! for shared-config; the user-typed phrase is the authorization.
//!
//! Three invariants the store enforces:
//!
//! - **Single-use.** Consumption deletes the marker. A second edit of
//!   the same file finds no marker and re-blocks. There is no
//!   "consumed" flag — file presence IS the unconsumed state.
//! - **Per-file scope (defense-in-depth).** The marker's on-disk
//!   filename is the SHA-256 hex of the full target path, so an
//!   approval for path A is stored at a different filename than an
//!   approval for path B. The marker body ALSO carries the target
//!   path and `check_and_consume_approval` re-verifies it, so a
//!   hand-moved marker file cannot satisfy a check for a different
//!   path.
//! - **Fail-closed corruption resilience.** Any unreadable,
//!   oversized, unparseable, wrong-root-type, `approved != true`, or
//!   target-mismatched marker yields no approval. The gate then
//!   still blocks — a corrupt marker can never become an escape
//!   hatch. This is the deliberate asymmetry vs. Layer 11
//!   (friction-prevention, fail-open): this gate is protective.
//!
//! Markers live at
//! `<project_root>/.flow-states/<branch>/shared-config-approvals/<sha256(target)>`
//! — branch-scoped under `.flow-states/` (project root, never the
//! worktree) so concurrent flows never collide and
//! `flow-abort`/`flow-complete` cleanup removes them with the
//! branch subdirectory. `clear_all` additionally clears them on
//! phase advance so a stale approval cannot bleed into a later phase.
//!
//! The branch reaches filesystem path construction only through
//! `FlowPaths::try_new`, which rejects empty / `.` / `..` /
//! `/`-bearing / NUL-bearing branches per
//! `.claude/rules/branch-path-safety.md`. The target path never
//! reaches path construction directly — it is hashed to a fixed
//! `[0-9a-f]{64}` filename, so a traversal-shaped target cannot
//! escape the approvals directory.
//!
//! Tests live at `tests/shared_config_approval.rs` per
//! `.claude/rules/test-placement.md`.

use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::flow_paths::FlowPaths;

/// Maximum bytes read from a marker file. Markers this module writes
/// are a few dozen bytes of JSON; the cap bounds I/O when the marker
/// path holds a corrupted or hostile oversized file (a hand-edit, an
/// interrupted unrelated write, a symlink to a large file). Per
/// `.claude/rules/external-input-path-construction.md` every external
/// read enforces a documented byte cap.
const MARKER_BYTE_CAP: u64 = 64 * 1024;

/// Directory (under the branch dir) that holds one marker file per
/// approved target path.
const APPROVALS_SUBDIR: &str = "shared-config-approvals";

/// The on-disk marker filename for `target_path`: the SHA-256 hex of
/// the full path string. Collision-safe (cryptographic digest) and
/// filesystem-safe (`[0-9a-f]{64}`, no separators or traversal
/// segments), so distinct target paths never share a marker and a
/// traversal-shaped target cannot escape the approvals directory.
fn target_key(target_path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(target_path.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// The marker file path for `(branch, target_path)`, or `None` when
/// `branch` fails `FlowPaths::is_valid_branch` (empty / `.` / `..` /
/// `/`-bearing / NUL-bearing). Callers treat `None` as "no approval
/// possible" — the gate keeps blocking, the subcommand returns a
/// structured error.
pub fn marker_path(root: &Path, branch: &str, target_path: &str) -> Option<PathBuf> {
    let paths = FlowPaths::try_new(root, branch)?;
    Some(
        paths
            .branch_dir()
            .join(APPROVALS_SUBDIR)
            .join(target_key(target_path)),
    )
}

/// Write an approval marker authorizing exactly one subsequent edit
/// of `target_path` under `branch`. Creates the branch-scoped
/// approvals directory if absent. Returns `Err` when `branch` is
/// invalid (no path can be constructed) or on any filesystem failure
/// — the caller surfaces a structured error rather than silently
/// approving.
pub fn write_approval(root: &Path, branch: &str, target_path: &str) -> io::Result<()> {
    let path = marker_path(root, branch, target_path).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid branch name: {branch:?}"),
        )
    })?;
    // `marker_path` always yields `<branch_dir>/shared-config-approvals/<hash>`
    // — a path at least three components deep — so `.parent()` is
    // structurally `Some`. The `.expect` documents the invariant; it
    // is unreachable, not a panic vector.
    let parent = path
        .parent()
        .expect("marker_path yields a path with a parent directory");
    fs::create_dir_all(parent)?;
    let body = json!({ "approved": true, "target": target_path });
    fs::write(&path, body.to_string())
}

/// Consult and consume the approval for `(branch, target_path)`.
///
/// Returns `true` iff a valid, unconsumed marker existed AND was
/// successfully deleted (single-use consume-on-allow). Every other
/// outcome returns `false` so the gate keeps blocking:
///
/// - invalid branch (no marker path constructible)
/// - missing / unreadable marker
/// - marker larger than `MARKER_BYTE_CAP`
/// - non-JSON or wrong-root-type content
/// - `approved` not boolean `true`
/// - `target` field absent or not equal to `target_path`
/// - the marker existed and validated but `fs::remove_file` failed
///   (fail-closed: if it cannot be consumed it must not authorize,
///   so a subsequent edit cannot reuse the same marker)
pub fn check_and_consume_approval(root: &Path, branch: &str, target_path: &str) -> bool {
    let path = match marker_path(root, branch, target_path) {
        Some(p) => p,
        None => return false,
    };
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
    if obj.get("target").and_then(Value::as_str) != Some(target_path) {
        return false;
    }
    // Valid + unconsumed: deleting the marker IS the consume. Only
    // report approval when the delete succeeds, so a failed remove
    // cannot leave a reusable marker behind.
    fs::remove_file(&path).is_ok()
}

/// Remove every approval marker for `branch` (best-effort). Called on
/// phase advance so an approval written in one phase cannot bleed
/// into the next. No-op when `branch` is invalid or the approvals
/// directory does not exist; filesystem errors are swallowed because
/// a failed clear must never block phase advance.
pub fn clear_all(root: &Path, branch: &str) {
    if let Some(paths) = FlowPaths::try_new(root, branch) {
        let dir = paths.branch_dir().join(APPROVALS_SUBDIR);
        let _ = fs::remove_dir_all(dir);
    }
}
