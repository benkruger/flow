//! Integration tests for `bin/flow update-deps`.
//!
//! Drives the compiled binary against a minimal project fixture so the
//! `run()` entry point and its dispatch into `update_deps::run_impl` are
//! exercised end-to-end. Matches the subprocess-hygiene pattern used in
//! `tests/main_dispatch.rs`.

use std::process::Command;

fn flow_rs_no_recursion() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd
}

/// `flow-rs update-deps --help` covers the Args clap parser and help path.
#[test]
fn update_deps_help_exits_0() {
    let output = flow_rs_no_recursion()
        .args(["update-deps", "--help"])
        .output()
        .expect("spawn flow-rs update-deps --help");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Usage:"),
        "expected Usage: header in --help output, got: {}",
        stdout
    );
}

/// `flow-rs update-deps` in a tempdir without Cargo.toml does not
/// panic — the module reports a structured result on stdout via its
/// dispatcher.
#[test]
fn update_deps_empty_tempdir_does_not_panic() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");

    let output = flow_rs_no_recursion()
        .arg("update-deps")
        .current_dir(&root)
        .env("GIT_CEILING_DIRECTORIES", &root)
        .env("GH_TOKEN", "invalid")
        .env("HOME", &root)
        .output()
        .expect("spawn flow-rs update-deps");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked at"),
        "update-deps must not panic outside a cargo project, got: {}",
        stderr
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":"),
        "update-deps must emit JSON status on stdout, got: {}",
        stdout
    );
}
