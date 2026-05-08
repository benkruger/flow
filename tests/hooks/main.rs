//! Cargo's directory-form auto-discover layout for `tests/hooks/`.
//!
//! With `autotests = true` (default), Cargo discovers a single binary
//! per directory under `tests/` rooted at `<dir>/main.rs`, registering
//! sibling `.rs` files as modules ONLY when declared via `mod` here.
//! This binary is named `hooks` and bundles every `tests/hooks/<name>.rs`
//! file as a `hooks::<name>` module — replacing the previous one-binary-
//! per-file layout that required `[[test]]` stanzas in `Cargo.toml`.
//!
//! `tests/common/mod.rs` is shared infrastructure; the path-aliased
//! `mod common;` declaration here exposes it to every sibling module
//! via `crate::common`.

#[path = "../common/mod.rs"]
mod common;

mod dispatcher;
mod shared;
mod stop_continue;
mod transcript_walker;
mod validate_ask_user;
mod validate_claude_paths;
mod validate_pretool;
mod validate_skill;
mod validate_worktree_paths;

#[allow(dead_code)]
fn main() {}
