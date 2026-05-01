//! Integration tests for start-workspace subcommand.
//!
//! start-workspace consolidates: worktree creation + PR creation + state
//! backfill + lock release into a single command.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use flow_rs::start_workspace::{run_impl_main, Args};
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

/// Drive the `Some(str)` branch of base_branch state-file parsing in
/// `run_impl_with_paths` and prove the value reaches
/// `gh pr create --base <base_branch>`. State file declares
/// `base_branch: "staging"`. The gh stub captures every argv to a
/// recorder file so the test can assert `--base staging` was passed —
/// proving the value flowed through from state file to `gh` instead
/// of the hardcoded `"main"` fallback.
#[test]
fn test_base_branch_from_state_used_for_pr_base() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);

    // Recorder gh stub: append every invocation to .gh-args, then emit
    // the standard PR URL on stdout so the rest of the workflow proceeds.
    let recorder_path = repo.join(".gh-args");
    let stub_script = format!(
        "#!/bin/bash\n\
         echo \"$@\" >> \"{}\"\n\
         echo \"https://github.com/test/repo/pull/42\"\n",
        recorder_path.to_string_lossy()
    );
    let stub_dir = create_gh_stub(&repo, &stub_script);

    // Pre-create state with non-default base_branch.
    let state_dir = flow_states_dir(&repo);
    fs::create_dir_all(&state_dir).unwrap();
    let state = json!({
        "schema_version": 1,
        "branch": "staging-flow",
        "base_branch": "staging",
        "relative_cwd": "",
        "started_at": "2026-01-01T00:00:00-08:00",
        "current_phase": "flow-start",
        "files": {
            "plan": null,
            "dag": null,
            "log": ".flow-states/staging-flow.log",
            "state": ".flow-states/staging-flow.json",
        },
        "phases": {},
        "phase_transitions": [],
        "notes": [],
        "prompt": "test",
    });
    fs::write(
        state_dir.join("staging-flow.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();
    create_lock_entry(&repo, "staging-flow");

    let output = run_start_workspace(&repo, "Staging Flow", "staging-flow", &stub_dir);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");

    // Verify the gh stub was called with `--base staging`. The recorder
    // file aggregates every invocation; the PR-create call must include
    // `--base staging` (not `--base main`).
    let recorded = fs::read_to_string(&recorder_path).expect("gh recorder file must exist");
    assert!(
        recorded.contains("--base staging"),
        "gh pr create must receive --base staging from state, got: {}",
        recorded
    );
    assert!(
        !recorded.contains("--base main"),
        "gh pr create must NOT receive --base main when state has staging, got: {}",
        recorded
    );
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

/// Covers the `Some(r) => json!(r)` arm of the backfill match on
/// `repo_clone` AND the valid-prompt-file `Ok(content)` branch
/// (lines 268 and 170-172). The repo's origin url is a fake
/// github.com URL (so detect_repo returns Some), while pushurl is
/// the real bare repo so `git push` still succeeds.
#[test]
fn test_backfill_with_repo_and_valid_prompt_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);
    let stub_dir = create_default_gh_stub(&repo);

    // Save current push URL and install a fake github URL as `url` so
    // `detect_repo` returns Some("owner/name"). The `pushurl` stays
    // pointed at the real bare remote so `git push` keeps working.
    let original_url = String::from_utf8_lossy(
        &Command::new("git")
            .args(["remote", "get-url", "origin"])
            .current_dir(&repo)
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();
    Command::new("git")
        .args(["remote", "set-url", "--push", "origin", &original_url])
        .current_dir(&repo)
        .output()
        .unwrap();
    Command::new("git")
        .args([
            "remote",
            "set-url",
            "origin",
            "https://github.com/owner/name.git",
        ])
        .current_dir(&repo)
        .output()
        .unwrap();

    // Write state file with repo set to "Some" value.
    let state_dir = flow_states_dir(&repo);
    fs::create_dir_all(&state_dir).unwrap();
    let state = json!({
        "schema_version": 1,
        "branch": "repo-set-branch",
        "repo": "owner/name",
        "pr_number": null,
        "pr_url": null,
        "started_at": "2026-01-01T00:00:00-08:00",
        "current_phase": "flow-start",
        "files": {
            "plan": null,
            "dag": null,
            "log": ".flow-states/repo-set-branch.log",
            "state": ".flow-states/repo-set-branch.json"
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
        state_dir.join("repo-set-branch.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();
    create_lock_entry(&repo, "repo-set-branch");

    let prompt_file = repo.join(".flow-prompt");
    fs::write(&prompt_file, "Make a real feature\n").unwrap();

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "start-workspace",
            "Real Feature",
            "--branch",
            "repo-set-branch",
            "--prompt-file",
            prompt_file.to_str().unwrap(),
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("panicked at"), "panicked: {}", stderr);
    // Prompt file should have been removed by successful read path.
    assert!(
        !prompt_file.exists(),
        "prompt file must be removed after successful Ok read"
    );
}

/// Covers the `git push` error propagation at line 122: state is
/// set up normally but `origin` remote URL points to an unreachable
/// destination so `git push` fails, `?` propagates, and the
/// subprocess surfaces a push-step error payload.
#[test]
fn test_push_failure_propagates_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);
    let stub_dir = create_default_gh_stub(&repo);

    // Point origin at an unreachable bogus path — push must fail.
    Command::new("git")
        .args([
            "remote",
            "set-url",
            "origin",
            "/nonexistent/bogus/path/to/a.git",
        ])
        .current_dir(&repo)
        .output()
        .unwrap();

    let state_dir = flow_states_dir(&repo);
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join("push-fail-branch.json"),
        serde_json::to_string(&json!({
            "schema_version": 1,
            "branch": "push-fail-branch",
            "repo": null,
            "started_at": "2026-01-01T00:00:00-08:00",
            "current_phase": "flow-start",
            "phases": {},
            "phase_transitions": [],
            "prompt": "feature",
        }))
        .unwrap(),
    )
    .unwrap();
    create_lock_entry(&repo, "push-fail-branch");

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "start-workspace",
            "Push Fail Feature",
            "--branch",
            "push-fail-branch",
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
        "push",
        "expected push step error, got: {:?}",
        data
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

// --- library-level tests (migrated from inline) ---

// Direct `extract_pr_number` tests removed — the helper is now
// private. Its edge cases (malformed URL, non-numeric, empty, no
// number after `pull`) are no longer directly tested; the
// production path hits the normal URL shape through `run_impl_main`
// below when a state file's `pr_url` is a typical github.com URL.

#[test]
fn start_workspace_run_impl_main_err_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let state_dir = root.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let args = Args {
        description: "workspace-err-feature".to_string(),
        branch: "workspace-err-branch".to_string(),
        prompt_file: None,
    };
    let (v, code) = run_impl_main(&args, &root, &root);
    assert_eq!(code, 0);
    assert_eq!(v["status"], "error");
}

/// Covers the `result?` Err propagation in `initial_commit_push_pr`
/// (line 115) — `git commit` inside the worktree fails because the
/// main repo has a pre-commit hook that exits non-zero. The commit
/// step returns Err; `?` propagates out of `initial_commit_push_pr`;
/// `run_impl_with_paths` surfaces a `status: error, step: commit`
/// payload and releases the lock.
#[cfg(unix)]
#[test]
fn test_commit_hook_failure_propagates_error() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);
    let stub_dir = create_default_gh_stub(&repo);
    create_state_file(&repo, "hook-fail-branch");
    create_lock_entry(&repo, "hook-fail-branch");

    // Install a pre-commit hook that exits non-zero. Worktrees share
    // the main repo's `.git/hooks/`, so `git commit --allow-empty` in
    // the new worktree triggers this hook and fails. `--allow-empty`
    // does NOT skip hooks (only `--no-verify` would).
    let hook = repo.join(".git").join("hooks").join("pre-commit");
    fs::create_dir_all(hook.parent().unwrap()).unwrap();
    fs::write(&hook, "#!/bin/bash\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();

    let output = run_start_workspace(&repo, "Hook Fail Feature", "hook-fail-branch", &stub_dir);
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(
        data["step"].as_str().unwrap_or(""),
        "commit",
        "expected commit-step error, got: {:?}",
        data
    );
    // Lock MUST still be released on error.
    let queue_dir = flow_states_dir(&repo).join("start-queue");
    assert!(
        !queue_dir.join("hook-fail-branch").exists(),
        "Lock must be released after commit-hook failure"
    );
}

/// Covers the `if state_path.exists() { ... }` false branch in
/// `run_impl_with_paths` — when start-workspace runs WITHOUT a
/// pre-existing state file, the backfill block is skipped and
/// execution falls through to lock release + response construction.
/// This can happen if `init-state` was never invoked before
/// `start-workspace`; the command still creates the worktree and PR,
/// just without state-file backfill.
#[test]
fn test_no_state_file_skips_backfill() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);
    let stub_dir = create_default_gh_stub(&repo);
    // NO create_state_file — the state file does not exist.
    // Still create a lock entry so release_lock finds something.
    create_lock_entry(&repo, "no-state-branch");

    let output = run_start_workspace(&repo, "No State Feature", "no-state-branch", &stub_dir);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["branch"], "no-state-branch");
    // Worktree created despite missing state file.
    assert!(repo.join(".worktrees").join("no-state-branch").is_dir());
    // Lock still released.
    let queue_dir = flow_states_dir(&repo).join("start-queue");
    assert!(!queue_dir.join("no-state-branch").exists());
    // State file was NOT created by backfill (branch block was skipped).
    assert!(!flow_states_dir(&repo).join("no-state-branch.json").exists());
}
