//! Unit tests for the phase-anchor marker writer (`src/phase_anchor.rs`).
//!
//! Drives the public surface — `marker_path`, `write_anchor`,
//! `resolve_session_id`, `write_anchor_if_resolvable` — directly so the
//! error and resolution branches the subprocess `phase-enter` tests in
//! `tests/phase_enter.rs` cannot reach (bad session id, unwritable
//! parent, symlink pre-placement, capture-file fallback) are exercised.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use flow_rs::phase_anchor::{
    marker_path, resolve_session_id, write_anchor, write_anchor_if_resolvable,
    PHASE_ANCHOR_FILENAME_PREFIX, PHASE_ANCHOR_SUBDIR,
};

// --- marker_path ---

#[test]
fn marker_path_valid_home_and_session_returns_canonical_path() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let path = marker_path(&home, "sess-123").expect("valid inputs yield a path");
    assert_eq!(
        path,
        home.join(PHASE_ANCHOR_SUBDIR)
            .join(format!("{}sess-123.json", PHASE_ANCHOR_FILENAME_PREFIX))
    );
}

#[test]
fn marker_path_rejects_empty_home() {
    assert!(marker_path(Path::new(""), "sess-123").is_none());
}

#[test]
fn marker_path_rejects_relative_home() {
    assert!(marker_path(Path::new("relative/home"), "sess-123").is_none());
}

#[test]
fn marker_path_rejects_home_with_nul() {
    assert!(marker_path(&PathBuf::from("/abs\0home"), "sess-123").is_none());
}

#[test]
fn marker_path_rejects_unsafe_session_id() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    // `/` is rejected by is_safe_session_id.
    assert!(marker_path(&home, "bad/session").is_none());
}

// --- write_anchor ---

#[test]
fn write_anchor_writes_marker_with_all_fields() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let path =
        write_anchor(&home, "sess-abc", "feat-branch", "/wt/cwd", "api").expect("write succeeds");
    let marker: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(marker["branch"], "feat-branch");
    assert_eq!(marker["worktree_cwd"], "/wt/cwd");
    assert_eq!(marker["relative_cwd"], "api");
    assert!(
        marker["written_at"].is_string(),
        "written_at must be recorded"
    );
}

#[test]
fn write_anchor_rejects_unsafe_session_id() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let err = write_anchor(&home, "bad/session", "b", "/wt", "").unwrap_err();
    assert!(
        err.contains("invalid session_id"),
        "expected invalid-session error, got: {}",
        err
    );
}

#[test]
fn write_anchor_create_dir_fails_when_ancestor_is_file() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    // Place a regular file at `<home>/.claude` so create_dir_all of
    // `<home>/.claude/flow` fails (an ancestor is not a directory).
    fs::write(home.join(".claude"), "blocker").unwrap();
    let err = write_anchor(&home, "sess-abc", "b", "/wt", "").unwrap_err();
    assert!(
        err.contains("create dir failed"),
        "expected create-dir error, got: {}",
        err
    );
}

#[test]
fn write_anchor_write_fails_when_parent_readonly() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let flow_dir = home.join(PHASE_ANCHOR_SUBDIR);
    fs::create_dir_all(&flow_dir).unwrap();
    // Make the parent directory read-only so create_dir_all is a no-op
    // (already exists) but fs::write fails.
    fs::set_permissions(&flow_dir, fs::Permissions::from_mode(0o555)).unwrap();
    let result = write_anchor(&home, "sess-abc", "b", "/wt", "");
    // Restore permissions for tempdir cleanup.
    let _ = fs::set_permissions(&flow_dir, fs::Permissions::from_mode(0o755));
    let err = result.unwrap_err();
    assert!(
        err.contains("write failed"),
        "expected write error, got: {}",
        err
    );
}

#[test]
fn write_anchor_replaces_pre_existing_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let flow_dir = home.join(PHASE_ANCHOR_SUBDIR);
    fs::create_dir_all(&flow_dir).unwrap();
    let marker = flow_dir.join(format!("{}sess-abc.json", PHASE_ANCHOR_FILENAME_PREFIX));
    // Pre-place a symlink at the marker path pointing at an unrelated
    // target. write_anchor must unlink it and write a regular file.
    let bait = home.join("bait.txt");
    fs::write(&bait, "original").unwrap();
    std::os::unix::fs::symlink(&bait, &marker).unwrap();

    write_anchor(&home, "sess-abc", "b", "/wt", "").expect("write succeeds");

    let meta = fs::symlink_metadata(&marker).unwrap();
    assert!(
        meta.file_type().is_file(),
        "marker must be a regular file after write, not a symlink"
    );
    // The symlink target must be untouched (write did not follow it).
    assert_eq!(fs::read_to_string(&bait).unwrap(), "original");
}

#[test]
fn write_anchor_overwrites_pre_existing_regular_file() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let flow_dir = home.join(PHASE_ANCHOR_SUBDIR);
    fs::create_dir_all(&flow_dir).unwrap();
    let marker = flow_dir.join(format!("{}sess-abc.json", PHASE_ANCHOR_FILENAME_PREFIX));
    // A stale regular marker from a prior entry exists — symlink_metadata
    // returns Ok with is_symlink() false, so the remove branch is skipped
    // and fs::write overwrites it in place.
    fs::write(&marker, "stale").unwrap();

    write_anchor(&home, "sess-abc", "new-branch", "/wt", "").expect("write succeeds");

    let refreshed: Value = serde_json::from_str(&fs::read_to_string(&marker).unwrap()).unwrap();
    assert_eq!(
        refreshed["branch"], "new-branch",
        "write_anchor must overwrite the stale regular marker"
    );
}

// --- resolve_session_id ---

#[test]
fn resolve_session_id_prefers_valid_env_value() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let sid = resolve_session_id(&home, Some("env-sess-1")).expect("env value resolves");
    assert_eq!(sid, "env-sess-1");
}

#[test]
fn resolve_session_id_falls_through_invalid_env_to_capture_file() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    write_capture_file(&home, "capture-sess");
    // Invalid env value (slash) falls through to the capture file.
    let sid = resolve_session_id(&home, Some("bad/env")).expect("capture file resolves");
    assert_eq!(sid, "capture-sess");
}

#[test]
fn resolve_session_id_reads_capture_file_when_env_absent() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    write_capture_file(&home, "capture-sess");
    let sid = resolve_session_id(&home, None).expect("capture file resolves");
    assert_eq!(sid, "capture-sess");
}

#[test]
fn resolve_session_id_returns_none_when_no_source() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    assert!(resolve_session_id(&home, None).is_none());
    // Empty env value is also treated as no source.
    assert!(resolve_session_id(&home, Some("")).is_none());
}

// --- write_anchor_if_resolvable ---

#[test]
fn write_anchor_if_resolvable_writes_when_session_resolves() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    write_anchor_if_resolvable(&home, Some("env-sess-2"), "b", "/wt", "");
    let marker = home
        .join(PHASE_ANCHOR_SUBDIR)
        .join(format!("{}env-sess-2.json", PHASE_ANCHOR_FILENAME_PREFIX));
    assert!(
        marker.exists(),
        "marker must be written when session resolves"
    );
}

#[test]
fn write_anchor_if_resolvable_skips_when_no_session() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    write_anchor_if_resolvable(&home, None, "b", "/wt", "");
    let flow_dir = home.join(PHASE_ANCHOR_SUBDIR);
    assert!(
        !flow_dir.exists() || fs::read_dir(&flow_dir).unwrap().next().is_none(),
        "no marker may be written when no session resolves"
    );
}

// --- test helpers ---

/// Write a SessionStart capture file at
/// `<home>/.claude/flow-current-session.json` carrying `session_id` so
/// `resolve_session_id`'s capture-file fallback resolves it.
fn write_capture_file(home: &Path, session_id: &str) {
    let claude = home.join(".claude");
    fs::create_dir_all(&claude).unwrap();
    fs::write(
        claude.join("flow-current-session.json"),
        json!({ "session_id": session_id }).to_string(),
    )
    .unwrap();
}
