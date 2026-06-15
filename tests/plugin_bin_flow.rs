//! Subprocess tests for `bin/flow plugin-bin-flow`. Mirrors
//! `src/plugin_bin_flow.rs`. Each test spawns the compiled `flow-rs`
//! binary with a controlled `CLAUDE_PLUGIN_ROOT` value and asserts the
//! resolved-path / structured-error contract.
//!
//! The subcommand resolves the absolute plugin `bin/flow` path so the
//! parent skill can substitute it into a sub-agent command instead of
//! the unexpanded `${CLAUDE_PLUGIN_ROOT}/bin/flow` token. On a missing,
//! empty, or non-absolute `CLAUDE_PLUGIN_ROOT` it must emit a non-zero
//! structured error (never a path, never a panic) so every consumer
//! halts rather than embedding a non-path string into an agent prompt.
//!
//! Subprocess hygiene per `.claude/rules/subprocess-test-hygiene.md`:
//! every spawn neutralizes `GH_TOKEN`, `HOME`, and `FLOW_CI_RUNNING`,
//! and sets `CLAUDE_PLUGIN_ROOT` explicitly so the child never inherits
//! the runner's value.

use std::process::{Command, Output};

/// Run `flow-rs plugin-bin-flow` with the given `CLAUDE_PLUGIN_ROOT`
/// value (`None` removes the variable from the child env). Returns the
/// captured Output.
fn run_plugin_bin_flow(plugin_root: Option<&str>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.arg("plugin-bin-flow")
        .env("GH_TOKEN", "invalid")
        .env("HOME", "/tmp")
        .env_remove("FLOW_CI_RUNNING");
    match plugin_root {
        Some(v) => {
            cmd.env("CLAUDE_PLUGIN_ROOT", v);
        }
        None => {
            cmd.env_remove("CLAUDE_PLUGIN_ROOT");
        }
    }
    cmd.output().expect("spawn flow-rs plugin-bin-flow")
}

#[test]
fn plugin_bin_flow_prints_absolute_path_when_root_is_absolute() {
    let output = run_plugin_bin_flow(Some("/abs/plugin/root"));
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "/abs/plugin/root/bin/flow\n");
}

#[test]
fn plugin_bin_flow_errors_when_root_unset() {
    let output = run_plugin_bin_flow(None);
    assert_ne!(
        output.status.code(),
        Some(0),
        "expected non-zero exit when CLAUDE_PLUGIN_ROOT is unset"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked at"),
        "must not panic when unset; stderr: {}",
        stderr
    );
    assert!(
        !stderr.is_empty(),
        "expected structured stderr message when unset, got empty stderr"
    );
    // Never emit a path on the error path.
    assert!(
        String::from_utf8_lossy(&output.stdout).trim().is_empty(),
        "stdout must be empty on the error path"
    );
}

#[test]
fn plugin_bin_flow_errors_when_root_empty() {
    let output = run_plugin_bin_flow(Some(""));
    assert_ne!(
        output.status.code(),
        Some(0),
        "expected non-zero exit when CLAUDE_PLUGIN_ROOT is empty"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.is_empty(),
        "expected structured stderr message when empty, got empty stderr"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).trim().is_empty(),
        "stdout must be empty on the error path"
    );
}

#[test]
fn plugin_bin_flow_errors_when_root_not_absolute() {
    let output = run_plugin_bin_flow(Some("relative/plugin/root"));
    assert_ne!(
        output.status.code(),
        Some(0),
        "expected non-zero exit when CLAUDE_PLUGIN_ROOT is not absolute"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.is_empty(),
        "expected structured stderr message when non-absolute, got empty stderr"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).trim().is_empty(),
        "stdout must be empty on the error path"
    );
}

#[test]
fn plugin_bin_flow_trims_surrounding_whitespace_in_root() {
    // A trailing-whitespace root (e.g. an env value with a stray
    // newline) must resolve to the same clean path the trimmed
    // agent_prompt_scan carve-out admits — not a space-bearing path the
    // carve-out could never match.
    let output = run_plugin_bin_flow(Some("  /abs/plugin/root  "));
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "/abs/plugin/root/bin/flow\n"
    );
}

// --- run_impl hygiene (unit) ---
//
// These call `run_impl` directly so the NUL-strip + trim hygiene is
// covered for inputs a subprocess env value cannot carry (an interior
// NUL is rejected by `Command::env`). `run_impl` is pure over its
// argument, so no ambient-env neutralization is needed.

#[test]
fn run_impl_strips_nul_bytes_matching_the_carveout() {
    let (path, code) = flow_rs::plugin_bin_flow::run_impl(Some("/abs/plugin\0root"))
        .expect("absolute root resolves");
    assert_eq!(code, 0);
    assert!(
        !path.contains('\0'),
        "run_impl must strip NUL bytes so its path matches the NUL-stripping carve-out; got {path:?}"
    );
    assert_eq!(path, "/abs/pluginroot/bin/flow");
}

#[test]
fn run_impl_trims_whitespace_matching_the_carveout() {
    let (path, _) = flow_rs::plugin_bin_flow::run_impl(Some(" /abs/plugin/root\n"))
        .expect("absolute root resolves");
    assert_eq!(path, "/abs/plugin/root/bin/flow");
}

#[test]
fn run_impl_errors_when_whitespace_only_after_trim() {
    let err = flow_rs::plugin_bin_flow::run_impl(Some("   \t  ")).unwrap_err();
    assert_eq!(err.1, 1);
    assert!(err.0.contains("unset or empty"), "got {}", err.0);
}
