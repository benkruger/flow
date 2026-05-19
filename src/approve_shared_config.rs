//! `bin/flow approve-shared-config --path <file>` — the user-driven
//! "proceed" half of the shared-config gate.
//!
//! The shared-config gate (`validate_worktree_paths::validate_shared_config`)
//! blocks Edit/Write on `.gitignore`/`Cargo.toml`/etc. inside a
//! worktree and instructs the user to grant the edit by typing the
//! fixed phrase `approve shared-config: <path>`. This subcommand,
//! invoked after that phrase, writes a single-use approval marker
//! (`shared_config_approval::write_approval`) that the gate then
//! consults+consumes on the next edit.
//!
//! Forgery model — identical anchor to `clear-halt`: the marker
//! itself is forgeable (any Bash call can invoke this subcommand),
//! so the subcommand SELF-GATES on the persisted transcript via
//! `transcript_walker::user_approved_shared_config_edit`. That
//! walker requires the most recent real user turn (a turn Claude
//! Code marks so the model cannot synthesize it) to carry the fixed
//! `approve shared-config: <path>` phrase AND a system-emitted
//! shared-config BLOCK naming the target in the same exchange.
//! Neither signal is model-forgeable, so the subcommand cannot be
//! used to self-authorize an edit the user never granted.
//!
//! Output shape (callers parse `status`/`reason`; exit 0 ok, exit 1
//! on any rejection so a non-grant never silently writes a marker):
//! - `{"status":"ok"}` — marker written for `(branch, path)`.
//! - `{"status":"error","reason":"cwd_drift",...}` — the
//!   state-mutator cwd guard (`cwd_scope::enforce`) rejected.
//! - `{"status":"error","reason":"invalid_branch"}` — branch
//!   undetectable or fails `FlowPaths::is_valid_branch`.
//! - `{"status":"error","reason":"invalid_path"}` — `--path` is
//!   empty, NUL-bearing, relative, or contains a `..` component.
//! - `{"status":"error","reason":"path_outside_worktree"}` —
//!   `--path` does not resolve under the flow's git worktree.
//! - `{"status":"error","reason":"no_state_file"}` — no active
//!   flow for the branch.
//! - `{"status":"error","reason":"no_transcript_path"}` — state
//!   lacks a usable `session_id`/`transcript_path`.
//! - `{"status":"error","reason":"not_user_approved"}` — the
//!   transcript does not show a genuine per-file user grant.
//! - `{"status":"error","reason":"write_failed"}` — the marker
//!   write failed (filesystem error).
//!
//! Tests live at `tests/approve_shared_config.rs` per
//! `.claude/rules/test-placement.md`.

use std::path::{Component, Path, PathBuf};
use std::process::Command;

use clap::Parser;
use serde_json::{json, Value};

use crate::flow_paths::FlowPaths;
use crate::git::resolve_branch_in;
use crate::hooks::transcript_walker::user_approved_shared_config_edit;
use crate::per_flow_capture::derive_transcript_path;
use crate::session_metrics::{is_safe_session_id, is_safe_transcript_path};
use crate::shared_config_approval::write_approval;

#[derive(Parser, Debug)]
#[command(
    name = "approve-shared-config",
    about = "Record a single-use user grant to edit a shared-config file"
)]
pub struct Args {
    /// Absolute path of the shared-config file the user granted.
    /// Must be the exact path the Edit/Write tool targets — the
    /// marker is keyed on this string and the gate consults the
    /// same string.
    #[arg(long)]
    pub path: String,

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

/// Reject `--path` shapes that are unsafe before any filesystem or
/// containment work: empty, NUL-bearing, relative, or carrying a
/// `..` (ParentDir) component. Per
/// `.claude/rules/external-input-path-construction.md` the
/// validator runs at the boundary before path construction.
fn path_shape_ok(path: &str) -> bool {
    if path.is_empty() || path.contains('\0') {
        return false;
    }
    let p = Path::new(path);
    if !p.is_absolute() {
        return false;
    }
    !p.components().any(|c| matches!(c, Component::ParentDir))
}

/// git worktree root for `cwd` (`git rev-parse --show-toplevel`),
/// matching `cwd_scope`'s notion of the worktree. `None` when `cwd`
/// is not git-managed (non-zero exit) — the caller fails closed
/// (`path_outside_worktree`).
///
/// Unreachability proof for the `.output().expect(...)` (per
/// `.claude/rules/external-input-path-construction.md` "No
/// `.expect()` on Filesystem Reads in Hooks or CLI Subcommands" —
/// the carve-out requires a proof the arm cannot be reached from
/// any production path): `Command::output()` only `Err`s when the
/// `git` binary cannot be spawned. `run_impl_main` calls
/// `cwd_scope::enforce(cwd, root)` BEFORE `worktree_root(cwd)`, and
/// `cwd_scope::enforce` itself runs `git rev-parse` with an
/// identical `.expect` — so in any environment where `git` cannot
/// spawn, that earlier call panics first and this arm is never
/// reached. The `.expect` here is documentation of that invariant,
/// not a reachable panic vector. A future refactor that moves this
/// call ahead of `cwd_scope::enforce` (or removes the cwd guard)
/// must convert this to a non-panicking `.ok()?` (the function
/// already returns `Option`, so `None` → `path_outside_worktree`).
fn worktree_root(cwd: &Path) -> Option<PathBuf> {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .expect("git is a hard dependency (cwd_scope precedent)");
    if !out.status.success() {
        return None;
    }
    // `--show-toplevel` on success always prints an existing path.
    Some(PathBuf::from(
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
    ))
}

/// True when `path` (already shape-validated: absolute, no `..`)
/// resolves inside `worktree`. The parent directory is
/// canonicalized so a `/var` vs `/private/var` symlink (macOS
/// tempdirs) cannot produce a spurious mismatch. Fails closed: a
/// rootless path or an unresolvable parent returns `false`.
fn path_inside_worktree(path: &str, worktree: &Path) -> bool {
    let parent = match Path::new(path).parent() {
        Some(p) => p,
        None => return false,
    };
    match (parent.canonicalize(), worktree.canonicalize()) {
        (Ok(p), Ok(w)) => p.starts_with(&w),
        _ => false,
    }
}

/// Resolve a validated transcript path from state's `session_id` /
/// `transcript_path`. Mirrors the `clear-halt` resolver: both
/// reuse the shared `is_safe_session_id` / `is_safe_transcript_path`
/// validators and `derive_transcript_path`. Returns `None` when
/// neither field yields a path under `<home>/.claude/projects/`.
fn resolve_transcript_path(state: &Value, home: &Path, project_root: &Path) -> Option<PathBuf> {
    let session_id = state
        .get("session_id")
        .and_then(|v| v.as_str())
        .filter(|s| is_safe_session_id(s))
        .map(|s| s.to_string());
    state
        .get("transcript_path")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .filter(|p| is_safe_transcript_path(p, home))
        .or_else(|| {
            session_id
                .as_ref()
                .map(|sid| derive_transcript_path(home, project_root, sid))
                .filter(|p| is_safe_transcript_path(p, home))
        })
}

/// Main-arm dispatcher that accepts `cwd` as a `Result` so the
/// `current_dir()`-failure fallback (deleted-cwd / chroot) lives in
/// the module where a unit test can drive it — keeping the
/// `src/main.rs` arm a closure-free one-liner. Mirrors
/// `add_finding::run_impl_main_with_cwd_result`.
pub fn run_impl_main_with_cwd_result(
    args: &Args,
    root: &Path,
    cwd_result: std::io::Result<PathBuf>,
    home: &Path,
) -> (Value, i32) {
    let cwd = cwd_result.unwrap_or(PathBuf::from("."));
    run_impl_main(args, root, &cwd, home)
}

/// Main-arm dispatcher. `cwd` is the subcommand's working directory
/// (inside the flow worktree); `home` is the user's home for
/// transcript resolution. Exit code is `1` on every rejection so a
/// non-grant can never silently produce an approval marker.
pub fn run_impl_main(args: &Args, root: &Path, cwd: &Path, home: &Path) -> (Value, i32) {
    // State-mutator cwd guard (rust-patterns "Guard Universality
    // Across CLI Entry Points"): this subcommand writes a marker, so
    // it must enforce the same drift guard as other state mutators.
    if let Err(message) = crate::cwd_scope::enforce(cwd, root) {
        return err("cwd_drift", message);
    }

    let branch = match resolve_branch_in(args.branch.as_deref(), cwd, root) {
        Some(b) => b,
        None => return err("invalid_branch", "could not determine branch"),
    };
    let paths = match FlowPaths::try_new(root, &branch) {
        Some(p) => p,
        None => return err("invalid_branch", format!("invalid branch: {branch:?}")),
    };

    if !path_shape_ok(&args.path) {
        return err(
            "invalid_path",
            "--path must be a non-empty absolute path with no NUL byte and no `..` segment",
        );
    }
    match worktree_root(cwd) {
        Some(wt) if path_inside_worktree(&args.path, &wt) => {}
        _ => {
            return err(
                "path_outside_worktree",
                "--path must resolve inside the flow's git worktree",
            )
        }
    }

    let state_path = paths.state_file();
    if !state_path.exists() {
        return err("no_state_file", "no active flow for this branch");
    }
    let state: Value = match std::fs::read_to_string(&state_path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
    {
        Some(v) => v,
        None => return err("no_state_file", "state file unreadable or unparseable"),
    };
    let transcript_path = match resolve_transcript_path(&state, home, root) {
        Some(p) => p,
        None => return err("no_transcript_path", "state has no usable transcript path"),
    };

    // Forgery self-gate: the marker is model-writable, so authority
    // comes from the unforgeable transcript signal, not the call.
    if !user_approved_shared_config_edit(&transcript_path, home, &args.path) {
        return err(
            "not_user_approved",
            "transcript does not show a per-file user grant for this path in the current exchange",
        );
    }

    // Gate-action atomicity: the walker and the marker key on the
    // same `args.path` (shape-validated, never transformed), which
    // is the exact string the gate later consults.
    match write_approval(root, &branch, &args.path) {
        Ok(()) => (json!({"status": "ok"}), 0),
        Err(e) => err("write_failed", format!("failed to write approval: {e}")),
    }
}
