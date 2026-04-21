//! Subprocess integration tests for `bin/flow complete-merge`.
//!
//! Drives the compiled `flow-rs complete-merge` binary via env-var-
//! controlled stubs for `bin/flow check-freshness`, `gh pr merge`, and
//! `git push`. Covers every arm of the freshness-status dispatch +
//! the merge/push exit-code branches.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use flow_rs::complete_merge::cmd_failure_message;

mod common;

// --- cmd_failure_message ---

#[test]
fn cmd_failure_message_ok_zero_returns_none() {
    let result = Ok((0, String::new(), String::new()));
    assert!(cmd_failure_message(result).is_none());
}

#[test]
fn cmd_failure_message_ok_nonzero_returns_stderr() {
    let result = Ok((1, String::new(), "  something bad  ".to_string()));
    assert_eq!(
        cmd_failure_message(result).as_deref(),
        Some("something bad")
    );
}

#[test]
fn cmd_failure_message_err_returns_err_message() {
    let result = Err("spawn failed".to_string());
    assert_eq!(cmd_failure_message(result).as_deref(), Some("spawn failed"));
}

/// bin/flow stub: handles `check-freshness` via FAKE_FRESHNESS_JSON
/// and optional FAKE_FRESHNESS_EXIT (default 0); other subcommands
/// exit 0.
fn write_flow_stub(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let script = r#"#!/bin/sh
case "$1" in
    check-freshness)
        if [ -n "$FAKE_FRESHNESS_JSON" ]; then
            printf '%s' "$FAKE_FRESHNESS_JSON"
        else
            printf '%s' '{"status":"up_to_date"}'
        fi
        exit ${FAKE_FRESHNESS_EXIT:-0}
        ;;
    *)
        exit 0
        ;;
esac
"#;
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// PATH stubs for `gh` (controls pr merge) and `git` (controls push).
fn build_path_stubs(parent: &Path) -> PathBuf {
    let stubs = parent.join("stubs");
    fs::create_dir_all(&stubs).unwrap();

    let gh_script = r#"#!/bin/sh
if [ "$1 $2" = "pr merge" ]; then
    if [ -n "$FAKE_MERGE_STDERR" ]; then printf '%s' "$FAKE_MERGE_STDERR" >&2; fi
    exit ${FAKE_MERGE_EXIT:-0}
fi
exit 0
"#;
    let gh_path = stubs.join("gh");
    fs::write(&gh_path, gh_script).unwrap();
    fs::set_permissions(&gh_path, fs::Permissions::from_mode(0o755)).unwrap();

    let git_script = r#"#!/bin/sh
if [ "$1" = "push" ]; then
    if [ -n "$FAKE_GIT_PUSH_STDERR" ]; then printf '%s' "$FAKE_GIT_PUSH_STDERR" >&2; fi
    exit ${FAKE_GIT_PUSH_EXIT:-0}
fi
exec /usr/bin/git "$@"
"#;
    let git_path = stubs.join("git");
    fs::write(&git_path, git_script).unwrap();
    fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();

    stubs
}

fn run_complete_merge_sub(
    cwd: &Path,
    pr: &str,
    state_file: &str,
    flow_bin_path: &Path,
    stubs: &Path,
    env: &[(&str, &str)],
) -> (i32, String, String) {
    let current_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", stubs.display(), current_path);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.args(["complete-merge", "--pr", pr, "--state-file", state_file])
        .current_dir(cwd)
        .env("PATH", new_path)
        .env("FLOW_BIN_PATH", flow_bin_path)
        .env_remove("FLOW_CI_RUNNING");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let output = cmd.output().expect("spawn flow-rs");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn last_json_line(stdout: &str) -> Value {
    let last = stdout
        .lines()
        .rfind(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON line in stdout; stdout={}", stdout));
    serde_json::from_str(last)
        .unwrap_or_else(|e| panic!("failed to parse JSON line '{}': {}", last, e))
}

fn setup(parent: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(parent);
    let state_path = parent.join("state.json");
    fs::write(&state_path, r#"{"branch":"test-feature"}"#).unwrap();
    (state_path, flow_bin, stubs)
}

#[test]
fn up_to_date_and_merge_succeeds_exits_0() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (state_path, flow_bin, stubs) = setup(&parent);

    let (code, stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[("FAKE_FRESHNESS_JSON", r#"{"status":"up_to_date"}"#)],
    );

    assert_eq!(code, 0, "merged must exit 0; stdout={}", stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "merged");
    assert_eq!(json["pr_number"], 42);
}

#[test]
fn main_moved_ci_rerun() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (state_path, flow_bin, stubs) = setup(&parent);

    let (code, stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[("FAKE_FRESHNESS_JSON", r#"{"status":"merged"}"#)],
    );

    assert_eq!(code, 1); // ci_rerun != merged → exit 1
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ci_rerun");
    assert_eq!(json["pushed"], true);
    assert_eq!(json["pr_number"], 42);
}

#[test]
fn merge_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (state_path, flow_bin, stubs) = setup(&parent);

    let (code, stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[(
            "FAKE_FRESHNESS_JSON",
            r#"{"status":"conflict","files":["lib/foo.rs","lib/bar.rs"]}"#,
        )],
    );

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "conflict");
    let files: Vec<String> = json["conflict_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(files, vec!["lib/foo.rs", "lib/bar.rs"]);
    assert_eq!(json["pr_number"], 42);
}

#[test]
fn conflict_with_missing_files_defaults_to_empty_array() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (state_path, flow_bin, stubs) = setup(&parent);

    let (code, stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[("FAKE_FRESHNESS_JSON", r#"{"status":"conflict"}"#)],
    );

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "conflict");
    assert_eq!(json["conflict_files"], serde_json::json!([]));
}

#[test]
fn max_retries_returns_max_retries_status() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (state_path, flow_bin, stubs) = setup(&parent);

    let (code, stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[(
            "FAKE_FRESHNESS_JSON",
            r#"{"status":"max_retries","retries":3}"#,
        )],
    );

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "max_retries");
}

#[test]
fn freshness_error_with_message() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (state_path, flow_bin, stubs) = setup(&parent);

    let (code, stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[(
            "FAKE_FRESHNESS_JSON",
            r#"{"status":"error","message":"network error"}"#,
        )],
    );

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap_or("")
        .contains("network error"));
}

#[test]
fn freshness_error_without_message_uses_default() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (state_path, flow_bin, stubs) = setup(&parent);

    let (code, stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[("FAKE_FRESHNESS_JSON", r#"{"status":"error"}"#)],
    );

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap_or("")
        .contains("check-freshness failed"));
}

#[test]
fn branch_protection_ci_pending() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (state_path, flow_bin, stubs) = setup(&parent);

    let (code, stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[
            ("FAKE_FRESHNESS_JSON", r#"{"status":"up_to_date"}"#),
            ("FAKE_MERGE_EXIT", "1"),
            (
                "FAKE_MERGE_STDERR",
                "base branch policy prohibits the merge",
            ),
        ],
    );

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ci_pending");
    assert_eq!(json["pr_number"], 42);
}

#[test]
fn merge_fails_other_reason() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (state_path, flow_bin, stubs) = setup(&parent);

    let (code, stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[
            ("FAKE_FRESHNESS_JSON", r#"{"status":"up_to_date"}"#),
            ("FAKE_MERGE_EXIT", "1"),
            ("FAKE_MERGE_STDERR", "unknown merge error"),
        ],
    );

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap_or("")
        .contains("unknown merge error"));
}

#[test]
fn push_failure_after_freshness_merge() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (state_path, flow_bin, stubs) = setup(&parent);

    let (code, stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[
            ("FAKE_FRESHNESS_JSON", r#"{"status":"merged"}"#),
            ("FAKE_GIT_PUSH_EXIT", "1"),
            ("FAKE_GIT_PUSH_STDERR", "remote rejected"),
        ],
    );

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap_or("")
        .to_lowercase()
        .contains("push"));
    assert!(json["message"]
        .as_str()
        .unwrap_or("")
        .contains("remote rejected"));
}

#[test]
fn check_freshness_invalid_json() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (state_path, flow_bin, stubs) = setup(&parent);

    let (code, stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[("FAKE_FRESHNESS_JSON", "not json at all")],
    );

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap_or("")
        .contains("Invalid JSON"));
}

#[test]
fn unknown_freshness_status() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (state_path, flow_bin, stubs) = setup(&parent);

    let (code, stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[("FAKE_FRESHNESS_JSON", r#"{"status":"unexpected_value"}"#)],
    );

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap_or("")
        .to_lowercase()
        .contains("unexpected"));
}

#[test]
fn missing_freshness_status_key_triggers_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (state_path, flow_bin, stubs) = setup(&parent);

    let (code, stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[("FAKE_FRESHNESS_JSON", r#"{"foo":"bar"}"#)],
    );

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
}

#[test]
fn step_counter_set_on_existing_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (state_path, flow_bin, stubs) = setup(&parent);

    let (_code, _stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[("FAKE_FRESHNESS_JSON", r#"{"status":"up_to_date"}"#)],
    );

    let state_content = fs::read_to_string(&state_path).unwrap();
    let state: Value = serde_json::from_str(&state_content).unwrap();
    assert_eq!(state["complete_step"], 5);
}

#[test]
fn missing_state_file_still_completes() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);
    let missing = parent.join("nonexistent.json");

    let (code, stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        missing.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[("FAKE_FRESHNESS_JSON", r#"{"status":"up_to_date"}"#)],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "merged");
}

#[test]
fn state_file_wrong_type_does_not_panic() {
    // mutate_state's object guard should absorb a state file whose
    // root is an array. The function still runs to merged.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);
    let state_path = parent.join("state.json");
    fs::write(&state_path, "[1,2,3]").unwrap();

    let (code, stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[("FAKE_FRESHNESS_JSON", r#"{"status":"up_to_date"}"#)],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "merged");
}

#[test]
fn freshness_spawn_error_returns_error_status() {
    // Point FLOW_BIN_PATH at a nonexistent binary → check-freshness
    // spawn fails → run_cmd_with_timeout returns Err.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let stubs = build_path_stubs(&parent);
    let state_path = parent.join("state.json");
    fs::write(&state_path, r#"{"branch":"feat"}"#).unwrap();
    let nonexistent = parent.join("does-not-exist").join("flow");

    let (code, stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &nonexistent,
        &stubs,
        &[],
    );

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
}
