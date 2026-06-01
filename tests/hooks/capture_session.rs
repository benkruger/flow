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
/// exit. Returns the child's exit code. Delegates to
/// [`crate::common::spawn_hook`], whose universal `env_remove("HOME")`
/// covers the `home_env: None` case (capture-session resolves its
/// capture-file location from `HOME`, so an unset `HOME` exercises the
/// fail-open path); `Some(h)` re-sets `HOME` via the `env` slice. The
/// `cwd` is an inert tempdir because capture-session reads `HOME`, not
/// the working directory.
fn run_capture_session(home_env: Option<&Path>, stdin_bytes: &[u8]) -> Option<i32> {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = home_env.map(|h| h.to_str().expect("HOME path is UTF-8"));
    let env: Vec<(&str, &str)> = match home {
        Some(h) => vec![("HOME", h)],
        None => vec![],
    };
    let output = crate::common::spawn_hook("capture-session", dir.path(), stdin_bytes, &env);
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
    let dir = tempfile::tempdir().expect("tempdir");
    let output = crate::common::spawn_hook(
        "capture-session",
        dir.path(),
        br#"{"session_id":"valid-sid"}"#,
        &[("HOME", "relative-home")],
    );
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

#[test]
fn capture_session_rejects_oversized_session_id() {
    // `is_safe_session_id` caps session id length at SESSION_ID_MAX_LEN
    // (256). A 257-char string should fail validation and the hook
    // should write no capture file. Defends against hostile producers
    // who could otherwise pass arbitrary-length payloads through the
    // alphanumeric character class.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let huge_sid: String = "a".repeat(257);
    let stdin = format!(r#"{{"session_id":"{}"}}"#, huge_sid);
    let code = run_capture_session(Some(&home), stdin.as_bytes());
    assert_eq!(code, Some(0));
    assert!(
        !capture_file(&home).exists(),
        "session_id over 256 bytes must be rejected; capture file must not be written"
    );
}

#[test]
fn capture_session_caps_stdin_payload_size() {
    // STDIN_BYTE_CAP truncates the read at 64 KiB. A multi-megabyte
    // stdin payload that would otherwise produce a multi-megabyte
    // capture file is truncated mid-string, fails JSON parse, and
    // the hook returns without writing. The parent's write_all may
    // raise BrokenPipe once the child closes its read end (cap hit
    // before parent finishes writing) — that is the cap working as
    // designed; tolerate the error and assert on the file outcome.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let huge_sid: String = "a".repeat(5 * 1024 * 1024);
    let stdin = format!(r#"{{"session_id":"{}"}}"#, huge_sid);

    let mut cmd = flow_rs_no_recursion();
    cmd.args(["hook", "capture-session"])
        .env("HOME", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn capture-session");
    // Best-effort write — BrokenPipe is the expected outcome when
    // the cap fires before the parent finishes the multi-MB write.
    let _ = child
        .stdin
        .as_mut()
        .expect("piped stdin")
        .write_all(stdin.as_bytes());
    let output = child.wait_with_output().expect("wait capture-session");
    assert_eq!(output.status.code(), Some(0));

    let path = capture_file(&home);
    if path.exists() {
        let written = fs::metadata(&path).unwrap().len();
        assert!(
            written < 1024 * 1024,
            "stdin cap must bound the written capture file; got {} bytes",
            written
        );
    }
}

#[test]
fn capture_session_overwrites_existing_regular_file() {
    // Symlink-safe gate's `else` arm: when fs::symlink_metadata
    // returns Ok AND the entry is a regular file (not a symlink),
    // remove_file is skipped and fs::write overwrites in place.
    // Exercised on every second-and-later hook invocation against
    // the same HOME.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let path = capture_file(&home);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, br#"{"session_id":"stale","transcript_path":null}"#).unwrap();

    let stdin = r#"{"session_id":"fresh-sid"}"#;
    let code = run_capture_session(Some(&home), stdin.as_bytes());
    assert_eq!(code, Some(0));

    let parsed: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(parsed["session_id"], "fresh-sid");
    let meta = fs::symlink_metadata(&path).unwrap();
    assert!(
        meta.file_type().is_file(),
        "regular file remains a regular file after overwrite"
    );
}

// --- read_captured_session coverage via `current-session-id` subcommand ---
//
// `read_captured_session` is `pub(crate)` so integration tests cannot
// call it directly. The CLI subcommand `current-session-id` calls it
// via `utility_marker::run_current_session_id_main(&home)` and prints
// the resolved session_id (or empty string on None). Driving it
// through the subprocess exercises every branch of read_captured_session
// without exposing the function publicly.

/// Spawn `flow-rs current-session-id` with `HOME=home_env` and
/// return (stdout, exit_code).
fn run_current_session_id(home_env: &str) -> (String, Option<i32>) {
    let mut cmd = flow_rs_no_recursion();
    cmd.args(["current-session-id"])
        .env("HOME", home_env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = cmd.output().expect("spawn current-session-id");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    (stdout, output.status.code())
}

/// `read_captured_session` returns None when `home` is empty (line 67
/// of src/hooks/capture_session.rs). Setting `HOME=""` makes
/// `utility_marker_home()` produce an empty PathBuf.
#[test]
fn current_session_id_returns_empty_when_home_is_empty_string() {
    let (stdout, code) = run_current_session_id("");
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "");
}

/// `read_captured_session` returns None when the capture file does
/// not exist (line 70 of src/hooks/capture_session.rs:
/// `fs::File::open(&path).ok()?`).
#[test]
fn current_session_id_returns_empty_when_capture_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let (stdout, code) = run_current_session_id(home.to_str().unwrap());
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "");
}

/// `read_captured_session` returns Some((sid, None)) when the capture
/// file has a valid session_id but no transcript_path. Exercises the
/// success path through lines 75-87 with the transcript filter
/// dropping the absent field.
#[test]
fn current_session_id_returns_session_id_when_only_session_id_present() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::write(capture_file(&home), br#"{"session_id":"sid-only"}"#).unwrap();
    let (stdout, code) = run_current_session_id(home.to_str().unwrap());
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "sid-only");
}

/// `read_captured_session` returns Some((sid, Some(tp))) when both
/// fields validate. Exercises the success path with the transcript
/// filter accepting a valid path under `<home>/.claude/projects/`.
#[test]
fn current_session_id_returns_session_id_when_both_fields_present() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let projects = home.join(".claude").join("projects").join("p");
    fs::create_dir_all(&projects).unwrap();
    let transcript = projects.join("session.jsonl");
    fs::write(&transcript, b"").unwrap();
    fs::write(
        capture_file(&home),
        format!(
            r#"{{"session_id":"sid-both","transcript_path":"{}"}}"#,
            transcript.display()
        )
        .as_bytes(),
    )
    .unwrap();
    let (stdout, code) = run_current_session_id(home.to_str().unwrap());
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "sid-both");
}

/// `read_captured_session` returns None when the capture file's
/// session_id fails `is_safe_session_id` validation. Exercises the
/// `.filter(...)` drop on line 79.
#[test]
fn current_session_id_returns_empty_when_session_id_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::write(capture_file(&home), br#"{"session_id":"../etc/passwd"}"#).unwrap();
    let (stdout, code) = run_current_session_id(home.to_str().unwrap());
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "");
}

/// `read_captured_session` returns None when the capture file is not
/// valid JSON. Exercises `serde_json::from_str(&content).ok()?` at
/// line 75.
#[test]
fn current_session_id_returns_empty_when_capture_file_malformed_json() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::write(capture_file(&home), b"not valid json").unwrap();
    let (stdout, code) = run_current_session_id(home.to_str().unwrap());
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "");
}

/// `read_captured_session` returns None when the capture file
/// contains non-UTF8 bytes that fail `read_to_string`. Exercises
/// the `?` short-circuit on line 74 of src/hooks/capture_session.rs.
#[test]
fn current_session_id_returns_empty_when_capture_file_not_utf8() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    fs::create_dir_all(home.join(".claude")).unwrap();
    // Invalid UTF-8 byte sequence: 0xFF is not a valid UTF-8 start byte.
    fs::write(capture_file(&home), [0xFF, 0xFE, 0xFD]).unwrap();
    let (stdout, code) = run_current_session_id(home.to_str().unwrap());
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "");
}

/// `read_captured_session` returns Some((sid, None)) when the capture
/// file has a valid session_id but the transcript_path fails
/// `is_safe_transcript_path` (e.g., outside the validated prefix).
/// Exercises the `.filter(...)` drop on line 85 — the session_id is
/// preserved but the transcript_path is dropped to None.
#[test]
fn current_session_id_returns_session_id_when_transcript_path_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::write(
        capture_file(&home),
        br#"{"session_id":"good-sid","transcript_path":"/etc/passwd"}"#,
    )
    .unwrap();
    let (stdout, code) = run_current_session_id(home.to_str().unwrap());
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "good-sid");
}

#[test]
fn capture_session_replaces_existing_symlink_at_capture_path() {
    // Symlink-safe write: a pre-existing symlink at the capture path
    // must NOT be followed (otherwise an attacker who can plant a
    // symlink under ~/.claude/ gains an arbitrary-write primitive).
    // The hook detects the symlink via fs::symlink_metadata and
    // removes it before writing, so the result is a regular file
    // and the symlink's target is untouched.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    fs::create_dir_all(home.join(".claude")).unwrap();

    // Plant a sentinel at a third-party path. The symlink will
    // point at this. After the hook runs, the sentinel must be
    // unchanged — the hook MUST NOT have followed the symlink.
    let sentinel_target = dir.path().join("sentinel.txt");
    fs::write(&sentinel_target, b"untouched").unwrap();
    let sentinel_canon = sentinel_target.canonicalize().unwrap();

    let path = capture_file(&home);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&sentinel_canon, &path).unwrap();

    let stdin = r#"{"session_id":"abc-123"}"#;
    let code = run_capture_session(Some(&home), stdin.as_bytes());
    assert_eq!(code, Some(0));

    // Sentinel must be unchanged — hook MUST NOT have written through the symlink.
    let sentinel_after = fs::read_to_string(&sentinel_canon).unwrap();
    assert_eq!(
        sentinel_after, "untouched",
        "fs::write must not have followed the symlink to overwrite the target"
    );

    // Capture path must now be a regular file (the symlink was removed).
    let meta = fs::symlink_metadata(&path).unwrap();
    assert!(
        meta.file_type().is_file(),
        "capture path must be a regular file after symlink-safe write; got {:?}",
        meta.file_type()
    );
    let parsed: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(parsed["session_id"], "abc-123");
}

/// Asserts that `capture_session::run` persists `transcript_path`
/// verbatim when the JSONL file the path references does not yet
/// exist on disk. SessionStart hooks receive `transcript_path` from
/// Claude Code before the JSONL is created; the structural
/// validator accepts shape-valid paths regardless of file existence
/// so the round-trip through `seed_session_id_from_capture` seeds a
/// real path into the new state file. Downstream
/// `record-agent-return` then has a transcript_path to verify agent
/// invocations against — if `transcript_path` were null at state
/// init, `record-agent-return` would report
/// `transcript_path_invalid`, the failure-classifier would map that
/// to `phase_marker_not_found`, and every Review and Learn agent
/// invocation would silently skip phase-finalize accounting.
///
/// Symlink-escape stays closed at every read-time consumer: the
/// transcript walkers in `src/hooks/transcript_walker.rs` and the
/// `record_agent_return::resolve_transcript_path` callsite
/// re-validate via the canonical wrapper before any `File::open`.
/// Storing a shape-valid path string in the capture file is inert
/// until one of those read-time consumers opens it.
#[test]
fn run_persists_transcript_path_when_jsonl_does_not_exist() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    // Build a transcript_path that is shape-valid (absolute, under
    // ~/.claude/projects/, no `..` components) but the JSONL file
    // does not exist on disk yet. The structural validator must
    // accept the path so the hook persists it; self-healing through
    // canonicalize happens later at read-time hook callsites.
    let projects_dir = home.join(".claude").join("projects").join("-tmp-abc");
    fs::create_dir_all(&projects_dir).unwrap();
    let transcript = projects_dir.join("nonexistent-session.jsonl");
    assert!(!transcript.exists());
    let stdin = format!(
        r#"{{"session_id":"valid-sid","transcript_path":"{}"}}"#,
        transcript.to_string_lossy()
    );
    let code = run_capture_session(Some(&home), stdin.as_bytes());
    assert_eq!(code, Some(0));
    let path = capture_file(&home);
    let parsed: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(parsed["session_id"], "valid-sid");
    assert_eq!(
        parsed["transcript_path"].as_str(),
        Some(transcript.to_string_lossy().as_ref()),
        "structural-validator-accepted path must be persisted verbatim; got: {}",
        parsed["transcript_path"]
    );
}

// --- refresh_active_flow_session ---
//
// When the Claude Code session rotates mid-flow (startup|resume|clear|
// compact), the SessionStart hook receives the new session_id and
// transcript_path. Without refreshing the active flow's state file,
// `record-agent-return` keeps reading the flow-start session's
// transcript and reports `phase_marker_not_found` for every Review and
// Learn agent. `capture_session::run` therefore refreshes the active
// flow's `session_id` AND `transcript_path` from the same payload,
// keyed by branch-from-cwd, fail-open. These tests drive the hook
// through the compiled binary and assert the state file update.

/// Build a refresh fixture: a main-root tempdir with a worktree at
/// `.worktrees/<branch>/` (carrying a `.git` marker file so
/// `detect_branch_from_path` resolves the branch from the path alone,
/// not a git subprocess) and a state file at
/// `.flow-states/<branch>/state.json` carrying `state_json`. The
/// tempdir is canonicalized so the child binary's cwd-derived paths
/// match the test's constructed paths on macOS (per
/// `.claude/rules/testing-gotchas.md` "macOS Subprocess Path
/// Canonicalization"). Returns `(TempDir, main_root, worktree_cwd,
/// state_path)`; the `TempDir` keeps the directory alive for the test
/// body's lifetime.
fn setup_refresh_fixture(
    branch: &str,
    state_json: &str,
) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let main_root = dir.path().canonicalize().unwrap();
    let worktree = main_root.join(".worktrees").join(branch);
    fs::create_dir_all(&worktree).unwrap();
    fs::write(worktree.join(".git"), "gitdir: /dev/null\n").unwrap();
    let state_dir = main_root.join(".flow-states").join(branch);
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("state.json");
    fs::write(&state_path, state_json).unwrap();
    (dir, main_root, worktree, state_path)
}

/// Construct a transcript path under `<home>/.claude/projects/<proj>/`
/// so it passes the structural validator. Creates the projects
/// directory; the JSONL file itself need not exist (SessionStart
/// delivers the path before Claude Code creates the file).
fn make_transcript_path(home: &Path, proj: &str, name: &str) -> PathBuf {
    let dir = home.join(".claude").join("projects").join(proj);
    fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn read_state(state_path: &Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(state_path).unwrap()).unwrap()
}

#[test]
fn refresh_updates_session_id_and_transcript_path_when_flow_active() {
    let (_dir, main_root, worktree, state_path) = setup_refresh_fixture(
        "feat",
        r#"{"session_id":"old-sid","transcript_path":"/old/path.jsonl"}"#,
    );
    let new_transcript = make_transcript_path(&main_root, "proj", "new-session.jsonl");
    let stdin = format!(
        r#"{{"session_id":"new-sid","transcript_path":"{}","cwd":"{}"}}"#,
        new_transcript.display(),
        worktree.display()
    );
    let output = crate::common::spawn_hook(
        "capture-session",
        &worktree,
        stdin.as_bytes(),
        &[("HOME", main_root.to_str().unwrap())],
    );
    assert_eq!(output.status.code(), Some(0));

    let state = read_state(&state_path);
    assert_eq!(state["session_id"], "new-sid");
    assert_eq!(
        state["transcript_path"].as_str(),
        Some(new_transcript.to_string_lossy().as_ref()),
        "active flow's transcript_path must be refreshed; got {}",
        state["transcript_path"]
    );
}

#[test]
fn refresh_writes_null_transcript_path_when_payload_transcript_invalid() {
    // An invalid transcript_path (outside ~/.claude/projects/) is
    // dropped to None by run()'s structural validator; the refresh
    // overwrites BOTH fields together so transcript_path becomes null
    // rather than retaining the stale value.
    let (_dir, main_root, worktree, state_path) = setup_refresh_fixture(
        "feat",
        r#"{"session_id":"old-sid","transcript_path":"/old/path.jsonl"}"#,
    );
    let stdin = format!(
        r#"{{"session_id":"new-sid","transcript_path":"/etc/passwd","cwd":"{}"}}"#,
        worktree.display()
    );
    let output = crate::common::spawn_hook(
        "capture-session",
        &worktree,
        stdin.as_bytes(),
        &[("HOME", main_root.to_str().unwrap())],
    );
    assert_eq!(output.status.code(), Some(0));

    let state = read_state(&state_path);
    assert_eq!(state["session_id"], "new-sid");
    assert!(
        state["transcript_path"].is_null(),
        "invalid transcript_path must be written as null; got {}",
        state["transcript_path"]
    );
}

#[test]
fn refresh_no_op_when_cwd_missing() {
    // Payload omits `cwd`; the spawn cwd (main_root) is not inside a
    // worktree, so branch detection yields None and the refresh is a
    // no-op. The stale state is preserved.
    let (_dir, main_root, _worktree, state_path) = setup_refresh_fixture(
        "feat",
        r#"{"session_id":"old-sid","transcript_path":"/old/path.jsonl"}"#,
    );
    let stdin = r#"{"session_id":"new-sid"}"#;
    let output = crate::common::spawn_hook(
        "capture-session",
        &main_root,
        stdin.as_bytes(),
        &[("HOME", main_root.to_str().unwrap())],
    );
    assert_eq!(output.status.code(), Some(0));

    let state = read_state(&state_path);
    assert_eq!(
        state["session_id"], "old-sid",
        "no cwd → branch undetectable → state must be unchanged"
    );
}

#[test]
fn refresh_no_op_when_branch_has_slash() {
    // A `.git` marker nested two levels deep under .worktrees/ makes
    // detect_branch_from_path resolve a slash-containing branch
    // (`a/b`), which FlowPaths::try_new rejects, so is_flow_active is
    // false and the refresh is a no-op.
    let dir = tempfile::tempdir().unwrap();
    let main_root = dir.path().canonicalize().unwrap();
    let nested = main_root.join(".worktrees").join("a").join("b");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join(".git"), "gitdir: /dev/null\n").unwrap();
    // A state file at the slash-branch path is unreachable via
    // FlowPaths; create one anyway to prove it is never touched.
    let state_dir = main_root.join(".flow-states").join("a").join("b");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("state.json");
    fs::write(&state_path, r#"{"session_id":"old-sid"}"#).unwrap();

    let stdin = format!(r#"{{"session_id":"new-sid","cwd":"{}"}}"#, nested.display());
    let output = crate::common::spawn_hook(
        "capture-session",
        &nested,
        stdin.as_bytes(),
        &[("HOME", main_root.to_str().unwrap())],
    );
    assert_eq!(output.status.code(), Some(0));

    let state = read_state(&state_path);
    assert_eq!(
        state["session_id"], "old-sid",
        "slash branch → no active flow → state must be unchanged"
    );
}

#[test]
fn refresh_no_op_when_no_active_flow() {
    // A worktree exists but no state file → is_flow_active is false →
    // the refresh is a no-op and no state file is created.
    let dir = tempfile::tempdir().unwrap();
    let main_root = dir.path().canonicalize().unwrap();
    let worktree = main_root.join(".worktrees").join("feat");
    fs::create_dir_all(&worktree).unwrap();
    fs::write(worktree.join(".git"), "gitdir: /dev/null\n").unwrap();
    let state_path = main_root
        .join(".flow-states")
        .join("feat")
        .join("state.json");

    let stdin = format!(
        r#"{{"session_id":"new-sid","cwd":"{}"}}"#,
        worktree.display()
    );
    let output = crate::common::spawn_hook(
        "capture-session",
        &worktree,
        stdin.as_bytes(),
        &[("HOME", main_root.to_str().unwrap())],
    );
    assert_eq!(output.status.code(), Some(0));
    assert!(
        !state_path.exists(),
        "no active flow → refresh must not create a state file"
    );
}

#[test]
fn refresh_no_op_when_state_file_is_array() {
    // A wrong-root-type state file (JSON array) hits the object guard
    // inside the mutate_state closure: the closure returns without
    // mutating, so the array is written back unchanged. Defends
    // against a corrupted or hand-edited state file panicking the
    // hook on string-key IndexMut.
    let (_dir, main_root, worktree, state_path) = setup_refresh_fixture("feat", "[]");
    let stdin = format!(
        r#"{{"session_id":"new-sid","cwd":"{}"}}"#,
        worktree.display()
    );
    let output = crate::common::spawn_hook(
        "capture-session",
        &worktree,
        stdin.as_bytes(),
        &[("HOME", main_root.to_str().unwrap())],
    );
    assert_eq!(output.status.code(), Some(0));

    let state = read_state(&state_path);
    assert!(
        state.is_array() && state.as_array().unwrap().is_empty(),
        "array-root state must be preserved unchanged; got {}",
        state
    );
}

#[test]
fn refresh_only_touches_cwd_branch() {
    // Two active flows under the same main root; the cwd points at
    // branch A's worktree. Only A's state is refreshed; B's stays
    // stale, proving the refresh is scoped to the cwd's branch.
    let (_dir, main_root, worktree_a, state_a) = setup_refresh_fixture(
        "feat-a",
        r#"{"session_id":"old-a","transcript_path":"/old/a.jsonl"}"#,
    );
    // Add a second flow under the same main root.
    let worktree_b = main_root.join(".worktrees").join("feat-b");
    fs::create_dir_all(&worktree_b).unwrap();
    fs::write(worktree_b.join(".git"), "gitdir: /dev/null\n").unwrap();
    let state_b_dir = main_root.join(".flow-states").join("feat-b");
    fs::create_dir_all(&state_b_dir).unwrap();
    let state_b = state_b_dir.join("state.json");
    fs::write(
        &state_b,
        r#"{"session_id":"old-b","transcript_path":"/old/b.jsonl"}"#,
    )
    .unwrap();

    let stdin = format!(
        r#"{{"session_id":"new-a","cwd":"{}"}}"#,
        worktree_a.display()
    );
    let output = crate::common::spawn_hook(
        "capture-session",
        &worktree_a,
        stdin.as_bytes(),
        &[("HOME", main_root.to_str().unwrap())],
    );
    assert_eq!(output.status.code(), Some(0));

    assert_eq!(read_state(&state_a)["session_id"], "new-a");
    assert_eq!(
        read_state(&state_b)["session_id"],
        "old-b",
        "sibling flow's state must be untouched"
    );
}

/// End-to-end contract for the session-rotation refresh: a mid-flow
/// session rotation (capture-session) must refresh the active flow's
/// transcript pointer so the downstream `record-agent-return`
/// verifier reads the LIVE transcript and confirms the agent. Without
/// the refresh, `record-agent-return` reads the stale flow-start
/// transcript, `verify_agent_returned_in_phase` finds no agent
/// invocation, and the verifier reports failure — the Review/Learn
/// deadlock this fix resolves.
///
/// The fixture uses a REAL linked git worktree so `project_root()`
/// (`git worktree list --porcelain`) resolves the worktree cwd back to
/// the main repo where `.flow-states/` lives, matching production.
#[test]
fn e2e_session_rotation_refresh_lets_record_agent_return_pass() {
    let dir = tempfile::tempdir().unwrap();
    let main_repo = crate::common::create_git_repo_with_remote(dir.path());
    let main_repo = main_repo.canonicalize().unwrap();
    let branch = "feat";
    let worktree = main_repo.join(".worktrees").join(branch);
    let add = Command::new("git")
        .args(["worktree", "add", worktree.to_str().unwrap(), "-b", branch])
        .current_dir(&main_repo)
        .output()
        .unwrap();
    assert!(
        add.status.success(),
        "git worktree add failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );

    let home = dir.path().canonicalize().unwrap();

    // The flow-start transcript: shape-valid and present, but carries
    // no agent invocation — the verifier must fail against it.
    let stale = make_transcript_path(&home, "stale", "stale-session.jsonl");
    fs::write(
        &stale,
        b"{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    )
    .unwrap();

    // Active flow state pointing at the stale transcript.
    let state_dir = main_repo.join(".flow-states").join(branch);
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("state.json");
    let state = serde_json::json!({
        "branch": branch,
        "session_id": "stale-session",
        "transcript_path": stale.to_string_lossy(),
        "phases": {"flow-review": {"status": "in_progress"}},
    });
    fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();

    // The rotated (live) transcript carries the phase-enter marker and
    // the reviewer Agent tool_use/tool_result pair.
    let live = make_transcript_path(&home, "live", "live-session.jsonl");
    let happy = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_b1\",\"input\":{\"command\":\"bin/flow phase-enter --phase flow-review\"}}]}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"toolu_a1\",\"input\":{\"subagent_type\":\"flow:reviewer\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_a1\",\"content\":\"findings\"}]}}\n";
    fs::write(&live, happy).unwrap();

    let run_rar = |worktree: &Path, home: &Path| -> serde_json::Value {
        let out = flow_rs_no_recursion()
            .args([
                "record-agent-return",
                "--branch",
                branch,
                "--agent",
                "reviewer",
                "--phase",
                "flow-review",
            ])
            .current_dir(worktree)
            .env("HOME", home)
            .env("GH_TOKEN", "invalid")
            .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        serde_json::from_str(stdout.trim().lines().last().unwrap_or(""))
            .unwrap_or(serde_json::Value::Null)
    };

    // Before rotation: the verifier reads the stale transcript and
    // fails — proving the test exercises the real verification path.
    let before = run_rar(&worktree, &home);
    assert_eq!(
        before["status"], "error",
        "stale transcript must fail verification; got {}",
        before
    );

    // Rotate the session: SessionStart fires with the live session's
    // id, transcript, and the worktree cwd.
    let stdin = format!(
        r#"{{"session_id":"live-session","transcript_path":"{}","cwd":"{}"}}"#,
        live.display(),
        worktree.display()
    );
    let cap = crate::common::spawn_hook(
        "capture-session",
        &worktree,
        stdin.as_bytes(),
        &[("HOME", home.to_str().unwrap())],
    );
    assert_eq!(cap.status.code(), Some(0));

    // After rotation: the verifier reads the refreshed (live)
    // transcript and confirms the agent.
    let after = run_rar(&worktree, &home);
    assert_eq!(
        after["status"], "ok",
        "refreshed transcript must verify; got {}",
        after
    );
}
