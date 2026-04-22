//! Adversarial tests for the validate_agent function in validate_pretool.rs.
//!
//! These tests target edge cases in the new Agent tool blocking logic
//! that blocks general-purpose sub-agents during active FLOW phases.

mod common;

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use common::flow_states_dir;

use serde_json::{json, Value};

/// Build a `Command` targeting the compiled `flow-rs` test binary.
fn flow_rs() -> Command {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
}

/// Initialize a git repo at `dir` on a specific `branch_name`, with
/// `.claude/settings.json` (so `find_settings_and_root` succeeds) and
/// `.flow-states/<branch_name>.json` with the given state.
///
/// The branch name in the git repo MUST match the state file name because
/// `validate_pretool.run()` uses `detect_branch_from_cwd()` (which falls
/// back to `git branch --show-current`) — not `current_branch()` which
/// honors `FLOW_SIMULATE_BRANCH`.
fn setup_flow_active_repo(dir: &Path, branch_name: &str, state: &Value) {
    // Create git repo on the specified branch
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git command failed");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(&["init", "--initial-branch", branch_name]);
    run(&["config", "user.email", "test@test.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "commit.gpgsign", "false"]);
    run(&["commit", "--allow-empty", "-m", "init"]);

    // Create .claude/settings.json so find_settings_and_root succeeds
    let claude_dir = dir.join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(
        claude_dir.join("settings.json"),
        r#"{"permissions":{"allow":[],"deny":[]}}"#,
    )
    .unwrap();

    // Create state file matching the branch name
    let state_dir = flow_states_dir(dir);
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join(format!("{}.json", branch_name)),
        serde_json::to_string_pretty(state).unwrap(),
    )
    .unwrap();
}

/// Spawn `flow-rs hook validate-pretool`, pipe `stdin_data` to the child,
/// and return the captured `Output`.
///
/// Does NOT set FLOW_SIMULATE_BRANCH because detect_branch_from_cwd()
/// does not consult it. Instead, the fixture repo must be on the correct
/// branch (via setup_flow_active_repo).
fn run_validate_pretool(dir: &Path, stdin_data: &[u8]) -> Output {
    let mut cmd = flow_rs();
    cmd.arg("hook")
        .arg("validate-pretool")
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
// Edge case 1: Case-insensitive bypass of "general-purpose"
//
// validate_agent uses exact match Some("general-purpose"). Case variants
// like "General-Purpose" fall through to Some(_) and are allowed.
// ---------------------------------------------------------------------------

#[test]
fn test_agent_block_case_insensitive_general_purpose() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "current_phase": "flow-code"
    });
    setup_flow_active_repo(dir.path(), branch, &state);

    let hook_input = json!({
        "tool_input": {
            "subagent_type": "General-Purpose"
        }
    });
    let stdin = serde_json::to_vec(&hook_input).unwrap();
    let output = run_validate_pretool(dir.path(), &stdin);

    assert_eq!(
        output.status.code().unwrap(),
        2,
        "General-Purpose (capitalized) should be blocked during FLOW phases, \
         but the exact-match implementation allows it through"
    );
}

#[test]
fn test_agent_block_uppercase_general_purpose() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "current_phase": "flow-code"
    });
    setup_flow_active_repo(dir.path(), branch, &state);

    let hook_input = json!({
        "tool_input": {
            "subagent_type": "GENERAL-PURPOSE"
        }
    });
    let stdin = serde_json::to_vec(&hook_input).unwrap();
    let output = run_validate_pretool(dir.path(), &stdin);

    assert_eq!(
        output.status.code().unwrap(),
        2,
        "GENERAL-PURPOSE (uppercase) should be blocked during FLOW phases, \
         but the exact-match implementation allows it through"
    );
}

// ---------------------------------------------------------------------------
// Edge case 2: Empty string subagent_type bypasses the block
//
// Some("") matches Some(_) and is allowed through, even though an empty
// string is semantically equivalent to an absent/default type.
// ---------------------------------------------------------------------------

#[test]
fn test_agent_block_empty_string_subagent_type() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "current_phase": "flow-code"
    });
    setup_flow_active_repo(dir.path(), branch, &state);

    let hook_input = json!({
        "tool_input": {
            "subagent_type": ""
        }
    });
    let stdin = serde_json::to_vec(&hook_input).unwrap();
    let output = run_validate_pretool(dir.path(), &stdin);

    assert_eq!(
        output.status.code().unwrap(),
        2,
        "Empty-string subagent_type should be blocked (treated as absent/default), \
         but the current implementation allows it through via the Some(_) catch-all"
    );
}

// ---------------------------------------------------------------------------
// Edge case 3: Whitespace-padded "general-purpose" bypasses the block
// ---------------------------------------------------------------------------

#[test]
fn test_agent_block_whitespace_padded_general_purpose() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "current_phase": "flow-code"
    });
    setup_flow_active_repo(dir.path(), branch, &state);

    let hook_input = json!({
        "tool_input": {
            "subagent_type": " general-purpose "
        }
    });
    let stdin = serde_json::to_vec(&hook_input).unwrap();
    let output = run_validate_pretool(dir.path(), &stdin);

    assert_eq!(
        output.status.code().unwrap(),
        2,
        "Whitespace-padded 'general-purpose' should be blocked, \
         but exact matching allows it through"
    );
}

// ---------------------------------------------------------------------------
// Edge case 4: Integration test - Agent call blocked during active flow
//
// Verifies the full run() path: stdin JSON -> tool_input extraction ->
// empty command detection -> validate_agent -> exit code 2.
// ---------------------------------------------------------------------------

#[test]
fn test_agent_integration_blocked_general_purpose_during_flow() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "current_phase": "flow-code"
    });
    setup_flow_active_repo(dir.path(), branch, &state);

    let hook_input = json!({
        "tool_input": {
            "subagent_type": "general-purpose",
            "prompt": "Do something"
        }
    });
    let stdin = serde_json::to_vec(&hook_input).unwrap();
    let output = run_validate_pretool(dir.path(), &stdin);

    assert_eq!(
        output.status.code().unwrap(),
        2,
        "general-purpose agent must be blocked during active FLOW phase"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("BLOCKED"),
        "stderr must contain BLOCKED message, got: {}",
        stderr
    );
    assert!(
        stderr.contains("general-purpose"),
        "stderr must name the blocked agent type, got: {}",
        stderr
    );
}

#[test]
fn test_agent_integration_allowed_flow_agent_during_flow() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "current_phase": "flow-code"
    });
    setup_flow_active_repo(dir.path(), branch, &state);

    let hook_input = json!({
        "tool_input": {
            "subagent_type": "flow:ci-fixer",
            "prompt": "Fix CI"
        }
    });
    let stdin = serde_json::to_vec(&hook_input).unwrap();
    let output = run_validate_pretool(dir.path(), &stdin);

    assert_eq!(
        output.status.code().unwrap(),
        0,
        "flow: namespace agents must be allowed through"
    );
}

#[test]
fn test_agent_integration_absent_type_blocked_during_flow() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "current_phase": "flow-code"
    });
    setup_flow_active_repo(dir.path(), branch, &state);

    let hook_input = json!({
        "tool_input": {
            "prompt": "Do something without specifying a type"
        }
    });
    let stdin = serde_json::to_vec(&hook_input).unwrap();
    let output = run_validate_pretool(dir.path(), &stdin);

    assert_eq!(
        output.status.code().unwrap(),
        2,
        "Absent subagent_type (default = general-purpose) must be blocked during active FLOW phase"
    );
}

#[test]
fn test_agent_integration_allowed_when_no_flow() {
    // When no flow is active (no state file), general-purpose agents
    // should be allowed through.
    let dir = tempfile::tempdir().unwrap();
    let _ = Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output();
    // Create .claude/settings.json so find_settings_and_root succeeds
    let claude_dir = dir.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(
        claude_dir.join("settings.json"),
        r#"{"permissions":{"allow":[],"deny":[]}}"#,
    )
    .unwrap();
    // No .flow-states/ directory -> flow_active is false

    let hook_input = json!({
        "tool_input": {
            "subagent_type": "general-purpose",
            "prompt": "Do something"
        }
    });
    let stdin = serde_json::to_vec(&hook_input).unwrap();
    let output = run_validate_pretool(dir.path(), &stdin);

    assert_eq!(
        output.status.code().unwrap(),
        0,
        "general-purpose agent must be allowed when no flow is active"
    );
}

// ---------------------------------------------------------------------------
// Edge case 5: subagent_type as non-string JSON value
//
// If subagent_type is a non-string JSON value (bool, int), .as_str()
// returns None, which maps to the blocked case via the None arm.
// ---------------------------------------------------------------------------

#[test]
fn test_agent_integration_non_string_subagent_type_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "current_phase": "flow-code"
    });
    setup_flow_active_repo(dir.path(), branch, &state);

    // subagent_type is a boolean, not a string
    let hook_input = json!({
        "tool_input": {
            "subagent_type": true,
            "prompt": "Try to bypass with non-string type"
        }
    });
    let stdin = serde_json::to_vec(&hook_input).unwrap();
    let output = run_validate_pretool(dir.path(), &stdin);

    // Non-string -> as_str() returns None -> treated as absent -> blocked
    assert_eq!(
        output.status.code().unwrap(),
        2,
        "Non-string subagent_type (bool) should be treated as absent and blocked"
    );
}

// ---------------------------------------------------------------------------
// Edge case 6: Agent tool call with command="" (explicitly empty string)
//
// When tool_input has command: "", this is still treated as empty command
// which triggers the Agent path. Verify blocking works.
// ---------------------------------------------------------------------------

#[test]
fn test_agent_integration_explicit_empty_command_triggers_agent_path() {
    let dir = tempfile::tempdir().unwrap();
    let branch = "test-feature";
    let state = json!({
        "branch": branch,
        "current_phase": "flow-code"
    });
    setup_flow_active_repo(dir.path(), branch, &state);

    let hook_input = json!({
        "tool_input": {
            "command": "",
            "subagent_type": "general-purpose",
            "prompt": "Do something"
        }
    });
    let stdin = serde_json::to_vec(&hook_input).unwrap();
    let output = run_validate_pretool(dir.path(), &stdin);

    assert_eq!(
        output.status.code().unwrap(),
        2,
        "Explicit empty command + general-purpose must still trigger Agent blocking"
    );
}

/// Subprocess test with cwd inode unlinked via `pre_exec` + `rmdir`.
/// Inside the child, `std::env::current_dir()` returns `ENOENT`,
/// which forces `hooks::find_settings_and_root()` to take its
/// `.unwrap_or_default()` branch AND `hooks::detect_branch_from_cwd()`
/// to take its `.ok()?` early-return. Both are private wrappers of
/// their `_from` variants, and this pre_exec test is the only path
/// that exercises the Err arm of the top-level `env::current_dir()`
/// call inside each.
#[cfg(unix)]
#[test]
fn validate_pretool_with_stale_cwd_does_not_panic() {
    use std::os::unix::process::CommandExt;

    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let cwd = root.join("doomed");
    fs::create_dir(&cwd).expect("mkdir doomed");

    let preexec_path =
        std::ffi::CString::new(cwd.to_str().expect("utf8").as_bytes()).expect("CString");

    let mut cmd = flow_rs();
    cmd.arg("hook")
        .arg("validate-pretool")
        .env_remove("FLOW_CI_RUNNING")
        .env_remove("FLOW_SIMULATE_BRANCH")
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // SAFETY: libc::rmdir is POSIX async-signal-safe. The closure
    // allocates nothing, produces no panic surface, and does not
    // interact with any parent-process state.
    unsafe {
        cmd.pre_exec(move || {
            libc::rmdir(preexec_path.as_ptr());
            Ok(())
        });
    }

    let mut child = cmd.spawn().expect("spawn flow-rs");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(br#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#)
        .unwrap();
    let output = child.wait_with_output().expect("wait");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked at"),
        "validate-pretool must not panic with stale cwd; stderr={}",
        stderr
    );
}
