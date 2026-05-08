//! Cargo's directory-form auto-discover layout for `tests/commands/`.
//!
//! With `autotests = true` (default), Cargo discovers a single binary
//! per directory under `tests/` rooted at `<dir>/main.rs`, registering
//! sibling `.rs` files as modules ONLY when declared via `mod` here.
//! This binary is named `commands` and bundles every
//! `tests/commands/<name>.rs` file as a `commands::<name>` module —
//! replacing the previous one-binary-per-file layout that required
//! `[[test]]` stanzas in `Cargo.toml`.
//!
//! `tests/common/mod.rs` is shared infrastructure; the path-aliased
//! `mod common;` declaration here exposes it to every sibling module
//! via `crate::common`.

#[path = "../common/mod.rs"]
mod common;

mod init_state;
mod set_timestamp;
mod start_lock;

#[allow(dead_code)]
fn main() {}
