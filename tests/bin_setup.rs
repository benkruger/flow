//! Tests for `bin/setup` — the one-time install-flow build script.
//!
//! The script is invoked by users from their plain terminal after
//! `/plugin install` and before `/flow:flow-prime`. It checks for
//! `cargo` and `cc` prereqs and runs `cargo build --release` when
//! both are present. The tests assert structural contracts (existence,
//! executable bit, bash syntax, required content snippets) so an
//! accidental edit that drops a prereq check, the build invocation,
//! the success message, or the executable bit fails CI immediately.
//! The build itself is not invoked here — it would take minutes and
//! the structural assertions are a high-confidence proxy at this
//! script size.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

/// bin/setup must exist and be executable.
#[test]
fn script_is_executable() {
    let script = common::bin_dir().join("setup");
    assert!(script.exists(), "bin/setup must exist");
    let meta = fs::metadata(&script).unwrap();
    assert!(
        meta.permissions().mode() & 0o111 != 0,
        "bin/setup must be executable"
    );
}

/// bin/setup must contain valid bash syntax.
#[test]
fn script_is_valid_bash() {
    let script = common::bin_dir().join("setup");
    let output = Command::new("bash")
        .arg("-n")
        .arg(&script)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Syntax error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// bin/setup must contain the prereq checks, install hints, build
/// invocation, and success message that the install-flow docs
/// reference. Guards against accidental edits that drop any of these.
#[test]
fn script_contains_expected_install_flow() {
    let script = common::bin_dir().join("setup");
    let content = fs::read_to_string(&script).expect("bin/setup must be readable");
    let required = [
        "command -v cargo",
        "command -v cc",
        "brew install rust",
        "xcode-select --install",
        "cargo build --release",
        "Setup complete",
    ];
    for snippet in required {
        assert!(
            content.contains(snippet),
            "bin/setup must contain '{}'",
            snippet
        );
    }
}
