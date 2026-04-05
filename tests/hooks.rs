//! Integration tests for `flow-rs hook <name>` subprocess dispatch.
//!
//! Covers the full dispatch chain for the three Claude Code hook handlers —
//! clap argument parsing → stdin reading → branch resolution → state file
//! mutation → stdout contract → exit code — by spawning `flow-rs hook <name>`
//! as a child process with crafted stdin. Closes the coverage gap identified
//! by issue #864, where `src/hooks/post_compact.rs`, `src/hooks/stop_failure.rs`,
//! and `src/hooks/stop_continue.rs` were tested only via in-process unit tests
//! that bypassed the clap wiring, stdin reading, and branch resolution layers.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::{json, Value};

/// Build a `Command` targeting the compiled `flow-rs` test binary.
///
/// `CARGO_BIN_EXE_flow-rs` is set by Cargo's integration test harness to the
/// absolute path of the just-built binary, so this is hermetic — it never
/// depends on `$PATH` or an installed `bin/flow` dispatcher.
fn flow_rs() -> Command {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
}

/// Initialize a bare git repo at `dir` and write `state` to
/// `<dir>/.flow-states/<branch>.json`.
///
/// The `git init` call is required so that `project_root()` in the child
/// subprocess (which calls `git worktree list --porcelain`) resolves to
/// the temp dir rather than falling back to `PathBuf::from(".")` — which
/// would then resolve against the child's `current_dir`, still the temp
/// dir, but only by coincidence. An explicit `git init` makes the
/// resolution deterministic and mirrors `tests/clear_blocked.rs`.
fn setup_git_and_state(dir: &Path, branch: &str, state: &Value) {
    let _ = Command::new("git").args(["init"]).current_dir(dir).output();
    let state_dir = dir.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join(format!("{}.json", branch)),
        serde_json::to_string_pretty(state).unwrap(),
    )
    .unwrap();
}

/// Spawn `flow-rs hook <hook>` with simulated branch resolution, pipe
/// `stdin_data` to the child, and return the captured `Output`.
///
/// - `FLOW_SIMULATE_BRANCH` is set on the child `Command` only (not the
///   test process) so parallel Cargo tests cannot race on it — this
///   satisfies `.claude/rules/testing-gotchas.md` Rust Parallel Test Env
///   Var Races. Both `current_branch()` (used by stop_continue) and
///   `resolve_branch()` (used by stop_failure and post_compact) honor the
///   env var, so one helper serves all three hooks.
/// - `current_dir(dir)` scopes `project_root()` discovery to the tempdir
///   so the child reads and mutates only the fixture's `.flow-states/`
///   directory — satisfies Subprocess CWD Parity in rust-port-parity.md.
/// - `wait_with_output()` captures stdout/stderr via stdlib reader threads;
///   `cargo test` does not capture inherited child fds, so omitting
///   capture here would leak git init noise — satisfies Test-Module
///   Subprocess Stdio in rust-port-parity.md.
fn run_hook(hook: &str, dir: &Path, branch: &str, stdin_data: &[u8]) -> Output {
    let mut cmd = flow_rs();
    cmd.arg("hook")
        .arg(hook)
        .env("FLOW_SIMULATE_BRANCH", branch)
        .current_dir(dir)
        .stdin(Stdio::piped());

    let mut child = cmd.spawn().unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(stdin_data).unwrap();
    }
    child.wait_with_output().unwrap()
}

// ---------------------------------------------------------------------------
// post-compact hook
// ---------------------------------------------------------------------------

#[test]
fn test_post_compact_happy_path_writes_state() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "current_phase": "flow-code"
    });
    setup_git_and_state(dir.path(), branch, &state);

    let stdin = br#"{"compact_summary":"Working on tests.","cwd":"/Users/ben/code/myapp","trigger":"manual"}"#;
    let output = run_hook("post-compact", dir.path(), branch, stdin);

    assert_eq!(output.status.code().unwrap(), 0);

    let on_disk: Value = serde_json::from_str(
        &fs::read_to_string(dir.path().join(".flow-states/test-feature.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(on_disk["compact_summary"], "Working on tests.");
    assert_eq!(on_disk["compact_cwd"], "/Users/ben/code/myapp");
    assert_eq!(on_disk["compact_count"], 1);
}

#[test]
fn test_post_compact_malformed_stdin_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({"branch": branch, "current_phase": "flow-code"});
    setup_git_and_state(dir.path(), branch, &state);

    let output = run_hook("post-compact", dir.path(), branch, b"not json at all");

    assert_eq!(output.status.code().unwrap(), 0);
    assert!(output.stdout.is_empty());

    // State must be unchanged — `run()` returns early on malformed JSON.
    let on_disk: Value = serde_json::from_str(
        &fs::read_to_string(dir.path().join(".flow-states/test-feature.json")).unwrap(),
    )
    .unwrap();
    assert!(on_disk.get("compact_summary").is_none());
    assert!(on_disk.get("compact_count").is_none());
}

#[test]
fn test_post_compact_no_state_file_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let _ = Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output();

    let stdin = br#"{"compact_summary":"Summary."}"#;
    let output = run_hook("post-compact", dir.path(), "test-feature", stdin);

    assert_eq!(output.status.code().unwrap(), 0);
    assert!(output.stdout.is_empty());
}

#[test]
fn test_post_compact_empty_stdin_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({"branch": branch, "current_phase": "flow-code"});
    setup_git_and_state(dir.path(), branch, &state);

    let output = run_hook("post-compact", dir.path(), branch, b"");

    assert_eq!(output.status.code().unwrap(), 0);
    assert!(output.stdout.is_empty());

    // Empty stdin → serde_json::from_str fails → run() returns before
    // touching the state file.
    let on_disk: Value = serde_json::from_str(
        &fs::read_to_string(dir.path().join(".flow-states/test-feature.json")).unwrap(),
    )
    .unwrap();
    assert!(on_disk.get("compact_summary").is_none());
    assert!(on_disk.get("compact_count").is_none());
}
