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
use flow_rs::hooks::validate_skill::{run_impl_main, validate};
use serde_json::json;

// --- validate (decision core) ---

#[test]
fn validate_allows_when_skill_not_in_user_only_set() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let tool_input = json!({"skill": "flow:flow-status"});
    let (allowed, msg) = validate(&tool_input, None, None, home);
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
    let (allowed, msg) = validate(&tool_input, Some(&missing), None, home);
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
    let (allowed, msg) = validate(&tool_input, Some(&path), None, home);
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
    let (allowed, msg) = validate(&tool_input, Some(&path), None, home);
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
    let (_, msg) = validate(&tool_input, Some(&path), None, home);
    assert!(msg.contains("`flow:flow-abort`"));
}

#[test]
fn validate_block_message_references_rule_file() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"unrelated\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    let tool_input = json!({"skill": "flow:flow-abort"});
    let (_, msg) = validate(&tool_input, Some(&path), None, home);
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
    assert!(USER_ONLY_SKILLS.contains(&"flow-release"));
}

#[test]
fn validate_user_only_skill_flow_prime_is_in_set() {
    assert!(USER_ONLY_SKILLS.contains(&"flow:flow-prime"));
}

#[test]
fn validate_user_only_skill_flow_qa_is_in_set() {
    assert!(USER_ONLY_SKILLS.contains(&"flow-qa"));
}

#[test]
fn validate_fail_open_when_tool_input_missing_skill_field() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // tool_input has no `skill` field. Treat as non-user-only and
    // allow. Defense in depth: the absent field is not a synthetic
    // block trigger.
    let tool_input = json!({});
    let (allowed, msg) = validate(&tool_input, None, None, home);
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
    let (allowed, msg) = validate(&tool_input, None, None, home);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

// --- Adversarial regression tests for gate normalization ---
//
// Each test below locks in a Review fix that closed a Layer 1
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
    let (allowed, msg) = validate(&tool_input, None, None, home);
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
    let (allowed, msg) = validate(&tool_input, None, None, home);
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
    let (allowed, msg) = validate(&tool_input, None, None, home);
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
    let (allowed, msg) = validate(&tool_input, None, None, home);
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
    let (allowed, msg) = validate(&tool_input, Some(&path), None, home);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

// --- halt-pending gate ---
//
// Layer 2 in `validate`: when `_halt_pending=true` in the state
// file, every Skill call is blocked except user-only-skill exits
// the user has already typed. The block message names the two
// sanctioned exits (`/flow:flow-continue`, `/flow:flow-abort`) so
// the model surfaces the recovery instruction to the user.

/// Write a minimal state file with `_halt_pending=<halt>` at
/// `<home>/state.json` and return its path. Tests use this to
/// drive the halt-gate branches without needing a full FlowPaths
/// fixture — `validate` accepts an opaque `state_path` and reads it
/// directly.
fn state_with_halt(home: &Path, halt: bool) -> std::path::PathBuf {
    let path = home.join("state.json");
    let content = json!({"_halt_pending": halt}).to_string();
    std::fs::write(&path, content).expect("write state fixture");
    path
}

#[test]
fn validate_skill_blocks_model_initiated_skill_during_halt() {
    // Non-user-only skill that would pass Layer 1 cleanly. With
    // halt set in the state file, the halt gate blocks it and the
    // message names the two exits the user has.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let state = state_with_halt(home, true);
    let tool_input = json!({"skill": "flow:flow-status"});
    let (allowed, msg) = validate(&tool_input, None, Some(&state), home);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"), "message: {}", msg);
    assert!(
        msg.contains("/flow:flow-continue"),
        "message must name /flow:flow-continue: {}",
        msg
    );
    assert!(
        msg.contains("/flow:flow-abort"),
        "message must name /flow:flow-abort: {}",
        msg
    );
}

#[test]
fn validate_skill_allows_user_only_skill_during_halt_when_user_typed_it() {
    // User-only skill with a matching user invocation in the
    // transcript: Layer 1 allows. The halt gate must NOT fire —
    // the user-only exits are the sanctioned halt-window exits.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let state = state_with_halt(home, true);
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-continue</command-name>\"}}\n";
    let transcript = crate::common::transcript_fixture(home, "p", jsonl);
    let tool_input = json!({"skill": "flow:flow-continue"});
    let (allowed, msg) = validate(&tool_input, Some(&transcript), Some(&state), home);
    assert!(allowed, "user-typed exit must pass: {}", msg);
    assert!(msg.is_empty());
}

#[test]
fn validate_skill_allows_skill_when_halt_not_set() {
    // Halt is false. Non-user-only skills pass through Layer 1
    // cleanly with no halt-gate interference.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let state = state_with_halt(home, false);
    let tool_input = json!({"skill": "flow:flow-status"});
    let (allowed, msg) = validate(&tool_input, None, Some(&state), home);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn validate_skill_halt_state_path_missing_passes_through() {
    // State path points to a nonexistent file. `File::open` Err
    // arm returns false (no halt), the gate stays silent, and the
    // non-user-only skill passes through.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let missing = home.join("nonexistent-state.json");
    let tool_input = json!({"skill": "flow:flow-status"});
    let (allowed, msg) = validate(&tool_input, None, Some(&missing), home);
    assert!(allowed, "missing state file must fail-open: {}", msg);
    assert!(msg.is_empty());
}

#[test]
fn validate_skill_halt_state_path_non_json_passes_through() {
    // State file exists but contains non-JSON content. `serde_json`
    // Err arm returns false (no halt), the gate stays silent, and
    // the non-user-only skill passes through.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let path = home.join("garbage.json");
    std::fs::write(&path, "this is not JSON").unwrap();
    let tool_input = json!({"skill": "flow:flow-status"});
    let (allowed, msg) = validate(&tool_input, None, Some(&path), home);
    assert!(allowed, "non-JSON state file must fail-open: {}", msg);
    assert!(msg.is_empty());
}

#[test]
fn validate_skill_halt_state_path_missing_field_passes_through() {
    // State file is well-formed JSON but lacks `_halt_pending`.
    // `value.get(...)` returns None, `is_truthy(None)` is false,
    // the gate stays silent.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let path = home.join("no-halt.json");
    std::fs::write(&path, r#"{"current_phase":"flow-code"}"#).unwrap();
    let tool_input = json!({"skill": "flow:flow-status"});
    let (allowed, msg) = validate(&tool_input, None, Some(&path), home);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn validate_skill_normalizes_halt_field_type() {
    // `_halt_pending` arrives over JSON as a value Claude Code
    // does not contractually pin to bool. Per
    // `.claude/rules/rust-patterns.md` "Hook Input Boolean Field
    // Tolerance", every truthy form must trigger the halt block on
    // a non-user-only skill: bool true, string "true", string "1",
    // non-zero number.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let truthy_values = vec![json!(true), json!("true"), json!("1"), json!(1)];
    for (idx, halt_value) in truthy_values.into_iter().enumerate() {
        let state_path = home.join(format!("state-{}.json", idx));
        let state = json!({"_halt_pending": halt_value.clone()});
        std::fs::write(&state_path, state.to_string()).unwrap();
        let tool_input = json!({"skill": "flow:flow-status"});
        let (allowed, msg) = validate(&tool_input, None, Some(&state_path), home);
        assert!(
            !allowed,
            "halt value {} must block (msg: {})",
            halt_value, msg
        );
        assert!(
            msg.contains("/flow:flow-continue"),
            "halt value {} block message must name /flow:flow-continue: {}",
            halt_value,
            msg
        );
    }
}

// --- run_impl_main: state-path resolution ---
//
// `run_impl_main` derives the state path from cwd → project_root →
// branch → FlowPaths. Each step has an early-return arm when the
// upstream lookup fails. These tests drive `run_impl_main` directly
// to cover every Option::? branch in the cwd-resolution closure.

#[test]
fn run_impl_main_returns_zero_when_hook_input_none() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let (code, msg) = run_impl_main(None, None, home);
    assert_eq!(code, 0);
    assert!(msg.is_none());
}

#[test]
fn run_impl_main_passes_through_when_cwd_none() {
    // No cwd → state_path stays None → halt gate sees false →
    // non-user-only skill passes through.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let payload = json!({"tool_input": {"skill": "flow:flow-status"}});
    let (code, msg) = run_impl_main(Some(payload), None, home);
    assert_eq!(code, 0);
    assert!(msg.is_none());
}

#[test]
fn run_impl_main_passes_through_when_cwd_outside_worktree() {
    // cwd points at a tempdir with no `.worktrees/` ancestor and no
    // `.git`. `detect_branch_from_path` falls back to `git
    // branch --show-current`; in a non-git tempdir this fails and
    // returns None. The `?` short-circuits the closure, state_path
    // stays None, the halt gate passes through.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let cwd = dir.path();
    let payload = json!({"tool_input": {"skill": "flow:flow-status"}});
    let (code, msg) = run_impl_main(Some(payload), Some(cwd), home);
    assert_eq!(code, 0);
    assert!(msg.is_none());
}

#[test]
fn run_impl_main_passes_through_when_no_settings_ancestor() {
    // cwd has a `.worktrees/<branch>` marker so
    // `detect_branch_from_path` returns Some, but no `.claude/settings.json`
    // ancestor exists so `project_root` resolves to None. The `?`
    // on `project_root.map(...)` short-circuits the closure;
    // state_path stays None; halt gate passes through.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let worktree = root.join(".worktrees").join("feat");
    std::fs::create_dir_all(&worktree).unwrap();
    // `.git` marker as a file (worktree convention) so
    // detect_branch_from_path picks it up without falling back to
    // the git subprocess.
    std::fs::write(worktree.join(".git"), "gitdir: ../../.git/worktrees/feat\n").unwrap();
    let payload = json!({"tool_input": {"skill": "flow:flow-status"}});
    let (code, msg) = run_impl_main(Some(payload), Some(&worktree), &root);
    assert_eq!(code, 0);
    assert!(msg.is_none());
}

#[test]
fn run_impl_main_passes_through_when_branch_invalid() {
    // cwd resolves to a slash-containing "branch" because the
    // worktree dir has an internal subdirectory under
    // `.worktrees/foo/bar/`. `FlowPaths::try_new` rejects slash-
    // containing branches and returns None; the `?` short-circuits;
    // state_path stays None; halt gate passes through.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    // `.claude/settings.json` at root so project_root resolves.
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    std::fs::write(root.join(".claude").join("settings.json"), "{}").unwrap();
    let worktree = root.join(".worktrees").join("foo").join("bar");
    std::fs::create_dir_all(&worktree).unwrap();
    // Place the `.git` marker file under the nested path so the
    // walker returns the slash-containing relative path as the
    // branch.
    std::fs::write(
        worktree.join(".git"),
        "gitdir: ../../../.git/worktrees/foo-bar\n",
    )
    .unwrap();
    let payload = json!({"tool_input": {"skill": "flow:flow-status"}});
    let (code, msg) = run_impl_main(Some(payload), Some(&worktree), &root);
    assert_eq!(code, 0);
    assert!(msg.is_none());
}

#[test]
fn run_impl_main_blocks_user_only_skill_via_validate() {
    // Wire validate's block path through run_impl_main: user-only
    // skill with no transcript triggers the Layer 1 block.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let payload = json!({"tool_input": {"skill": "flow:flow-abort"}});
    let (code, msg) = run_impl_main(Some(payload), None, home);
    assert_eq!(code, 2);
    assert!(msg.as_deref().is_some_and(|m| m.contains("BLOCKED")));
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

/// Spawn validate-skill with an explicit payload `cwd` field while the
/// process's real cwd (`.current_dir`) is a DIFFERENT directory.
/// Proves `run()` resolves the state file's worktree from the payload
/// `cwd`, not env::current_dir(). HOME is pinned to `real_cwd` per
/// `.claude/rules/subprocess-test-hygiene.md`. Returns (exit, stderr).
fn spawn_skill_with_payload_cwd(real_cwd: &Path, stdin_input: &str) -> (i32, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["hook", "validate-skill"])
        .env_remove("FLOW_CI_RUNNING")
        .env("HOME", real_cwd)
        .current_dir(real_cwd)
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
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn validate_skill_reads_payload_cwd_engages_gate() {
    // The halt gate must resolve the state file's worktree from the
    // payload `cwd` field, not env::current_dir(). Real cwd is the
    // project root (non-worktree): branch detection fails there,
    // state_path stays None, and the halt gate passes through. The
    // payload cwd points at the worktree whose flow has
    // `_halt_pending=true`. With the payload honored, a non-user-only
    // Skill call is blocked (exit 2). Reading env::current_dir() (the
    // project root) would allow (exit 0).
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    std::fs::write(root.join(".claude").join("settings.json"), "{}").unwrap();
    let worktree = root.join(".worktrees").join("feat");
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(worktree.join(".git"), "gitdir: fake\n").unwrap();
    let branch_dir = root.join(".flow-states").join("feat");
    std::fs::create_dir_all(&branch_dir).unwrap();
    std::fs::write(
        branch_dir.join("state.json"),
        json!({"_halt_pending": true}).to_string(),
    )
    .unwrap();
    let input = format!(
        r#"{{"cwd":"{}","tool_input":{{"skill":"flow:flow-status"}}}}"#,
        worktree.to_string_lossy()
    );
    let (code, stderr) = spawn_skill_with_payload_cwd(&root, &input);
    assert_eq!(
        code, 2,
        "payload cwd must engage the halt gate; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
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
