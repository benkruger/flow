//! Integration tests for `src/hooks/validate_ask_user.rs`.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use flow_rs::hooks::validate_ask_user::{set_blocked, validate};
use serde_json::{json, Value};

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
        "_auto_continue": "/flow:flow-plan",
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
        .contains("/flow:flow-plan"));
}

#[test]
fn test_validate_auto_continue_includes_command() {
    let dir = tempfile::tempdir().unwrap();
    let state = json!({
        "current_phase": "flow-code",
        "branch": "test",
        "_auto_continue": "/flow:flow-code-review",
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
        .contains("/flow:flow-code-review"));
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
        "_auto_continue": "/flow:flow-code-review",
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
        "_auto_continue": "/flow:flow-code-review",
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
        "current_phase": "flow-code-review",
        "branch": "test",
        "skills": {"flow-code-review": {"continue": "auto"}},
        "phases": {
            "flow-code": {"status": "complete"},
            "flow-code-review": {"status": "pending"},
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
        "_auto_continue": "/flow:flow-code-review",
    });
    write_state(&root, "test", &state);

    let (code, stdout, _stderr) = run_hook(&root, "{}");
    assert_eq!(code, 0);
    assert!(stdout.contains("permissionDecision"), "stdout: {}", stdout);
    assert!(
        stdout.contains("/flow:flow-code-review"),
        "stdout: {}",
        stdout
    );
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
