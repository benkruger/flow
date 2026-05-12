//! Tests for `src/commands/utility_marker.rs`.
//!
//! Exercises the marker-file lifecycle for multi-step utility skills:
//! `write_marker` creates the per-session marker under
//! `<home>/.claude/flow/utility-in-progress-<session_id>.json`, and
//! `clear_marker` removes it idempotently. Both helpers validate
//! `skill` and `session_id` per `.claude/rules/external-input-validation.md`
//! and `.claude/rules/external-input-path-construction.md` so a hostile
//! or corrupted state-file value cannot escape the canonical directory.

use std::fs;
use std::path::{Path, PathBuf};

use flow_rs::commands::utility_marker::{
    clear_marker, is_safe_skill_name, marker_path, run_clear_main, run_current_session_id_main,
    run_set_main, write_marker,
};

const TEST_SKILL: &str = "flow:flow-explore";
const TEST_SESSION: &str = "abc12345";

// --- write_marker ---

#[test]
fn set_utility_in_progress_writes_marker_with_session_id() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let path = write_marker(&home, TEST_SKILL, TEST_SESSION).expect("write_marker ok");
    assert!(path.exists(), "marker file must exist after write_marker");

    // Path layout: <home>/.claude/flow/utility-in-progress-<session>.json
    let expected = home
        .join(".claude")
        .join("flow")
        .join(format!("utility-in-progress-{}.json", TEST_SESSION));
    assert_eq!(path, expected);

    let content = fs::read_to_string(&path).expect("read marker");
    let json: serde_json::Value = serde_json::from_str(&content).expect("parse marker JSON");
    assert_eq!(json["skill"], TEST_SKILL);
    assert_eq!(json["session_id"], TEST_SESSION);
    assert!(json["started_at"].is_string(), "started_at must be present");
}

#[test]
fn set_utility_in_progress_creates_directory_if_missing() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let claude_flow = home.join(".claude").join("flow");
    assert!(!claude_flow.exists(), "directory must be missing pre-write");
    write_marker(&home, TEST_SKILL, TEST_SESSION).expect("write_marker ok");
    assert!(
        claude_flow.is_dir(),
        "write_marker must create .claude/flow"
    );
}

// --- clear_marker ---

#[test]
fn clear_utility_in_progress_removes_marker() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let path = write_marker(&home, TEST_SKILL, TEST_SESSION).expect("write_marker ok");
    assert!(path.exists());
    let removed = clear_marker(&home, TEST_SKILL, TEST_SESSION).expect("clear_marker ok");
    assert!(
        removed,
        "clear_marker must report removal when file existed"
    );
    assert!(
        !path.exists(),
        "marker file must be gone after clear_marker"
    );
}

#[test]
fn clear_utility_in_progress_is_idempotent_when_marker_absent() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let removed = clear_marker(&home, TEST_SKILL, TEST_SESSION).expect("clear_marker ok");
    assert!(
        !removed,
        "clear_marker on missing file must report not-removed (no error)"
    );
}

// --- skill validation ---

#[test]
fn set_utility_in_progress_validates_skill_name() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();

    // Empty
    assert!(
        write_marker(&home, "", TEST_SESSION).is_err(),
        "empty skill must reject"
    );
    // Path traversal
    assert!(
        write_marker(&home, "../etc/passwd", TEST_SESSION).is_err(),
        "traversal in skill name must reject"
    );
    // Slash
    assert!(
        write_marker(&home, "flow/create-issue", TEST_SESSION).is_err(),
        "slash in skill name must reject"
    );
    // NUL byte
    assert!(
        write_marker(&home, "flow:flow\0create-issue", TEST_SESSION).is_err(),
        "NUL in skill name must reject"
    );
    // Backslash
    assert!(
        write_marker(&home, "flow:flow\\create", TEST_SESSION).is_err(),
        "backslash in skill name must reject"
    );
}

// --- session_id validation ---

#[test]
fn set_utility_in_progress_validates_session_id() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();

    // Empty
    assert!(
        write_marker(&home, TEST_SKILL, "").is_err(),
        "empty session_id must reject"
    );
    // Dot / parent
    assert!(
        write_marker(&home, TEST_SKILL, ".").is_err(),
        "dot session_id must reject"
    );
    assert!(
        write_marker(&home, TEST_SKILL, "..").is_err(),
        "parent session_id must reject"
    );
    // Slash
    assert!(
        write_marker(&home, TEST_SKILL, "abc/def").is_err(),
        "slash in session_id must reject"
    );
    // NUL byte
    assert!(
        write_marker(&home, TEST_SKILL, "abc\0def").is_err(),
        "NUL in session_id must reject"
    );
}

// --- marker_path ---

#[test]
fn marker_path_returns_some_for_valid_session_id() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let path = marker_path(&home, TEST_SESSION).expect("valid session_id must produce a path");
    let expected = home
        .join(".claude")
        .join("flow")
        .join(format!("utility-in-progress-{}.json", TEST_SESSION));
    assert_eq!(path, expected);
}

#[test]
fn marker_path_returns_none_for_invalid_session_id() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    assert!(
        marker_path(&home, "..").is_none(),
        "marker_path must reject `..`"
    );
    assert!(
        marker_path(&home, "abc/def").is_none(),
        "marker_path must reject slash"
    );
    assert!(
        marker_path(&home, "").is_none(),
        "marker_path must reject empty"
    );
}

#[test]
fn marker_path_rejects_empty_home() {
    // An empty PathBuf passed as home — a hostile env where $HOME is
    // unset and home_dir_or_empty returned "" — must produce None
    // rather than a cwd-relative path that would silently miss or
    // spuriously hit unrelated files.
    assert!(
        marker_path(&PathBuf::new(), TEST_SESSION).is_none(),
        "marker_path must reject empty home"
    );
}

#[test]
fn marker_path_rejects_relative_home() {
    // A relative path as home — env-var-derived value that wasn't
    // absolute — must produce None per
    // .claude/rules/external-input-path-construction.md rule 5.
    assert!(
        marker_path(Path::new("relative/home"), TEST_SESSION).is_none(),
        "marker_path must reject relative home"
    );
    assert!(
        marker_path(Path::new("./home"), TEST_SESSION).is_none(),
        "marker_path must reject dot-prefixed relative home"
    );
}

#[test]
fn write_marker_returns_err_when_home_is_invalid() {
    // The same absolute-home guard that marker_path enforces must
    // also surface as an error from write_marker.
    let result = write_marker(&PathBuf::new(), TEST_SKILL, TEST_SESSION);
    assert!(result.is_err(), "write_marker must reject empty home");
    let result = write_marker(Path::new("relative/home"), TEST_SKILL, TEST_SESSION);
    assert!(result.is_err(), "write_marker must reject relative home");
}

#[test]
fn write_marker_overwrites_pre_existing_regular_file_at_marker_path() {
    // The symlink-safety check inspects symlink_metadata; if the
    // existing entry is a regular file (idempotent re-write), the
    // write proceeds in place. This exercises the "meta exists AND
    // is a regular file" branch of the symlink check.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let marker_dir = home.join(".claude").join("flow");
    fs::create_dir_all(&marker_dir).unwrap();
    let marker = marker_dir.join(format!("utility-in-progress-{}.json", TEST_SESSION));
    fs::write(&marker, b"stale-content-from-prior-run").unwrap();

    let path = write_marker(&home, TEST_SKILL, TEST_SESSION).expect("write_marker ok");
    assert_eq!(path, marker);
    let content = fs::read_to_string(&marker).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(json["session_id"], TEST_SESSION);
}

#[test]
fn write_marker_replaces_pre_existing_symlink_at_marker_path() {
    use std::os::unix::fs::symlink;
    let home_dir = tempfile::tempdir().unwrap();
    let escape_dir = tempfile::tempdir().unwrap();
    let home = home_dir.path().canonicalize().unwrap();
    let escape_target = escape_dir.path().canonicalize().unwrap().join("victim");
    fs::write(&escape_target, b"sentinel-content").unwrap();

    // Pre-create the marker dir + a symlink at the marker path that
    // points outside home. write_marker must NOT follow the symlink
    // and overwrite escape_target — instead it must replace the
    // symlink with a fresh regular file at the marker path.
    let marker_dir = home.join(".claude").join("flow");
    fs::create_dir_all(&marker_dir).unwrap();
    let marker = marker_dir.join(format!("utility-in-progress-{}.json", TEST_SESSION));
    symlink(&escape_target, &marker).unwrap();

    let path = write_marker(&home, TEST_SKILL, TEST_SESSION).expect("write_marker ok");
    assert_eq!(path, marker, "marker written at canonical path");
    let escape_after = fs::read(&escape_target).unwrap();
    assert_eq!(
        escape_after, b"sentinel-content",
        "symlink target outside HOME must be untouched"
    );
    let marker_meta = fs::symlink_metadata(&marker).unwrap();
    assert!(
        marker_meta.file_type().is_file(),
        "marker is now a regular file, not a symlink"
    );
    let content = fs::read_to_string(&marker).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(json["session_id"], TEST_SESSION);
}

// --- is_safe_skill_name ---

#[test]
fn is_safe_skill_name_accepts_canonical_flow_skill() {
    assert!(is_safe_skill_name("flow:flow-explore"));
    assert!(is_safe_skill_name("flow:flow-start"));
    assert!(is_safe_skill_name("a"));
    assert!(is_safe_skill_name("a_b-c:d"));
}

#[test]
fn is_safe_skill_name_rejects_malformed() {
    assert!(!is_safe_skill_name(""), "empty rejects");
    assert!(!is_safe_skill_name("."), "dot rejects");
    assert!(!is_safe_skill_name(".."), "parent rejects");
    assert!(!is_safe_skill_name("flow/foo"), "slash rejects");
    assert!(!is_safe_skill_name("flow\\foo"), "backslash rejects");
    assert!(!is_safe_skill_name("flow\0foo"), "NUL rejects");
    assert!(!is_safe_skill_name("a b"), "space rejects");
    assert!(
        !is_safe_skill_name(&"a".repeat(65)),
        "over 64 chars rejects"
    );
}

// --- run_set_main / run_clear_main ---

#[test]
fn run_set_main_returns_ok_envelope_on_success() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let (value, code) = run_set_main(&home, TEST_SKILL, Some(TEST_SESSION), None);
    assert_eq!(code, 0, "exit code must be 0 (business outcome via JSON)");
    assert_eq!(value["status"], "ok");
    assert!(value["path"].is_string());
    let path_str = value["path"].as_str().unwrap();
    assert!(
        path_str.ends_with(&format!("utility-in-progress-{}.json", TEST_SESSION)),
        "path must reference the canonical marker filename"
    );
}

#[test]
fn run_set_main_returns_error_envelope_on_invalid_skill() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let (value, code) = run_set_main(&home, "../bad", Some(TEST_SESSION), None);
    assert_eq!(code, 0, "business errors stay at exit 0");
    assert_eq!(value["status"], "error");
    assert!(
        value["message"]
            .as_str()
            .unwrap_or("")
            .contains("invalid skill"),
        "error message must name the invalid skill"
    );
}

#[test]
fn run_set_main_returns_error_envelope_when_no_session_id_available() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    // No --session-id passed, no env value, AND no capture file at
    // <home>/.claude/flow-current-session.json — exercises the
    // resolve_session_id_from None-fallthrough arm through the
    // run_set_main wrapper.
    let (value, code) = run_set_main(&home, TEST_SKILL, None, None);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert!(
        value["message"]
            .as_str()
            .unwrap_or("")
            .contains("no session_id available"),
        "error must name the missing session_id condition"
    );
}

#[test]
fn run_set_main_falls_back_to_capture_file_when_session_id_omitted() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    // Seed the SessionStart capture file with a known session_id.
    let claude = home.join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let capture = claude.join("flow-current-session.json");
    fs::write(&capture, format!(r#"{{"session_id": "{}"}}"#, TEST_SESSION)).unwrap();

    let (value, code) = run_set_main(&home, TEST_SKILL, None, None);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    let path_str = value["path"].as_str().unwrap();
    assert!(
        path_str.ends_with(&format!("utility-in-progress-{}.json", TEST_SESSION)),
        "fallback must use the captured session_id"
    );
}

#[test]
fn run_set_main_treats_empty_explicit_session_id_as_omitted() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    // Seed the capture file so the fallback resolves to a value.
    let claude = home.join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let capture = claude.join("flow-current-session.json");
    fs::write(&capture, format!(r#"{{"session_id": "{}"}}"#, TEST_SESSION)).unwrap();
    // Passing Some("") (e.g., a clap flag with empty value) must fall
    // through to the capture-file branch — not write a marker keyed
    // by the empty string.
    let (value, code) = run_set_main(&home, TEST_SKILL, Some(""), None);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    let path_str = value["path"].as_str().unwrap();
    assert!(
        path_str.ends_with(&format!("utility-in-progress-{}.json", TEST_SESSION)),
        "Some(\"\") must be treated as omitted and use the capture file"
    );
}

#[test]
fn run_clear_main_returns_ok_envelope_when_marker_exists() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    write_marker(&home, TEST_SKILL, TEST_SESSION).expect("write_marker ok");
    let (value, code) = run_clear_main(&home, TEST_SKILL, Some(TEST_SESSION), None);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["removed"], true);
}

#[test]
fn run_clear_main_returns_ok_envelope_when_marker_absent() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let (value, code) = run_clear_main(&home, TEST_SKILL, Some(TEST_SESSION), None);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["removed"], false, "absent marker reports not-removed");
}

#[test]
fn run_clear_main_returns_error_envelope_on_invalid_session_id() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let (value, code) = run_clear_main(&home, TEST_SKILL, Some(".."), None);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert!(value["message"]
        .as_str()
        .unwrap_or("")
        .contains("invalid session_id"));
}

#[test]
fn run_clear_main_returns_error_envelope_when_no_session_id_available() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let (value, code) = run_clear_main(&home, TEST_SKILL, None, None);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert!(value["message"]
        .as_str()
        .unwrap_or("")
        .contains("no session_id available"));
}

#[test]
fn run_clear_main_falls_back_to_capture_file_when_session_id_omitted() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let claude = home.join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let capture = claude.join("flow-current-session.json");
    fs::write(&capture, format!(r#"{{"session_id": "{}"}}"#, TEST_SESSION)).unwrap();
    // Pre-populate the marker so the clear hits the Ok(true) branch.
    write_marker(&home, TEST_SKILL, TEST_SESSION).expect("seed marker");

    let (value, code) = run_clear_main(&home, TEST_SKILL, None, None);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["removed"], true);
}

#[test]
fn run_set_main_uses_env_value_when_session_id_absent() {
    // Production CLI boundary reads CLAUDE_CODE_SESSION_ID and
    // forwards it; this test exercises the env-arm of run_set_main
    // by passing env_value as a parameter — covers the no-explicit,
    // valid-env path through the wrapper.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let (value, code) = run_set_main(&home, TEST_SKILL, None, Some("env-session-xyz"));
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    let path_str = value["path"].as_str().unwrap();
    assert!(
        path_str.ends_with("utility-in-progress-env-session-xyz.json"),
        "env value must reach marker_path when explicit is None"
    );
}

#[test]
fn run_clear_main_uses_env_value_when_session_id_absent() {
    // Mirror of run_set_main_uses_env_value: covers the env-arm of
    // run_clear_main so both wrappers' (None, Some(env)) precedence
    // path is regression-protected.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    write_marker(&home, TEST_SKILL, "env-session-xyz").expect("seed marker");
    let (value, code) = run_clear_main(&home, TEST_SKILL, None, Some("env-session-xyz"));
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["removed"], true);
}

// --- run_current_session_id_main ---

#[test]
fn run_current_session_id_main_returns_captured_value() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let claude = home.join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let capture = claude.join("flow-current-session.json");
    fs::write(&capture, format!(r#"{{"session_id": "{}"}}"#, TEST_SESSION)).unwrap();
    let (text, code) = run_current_session_id_main(&home);
    assert_eq!(code, 0);
    assert_eq!(text, TEST_SESSION);
}

#[test]
fn run_current_session_id_main_returns_empty_when_capture_missing() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let (text, code) = run_current_session_id_main(&home);
    assert_eq!(code, 0);
    assert_eq!(
        text, "",
        "missing capture file → empty stdout (skill treats as no marker)"
    );
}

// --- error path: write fails when parent isn't writable ---

#[test]
fn write_marker_returns_err_when_parent_not_writable() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    // Make .claude/flow exist but read-only so fs::write fails.
    let claude_flow = home.join(".claude").join("flow");
    fs::create_dir_all(&claude_flow).unwrap();
    let mut perms = fs::metadata(&claude_flow).unwrap().permissions();
    perms.set_mode(0o500);
    fs::set_permissions(&claude_flow, perms).unwrap();
    let result = write_marker(&home, TEST_SKILL, TEST_SESSION);
    // Restore so TempDir can clean up.
    let mut restore = fs::metadata(&claude_flow).unwrap().permissions();
    restore.set_mode(0o700);
    fs::set_permissions(&claude_flow, restore).unwrap();
    assert!(
        result.is_err(),
        "write_marker must surface fs::write errors"
    );
    let msg = result.unwrap_err();
    assert!(
        msg.contains("write failed"),
        "error message must name the failure step: {}",
        msg
    );
}

#[test]
fn write_marker_returns_err_when_create_dir_fails() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    // Make .claude exist but read-only so fs::create_dir_all(.claude/flow)
    // cannot create the missing leaf — exercises the create_dir_all
    // map_err arm in write_marker.
    let claude = home.join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let mut perms = fs::metadata(&claude).unwrap().permissions();
    perms.set_mode(0o500);
    fs::set_permissions(&claude, perms).unwrap();
    let result = write_marker(&home, TEST_SKILL, TEST_SESSION);
    // Restore so TempDir can clean up.
    let mut restore = fs::metadata(&claude).unwrap().permissions();
    restore.set_mode(0o700);
    fs::set_permissions(&claude, restore).unwrap();
    assert!(result.is_err(), "write_marker must surface mkdir errors");
    let msg = result.unwrap_err();
    assert!(
        msg.contains("create dir failed"),
        "error message must name the create-dir step: {}",
        msg
    );
}

#[test]
fn clear_marker_returns_err_on_invalid_skill_name() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let result = clear_marker(&home, "../bad", TEST_SESSION);
    assert!(result.is_err(), "clear_marker must validate skill name");
    assert!(
        result.unwrap_err().contains("invalid skill"),
        "error must name the invalid skill"
    );
}

#[test]
fn run_set_main_prefers_explicit_session_id_over_env_value() {
    // Wrapper-level test for the explicit-wins-over-env precedence
    // branch: when both `--session-id` and the env var carry a
    // value, the explicit value reaches `marker_path`. The wrapper
    // drives the same precedence chain as the private helper but
    // through the only production callsite, so the precedence
    // contract is regression-protected via the public API.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let (value, code) = run_set_main(
        &home,
        TEST_SKILL,
        Some("explicit-id-abc"),
        Some("env-id-xyz"),
    );
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    let path_str = value["path"].as_str().unwrap();
    assert!(
        path_str.ends_with("utility-in-progress-explicit-id-abc.json"),
        "explicit arg must win over env value at the wrapper boundary"
    );
}

#[test]
fn run_set_main_rejects_invalid_env_value_and_falls_through() {
    // Wrapper-level test for the invalid-env-fallthrough branch:
    // an env value that fails `is_safe_session_id` must not flow
    // into `marker_path`. With no capture file present, the wrapper
    // surfaces the structured no-session-available error rather than
    // writing a marker keyed by the hostile string.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let (value, code) = run_set_main(&home, TEST_SKILL, None, Some("../escape-attempt"));
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert!(
        value["message"]
            .as_str()
            .unwrap_or("")
            .contains("no session_id available"),
        "invalid env value must not reach marker_path"
    );
}

#[test]
fn clear_marker_surfaces_io_error_when_path_is_directory() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    // Create a directory at the marker path so fs::remove_file fails
    // with a non-NotFound error (IsADirectory or PermissionDenied).
    let path = marker_path(&home, TEST_SESSION).expect("valid session_id");
    fs::create_dir_all(&path).unwrap();
    let result = clear_marker(&home, TEST_SKILL, TEST_SESSION);
    // Clean up before asserting so the TempDir drop succeeds.
    let _ = fs::remove_dir(&path);
    assert!(
        result.is_err(),
        "clear_marker must surface non-NotFound IO errors"
    );
    assert!(
        result.unwrap_err().contains("remove failed"),
        "error must name the remove step"
    );
}
