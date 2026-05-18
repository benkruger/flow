//! `bin/flow base-branch` — print the integration branch this flow
//! coordinates against.
//!
//! Thin wrapper around `git::default_branch_in`. Git is the single
//! source of truth for the integration branch — the subcommand
//! queries `git symbolic-ref --short refs/remotes/origin/HEAD` from
//! the project root and prints the resolved branch name (`main`,
//! `staging`, `develop`, …) on stdout with a trailing newline.
//!
//! Returns exit code 0 on success. On any git failure (no `origin`
//! remote, symbolic-ref unset, non-git directory, git binary
//! unavailable) the message lands on stderr and the process exits 1.
//!
//! Tests live at `tests/base_branch_cmd.rs` and drive the binary
//! through `CARGO_BIN_EXE_flow-rs`.

use std::path::Path;

use crate::git::default_branch_in;

/// Main-arm dispatcher for `bin/flow base-branch`. Returns
/// `Ok((value, 0))` with the integration branch name (no trailing
/// newline — `dispatch::dispatch_text` adds one via `println!`) when
/// git resolves the symbolic-ref cleanly. Returns `Err((msg, 1))`
/// when git cannot determine the integration branch.
///
/// Reads the integration branch directly from git rather than from
/// any state file: every FLOW phase trusts git as the authoritative
/// source for the integration branch, so the subcommand stays
/// branch-agnostic (no `--branch` flag, no state-file lookup).
pub fn run_impl_main(root: &Path) -> Result<(String, i32), (String, i32)> {
    match default_branch_in(root) {
        Ok(value) => Ok((value, 0)),
        Err(msg) => Err((msg, 1)),
    }
}
