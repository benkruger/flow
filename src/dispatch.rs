//! Shared CLI dispatch helpers for `main.rs` match arms.
//!
//! Match arms whose owning module exposes a `run_impl_main` pure
//! function call one of these helpers to print the result and exit.
//! Centralizing the print-then-exit pair keeps match arms short and
//! ensures a uniform output contract: JSON for structured commands,
//! plain text for human-readable output.

use serde_json::Value;

/// Serialize `result` as JSON to stdout, then exit with `code`.
pub fn dispatch_json(result: Value, code: i32) -> ! {
    println!("{}", serde_json::to_string(&result).unwrap());
    std::process::exit(code)
}

/// Print `text` to stdout when non-empty, then exit with `code`.
/// Empty strings produce no output so callers can represent a
/// success-with-no-text result without an extra blank line.
pub fn dispatch_text(text: &str, code: i32) -> ! {
    if !text.is_empty() {
        println!("{}", text);
    }
    std::process::exit(code)
}
