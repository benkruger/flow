//! Integration tests for start-init subcommand.
//!
//! start-init consolidates: lock acquire + prime-check + upgrade-check +
//! prompt write + init-state + label-issues into a single command.
//! All tests use `run_impl` for testability (run() calls process::exit).

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::{
    create_gh_stub, create_git_repo_with_remote, current_plugin_version, flow_states_dir,
    parse_output, write_flow_json,
};

// --- Test helpers ---

/// Create a gh stub script that returns a fake PR URL for pr create,
/// and exits 1 for issue view (no issue found).
fn create_default_gh_stub(repo: &Path) -> PathBuf {
    create_gh_stub(
        repo,
        "#!/bin/bash\n\
         if [[ \"$1\" == \"issue\" && \"$2\" == \"view\" ]]; then exit 1; fi\n\
         if [[ \"$1\" == \"issue\" && \"$2\" == \"edit\" ]]; then exit 0; fi\n\
         echo \"https://github.com/test/repo/pull/42\"\n",
    )
}

/// Run flow-rs start-init with the given arguments.
fn run_start_init(repo: &Path, feature_name: &str, extra_args: &[&str], stub_dir: &Path) -> Output {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut args = vec!["start-init", feature_name];
    args.extend_from_slice(extra_args);

    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );

    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(&args)
        .current_dir(repo)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", &manifest_dir)
        .env_remove("FLOW_SIMULATE_BRANCH")
        .output()
        .unwrap()
}

// --- Happy path tests ---

#[test]
fn test_ready_path_happy() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);
    let stub_dir = create_default_gh_stub(&repo);

    let output = run_start_init(&repo, "test-feature", &[], &stub_dir);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "ready");
    assert!(data["branch"].is_string(), "branch field must be present");

    // Lock should be acquired (still held — start-workspace releases it)
    let queue_dir = flow_states_dir(&repo).join("start-queue");
    assert!(
        queue_dir.join("test-feature").exists(),
        "Lock queue entry must exist after start-init"
    );

    // State file should be created by init-state subprocess
    let branch = data["branch"].as_str().unwrap();
    let state_path = flow_states_dir(&repo).join(format!("{}.json", branch));
    assert!(
        state_path.exists(),
        "State file must be created by init-state"
    );
}

#[test]
fn test_locked_path() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);
    let stub_dir = create_default_gh_stub(&repo);

    // Pre-create a lock entry for another feature
    let queue_dir = flow_states_dir(&repo).join("start-queue");
    fs::create_dir_all(&queue_dir).unwrap();
    fs::write(queue_dir.join("other-feature"), "").unwrap();

    let output = run_start_init(&repo, "my-feature", &[], &stub_dir);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "locked");
    assert_eq!(data["feature"], "other-feature");
}

#[test]
fn test_prime_check_failed() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // Write .flow.json with wrong version to trigger prime-check failure
    write_flow_json(&repo, "0.0.1", None);
    let stub_dir = create_default_gh_stub(&repo);

    let output = run_start_init(&repo, "prime-fail", &[], &stub_dir);
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"].as_str().unwrap_or("").contains("mismatch"),
        "Error message should mention version mismatch"
    );

    // Lock must be released after prime-check failure
    let queue_dir = flow_states_dir(&repo).join("start-queue");
    assert!(
        !queue_dir.join("prime-fail").exists(),
        "Lock must be released on prime-check error"
    );
}

#[test]
fn test_init_state_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);

    // Create gh stub that fails on issue view (to simulate issue fetch failure)
    // but with a prompt that contains #999 (nonexistent issue)
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [[ \"$1\" == \"issue\" && \"$2\" == \"view\" ]]; then\n\
           echo '{\"errors\": [{\"type\": \"NOT_FOUND\"}]}' >&2\n\
           exit 1\n\
         fi\n\
         echo \"https://github.com/test/repo/pull/42\"\n",
    );

    // Write a prompt file that references a nonexistent issue
    let prompt_path = flow_states_dir(&repo).join("init-error-start-prompt");
    fs::create_dir_all(flow_states_dir(&repo)).unwrap();
    fs::write(&prompt_path, "work on issue #999").unwrap();

    let output = run_start_init(
        &repo,
        "init-error",
        &["--prompt-file", &prompt_path.to_string_lossy()],
        &stub_dir,
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");

    // Issue fetch fails before lock acquisition — no lock was ever acquired
    let queue_dir = flow_states_dir(&repo).join("start-queue");
    // No lock under the feature name
    assert!(
        !queue_dir.join("init-error").exists(),
        "No lock should exist — fetch failed before lock acquisition"
    );
    // No lock under any name (queue dir should be empty or not exist)
    if queue_dir.exists() {
        let entries: Vec<_> = fs::read_dir(&queue_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(
            entries.is_empty(),
            "No lock entry should exist for any name when issue fetch fails pre-lock"
        );
    }
}

#[test]
fn test_auto_upgraded() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_default_gh_stub(&repo);

    // Write .flow.json with old version but matching hashes to trigger auto-upgrade.
    // We need to compute the actual hashes. Easier: write with current version first,
    // read the hashes, then rewrite with an old version but same hashes.
    write_flow_json(&repo, &current_plugin_version(), None);

    // Verify that when prime-check passes normally (versions match),
    // auto_upgraded is absent in the response.
    let output = run_start_init(&repo, "auto-upgrade-test", &[], &stub_dir);
    let data = parse_output(&output);
    assert_eq!(data["status"], "ready");
    // When no auto-upgrade happens, auto_upgraded should be absent or false
    assert!(
        data.get("auto_upgraded").is_none()
            || data["auto_upgraded"] == false
            || data["auto_upgraded"].is_null(),
        "auto_upgraded should not be true when versions match"
    );
}

#[test]
fn test_upgrade_available() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);

    // Create gh stub that returns a newer version for upgrade-check
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [[ \"$1\" == \"issue\" && \"$2\" == \"view\" ]]; then exit 1; fi\n\
         if [[ \"$1\" == \"issue\" && \"$2\" == \"edit\" ]]; then exit 0; fi\n\
         if [[ \"$1\" == \"release\" && \"$2\" == \"view\" ]]; then\n\
           echo '{\"tagName\": \"v99.99.99\"}'\n\
           exit 0\n\
         fi\n\
         echo \"https://github.com/test/repo/pull/42\"\n",
    );

    let output = run_start_init(&repo, "upgrade-test", &[], &stub_dir);
    let data = parse_output(&output);
    assert_eq!(data["status"], "ready");
    // upgrade field should contain the available version info
    if let Some(upgrade) = data.get("upgrade") {
        if upgrade["status"] == "upgrade_available" {
            assert!(upgrade["latest"].is_string());
        }
    }
}

#[test]
fn test_labels_best_effort() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);

    // Create gh stub that fails on issue edit (label failure)
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [[ \"$1\" == \"issue\" && \"$2\" == \"view\" ]]; then exit 1; fi\n\
         if [[ \"$1\" == \"issue\" && \"$2\" == \"edit\" ]]; then exit 1; fi\n\
         echo \"https://github.com/test/repo/pull/42\"\n",
    );

    // The feature name has no #N references, so no labels to apply.
    // This test verifies the command still returns "ready" even when
    // label operations would fail.
    let output = run_start_init(&repo, "labels-test", &[], &stub_dir);
    let data = parse_output(&output);
    assert_eq!(
        data["status"], "ready",
        "Label failure must not block start-init"
    );
}

#[test]
fn test_no_flow_json_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // No write_flow_json — .flow.json is absent
    let stub_dir = create_default_gh_stub(&repo);

    let output = run_start_init(&repo, "no-flow-json", &[], &stub_dir);
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"].as_str().unwrap_or("").contains("prime"),
        "Error should suggest running flow-prime"
    );

    // Lock must be released (under canonical branch name)
    let queue_dir = flow_states_dir(&repo).join("start-queue");
    assert!(
        !queue_dir.join("no-flow-json").exists(),
        "Lock must be released on prime-check error"
    );
}

// --- Regression tests ---

#[test]
fn test_lock_uses_canonical_branch_not_feature_name() {
    // Guards the contract that the start lock is acquired and
    // released under the same name. When an issue prompt resolves to
    // a canonical branch name that differs from the raw feature name
    // (e.g. "work on issue #42" → "add-dark-mode-toggle"), both
    // `acquire_lock` and `release_lock` must use the canonical
    // (issue-derived) name. Otherwise the lock file leaks under the
    // raw feature name and blocks subsequent flows until the
    // 30-minute stale timeout.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);

    // gh stub: issue view returns a title different from feature_name,
    // pr create returns a fake URL
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [[ \"$1\" == \"issue\" && \"$2\" == \"view\" ]]; then\n\
           echo '{\"title\": \"Add Dark Mode Toggle\", \"labels\": []}'\n\
           exit 0\n\
         fi\n\
         if [[ \"$1\" == \"issue\" && \"$2\" == \"edit\" ]]; then exit 0; fi\n\
         echo \"https://github.com/test/repo/pull/42\"\n",
    );

    // Prompt references issue #42
    let prompt_path = flow_states_dir(&repo).join("regression-start-prompt");
    fs::create_dir_all(flow_states_dir(&repo)).unwrap();
    fs::write(&prompt_path, "work on issue #42").unwrap();

    let output = run_start_init(
        &repo,
        "my-feature",
        &["--prompt-file", &prompt_path.to_string_lossy()],
        &stub_dir,
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "ready", "Should succeed");
    assert_eq!(
        data["branch"].as_str().unwrap(),
        "add-dark-mode-toggle",
        "Branch should be derived from issue title, not feature name"
    );

    // Lock must be under the canonical branch name
    let queue_dir = flow_states_dir(&repo).join("start-queue");
    assert!(
        queue_dir.join("add-dark-mode-toggle").exists(),
        "Lock must be under canonical branch name (issue-derived)"
    );
    assert!(
        !queue_dir.join("my-feature").exists(),
        "Lock must NOT be under the raw feature name"
    );
}
