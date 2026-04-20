//! Generate a short UUID-derived identifier.
//!
//! Tests live at tests/generate_id.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use uuid::Uuid;

/// Generate an 8-character lowercase hex string from UUID4.
pub fn generate_id() -> String {
    Uuid::new_v4().as_simple().to_string()[..8].to_string()
}

/// CLI entry point — prints the ID to stdout.
pub fn run() {
    println!("{}", generate_id());
}
