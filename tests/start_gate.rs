//! Integration tests for start-gate subcommand.
//!
//! start-gate consolidates: git pull + CI baseline (retry 3) + update-deps +
//! post-deps CI (retry 3 if deps changed) into a single command.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::json;

use common::{create_git_repo_with_remote, parse_output};

// --- Test helpers ---

/// Create a bin/ci script that exits with a given code.
fn create_bin_ci(repo: &Path, exit_code: i32) {
    let bin_dir = repo.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let script = format!("#!/bin/bash\nexit {}\n", exit_code);
    let ci_path = bin_dir.join("ci");
    fs::write(&ci_path, script).unwrap();
    fs::set_permissions(&ci_path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Create a bin/ci script that fails N times then succeeds.
fn create_flaky_bin_ci(repo: &Path, fail_count: u32) {
    let bin_dir = repo.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let counter_path = repo.join(".ci-counter");
    let script = format!(
        "#!/bin/bash\n\
         COUNTER_FILE=\"{}\"\n\
         if [ ! -f \"$COUNTER_FILE\" ]; then echo 0 > \"$COUNTER_FILE\"; fi\n\
         COUNT=$(cat \"$COUNTER_FILE\")\n\
         COUNT=$((COUNT + 1))\n\
         echo $COUNT > \"$COUNTER_FILE\"\n\
         if [ $COUNT -le {} ]; then\n\
           echo \"FLAKY FAILURE attempt $COUNT\" >&2\n\
           exit 1\n\
         fi\n\
         exit 0\n",
        counter_path.to_string_lossy(),
        fail_count
    );
    let ci_path = bin_dir.join("ci");
    fs::write(&ci_path, script).unwrap();
    fs::set_permissions(&ci_path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Create a bin/dependencies script.
fn create_bin_deps(repo: &Path, script_body: &str) {
    let bin_dir = repo.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let deps_path = bin_dir.join("dependencies");
    let script = format!("#!/bin/bash\n{}\n", script_body);
    fs::write(&deps_path, script).unwrap();
    fs::set_permissions(&deps_path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Set up a state file so start-gate can find the branch.
fn create_state_file(repo: &Path, branch: &str) {
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let state = json!({
        "schema_version": 1,
        "branch": branch,
        "current_phase": "flow-start",
        "start_step": 1,
        "start_steps_total": 5,
        "phases": {}
    });
    fs::write(
        state_dir.join(format!("{}.json", branch)),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();
}

/// Run flow-rs start-gate with the given arguments.
fn run_start_gate(repo: &Path, branch: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["start-gate", "--branch", branch])
        .current_dir(repo)
        .env_remove("FLOW_SIMULATE_BRANCH")
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap()
}

// --- Tests ---

#[test]
fn test_clean_path() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    create_bin_ci(&repo, 0);
    create_state_file(&repo, "test-branch");

    let output = run_start_gate(&repo, "test-branch");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "clean");
}

#[test]
fn test_ci_flaky_baseline() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    create_flaky_bin_ci(&repo, 1); // fail once, then succeed
    create_state_file(&repo, "flaky-branch");

    let output = run_start_gate(&repo, "flaky-branch");
    let data = parse_output(&output);
    assert_eq!(data["status"], "ci_flaky");
    assert!(
        data["first_failure_output"].is_string(),
        "Must include first failure output"
    );
    assert!(data["attempts"].is_number(), "Must include attempt count");
    assert_eq!(
        data["flaky_context"], "CI baseline on pristine main during flow-start",
        "Must include correct flaky context"
    );
}

#[test]
fn test_ci_failed_baseline() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    create_bin_ci(&repo, 1); // always fail
    create_state_file(&repo, "failed-branch");

    let output = run_start_gate(&repo, "failed-branch");
    let data = parse_output(&output);
    assert_eq!(data["status"], "ci_failed");
    assert!(data["output"].is_string(), "Must include CI output");
}

#[test]
fn test_deps_changed_ci_passes() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    create_bin_ci(&repo, 0);
    // bin/dependencies that creates a file (git status shows changes)
    create_bin_deps(&repo, "echo 'updated' > deps-output.txt");
    create_state_file(&repo, "deps-branch");

    let output = run_start_gate(&repo, "deps-branch");
    let data = parse_output(&output);
    assert_eq!(data["status"], "clean");
    assert_eq!(data["deps_changed"], true);
}

#[test]
fn test_deps_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    create_bin_ci(&repo, 0);
    // No bin/dependencies — deps step is skipped
    create_state_file(&repo, "no-deps-branch");

    let output = run_start_gate(&repo, "no-deps-branch");
    let data = parse_output(&output);
    assert_eq!(data["status"], "clean");
}

#[test]
fn test_deps_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    create_bin_ci(&repo, 0);
    create_bin_deps(&repo, "exit 1"); // deps fails
    create_state_file(&repo, "deps-error-branch");

    let output = run_start_gate(&repo, "deps-error-branch");
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .contains("dependencies"),
        "Error should mention dependencies"
    );
}

#[test]
fn test_deps_ci_failed() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // bin/dependencies creates a file, then CI fails on post-deps run
    create_bin_deps(&repo, "echo 'updated' > deps-output.txt");
    create_state_file(&repo, "deps-ci-fail-branch");

    // CI needs to pass for baseline but fail for post-deps.
    // Use a counter: first 3 calls pass (baseline retry 3), next 3 fail (post-deps).
    let bin_dir = repo.join("bin");
    let counter_path = repo.join(".ci-counter");
    let script = format!(
        "#!/bin/bash\n\
         COUNTER_FILE=\"{}\"\n\
         if [ ! -f \"$COUNTER_FILE\" ]; then echo 0 > \"$COUNTER_FILE\"; fi\n\
         COUNT=$(cat \"$COUNTER_FILE\")\n\
         COUNT=$((COUNT + 1))\n\
         echo $COUNT > \"$COUNTER_FILE\"\n\
         if [ $COUNT -le 1 ]; then exit 0; fi\n\
         echo \"POST-DEPS FAILURE\" >&2\n\
         exit 1\n",
        counter_path.to_string_lossy()
    );
    fs::write(bin_dir.join("ci"), script).unwrap();
    fs::set_permissions(bin_dir.join("ci"), fs::Permissions::from_mode(0o755)).unwrap();

    let output = run_start_gate(&repo, "deps-ci-fail-branch");
    let data = parse_output(&output);
    assert_eq!(data["status"], "deps_ci_failed");
    assert!(data["output"].is_string(), "Must include CI output");
}

#[test]
fn test_deps_ci_flaky() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    create_bin_deps(&repo, "echo 'updated' > deps-output.txt");
    create_state_file(&repo, "deps-flaky-branch");

    // CI passes baseline (1st call), then fails once (2nd call = post-deps attempt 1),
    // then passes (3rd call = post-deps attempt 2)
    let bin_dir = repo.join("bin");
    let counter_path = repo.join(".ci-counter");
    let script = format!(
        "#!/bin/bash\n\
         COUNTER_FILE=\"{}\"\n\
         if [ ! -f \"$COUNTER_FILE\" ]; then echo 0 > \"$COUNTER_FILE\"; fi\n\
         COUNT=$(cat \"$COUNTER_FILE\")\n\
         COUNT=$((COUNT + 1))\n\
         echo $COUNT > \"$COUNTER_FILE\"\n\
         if [ $COUNT -eq 2 ]; then\n\
           echo \"FLAKY POST-DEPS FAILURE\" >&2\n\
           exit 1\n\
         fi\n\
         exit 0\n",
        counter_path.to_string_lossy()
    );
    fs::write(bin_dir.join("ci"), script).unwrap();
    fs::set_permissions(bin_dir.join("ci"), fs::Permissions::from_mode(0o755)).unwrap();

    let output = run_start_gate(&repo, "deps-flaky-branch");
    let data = parse_output(&output);
    // When post-deps CI is flaky, start-gate should still return clean
    // but include flaky info
    assert!(
        data["status"] == "clean" || data["status"] == "ci_flaky",
        "Expected clean or ci_flaky, got: {}",
        data["status"]
    );
    if data["status"] == "ci_flaky" {
        assert_eq!(
            data["flaky_context"],
            "CI post-deps gate during flow-start after dependency update"
        );
    }
}

#[test]
fn test_pull_failure() {
    let dir = tempfile::tempdir().unwrap();
    // Init a repo without a remote — git pull will fail
    let repo = dir.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    Command::new("git")
        .args(["-c", "init.defaultBranch=main", "init"])
        .current_dir(&repo)
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

    create_bin_ci(&repo, 0);
    create_state_file(&repo, "pull-fail-branch");

    let output = run_start_gate(&repo, "pull-fail-branch");
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"].as_str().unwrap_or("").contains("pull"),
        "Error should mention git pull"
    );
}
