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

/// Read `<dir>/.flow-states/<branch>.json` and parse it as a `Value`.
///
/// Every test that writes fixture state with `setup_git_and_state` and
/// then asserts on the mutated state after running a hook needs this
/// exact four-line read-and-parse dance. Extracting it keeps the test
/// bodies focused on the assertions that matter and eliminates the
/// risk that a branch-name typo in the path diverges from the
/// `setup_git_and_state` call.
fn read_state(dir: &Path, branch: &str) -> Value {
    let path = dir.join(format!(".flow-states/{}.json", branch));
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

/// Initialize a git repo in `dir` with an initial commit on `branch_name`.
///
/// Creates a deterministic HEAD so `git branch --show-current` returns
/// `branch_name` inside the child process. Mirrors `init_git_repo` from
/// `src/git.rs` tests but is self-contained in this integration test module.
/// Uses `Command::output()` (not `.status()`) per rust-port-parity rules.
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
/// Tombstone: .flow-states/ scan removed from resolve_branch in PR #924.
/// When on main with another branch's state file, the hook must NOT
/// resolve to that branch — it silently exits without blocking.
fn test_stop_continue_no_scan_on_main_tombstone() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo_on_branch(dir.path(), "main");

    let feature_branch = "feature-xyz";
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state = json!({
        "branch": feature_branch,
        "_continue_pending": "flow-plan",
        "_continue_context": "Resume plan phase."
    });
    fs::write(
        state_dir.join(format!("{}.json", feature_branch)),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();

    let output = run_hook_no_simulate("stop-continue", dir.path(), b"{}");

    assert_eq!(output.status.code().unwrap(), 0);
    let stdout = std::str::from_utf8(&output.stdout).unwrap().trim();
    // No output — hook exits silently because main has no state file
    assert!(
        stdout.is_empty(),
        "hook must NOT block when on main with another branch's state file — scan removed in PR #924, got: {}",
        stdout
    );

    // State file for feature-xyz must be UNTOUCHED — hook did not find or modify it.
    let on_disk = read_state(dir.path(), feature_branch);
    assert_eq!(on_disk["_continue_pending"], "flow-plan");
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
    // model to check for user messages before auto-continuing. This is the fix
    // for issue #950: _continue_pending no longer tramples user conversations.
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
