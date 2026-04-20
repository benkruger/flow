//! Tests for `bin/build` — the FLOW dogfood build script.
//!
//! In this repo `bin/build` is a no-op: compilation happens inside
//! `bin/test` via `cargo-llvm-cov nextest`. A real cargo build here
//! would duplicate that work. See CLAUDE.md "Development Environment"
//! and `.claude/rules/tool-dispatch.md`.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

/// bin/build must exist and be executable.
#[test]
fn script_is_executable() {
    let script = common::bin_dir().join("build");
    assert!(script.exists(), "bin/build must exist");
    let meta = fs::metadata(&script).unwrap();
    assert!(
        meta.permissions().mode() & 0o111 != 0,
        "bin/build must be executable"
    );
}

/// bin/build must contain valid bash syntax.
#[test]
fn script_is_valid_bash() {
    let script = common::bin_dir().join("build");
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

/// bin/build is a no-op: exits 0 and does not shell out to cargo.
/// Asserted structurally by scanning the script source for the
/// cargo-invocation patterns that would reintroduce the compile step.
/// The explanatory `echo` message can mention "cargo-llvm-cov" as
/// prose; what we guard against is an executable `cargo <subcmd>`.
#[test]
fn script_does_not_invoke_cargo() {
    let script = common::bin_dir().join("build");
    let content = fs::read_to_string(&script).unwrap();
    const FORBIDDEN: &[&str] = &[
        "exec cargo",
        "cargo build",
        "cargo check",
        "cargo test",
        "cargo nextest",
        "cargo llvm-cov",
    ];
    for pattern in FORBIDDEN {
        assert!(
            !content.contains(pattern),
            "bin/build must not contain `{}` — no-op by design",
            pattern
        );
    }
}

#[test]
fn no_op_exits_0() {
    let script = common::bin_dir().join("build");
    let output = Command::new(&script).output().unwrap();
    assert!(
        output.status.success(),
        "bin/build must exit 0, got: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

/// bin/build surfaces a message on stderr directing the reader to
/// `bin/test`, so anyone invoking it habitually sees the redirection.
#[test]
fn prints_redirect_message_on_stderr() {
    let script = common::bin_dir().join("build");
    let output = Command::new(&script).output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("bin/test"),
        "expected stderr to mention bin/test, got: {}",
        stderr
    );
    assert!(
        stderr.contains("no-op"),
        "expected stderr to say no-op, got: {}",
        stderr
    );
}
