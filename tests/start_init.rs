//! Integration tests for start-init subcommand.
//!
//! start-init consolidates: lock acquire + prime-check + upgrade-check +
//! prompt write + init-state + label-issues into a single command.
//! Every test drives through the compiled binary — no library seams.

mod common;

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
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

// --- Coverage tests ---

#[test]
fn test_flow_in_progress_label_returns_error() {
    // Exercises the Flow In-Progress label guard: issue carries
    // "Flow In-Progress" label → error.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);

    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [[ \"$1\" == \"issue\" && \"$2\" == \"view\" ]]; then\n\
           echo '{\"title\": \"Some Issue\", \"labels\": [\"Flow In-Progress\"]}'\n\
           exit 0\n\
         fi\n\
         echo \"https://github.com/test/repo/pull/42\"\n",
    );

    let prompt_path = flow_states_dir(&repo).join("fip-start-prompt");
    fs::create_dir_all(flow_states_dir(&repo)).unwrap();
    fs::write(&prompt_path, "work on issue #42").unwrap();

    let output = run_start_init(
        &repo,
        "fip-test",
        &["--prompt-file", &prompt_path.to_string_lossy()],
        &stub_dir,
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(
        data["step"].as_str().unwrap_or(""),
        "flow_in_progress_label",
        "step should be flow_in_progress_label"
    );
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .contains("Flow In-Progress"),
        "message should mention the label"
    );
}

#[test]
fn test_duplicate_issue_returns_error() {
    // Another flow targets the same issue → error.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);

    // Create an existing state file that references issue #42
    let state_dir = flow_states_dir(&repo);
    fs::create_dir_all(&state_dir).unwrap();
    let existing_state = serde_json::json!({
        "schema_version": 1,
        "branch": "existing-branch",
        "current_phase": "flow-code",
        "pr_url": "https://github.com/test/repo/pull/99",
        "prompt": "work on issue #42",
        "phases": {
            "flow-start": {"status": "complete"},
            "flow-plan": {"status": "complete"},
            "flow-code": {"status": "in_progress"},
            "flow-complete": {"status": "pending"}
        }
    });
    fs::write(
        state_dir.join("existing-branch.json"),
        serde_json::to_string_pretty(&existing_state).unwrap(),
    )
    .unwrap();

    // gh stub returns a clean issue (no Flow In-Progress label)
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [[ \"$1\" == \"issue\" && \"$2\" == \"view\" ]]; then\n\
           echo '{\"title\": \"Some Issue\", \"labels\": []}'\n\
           exit 0\n\
         fi\n\
         echo \"https://github.com/test/repo/pull/42\"\n",
    );

    let prompt_path = state_dir.join("dup-start-prompt");
    fs::write(&prompt_path, "work on issue #42").unwrap();

    let output = run_start_init(
        &repo,
        "dup-test",
        &["--prompt-file", &prompt_path.to_string_lossy()],
        &stub_dir,
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(
        data["step"].as_str().unwrap_or(""),
        "duplicate_issue",
        "step should be duplicate_issue"
    );
}

#[test]
fn test_init_state_error_releases_lock() {
    // Verifies lock lifecycle: on both success and error, start-init
    // holds the lock (start-workspace releases it later). On error
    // paths, the lock IS released before returning. This test uses
    // unconditional assertions regardless of outcome.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);
    let stub_dir = create_default_gh_stub(&repo);

    let output = run_start_init(&repo, "lock-lifecycle", &[], &stub_dir);
    let data = parse_output(&output);
    let queue_dir = flow_states_dir(&repo).join("start-queue");

    if data["status"] == "ready" {
        // On success, lock is held (awaiting start-workspace release)
        assert!(
            queue_dir.join("lock-lifecycle").exists(),
            "Lock must be held after successful start-init"
        );
    } else {
        // On error, lock is released
        assert!(
            !queue_dir.join("lock-lifecycle").exists(),
            "Lock must be released on start-init error"
        );
    }
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

// --- Edge-case coverage tests ---

/// Plugin root undetectable: CLAUDE_PLUGIN_ROOT points to a dir
/// without flow-phases.json AND the flow-rs binary is in a location
/// whose parent chain doesn't reach a plugin root either. `plugin_root()`
/// returns None, `run_impl` returns Err, `run_impl_main` wraps as
/// `(err_json, 1)`.
#[test]
fn test_plugin_root_undetectable_returns_exit_1() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = create_git_repo_with_remote(&parent);
    let stub_dir = create_default_gh_stub(&repo);

    // Copy flow-rs to an isolated location. parent-chain traversal
    // from this copy won't find flow-phases.json anywhere.
    let isolated_bin_dir = parent.join("isolated-bin");
    fs::create_dir_all(&isolated_bin_dir).unwrap();
    let isolated_bin = isolated_bin_dir.join("flow-rs");
    fs::copy(env!("CARGO_BIN_EXE_flow-rs"), &isolated_bin).unwrap();
    #[cfg(unix)]
    {
        fs::set_permissions(&isolated_bin, fs::Permissions::from_mode(0o755)).unwrap();
    }

    // CLAUDE_PLUGIN_ROOT points at a dir without flow-phases.json.
    let invalid_plugin_root = parent.join("no-flow-phases-dir");
    fs::create_dir_all(&invalid_plugin_root).unwrap();

    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(&isolated_bin)
        .args(["start-init", "plugroot-none"])
        .current_dir(&repo)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", &invalid_plugin_root)
        .env_remove("FLOW_SIMULATE_BRANCH")
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "plugin_root undetectable should exit 1: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["step"], "start_init_run_impl");
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .contains("CLAUDE_PLUGIN_ROOT"),
        "error message should mention CLAUDE_PLUGIN_ROOT: {}",
        data["message"]
    );
}

/// --auto flag routes through to the init-state subprocess, which
/// translates it into fully-autonomous skill config in the state file.
#[test]
fn test_auto_flag_produces_auto_skill_config_in_state() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);
    let stub_dir = create_default_gh_stub(&repo);

    let output = run_start_init(&repo, "auto-flag-feature", &["--auto"], &stub_dir);
    let data = parse_output(&output);
    assert_eq!(data["status"], "ready");

    let branch = data["branch"].as_str().unwrap();
    let state_path = flow_states_dir(&repo).join(format!("{}.json", branch));
    let content = fs::read_to_string(&state_path).unwrap();
    let state: serde_json::Value = serde_json::from_str(&content).unwrap();

    // --auto → init-state sets every skill to "auto" continue mode.
    let skills = state["skills"].as_object().expect("skills object present");
    assert!(
        !skills.is_empty(),
        "skills config should be populated under --auto"
    );
    // At least one skill should be "auto" — verifies the flag propagated.
    let any_auto = skills.values().any(|v| {
        v.as_str() == Some("auto") || v.get("continue").and_then(|c| c.as_str()) == Some("auto")
    });
    assert!(
        any_auto,
        "at least one skill should resolve to auto continue mode under --auto"
    );
}

/// cwd outside root: when the user runs start-init from a directory
/// that isn't a subpath of the project root, `strip_prefix` returns Err
/// and `relative_cwd` falls back to empty string. Exercised by spawning
/// flow-rs with `current_dir` set to a path unrelated to the repo.
#[test]
fn test_cwd_outside_root_produces_empty_relative_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = create_git_repo_with_remote(&parent);
    write_flow_json(&repo, &current_plugin_version(), None);
    let stub_dir = create_default_gh_stub(&repo);

    // cwd is a sibling of the repo (both under `parent` but not a parent-child
    // relationship). run-impl computes relative_cwd from cwd vs project_root
    // — project_root resolves from cwd upward (via git rev-parse), so we
    // exercise the branch where the auto-detected root differs from `repo`.
    let unrelated_cwd = parent.join("unrelated-cwd");
    fs::create_dir_all(&unrelated_cwd).unwrap();
    // Initialize a git repo in the unrelated cwd so project_root resolves.
    Command::new("git")
        .args(["-c", "init.defaultBranch=main", "init"])
        .current_dir(&unrelated_cwd)
        .output()
        .unwrap();
    for (key, val) in [
        ("user.email", "test@test.com"),
        ("user.name", "Test"),
        ("commit.gpgsign", "false"),
    ] {
        Command::new("git")
            .args(["config", key, val])
            .current_dir(&unrelated_cwd)
            .output()
            .unwrap();
    }
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&unrelated_cwd)
        .output()
        .unwrap();
    write_flow_json(&unrelated_cwd, &current_plugin_version(), None);

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["start-init", "unrelated-cwd-feature"])
        .current_dir(&unrelated_cwd)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", &manifest_dir)
        .env_remove("FLOW_SIMULATE_BRANCH")
        .output()
        .unwrap();

    // We just need the happy path to succeed here — the test's purpose
    // is to exercise the branch where cwd.canonicalize vs root.canonicalize
    // produces an Err from strip_prefix (relative_cwd = "").
    let data = parse_output(&output);
    assert_eq!(data["status"], "ready", "got: {}", data);
}

/// Auto-upgrade response includes old_version and new_version fields
/// when prime_check returns `auto_upgraded: true`. Triggered by a
/// `.flow.json` file whose config/setup hashes match the current plugin
/// but whose `flow_version` field is stale.
#[test]
fn test_auto_upgrade_fields_in_response() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_default_gh_stub(&repo);
    let plug_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // Compute the current hashes so prime_check recognizes a matching
    // priming with only a stale version.
    let config_hash = flow_rs::prime_check::compute_config_hash();
    let setup_hash = flow_rs::prime_check::compute_setup_hash(&plug_root).unwrap();

    let flow_json = serde_json::json!({
        "flow_version": "0.0.1",
        "config_hash": config_hash,
        "setup_hash": setup_hash,
    });
    fs::write(
        repo.join(".flow.json"),
        serde_json::to_string_pretty(&flow_json).unwrap(),
    )
    .unwrap();

    let output = run_start_init(&repo, "auto-upgrade-feature", &[], &stub_dir);
    let data = parse_output(&output);
    assert_eq!(data["status"], "ready", "got: {}", data);
    assert_eq!(data["auto_upgraded"], true);
    assert_eq!(data["old_version"], "0.0.1");
    assert_eq!(data["new_version"], current_plugin_version());
}

/// Upgrade-available response includes the `upgrade` field when the gh
/// stub reports a newer release is available.
#[test]
fn test_upgrade_available_adds_upgrade_field() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);

    // gh stub: `gh api .../releases/latest --jq .tag_name` returns a
    // much newer tag than the current plugin version.
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [[ \"$1\" == \"issue\" && \"$2\" == \"view\" ]]; then exit 1; fi\n\
         if [[ \"$1\" == \"issue\" && \"$2\" == \"edit\" ]]; then exit 0; fi\n\
         if [[ \"$1\" == \"api\" ]]; then\n\
           echo 'v999.0.0'\n\
           exit 0\n\
         fi\n\
         echo \"https://github.com/test/repo/pull/42\"\n",
    );

    let output = run_start_init(&repo, "upgrade-avail-feature", &[], &stub_dir);
    let data = parse_output(&output);
    assert_eq!(data["status"], "ready");
    let upgrade = data.get("upgrade").expect("upgrade field present");
    assert_eq!(upgrade["status"], "upgrade_available");
    assert!(upgrade["latest"].is_string());
}

/// init-state returning a `status: error` JSON — exercised by blocking
/// the state file write. A directory is pre-created at the path where
/// init-state wants to write the state file (`<branch>.json`), so
/// `fs::write` inside init-state's create_state fails. init-state
/// emits `status:error` and exits 1. The outer start-init sees the
/// error JSON, releases the lock, and propagates the error.
#[test]
fn test_init_state_error_releases_lock_and_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), None);
    let stub_dir = create_default_gh_stub(&repo);

    // Pre-create a DIRECTORY at the state file path. fs::write fails
    // because target is a directory, not a file.
    let state_dir = flow_states_dir(&repo);
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(state_dir.join("init-err-branch.json")).unwrap();

    let output = run_start_init(&repo, "init-err-branch", &[], &stub_dir);
    let data = parse_output(&output);
    assert_eq!(data["status"], "error", "got: {}", data);

    // Lock must be released on init-state error.
    let queue_dir = flow_states_dir(&repo).join("start-queue");
    assert!(
        !queue_dir.join("init-err-branch").exists(),
        "Lock must be released on init-state error"
    );
}

/// prime_check infrastructure Err: the plugin.json at CLAUDE_PLUGIN_ROOT
/// is unreadable/malformed. `prime_check::run_impl` returns Err, which
/// start-init folds into a status:error with the Err message.
#[test]
fn test_prime_check_infrastructure_err_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = create_git_repo_with_remote(&parent);
    write_flow_json(&repo, &current_plugin_version(), None);
    let stub_dir = create_default_gh_stub(&repo);

    // Construct a plugin-root-like dir with flow-phases.json so
    // `plugin_root()` accepts it, but with plugin.json that is NOT
    // valid JSON so prime_check::run_impl returns Err.
    let fake_plugin_root = parent.join("fake-plugin-root");
    fs::create_dir_all(fake_plugin_root.join(".claude-plugin")).unwrap();
    fs::write(fake_plugin_root.join("flow-phases.json"), "{}").unwrap();
    fs::write(
        fake_plugin_root.join(".claude-plugin").join("plugin.json"),
        "not valid json at all",
    )
    .unwrap();

    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["start-init", "prime-infra-err"])
        .current_dir(&repo)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", &fake_plugin_root)
        .env_remove("FLOW_SIMULATE_BRANCH")
        .output()
        .unwrap();
    let data = parse_output(&output);
    assert_eq!(data["status"], "error", "got: {}", data);
    let msg = data["message"].as_str().unwrap_or("");
    // prime_check's Err message mentions parsing plugin.json.
    assert!(
        msg.to_lowercase().contains("plugin.json") || msg.to_lowercase().contains("parse"),
        "expected prime_check infrastructure error, got: {}",
        msg
    );

    // Lock is released on prime-check error path.
    let queue_dir = flow_states_dir(&repo).join("start-queue");
    assert!(
        !queue_dir.join("prime-infra-err").exists(),
        "Lock must be released on prime-check infrastructure error"
    );
}
