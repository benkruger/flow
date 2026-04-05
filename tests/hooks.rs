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
/// - All three stdio streams must be piped explicitly. `Command::spawn`
///   defaults to inheriting stdout/stderr, which means `wait_with_output`
///   would return empty buffers while the child's output leaks directly
///   to the test harness terminal — the exact failure mode that the
///   Test-Module Subprocess Stdio rule in rust-port-parity.md forbids.
///   Piping stdout and stderr lets `wait_with_output` capture them for
///   assertion AND keeps cargo test output clean.
fn run_hook(hook: &str, dir: &Path, branch: &str, stdin_data: &[u8]) -> Output {
    let mut cmd = flow_rs();
    cmd.arg("hook")
        .arg(hook)
        .env("FLOW_SIMULATE_BRANCH", branch)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

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

// ---------------------------------------------------------------------------
// stop-failure hook
// ---------------------------------------------------------------------------

#[test]
fn test_stop_failure_happy_path_writes_last_failure() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "current_phase": "flow-code"
    });
    setup_git_and_state(dir.path(), branch, &state);

    let stdin = br#"{"error_type":"rate_limit","error_message":"429 Too Many Requests"}"#;
    let output = run_hook("stop-failure", dir.path(), branch, stdin);

    assert_eq!(output.status.code().unwrap(), 0);

    let on_disk: Value = serde_json::from_str(
        &fs::read_to_string(dir.path().join(".flow-states/test-feature.json")).unwrap(),
    )
    .unwrap();
    let failure = &on_disk["_last_failure"];
    assert_eq!(failure["type"], "rate_limit");
    assert_eq!(failure["message"], "429 Too Many Requests");
    assert!(
        failure["timestamp"]
            .as_str()
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "timestamp must be a non-empty string"
    );
}

#[test]
fn test_stop_failure_malformed_stdin_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({"branch": branch, "current_phase": "flow-code"});
    setup_git_and_state(dir.path(), branch, &state);

    let output = run_hook("stop-failure", dir.path(), branch, b"not json at all");

    assert_eq!(output.status.code().unwrap(), 0);
    assert!(output.stdout.is_empty());

    // State unchanged — run() returns early on JSON parse failure.
    let on_disk: Value = serde_json::from_str(
        &fs::read_to_string(dir.path().join(".flow-states/test-feature.json")).unwrap(),
    )
    .unwrap();
    assert!(on_disk.get("_last_failure").is_none());
}

#[test]
fn test_stop_failure_no_state_file_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let _ = Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output();

    let stdin = br#"{"error_type":"rate_limit","error_message":"429"}"#;
    let output = run_hook("stop-failure", dir.path(), "test-feature", stdin);

    assert_eq!(output.status.code().unwrap(), 0);
    assert!(output.stdout.is_empty());
}

#[test]
fn test_stop_failure_empty_stdin_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({"branch": branch, "current_phase": "flow-code"});
    setup_git_and_state(dir.path(), branch, &state);

    let output = run_hook("stop-failure", dir.path(), branch, b"");

    assert_eq!(output.status.code().unwrap(), 0);
    assert!(output.stdout.is_empty());

    let on_disk: Value = serde_json::from_str(
        &fs::read_to_string(dir.path().join(".flow-states/test-feature.json")).unwrap(),
    )
    .unwrap();
    assert!(on_disk.get("_last_failure").is_none());
}

// ---------------------------------------------------------------------------
// stop-continue hook
// ---------------------------------------------------------------------------

#[test]
fn test_stop_continue_pending_set_outputs_block_json() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "_continue_pending": "simplify"
    });
    setup_git_and_state(dir.path(), branch, &state);

    let output = run_hook("stop-continue", dir.path(), branch, b"{}");

    assert_eq!(output.status.code().unwrap(), 0);

    // Stdout contract: `{"decision": "block", "reason": "..."}` — this is what
    // Claude Code's continue=auto session continuation depends on. Regressing
    // this JSON shape breaks every FLOW auto-advance flow.
    let stdout = std::str::from_utf8(&output.stdout).unwrap().trim();
    assert!(!stdout.is_empty(), "stdout must contain block JSON");
    let parsed: Value = serde_json::from_str(stdout).unwrap();
    assert_eq!(parsed["decision"], "block");
    let reason = parsed["reason"].as_str().unwrap();
    assert!(
        reason.contains("simplify"),
        "reason must name the pending skill, got: {}",
        reason
    );
}

#[test]
fn test_stop_continue_context_included_in_block_reason() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "_continue_pending": "commit",
        "_continue_context": "Set learn_step=5, then self-invoke flow:flow-learn --continue-step."
    });
    setup_git_and_state(dir.path(), branch, &state);

    let output = run_hook("stop-continue", dir.path(), branch, b"{}");

    assert_eq!(output.status.code().unwrap(), 0);
    let stdout = std::str::from_utf8(&output.stdout).unwrap().trim();
    let parsed: Value = serde_json::from_str(stdout).unwrap();
    assert_eq!(parsed["decision"], "block");
    let reason = parsed["reason"].as_str().unwrap();
    assert!(reason.contains("Next steps:"), "reason must include 'Next steps:' header");
    assert!(reason.contains("learn_step=5"), "reason must embed the context body");
}

#[test]
fn test_stop_continue_no_context_uses_generic_reason() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "_continue_pending": "commit"
    });
    setup_git_and_state(dir.path(), branch, &state);

    let output = run_hook("stop-continue", dir.path(), branch, b"{}");

    assert_eq!(output.status.code().unwrap(), 0);
    let stdout = std::str::from_utf8(&output.stdout).unwrap().trim();
    let parsed: Value = serde_json::from_str(stdout).unwrap();
    assert_eq!(parsed["decision"], "block");
    let reason = parsed["reason"].as_str().unwrap();
    assert!(
        reason.contains("Resume the parent skill instructions"),
        "reason must use generic wording when context is absent, got: {}",
        reason
    );
    assert!(!reason.contains("Next steps:"), "no context → no 'Next steps:' header");
}

#[test]
fn test_stop_continue_empty_pending_no_output() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "_continue_pending": ""
    });
    setup_git_and_state(dir.path(), branch, &state);

    let output = run_hook("stop-continue", dir.path(), branch, b"{}");

    assert_eq!(output.status.code().unwrap(), 0);
    assert!(output.stdout.is_empty());
}

#[test]
fn test_stop_continue_malformed_stdin_no_output() {
    let dir = tempfile::tempdir().unwrap();
    let _ = Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output();

    // Malformed stdin → `serde_json::from_str` fails → the hook falls back to
    // an empty `{}` hook_input and continues. With no state file present,
    // `check_continue` returns no block and stdout stays empty.
    let output = run_hook(
        "stop-continue",
        dir.path(),
        "test-feature",
        b"not json at all",
    );

    assert_eq!(output.status.code().unwrap(), 0);
    assert!(output.stdout.is_empty());
}

#[test]
fn test_stop_continue_qa_pending_fallback_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let _ = Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output();

    // No branch state file — only a qa-pending breadcrumb. The hook's
    // `check_qa_pending` fallback in `run()` should fire and produce block
    // output carrying the QA context.
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join("qa-pending.json"),
        r#"{"_continue_context": "Return to FLOW repo and verify."}"#,
    )
    .unwrap();

    let output = run_hook("stop-continue", dir.path(), "test-feature", b"{}");

    assert_eq!(output.status.code().unwrap(), 0);
    let stdout = std::str::from_utf8(&output.stdout).unwrap().trim();
    let parsed: Value = serde_json::from_str(stdout).unwrap();
    assert_eq!(parsed["decision"], "block");
    let reason = parsed["reason"].as_str().unwrap();
    assert!(
        reason.contains("Return to FLOW repo"),
        "qa-pending context must be embedded in reason, got: {}",
        reason
    );
}

#[test]
fn test_stop_continue_stale_session_clears_and_captures_new() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "session_id": "old-session",
        "_continue_pending": "simplify",
        "_continue_context": "stale"
    });
    setup_git_and_state(dir.path(), branch, &state);

    // Hook stdin carries a different session_id than the state file — the
    // session isolation path in `check_continue` should clear the flag and
    // allow the stop (empty stdout). Then `capture_session_id` runs AFTER
    // `check_continue` and must write the new session_id and transcript_path.
    // This test proves the dispatch ordering in `run()`: check_continue fires
    // BEFORE capture_session_id.
    let stdin = br#"{"session_id":"new-session","transcript_path":"/p.jsonl"}"#;
    let output = run_hook("stop-continue", dir.path(), branch, stdin);

    assert_eq!(output.status.code().unwrap(), 0);
    assert!(output.stdout.is_empty(), "session mismatch must not emit block output");

    let on_disk: Value = serde_json::from_str(
        &fs::read_to_string(dir.path().join(".flow-states/test-feature.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(on_disk["_continue_pending"], "", "pending must be cleared");
    assert_eq!(on_disk["_continue_context"], "", "context must be cleared");
    assert_eq!(
        on_disk["session_id"], "new-session",
        "capture_session_id must record the new session (proves check→capture ordering)"
    );
    assert_eq!(on_disk["transcript_path"], "/p.jsonl");
}

#[test]
fn test_stop_continue_sets_blocked_when_idle() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "current_phase": "flow-code"
    });
    setup_git_and_state(dir.path(), branch, &state);

    // No `_continue_pending` → hook does not block → `set_blocked_idle` runs
    // in the not-blocking branch of `run()`, writing `_blocked` as the current
    // timestamp. Proves the idle side of the clear/set blocked branch.
    let stdin = br#"{"session_id":"test-session"}"#;
    let output = run_hook("stop-continue", dir.path(), branch, stdin);

    assert_eq!(output.status.code().unwrap(), 0);
    assert!(output.stdout.is_empty());

    let on_disk: Value = serde_json::from_str(
        &fs::read_to_string(dir.path().join(".flow-states/test-feature.json")).unwrap(),
    )
    .unwrap();
    let blocked = on_disk["_blocked"].as_str();
    assert!(
        blocked.map(|s| !s.is_empty()).unwrap_or(false),
        "_blocked must be a non-empty timestamp string after idle run"
    );
}
