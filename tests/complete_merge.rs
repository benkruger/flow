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

/// Seed passing `bin/{format,lint,build,test}` scripts (each `exit 0`,
/// no `FLOW-STUB-UNCONFIGURED` marker) into `<parent>/bin/` so the
/// sentinel-gated `ci::run_impl` complete-merge runs at the
/// freshness-`merged` branch executes tools and passes — returning
/// `ci_rerun`. Without these scripts `bin_tool_sequence` is empty and
/// CI fails (the empty-tools error), returning `ci_failed`. CI runs
/// with cwd = the spawn dir (`parent`), so the scripts live there.
fn write_passing_bin_stubs(parent: &Path) {
    let bin_dir = parent.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    for tool in &["format", "lint", "build", "test"] {
        let p = bin_dir.join(tool);
        fs::write(&p, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn setup(parent: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(parent);
    let state_path = parent.join("state.json");
    // `skills.flow-complete: auto` keeps the merge-approval gate inert
    // for the freshness-dispatch tests, which exercise the merge path
    // and not the confirmation gate.
    fs::write(
        &state_path,
        r#"{"branch":"test-feature","skills":{"flow-complete":{"continue":"auto"}}}"#,
    )
    .unwrap();
    (state_path, flow_bin, stubs)
}

/// Build a canonical `<parent>/.flow-states/<branch>/state.json`
/// layout so the merge gate's marker lookup (the state file's parent
/// directory) resolves to a real per-branch directory. Returns the
/// state-file path.
///
/// `branch` names the per-branch subdirectory under `.flow-states/`.
/// `skills_continue` is written verbatim into
/// `skills.flow-complete.continue`: pass `"manual"` to arm the
/// merge-approval gate (the merge then requires a confirmation
/// marker) or `"auto"` to leave it inert (the merge proceeds with no
/// marker).
fn setup_flow_layout(parent: &Path, branch: &str, skills_continue: &str) -> PathBuf {
    let branch_dir = parent.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    fs::write(
        &state_path,
        format!(
            r#"{{"branch":"{branch}","skills":{{"flow-complete":{{"continue":"{skills_continue}"}}}}}}"#
        ),
    )
    .unwrap();
    state_path
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
fn main_moved_ci_passes_returns_ci_rerun() {
    // Main moved → freshness `merged` → push, then sentinel-gated CI
    // runs inline on the freshly-merged tree. With passing `bin/*`
    // stubs CI passes, so complete-merge returns `ci_rerun` (the
    // Step-4 loop re-checks freshness and squash-merges; its CI skips
    // on the sentinel). Pre-inline-CI this deferred to `ci_rerun`
    // without running CI at all.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (state_path, flow_bin, stubs) = setup(&parent);
    write_passing_bin_stubs(&parent);

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
fn main_moved_ci_fails_returns_ci_failed() {
    // Main moved → freshness `merged` → push, then sentinel-gated CI
    // runs inline. With no `bin/*` scripts present, `ci::run_impl`
    // hits the empty-tools error → complete-merge returns `ci_failed`
    // (Step 4 launches ci-fixer directly rather than deferring an
    // untested tree). `pushed` stays true — the freshness merge landed.
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

    assert_eq!(code, 1); // ci_failed != merged → exit 1
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ci_failed");
    assert_eq!(json["pushed"], true);
    assert_eq!(json["pr_number"], 42);
    assert!(
        json.get("output").is_some(),
        "ci_failed must carry the CI output for ci-fixer; got: {}",
        json
    );
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
fn branch_protection_not_mergeable() {
    // `gh pr merge --squash` refused with a base-branch-policy stderr.
    // complete-merge surfaces the verbatim gh stderr as status:
    // "not_mergeable" with a `message` field; run_impl_main routes the
    // non-`merged` status to exit 1.
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
    assert_eq!(json["status"], "not_mergeable");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("base branch policy"));
    assert_eq!(json["pr_number"], 42);
}

#[test]
fn branch_protection_not_mergeable_case_insensitive() {
    // The not_mergeable classification case-folds the gh stderr per
    // `.claude/rules/security-gates.md` "Normalize Before Comparing".
    // A capitalized "Base branch policy" refusal must still classify
    // as not_mergeable (exit 1), not fall through to status:error.
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
                "Base branch policy prohibits the merge",
            ),
        ],
    );

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "not_mergeable");
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
    assert_eq!(state["complete_step"], 4);
}

#[test]
fn missing_state_file_refuses_merge_fail_closed() {
    // No state file → the merge gate cannot resolve the configured
    // flow-complete mode and fails closed to `manual`; with no
    // confirmation marker the merge is refused rather than
    // proceeding unconfirmed.
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

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert_eq!(json["reason"], "merge_not_confirmed");
}

#[test]
fn non_json_state_file_refuses_merge_fail_closed() {
    // A non-JSON state file cannot be parsed → the gate fails closed
    // to `manual` and refuses the merge without a marker.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);
    let state_path = parent.join("state.json");
    fs::write(&state_path, "not json at all").unwrap();

    let (code, stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[("FAKE_FRESHNESS_JSON", r#"{"status":"up_to_date"}"#)],
    );

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["reason"], "merge_not_confirmed");
}

#[test]
fn wrong_type_state_file_refuses_merge_no_panic() {
    // A non-object JSON root (array) carries no skills config; the
    // gate resolves `manual` and refuses the merge without
    // panicking — mutate_state's object guard still absorbs the
    // array for the earlier `complete_step` write.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);
    let state_path = parent.join("state.json");
    fs::write(&state_path, "[1,2,3]").unwrap();

    let (code, stdout, stderr) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[("FAKE_FRESHNESS_JSON", r#"{"status":"up_to_date"}"#)],
    );

    assert_eq!(code, 1);
    assert!(!stderr.contains("panicked"));
    let json = last_json_line(&stdout);
    assert_eq!(json["reason"], "merge_not_confirmed");
}

// --- merge-approval gate (manual mode) ---

#[test]
fn manual_config_without_marker_refuses_merge() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);
    let state_path = setup_flow_layout(&parent, "feat-merge", "manual");

    let (code, stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[("FAKE_FRESHNESS_JSON", r#"{"status":"up_to_date"}"#)],
    );

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert_eq!(json["reason"], "merge_not_confirmed");
    assert_eq!(json["pr_number"], 42);
}

#[test]
fn manual_config_with_valid_marker_merges_and_consumes() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);
    let state_path = setup_flow_layout(&parent, "feat-merge", "manual");
    let branch_dir = state_path.parent().unwrap();
    flow_rs::merge_approval::write_approval(branch_dir, "feat-merge").unwrap();

    let (code, stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[("FAKE_FRESHNESS_JSON", r#"{"status":"up_to_date"}"#)],
    );

    assert_eq!(code, 0, "stdout={}", stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "merged");
    // Single-use: the merge consumed the marker.
    assert!(!flow_rs::merge_approval::check_and_consume_approval(
        branch_dir
    ));
}

/// Regression: a `--continue-step` resume of Complete previously
/// skipped the SOFT-GATE that resolved the mode, so a resumed merge
/// depended on a model-threaded flag instead of the state file and
/// could squash-merge a manual-configured flow unconfirmed. This
/// reproduces that scenario end to end — a `flow-complete: manual`
/// state file reached at the merge step (as a resumed run does) with
/// no confirmation marker — and asserts `complete-merge` structurally
/// refuses it with `merge_not_confirmed` rather than merging.
#[test]
fn regression_resumed_manual_merge_without_marker_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);
    let state_path = setup_flow_layout(&parent, "resumed-feature", "manual");

    let (code, stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[("FAKE_FRESHNESS_JSON", r#"{"status":"up_to_date"}"#)],
    );

    assert_eq!(code, 1, "a manual flow with no marker must not merge");
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert_eq!(json["reason"], "merge_not_confirmed");
}

#[test]
fn manual_config_with_corrupt_marker_refuses_merge() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);
    let state_path = setup_flow_layout(&parent, "feat-merge", "manual");
    // A non-JSON marker fails closed: no approval, merge refused.
    fs::write(
        state_path.parent().unwrap().join("merge-approval"),
        "not json",
    )
    .unwrap();

    let (code, stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[("FAKE_FRESHNESS_JSON", r#"{"status":"up_to_date"}"#)],
    );

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["reason"], "merge_not_confirmed");
}

/// Regression: the merge-approval marker must be consumed on EVERY
/// merge attempt, not only when the freshness check returns
/// `up_to_date`. A `merged` freshness outcome returns `ci_rerun`
/// (loop-back through CI) without reaching the squash-merge — but the
/// marker must still be consumed so the loop-back forces a fresh
/// confirmation rather than reusing the stale approval. Guards a
/// regression where the gate is moved back into the `up_to_date` arm,
/// leaving the marker live across a `ci_rerun` loop-back.
#[test]
fn manual_config_marker_consumed_on_ci_rerun_loopback() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_flow_stub(&flow_bin);
    let stubs = build_path_stubs(&parent);
    let state_path = setup_flow_layout(&parent, "feat-merge", "manual");
    let branch_dir = state_path.parent().unwrap().to_path_buf();
    flow_rs::merge_approval::write_approval(&branch_dir, "feat-merge").unwrap();
    // Freshness `merged` reaches the inline CI; passing stubs keep the
    // outcome `ci_rerun` so this test isolates the marker-consumption
    // invariant rather than a CI failure.
    write_passing_bin_stubs(&parent);

    let (code, stdout, _) = run_complete_merge_sub(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &flow_bin,
        &stubs,
        &[("FAKE_FRESHNESS_JSON", r#"{"status":"merged"}"#)],
    );

    // Freshness `merged` → inline CI passes → ci_rerun, no squash-merge reached.
    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ci_rerun");
    // The marker was consumed before the freshness check, so it is
    // gone even though the merge did not happen.
    assert!(!flow_rs::merge_approval::check_and_consume_approval(
        &branch_dir
    ));
}

#[test]
fn freshness_spawn_error_returns_error_status() {
    // Point FLOW_BIN_PATH at a nonexistent binary → check-freshness
    // spawn fails → run_cmd_with_timeout returns Err.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let stubs = build_path_stubs(&parent);
    let state_path = parent.join("state.json");
    // `flow-complete: auto` so the merge-approval gate is inert and the
    // run reaches the check-freshness spawn this test exercises.
    fs::write(
        &state_path,
        r#"{"branch":"feat","skills":{"flow-complete":{"continue":"auto"}}}"#,
    )
    .unwrap();
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
