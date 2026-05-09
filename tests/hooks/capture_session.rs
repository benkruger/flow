//! Tests for `src/hooks/capture_session.rs`.
//!
//! Drive the SessionStart hook through the compiled binary so the
//! production stdin parse + path resolution + write path is exercised
//! end-to-end. Each test sets `HOME=<tempdir>` to scope the capture
//! file location (per `.claude/rules/external-input-path-construction.md`
//! "Validate env-var-derived paths as absolute") and uses
//! `env_remove("FLOW_CI_RUNNING")` so the child inherits a fresh
//! recursion guard.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn flow_rs_no_recursion() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd
}

fn capture_file(home: &Path) -> PathBuf {
    home.join(".claude").join("flow-current-session.json")
}

/// Spawn `flow-rs hook capture-session`, pipe `stdin_bytes`, wait for
/// exit. Returns the child's exit code. Wraps the verbose stdin/stdout
/// boilerplate so individual tests stay focused on assertions.
fn run_capture_session(home_env: Option<&Path>, stdin_bytes: &[u8]) -> Option<i32> {
    let mut cmd = flow_rs_no_recursion();
    cmd.args(["hook", "capture-session"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match home_env {
        Some(h) => {
            cmd.env("HOME", h);
        }
        None => {
            cmd.env_remove("HOME");
        }
    }
    let mut child = cmd.spawn().expect("spawn capture-session");
    child
        .stdin
        .as_mut()
        .expect("piped stdin")
        .write_all(stdin_bytes)
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait capture-session");
    output.status.code()
}

#[test]
fn capture_session_writes_file_when_session_id_present() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let projects = home.join(".claude").join("projects").join("proj");
    fs::create_dir_all(&projects).unwrap();
    let transcript = projects.join("session.jsonl");
    fs::write(&transcript, "").unwrap();

    let stdin = format!(
        r#"{{"session_id":"abc-123","transcript_path":"{}"}}"#,
        transcript.display()
    );
    let code = run_capture_session(Some(&home), stdin.as_bytes());
    assert_eq!(code, Some(0));

    let path = capture_file(&home);
    assert!(path.exists(), "capture file must be written");
    let parsed: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(parsed["session_id"], "abc-123");
    assert_eq!(parsed["transcript_path"], transcript.display().to_string());
}

#[test]
fn capture_session_skips_when_session_id_missing() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let stdin = r#"{}"#;
    let code = run_capture_session(Some(&home), stdin.as_bytes());
    assert_eq!(code, Some(0));
    assert!(
        !capture_file(&home).exists(),
        "no file when session_id absent"
    );
}

#[test]
fn capture_session_rejects_invalid_session_id() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    // Slash → fails is_safe_session_id.
    let stdin = r#"{"session_id":"../etc/passwd"}"#;
    let code = run_capture_session(Some(&home), stdin.as_bytes());
    assert_eq!(code, Some(0));
    assert!(
        !capture_file(&home).exists(),
        "no file when session_id is path-traversal-shaped"
    );
}

#[test]
fn capture_session_omits_invalid_transcript_path() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    // session_id valid; transcript_path outside ~/.claude/projects/.
    let stdin = r#"{"session_id":"valid-sid","transcript_path":"/etc/passwd"}"#;
    let code = run_capture_session(Some(&home), stdin.as_bytes());
    assert_eq!(code, Some(0));
    let path = capture_file(&home);
    assert!(path.exists());
    let parsed: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(parsed["session_id"], "valid-sid");
    assert!(
        parsed["transcript_path"].is_null(),
        "invalid transcript_path must be stored as null; got: {}",
        parsed["transcript_path"]
    );
}

#[test]
fn capture_session_skips_when_home_empty() {
    let stdin = r#"{"session_id":"valid-sid"}"#;
    // No HOME set → hook fails open, returns 0, writes nothing.
    let code = run_capture_session(None, stdin.as_bytes());
    assert_eq!(code, Some(0));
}

#[test]
fn capture_session_skips_when_home_is_relative() {
    // HOME=relative-path triggers the `!is_absolute()` arm of the
    // empty-or-relative gate. The capture file is never written
    // because joining a relative HOME with `.claude/...` would
    // resolve against the worktree's cwd — exactly the
    // hostile-config trap `.claude/rules/external-input-path-construction.md`
    // "Validate env-var-derived paths as absolute" defends against.
    let mut cmd = flow_rs_no_recursion();
    cmd.args(["hook", "capture-session"])
        .env("HOME", "relative-home")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn capture-session");
    child
        .stdin
        .as_mut()
        .expect("piped stdin")
        .write_all(br#"{"session_id":"valid-sid"}"#)
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait capture-session");
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn capture_session_handles_unparseable_stdin() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let stdin = b"not valid JSON";
    let code = run_capture_session(Some(&home), stdin);
    assert_eq!(code, Some(0));
    assert!(
        !capture_file(&home).exists(),
        "no file when stdin is unparseable"
    );
}
