//! Subprocess integration tests for `bin/flow complete-finalize`.
//!
//! post_merge_inner and run_impl_with_deps seams were removed; the
//! module now runs post-merge and cleanup inline. Tests drive the
//! public `run_impl` via the compiled binary with fixtures that
//! control bin/flow stub behavior (so post_merge's failures map is
//! populated on broken subprocesses) and `.flow-states/` layout (so
//! the log-closure existence branch flips).

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

mod common;

const BRANCH: &str = "test-feature";
const SLASH_BRANCH: &str = "feature/foo";

fn make_repo_fixture(parent: &Path) -> PathBuf {
    let repo = common::create_git_repo_with_remote(parent);
    repo.canonicalize().expect("canonicalize repo")
}

/// Make a fixture WITHOUT `refs/remotes/origin/HEAD` so
/// `default_branch_in` returns Err and the fail-closed error envelope
/// path runs.
fn make_repo_fixture_no_origin_head(parent: &Path) -> PathBuf {
    let repo = parent.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let run = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
    };
    run(&["init", "-b", "main"]);
    run(&["config", "user.email", "t@t.com"]);
    run(&["config", "user.name", "T"]);
    run(&["config", "commit.gpgsign", "false"]);
    run(&["commit", "--allow-empty", "-m", "init"]);
    repo.canonicalize().expect("canonicalize repo")
}

#[test]
fn complete_finalize_errors_when_default_branch_resolve_fails() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture_no_origin_head(&parent);
    let state_path = write_state_file(&repo, BRANCH, true);
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (_code, stdout, _stderr) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        false,
        Some(&flow_bin),
        Some(&stubs),
    );
    let value = last_json_line(&stdout);
    assert_eq!(value["status"], "error");
    assert_eq!(value["step"], "resolve_base_branch");
    assert!(
        value["message"]
            .as_str()
            .unwrap_or("")
            .contains("symbolic-ref"),
        "expected resolve failure message naming git symbolic-ref, got: {}",
        value
    );
}

fn write_state_file(repo: &Path, branch: &str, create_flow_states_dir: bool) -> PathBuf {
    let branch_dir = repo.join(".flow-states").join(branch);
    let state_path = branch_dir.join("state.json");
    if create_flow_states_dir {
        fs::create_dir_all(&branch_dir).unwrap();
        let state = common::make_complete_state(branch, "complete", None);
        fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    }
    state_path
}

/// bin/flow stub that returns valid JSON for complete-finalize's
/// downstream subcommands (phase-transition, render-pr-body, etc.)
/// so post_merge does not accumulate failures. Used for happy-path
/// subprocess tests.
fn write_success_flow_stub(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let script = r#"#!/bin/sh
case "$1" in
    phase-transition)
        printf '%s' '{"status":"ok","formatted_time":"1m","cumulative_seconds":60}'
        ;;
    render-pr-body|label-issues|add-notification)
        ;;
    format-issues-summary)
        printf '%s' '{"status":"ok","has_issues":false}'
        ;;
    close-issues)
        printf '%s' '{"status":"ok","closed":[],"failed":[]}'
        ;;
    format-complete-summary)
        printf '%s' '{"status":"ok","summary":"done","issues_links":""}'
        ;;
    auto-close-parent)
        printf '%s' '{"status":"ok","closed_issues":[],"milestone_closed":false}'
        ;;
    notify-slack)
        printf '%s' '{"status":"ok","ts":"1234.5678"}'
        ;;
    *)
        ;;
esac
exit 0
"#;
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn path_stub_dir(parent: &Path) -> PathBuf {
    let stubs = parent.join("stubs");
    fs::create_dir_all(&stubs).unwrap();
    let gh = stubs.join("gh");
    fs::write(&gh, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
    stubs
}

/// Seed passing `bin/{format,lint,build,test}` scripts (each `exit 0`,
/// no `FLOW-STUB-UNCONFIGURED` marker) into `repo/bin/` so the
/// sentinel-gated `ci::run_impl` complete-finalize runs against the
/// integration branch after a clean `--pull` actually executes tools
/// and passes — writing the base-branch sentinel only on a real CI
/// pass. Without these scripts `bin_tool_sequence` is empty and CI
/// fails (the empty-tools error), so the sentinel is never written.
fn write_passing_bin_stubs(repo: &Path) {
    let bin_dir = repo.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    for tool in &["format", "lint", "build", "test"] {
        let p = bin_dir.join(tool);
        fs::write(&p, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[allow(clippy::too_many_arguments)]
fn run_complete_finalize(
    repo: &Path,
    pr: &str,
    state_file: &str,
    branch: &str,
    worktree: &str,
    pull: bool,
    flow_bin_path: Option<&Path>,
    path_stubs: Option<&Path>,
) -> (i32, String, String) {
    let current_path = std::env::var("PATH").unwrap_or_default();
    let new_path = if let Some(stubs) = path_stubs {
        format!("{}:{}", stubs.display(), current_path)
    } else {
        current_path
    };
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.args([
        "complete-finalize",
        "--pr",
        pr,
        "--state-file",
        state_file,
        "--branch",
        branch,
        "--worktree",
        worktree,
    ])
    .current_dir(repo)
    .env("PATH", new_path)
    .env_remove("FLOW_CI_RUNNING");
    if let Some(p) = flow_bin_path {
        cmd.env("FLOW_BIN_PATH", p);
    }
    if pull {
        cmd.arg("--pull");
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

#[test]
fn finalize_happy_path_no_failures() {
    // Happy path: bin/flow stub returns valid JSON for every subcommand
    // so post_merge's failures map stays empty → post_merge_failures
    // field absent on the outer result.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = write_state_file(&repo, BRANCH, true);
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, stdout, _) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        false,
        Some(&flow_bin),
        Some(&stubs),
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["formatted_time"], "1m");
    assert_eq!(json["cumulative_seconds"], 60);
    assert_eq!(json["summary"], "done");
    assert!(json.get("post_merge_failures").is_none());
    assert!(json.get("cleanup").is_some());
}

#[test]
fn finalize_with_broken_flow_stubs_populates_post_merge_failures() {
    // No FLOW_BIN_PATH / PATH stubs → every subcommand spawn or call
    // fails → post_merge records entries in its `failures` map →
    // outer result carries `post_merge_failures` with at least one key.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = write_state_file(&repo, BRANCH, true);
    let nonexistent = parent.join("does-not-exist").join("flow");

    let (code, stdout, _) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        false,
        Some(&nonexistent),
        None,
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    let failures = json
        .get("post_merge_failures")
        .and_then(|v| v.as_object())
        .expect("post_merge_failures must be populated when subprocesses fail");
    assert!(
        !failures.is_empty(),
        "failures map should have at least one key; got: {:?}",
        failures
    );
}

/// The log closure inside `complete_finalize::run_impl` writes the
/// pre-cleanup "starting" line while the branch directory still
/// exists, then attempts a post-cleanup "done" line. The closure's
/// guard is scoped to `branch_dir().is_dir()` — narrower than the
/// parent `.flow-states/` directory which survives cleanup —
/// because a parent-scoped guard would let `append_log` call
/// `ensure_branch_dir()` and recreate the directory cleanup just
/// removed, silently undoing the removal step. On the happy path
/// the post-cleanup call is therefore a no-op; the partial-failure
/// path (cleanup leaves the branch directory intact) is the only
/// case where the "done" line lands on disk.
///
/// This test exercises the full subprocess path with a fixture that
/// pre-creates the branch directory. After the call returns, the
/// branch directory must NOT exist on disk: cleanup removed it and
/// no subsequent code path may resurrect it. The pre-cleanup
/// "starting" log line still fires while the branch directory
/// exists, but its file is removed alongside the branch directory.
/// The cleanup-side audit trail (the `[Phase 5] cleanup — in progress`
/// line) is covered by `tests/cleanup.rs` tests against the cleanup
/// module directly, and the cleanup result is always available to
/// callers via the JSON `cleanup` envelope `complete_finalize`
/// returns.
#[test]
fn complete_finalize_does_not_recreate_branch_dir_after_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = write_state_file(&repo, BRANCH, true);
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let branch_dir = repo.join(".flow-states").join(BRANCH);
    assert!(
        branch_dir.exists(),
        "fixture must pre-create the branch dir at {}",
        branch_dir.display()
    );

    let (code, _, _) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        false,
        Some(&flow_bin),
        Some(&stubs),
    );

    assert_eq!(code, 0);
    assert!(
        !branch_dir.exists(),
        "branch directory must not exist after cleanup; found: {}",
        branch_dir.display()
    );
}

#[test]
fn finalize_log_closure_skips_when_flow_states_dir_missing() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    // State file outside .flow-states/; directory is NOT created.
    let state_path = repo.join("external-state.json");
    let state = common::make_complete_state(BRANCH, "complete", None);
    fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, _, _) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        false,
        Some(&flow_bin),
        Some(&stubs),
    );

    assert_eq!(code, 0);
    // complete-finalize's log file should NOT exist. complete_post_merge
    // may have created .flow-states/ when writing its own artifacts, but
    // the log FILE is the specific assertion.
    let log_path = repo.join(".flow-states").join(BRANCH).join("log");
    assert!(
        !log_path.exists(),
        "log closure must skip logging when .flow-states/ is missing at entry; found: {}",
        log_path.display()
    );
}

#[test]
fn finalize_slash_branch_does_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = repo.join("external-state.json");
    let state = common::make_complete_state(SLASH_BRANCH, "complete", None);
    fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, _, stderr) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        SLASH_BRANCH,
        ".worktrees/feature-foo",
        false,
        Some(&flow_bin),
        Some(&stubs),
    );

    assert_eq!(code, 0);
    assert!(
        !stderr.contains("panicked at"),
        "slash branch triggered a Rust panic: stderr={}",
        stderr
    );
}

/// `complete_finalize` writes the account-window snapshot to BOTH
/// the top-level `window_at_complete` field AND the phase-scoped
/// `phases.flow-complete.window_at_complete` field. Without the dual
/// write, `format_complete_summary`'s `phase_delta` short-circuits
/// for flow-complete because it reads `phase.window_at_complete`,
/// leaving the Complete row in the Token Cost section as a
/// placeholder.
///
/// Uses an external state-file path so the per-branch cleanup pass
/// at the end of `complete_finalize` does not wipe the file before
/// the post-run assertions.
#[test]
fn complete_finalize_writes_phase_scoped_window_at_complete_for_flow_complete() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = repo.join("external-state.json");
    let state = common::make_complete_state(BRANCH, "complete", None);
    fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, _, _) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        false,
        Some(&flow_bin),
        Some(&stubs),
    );

    assert_eq!(code, 0);

    let content = fs::read_to_string(&state_path).expect("state file must survive");
    let state: Value = serde_json::from_str(&content).expect("state must parse");

    let top_level = &state["window_at_complete"];
    assert!(
        top_level.is_object(),
        "top-level window_at_complete must be populated; got: {}",
        top_level
    );

    let phase_scoped = &state["phases"]["flow-complete"]["window_at_complete"];
    assert!(
        phase_scoped.is_object(),
        "phases.flow-complete.window_at_complete must be populated alongside the top-level write; got: {}",
        phase_scoped
    );

    // Both writes share the same snapshot — captured_at and
    // session_id must match because they came from the same
    // `capture_for_active_state` call inside the same mutate_state
    // closure.
    assert_eq!(
        top_level["captured_at"], phase_scoped["captured_at"],
        "top-level and phase-scoped writes must share captured_at; top: {} phase: {}",
        top_level["captured_at"], phase_scoped["captured_at"]
    );
    assert_eq!(
        top_level["session_id"], phase_scoped["session_id"],
        "top-level and phase-scoped writes must share session_id; top: {} phase: {}",
        top_level["session_id"], phase_scoped["session_id"]
    );
}

/// Adversarial guard: when the on-disk state file has `phases`
/// as a non-object value (hand-edited or corrupted), the new
/// phase-scoped write must heal the path instead of panicking
/// on the chained IndexMut. Reference: per-level object guards
/// per `.claude/rules/rust-patterns.md` "State Mutation Object
/// Guards".
#[test]
fn complete_finalize_heals_non_object_phases_without_panic() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = repo.join("external-state.json");
    // `phases` is a JSON array — IndexMut on a string key would
    // panic without the guard.
    let state = json!({
        "schema_version": 1,
        "branch": BRANCH,
        "repo": "test/test",
        "pr_number": 42,
        "phases": [],
    });
    fs::write(&state_path, state.to_string()).unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, _stdout, stderr) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        false,
        Some(&flow_bin),
        Some(&stubs),
    );

    assert_eq!(code, 0);
    assert!(
        !stderr.contains("panicked at"),
        "complete-finalize must not panic when phases is non-object; stderr={}",
        stderr
    );
    // The healed phases object now carries the snapshot.
    let content = fs::read_to_string(&state_path).expect("state must survive");
    let state: Value = serde_json::from_str(&content).expect("state must parse");
    assert!(
        state["phases"]["flow-complete"]["window_at_complete"].is_object(),
        "guard must heal phases and write the snapshot; got: {}",
        state["phases"]
    );
}

/// Adversarial guard: when `phases.flow-complete` is a non-object
/// value (e.g. a hand-edited state file flattened the phase entry
/// to a string), the new write must heal the inner level too. The
/// outer `phases` is still a valid object here so the first guard
/// is a no-op; the second guard does the healing.
#[test]
fn complete_finalize_heals_non_object_flow_complete_without_panic() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = repo.join("external-state.json");
    let state = json!({
        "schema_version": 1,
        "branch": BRANCH,
        "repo": "test/test",
        "pr_number": 42,
        "phases": {
            "flow-start": {"status": "complete"},
            "flow-code": {"status": "complete"},
            "flow-review": {"status": "complete"},
            "flow-learn": {"status": "complete"},
            "flow-complete": "stringified-phase-state",
        },
    });
    fs::write(&state_path, state.to_string()).unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, _stdout, stderr) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        false,
        Some(&flow_bin),
        Some(&stubs),
    );

    assert_eq!(code, 0);
    assert!(
        !stderr.contains("panicked at"),
        "complete-finalize must not panic when phases.flow-complete is non-object; stderr={}",
        stderr
    );
    let content = fs::read_to_string(&state_path).expect("state must survive");
    let state: Value = serde_json::from_str(&content).expect("state must parse");
    assert!(
        state["phases"]["flow-complete"]["window_at_complete"].is_object(),
        "guard must heal flow-complete and write the snapshot; got: {}",
        state["phases"]["flow-complete"]
    );
}

/// Adversarial guard: when the entire state root is non-object
/// (hand edit replaced the JSON object with an array or scalar),
/// the outer `state.is_object()` guard must skip the write block
/// entirely. The pre-existing `write_snapshot_into_state` helper
/// already guards on `state.as_object_mut()` and silently no-ops,
/// so the closure runs without side effects.
#[test]
fn complete_finalize_skips_non_object_state_without_panic() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = repo.join("external-state.json");
    // State root is a JSON array — both write_snapshot_into_state
    // and the new chained IndexMut must skip without panic.
    fs::write(&state_path, "[1,2,3]").unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, _stdout, stderr) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        false,
        Some(&flow_bin),
        Some(&stubs),
    );

    assert_eq!(code, 0);
    assert!(
        !stderr.contains("panicked at"),
        "complete-finalize must not panic when state root is non-object; stderr={}",
        stderr
    );
}

#[test]
fn finalize_pull_flag_threads_to_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = write_state_file(&repo, BRANCH, true);
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, stdout, _) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        true,
        Some(&flow_bin),
        Some(&stubs),
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    let cleanup = json
        .get("cleanup")
        .and_then(|v| v.as_object())
        .expect("cleanup map must be present");
    let _ = cleanup;
}

// --- run_impl: integration-branch CI after clean pull ---

/// `--pull` was passed, `git pull origin main` succeeded, and the
/// integration-branch CI passed — `ci::run_impl` runs format/lint/
/// build/test and writes the base-branch sentinel only on the real
/// pass, so the next `start-gate` can skip CI. Asserts the sentinel
/// exists at the canonical path (`<root>/.flow-states/main-ci-passed`)
/// and that no `base_ci` failure field is surfaced.
#[test]
fn complete_finalize_writes_sentinel_after_clean_pull_ci_pass() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    write_passing_bin_stubs(&repo);
    let state_path = write_state_file(&repo, BRANCH, true);
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let sentinel_path = flow_rs::ci::sentinel_path(&repo, "main");
    assert!(
        !sentinel_path.exists(),
        "sentinel must not exist before complete-finalize runs"
    );

    let (code, stdout, _) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        true, // --pull
        Some(&flow_bin),
        Some(&stubs),
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(
        json["cleanup"]["git_pull"], "pulled",
        "fixture must produce a clean pull: {}",
        json
    );

    assert!(
        sentinel_path.exists(),
        "sentinel must exist at {} after a real CI pass on the base branch",
        sentinel_path.display()
    );
    assert!(
        json.get("base_ci").is_none(),
        "no base_ci failure field when integration-branch CI passes; got: {}",
        json
    );
}

/// `--pull` succeeded but the integration-branch CI failed — no
/// `bin/*` tool scripts are present, so `ci::run_impl` hits the
/// empty-tools error. The failure is surfaced in the result's
/// `base_ci` field WITHOUT erroring the finalize (the squash merge
/// already landed upstream and cannot be rolled back) and WITHOUT
/// writing the green sentinel.
#[test]
fn complete_finalize_clean_pull_ci_fail_surfaces_base_ci() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    // No bin/* scripts → ci::run_impl empty-tools error → CI fails.
    let state_path = write_state_file(&repo, BRANCH, true);
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let sentinel_path = flow_rs::ci::sentinel_path(&repo, "main");

    let (code, stdout, _) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        true, // --pull
        Some(&flow_bin),
        Some(&stubs),
    );

    // The finalize itself does not error — the merge already landed.
    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    assert_eq!(
        json["cleanup"]["git_pull"], "pulled",
        "fixture must produce a clean pull: {}",
        json
    );
    let base_ci = json
        .get("base_ci")
        .expect("base_ci field must surface the integration-branch CI failure");
    assert_eq!(base_ci["status"], "failed");
    assert!(
        !sentinel_path.exists(),
        "no green sentinel may be written when integration-branch CI fails"
    );
}

/// `--pull` flag was NOT set, so no pull happened. We don't know
/// main's state, so no integration-branch CI runs: no sentinel is
/// written and no `base_ci` field appears.
#[test]
fn complete_finalize_no_sentinel_when_pull_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = write_state_file(&repo, BRANCH, true);
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, stdout, _) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        false, // --pull NOT set
        Some(&flow_bin),
        Some(&stubs),
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert!(
        json.get("base_ci").is_none(),
        "no base_ci when --pull is unset (CI does not run); got: {}",
        json
    );
    let sentinel_path = flow_rs::ci::sentinel_path(&repo, "main");
    assert!(
        !sentinel_path.exists(),
        "sentinel must NOT exist when --pull is unset"
    );
}

/// `--pull` succeeds but the integration branch (`origin/HEAD`)
/// resolves to a slash-containing value like `feature/foo`. Without
/// an `is_valid_branch` guard around the integration-branch CI call,
/// `ci::run_impl` would reach `sentinel_path`, which calls
/// `FlowPaths::try_new(...).expect()` and panics — surfacing a Rust
/// backtrace to the user mid-cleanup. Per
/// `.claude/rules/branch-path-safety.md`, state-derived branches must
/// pass `is_valid_branch` before reaching the panicking constructor.
/// The integration-branch CI is best-effort, so the invalid-branch
/// path skips the CI run (and therefore the sentinel) entirely; the
/// next start-gate run re-establishes the sentinel.
#[test]
fn complete_finalize_skips_sentinel_when_base_branch_is_slash_containing() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    // Push a feature/foo branch to the bare remote so the cleanup's
    // git pull origin feature/foo succeeds — without that, git_pull
    // returns "failed: ..." and the outer if-block never runs, so
    // the inner is_valid_branch guard's false branch isn't exercised.
    let _ = std::process::Command::new("git")
        .args(["push", "origin", "HEAD:refs/heads/feature/foo"])
        .current_dir(&repo)
        .output()
        .expect("push feature/foo");
    // Point origin/HEAD at the slash-containing branch so
    // `default_branch_in` returns "feature/foo" and the
    // `is_valid_branch` guard's false arm runs.
    let _ = std::process::Command::new("git")
        .args(["remote", "set-head", "origin", "feature/foo"])
        .current_dir(&repo)
        .output()
        .expect("set-head feature/foo");
    let branch_dir = repo.join(".flow-states").join(BRANCH);
    fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    let state = common::make_complete_state(BRANCH, "complete", None);
    fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, stdout, stderr) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        true, // --pull
        Some(&flow_bin),
        Some(&stubs),
    );

    assert_eq!(code, 0);
    assert!(
        !stderr.contains("panicked at"),
        "complete-finalize must not panic on slash-containing base_branch; stderr={}",
        stderr
    );
    let json = last_json_line(&stdout);
    assert_eq!(
        json["cleanup"]["git_pull"], "pulled",
        "fixture must produce a clean pull so the sentinel branch is reachable: {}",
        json
    );
    assert!(
        json.get("base_ci").is_none(),
        "the is_valid_branch guard skips the integration-branch CI entirely \
         for a slash branch, so no base_ci field is surfaced; got: {}",
        json
    );
    // No sentinel path is computable for a slash-containing branch
    // (FlowPaths::try_new returns None), so we cannot assert against
    // a canonical path. Instead assert no sentinel ever lands under
    // .flow-states/feature/ — which would only exist if the guard
    // failed and the write traversed into the slash-containing path.
    let invalid_sentinel_root = repo.join(".flow-states").join("feature");
    assert!(
        !invalid_sentinel_root.exists(),
        "guard must skip sentinel write entirely for slash-containing \
         base_branch; found unexpected path: {}",
        invalid_sentinel_root.display()
    );
}

/// `--pull` was passed but the bare remote was removed before
/// complete-finalize ran, so `git pull` fails. The sentinel must
/// not be written when the pull did not complete cleanly — main's
/// local state may be stale or inconsistent with what CI tested.
#[test]
fn complete_finalize_no_sentinel_when_pull_failed() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = write_state_file(&repo, BRANCH, true);
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    // Remove the bare remote so `git pull origin main` fails.
    fs::remove_dir_all(parent.join("bare.git")).expect("remove bare remote");

    let (code, stdout, _) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        true, // --pull
        Some(&flow_bin),
        Some(&stubs),
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    let git_pull = json["cleanup"]["git_pull"].as_str().unwrap_or("");
    assert!(
        git_pull.starts_with("failed"),
        "fixture must produce a failed pull: got {}",
        git_pull
    );

    let sentinel_path = flow_rs::ci::sentinel_path(&repo, "main");
    assert!(
        !sentinel_path.exists(),
        "sentinel must NOT exist when pull failed"
    );
}

#[test]
fn finalize_has_failures_ok_status_absent_failures() {
    // post_merge returns no failures → post_merge_failures absent →
    // effective_status == "ok" on the log line. Drive through the
    // success stub.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = write_state_file(&repo, BRANCH, true);
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, stdout, _) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        false,
        Some(&flow_bin),
        Some(&stubs),
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    assert!(json.get("post_merge_failures").is_none());
}

/// Covers the `if state_path.exists() { ... }` false branch in
/// run_impl. The --state-file points at a path that doesn't exist on
/// disk, so the snap-capture mutate_state call is skipped. Without
/// this test, line 85's close brace shows count 0 and the file
/// stays at <100% per-region/per-line coverage.
#[test]
fn finalize_state_file_missing_skips_snapshot_capture() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    // state_file points at a path that does NOT exist on disk.
    let state_path = parent.join("nonexistent-state.json");
    assert!(!state_path.exists());
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, _stdout, _) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        false,
        Some(&flow_bin),
        Some(&stubs),
    );

    // Subprocess exits 0 — missing state file is not an error here;
    // run_impl just skips the snapshot capture and proceeds with
    // post_merge + cleanup using whatever fallbacks they have.
    assert_eq!(code, 0);
}

/// Rejection arm: spawning complete-finalize with cwd equal to the
/// worktree root must trigger the cwd-inside-worktree guard and
/// return a structured error before any side effect runs.
#[test]
fn complete_finalize_rejects_cwd_inside_worktree() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = write_state_file(&repo, BRANCH, true);
    let worktree_abs = repo.join(".worktrees").join("test-feature");
    fs::create_dir_all(&worktree_abs).unwrap();
    let worktree_canon = worktree_abs.canonicalize().unwrap();

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.args([
        "complete-finalize",
        "--pr",
        "1",
        "--state-file",
        state_path.to_string_lossy().as_ref(),
        "--branch",
        BRANCH,
        "--worktree",
        worktree_canon.to_string_lossy().as_ref(),
    ])
    .current_dir(&worktree_canon)
    .env_remove("FLOW_CI_RUNNING")
    .env("GH_TOKEN", "invalid")
    .env("HOME", &parent);
    let output = cmd.output().expect("spawn flow-rs");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert_eq!(json["reason"], "cwd_inside_worktree");
    let msg = json["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("cd to"),
        "message should name the project root to cd to; got: {}",
        msg
    );
    assert!(
        worktree_abs.exists(),
        "worktree must still exist when guard rejects; the guard runs before cleanup"
    );
}

/// Descendant arm: a nested subdirectory of the worktree must also
/// trigger the guard via the prefix check (cwd starts_with worktree
/// after canonicalization).
#[test]
fn complete_finalize_rejects_cwd_in_nested_subdir_of_worktree() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = write_state_file(&repo, BRANCH, true);
    let worktree_abs = repo.join(".worktrees").join("test-feature");
    let nested = worktree_abs.join("nested");
    fs::create_dir_all(&nested).unwrap();
    let worktree_canon = worktree_abs.canonicalize().unwrap();
    let nested_canon = nested.canonicalize().unwrap();

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.args([
        "complete-finalize",
        "--pr",
        "1",
        "--state-file",
        state_path.to_string_lossy().as_ref(),
        "--branch",
        BRANCH,
        "--worktree",
        worktree_canon.to_string_lossy().as_ref(),
    ])
    .current_dir(&nested_canon)
    .env_remove("FLOW_CI_RUNNING")
    .env("GH_TOKEN", "invalid")
    .env("HOME", &parent);
    let output = cmd.output().expect("spawn flow-rs");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert_eq!(json["reason"], "cwd_inside_worktree");
}

/// Pass-through arm: cwd at the project root (which is NOT inside
/// the worktree) must NOT trigger the guard. Downstream errors are
/// acceptable; the test only asserts the guard's reason string is
/// absent so the binary proceeded past the guard.
#[test]
fn complete_finalize_proceeds_when_cwd_is_project_root() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = write_state_file(&repo, BRANCH, true);
    let worktree_abs = repo.join(".worktrees").join("test-feature");
    fs::create_dir_all(&worktree_abs).unwrap();
    let worktree_canon = worktree_abs.canonicalize().unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);
    let path_with_stubs = format!(
        "{}:{}",
        stubs.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.args([
        "complete-finalize",
        "--pr",
        "1",
        "--state-file",
        state_path.to_string_lossy().as_ref(),
        "--branch",
        BRANCH,
        "--worktree",
        worktree_canon.to_string_lossy().as_ref(),
    ])
    .current_dir(&repo)
    .env_remove("FLOW_CI_RUNNING")
    .env("FLOW_BIN_PATH", &flow_bin)
    .env("PATH", &path_with_stubs)
    .env("GH_TOKEN", "invalid")
    .env("HOME", &parent);
    let output = cmd.output().expect("spawn flow-rs");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    assert!(
        !stdout.contains("\"reason\":\"cwd_inside_worktree\""),
        "guard must not false-positive when cwd is the project root; stdout={}",
        stdout
    );
}

#[test]
fn finalize_result_includes_empty_banner_and_issues_links_on_bare_state() {
    // The state file omits slack thread and has no prompt → various
    // fields in post_merge_data default to "" → outer result mirrors.
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join(format!("{}.json", BRANCH));
    // Minimal state with only branch/pr_number.
    fs::write(
        &state_path,
        json!({"branch": BRANCH, "pr_number": 42}).to_string(),
    )
    .unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_success_flow_stub(&flow_bin);
    let stubs = path_stub_dir(&parent);

    let (code, stdout, _) = run_complete_finalize(
        &repo,
        "42",
        state_path.to_string_lossy().as_ref(),
        BRANCH,
        ".worktrees/test-feature",
        false,
        Some(&flow_bin),
        Some(&stubs),
    );

    assert_eq!(code, 0);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "ok");
    assert!(json.get("issues_links").is_some());
    assert!(json.get("banner_line").is_some());
}

/// Drives the `invalid_branch` early-return in `run_impl` when
/// `--branch` fails `FlowPaths::is_valid_branch` (empty, contains
/// `/`, `.`, `..`, or NUL). The guard runs after the cwd-inside-
/// worktree check and before any cleanup, so the worktree must
/// still exist after rejection — proving the validation gate the
/// branch-path-safety rule requires.
#[test]
fn complete_finalize_rejects_invalid_branch_arg() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    // State file: write under a valid sibling branch since the
    // state file path is a CLI arg, not constrained by the branch
    // arg under test.
    let state_path = write_state_file(&repo, BRANCH, true);
    // Use a sibling worktree path so the cwd-inside-worktree guard
    // does not preempt this test.
    let worktree_abs = repo.join(".worktrees").join("sibling-feature");
    fs::create_dir_all(&worktree_abs).unwrap();

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.args([
        "complete-finalize",
        "--pr",
        "1",
        "--state-file",
        state_path.to_string_lossy().as_ref(),
        "--branch",
        // Slash-containing branch — rejected by is_valid_branch.
        SLASH_BRANCH,
        "--worktree",
        worktree_abs.to_string_lossy().as_ref(),
    ])
    .current_dir(&repo)
    .env_remove("FLOW_CI_RUNNING")
    .env("GH_TOKEN", "invalid")
    .env("HOME", &parent);
    let output = cmd.output().expect("spawn flow-rs");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert_eq!(json["reason"], "invalid_branch");
    let msg = json["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("not a valid FLOW branch name"),
        "message should name the validation failure; got: {}",
        msg
    );
    assert!(
        worktree_abs.exists(),
        "worktree must still exist when invalid_branch rejects — \
         the guard runs before cleanup"
    );
}

/// Sibling of `complete_finalize_rejects_invalid_branch_arg` with an
/// empty branch (another `is_valid_branch == false` case). Exercises
/// the same return-arm with a different rejection cause to ensure the
/// format! Debug-formatter handles empty strings as well as slash-
/// containing ones.
#[test]
fn complete_finalize_rejects_empty_branch_arg() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = make_repo_fixture(&parent);
    let state_path = write_state_file(&repo, BRANCH, true);
    let worktree_abs = repo.join(".worktrees").join("sibling-feature");
    fs::create_dir_all(&worktree_abs).unwrap();

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.args([
        "complete-finalize",
        "--pr",
        "1",
        "--state-file",
        state_path.to_string_lossy().as_ref(),
        "--branch",
        "",
        "--worktree",
        worktree_abs.to_string_lossy().as_ref(),
    ])
    .current_dir(&repo)
    .env_remove("FLOW_CI_RUNNING")
    .env("GH_TOKEN", "invalid")
    .env("HOME", &parent);
    let output = cmd.output().expect("spawn flow-rs");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert_eq!(json["reason"], "invalid_branch");
}
