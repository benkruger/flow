//! Integration tests for `src/hooks/validate_skill.rs`.
//!
//! Drives the `validate` decision core directly with controlled
//! `tool_input`, `transcript_path`, and `home` fixtures. Subprocess
//! integration test (`subprocess_validate_skill_blocks_user_only_invocation_without_user_command`
//! and siblings) lives below the unit tests and exercises the
//! compiled binary. `transcript_fixture` reaches in from
//! `tests/common/mod.rs` via `crate::common` because
//! `tests/hooks/main.rs` declares the path-aliased common module.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use flow_rs::hooks::transcript_walker::USER_ONLY_SKILLS;
use flow_rs::hooks::validate_skill::validate;
use serde_json::json;

// --- validate (decision core) ---

#[test]
fn validate_allows_when_skill_not_in_user_only_set() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let tool_input = json!({"skill": "flow:flow-status"});
    let (allowed, msg) = validate(&tool_input, None, home);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn validate_allows_when_skill_not_in_user_only_set_even_if_transcript_missing() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let missing = home
        .join(".claude")
        .join("projects")
        .join("p")
        .join("nonexistent.jsonl");
    let tool_input = json!({"skill": "flow:flow-status"});
    let (allowed, msg) = validate(&tool_input, Some(&missing), home);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn validate_blocks_when_user_only_skill_lacks_user_invocation() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // Transcript exists and is well-formed but has no matching
    // `<command-name>` tag. Layer 1 must block.
    let jsonl =
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"unrelated message\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    let tool_input = json!({"skill": "flow:flow-abort"});
    let (allowed, msg) = validate(&tool_input, Some(&path), home);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn validate_allows_when_user_only_skill_has_user_invocation() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    let tool_input = json!({"skill": "flow:flow-abort"});
    let (allowed, msg) = validate(&tool_input, Some(&path), home);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn validate_block_message_names_skill() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"unrelated\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    let tool_input = json!({"skill": "flow:flow-abort"});
    let (_, msg) = validate(&tool_input, Some(&path), home);
    assert!(msg.contains("`flow:flow-abort`"));
}

#[test]
fn validate_block_message_references_rule_file() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"unrelated\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    let tool_input = json!({"skill": "flow:flow-abort"});
    let (_, msg) = validate(&tool_input, Some(&path), home);
    assert!(msg.contains(".claude/rules/user-only-skills.md"));
}

// One test per user-only skill — verifies each is in the set.
#[test]
fn validate_user_only_skill_flow_abort_is_in_set() {
    assert!(USER_ONLY_SKILLS.contains(&"flow:flow-abort"));
}

#[test]
fn validate_user_only_skill_flow_reset_is_in_set() {
    assert!(USER_ONLY_SKILLS.contains(&"flow:flow-reset"));
}

#[test]
fn validate_user_only_skill_flow_release_is_in_set() {
    assert!(USER_ONLY_SKILLS.contains(&"flow:flow-release"));
}

#[test]
fn validate_user_only_skill_flow_prime_is_in_set() {
    assert!(USER_ONLY_SKILLS.contains(&"flow:flow-prime"));
}

#[test]
fn validate_fail_open_when_tool_input_missing_skill_field() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // tool_input has no `skill` field. Treat as non-user-only and
    // allow. Defense in depth: the absent field is not a synthetic
    // block trigger.
    let tool_input = json!({});
    let (allowed, msg) = validate(&tool_input, None, home);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn validate_blocks_when_user_only_skill_and_no_transcript_path() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // No transcript path — walker can't verify user invocation, so
    // the user-only skill is blocked by default.
    let tool_input = json!({"skill": "flow:flow-abort"});
    let (allowed, msg) = validate(&tool_input, None, home);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

// --- Adversarial regression tests for gate normalization ---
//
// Each test below locks in a Code Review fix that closed a Layer 1
// bypass — case variation, trailing whitespace, NUL padding. The
// `normalize_gate_input` helper now strips all three before the
// `USER_ONLY_SKILLS` membership check.

#[test]
fn validate_blocks_case_variant_user_only_skill_name() {
    // `Flow:Flow-Abort` previously bypassed `USER_ONLY_SKILLS.contains`
    // because the membership check was exact-string. With
    // `normalize_gate_input`, both sides lowercase before comparison.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let tool_input = json!({"skill": "Flow:Flow-Abort"});
    let (allowed, msg) = validate(&tool_input, None, home);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn validate_blocks_whitespace_padded_user_only_skill_name() {
    // `flow:flow-abort ` (trailing space) previously bypassed the
    // membership check. Normalization trims whitespace.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let tool_input = json!({"skill": "  flow:flow-abort  "});
    let (allowed, msg) = validate(&tool_input, None, home);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn validate_blocks_nul_padded_user_only_skill_name() {
    // `flow:flow-abort\0` previously bypassed the membership check.
    // Normalization strips NUL bytes.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let tool_input = json!({"skill": "flow:flow-abort\0"});
    let (allowed, msg) = validate(&tool_input, None, home);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn validate_block_message_uses_normalized_skill_name() {
    // The block message echoes the normalized skill name so the
    // user always sees the canonical form regardless of how the
    // model phrased the bypass attempt.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let tool_input = json!({"skill": "  Flow:FLOW-Abort\0"});
    let (allowed, msg) = validate(&tool_input, None, home);
    assert!(!allowed);
    assert!(msg.contains("`flow:flow-abort`"));
}

#[test]
fn validate_blocks_user_prose_mention_of_command_marker() {
    // A user message that mentions the literal
    // `<command-name>/flow:flow-abort</command-name>` substring
    // mid-text is NOT a slash-command invocation. The walker
    // requires the marker at the START of the trimmed content,
    // so this case must block.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"can you describe what <command-name>/flow:flow-abort</command-name> does?\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    let tool_input = json!({"skill": "flow:flow-abort"});
    let (allowed, msg) = validate(&tool_input, Some(&path), home);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

// --- subprocess integration tests ---

fn run_hook_subprocess(stdin_input: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["hook", "validate-skill"])
        .env_remove("FLOW_CI_RUNNING")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn flow-rs");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(stdin_input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().expect("wait");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn write_jsonl_fixture(home: &Path, jsonl: &str) -> std::path::PathBuf {
    crate::common::transcript_fixture(home, "p", jsonl)
}

#[test]
fn subprocess_validate_skill_blocks_user_only_invocation_without_user_command() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-abort\"}}]}}\n";
    let path = write_jsonl_fixture(home, jsonl);
    let payload = json!({
        "tool_input": {"skill": "flow:flow-abort"},
        "transcript_path": path.to_string_lossy(),
    });
    // Override HOME for the subprocess so is_safe_transcript_path
    // accepts the tempdir-rooted fixture.
    let mut child = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["hook", "validate-skill"])
        .env_remove("FLOW_CI_RUNNING")
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn flow-rs");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().expect("wait");
    assert_eq!(output.status.code().unwrap_or(-1), 2);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("BLOCKED"), "stderr: {}", stderr);
    assert!(
        stderr.contains("flow:flow-abort"),
        "stderr should name skill: {}",
        stderr
    );
}

#[test]
fn subprocess_validate_skill_allows_when_user_invocation_present() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n";
    let path = write_jsonl_fixture(home, jsonl);
    let payload = json!({
        "tool_input": {"skill": "flow:flow-abort"},
        "transcript_path": path.to_string_lossy(),
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["hook", "validate-skill"])
        .env_remove("FLOW_CI_RUNNING")
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn flow-rs");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().expect("wait");
    assert_eq!(output.status.code().unwrap_or(-1), 0);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.is_empty(), "stderr should be empty: {}", stderr);
}

#[test]
fn subprocess_validate_skill_allows_when_skill_not_user_only() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"unrelated\"}}\n";
    let path = write_jsonl_fixture(home, jsonl);
    let payload = json!({
        "tool_input": {"skill": "flow:flow-status"},
        "transcript_path": path.to_string_lossy(),
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["hook", "validate-skill"])
        .env_remove("FLOW_CI_RUNNING")
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn flow-rs");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().expect("wait");
    assert_eq!(output.status.code().unwrap_or(-1), 0);
}

#[test]
fn subprocess_validate_skill_allows_when_no_stdin() {
    // No stdin payload — hook silently allows (exit 0, no stderr).
    let (code, _stdout, stderr) = run_hook_subprocess("");
    assert_eq!(code, 0);
    assert!(stderr.is_empty(), "stderr should be empty: {}", stderr);
}
