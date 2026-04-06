//! Integration tests for start-workspace subcommand.
//!
//! start-workspace consolidates: worktree creation + PR creation + state
//! backfill + lock release into a single command.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

// --- Test helpers ---

/// Read current plugin version from .claude-plugin/plugin.json.
fn current_plugin_version() -> String {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let plugin_path = manifest_dir.join(".claude-plugin").join("plugin.json");
    let content = fs::read_to_string(&plugin_path).expect("plugin.json must exist");
    let data: Value = serde_json::from_str(&content).expect("plugin.json must be valid JSON");
    data["version"]
        .as_str()
        .expect("plugin.json must have version")
        .to_string()
}

/// Create a bare+clone git repo pair for testing.
fn create_git_repo_with_remote(parent: &Path) -> PathBuf {
    let bare = parent.join("bare.git");
    let repo = parent.join("repo");

    Command::new("git")
        .args(["init", "--bare", "-b", "main", &bare.to_string_lossy()])
        .output()
        .unwrap();

    Command::new("git")
        .args(["clone", &bare.to_string_lossy(), &repo.to_string_lossy()])
        .output()
        .unwrap();

    for (key, val) in [
        ("user.email", "test@test.com"),
        ("user.name", "Test"),
        ("commit.gpgsign", "false"),
    ] {
        Command::new("git")
            .args(["config", key, val])
            .current_dir(&repo)
            .output()
            .unwrap();
    }

    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&repo)
        .output()
        .unwrap();

    Command::new("git")
        .args(["push", "-u", "origin", "main"])
        .current_dir(&repo)
        .output()
        .unwrap();

    repo
}

/// Write .flow.json with version and framework.
fn write_flow_json(repo: &Path, version: &str, framework: &str) {
    let data = json!({
        "flow_version": version,
        "framework": framework,
    });
    fs::write(repo.join(".flow.json"), data.to_string()).unwrap();
}

/// Create a gh stub script.
fn create_gh_stub(repo: &Path, script: &str) -> PathBuf {
    let stub_dir = repo.join(".stub-bin");
    fs::create_dir_all(&stub_dir).unwrap();
    let gh_stub = stub_dir.join("gh");
    fs::write(&gh_stub, script).unwrap();
    fs::set_permissions(&gh_stub, fs::Permissions::from_mode(0o755)).unwrap();
    stub_dir
}

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
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state = json!({
        "schema_version": 1,
        "branch": branch,
        "repo": null,
        "pr_number": null,
        "pr_url": null,
        "started_at": "2026-01-01T00:00:00-08:00",
        "current_phase": "flow-start",
        "framework": "python",
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
    let queue_dir = repo.join(".flow-states").join("start-queue");
    fs::create_dir_all(&queue_dir).unwrap();
    fs::write(queue_dir.join(feature), "").unwrap();
}

/// Run flow-rs start-workspace.
fn run_start_workspace(
    repo: &Path,
    feature: &str,
    branch: &str,
    stub_dir: &Path,
) -> Output {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "start-workspace",
            feature,
            "--branch",
            branch,
        ])
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

/// Parse JSON from the last line of stdout.
fn parse_output(output: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last_line = stdout.trim().lines().last().unwrap_or("");
    serde_json::from_str(last_line).unwrap_or_else(|_| json!({"raw": stdout.trim()}))
}

// --- Tests ---

#[test]
fn test_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "python");
    let stub_dir = create_default_gh_stub(&repo);
    create_state_file(&repo, "test-branch");
    create_lock_entry(&repo, "test-feature");

    let output = run_start_workspace(&repo, "test-feature", "test-branch", &stub_dir);
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

    // Lock should be released
    let queue_dir = repo.join(".flow-states").join("start-queue");
    assert!(
        !queue_dir.join("test-feature").exists(),
        "Lock must be released after start-workspace"
    );

    // State file should have PR fields backfilled
    let state_path = repo.join(".flow-states").join("test-branch.json");
    let state: Value =
        serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert!(state["pr_number"].is_number());
    assert!(state["pr_url"].is_string());
}

#[test]
fn test_worktree_failure_releases_lock() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "python");
    let stub_dir = create_default_gh_stub(&repo);
    create_state_file(&repo, "test-branch");
    create_lock_entry(&repo, "fail-feature");

    // Create the worktree dir to make git worktree add fail
    let wt_path = repo.join(".worktrees").join("test-branch");
    fs::create_dir_all(&wt_path).unwrap();
    // Also create a branch with this name so git worktree add -b fails
    Command::new("git")
        .args(["branch", "test-branch"])
        .current_dir(&repo)
        .output()
        .unwrap();

    let output = run_start_workspace(&repo, "fail-feature", "test-branch", &stub_dir);
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");

    // Lock MUST still be released on error
    let queue_dir = repo.join(".flow-states").join("start-queue");
    assert!(
        !queue_dir.join("fail-feature").exists(),
        "Lock must be released even on worktree failure"
    );
}

#[test]
fn test_pr_creation_failure_releases_lock() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "python");
    // gh stub that fails on pr create
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\nexit 1\n",
    );
    create_state_file(&repo, "pr-fail-branch");
    create_lock_entry(&repo, "pr-fail-feature");

    let output = run_start_workspace(&repo, "pr-fail-feature", "pr-fail-branch", &stub_dir);
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");

    // Lock must be released
    let queue_dir = repo.join(".flow-states").join("start-queue");
    assert!(
        !queue_dir.join("pr-fail-feature").exists(),
        "Lock must be released even on PR creation failure"
    );
}

#[test]
fn test_venv_symlinked() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "python");
    let stub_dir = create_default_gh_stub(&repo);
    create_state_file(&repo, "venv-branch");
    create_lock_entry(&repo, "venv-feature");

    // Create .venv dir
    let venv_dir = repo.join(".venv");
    fs::create_dir_all(venv_dir.join("bin")).unwrap();
    fs::write(venv_dir.join("bin").join("python3"), "fake").unwrap();

    let output = run_start_workspace(&repo, "venv-feature", "venv-branch", &stub_dir);
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
    write_flow_json(&repo, &current_plugin_version(), "python");
    let stub_dir = create_default_gh_stub(&repo);
    create_state_file(&repo, "backfill-branch");
    create_lock_entry(&repo, "backfill-feature");

    let output = run_start_workspace(&repo, "backfill-feature", "backfill-branch", &stub_dir);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let state_path = repo.join(".flow-states").join("backfill-branch.json");
    let state: Value =
        serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    // Original fields preserved
    assert_eq!(state["started_at"], "2026-01-01T00:00:00-08:00");
    assert_eq!(state["branch"], "backfill-branch");
    // PR fields backfilled
    assert_eq!(state["pr_number"], 42);
    assert!(state["pr_url"].as_str().unwrap().contains("pull/42"));
}
