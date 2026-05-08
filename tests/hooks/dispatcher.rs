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

use crate::common::flow_states_dir;

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
/// resolution deterministic (same pattern as `tests/clear_blocked.rs`).
fn setup_git_and_state(dir: &Path, branch: &str, state: &Value) {
    let _ = Command::new("git").args(["init"]).current_dir(dir).output();
    let branch_dir = flow_states_dir(dir).join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(
        branch_dir.join("state.json"),
        serde_json::to_string_pretty(state).unwrap(),
    )
    .unwrap();
}

/// Read `<dir>/.flow-states/<branch>.json` and parse it as a `Value`.
///
/// Every test that writes fixture state with `setup_git_and_state` and
/// then asserts on the mutated state after running a hook needs this
/// exact four-line read-and-parse dance. Extracting it keeps the test
/// bodies focused on the assertions that matter and eliminates the
/// risk that a branch-name typo in the path diverges from the
/// `setup_git_and_state` call.
fn read_state(dir: &Path, branch: &str) -> Value {
    let path = flow_states_dir(dir).join(branch).join("state.json");
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

/// Spawn `flow-rs hook <hook>` with simulated branch resolution, pipe
/// `stdin_data` to the child, and return the captured `Output`.
///
/// - `FLOW_SIMULATE_BRANCH` is set on the child `Command` only (not the
///   test process) so parallel Cargo tests cannot race on it — this
///   satisfies `.claude/rules/testing-gotchas.md` Rust Parallel Test Env
///   Var Races. All three hooks use `resolve_branch()` (which delegates
///   to `current_branch()` internally), and both functions honor the
///   env var, so one helper serves all three hooks.
/// - `current_dir(dir)` scopes `project_root()` discovery to the tempdir
///   so the child reads and mutates only the fixture's `.flow-states/`
///   directory — prevents host environment leaks (see testing-gotchas.md).
/// - All three stdio streams must be piped explicitly. `Command::spawn`
///   defaults to inheriting stdout/stderr, which means `wait_with_output`
///   would return empty buffers while the child's output leaks directly
///   to the test harness terminal.
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

/// Initialize a git repo in `dir` with an initial commit on `branch_name`.
///
/// Creates a deterministic HEAD so `git branch --show-current` returns
/// `branch_name` inside the child process. Mirrors `init_git_repo` from
/// `src/git.rs` tests but is self-contained in this integration test module.
/// Uses `Command::output()` so child stdout/stderr are captured, not inherited.
fn setup_git_repo_on_branch(dir: &Path, branch_name: &str) {
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git command failed");
        assert!(output.status.success(), "git {:?} failed", args);
    };
    run(&["init", "--initial-branch", branch_name]);
    run(&["config", "user.email", "test@test.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "commit.gpgsign", "false"]);
    run(&["commit", "--allow-empty", "-m", "init"]);
}

/// Spawn `flow-rs hook <hook>` WITHOUT `FLOW_SIMULATE_BRANCH`, pipe
/// `stdin_data` to the child, and return the captured `Output`.
///
/// Unlike [`run_hook`], this helper does NOT set `FLOW_SIMULATE_BRANCH`
/// on the child process, and explicitly removes it from the inherited
/// environment via `env_remove`. This forces the hook to resolve the
/// branch via real git (`git branch --show-current`) and the
/// `resolve_branch` state-file-scan fallback — exercising the exact
/// production code path that `FLOW_SIMULATE_BRANCH` short-circuits.
///
/// Callers must use [`setup_git_repo_on_branch`] (not
/// [`setup_git_and_state`]) so the fixture repo has a deterministic
/// HEAD branch and an initial commit.
fn run_hook_no_simulate(hook: &str, dir: &Path, stdin_data: &[u8]) -> Output {
    let mut cmd = flow_rs();
    cmd.arg("hook")
        .arg(hook)
        .env_remove("FLOW_SIMULATE_BRANCH")
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

    let on_disk = read_state(dir.path(), branch);
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
    let on_disk = read_state(dir.path(), branch);
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
fn test_post_compact_no_branch_exits_zero() {
    // No FLOW_SIMULATE_BRANCH, no git → resolve_branch returns None →
    // `None => return` arm in run().
    let dir = tempfile::tempdir().unwrap();
    let stdin = br#"{"compact_summary":"test"}"#;
    let output = run_hook_no_simulate("post-compact", dir.path(), stdin);
    assert_eq!(output.status.code().unwrap(), 0);
    assert!(output.stdout.is_empty());
}

#[test]
fn test_post_compact_slash_branch_exits_zero() {
    // Slash-branch → FlowPaths::try_new returns None → second None arm.
    let dir = tempfile::tempdir().unwrap();
    let stdin = br#"{"compact_summary":"test"}"#;
    let output = run_hook("post-compact", dir.path(), "feature/slash/nope", stdin);
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
    let on_disk = read_state(dir.path(), branch);
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

    let on_disk = read_state(dir.path(), branch);
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
    let on_disk = read_state(dir.path(), branch);
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
fn test_stop_failure_no_branch_exits_zero() {
    // Non-git dir with no FLOW_SIMULATE_BRANCH → resolve_branch returns
    // None → `None => return` arm in run().
    let dir = tempfile::tempdir().unwrap();
    let stdin = br#"{"error_type":"rate_limit","error_message":"429"}"#;
    let output = run_hook_no_simulate("stop-failure", dir.path(), stdin);
    assert_eq!(output.status.code().unwrap(), 0);
    assert!(output.stdout.is_empty());
}

#[test]
fn test_stop_failure_slash_branch_exits_zero() {
    // A slash-containing branch is not a valid FLOW branch —
    // `FlowPaths::try_new` returns None and `run()` returns early.
    let dir = tempfile::tempdir().unwrap();
    let stdin = br#"{"error_type":"rate_limit","error_message":"429"}"#;
    let output = run_hook("stop-failure", dir.path(), "feature/with/slash", stdin);
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

    let on_disk = read_state(dir.path(), branch);
    assert!(on_disk.get("_last_failure").is_none());
}

// ---------------------------------------------------------------------------
// stop-continue hook
// ---------------------------------------------------------------------------

#[test]
fn test_stop_continue_pending_set_outputs_block_json() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    // Pre-set `_blocked` so the test can verify the blocking path clears it.
    // `run()` calls `clear_blocked(&state_path)` in the `should_block=true`
    // branch; without a pre-existing `_blocked` value the clear would be a
    // no-op and the path would go untested at the subprocess level.
    let state = json!({
        "branch": branch,
        "_continue_pending": "simplify",
        "_blocked": "2026-01-01T10:00:00-08:00"
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

    // `_blocked` must be cleared when the hook blocks — proves the
    // `clear_blocked(&state_path)` call in the blocking branch of `run()`.
    let on_disk = read_state(dir.path(), branch);
    assert!(
        on_disk.get("_blocked").is_none(),
        "_blocked must be removed when blocking for continuation"
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
    assert!(
        reason.contains("Next steps:"),
        "reason must include 'Next steps:' header"
    );
    assert!(
        reason.contains("learn_step=5"),
        "reason must embed the context body"
    );
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
    assert!(
        !reason.contains("Next steps:"),
        "no context → no 'Next steps:' header"
    );
}

#[test]
fn test_stop_continue_empty_pending_no_output() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "_continue_pending": "",
        // Bypass discussion-mode block — this test exercises the empty-pending idle path.
        "_stop_instructed": true
    });
    setup_git_and_state(dir.path(), branch, &state);

    let output = run_hook("stop-continue", dir.path(), branch, b"{}");

    assert_eq!(output.status.code().unwrap(), 0);
    assert!(output.stdout.is_empty());

    // Empty-string pending is distinct from missing pending: both should
    // reach `set_blocked_idle` and write `_blocked`, but they exercise
    // different branches of the `pending.is_empty()` check in
    // `check_continue`. This assertion verifies the empty-string branch
    // does not corrupt the state or skip the idle side effect.
    let on_disk = read_state(dir.path(), branch);
    let blocked = on_disk["_blocked"].as_str();
    assert!(
        blocked.map(|s| !s.is_empty()).unwrap_or(false),
        "_blocked must be set when pending is the empty string"
    );
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
fn test_stop_continue_stale_session_clears_and_captures_new() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "session_id": "old-session",
        "_continue_pending": "simplify",
        "_continue_context": "stale",
        // Bypass discussion-mode block — this test exercises the session-mismatch path.
        "_stop_instructed": true
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
    assert!(
        output.stdout.is_empty(),
        "session mismatch must not emit block output"
    );

    let on_disk = read_state(dir.path(), branch);
    assert_eq!(on_disk["_continue_pending"], "", "pending must be cleared");
    assert_eq!(on_disk["_continue_context"], "", "context must be cleared");
    assert_eq!(
        on_disk["session_id"], "new-session",
        "capture_session_id must record the new session (proves check→capture ordering)"
    );
    assert_eq!(on_disk["transcript_path"], "/p.jsonl");

    // Stale-session path reaches `set_blocked_idle` because `should_block`
    // is false after the session-mismatch clear. Assert `_blocked` is set
    // to a non-empty timestamp so this distinct path through the idle
    // branch is verified separately from `test_stop_continue_sets_blocked_when_idle`.
    let blocked = on_disk["_blocked"].as_str();
    assert!(
        blocked.map(|s| !s.is_empty()).unwrap_or(false),
        "_blocked must be set on the stale-session idle path"
    );
}

#[test]
fn test_stop_continue_sets_blocked_when_idle() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "current_phase": "flow-code",
        // Bypass discussion-mode block — this test exercises the idle/blocked path.
        "_stop_instructed": true
    });
    setup_git_and_state(dir.path(), branch, &state);

    // No `_continue_pending` → hook does not block → `set_blocked_idle` runs
    // in the not-blocking branch of `run()`, writing `_blocked` as the current
    // timestamp. Proves the idle side of the clear/set blocked branch.
    let stdin = br#"{"session_id":"test-session"}"#;
    let output = run_hook("stop-continue", dir.path(), branch, stdin);

    assert_eq!(output.status.code().unwrap(), 0);
    assert!(output.stdout.is_empty());

    let on_disk = read_state(dir.path(), branch);
    let blocked = on_disk["_blocked"].as_str();
    assert!(
        blocked.map(|s| !s.is_empty()).unwrap_or(false),
        "_blocked must be a non-empty timestamp string after idle run"
    );
}

#[test]
fn test_stop_continue_discussion_mode_blocks_first_interrupt() {
    // Integration test: state file exists with no _stop_instructed —
    // discussion mode blocks, outputs block JSON with flow-note instruction,
    // and sets _stop_instructed in the state file.
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "current_phase": "flow-code"
    });
    setup_git_and_state(dir.path(), branch, &state);

    let output = run_hook("stop-continue", dir.path(), branch, b"{}");

    assert_eq!(output.status.code().unwrap(), 0);
    assert!(
        !output.stdout.is_empty(),
        "discussion mode must block the first interrupt"
    );
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");
    assert_eq!(parsed["decision"], "block");
    let reason = parsed["reason"].as_str().unwrap();
    assert!(
        reason.contains("flow:flow-note"),
        "block reason must instruct model to invoke flow:flow-note"
    );
    assert!(
        !reason.contains("child skill"),
        "discussion mode must not use 'child skill returned' framing"
    );

    let on_disk = read_state(dir.path(), branch);
    assert_eq!(
        on_disk["_stop_instructed"], true,
        "_stop_instructed must be set after first interrupt"
    );
}

#[test]
fn test_stop_continue_session_mismatch_preserves_stop_instructed() {
    // Session mismatch does NOT clear _stop_instructed — clearing it would
    // cause check_discussion_mode to re-fire in the same hook invocation
    // (a non-user-initiated Stop). phase_enter() handles the reset when
    // the new session enters its first phase.
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "session_id": "old-session",
        "_continue_pending": "commit",
        "_continue_context": "stale",
        "_stop_instructed": true
    });
    setup_git_and_state(dir.path(), branch, &state);

    let stdin = br#"{"session_id":"new-session"}"#;
    let output = run_hook("stop-continue", dir.path(), branch, stdin);

    assert_eq!(output.status.code().unwrap(), 0);
    assert!(
        output.stdout.is_empty(),
        "session mismatch must not emit block output"
    );

    let on_disk = read_state(dir.path(), branch);
    assert_eq!(
        on_disk["_stop_instructed"], true,
        "_stop_instructed must persist across session mismatch"
    );
}

#[test]
fn test_stop_continue_no_block_after_cleared_continue_pending() {
    // Integration test for the finalize-commit → stop-continue contract:
    // when finalize-commit clears _continue_pending and _continue_context
    // on error (setting both to ""), the stop-continue hook must NOT block.
    // This verifies the E2E path from issue #943.
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    // State represents what finalize-commit writes on error:
    // both flags set to empty string (not absent — mutate_state writes "").
    let state = json!({
        "branch": branch,
        "current_phase": "flow-code",
        "_continue_pending": "",
        "_continue_context": "",
        // Bypass discussion-mode block — this test exercises the cleared-flags path.
        "_stop_instructed": true
    });
    setup_git_and_state(dir.path(), branch, &state);

    let output = run_hook("stop-continue", dir.path(), branch, b"{}");

    assert_eq!(output.status.code().unwrap(), 0);
    assert!(
        output.stdout.is_empty(),
        "hook must not block when _continue_pending was cleared by finalize-commit"
    );

    // _blocked should be set — the hook took the idle path
    let on_disk = read_state(dir.path(), branch);
    let blocked = on_disk["_blocked"].as_str();
    assert!(
        blocked.map(|s| !s.is_empty()).unwrap_or(false),
        "_blocked must be set on the idle path after cleared flags"
    );
}

#[test]
fn test_stop_continue_pending_with_first_stop_uses_conditional_message() {
    // When _continue_pending is set and _stop_instructed is NOT set (first stop),
    // check_first_stop fires and produces a conditional message that tells the
    // model to check for user messages before auto-continuing. Guards against
    // _continue_pending trampling user conversations (issue #950).
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "_continue_pending": "commit",
        "_continue_context": "Do the thing"
    });
    setup_git_and_state(dir.path(), branch, &state);

    let output = run_hook("stop-continue", dir.path(), branch, b"{}");

    assert_eq!(output.status.code().unwrap(), 0);
    let stdout = std::str::from_utf8(&output.stdout).unwrap().trim();
    assert!(!stdout.is_empty(), "first stop with pending must block");
    let parsed: Value = serde_json::from_str(stdout).unwrap();
    assert_eq!(parsed["decision"], "block");
    let reason = parsed["reason"].as_str().unwrap();

    // Must use the conditional message, NOT the old "Continue parent phase" framing
    assert!(
        reason.contains("Check the conversation context"),
        "reason must contain conditional user-check instructions, got: {}",
        reason
    );
    assert!(
        reason.contains("flow:flow-note"),
        "reason must mention flow:flow-note for capturing corrections"
    );
    assert!(
        !reason.contains("Continue parent phase"),
        "reason must NOT use the old unconditional framing"
    );
    assert!(
        reason.contains("Do the thing"),
        "reason must include the continuation context"
    );

    // _stop_instructed must be set and preserved (not removed like check_continue does)
    let on_disk = read_state(dir.path(), branch);
    assert_eq!(
        on_disk["_stop_instructed"], true,
        "_stop_instructed must be set after first stop with pending"
    );
    // Pending must be consumed
    assert_eq!(on_disk["_continue_pending"], "");
    assert_eq!(on_disk["_continue_context"], "");
}

#[test]
fn test_stop_continue_no_state_no_simulate_exits_cleanly() {
    // Complementary test: git repo on main, no state files at all.
    // resolve_branch returns (Some("main"), []), state_path for main
    // does not exist, hook exits cleanly with no output.
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo_on_branch(dir.path(), "main");

    let output = run_hook_no_simulate("stop-continue", dir.path(), b"{}");

    assert_eq!(output.status.code().unwrap(), 0);
    assert!(output.stdout.is_empty(), "no state files → no block output");
}

// --- validate-ask-user integration tests (issue #1145 Task 3) ---
//
// These drive `src/hooks/validate_ask_user::run()` through `flow-rs hook
// validate-ask-user` with a prepared state file and crafted stdin. They
// cover every exit path: malformed stdin, missing branch/state, the
// slash-branch FlowPaths::try_new None arm required by
// `.claude/rules/external-input-validation.md`, the in-progress+auto
// block path with exit 2, the _auto_continue auto-answer JSON path, and
// the plain-allow path that writes `_blocked` and exits 0.

#[test]
fn validate_ask_user_malformed_stdin_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_and_state(dir.path(), "feat", &json!({"current_phase": "flow-code"}));
    let output = run_hook("validate-ask-user", dir.path(), "feat", b"not json at all");
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn validate_ask_user_empty_stdin_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_and_state(dir.path(), "feat", &json!({"current_phase": "flow-code"}));
    let output = run_hook("validate-ask-user", dir.path(), "feat", b"");
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn validate_ask_user_no_state_file_exits_zero() {
    // FLOW_SIMULATE_BRANCH resolves the branch, but no state file exists
    // at `.flow-states/feat.json`. `validate(Some(&state_path))` returns
    // `(true, _, None)` because the `path.exists()` check fails.
    let dir = tempfile::tempdir().unwrap();
    let _ = std::process::Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output();
    let output = run_hook("validate-ask-user", dir.path(), "feat", b"{}");
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn validate_ask_user_slash_branch_exits_zero_no_panic() {
    // Regression guard per .claude/rules/external-input-validation.md
    // Hook callsite discipline: slash-branches are external input from
    // git, not valid FLOW branch names. FlowPaths::try_new returns
    // None, and run() treats that as "no active flow on this branch"
    // and exits 0 — the hook neither panics nor constructs a state
    // path against the invalid branch.
    let dir = tempfile::tempdir().unwrap();
    let _ = std::process::Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output();
    let output = run_hook("validate-ask-user", dir.path(), "feature/foo", b"{}");
    assert_eq!(output.status.code().unwrap(), 0);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked"),
        "slash-branch must not panic the hook; stderr: {}",
        stderr
    );
}

#[test]
fn validate_ask_user_blocks_in_progress_auto_exits_2_with_phase_name() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "skills": {"flow-code": {"continue": "auto"}},
        "phases": {"flow-code": {"status": "in_progress"}},
    });
    setup_git_and_state(dir.path(), "feat", &state);
    let output = run_hook("validate-ask-user", dir.path(), "feat", b"{}");
    assert_eq!(output.status.code().unwrap(), 2);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("flow-code"),
        "stderr must name the phase: {}",
        stderr
    );
    assert!(
        stderr.contains("BLOCKED"),
        "stderr must say BLOCKED: {}",
        stderr
    );
}

#[test]
fn validate_ask_user_auto_continue_writes_stdout_json_exits_zero() {
    // _auto_continue without in_progress+auto → the hook prints a JSON
    // permissionDecision=allow with updatedInput naming the successor
    // command so Claude Code auto-answers the AskUserQuestion.
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "skills": {"flow-code": {"continue": "manual"}},
        "phases": {"flow-code": {"status": "in_progress"}},
        "_auto_continue": "/flow:flow-code-review",
    });
    setup_git_and_state(dir.path(), "feat", &state);
    let output = run_hook("validate-ask-user", dir.path(), "feat", b"{}");
    assert_eq!(output.status.code().unwrap(), 0);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("permissionDecision") && stdout.contains("/flow:flow-code-review"),
        "auto-answer stdout must carry permissionDecision + successor: {}",
        stdout
    );
}

#[test]
fn validate_ask_user_plain_allow_writes_blocked_timestamp_exits_zero() {
    // No block conditions, no _auto_continue → plain allow path that
    // writes _blocked timestamp to the state file before exit 0.
    let dir = tempfile::tempdir().unwrap();
    let state = json!({"current_phase": "flow-code"});
    setup_git_and_state(dir.path(), "feat", &state);
    let output = run_hook("validate-ask-user", dir.path(), "feat", b"{}");
    assert_eq!(output.status.code().unwrap(), 0);
    let post = read_state(dir.path(), "feat");
    let blocked = post.get("_blocked").and_then(|v| v.as_str()).unwrap_or("");
    assert!(
        !blocked.is_empty(),
        "plain-allow path must write _blocked timestamp; state: {}",
        post
    );
}

// --- validate-claude-paths integration tests (issue #1145 Task 4) ---
//
// validate_claude_paths::run() uses `detect_branch_from_cwd()` and
// `find_project_root()` rather than FLOW_SIMULATE_BRANCH — both walk
// the process cwd. Fixtures therefore create a tempdir with a
// `.flow-states/<branch>.json` at the root and a `.worktrees/<branch>/`
// subdir, then spawn the child subprocess with
// `current_dir(<tempdir>/.worktrees/<branch>)` so both helpers succeed
// and `is_flow_active` sees the state file.

/// Prepare a `<tempdir>/.worktrees/<branch>/` subtree with
/// `.flow-states/<branch>.json` at the project root when
/// `with_state_file` is true. Returns the worktree cwd path.
/// Create a `<dir>/.worktrees/<branch>/` subtree and return the
/// worktree cwd path.
///
/// Writes a stub `.git` marker file inside the worktree dir so the
/// hook's `detect_branch_from_cwd` finds the branch via the
/// `.worktrees/<branch>/<has .git>` pattern the production code walks.
/// Without the marker, detection falls back to `git branch --show-current`,
/// which returns None in a tempdir fixture, and the hook
/// allow-short-circuits before exercising the validate path the test
/// is trying to reach.
///
/// When `with_state_file` is true, also creates
/// `<dir>/.flow-states/<branch>.json` with a minimal state body so
/// `is_flow_active` returns true — required by tests exercising
/// the block path, optional for tests exercising the no-active-flow
/// allow path.
fn setup_worktree_fixture(dir: &Path, branch: &str, with_state_file: bool) -> std::path::PathBuf {
    let worktree = dir.join(".worktrees").join(branch);
    fs::create_dir_all(&worktree).unwrap();
    fs::write(worktree.join(".git"), "gitdir: fake\n").unwrap();
    if with_state_file {
        let branch_dir = flow_states_dir(dir).join(branch);
        fs::create_dir_all(&branch_dir).unwrap();
        fs::write(
            branch_dir.join("state.json"),
            serde_json::to_string(&json!({"current_phase": "flow-code"})).unwrap(),
        )
        .unwrap();
    }
    worktree
}

fn run_hook_in(cwd: &Path, hook: &str, stdin_data: &[u8]) -> Output {
    let mut cmd = flow_rs();
    cmd.arg("hook")
        .arg(hook)
        .env_remove("FLOW_SIMULATE_BRANCH")
        .env_remove("FLOW_CI_RUNNING")
        .current_dir(cwd)
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

#[test]
fn validate_claude_paths_malformed_stdin_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = setup_worktree_fixture(dir.path(), "feat", true);
    let output = run_hook_in(&cwd, "validate-claude-paths", b"not json");
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn validate_claude_paths_empty_file_path_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = setup_worktree_fixture(dir.path(), "feat", true);
    // `tool_input.file_path` missing entirely → empty string → exit 0.
    let input = serde_json::to_vec(&json!({"tool_input": {}})).unwrap();
    let output = run_hook_in(&cwd, "validate-claude-paths", &input);
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn validate_claude_paths_no_flow_states_dir_allows_any_path() {
    // cwd has no `.flow-states/` anywhere above it → find_project_root
    // returns None → flow_active=false → allow even a protected path.
    let dir = tempfile::tempdir().unwrap();
    let input = serde_json::to_vec(&json!({
        "tool_input": {
            "file_path": dir.path().join(".claude/rules/foo.md").to_string_lossy(),
        }
    }))
    .unwrap();
    let output = run_hook_in(dir.path(), "validate-claude-paths", &input);
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn validate_claude_paths_worktree_blocks_claude_rules() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = setup_worktree_fixture(dir.path(), "feat", true);
    let target = cwd.join(".claude/rules/foo.md");
    let input = serde_json::to_vec(&json!({
        "tool_input": {"file_path": target.to_string_lossy()}
    }))
    .unwrap();
    let output = run_hook_in(&cwd, "validate-claude-paths", &input);
    assert_eq!(output.status.code().unwrap(), 2);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("BLOCKED"), "stderr: {}", stderr);
    assert!(
        stderr.contains("write-rule"),
        "stderr must name the redirect command: {}",
        stderr
    );
}

#[test]
fn validate_claude_paths_worktree_blocks_claude_skills() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = setup_worktree_fixture(dir.path(), "feat", true);
    let target = cwd.join(".claude/skills/foo/SKILL.md");
    let input = serde_json::to_vec(&json!({
        "tool_input": {"file_path": target.to_string_lossy()}
    }))
    .unwrap();
    let output = run_hook_in(&cwd, "validate-claude-paths", &input);
    assert_eq!(output.status.code().unwrap(), 2);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("BLOCKED"), "stderr: {}", stderr);
}

#[test]
fn validate_claude_paths_worktree_blocks_claude_md() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = setup_worktree_fixture(dir.path(), "feat", true);
    let target = cwd.join("CLAUDE.md");
    let input = serde_json::to_vec(&json!({
        "tool_input": {"file_path": target.to_string_lossy()}
    }))
    .unwrap();
    let output = run_hook_in(&cwd, "validate-claude-paths", &input);
    assert_eq!(output.status.code().unwrap(), 2);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("BLOCKED"), "stderr: {}", stderr);
}

#[test]
fn validate_claude_paths_worktree_allows_settings_json() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = setup_worktree_fixture(dir.path(), "feat", true);
    let target = cwd.join(".claude/settings.json");
    let input = serde_json::to_vec(&json!({
        "tool_input": {"file_path": target.to_string_lossy()}
    }))
    .unwrap();
    let output = run_hook_in(&cwd, "validate-claude-paths", &input);
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn validate_claude_paths_worktree_allows_src_file() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = setup_worktree_fixture(dir.path(), "feat", true);
    let target = cwd.join("src/lib.rs");
    let input = serde_json::to_vec(&json!({
        "tool_input": {"file_path": target.to_string_lossy()}
    }))
    .unwrap();
    let output = run_hook_in(&cwd, "validate-claude-paths", &input);
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn validate_claude_paths_flow_state_file_missing_allows() {
    // find_project_root finds `.flow-states/` at the project root, but
    // no state file exists for this branch → is_flow_active returns
    // false → allow even a protected path.
    let dir = tempfile::tempdir().unwrap();
    // Create `.flow-states/` so find_project_root succeeds, but skip
    // the per-branch state file.
    let states = flow_states_dir(dir.path());
    fs::create_dir_all(&states).unwrap();
    let cwd = dir.path().join(".worktrees").join("feat");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(cwd.join(".git"), "gitdir: fake\n").unwrap();
    let target = cwd.join(".claude/rules/foo.md");
    let input = serde_json::to_vec(&json!({
        "tool_input": {"file_path": target.to_string_lossy()}
    }))
    .unwrap();
    let output = run_hook_in(&cwd, "validate-claude-paths", &input);
    assert_eq!(output.status.code().unwrap(), 0);
}

// --- validate-pretool integration tests (issue #1145 Task 5) ---
//
// validate_pretool::run() uses `find_settings_and_root()`,
// `detect_branch_from_cwd()`, and `is_flow_active()` together to
// compute `flow_active`. The background-block path fires for
// `bin/flow` and `bin/ci` commands regardless of flow_active, but
// the agent and bash block paths require flow_active=true, so
// tests that exercise those paths build a settings file at the
// project root plus the full worktree fixture.

/// Extend `setup_worktree_fixture` with a `.claude/settings.json` at
/// the project root.
///
/// `validate_pretool::run` reads `settings.json` to compile its
/// permission allow list; without the file, `find_settings_and_root`
/// returns None and `flow_active` is forced to false, which skips the
/// agent/bash validate paths tests want to exercise. The
/// `allow_patterns` slice is the `permissions.allow` list written to
/// the settings file — any Bash command pattern a test exercises on
/// the allow (not block) path must appear here, in the standard
/// `Bash(<glob>)` form.
fn setup_pretool_fixture(dir: &Path, branch: &str, allow_patterns: &[&str]) -> std::path::PathBuf {
    let cwd = setup_worktree_fixture(dir, branch, true);
    let claude = dir.join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let allow: Vec<Value> = allow_patterns.iter().map(|p| json!(p)).collect();
    let settings = json!({"permissions": {"allow": allow, "deny": []}});
    fs::write(
        claude.join("settings.json"),
        serde_json::to_string(&settings).unwrap(),
    )
    .unwrap();
    cwd
}

#[test]
fn validate_pretool_malformed_stdin_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = setup_pretool_fixture(dir.path(), "feat", &[]);
    let output = run_hook_in(&cwd, "validate-pretool", b"not json");
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn validate_pretool_background_bin_flow_ci_blocked_exits_2() {
    // Suffix-match coverage per .claude/rules/testing-gotchas.md
    // "Suffix-Match Path Coverage" — bare form (`bin/flow`) variant
    // paired with validate_pretool_background_absolute_bin_flow_blocked_exits_2.
    let dir = tempfile::tempdir().unwrap();
    let cwd = setup_pretool_fixture(dir.path(), "feat", &[]);
    let input = serde_json::to_vec(&json!({
        "tool_input": {"command": "bin/flow ci", "run_in_background": true}
    }))
    .unwrap();
    let output = run_hook_in(&cwd, "validate-pretool", &input);
    assert_eq!(output.status.code().unwrap(), 2);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("bin/flow"),
        "stderr must name bin/flow variant: {}",
        stderr
    );
}

#[test]
fn validate_pretool_background_bin_ci_blocked_exits_2() {
    // Suffix-match coverage per .claude/rules/testing-gotchas.md
    // "Suffix-Match Path Coverage" — bare form (`bin/ci`) variant
    // paired with validate_pretool_background_absolute_bin_ci_blocked_exits_2.
    let dir = tempfile::tempdir().unwrap();
    let cwd = setup_pretool_fixture(dir.path(), "feat", &[]);
    let input = serde_json::to_vec(&json!({
        "tool_input": {"command": "bin/ci", "run_in_background": true}
    }))
    .unwrap();
    let output = run_hook_in(&cwd, "validate-pretool", &input);
    assert_eq!(output.status.code().unwrap(), 2);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("bin/ci"),
        "stderr must name bin/ci variant: {}",
        stderr
    );
}

#[test]
fn validate_pretool_background_absolute_bin_flow_blocked_exits_2() {
    // Suffix-match coverage per .claude/rules/testing-gotchas.md
    // "Suffix-Match Path Coverage" — absolute-path form of bin/flow.
    let dir = tempfile::tempdir().unwrap();
    let cwd = setup_pretool_fixture(dir.path(), "feat", &[]);
    let input = serde_json::to_vec(&json!({
        "tool_input": {
            "command": "/Users/example/project/bin/flow ci",
            "run_in_background": true,
        }
    }))
    .unwrap();
    let output = run_hook_in(&cwd, "validate-pretool", &input);
    assert_eq!(output.status.code().unwrap(), 2);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("bin/flow"),
        "stderr must name bin/flow: {}",
        stderr
    );
}

#[test]
fn validate_pretool_background_absolute_bin_ci_blocked_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = setup_pretool_fixture(dir.path(), "feat", &[]);
    let input = serde_json::to_vec(&json!({
        "tool_input": {
            "command": "/opt/tools/bin/ci",
            "run_in_background": true,
        }
    }))
    .unwrap();
    let output = run_hook_in(&cwd, "validate-pretool", &input);
    assert_eq!(output.status.code().unwrap(), 2);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("bin/ci"),
        "stderr must name bin/ci: {}",
        stderr
    );
}

#[test]
fn validate_pretool_general_purpose_agent_blocked_during_flow_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = setup_pretool_fixture(dir.path(), "feat", &[]);
    // Agent tool call: no `command`, subagent_type=general-purpose.
    let input = serde_json::to_vec(&json!({
        "tool_input": {"subagent_type": "general-purpose"}
    }))
    .unwrap();
    let output = run_hook_in(&cwd, "validate-pretool", &input);
    assert_eq!(output.status.code().unwrap(), 2);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("general-purpose"),
        "stderr must name subagent type: {}",
        stderr
    );
}

#[test]
fn validate_pretool_non_general_purpose_agent_allowed_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = setup_pretool_fixture(dir.path(), "feat", &[]);
    let input = serde_json::to_vec(&json!({
        "tool_input": {"subagent_type": "flow:reviewer"}
    }))
    .unwrap();
    let output = run_hook_in(&cwd, "validate-pretool", &input);
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn validate_pretool_compound_command_blocked_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = setup_pretool_fixture(dir.path(), "feat", &["Bash(echo *)"]);
    let input = serde_json::to_vec(&json!({
        "tool_input": {"command": "echo a && echo b"}
    }))
    .unwrap();
    let output = run_hook_in(&cwd, "validate-pretool", &input);
    assert_eq!(output.status.code().unwrap(), 2);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("BLOCKED"),
        "compound command must block: {}",
        stderr
    );
}

#[test]
fn validate_pretool_safe_command_allowed_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = setup_pretool_fixture(dir.path(), "feat", &["Bash(git status)"]);
    let input = serde_json::to_vec(&json!({
        "tool_input": {"command": "git status"}
    }))
    .unwrap();
    let output = run_hook_in(&cwd, "validate-pretool", &input);
    assert_eq!(output.status.code().unwrap(), 0);
}

// --- validate-worktree-paths integration tests (issue #1145 Task 6) ---
//
// validate_worktree_paths::run() reads only stdin and the process cwd
// via `std::env::current_dir()`. No FLOW state file or git repo is
// required — the hook decides allow vs. block from cwd path structure
// alone. Tests spawn the child with `current_dir` set to the tempdir
// (or a `.worktrees/<branch>/` subtree) and feed tool_input JSON on
// stdin. The `std::env::current_dir` Err arm is unreachable from
// subprocess tests (a spawned child always has a valid cwd), so those
// lines remain uncovered by this set.

fn run_worktree_hook(cwd: &Path, stdin_data: &[u8]) -> Output {
    run_hook_in(cwd, "validate-worktree-paths", stdin_data)
}

#[test]
fn validate_worktree_paths_malformed_stdin_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let output = run_worktree_hook(dir.path(), b"not json at all");
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn validate_worktree_paths_empty_file_path_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let input = serde_json::to_vec(&json!({"tool_input": {}})).unwrap();
    let output = run_worktree_hook(dir.path(), &input);
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn validate_worktree_paths_glob_path_key_recognized() {
    // Glob/Grep tool_input uses `path`, not `file_path`. The hook's
    // `get_file_path` prefers `file_path` then falls back to `path`.
    // cwd is inside a worktree; the path points to the main repo, so
    // the hook must recognize the `path` key, route through validate,
    // and block.
    //
    // macOS note: spawned subprocesses receive a canonicalized cwd
    // (/private/var/folders/... rather than /var/folders/...). The
    // hook computes project_root from that canonical cwd, so the
    // file_path passed in tool_input must also be rooted at the
    // canonical path for the block-branch `starts_with` prefix check
    // to succeed.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let worktree = root.join(".worktrees").join("feat");
    fs::create_dir_all(&worktree).unwrap();
    let target = root.join("src/lib.rs");
    let input = serde_json::to_vec(&json!({
        "tool_input": {"path": target.to_string_lossy()}
    }))
    .unwrap();
    let output = run_worktree_hook(&worktree, &input);
    assert_eq!(output.status.code().unwrap(), 2);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("BLOCKED"),
        "glob path key should route through block: {}",
        stderr
    );
}

#[test]
fn validate_worktree_paths_outside_worktree_allows_main_edit() {
    // cwd has no `.worktrees/` marker in its path → not in a worktree →
    // validate returns allow regardless of file_path. macOS tempdirs
    // live under `/var/folders/...` which is symlinked to
    // `/private/var/folders/...`; the subprocess's current_dir resolves
    // through the symlink, so canonicalize the root before building
    // the target — otherwise the file_path prefix check silently
    // fails and the test passes via an unrelated early-return.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let target = root.join("src/lib.rs");
    let input = serde_json::to_vec(&json!({
        "tool_input": {"file_path": target.to_string_lossy()}
    }))
    .unwrap();
    let output = run_worktree_hook(&root, &input);
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn validate_worktree_paths_inside_worktree_blocks_main_edit() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let worktree = root.join(".worktrees").join("feat");
    fs::create_dir_all(&worktree).unwrap();
    let target = root.join("src/lib.rs");
    let input = serde_json::to_vec(&json!({
        "tool_input": {"file_path": target.to_string_lossy()}
    }))
    .unwrap();
    let output = run_worktree_hook(&worktree, &input);
    assert_eq!(output.status.code().unwrap(), 2);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The block message names the corrected in-worktree path.
    let corrected = worktree.join("src/lib.rs");
    assert!(
        stderr.contains(corrected.to_string_lossy().as_ref()),
        "stderr must show corrected worktree path: {}",
        stderr
    );
}

#[test]
fn validate_worktree_paths_inside_worktree_allows_worktree_edit() {
    // See validate_worktree_paths_inside_worktree_blocks_main_edit for
    // why the macOS `canonicalize()` dance is needed — without it, the
    // allow-path tests pass vacuously via the "outside project" early
    // return instead of exercising the worktree-cwd allowlist.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let worktree = root.join(".worktrees").join("feat");
    fs::create_dir_all(&worktree).unwrap();
    let target = worktree.join("src/lib.rs");
    let input = serde_json::to_vec(&json!({
        "tool_input": {"file_path": target.to_string_lossy()}
    }))
    .unwrap();
    let output = run_worktree_hook(&worktree, &input);
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn validate_worktree_paths_inside_worktree_allows_flow_states() {
    // `.flow-states/` lives at the main project root and is shared —
    // edits must pass through even from inside a worktree. Canonicalize
    // the root so the macOS `/var/folders` symlink doesn't mask the
    // `.flow-states/` allowlist branch behind the "outside project"
    // early return.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let worktree = root.join(".worktrees").join("feat");
    fs::create_dir_all(&worktree).unwrap();
    let target = flow_states_dir(&root).join("feat.json");
    let input = serde_json::to_vec(&json!({
        "tool_input": {"file_path": target.to_string_lossy()}
    }))
    .unwrap();
    let output = run_worktree_hook(&worktree, &input);
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn validate_worktree_paths_inside_worktree_allows_home_path() {
    // Paths outside the project prefix are always allowed
    // (~/.claude, plugin cache, /tmp, etc.). `/Users/example/...` is
    // never a prefix of the canonical `/private/var/folders/...`
    // tempdir cwd, so this test exercises the real "outside project"
    // allow branch whether or not the root is canonicalized. We still
    // canonicalize for symmetry with the sibling tests.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let worktree = root.join(".worktrees").join("feat");
    fs::create_dir_all(&worktree).unwrap();
    let input = serde_json::to_vec(&json!({
        "tool_input": {"file_path": "/Users/example/.claude/plans/p.md"}
    }))
    .unwrap();
    let output = run_worktree_hook(&worktree, &input);
    assert_eq!(output.status.code().unwrap(), 0);
}

// --- validate-worktree-paths shared-config integration tests (issue #1170) ---
//
// Subprocess tests for the shared-config protection layer added in
// validate_worktree_paths.rs. The hook blocks Edit/Write on shared
// config files inside a worktree (exit 2) and allows Read/Grep (exit 0).
// Tests pass `tool_name` in the hook input JSON to exercise the
// tool-name gating in `run()`.

#[test]
fn validate_worktree_paths_shared_config_edit_gitignore_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let worktree = root.join(".worktrees").join("feat");
    fs::create_dir_all(&worktree).unwrap();
    let target = worktree.join(".gitignore");
    let input = serde_json::to_vec(&json!({
        "tool_name": "Edit",
        "tool_input": {"file_path": target.to_string_lossy()}
    }))
    .unwrap();
    let output = run_worktree_hook(&worktree, &input);
    assert_eq!(output.status.code().unwrap(), 2);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("shared configuration"),
        "stderr must mention shared configuration: {}",
        stderr
    );
}

#[test]
fn validate_worktree_paths_shared_config_write_package_json_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let worktree = root.join(".worktrees").join("feat");
    fs::create_dir_all(&worktree).unwrap();
    let target = worktree.join("package.json");
    let input = serde_json::to_vec(&json!({
        "tool_name": "Write",
        "tool_input": {"file_path": target.to_string_lossy()}
    }))
    .unwrap();
    let output = run_worktree_hook(&worktree, &input);
    assert_eq!(output.status.code().unwrap(), 2);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("shared configuration"),
        "stderr must mention shared configuration: {}",
        stderr
    );
}

#[test]
fn validate_worktree_paths_shared_config_edit_github_workflow_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let worktree = root.join(".worktrees").join("feat");
    fs::create_dir_all(&worktree).unwrap();
    let target = worktree.join(".github").join("workflows").join("ci.yml");
    let input = serde_json::to_vec(&json!({
        "tool_name": "Edit",
        "tool_input": {"file_path": target.to_string_lossy()}
    }))
    .unwrap();
    let output = run_worktree_hook(&worktree, &input);
    assert_eq!(output.status.code().unwrap(), 2);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("shared configuration"),
        "stderr must mention shared configuration: {}",
        stderr
    );
}

#[test]
fn validate_worktree_paths_shared_config_read_gitignore_allowed() {
    // Read on shared config should be allowed — only Edit/Write are blocked
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let worktree = root.join(".worktrees").join("feat");
    fs::create_dir_all(&worktree).unwrap();
    let target = worktree.join(".gitignore");
    let input = serde_json::to_vec(&json!({
        "tool_name": "Read",
        "tool_input": {"file_path": target.to_string_lossy()}
    }))
    .unwrap();
    let output = run_worktree_hook(&worktree, &input);
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn validate_worktree_paths_shared_config_edit_regular_file_allowed() {
    // Regular files inside the worktree should not be blocked
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let worktree = root.join(".worktrees").join("feat");
    fs::create_dir_all(&worktree).unwrap();
    let target = worktree.join("src").join("lib.rs");
    let input = serde_json::to_vec(&json!({
        "tool_name": "Edit",
        "tool_input": {"file_path": target.to_string_lossy()}
    }))
    .unwrap();
    let output = run_worktree_hook(&worktree, &input);
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn validate_worktree_paths_shared_config_edit_outside_worktree_allowed() {
    // Edit on .gitignore when NOT in a worktree should pass through
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let target = root.join(".gitignore");
    let input = serde_json::to_vec(&json!({
        "tool_name": "Edit",
        "tool_input": {"file_path": target.to_string_lossy()}
    }))
    .unwrap();
    let output = run_worktree_hook(&root, &input);
    assert_eq!(output.status.code().unwrap(), 0);
}

// --- stop-continue remaining-gap tests (issue #1145 Task 7) ---
//
// The existing `test_stop_continue_*` tests in this file cover most
// of `stop_continue::run()`. These plan-named tests close the 8-line
// coverage gap by pinning three specific paths: the slash-branch
// `FlowPaths::try_new` None arm (a regression guard per
// `.claude/rules/external-input-validation.md`), the QA-pending
// fallback when no branch state file exists, and the
// discussion-with-pending skill-name branch in `run()`'s output
// formatter (lines 607–608). Similarly-named tests above use the
// `test_` prefix; these keep the plan's naming convention (no
// prefix) so both sets coexist.

#[test]
fn stop_continue_slash_branch_exits_zero_no_panic() {
    // FLOW_SIMULATE_BRANCH=feature/foo → resolve_branch returns
    // Some("feature/foo") → FlowPaths::try_new returns None → early
    // return without panic. Regression guard per
    // .claude/rules/external-input-validation.md.
    let dir = tempfile::tempdir().unwrap();
    let _ = Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output();
    let output = run_hook("stop-continue", dir.path(), "feature/foo", b"{}");
    assert_eq!(output.status.code().unwrap(), 0);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked"),
        "slash-branch must not panic: {}",
        stderr
    );
    // No state file for a slash branch → no block output.
    assert!(
        output.stdout.is_empty(),
        "slash-branch must produce no block output: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn stop_continue_discussion_with_pending_uses_context_message() {
    // First-stop + pending path: check_first_stop sets
    // skill=discussion-with-pending. run()'s output formatter branches
    // on that name (lines 607–608) and uses result.context directly as
    // the block reason — bypassing format_block_output's "child skill
    // returned" framing.
    let dir = tempfile::tempdir().unwrap();
    let branch = "feat-ctx";
    let state = json!({
        "branch": branch,
        "_continue_pending": "commit",
        "_continue_context": "Write the commit message now."
    });
    setup_git_and_state(dir.path(), branch, &state);
    let output = run_hook("stop-continue", dir.path(), branch, b"{}");
    assert_eq!(output.status.code().unwrap(), 0);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["decision"], "block");
    let reason = parsed["reason"].as_str().unwrap_or("");
    assert!(
        reason.contains("Write the commit message now."),
        "reason must embed the pending context verbatim: {}",
        reason
    );
}
