//! Integration tests for `src/hooks/validate_ask_user.rs`.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use flow_rs::hooks::validate_ask_user::{set_blocked, user_only_skill_carve_out_applies, validate};
use serde_json::{json, Value};

/// Build a JSONL transcript fixture under
/// `<home>/.claude/projects/p/session.jsonl`. Returns the path.
/// Inlined here rather than imported from `tests/common/mod.rs`
/// because subdirectory tests use `#[path = "../common/mod.rs"]
/// mod common;` and we already have `mod common;` indirectly via
/// other tests in this module — keeping this helper self-contained
/// avoids the import dance.
fn carve_out_transcript_fixture(home: &Path, jsonl: &str) -> std::path::PathBuf {
    let dir = home.join(".claude").join("projects").join("p");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("session.jsonl");
    fs::write(&path, jsonl).unwrap();
    path
}

fn write_state(dir: &Path, branch: &str, state: &Value) -> std::path::PathBuf {
    let branch_dir = dir.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    let path = branch_dir.join("state.json");
    fs::write(&path, serde_json::to_string_pretty(state).unwrap()).unwrap();
    path
}

// --- validate tests ---

#[test]
fn test_validate_allows_no_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.json");
    let (allowed, msg, resp) = validate(Some(&path));
    assert!(allowed);
    assert!(msg.is_empty());
    assert!(resp.is_none());
}

#[test]
fn test_validate_allows_none_state_path() {
    let (allowed, msg, resp) = validate(None);
    assert!(allowed);
    assert!(msg.is_empty());
    assert!(resp.is_none());
}

#[test]
fn test_validate_allows_invalid_json() {
    let dir = tempfile::tempdir().unwrap();
    let bad_file = dir.path().join("bad.json");
    fs::write(&bad_file, "not json at all").unwrap();
    let (allowed, msg, resp) = validate(Some(&bad_file));
    assert!(allowed);
    assert!(msg.is_empty());
    assert!(resp.is_none());
}

struct PermissionGuard {
    path: std::path::PathBuf,
    restore_mode: u32,
}
impl Drop for PermissionGuard {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(self.restore_mode));
    }
}

#[test]
fn test_validate_allows_unreadable_state_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let unreadable = dir.path().join("unreadable.json");
    fs::write(&unreadable, "{}").unwrap();
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();
    let _guard = PermissionGuard {
        path: unreadable.clone(),
        restore_mode: 0o644,
    };
    let (allowed, msg, resp) = validate(Some(&unreadable));
    assert!(allowed);
    assert!(msg.is_empty());
    assert!(resp.is_none());
}

#[test]
fn test_validate_allows_no_auto_continue() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({"current_phase": "flow-start", "branch": "test"});
    let path = write_state(dir.path(), "test", &state);
    let (allowed, msg, resp) = validate(Some(&path));
    assert!(allowed);
    assert!(msg.is_empty());
    assert!(resp.is_none());
}

#[test]
fn test_validate_allows_empty_auto_continue() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-start",
        "branch": "test",
        "_auto_continue": "",
    });
    let path = write_state(dir.path(), "test", &state);
    let (allowed, msg, resp) = validate(Some(&path));
    assert!(allowed);
    assert!(msg.is_empty());
    assert!(resp.is_none());
}

#[test]
fn test_validate_auto_continue_returns_hook_response() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-start",
        "branch": "test",
        "_auto_continue": "/flow:flow-code",
    });
    let path = write_state(dir.path(), "test", &state);
    let (allowed, _msg, resp) = validate(Some(&path));
    assert!(allowed);
    assert!(resp.is_some());
    let resp = resp.unwrap();
    assert_eq!(resp["permissionDecision"], "allow");
    assert!(resp["updatedInput"]
        .as_str()
        .unwrap()
        .contains("/flow:flow-code"));
}

#[test]
fn test_validate_auto_continue_includes_command() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "_auto_continue": "/flow:flow-review",
    });
    let path = write_state(dir.path(), "test", &state);
    let (allowed, _msg, resp) = validate(Some(&path));
    assert!(allowed);
    assert!(resp.is_some());
    let resp = resp.unwrap();
    assert_eq!(resp["permissionDecision"], "allow");
    assert!(resp["updatedInput"]
        .as_str()
        .unwrap()
        .contains("/flow:flow-review"));
}

// --- validate BLOCK path tests ---

#[test]
fn test_validate_blocks_when_skills_continue_auto_detailed() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "skills": {"flow-code": {"continue": "auto", "commit": "auto"}},
        "phases": {"flow-code": {"status": "in_progress"}},
    });
    let path = write_state(dir.path(), "test", &state);
    let (allowed, msg, resp) = validate(Some(&path));
    assert!(!allowed);
    assert!(msg.contains("flow-code"));
    assert!(resp.is_none());
}

#[test]
fn test_validate_blocks_when_skills_continue_auto_simple() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "skills": {"flow-code": "auto"},
        "phases": {"flow-code": {"status": "in_progress"}},
    });
    let path = write_state(dir.path(), "test", &state);
    let (allowed, msg, resp) = validate(Some(&path));
    assert!(!allowed);
    assert!(msg.contains("flow-code"));
    assert!(resp.is_none());
}

#[test]
fn test_validate_allows_when_skills_continue_manual() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "skills": {"flow-code": {"continue": "manual"}},
        "phases": {"flow-code": {"status": "in_progress"}},
    });
    let path = write_state(dir.path(), "test", &state);
    let (allowed, msg, resp) = validate(Some(&path));
    assert!(allowed);
    assert!(msg.is_empty());
    assert!(resp.is_none());
}

#[test]
fn test_validate_allows_when_skills_key_missing() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "phases": {"flow-code": {"status": "in_progress"}},
    });
    let path = write_state(dir.path(), "test", &state);
    let (allowed, msg, resp) = validate(Some(&path));
    assert!(allowed);
    assert!(msg.is_empty());
    assert!(resp.is_none());
}

#[test]
fn test_validate_allows_when_current_phase_not_in_skills() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "skills": {"flow-start": {"continue": "auto"}},
        "phases": {"flow-code": {"status": "in_progress"}},
    });
    let path = write_state(dir.path(), "test", &state);
    let (allowed, _msg, resp) = validate(Some(&path));
    assert!(allowed);
    assert!(resp.is_none());
}

#[test]
fn test_validate_block_precedes_auto_continue() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "skills": {"flow-code": {"continue": "auto"}},
        "phases": {"flow-code": {"status": "in_progress"}},
        "_auto_continue": "/flow:flow-review",
    });
    let path = write_state(dir.path(), "test", &state);
    let (allowed, msg, resp) = validate(Some(&path));
    assert!(!allowed);
    assert!(msg.contains("flow-code"));
    assert!(resp.is_none());
}

#[test]
fn test_validate_auto_continue_without_skills_auto() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "skills": {"flow-code": {"continue": "manual"}},
        "phases": {"flow-code": {"status": "in_progress"}},
        "_auto_continue": "/flow:flow-review",
    });
    let path = write_state(dir.path(), "test", &state);
    let (allowed, _msg, resp) = validate(Some(&path));
    assert!(allowed);
    assert!(resp.is_some());
    let resp = resp.unwrap();
    assert_eq!(resp["permissionDecision"], "allow");
}

#[test]
fn test_validate_block_message_names_phase() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-learn",
        "branch": "test",
        "skills": {"flow-learn": {"continue": "auto"}},
        "phases": {"flow-learn": {"status": "in_progress"}},
    });
    let path = write_state(dir.path(), "test", &state);
    let (_allowed, msg, _resp) = validate(Some(&path));
    assert!(msg.contains("flow-learn"));
}

#[test]
fn test_validate_allows_no_current_phase() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "branch": "test",
        "skills": {"flow-code": {"continue": "auto"}},
        "phases": {"flow-code": {"status": "in_progress"}},
    });
    let path = write_state(dir.path(), "test", &state);
    let (allowed, _msg, resp) = validate(Some(&path));
    assert!(allowed);
    assert!(resp.is_none());
}

#[test]
fn test_validate_corrupt_skills_value() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "skills": [1, 2, 3],
        "phases": {"flow-code": {"status": "in_progress"}},
    });
    let path = write_state(dir.path(), "test", &state);
    let (allowed, _msg, resp) = validate(Some(&path));
    assert!(allowed);
    assert!(resp.is_none());
}

#[test]
fn test_validate_allows_at_transition_boundary_pending_phase() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-review",
        "branch": "test",
        "skills": {"flow-review": {"continue": "auto"}},
        "phases": {
            "flow-code": {"status": "complete"},
            "flow-review": {"status": "pending"},
        },
    });
    let path = write_state(dir.path(), "test", &state);
    let (allowed, msg, resp) = validate(Some(&path));
    assert!(allowed);
    assert!(msg.is_empty());
    assert!(resp.is_none());
}

#[test]
fn test_validate_allows_when_phase_status_missing() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "skills": {"flow-code": {"continue": "auto"}},
    });
    let path = write_state(dir.path(), "test", &state);
    let (allowed, _msg, resp) = validate(Some(&path));
    assert!(allowed);
    assert!(resp.is_none());
}

#[test]
fn test_validate_allows_when_phase_status_complete() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "skills": {"flow-code": {"continue": "auto"}},
        "phases": {"flow-code": {"status": "complete"}},
    });
    let path = write_state(dir.path(), "test", &state);
    let (allowed, _msg, resp) = validate(Some(&path));
    assert!(allowed);
    assert!(resp.is_none());
}

#[test]
fn test_validate_corrupt_phases_value() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "skills": {"flow-code": {"continue": "auto"}},
        "phases": "not-an-object",
    });
    let path = write_state(dir.path(), "test", &state);
    let (allowed, _msg, resp) = validate(Some(&path));
    assert!(allowed);
    assert!(resp.is_none());
}

// --- set_blocked tests ---

#[test]
fn test_set_blocked_sets_timestamp() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({"current_phase": "flow-code", "branch": "test"});
    let path = write_state(dir.path(), "test", &state);
    set_blocked(&path);
    let updated: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(updated.get("_blocked").is_some());
    assert!(!updated["_blocked"].as_str().unwrap().is_empty());
}

#[test]
fn test_set_blocked_no_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.json");
    set_blocked(&path);
}

#[test]
fn test_set_blocked_corrupt_state() {
    let dir = tempfile::tempdir().unwrap();
    let bad_file = dir.path().join("bad.json");
    fs::write(&bad_file, "{bad json").unwrap();
    set_blocked(&bad_file);
}

#[test]
fn test_set_blocked_non_object_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("array.json");
    fs::write(&path, "[1, 2, 3]").unwrap();
    set_blocked(&path);
    let content = fs::read_to_string(&path).unwrap();
    let parsed: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed, json!([1, 2, 3]));
}

#[test]
fn test_set_blocked_preserves_other_fields() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "session_id": "existing-session",
        "notes": [{"note": "a correction"}],
    });
    let path = write_state(dir.path(), "test", &state);
    set_blocked(&path);
    let updated: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(updated["session_id"], "existing-session");
    assert_eq!(updated["notes"][0]["note"], "a correction");
    assert!(updated.get("_blocked").is_some());
}

// --- run() subprocess test ---

fn run_hook(cwd: &Path, stdin_input: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["hook", "validate-ask-user"])
        .current_dir(cwd)
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

/// `run()` exits 0 when the cwd isn't a git repo (current_branch None)
/// or the state file doesn't exist. Exercises the real-subprocess
/// wrapper.
#[test]
fn run_subprocess_exits_0_outside_git_repo() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (code, _stdout, _stderr) = run_hook(&root, "{}");
    assert_eq!(code, 0);
}

// Direct `run_impl_main` / `HookAction` tests removed — the decision
// core is now private, and its branches are exercised through the
// subprocess tests below that spawn `bin/flow hook validate-ask-user`
// against fixture state files.

// Exercise the block and auto-answer subprocess paths so the stdio
// side-effect branches of `run()` are covered.
#[test]
fn run_subprocess_exits_2_when_phase_in_progress_auto() {
    // The subprocess needs a git repo where `current_branch` resolves
    // AND a state file at the resolved path. Build both.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    // minimal git repo on branch `test`
    Command::new("git")
        .args(["init", "--initial-branch", "test"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "a@b"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "t"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "commit.gpgsign", "false"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&root)
        .output()
        .unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "skills": {"flow-code": {"continue": "auto"}},
        "phases": {"flow-code": {"status": "in_progress"}},
    });
    write_state(&root, "test", &state);

    let (code, _stdout, stderr) = run_hook(&root, "{}");
    assert_eq!(code, 2);
    assert!(stderr.contains("BLOCKED"), "stderr: {}", stderr);
}

#[test]
fn run_subprocess_auto_answers_when_auto_continue_set() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    Command::new("git")
        .args(["init", "--initial-branch", "test"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "a@b"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "t"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "commit.gpgsign", "false"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&root)
        .output()
        .unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "_auto_continue": "/flow:flow-review",
    });
    write_state(&root, "test", &state);

    let (code, stdout, _stderr) = run_hook(&root, "{}");
    assert_eq!(code, 0);
    assert!(stdout.contains("permissionDecision"), "stdout: {}", stdout);
    assert!(stdout.contains("/flow:flow-review"), "stdout: {}", stdout);
}

#[test]
fn run_subprocess_sets_blocked_on_allow_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    Command::new("git")
        .args(["init", "--initial-branch", "test"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "a@b"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "t"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "commit.gpgsign", "false"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&root)
        .output()
        .unwrap();
    let state = json!({"current_phase": "flow-code", "branch": "test"});
    let state_path = write_state(&root, "test", &state);

    let (code, _stdout, _stderr) = run_hook(&root, "{}");
    assert_eq!(code, 0);
    let updated: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert!(updated.get("_blocked").is_some(), "state: {:?}", updated);
}

// --- user_only_skill_carve_out_applies ---

#[test]
fn validate_ask_user_carve_out_allows_when_user_only_skill_in_transcript_during_in_progress_auto() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"do something\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-abort\"}}]}}\n";
    let path = carve_out_transcript_fixture(home, jsonl);
    assert!(user_only_skill_carve_out_applies(Some(&path), home));
}

#[test]
fn validate_ask_user_carve_out_does_not_apply_when_skill_not_user_only() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"check\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-create-issue\"}}]}}\n";
    let path = carve_out_transcript_fixture(home, jsonl);
    assert!(!user_only_skill_carve_out_applies(Some(&path), home));
}

#[test]
fn validate_ask_user_carve_out_does_not_apply_when_no_recent_skill_invocation() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // Transcript contains only a user turn — walker hits the user
    // turn without finding any Skill tool_use call. Carve-out
    // returns false; existing block stands.
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n";
    let path = carve_out_transcript_fixture(home, jsonl);
    assert!(!user_only_skill_carve_out_applies(Some(&path), home));
}

#[test]
fn validate_ask_user_carve_out_does_not_apply_when_transcript_path_missing() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // No transcript path at all — carve-out cannot fire. Existing
    // block stands.
    assert!(!user_only_skill_carve_out_applies(None, home));
}

#[test]
fn validate_ask_user_unaffected_when_phase_not_in_progress_auto() {
    // Pre-existing behavior preserved: when validate would NOT have
    // blocked (manual phase), the carve-out is irrelevant. Verify
    // the existing validate path returns allow without invoking the
    // carve-out helper.
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "phases": {"flow-code": {"status": "in_progress"}},
        "skills": {"flow-code": {"continue": "manual"}},
    });
    let path = write_state(dir.path(), "test", &state);
    let (allowed, _msg, _resp) = validate(Some(&path));
    assert!(allowed);
}

#[test]
fn validate_ask_user_carve_out_subprocess_allows_during_in_progress_auto() {
    // Subprocess test for the integrated carve-out behavior:
    // state file has in_progress + auto, transcript has assistant
    // Skill invocation of flow:flow-abort. The hook would normally
    // block, but the carve-out fires and allows.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    Command::new("git")
        .args(["init", "--initial-branch=test", "."])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "x@y.z"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "X"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&root)
        .output()
        .unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "phases": {"flow-code": {"status": "in_progress"}},
        "skills": {"flow-code": {"continue": "auto"}},
    });
    write_state(&root, "test", &state);

    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-abort\"}}]}}\n";
    let transcript = carve_out_transcript_fixture(&root, jsonl);
    let payload = json!({"transcript_path": transcript.to_string_lossy()});

    let mut child = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["hook", "validate-ask-user"])
        .current_dir(&root)
        .env_remove("FLOW_CI_RUNNING")
        .env("HOME", &root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    // Without the carve-out the in_progress + auto block would
    // exit 2. Verify it exited 0 and stderr is empty.
    assert_eq!(
        output.status.code().unwrap_or(-1),
        0,
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn validate_ask_user_block_persists_when_no_carve_out_during_in_progress_auto() {
    // Subprocess test: state file has in_progress + auto and the
    // transcript has NO Skill tool_use. The hook should still
    // block — carve-out does not fire.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    Command::new("git")
        .args(["init", "--initial-branch=test", "."])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "x@y.z"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "X"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&root)
        .output()
        .unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "phases": {"flow-code": {"status": "in_progress"}},
        "skills": {"flow-code": {"continue": "auto"}},
    });
    write_state(&root, "test", &state);

    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n";
    let transcript = carve_out_transcript_fixture(&root, jsonl);
    let payload = json!({"transcript_path": transcript.to_string_lossy()});

    let mut child = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["hook", "validate-ask-user"])
        .current_dir(&root)
        .env_remove("FLOW_CI_RUNNING")
        .env("HOME", &root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code().unwrap_or(-1), 2);
    assert!(String::from_utf8_lossy(&output.stderr).contains("BLOCKED"));
}

// --- Shared-config carve-out wiring ---
//
// Verifies the second carve-out in `run_impl_main`: when the
// transcript shows a recent shared-config edit block emitted by
// `validate_worktree_paths::validate_shared_config`, the
// AskUserQuestion that the BLOCKED message itself instructs the
// model to call must fire even during in-progress autonomous
// phases. Without the carve-out, the model deadlocks — one hook
// says "ask the user" while another hook blocks AskUserQuestion.
//
// Tests that drive `validate(state_path)` directly (Tests 3 and 4)
// verify pre-existing allow paths are preserved by the new wiring.
// Tests that drive the integrated subprocess (Tests 1, 2, 5, 6, 7,
// 8) verify the carve-out's wiring inside `run_impl_main`, since
// the function is private and can only be exercised through the
// real CLI.

/// Build a minimal git repo + state file pair for subprocess tests.
/// Returns the canonicalized root. The state file lands at
/// `<root>/.flow-states/test/state.json` matching `--initial-branch=test`.
fn shared_config_subprocess_fixture(state: &Value) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    Command::new("git")
        .args(["init", "--initial-branch=test", "."])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "x@y.z"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "X"])
        .current_dir(&root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&root)
        .output()
        .unwrap();
    write_state(&root, "test", state);
    (tmp, root)
}

/// Spawn `bin/flow hook validate-ask-user` with the given stdin
/// payload from the given `root` cwd, returning (exit_code, stdout,
/// stderr). HOME is set to `root` so the transcript-path validator's
/// `<home>/.claude/projects/` prefix check resolves to the same root
/// the transcript fixture is written under.
fn run_validate_ask_user(root: &Path, payload: &Value) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["hook", "validate-ask-user"])
        .current_dir(root)
        .env_remove("FLOW_CI_RUNNING")
        .env("HOME", root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn autonomous_block_still_fires_without_recent_shared_config_block() {
    // Negative regression guard: when the transcript carries no
    // shared-config block AND no user-only Skill call, the
    // autonomous-phase block must still fire.
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "phases": {"flow-code": {"status": "in_progress"}},
        "skills": {"flow-code": {"continue": "auto"}},
    });
    let (_tmp, root) = shared_config_subprocess_fixture(&state);
    // Transcript: an assistant turn that ran some other tool with
    // a non-shared-config success. No carve-out conditions met.
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"do something\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"t1\",\"input\":{\"command\":\"ls\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"file1\\nfile2\\n\",\"is_error\":false}]}}\n";
    let transcript = carve_out_transcript_fixture(&root, jsonl);
    let payload = json!({"transcript_path": transcript.to_string_lossy()});
    let (code, _stdout, stderr) = run_validate_ask_user(&root, &payload);
    assert_eq!(code, 2, "stderr: {}", stderr);
    assert!(stderr.contains("BLOCKED"), "stderr: {}", stderr);
}

#[test]
fn shared_config_carve_out_allows_ask_during_autonomous() {
    // Positive case: in_progress + auto + recent shared-config
    // BLOCKED tool_result. The carve-out fires and the
    // AskUserQuestion is allowed to proceed (exit 0).
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "phases": {"flow-code": {"status": "in_progress"}},
        "skills": {"flow-code": {"continue": "auto"}},
    });
    let (_tmp, root) = shared_config_subprocess_fixture(&state);
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"add a line\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Edit\",\"id\":\"t1\",\"input\":{\"file_path\":\"/p/requirements.txt\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"BLOCKED: requirements.txt is a shared configuration file that affects every engineer in the repository.\",\"is_error\":true}]}}\n";
    let transcript = carve_out_transcript_fixture(&root, jsonl);
    let payload = json!({"transcript_path": transcript.to_string_lossy()});
    let (code, _stdout, stderr) = run_validate_ask_user(&root, &payload);
    assert_eq!(code, 0, "stderr: {}", stderr);
}

#[test]
fn carve_out_does_not_fire_outside_in_progress_phase() {
    // Pre-existing behavior preserved: when the current phase is
    // not in_progress (status = "complete" here, simulating the
    // transition window between phase_complete and phase_enter),
    // validate's block doesn't fire and AskUserQuestion is
    // allowed regardless of any carve-out.
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "phases": {"flow-code": {"status": "complete"}},
        "skills": {"flow-code": {"continue": "auto"}},
    });
    let path = write_state(dir.path(), "test", &state);
    let (allowed, _msg, _resp) = validate(Some(&path));
    assert!(allowed);
}

#[test]
fn carve_out_does_not_fire_in_manual_phase() {
    // Pre-existing behavior preserved: when in_progress but
    // continue is manual, the autonomous-phase block doesn't
    // fire — manual phases don't restrict AskUserQuestion. The
    // carve-out is irrelevant here.
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "phases": {"flow-code": {"status": "in_progress"}},
        "skills": {"flow-code": {"continue": "manual"}},
    });
    let path = write_state(dir.path(), "test", &state);
    let (allowed, _msg, _resp) = validate(Some(&path));
    assert!(allowed);
}

#[test]
fn user_only_skill_carve_out_still_works_alongside() {
    // The existing user-only Skill carve-out must continue to fire
    // when the transcript shows a user-only Skill invocation but
    // no shared-config block. Verifies the new wiring did not
    // break the prior behavior.
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "phases": {"flow-code": {"status": "in_progress"}},
        "skills": {"flow-code": {"continue": "auto"}},
    });
    let (_tmp, root) = shared_config_subprocess_fixture(&state);
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-abort\"}}]}}\n";
    let transcript = carve_out_transcript_fixture(&root, jsonl);
    let payload = json!({"transcript_path": transcript.to_string_lossy()});
    let (code, _stdout, stderr) = run_validate_ask_user(&root, &payload);
    assert_eq!(code, 0, "stderr: {}", stderr);
}

#[test]
fn both_carve_outs_can_apply_user_only_wins_first() {
    // Regression guard for the carve-out ordering inside
    // run_impl_main. When the transcript carries BOTH a recent
    // user-only Skill invocation AND a shared-config tool_result
    // block, both carve-outs return true. The integration must
    // still allow (exit 0) — semantically the order doesn't matter
    // since both produce the same outcome, but the test locks the
    // path so a future refactor that swaps the order or breaks one
    // of the branches still passes.
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "phases": {"flow-code": {"status": "in_progress"}},
        "skills": {"flow-code": {"continue": "auto"}},
    });
    let (_tmp, root) = shared_config_subprocess_fixture(&state);
    // Transcript walking backward: most recent assistant turn fires
    // a user-only Skill call; an earlier turn carries a
    // shared-config BLOCKED tool_result. The user-only walker stops
    // at the most recent assistant turn (finds the user-only
    // Skill, returns true). The shared-config walker stops at the
    // most recent real user turn before any matching block — but
    // there's also a tool_result-wrapped user turn carrying the
    // shared-config substring, so it returns true too.
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"BLOCKED: Cargo.toml is a shared configuration file that affects every engineer in the repository.\",\"is_error\":true}]}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-abort\"}}]}}\n";
    let transcript = carve_out_transcript_fixture(&root, jsonl);
    let payload = json!({"transcript_path": transcript.to_string_lossy()});
    let (code, _stdout, stderr) = run_validate_ask_user(&root, &payload);
    assert_eq!(code, 0, "stderr: {}", stderr);
}

#[test]
fn carve_out_respects_unsafe_transcript_path() {
    // When transcript_path fails is_safe_transcript_path validation
    // (relative path here), neither carve-out fires. The
    // autonomous-phase block stays in effect and the hook exits 2.
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "phases": {"flow-code": {"status": "in_progress"}},
        "skills": {"flow-code": {"continue": "auto"}},
    });
    let (_tmp, root) = shared_config_subprocess_fixture(&state);
    // Relative path — validator rejects (must be absolute under
    // <home>/.claude/projects/).
    let payload = json!({"transcript_path": "relative/path.jsonl"});
    let (code, _stdout, stderr) = run_validate_ask_user(&root, &payload);
    assert_eq!(code, 2, "stderr: {}", stderr);
    assert!(stderr.contains("BLOCKED"), "stderr: {}", stderr);
}

#[test]
fn subprocess_integration_shared_config_carve_out_allows() {
    // Subprocess integration test for the shared-config carve-out's
    // happy path. Sibling to `..._block_persists_no_marker` below.
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "phases": {"flow-code": {"status": "in_progress"}},
        "skills": {"flow-code": {"continue": "auto"}},
    });
    let (_tmp, root) = shared_config_subprocess_fixture(&state);
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Edit\",\"id\":\"t1\",\"input\":{\"file_path\":\"/p/.gitignore\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"BLOCKED: .gitignore is a shared configuration file that affects every engineer in the repository.\",\"is_error\":true}]}}\n";
    let transcript = carve_out_transcript_fixture(&root, jsonl);
    let payload = json!({"transcript_path": transcript.to_string_lossy()});
    let (code, _stdout, stderr) = run_validate_ask_user(&root, &payload);
    assert_eq!(code, 0, "stderr: {}", stderr);
}

#[test]
fn subprocess_integration_shared_config_block_persists_no_marker() {
    // Sibling subprocess test for the negative case: the same
    // setup as the happy path above EXCEPT the transcript's
    // tool_result lacks the shared-config substring. The carve-out
    // does NOT fire and the autonomous-phase block exits 2.
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "phases": {"flow-code": {"status": "in_progress"}},
        "skills": {"flow-code": {"continue": "auto"}},
    });
    let (_tmp, root) = shared_config_subprocess_fixture(&state);
    // Successful Edit — no BLOCKED message, no carve-out.
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Edit\",\"id\":\"t1\",\"input\":{\"file_path\":\"/p/src/foo.rs\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"The file has been updated.\",\"is_error\":false}]}}\n";
    let transcript = carve_out_transcript_fixture(&root, jsonl);
    let payload = json!({"transcript_path": transcript.to_string_lossy()});
    let (code, _stdout, stderr) = run_validate_ask_user(&root, &payload);
    assert_eq!(code, 2, "stderr: {}", stderr);
    assert!(stderr.contains("BLOCKED"), "stderr: {}", stderr);
}
