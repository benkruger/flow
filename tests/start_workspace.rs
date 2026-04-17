//! Integration tests for start-workspace subcommand.
//!
//! start-workspace consolidates: worktree creation + PR creation + state
//! backfill + lock release into a single command.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

use common::{
    create_gh_stub, create_git_repo_with_remote, current_plugin_version, flow_states_dir,
    parse_output, write_flow_json,
};

// --- Test helpers ---

/// Create a default gh stub (PR create returns fake URL).
fn create_default_gh_stub(repo: &Path) -> PathBuf {
    create_gh_stub(
        repo,
        "#!/bin/bash\n\
         echo \"https://github.com/test/repo/pull/42\"\n",
    )
}

/// Set up a pre-existing state file (simulating init-state already ran).
fn create_state_file(repo: &Path, branch: &str) {
    let state_dir = flow_states_dir(repo);
    fs::create_dir_all(&state_dir).unwrap();
    let state = json!({
        "schema_version": 1,
        "branch": branch,
        "repo": null,
        "pr_number": null,
        "pr_url": null,
        "started_at": "2026-01-01T00:00:00-08:00",
        "current_phase": "flow-start",
        "files": {
            "plan": null,
            "dag": null,
            "log": format!(".flow-states/{}.log", branch),
            "state": format!(".flow-states/{}.json", branch)
        },
        "session_tty": null,
        "session_id": null,
        "transcript_path": null,
        "notes": [],
        "prompt": "test feature",
        "phases": {},
        "phase_transitions": [],
        "start_step": 2,
        "start_steps_total": 5
    });
    fs::write(
        state_dir.join(format!("{}.json", branch)),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();
}

/// Create a lock queue entry for this feature.
fn create_lock_entry(repo: &Path, feature: &str) {
    let queue_dir = flow_states_dir(repo).join("start-queue");
    fs::create_dir_all(&queue_dir).unwrap();
    fs::write(queue_dir.join(feature), "").unwrap();
}

/// Run flow-rs start-workspace.
fn run_start_workspace(repo: &Path, feature: &str, branch: &str, stub_dir: &Path) -> Output {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["start-workspace", feature, "--branch", branch])
        .current_dir(repo)
        .env(
            "PATH",
            format!(
                "{}:{}",
                stub_dir.to_string_lossy(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("CLAUDE_PLUGIN_ROOT", &manifest_dir)
        .env_remove("FLOW_SIMULATE_BRANCH")
        .output()
        .unwrap()
}

// --- Tests ---

#[test]
fn test_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);
    let stub_dir = create_default_gh_stub(&repo);
    create_state_file(&repo, "test-branch");
    // Lock entry uses branch name (what start-init creates).
    // CLI description arg is a different string (what the skill passes).
    create_lock_entry(&repo, "test-branch");

    let output = run_start_workspace(&repo, "Test Feature Title", "test-branch", &stub_dir);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["branch"], "test-branch");
    assert!(data["pr_url"].is_string());
    assert!(data["pr_number"].is_number());

    // Worktree should exist
    assert!(repo.join(".worktrees").join("test-branch").is_dir());

    // Lock should be released (keyed by branch, not by description)
    let queue_dir = flow_states_dir(&repo).join("start-queue");
    assert!(
        !queue_dir.join("test-branch").exists(),
        "Lock must be released after start-workspace"
    );

    // State file should have PR fields backfilled
    let state_path = flow_states_dir(&repo).join("test-branch.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert!(state["pr_number"].is_number());
    assert!(state["pr_url"].is_string());
}

/// Guards the contract that `release_lock` is invoked with the
/// canonical branch name, not the human-readable feature description.
/// When start-workspace is called with a description that differs
/// from the branch (a common shape: title-cased PR title vs
/// kebab-case branch), the lock file — named after the branch — must
/// still be deleted at the end of the workflow. Without this
/// guarantee, every mismatched-description run would leave an orphan
/// lock that blocks subsequent flows for the 30-minute stale timeout.
#[test]
fn test_lock_released_with_mismatched_description() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);
    let stub_dir = create_default_gh_stub(&repo);
    create_state_file(&repo, "mismatch-branch");
    // Lock acquired under branch name (by start-init)
    create_lock_entry(&repo, "mismatch-branch");

    // CLI passes human-readable title as description, branch name as --branch
    let output = run_start_workspace(
        &repo,
        "A Completely Different Human Readable Title",
        "mismatch-branch",
        &stub_dir,
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");

    // Lock must be released under the BRANCH name, not the description
    let queue_dir = flow_states_dir(&repo).join("start-queue");
    assert!(
        !queue_dir.join("mismatch-branch").exists(),
        "Lock must be released using branch name, not description"
    );
    // Verify no stale lock under the description name either
    assert!(
        !queue_dir
            .join("A Completely Different Human Readable Title")
            .exists(),
        "No lock file should exist under the description name"
    );
}

#[test]
fn test_worktree_failure_releases_lock() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);
    let stub_dir = create_default_gh_stub(&repo);
    create_state_file(&repo, "test-branch");
    // Lock under branch name (what start-init creates)
    create_lock_entry(&repo, "test-branch");

    // Create the worktree dir to make git worktree add fail
    let wt_path = repo.join(".worktrees").join("test-branch");
    fs::create_dir_all(&wt_path).unwrap();
    // Also create a branch with this name so git worktree add -b fails
    Command::new("git")
        .args(["branch", "test-branch"])
        .current_dir(&repo)
        .output()
        .unwrap();

    let output = run_start_workspace(&repo, "Fail Feature Title", "test-branch", &stub_dir);
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");

    // Lock MUST still be released on error
    let queue_dir = flow_states_dir(&repo).join("start-queue");
    assert!(
        !queue_dir.join("test-branch").exists(),
        "Lock must be released even on worktree failure"
    );
}

#[test]
fn test_pr_creation_failure_releases_lock() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);
    // gh stub that fails on pr create
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 1\n");
    create_state_file(&repo, "pr-fail-branch");
    // Lock under branch name (what start-init creates)
    create_lock_entry(&repo, "pr-fail-branch");

    let output = run_start_workspace(&repo, "PR Fail Feature Title", "pr-fail-branch", &stub_dir);
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");

    // Lock must be released
    let queue_dir = flow_states_dir(&repo).join("start-queue");
    assert!(
        !queue_dir.join("pr-fail-branch").exists(),
        "Lock must be released even on PR creation failure"
    );
}

#[test]
fn test_venv_symlinked() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);
    let stub_dir = create_default_gh_stub(&repo);
    create_state_file(&repo, "venv-branch");
    create_lock_entry(&repo, "venv-branch");

    // Create .venv dir
    let venv_dir = repo.join(".venv");
    fs::create_dir_all(venv_dir.join("bin")).unwrap();
    fs::write(venv_dir.join("bin").join("python3"), "fake").unwrap();

    let output = run_start_workspace(&repo, "Venv Feature Title", "venv-branch", &stub_dir);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let wt_venv = repo.join(".worktrees").join("venv-branch").join(".venv");
    assert!(wt_venv.is_symlink());
}

#[test]
fn test_state_backfill_preserves_existing_fields() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);
    let stub_dir = create_default_gh_stub(&repo);
    create_state_file(&repo, "backfill-branch");
    create_lock_entry(&repo, "backfill-branch");

    let output = run_start_workspace(
        &repo,
        "Backfill Feature Title",
        "backfill-branch",
        &stub_dir,
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let state_path = flow_states_dir(&repo).join("backfill-branch.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    // Original fields preserved
    assert_eq!(state["started_at"], "2026-01-01T00:00:00-08:00");
    assert_eq!(state["branch"], "backfill-branch");
    // PR fields backfilled
    assert_eq!(state["pr_number"], 42);
    assert!(state["pr_url"].as_str().unwrap().contains("pull/42"));
}

#[test]
fn test_worktree_cwd_root_when_relative_cwd_empty() {
    // When relative_cwd is empty (root-level flow), worktree_cwd in the
    // response equals worktree itself (no subdir suffix). The skill cds
    // into this path; an empty relative_cwd means cd to .worktrees/<branch>.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);
    let stub_dir = create_default_gh_stub(&repo);
    create_state_file(&repo, "root-flow");
    create_lock_entry(&repo, "root-flow");

    let output = run_start_workspace(&repo, "Root Flow", "root-flow", &stub_dir);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["worktree"], ".worktrees/root-flow");
    assert_eq!(data["worktree_cwd"], ".worktrees/root-flow");
    assert_eq!(data["relative_cwd"], "");
}

#[test]
fn test_worktree_cwd_includes_relative_cwd_suffix() {
    // When the state file has a non-empty relative_cwd (set by start-init
    // when the user starts a flow inside a mono-repo subdir), start-workspace
    // returns worktree_cwd with that suffix appended so the skill can cd
    // the agent into the same subdirectory after the worktree is created.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);
    let stub_dir = create_default_gh_stub(&repo);

    // Pre-create state file with non-empty relative_cwd
    let state_dir = flow_states_dir(&repo);
    fs::create_dir_all(&state_dir).unwrap();
    let state = json!({
        "schema_version": 1,
        "branch": "subdir-flow",
        "relative_cwd": "api",
        "started_at": "2026-01-01T00:00:00-08:00",
        "current_phase": "flow-start",
        "files": {
            "plan": null,
            "dag": null,
            "log": ".flow-states/subdir-flow.log",
            "state": ".flow-states/subdir-flow.json",
        },
        "phases": {},
        "phase_transitions": [],
        "notes": [],
        "prompt": "test",
    });
    fs::write(
        state_dir.join("subdir-flow.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();
    create_lock_entry(&repo, "subdir-flow");

    let output = run_start_workspace(&repo, "Subdir Flow", "subdir-flow", &stub_dir);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["worktree"], ".worktrees/subdir-flow");
    assert_eq!(data["worktree_cwd"], ".worktrees/subdir-flow/api");
    assert_eq!(data["relative_cwd"], "api");
}

#[test]
fn test_worktree_partial_failure_recovery_after_cleanup() {
    // Simulates a partial failure where a directory exists at the worktree path
    // (e.g., from a crashed start-workspace). First attempt fails. After removing
    // the blocking directory, the retry succeeds.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);
    let stub_dir = create_default_gh_stub(&repo);
    create_state_file(&repo, "recovery-branch");
    create_lock_entry(&repo, "recovery-branch");

    // Pre-create the worktree directory to simulate partial failure residue
    let wt_path = repo.join(".worktrees").join("recovery-branch");
    fs::create_dir_all(&wt_path).unwrap();
    // Create a branch so git worktree add -b fails (branch exists + dir exists)
    Command::new("git")
        .args(["branch", "recovery-branch"])
        .current_dir(&repo)
        .output()
        .unwrap();

    // First attempt: fails because directory exists
    let output = run_start_workspace(&repo, "Recovery Feature", "recovery-branch", &stub_dir);
    let data = parse_output(&output);
    assert_eq!(
        data["status"], "error",
        "First attempt should fail with existing directory"
    );

    // Cleanup: remove the blocking directory and stale branch
    fs::remove_dir_all(&wt_path).unwrap();
    Command::new("git")
        .args(["branch", "-D", "recovery-branch"])
        .current_dir(&repo)
        .output()
        .unwrap();

    // Re-create state and lock (first attempt consumed them)
    create_state_file(&repo, "recovery-branch");
    create_lock_entry(&repo, "recovery-branch");

    // Retry: should succeed now
    let output = run_start_workspace(&repo, "Recovery Feature", "recovery-branch", &stub_dir);
    assert_eq!(
        output.status.code(),
        Some(0),
        "Retry after cleanup should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert!(
        wt_path.is_dir(),
        "Worktree directory should exist after successful retry"
    );
}

#[test]
fn test_prompt_file_not_found_releases_lock() {
    // Exercises lines 171-188: prompt file read fails → error + lock released.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);
    let stub_dir = create_default_gh_stub(&repo);
    create_state_file(&repo, "prompt-fail-branch");
    create_lock_entry(&repo, "prompt-fail-branch");

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "start-workspace",
            "Prompt Fail Feature",
            "--branch",
            "prompt-fail-branch",
            "--prompt-file",
            "/nonexistent/path/to/prompt",
        ])
        .current_dir(&repo)
        .env(
            "PATH",
            format!(
                "{}:{}",
                stub_dir.to_string_lossy(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("CLAUDE_PLUGIN_ROOT", &manifest_dir)
        .env_remove("FLOW_SIMULATE_BRANCH")
        .output()
        .unwrap();

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(
        data["step"].as_str().unwrap_or(""),
        "prompt_file",
        "step should be prompt_file"
    );

    // Lock must be released
    let queue_dir = flow_states_dir(&repo).join("start-queue");
    assert!(
        !queue_dir.join("prompt-fail-branch").exists(),
        "Lock must be released on prompt file error"
    );
}

#[test]
fn test_backfill_non_object_state_guard() {
    // Exercises lines 264-266: state file has array content → backfill
    // guard fires, IndexMut crash prevented. The command still succeeds
    // (worktree + PR created), but state is not backfilled.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);
    let stub_dir = create_default_gh_stub(&repo);

    // Write array content as state file instead of the normal object
    let state_dir = flow_states_dir(&repo);
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("array-state-branch.json"), "[]").unwrap();
    create_lock_entry(&repo, "array-state-branch");

    let output = run_start_workspace(
        &repo,
        "Array State Feature",
        "array-state-branch",
        &stub_dir,
    );
    let data = parse_output(&output);
    assert_eq!(
        data["status"],
        "ok",
        "Should succeed despite array state; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // State file should still be array (guard prevented IndexMut write)
    let state_content = fs::read_to_string(state_dir.join("array-state-branch.json")).unwrap();
    let state_val: Value = serde_json::from_str(&state_content).unwrap();
    assert!(
        state_val.is_array(),
        "Array state root should be preserved by the guard"
    );
}

#[test]
fn start_workspace_corrupt_state_returns_backfill_error() {
    // Exercises the backfill error branch in src/start_workspace.rs
    // (mutate_state fails on a corrupt JSON state file). Pre-seeds the
    // state file with invalid JSON; the worktree + PR succeed, then
    // backfill hits parse failure and returns status="error" with
    // step="backfill", releasing the lock.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);
    let stub_dir = create_default_gh_stub(&repo);

    // Pre-seed corrupt JSON as the state file — mutate_state will fail
    // parsing it.
    let state_dir = flow_states_dir(&repo);
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join("corrupt-backfill-branch.json"),
        "not json{{{",
    )
    .unwrap();
    create_lock_entry(&repo, "corrupt-backfill-branch");

    let output = run_start_workspace(
        &repo,
        "Corrupt Backfill Feature",
        "corrupt-backfill-branch",
        &stub_dir,
    );
    let data = parse_output(&output);
    assert_eq!(
        data["status"],
        "error",
        "Corrupt state file must surface as error; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        data["step"].as_str().unwrap_or(""),
        "backfill",
        "step should name the failed phase"
    );
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .contains("Failed to backfill state"),
        "error message should mention backfill; got: {}",
        data["message"]
    );

    // Lock must be released even on backfill error.
    let queue_dir = flow_states_dir(&repo).join("start-queue");
    assert!(
        !queue_dir.join("corrupt-backfill-branch").exists(),
        "Lock must be released on backfill error"
    );
}
