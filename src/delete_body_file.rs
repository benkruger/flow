//! `bin/flow delete-body-file` — dispose of an edit-in-place issue-body
//! temp file.
//!
//! When a skill edits an existing GitHub issue in place it writes the new
//! body to a worktree-local temp file, runs `gh issue edit --body-file`,
//! and is then responsible for removing that temp file (unlike the create
//! path, where `bin/flow issue` self-cleans). This subcommand owns that
//! disposal so the only orphaning path routes through one validated,
//! audit-trailed delete.
//!
//! Path validation — a destructive operation on a caller-supplied path, so
//! a positive validator per `external-input-path-construction.md`: reject an
//! empty argument; reject `..` traversal in ANY path, absolute or relative
//! (`fs::remove_file` resolves `..` through the OS, so an unguarded segment
//! escapes the intended directory); resolve a relative path against an
//! injected `cwd` (a parameter, not ambient `env::current_dir`, so the
//! resolution branches are fixture-testable per `reachable-is-testable.md`);
//! require the final component to belong to the `.flow-issue-body` temp-file
//! family (the subcommand's sole legitimate target — a positive allowlist
//! so a mistargeted absolute path, or a regular file reached through a
//! symlinked parent, cannot delete an unrelated file); reject a target that
//! exists but is not a regular file (the final-component symlink/directory
//! is not followed or removed). Per `external-input-path-construction.md` no
//! `fs` call uses `.expect()`.
//!
//! A NUL byte or other structurally-invalid path is rejected natively by
//! `fs::symlink_metadata`/`fs::remove_file` returning an `Err` (the
//! `error` outcome), so no separate normalization is copied from the
//! sibling — this is a path argument consumed by `fs`, not an allowlist
//! gate over a domain value.

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::{json, Value};

/// CLI arguments for `bin/flow delete-body-file`.
#[derive(clap::Parser, Debug)]
#[command(name = "delete-body-file")]
pub struct Args {
    /// Path to the issue-body temp file to remove. Absolute, or relative
    /// to the process cwd (no `..` segments). The final component must
    /// belong to the `.flow-issue-body` family.
    #[arg(long)]
    pub path: String,
}

/// Required basename prefix of a disposable issue-body temp file. The
/// edit-in-place path writes `.flow-issue-body-<id>` (the create path writes
/// bare `.flow-issue-body`); restricting deletion to this family bounds a
/// destructive operation to its sole legitimate target so a mistargeted
/// `--path` cannot remove an unrelated file.
const BODY_FILE_PREFIX: &str = ".flow-issue-body";

/// Disposal core. Returns the outcome word on success
/// (`deleted` / `missing` / `error`) or an `Err` for a rejected path
/// (empty, `..` traversal, or an existing non-regular-file target).
///
/// `cwd` resolves a relative `--path`; it is injected so the
/// relative-resolution branches are testable without mutating the
/// process environment.
pub fn run_impl(args: &Args, cwd: &Path) -> Result<String, String> {
    let path = &args.path;
    if path.is_empty() {
        return Err("delete-body-file: --path argument is empty".to_string());
    }

    // Reject `..` in any path — absolute or relative. `fs::remove_file`
    // resolves `..` through the OS, so an unguarded segment escapes the
    // intended directory regardless of whether the path is absolute.
    if Path::new(path)
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(format!(
            "delete-body-file: path '{}' contains forbidden `..` traversal segments",
            path
        ));
    }

    let resolved: PathBuf = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        cwd.join(path)
    };

    // Positive allowlist: the final component must belong to the
    // `.flow-issue-body` temp-file family. This bounds the deletable set to
    // the subcommand's sole legitimate target, so an arbitrary file reached
    // by an absolute path — or a regular file behind a symlinked parent
    // directory — is rejected before any removal.
    let basename_ok = match resolved.file_name() {
        Some(name) => name.to_string_lossy().starts_with(BODY_FILE_PREFIX),
        None => false,
    };
    if !basename_ok {
        return Err(format!(
            "delete-body-file: '{}' is not a '{}' temp file",
            resolved.display(),
            BODY_FILE_PREFIX
        ));
    }

    // Reject a target that exists but is not a regular file (the
    // final-component symlink or directory must not be followed or removed).
    // A stat failure (the file is absent, or its parent is unreadable) falls
    // through — the `fs::remove_file` match below reports `missing` or
    // `error`.
    if let Ok(meta) = fs::symlink_metadata(&resolved) {
        if !meta.file_type().is_file() {
            return Err(format!(
                "delete-body-file: '{}' is not a regular file",
                resolved.display()
            ));
        }
    }

    match fs::remove_file(&resolved) {
        Ok(()) => Ok("deleted".to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok("missing".to_string()),
        Err(_) => Ok("error".to_string()),
    }
}

/// Main-arm wrapper: maps `run_impl` to the JSON envelope and exit code.
/// `Ok(outcome)` → `{"status":"ok","outcome":<word>}` exit 0;
/// `Err(msg)` → `{"status":"error","message":<msg>}` exit 1.
pub fn run_impl_main(args: &Args, cwd: &Path) -> (Value, i32) {
    match run_impl(args, cwd) {
        Ok(outcome) => (json!({ "status": "ok", "outcome": outcome }), 0),
        Err(msg) => (json!({ "status": "error", "message": msg }), 1),
    }
}
