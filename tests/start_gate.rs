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

use common::{create_git_repo_with_remote, flow_states_dir, parse_output};

// --- Test helpers ---

/// Create the four `bin/{format,lint,build,test}` stubs the CI dispatch
/// looks for. All four exit with `exit_code`. Writing all four ensures
/// `ci::run_impl` does not short-circuit on "no tools found" before
/// encountering the failure we want to exercise.
fn create_bin_tools(repo: &Path, exit_code: i32) {
    let bin_dir = repo.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let script = format!("#!/bin/bash\nexit {}\n", exit_code);
    for tool in ["format", "lint", "build", "test"] {
        let path = bin_dir.join(tool);
        fs::write(&path, &script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// Create the four `bin/*` stubs where format/lint/build always pass and
/// `bin/test` fails `fail_count` times then succeeds. `bin/test` runs
/// last in the dispatch order, so baseline CI sees the failures from it
/// until the counter elapses.
fn create_flaky_bin_tools(repo: &Path, fail_count: u32) {
    let bin_dir = repo.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let pass = "#!/bin/bash\nexit 0\n";
    for tool in ["format", "lint", "build"] {
        let path = bin_dir.join(tool);
        fs::write(&path, pass).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
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
    let test_path = bin_dir.join("test");
    fs::write(&test_path, script).unwrap();
    fs::set_permissions(&test_path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Install no-op `bin/{format,lint,build}` stubs that always pass. The
/// caller is expected to install `bin/test` separately with the
/// behavior it wants to exercise.
fn install_passing_noncritical_tools(repo: &Path) {
    let bin_dir = repo.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let pass = "#!/bin/bash\nexit 0\n";
    for tool in ["format", "lint", "build"] {
        let path = bin_dir.join(tool);
        fs::write(&path, pass).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
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
    let state_dir = flow_states_dir(repo);
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

/// Write a CI sentinel so ci::run_impl takes the fast skip path
/// without spawning any bin/* scripts. Excludes `.flow-states/` from
/// git so the sentinel itself doesn't change the tree snapshot
/// (chicken-and-egg problem).
fn write_ci_sentinel(repo: &Path) {
    // Exclude .flow-states/ from untracked file list
    let exclude_dir = repo.join(".git").join("info");
    fs::create_dir_all(&exclude_dir).unwrap();
    let exclude_file = exclude_dir.join("exclude");
    let existing = fs::read_to_string(&exclude_file).unwrap_or_default();
    if !existing.contains(".flow-states/") {
        fs::write(&exclude_file, format!("{}.flow-states/\n", existing)).unwrap();
    }
    let snapshot = flow_rs::ci::tree_snapshot(repo, None);
    let sentinel = flow_rs::ci::sentinel_path(repo, "main");
    fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
    fs::write(&sentinel, &snapshot).unwrap();
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
    create_state_file(&repo, "test-branch");
    write_ci_sentinel(&repo);

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
    create_flaky_bin_tools(&repo, 1); // fail once, then succeed
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
    create_bin_tools(&repo, 1); // always fail
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
    write_ci_sentinel(&repo);
    // Provide the 4 bin/* stubs (all passing) so post-deps CI has tools
    // to invoke after bin/dependencies modifies the tree.
    create_bin_tools(&repo, 0);
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
    // No bin/dependencies — deps step is skipped
    create_state_file(&repo, "no-deps-branch");
    write_ci_sentinel(&repo);

    let output = run_start_gate(&repo, "no-deps-branch");
    let data = parse_output(&output);
    assert_eq!(data["status"], "clean");
}

#[test]
fn test_deps_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    create_bin_deps(&repo, "exit 1"); // deps fails
    create_state_file(&repo, "deps-error-branch");
    write_ci_sentinel(&repo);

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

    // bin/{format,lint,build} always pass; bin/test passes on the
    // first invocation (baseline) and fails on every subsequent
    // invocation (post-deps gate, retries included).
    install_passing_noncritical_tools(&repo);
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
    fs::write(bin_dir.join("test"), script).unwrap();
    fs::set_permissions(bin_dir.join("test"), fs::Permissions::from_mode(0o755)).unwrap();

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

    // bin/{format,lint,build} always pass. bin/test passes baseline
    // (call #1), fails once on post-deps attempt 1 (call #2), then
    // passes on post-deps attempt 2 (call #3).
    install_passing_noncritical_tools(&repo);
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
    fs::write(bin_dir.join("test"), script).unwrap();
    fs::set_permissions(bin_dir.join("test"), fs::Permissions::from_mode(0o755)).unwrap();

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

    create_bin_tools(&repo, 0);
    create_state_file(&repo, "pull-fail-branch");

    let output = run_start_gate(&repo, "pull-fail-branch");
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"].as_str().unwrap_or("").contains("pull"),
        "Error should mention git pull"
    );
}
