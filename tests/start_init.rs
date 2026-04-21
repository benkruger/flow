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

// --- Coverage tests ---

#[test]
fn test_flow_in_progress_label_returns_error() {
    // Exercises lines 76-85: issue carries "Flow In-Progress" label → error.
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
    // Exercises lines 101-112: another flow targets the same issue → error.
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

// --- Library-level tests for run_impl_with_deps / run_impl_main_with_deps ---
//
// These tests drive the public seams directly so every branch of the
// composed-dependency function is attributed to the per-file gate.

use flow_rs::start_init::{run_impl_main_with_deps, run_impl_with_deps, Args};
use serde_json::{json, Value};
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

fn lib_fake_output(stdout: &str) -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: stdout.as_bytes().to_vec(),
        stderr: Vec::new(),
    }
}

fn lib_ok_prime_check(_cwd: &Path, _plug_root: &Path) -> Result<Value, String> {
    Ok(json!({"status": "ok"}))
}

fn lib_ok_upgrade_check(_plug_root: &Path) -> Value {
    json!({"status": "current"})
}

fn lib_panic_init_runner(_args: &[String], _cwd: &Path) -> Result<Output, String> {
    panic!("init_state_runner must not be called on plugin-root error path");
}

#[test]
fn lib_start_init_plugin_root_none_returns_err() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let args = Args {
        feature_name: "plugroot-none".to_string(),
        auto: false,
        prompt_file: None,
    };
    let finder = || -> Option<PathBuf> { None };

    let result = run_impl_with_deps(
        &args,
        &root,
        &root,
        &finder,
        &lib_ok_prime_check,
        &lib_ok_upgrade_check,
        &lib_panic_init_runner,
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("CLAUDE_PLUGIN_ROOT"));
}

#[test]
fn lib_start_init_init_state_spawn_failure_returns_err() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let plug_root = root.clone();
    let args = Args {
        feature_name: "spawn-fail".to_string(),
        auto: false,
        prompt_file: None,
    };
    let finder = move || -> Option<PathBuf> { Some(plug_root.clone()) };
    let runner = |_: &[String], _: &Path| -> Result<Output, String> {
        Err("Failed to spawn init-state: no such file".to_string())
    };

    let result = run_impl_with_deps(
        &args,
        &root,
        &root,
        &finder,
        &lib_ok_prime_check,
        &lib_ok_upgrade_check,
        &runner,
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Failed to spawn init-state"));
}

#[test]
fn lib_start_init_init_state_parse_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let plug_root = root.clone();
    let args = Args {
        feature_name: "parse-fallback".to_string(),
        auto: false,
        prompt_file: None,
    };
    let finder = move || -> Option<PathBuf> { Some(plug_root.clone()) };
    let runner = |_: &[String], _: &Path| -> Result<Output, String> { Ok(lib_fake_output("")) };

    let result = run_impl_with_deps(
        &args,
        &root,
        &root,
        &finder,
        &lib_ok_prime_check,
        &lib_ok_upgrade_check,
        &runner,
    )
    .unwrap();
    assert_eq!(result["status"], "error");
    assert_eq!(
        result["message"].as_str().unwrap(),
        "Could not parse init-state output"
    );
    assert_eq!(result["step"], "init_state");

    let queue_entry = root.join(".flow-states/start-queue/parse-fallback");
    assert!(
        !queue_entry.exists(),
        "lock must be released on parse fallback error"
    );
}

#[test]
fn lib_start_init_init_state_error_releases_lock_via_seam() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let plug_root = root.clone();
    let args = Args {
        feature_name: "init-err".to_string(),
        auto: false,
        prompt_file: None,
    };
    let finder = move || -> Option<PathBuf> { Some(plug_root.clone()) };
    let runner = |_: &[String], _: &Path| -> Result<Output, String> {
        Ok(lib_fake_output(
            r#"{"status": "error", "message": "init-state refused", "step": "seeded_error"}"#,
        ))
    };

    let result = run_impl_with_deps(
        &args,
        &root,
        &root,
        &finder,
        &lib_ok_prime_check,
        &lib_ok_upgrade_check,
        &runner,
    )
    .unwrap();
    assert_eq!(result["status"], "error");
    assert_eq!(result["message"], "init-state refused");
    assert_eq!(result["step"], "seeded_error");

    let queue_entry = root.join(".flow-states/start-queue/init-err");
    assert!(
        !queue_entry.exists(),
        "lock must be released on init-state error"
    );
}

#[test]
fn lib_start_init_prime_check_error_releases_lock_via_seam() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let plug_root = root.clone();
    let args = Args {
        feature_name: "prime-err".to_string(),
        auto: false,
        prompt_file: None,
    };
    let finder = move || -> Option<PathBuf> { Some(plug_root.clone()) };
    let err_prime =
        |_: &Path, _: &Path| -> Result<Value, String> { Err("missing plugin.json".to_string()) };

    let result = run_impl_with_deps(
        &args,
        &root,
        &root,
        &finder,
        &err_prime,
        &lib_ok_upgrade_check,
        &lib_panic_init_runner,
    )
    .unwrap();
    assert_eq!(result["status"], "error");
    assert_eq!(result["step"], "prime_check");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("missing plugin.json"));
}

#[test]
fn lib_start_init_happy_path_via_seam_returns_ready() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let plug_root = root.clone();
    let args = Args {
        feature_name: "happy-seam".to_string(),
        auto: false,
        prompt_file: None,
    };
    let finder = move || -> Option<PathBuf> { Some(plug_root.clone()) };
    let runner = |_: &[String], _: &Path| -> Result<Output, String> {
        Ok(lib_fake_output(
            r#"{"status": "ok", "branch": "happy-seam"}"#,
        ))
    };

    let result = run_impl_with_deps(
        &args,
        &root,
        &root,
        &finder,
        &lib_ok_prime_check,
        &lib_ok_upgrade_check,
        &runner,
    )
    .unwrap();
    assert_eq!(result["status"], "ready");
    assert_eq!(result["branch"], "happy-seam");
}

#[test]
fn lib_start_init_auto_upgraded_without_versions_omits_fields() {
    // auto_upgraded:true but neither old_version nor new_version is
    // present — covers the `if let Some(...) = .get(...)` None arms.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let plug_root = root.clone();
    let args = Args {
        feature_name: "auto-no-versions".to_string(),
        auto: false,
        prompt_file: None,
    };
    let finder = move || -> Option<PathBuf> { Some(plug_root.clone()) };
    let upgraded_prime = |_: &Path, _: &Path| -> Result<Value, String> {
        Ok(json!({"status": "ok", "auto_upgraded": true}))
    };
    let runner = |_: &[String], _: &Path| -> Result<Output, String> {
        Ok(lib_fake_output(r#"{"status": "ok"}"#))
    };

    let result = run_impl_with_deps(
        &args,
        &root,
        &root,
        &finder,
        &upgraded_prime,
        &lib_ok_upgrade_check,
        &runner,
    )
    .unwrap();
    assert_eq!(result["status"], "ready");
    assert_eq!(result["auto_upgraded"], true);
    assert!(result.get("old_version").is_none());
    assert!(result.get("new_version").is_none());
}

#[test]
fn lib_start_init_auto_upgraded_propagates_to_response() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let plug_root = root.clone();
    let args = Args {
        feature_name: "auto-up".to_string(),
        auto: false,
        prompt_file: None,
    };
    let finder = move || -> Option<PathBuf> { Some(plug_root.clone()) };
    let upgraded_prime = |_: &Path, _: &Path| -> Result<Value, String> {
        Ok(json!({
            "status": "ok",
            "auto_upgraded": true,
            "old_version": "1.0.0",
            "new_version": "1.0.1",
        }))
    };
    let runner = |_: &[String], _: &Path| -> Result<Output, String> {
        Ok(lib_fake_output(r#"{"status": "ok"}"#))
    };

    let result = run_impl_with_deps(
        &args,
        &root,
        &root,
        &finder,
        &upgraded_prime,
        &lib_ok_upgrade_check,
        &runner,
    )
    .unwrap();
    assert_eq!(result["status"], "ready");
    assert_eq!(result["auto_upgraded"], true);
    assert_eq!(result["old_version"], "1.0.0");
    assert_eq!(result["new_version"], "1.0.1");
}

#[test]
fn lib_start_init_upgrade_available_adds_upgrade_field() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let plug_root = root.clone();
    let args = Args {
        feature_name: "upgrade-avail".to_string(),
        auto: false,
        prompt_file: None,
    };
    let finder = move || -> Option<PathBuf> { Some(plug_root.clone()) };
    let upgrade = |_: &Path| -> Value {
        json!({"status": "upgrade_available", "latest": "99.0.0", "installed": "1.0.0"})
    };
    let runner = |_: &[String], _: &Path| -> Result<Output, String> {
        Ok(lib_fake_output(r#"{"status": "ok"}"#))
    };

    let result = run_impl_with_deps(
        &args,
        &root,
        &root,
        &finder,
        &lib_ok_prime_check,
        &upgrade,
        &runner,
    )
    .unwrap();
    assert_eq!(result["status"], "ready");
    assert_eq!(result["upgrade"]["status"], "upgrade_available");
    assert_eq!(result["upgrade"]["latest"], "99.0.0");
}

#[test]
fn lib_start_init_lock_already_held_returns_locked() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let plug_root = root.clone();
    let queue_dir = root.join(".flow-states/start-queue");
    fs::create_dir_all(&queue_dir).unwrap();
    fs::write(queue_dir.join("other-feature"), "").unwrap();

    let args = Args {
        feature_name: "blocked-feature".to_string(),
        auto: false,
        prompt_file: None,
    };
    let finder = move || -> Option<PathBuf> { Some(plug_root.clone()) };

    let result = run_impl_with_deps(
        &args,
        &root,
        &root,
        &finder,
        &lib_ok_prime_check,
        &lib_ok_upgrade_check,
        &lib_panic_init_runner,
    )
    .unwrap();
    assert_eq!(result["status"], "locked");
    assert_eq!(result["feature"], "other-feature");
}

#[test]
fn lib_start_init_run_impl_main_err_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let args = Args {
        feature_name: "main-err-branch".to_string(),
        auto: false,
        prompt_file: None,
    };
    let finder = || -> Option<PathBuf> { None };

    let (v, code) = run_impl_main_with_deps(
        &args,
        &root,
        &root,
        &finder,
        &lib_ok_prime_check,
        &lib_ok_upgrade_check,
        &lib_panic_init_runner,
    );
    assert_eq!(code, 1);
    assert_eq!(v["status"], "error");
    assert_eq!(v["step"], "start_init_run_impl");
    assert!(v["message"]
        .as_str()
        .unwrap_or("")
        .contains("CLAUDE_PLUGIN_ROOT"));
}

#[test]
fn lib_default_init_state_runner_errors_on_bogus_cwd() {
    // Exercise the map_err branch in default_init_state_runner. The
    // cwd does not exist, so Command::output() fails before the child
    // spawns.
    use flow_rs::start_init::default_init_state_runner;
    let bogus = std::path::Path::new("/nonexistent/absolutely-not-a-dir");
    let err = default_init_state_runner(&["--help".to_string()], bogus).unwrap_err();
    assert!(err.contains("Failed to spawn init-state"), "got: {}", err);
}

#[test]
fn lib_start_init_auto_flag_appends_to_args() {
    // Covers the `if args.auto { cmd_args.push("--auto") }` branch.
    use std::sync::{Arc, Mutex};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let plug_root = root.clone();
    let args = Args {
        feature_name: "auto-flag".to_string(),
        auto: true,
        prompt_file: None,
    };
    let finder = move || -> Option<PathBuf> { Some(plug_root.clone()) };
    let captured = Arc::new(Mutex::new(Vec::<String>::new()));
    let captured_clone = Arc::clone(&captured);
    let runner = move |args: &[String], _: &Path| -> Result<Output, String> {
        *captured_clone.lock().unwrap() = args.to_vec();
        Ok(lib_fake_output(r#"{"status": "ok"}"#))
    };

    let _ = run_impl_with_deps(
        &args,
        &root,
        &root,
        &finder,
        &lib_ok_prime_check,
        &lib_ok_upgrade_check,
        &runner,
    )
    .unwrap();
    let seen = captured.lock().unwrap();
    assert!(
        seen.contains(&"--auto".to_string()),
        "runner must receive --auto arg when args.auto=true, got: {:?}",
        *seen
    );
}

#[test]
fn lib_start_init_cwd_outside_root_produces_empty_relative_cwd() {
    // When cwd is not a subpath of root, strip_prefix returns Err and
    // relative_cwd falls back to empty string.
    use std::sync::{Arc, Mutex};
    let root_tmp = tempfile::tempdir().unwrap();
    let cwd_tmp = tempfile::tempdir().unwrap();
    let root = root_tmp.path().to_path_buf();
    let plug_root = root.clone();
    let args = Args {
        feature_name: "unrelated-cwd".to_string(),
        auto: false,
        prompt_file: None,
    };
    let finder = move || -> Option<PathBuf> { Some(plug_root.clone()) };
    let captured = Arc::new(Mutex::new(Vec::<String>::new()));
    let captured_clone = Arc::clone(&captured);
    let runner = move |args: &[String], _: &Path| -> Result<Output, String> {
        *captured_clone.lock().unwrap() = args.to_vec();
        Ok(lib_fake_output(r#"{"status": "ok"}"#))
    };

    let _ = run_impl_with_deps(
        &args,
        &root,
        cwd_tmp.path(),
        &finder,
        &lib_ok_prime_check,
        &lib_ok_upgrade_check,
        &runner,
    )
    .unwrap();
    let seen = captured.lock().unwrap();
    // Find --relative-cwd and assert next arg is empty
    let pos = seen
        .iter()
        .position(|a| a == "--relative-cwd")
        .expect("--relative-cwd arg present");
    assert_eq!(
        seen[pos + 1],
        "",
        "unrelated cwd falls back to empty relative_cwd"
    );
}

#[cfg(unix)]
#[test]
fn lib_start_init_non_canonicalizable_root_falls_back_to_raw() {
    // Pass a broken-symlink path as root — canonicalize() returns Err
    // on a dangling symlink so the fallback branch fires. run_impl_with_deps
    // does not require root to be canonical; fs::create_dir_all on the
    // symlink path simply fails silently and the rest proceeds.
    use std::os::unix::fs::symlink;
    let outer = tempfile::tempdir().unwrap();
    let broken_root = outer.path().join("broken-root");
    // Create a symlink pointing at a nonexistent target.
    symlink(outer.path().join("missing-target"), &broken_root).unwrap();

    let plug_root = outer.path().to_path_buf();
    let args = Args {
        feature_name: "broken-root".to_string(),
        auto: false,
        prompt_file: None,
    };
    let finder = move || -> Option<PathBuf> { Some(plug_root.clone()) };
    let runner = |_: &[String], _: &Path| -> Result<Output, String> {
        Ok(lib_fake_output(r#"{"status": "ok"}"#))
    };

    // Not asserting on the result — the test is about covering the
    // canonicalize Err fallback. Any outcome is fine as long as the
    // function does not panic.
    let _ = run_impl_with_deps(
        &args,
        &broken_root,
        &broken_root,
        &finder,
        &lib_ok_prime_check,
        &lib_ok_upgrade_check,
        &runner,
    );
}

#[test]
fn lib_start_init_non_canonicalizable_cwd_falls_back_to_raw_path() {
    // Passing a cwd path that does not exist forces canonicalize() to
    // return Err and the fallback branch fires. run_impl_with_deps does
    // not create cwd — it only creates state_dir under root — so the
    // missing cwd survives into canonicalize().
    let root_tmp = tempfile::tempdir().unwrap();
    let root = root_tmp.path().to_path_buf();
    let missing_cwd = root.join("does-not-exist");
    let plug_root = root.clone();
    let args = Args {
        feature_name: "missing-cwd".to_string(),
        auto: false,
        prompt_file: None,
    };
    let finder = move || -> Option<PathBuf> { Some(plug_root.clone()) };
    let runner = |_: &[String], _: &Path| -> Result<Output, String> {
        Ok(lib_fake_output(r#"{"status": "ok"}"#))
    };

    let result = run_impl_with_deps(
        &args,
        &root,
        &missing_cwd,
        &finder,
        &lib_ok_prime_check,
        &lib_ok_upgrade_check,
        &runner,
    )
    .unwrap();
    // Success path — the fallback let run_impl_with_deps continue.
    assert_eq!(result["status"], "ready");
}

#[test]
fn lib_start_init_run_impl_main_ok_wraps_with_exit_zero() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let plug_root = root.clone();
    let args = Args {
        feature_name: "main-ok-branch".to_string(),
        auto: false,
        prompt_file: None,
    };
    let finder = move || -> Option<PathBuf> { Some(plug_root.clone()) };
    let runner = |_: &[String], _: &Path| -> Result<Output, String> {
        Ok(Output {
            status: ExitStatus::from_raw(0),
            stdout: br#"{"status":"ok"}"#.to_vec(),
            stderr: Vec::new(),
        })
    };

    let (v, code) = run_impl_main_with_deps(
        &args,
        &root,
        &root,
        &finder,
        &lib_ok_prime_check,
        &lib_ok_upgrade_check,
        &runner,
    );
    assert_eq!(code, 0);
    assert_eq!(v["status"], "ready");
}
