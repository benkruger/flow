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
//! The path validation mirrors the sibling `issue::read_body_file`
//! (`src/issue.rs`) minus the body read and byte cap: reject an empty
//! argument; reject `..` traversal in a relative path; resolve a relative
//! path against an injected `cwd` (a parameter, not ambient
//! `env::current_dir`, so the relative/`..` branches are fixture-testable
//! per `reachable-is-testable.md`); reject a target that exists but is not
//! a regular file (a symlink or directory must not be followed/removed).
//! Per `external-input-path-construction.md` no `fs` call uses `.expect()`.
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
    /// to the process cwd (no `..` segments).
    #[arg(long)]
    pub path: String,
}

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

    let resolved: PathBuf = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        if Path::new(path)
            .components()
            .any(|c| matches!(c, Component::ParentDir))
        {
            return Err(format!(
                "delete-body-file: path '{}' contains forbidden `..` traversal segments",
                path
            ));
        }
        cwd.join(path)
    };

    // Reject a target that exists but is not a regular file (a symlink or
    // directory must not be followed or removed). A stat failure (the file
    // is absent, or its parent is unreadable) falls through — the
    // `fs::remove_file` match below reports `missing` or `error`.
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
