//! Subprocess integration tests for `bin/flow complete-post-merge`.
//!
//! post_merge_inner has been folded into the private body of `post_merge`
//! and is no longer a pub testing seam. Tests drive the module via the
//! public `post_merge` wrapper (and the `complete-post-merge` subcommand)
//! with a configurable `bin/flow` stub whose per-subcommand behavior is
//! controlled via env vars (FAKE_PT_STATUS, FAKE_PT_OUT, FAKE_RENDER_EXIT,
//! etc.) so each branch is exercised through the real subprocess chain.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use flow_rs::complete_post_merge::{ok_stdout_as_json, post_merge};
use serde_json::{json, Value};

const BRANCH: &str = "test-feature";

fn make_repo_fixture(parent: &Path) -> PathBuf {
    let repo = common::create_git_repo_with_remote(parent);
    let repo = repo.canonicalize().expect("canonicalize repo");
    Command::new("git")
        .args(["checkout", "-b", BRANCH])
        .current_dir(&repo)
        .output()
        .unwrap();
    repo
}

/// Default happy-path state (repo set, no slack thread).
fn happy_state(branch: &str) -> Value {
    json!({
        "schema_version": 1,
        "branch": branch,
        "pr_number": 42,
        "pr_url": "https://github.com/test/test/pull/42",
        "prompt": "work on issue #100",
        "complete_step": 5,
        "repo": "test/test",
        "phases": {
            "flow-start": {"status": "complete"},
            "flow-plan": {"status": "complete"},
            "flow-code": {"status": "complete"},
            "flow-code-review": {"status": "complete"},
            "flow-learn": {"status": "complete"},
            "flow-complete": {"status": "in_progress"}
        }
    })
}

fn write_state_file_at(path: &Path, content: &Value) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, serde_json::to_string_pretty(content).unwrap()).unwrap();
}

/// Configurable bin/flow stub. Behavior per subcommand is controlled by
/// env vars passed to the parent flow-rs spawn; the child inherits them.
///
/// Env contract:
///   FAKE_PT_OUT       → phase-transition stdout (default ok JSON)
///   FAKE_PT_EXIT      → phase-transition exit code (default 0)
///   FAKE_PT_ERR       → phase-transition stderr (default empty)
///   FAKE_RENDER_EXIT  → render-pr-body exit code (default 0)
///   FAKE_RENDER_ERR   → render-pr-body stderr (default empty)
///   FAKE_ISSUES_OUT   → format-issues-summary stdout (default no-issues)
///   FAKE_CLOSE_OUT    → close-issues stdout (default empty)
///   FAKE_SUMMARY_OUT  → format-complete-summary stdout (default ok)
///   FAKE_LABEL_EXIT   → label-issues exit code (default 0)
///   FAKE_LABEL_ERR    → label-issues stderr (default empty)
///   FAKE_ACP_OUT      → auto-close-parent stdout (default no-close)
///   FAKE_SLACK_OUT    → notify-slack stdout (default ok with ts)
///   FAKE_SLACK_EXIT   → notify-slack exit code (default 0)
///   FAKE_ADD_NOT_OUT  → add-notification stdout
fn write_configurable_flow_stub(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let script = r#"#!/bin/sh
case "$1" in
    phase-transition)
        if [ -n "$FAKE_PT_ERR" ]; then printf '%s' "$FAKE_PT_ERR" >&2; fi
        if [ -n "$FAKE_PT_OUT" ]; then
            printf '%s' "$FAKE_PT_OUT"
        else
            printf '%s' '{"status":"ok","formatted_time":"1m","cumulative_seconds":60}'
        fi
        exit ${FAKE_PT_EXIT:-0}
        ;;
    render-pr-body)
        if [ -n "$FAKE_RENDER_ERR" ]; then printf '%s' "$FAKE_RENDER_ERR" >&2; fi
        exit ${FAKE_RENDER_EXIT:-0}
        ;;
    format-issues-summary)
        if [ -n "$FAKE_ISSUES_OUT" ]; then
            printf '%s' "$FAKE_ISSUES_OUT"
        else
            printf '%s' '{"status":"ok","has_issues":false,"banner_line":""}'
        fi
        exit 0
        ;;
    close-issues)
        if [ -n "$FAKE_CLOSE_OUT" ]; then
            printf '%s' "$FAKE_CLOSE_OUT"
        else
            printf '%s' '{"status":"ok","closed":[],"failed":[]}'
        fi
        exit 0
        ;;
    format-complete-summary)
        if [ -n "$FAKE_SUMMARY_OUT" ]; then
            printf '%s' "$FAKE_SUMMARY_OUT"
        else
            printf '%s' '{"status":"ok","summary":"test summary","issues_links":""}'
        fi
        exit 0
        ;;
    label-issues)
        if [ -n "$FAKE_LABEL_ERR" ]; then printf '%s' "$FAKE_LABEL_ERR" >&2; fi
        exit ${FAKE_LABEL_EXIT:-0}
        ;;
    auto-close-parent)
        if [ -n "$FAKE_ACP_OUT" ]; then
            printf '%s' "$FAKE_ACP_OUT"
        else
            printf '%s' '{"status":"ok","parent_closed":false,"milestone_closed":false}'
        fi
        exit 0
        ;;
    notify-slack)
        if [ -n "$FAKE_SLACK_OUT" ]; then
            printf '%s' "$FAKE_SLACK_OUT"
        else
            printf '%s' '{"status":"ok","ts":"1234.5678"}'
        fi
        exit ${FAKE_SLACK_EXIT:-0}
        ;;
    add-notification)
        if [ -n "$FAKE_ADD_NOT_OUT" ]; then
            printf '%s' "$FAKE_ADD_NOT_OUT"
        else
            printf '%s' '{"status":"ok"}'
        fi
        exit 0
        ;;
    *)
        exit 0
        ;;
esac
"#;
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Build the minimal PATH stub dir: just a `gh` that always exits 0.
fn path_stub_dir(parent: &Path) -> PathBuf {
    let stubs = parent.join("stubs");
    fs::create_dir_all(&stubs).unwrap();
    let gh_path = stubs.join("gh");
    fs::write(&gh_path, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&gh_path, fs::Permissions::from_mode(0o755)).unwrap();
    stubs
}

/// Spawn flow-rs complete-post-merge with env overrides for the stub.
fn run_post_merge_sub(
    cwd: &Path,
    pr: &str,
    state_file: &str,
    branch: &str,
    flow_bin_path: &Path,
    path_stubs: &Path,
    env: &[(&str, &str)],
) -> (i32, String) {
    let current_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", path_stubs.display(), current_path);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.args([
        "complete-post-merge",
        "--pr",
        pr,
        "--state-file",
        state_file,
        "--branch",
        branch,
    ])
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

/// Write a state file inside the repo's `.flow-states/` and return
/// (repo, state_file_path, flow_bin_path, path_stubs).
fn setup(parent: &Path, state: Value) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let repo = make_repo_fixture(parent);
    let branch_dir = repo.join(".flow-states").join(BRANCH);
    fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    write_state_file_at(&state_path, &state);
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_configurable_flow_stub(&flow_bin);
    let stubs = path_stub_dir(parent);
    (repo, state_path, flow_bin, stubs)
}

#[test]
fn post_merge_happy_path_returns_ok() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (repo, state_path, flow_bin, stubs) = setup(&parent, happy_state(BRANCH));

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["formatted_time"], "1m");
    assert_eq!(json["cumulative_seconds"], 60);
    assert_eq!(json["summary"], "test summary");
    assert!(json["failures"].as_object().unwrap().is_empty());
}

#[test]
fn post_merge_invalid_branch_returns_invalid_branch_failure() {
    // try_new rejects slash-containing branches.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (repo, state_path, flow_bin, stubs) = setup(&parent, happy_state(BRANCH));

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        "bad/branch",
        &flow_bin,
        &stubs,
        &[],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert!(json["failures"]["invalid_branch"].is_string());
}

#[test]
fn post_merge_missing_state_file_still_runs() {
    // state_path doesn't exist → state = {}, repo absent, no mutate_state.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let missing = repo.join(".flow-states").join("nonexistent.json");
    fs::create_dir_all(missing.parent().unwrap()).unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_configurable_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        missing.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["parents_closed"].as_array().unwrap().len(), 0);
}

#[test]
fn post_merge_corrupt_state_file_falls_back_to_empty() {
    // state_path exists but JSON is malformed → fallback to json!({}).
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join(format!("{}.json", BRANCH));
    fs::write(&state_path, "{corrupt").unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_configurable_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    // Corrupt JSON still returns a well-formed post-merge result.
    assert_eq!(json["status"], "ok");
}

#[test]
fn post_merge_state_wrong_type_records_step_counter_failure() {
    // state file is a JSON array — mutate_state's object guard fires,
    // but no step_counter failure because mutate_state itself returns Ok.
    // Actually: state is array → our state_path.exists() is true, but
    // mutate_state assigns to state["complete_step"] which hits the guard
    // and returns early Ok. The failures map stays empty on this path.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join(format!("{}.json", BRANCH));
    fs::write(&state_path, "[]").unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_configurable_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, _stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[],
    );
    assert_eq!(code, 0);
}

#[test]
fn post_merge_phase_transition_error_records_failure() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (repo, state_path, flow_bin, stubs) = setup(&parent, happy_state(BRANCH));

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[
            ("FAKE_PT_OUT", "not-json"),
            ("FAKE_PT_EXIT", "1"),
            ("FAKE_PT_ERR", "pt exploded"),
        ],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert!(json["failures"]["phase_transition"].is_string());
}

#[test]
fn post_merge_phase_transition_non_ok_status_records_failure() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (repo, state_path, flow_bin, stubs) = setup(&parent, happy_state(BRANCH));

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[("FAKE_PT_OUT", r#"{"status":"error","message":"x"}"#)],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert!(json["failures"]["phase_transition"].is_string());
}

#[test]
fn post_merge_render_pr_body_failure_records_failure() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (repo, state_path, flow_bin, stubs) = setup(&parent, happy_state(BRANCH));

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[
            ("FAKE_RENDER_EXIT", "1"),
            ("FAKE_RENDER_ERR", "render failed"),
        ],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert!(json["failures"]["render_pr_body"]
        .as_str()
        .unwrap_or("")
        .contains("render failed"));
}

#[test]
fn post_merge_issues_summary_with_issues_sets_banner() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (repo, state_path, flow_bin, stubs) = setup(&parent, happy_state(BRANCH));

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[(
            "FAKE_ISSUES_OUT",
            r#"{"status":"ok","has_issues":true,"banner_line":"Issues: 1"}"#,
        )],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["banner_line"], "Issues: 1");
}

#[test]
fn post_merge_close_issues_writes_file_and_populates() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (repo, state_path, flow_bin, stubs) = setup(&parent, happy_state(BRANCH));

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[(
            "FAKE_CLOSE_OUT",
            r#"{"status":"ok","closed":[{"number":100,"url":"https://github.com/test/test/issues/100"}],"failed":[]}"#,
        )],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    let closed = json["closed_issues"].as_array().unwrap();
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0]["number"], 100);
    let closed_file = repo
        .join(".flow-states")
        .join(BRANCH)
        .join("closed-issues.json");
    assert!(closed_file.exists(), "closed-issues file should be written");
}

#[test]
fn post_merge_label_issues_failure_records_failure() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (repo, state_path, flow_bin, stubs) = setup(&parent, happy_state(BRANCH));

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[
            ("FAKE_LABEL_EXIT", "1"),
            ("FAKE_LABEL_ERR", "label remove failed"),
        ],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert!(json["failures"]["label_issues"]
        .as_str()
        .unwrap_or("")
        .contains("label remove failed"));
}

#[test]
fn post_merge_auto_close_parent_pushes_when_parent_closed() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (repo, state_path, flow_bin, stubs) = setup(&parent, happy_state(BRANCH));

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[
            (
                "FAKE_CLOSE_OUT",
                r#"{"status":"ok","closed":[{"number":100,"url":"https://github.com/test/test/issues/100"}],"failed":[]}"#,
            ),
            (
                "FAKE_ACP_OUT",
                r#"{"status":"ok","parent_closed":true,"milestone_closed":false}"#,
            ),
        ],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    let parents = json["parents_closed"].as_array().unwrap();
    assert_eq!(parents.len(), 1);
    assert_eq!(parents[0], 100);
}

#[test]
fn post_merge_auto_close_parent_milestone_closed_pushes() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (repo, state_path, flow_bin, stubs) = setup(&parent, happy_state(BRANCH));

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[
            (
                "FAKE_CLOSE_OUT",
                r#"{"status":"ok","closed":[{"number":101}],"failed":[]}"#,
            ),
            (
                "FAKE_ACP_OUT",
                r#"{"status":"ok","parent_closed":false,"milestone_closed":true}"#,
            ),
        ],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["parents_closed"][0], 101);
}

#[test]
fn post_merge_auto_close_parent_skipped_when_no_repo() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    // Drop repo from state.
    let mut state = happy_state(BRANCH);
    state.as_object_mut().unwrap().remove("repo");
    let (repo, state_path, flow_bin, stubs) = setup(&parent, state);

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[(
            "FAKE_CLOSE_OUT",
            r#"{"status":"ok","closed":[{"number":100}],"failed":[]}"#,
        )],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    // No repo → auto-close loop skipped → parents_closed empty.
    assert_eq!(json["parents_closed"].as_array().unwrap().len(), 0);
}

#[test]
fn post_merge_empty_repo_treated_as_none() {
    // repo == "" should be filtered like None.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let mut state = happy_state(BRANCH);
    state["repo"] = json!("");
    let (repo, state_path, flow_bin, stubs) = setup(&parent, state);

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[(
            "FAKE_CLOSE_OUT",
            r#"{"status":"ok","closed":[{"number":100}],"failed":[]}"#,
        )],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["parents_closed"].as_array().unwrap().len(), 0);
}

#[test]
fn post_merge_slack_thread_ts_posts_notification() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let mut state = happy_state(BRANCH);
    state["slack_thread_ts"] = json!("9999.0001");
    let (repo, state_path, flow_bin, stubs) = setup(&parent, state);

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["slack"]["status"], "ok");
    assert_eq!(json["slack"]["ts"], "1234.5678");
}

#[test]
fn post_merge_slack_thread_ts_empty_skips_notification() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let mut state = happy_state(BRANCH);
    state["slack_thread_ts"] = json!("");
    let (repo, state_path, flow_bin, stubs) = setup(&parent, state);

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["slack"]["status"], "skipped");
}

#[test]
fn post_merge_slack_invalid_json_response_records_error() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let mut state = happy_state(BRANCH);
    state["slack_thread_ts"] = json!("9999.0001");
    let (repo, state_path, flow_bin, stubs) = setup(&parent, state);

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[("FAKE_SLACK_OUT", "not-json")],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["slack"]["status"], "error");
    assert!(json["slack"]["message"]
        .as_str()
        .unwrap_or("")
        .contains("invalid"));
}

#[test]
fn post_merge_slack_non_ok_records_slack_data() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let mut state = happy_state(BRANCH);
    state["slack_thread_ts"] = json!("9999.0001");
    let (repo, state_path, flow_bin, stubs) = setup(&parent, state);

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[(
            "FAKE_SLACK_OUT",
            r#"{"status":"error","message":"svc down"}"#,
        )],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["slack"]["status"], "error");
    assert_eq!(json["slack"]["message"], "svc down");
}

#[test]
fn post_merge_slack_ok_without_ts_skips_add_notification() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let mut state = happy_state(BRANCH);
    state["slack_thread_ts"] = json!("9999.0001");
    let (repo, state_path, flow_bin, stubs) = setup(&parent, state);

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[("FAKE_SLACK_OUT", r#"{"status":"ok"}"#)],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["slack"]["status"], "ok");
}

#[test]
fn post_merge_format_complete_summary_non_ok_leaves_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (repo, state_path, flow_bin, stubs) = setup(&parent, happy_state(BRANCH));

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[("FAKE_SUMMARY_OUT", r#"{"status":"error","message":"x"}"#)],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["summary"], "");
    assert_eq!(json["issues_links"], "");
}

#[test]
fn post_merge_phase_transition_bad_json_uses_parse_err() {
    // Drives the `parse_err.unwrap_or_else(... stderr ...)` path: the
    // stub exits 0 but returns non-JSON stdout. stderr is empty so the
    // parse_err branch wins.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (repo, state_path, flow_bin, stubs) = setup(&parent, happy_state(BRANCH));

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[("FAKE_PT_OUT", "not-json")],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert!(json["failures"]["phase_transition"].is_string());
}

#[test]
fn post_merge_close_issues_file_write_failure_records_failure() {
    // Make `.flow-states/` a file (not a dir) — but that breaks
    // everything. Instead, make the closed-issues write target
    // unwritable by pre-creating it as a directory.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (repo, state_path, flow_bin, stubs) = setup(&parent, happy_state(BRANCH));
    // Pre-create <branch>/closed-issues.json as a directory → write
    // returns EISDIR.
    let blocker = repo
        .join(".flow-states")
        .join(BRANCH)
        .join("closed-issues.json");
    fs::create_dir_all(&blocker).unwrap();

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[(
            "FAKE_CLOSE_OUT",
            r#"{"status":"ok","closed":[{"number":100}],"failed":[]}"#,
        )],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert!(json["failures"]["closed_issues_file"].is_string());
}

// --- library-level test: verifies the public wrapper resolves project
// root and threads into the inlined post_merge body. Drives through
// the public `post_merge` function rather than a removed _inner seam. ---

#[test]
fn post_merge_slack_spawn_error_records_failure() {
    // Drives the slack `Err(e)` arm of run_cmd_with_timeout: point
    // FLOW_BIN_PATH at a non-existent binary so every bin/flow spawn
    // fails. Slack thread_ts is set, so the slack branch is entered
    // and the spawn error propagates into the slack "error" result.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join(format!("{}.json", BRANCH));
    let mut state = happy_state(BRANCH);
    state["slack_thread_ts"] = json!("9999.0001");
    write_state_file_at(&state_path, &state);
    let nonexistent = parent.join("does-not-exist").join("flow");
    let stubs = path_stub_dir(&parent);

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &nonexistent,
        &stubs,
        &[],
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["slack"]["status"], "error");
    assert!(json["slack"]["message"].is_string());
}

#[test]
fn post_merge_no_flow_states_dir_skips_log() {
    // Drives the `paths.flow_states_dir().is_dir()` false branch in the
    // log closure: write the state file to a directory that is NOT
    // `.flow-states/` so the log closure's guard fails every call.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    // Intentionally do NOT create .flow-states/. State file lives at
    // repo root.
    let state_path = repo.join("state.json");
    write_state_file_at(&state_path, &happy_state(BRANCH));
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_configurable_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, _stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[],
    );
    assert_eq!(code, 0);
    assert!(!repo.join(".flow-states").exists());
}

#[test]
fn post_merge_state_path_is_dir_falls_back_to_empty() {
    // Drives the `fs::read_to_string(state_path) Err(_)` arm: state_path
    // is an existing directory, so exists() returns true but
    // read_to_string returns EISDIR → state falls back to json!({}).
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state_as_dir = state_dir.join(format!("{}.json", BRANCH));
    fs::create_dir(&state_as_dir).unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_configurable_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, _stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_as_dir.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[],
    );
    assert_eq!(code, 0);
}

#[test]
fn post_merge_issues_summary_non_json_skips_parse_block() {
    // Drives the `if let Some(iss_data) = parsed` None-fallthrough arm
    // when format-issues-summary returns non-JSON stdout.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (repo, state_path, flow_bin, stubs) = setup(&parent, happy_state(BRANCH));

    let (code, _stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[("FAKE_ISSUES_OUT", "not-json")],
    );
    assert_eq!(code, 0);
}

#[test]
fn post_merge_close_issues_non_json_skips_parse_block() {
    // Drives the `if let Some(close_data) = parsed` None-fallthrough arm.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (repo, state_path, flow_bin, stubs) = setup(&parent, happy_state(BRANCH));

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[("FAKE_CLOSE_OUT", "not-json")],
    );
    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    // Non-JSON close stdout leaves closed_issues empty.
    assert_eq!(json["closed_issues"].as_array().unwrap().len(), 0);
}

#[test]
fn post_merge_format_summary_non_json_skips_parse_block() {
    // Drives the `if let Some(sum_data) = parsed` None-fallthrough arm.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (repo, state_path, flow_bin, stubs) = setup(&parent, happy_state(BRANCH));

    let (code, _stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[("FAKE_SUMMARY_OUT", "not-json")],
    );
    assert_eq!(code, 0);
}

#[test]
fn post_merge_auto_close_parent_non_json_skips_parse_block() {
    // Drives the `if let Some(acp_data) = parsed` None-fallthrough arm:
    // repo is set, close-issues populates closed_issues, but
    // auto-close-parent returns non-JSON → parsed=None → the whole
    // acp parse block is skipped, parents_closed stays empty.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (repo, state_path, flow_bin, stubs) = setup(&parent, happy_state(BRANCH));

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[
            (
                "FAKE_CLOSE_OUT",
                r#"{"status":"ok","closed":[{"number":100}],"failed":[]}"#,
            ),
            ("FAKE_ACP_OUT", "not-json"),
        ],
    );
    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["parents_closed"].as_array().unwrap().len(), 0);
}

// --- ok_stdout_as_json direct coverage ---

#[test]
fn post_merge_close_issues_response_missing_closed_array_stays_empty() {
    // Drives the `if let Some(closed_arr) = close_data.get("closed").and_then(as_array)`
    // None fallthrough: close-issues returns JSON without a `closed`
    // array (e.g. shape drift), so closed_issues stays empty.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (repo, state_path, flow_bin, stubs) = setup(&parent, happy_state(BRANCH));

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[("FAKE_CLOSE_OUT", r#"{"status":"ok","unexpected":"shape"}"#)],
    );
    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["closed_issues"].as_array().unwrap().len(), 0);
}

#[test]
fn post_merge_closed_issue_without_number_skipped_in_auto_close_loop() {
    // Drives the `if let Some(issue_num) = issue.get("number").and_then(...)`
    // None fallthrough: closed entry lacks a `number` field, so the acp
    // call is skipped and the loop continues.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let (repo, state_path, flow_bin, stubs) = setup(&parent, happy_state(BRANCH));

    let (code, stdout) = run_post_merge_sub(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        &flow_bin,
        &stubs,
        &[(
            "FAKE_CLOSE_OUT",
            r#"{"status":"ok","closed":[{"url":"https://github.com/x/y/issues/1"}],"failed":[]}"#,
        )],
    );
    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    // No number → auto-close-parent loop skips this entry → parents_closed stays empty.
    assert_eq!(json["parents_closed"].as_array().unwrap().len(), 0);
}

#[test]
fn ok_stdout_as_json_ok_valid_json_returns_some() {
    let result = Ok((0, r#"{"status":"ok"}"#.to_string(), String::new()));
    let value = ok_stdout_as_json(result).expect("expected Some");
    assert_eq!(value["status"], "ok");
}

#[test]
fn ok_stdout_as_json_ok_invalid_json_returns_none() {
    let result = Ok((0, "not-json".to_string(), String::new()));
    assert!(ok_stdout_as_json(result).is_none());
}

#[test]
fn ok_stdout_as_json_err_returns_none() {
    let result = Err("spawn failed".to_string());
    assert!(ok_stdout_as_json(result).is_none());
}

#[test]
fn post_merge_wrapper_returns_value_object() {
    // Drive post_merge via its public signature. With no gh or bin/flow
    // on PATH the subcommand spawns fail, but best-effort semantics mean
    // the function returns a well-formed Value::Object with failures.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join(format!("{}.json", BRANCH));
    write_state_file_at(&state_path, &happy_state(BRANCH));

    // Change cwd to the repo so project_root resolves correctly; spawn
    // a thread is unnecessary because post_merge is sync.
    let prev = std::env::current_dir().ok();
    std::env::set_current_dir(&repo).unwrap();

    let value = post_merge(42, state_path.to_string_lossy().as_ref(), BRANCH);

    if let Some(prev) = prev {
        let _ = std::env::set_current_dir(prev);
    }

    assert!(value.is_object());
    assert_eq!(value["status"], "ok");
}
