//! Tests for the `resume-anchor` resolver subcommand
//! (`src/resume_anchor.rs`).
//!
//! Library-level tests drive `run_impl` / `run_impl_main` with a
//! fixture HOME and an explicit `env_value` so the env-reading boundary
//! never races (per `.claude/rules/testing-gotchas.md`). Subprocess
//! tests spawn `bin/flow resume-anchor` env-neutralized per
//! `.claude/rules/subprocess-test-hygiene.md` to verify CLI dispatch
//! and end-to-end env reading.
//!
//! The resolver reads the session-keyed marker `phase-enter` wrote
//! (`src/phase_anchor.rs`) and emits exactly one of three outcomes:
//! `{status:"ok", worktree_cwd}`, `{status:"no_marker"}`, or
//! `{status:"error", message}` (fail-closed on a corrupt/oversized/
//! unsafe marker).

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};

use flow_rs::phase_anchor::marker_path;
use flow_rs::resume_anchor::{run_impl, run_impl_main, MARKER_BYTE_CAP};

// --- helpers ---

/// Write a phase-anchor marker for `session_id` under `home` with the
/// given raw bytes (caller controls validity to drive error branches).
fn write_marker_raw(home: &Path, session_id: &str, contents: &str) {
    let path = marker_path(home, session_id).expect("valid marker path");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, contents).unwrap();
}

/// Spawn `bin/flow resume-anchor` with env neutralized — `HOME` set to
/// the fixture, `FLOW_CI_RUNNING` removed, and a fixture cwd so the
/// binary never reads the real worktree's state.
fn run_resume_anchor_subprocess(home: &Path, cwd: &Path, session_id: Option<&str>) -> Value {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.arg("resume-anchor")
        .current_dir(cwd)
        .env("HOME", home)
        .env_remove("FLOW_CI_RUNNING")
        .env_remove("FLOW_SIMULATE_BRANCH");
    match session_id {
        Some(sid) => {
            cmd.env("CLAUDE_CODE_SESSION_ID", sid);
        }
        None => {
            cmd.env_remove("CLAUDE_CODE_SESSION_ID");
        }
    }
    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last = stdout.trim().lines().last().unwrap_or("");
    serde_json::from_str(last).unwrap_or_else(|_| json!({"raw": stdout.trim()}))
}

// --- run_impl: ok ---

#[test]
fn resume_anchor_returns_ok_with_worktree_cwd_for_valid_marker() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    write_marker_raw(
        &home,
        "sess-ok",
        &json!({
            "branch": "feat",
            "worktree_cwd": "/abs/worktree/feat",
            "relative_cwd": "",
            "written_at": "2026-01-01T00:00:00-08:00"
        })
        .to_string(),
    );
    let value = run_impl(&home, Some("sess-ok"));
    assert_eq!(value["status"], "ok");
    assert_eq!(value["worktree_cwd"], "/abs/worktree/feat");
}

// --- run_impl: no_marker ---

#[test]
fn resume_anchor_returns_no_marker_when_marker_missing() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    // Session resolves but no marker file exists.
    let value = run_impl(&home, Some("sess-absent"));
    assert_eq!(value["status"], "no_marker");
}

#[test]
fn resume_anchor_returns_no_marker_when_session_unresolvable() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    // No env value, no capture file → nothing to recover.
    let value = run_impl(&home, None);
    assert_eq!(value["status"], "no_marker");
}

#[test]
fn resume_anchor_returns_no_marker_when_home_unsafe() {
    // Empty home → marker_path returns None → nothing to recover.
    let value = run_impl(Path::new(""), Some("sess-ok"));
    assert_eq!(value["status"], "no_marker");
}

// --- run_impl: error (fail-closed) ---

#[test]
fn resume_anchor_errors_on_corrupt_json() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    write_marker_raw(&home, "sess-bad", "{ not valid json");
    let value = run_impl(&home, Some("sess-bad"));
    assert_eq!(value["status"], "error");
    assert!(value["message"].is_string());
}

#[test]
fn resume_anchor_errors_on_oversized_marker() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let oversized = "x".repeat((MARKER_BYTE_CAP as usize) + 1);
    write_marker_raw(&home, "sess-big", &oversized);
    let value = run_impl(&home, Some("sess-big"));
    assert_eq!(value["status"], "error");
}

#[test]
fn resume_anchor_errors_on_missing_worktree_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    write_marker_raw(
        &home,
        "sess-nofield",
        &json!({"branch": "feat", "relative_cwd": ""}).to_string(),
    );
    let value = run_impl(&home, Some("sess-nofield"));
    assert_eq!(value["status"], "error");
}

#[test]
fn resume_anchor_errors_on_unsafe_worktree_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    // A relative / traversal-bearing worktree_cwd must fail closed —
    // the skill cd's into the returned value, so it must be absolute
    // and free of `..`.
    write_marker_raw(
        &home,
        "sess-unsafe",
        &json!({"worktree_cwd": "../escape", "branch": "feat"}).to_string(),
    );
    let value = run_impl(&home, Some("sess-unsafe"));
    assert_eq!(value["status"], "error");
}

#[test]
fn resume_anchor_errors_on_nul_in_worktree_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    // An absolute-looking value carrying a NUL byte must fail closed.
    write_marker_raw(
        &home,
        "sess-nul",
        &json!({"worktree_cwd": "/abs/\u{0000}/x", "branch": "feat"}).to_string(),
    );
    let value = run_impl(&home, Some("sess-nul"));
    assert_eq!(value["status"], "error");
}

#[test]
fn resume_anchor_errors_on_absolute_traversal_worktree_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    // Absolute but carries a `..` component — fail closed so the
    // skill's `cd` cannot escape via traversal.
    write_marker_raw(
        &home,
        "sess-trav",
        &json!({"worktree_cwd": "/abs/../escape", "branch": "feat"}).to_string(),
    );
    let value = run_impl(&home, Some("sess-trav"));
    assert_eq!(value["status"], "error");
}

#[test]
fn resume_anchor_errors_when_marker_parent_is_file() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    // Place a regular file where the marker's parent directory
    // (`<home>/.claude/flow`) should be, so File::open of the marker
    // path fails with a non-NotFound error (ENOTDIR).
    let claude = home.join(".claude");
    fs::create_dir_all(&claude).unwrap();
    fs::write(claude.join("flow"), "blocker").unwrap();
    let value = run_impl(&home, Some("sess-enotdir"));
    assert_eq!(value["status"], "error");
}

#[test]
fn resume_anchor_errors_when_marker_path_is_directory() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    // Create a DIRECTORY at the marker path so File::open fails with a
    // non-NotFound error (EISDIR), exercising the io-error branch.
    let path = marker_path(&home, "sess-dir").unwrap();
    fs::create_dir_all(&path).unwrap();
    let value = run_impl(&home, Some("sess-dir"));
    assert_eq!(value["status"], "error");
}

// --- run_impl_main ---

#[test]
fn run_impl_main_wraps_value_with_exit_zero() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let (value, code) = run_impl_main(&home, None);
    assert_eq!(code, 0, "business outcomes always exit 0");
    assert_eq!(value["status"], "no_marker");
}

// --- subprocess ---

#[test]
fn resume_anchor_subprocess_returns_ok() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let cwd = dir.path().join("cwd");
    fs::create_dir_all(&cwd).unwrap();
    write_marker_raw(
        &home,
        "sess-sub",
        &json!({
            "branch": "feat",
            "worktree_cwd": "/abs/worktree/feat",
            "relative_cwd": ""
        })
        .to_string(),
    );
    let value = run_resume_anchor_subprocess(&home, &cwd, Some("sess-sub"));
    assert_eq!(value["status"], "ok");
    assert_eq!(value["worktree_cwd"], "/abs/worktree/feat");
}

#[test]
fn resume_anchor_subprocess_returns_no_marker_without_session() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let cwd = dir.path().join("cwd");
    fs::create_dir_all(&cwd).unwrap();
    let value = run_resume_anchor_subprocess(&home, &cwd, None);
    assert_eq!(value["status"], "no_marker");
}
