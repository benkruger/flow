//! `bin/flow base-branch` — print the integration branch this flow
//! coordinates against, the value originally captured at flow-start
//! by `init_state` from `git symbolic-ref --short
//! refs/remotes/origin/HEAD`.
//!
//! This is the skill-side single source of truth for the base branch.
//! Skills shell into `bin/flow base-branch` and interpolate the result
//! into git diff ranges, AskUserQuestion text, and other prose so a
//! repo whose default branch is e.g. `staging` coordinates against its
//! actual integration branch instead of a hardcoded `main`. The Rust
//! side reads through `git::read_base_branch`; both sides hit the
//! same `state["base_branch"]` field, eliminating drift between the
//! two consumers.
//!
//! Returns the value to stdout with a trailing newline and exits 0
//! on success. On any error (no current branch, invalid `--branch`
//! input per `FlowPaths::try_new`, missing or corrupt state file,
//! field absent or wrong type) the message lands on stderr and the
//! process exits non-zero — never silently substitutes `"main"`.
//! Tests live at `tests/base_branch_cmd.rs` and drive the binary
//! through `CARGO_BIN_EXE_flow-rs`.

use std::path::Path;

use crate::flow_paths::FlowPaths;
use crate::git::{read_base_branch, resolve_branch};

/// Main-arm dispatcher for `bin/flow base-branch`. Returns
/// `Ok((value, 0))` with the base-branch value (no trailing newline —
/// `dispatch::dispatch_text` adds one via `println!`) when the read
/// succeeds, or `Err((msg, code))` for every failure class. `code`
/// is `2` for input-resolution failures (no current branch, invalid
/// `--branch` override) and `1` for state-file failures (missing,
/// empty, parse error, missing field, wrong type).
///
/// Per `.claude/rules/external-input-validation.md` and
/// `.claude/rules/branch-path-safety.md`, `--branch` overrides come
/// from the shell (untrusted) and must route through
/// `FlowPaths::try_new` rather than `FlowPaths::new` so a
/// slash-containing or empty branch produces a structured error
/// rather than a panic.
pub fn run_impl_main(
    branch_override: Option<&str>,
    root: &Path,
) -> Result<(String, i32), (String, i32)> {
    let branch = match resolve_branch(branch_override, root) {
        Some(b) => b,
        None => return Err(("Could not determine current branch".to_string(), 2)),
    };
    let paths = match FlowPaths::try_new(root, &branch) {
        Some(p) => p,
        None => {
            return Err((
                format!(
                    "invalid branch '{}': empty, '.', '..', or contains '/' or NUL",
                    branch
                ),
                2,
            ));
        }
    };
    match read_base_branch(&paths.state_file()) {
        Ok(value) => Ok((value, 0)),
        Err(msg) => Err((msg, 1)),
    }
}
