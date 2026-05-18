//! `bin/flow reset` — Rust shim that exec's the existing
//! `${CLAUDE_PLUGIN_ROOT}/bin/reset` bash script.
//!
//! Routes `/flow:flow-reset` through the canonical `bin/flow`
//! dispatcher covered by `Bash(*bin/flow *)` so model-invoked reset
//! calls reuse the single sanctioned `bin/flow` allow entry rather
//! than a script-specific wildcard. The bash script body stays in
//! bash because it does non-trivial `git rev-parse` discovery across
//! worktrees, submodules, and bare repos that has no Rust win to
//! port. See `.claude/rules/permissions.md` "bin/flow Dispatch First"
//! for the dispatch-first principle this module embodies.
//!
//! Tests live at `tests/reset.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` in
//! this file.

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use clap::Args as ClapArgs;
use serde_json::{json, Value};

#[derive(ClapArgs)]
pub struct Args {}

/// Resolve `plugin_root` and exec the bash script. Returns the error
/// envelope and exit code for the two reachable failure branches: a
/// missing plugin root (None) and an exec syscall failure.
///
/// The happy path does not return — `Command::exec()` replaces the
/// current process image with the bash script, so the script's stdout
/// becomes the binary's stdout and the script's exit code becomes the
/// binary's exit code. `(Value, i32)` here is the failure-only
/// contract.
pub fn run_impl_main(plugin_root: Option<PathBuf>) -> (Value, i32) {
    let root = match plugin_root {
        Some(r) => r,
        None => {
            return (
                json!({"status": "error", "message": "Plugin root not found"}),
                1,
            );
        }
    };
    let script = root.join("bin").join("reset");
    let err = Command::new(&script).exec();
    (
        json!({
            "status": "error",
            "message": format!("Could not exec {}: {}", script.display(), err),
        }),
        1,
    )
}
