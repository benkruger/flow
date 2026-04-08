//! Integration tests for start-setup subcommand (port of test_start_setup.py).

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

use common::{create_gh_stub, create_git_repo_with_remote, current_plugin_version, parse_output, write_flow_json};

// --- Test helpers ---

/// Create a gh stub script that returns a fake PR URL.
/// For issue view, exits 1 (no issue found).
fn create_default_gh_stub(repo: &Path) -> PathBuf {
    create_gh_stub(
        repo,
        "#!/bin/bash\n\
         if [[ \"$1\" == \"issue\" && \"$2\" == \"view\" ]]; then exit 1; fi\n\
         echo \"https://github.com/test/repo/pull/42\"\n",
    )
}

/// Run flow-rs start-setup with the given arguments in a test repo.
fn run_start_setup(repo: &Path, feature_name: &str, extra_args: &[&str], stub_dir: &Path) -> Output {
    let mut args = vec!["start-setup", feature_name];
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
        .env_remove("FLOW_SIMULATE_BRANCH")
        .output()
        .unwrap()
}

// --- Happy path tests ---

#[test]
fn happy_path_returns_ok_json() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "rails", None);
    let stub_dir = create_default_gh_stub(&repo);

    let output = run_start_setup(&repo, "test feature", &["--skip-pull"], &stub_dir);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["worktree"], ".worktrees/test-feature");
    assert_eq!(data["feature"], "Test Feature");
    assert_eq!(data["branch"], "test-feature");
    assert_eq!(data["pr_url"], "https://github.com/test/repo/pull/42");
    assert_eq!(data["pr_number"], 42);
}

#[test]
fn worktree_created() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "rails", None);
    let stub_dir = create_default_gh_stub(&repo);

    let output = run_start_setup(&repo, "wt test", &["--skip-pull"], &stub_dir);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert!(repo.join(".worktrees").join("wt-test").is_dir());
}

#[test]
fn state_file_created_with_all_phases() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "rails", None);
    let stub_dir = create_default_gh_stub(&repo);

    let output = run_start_setup(&repo, "state test", &["--skip-pull"], &stub_dir);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    let state_path = repo.join(".flow-states").join("state-test.json");
    assert!(state_path.exists());
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();

    assert_eq!(state["branch"], "state-test");
    assert_eq!(state["current_phase"], "flow-start");
    assert_eq!(state["schema_version"], 1);
    assert!(state["notes"].as_array().unwrap().is_empty());

    // All 6 phases
    let phases = state["phases"].as_object().unwrap();
    assert_eq!(phases.len(), 6);
    assert_eq!(phases["flow-start"]["status"], "in_progress");
    for key in ["flow-plan", "flow-code", "flow-code-review", "flow-learn", "flow-complete"] {
        assert_eq!(phases[key]["status"], "pending");
    }
}

#[test]
fn state_file_has_files_block() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "rails", None);
    let stub_dir = create_default_gh_stub(&repo);

    run_start_setup(&repo, "files test", &["--skip-pull"], &stub_dir);

    let state_path = repo.join(".flow-states").join("files-test.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    let files = &state["files"];
    assert!(files["plan"].is_null());
    assert!(files["dag"].is_null());
    assert_eq!(files["log"], ".flow-states/files-test.log");
    assert_eq!(files["state"], ".flow-states/files-test.json");
}

#[test]
fn log_file_created() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "rails", None);
    let stub_dir = create_default_gh_stub(&repo);

    run_start_setup(&repo, "log test", &["--skip-pull"], &stub_dir);

    let log_path = repo.join(".flow-states").join("log-test.log");
    assert!(log_path.exists());
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains("[Phase 1]"));
}

#[test]
fn missing_feature_name_fails() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["start-setup"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    // clap exits with code 2 for missing required args
    assert_ne!(output.status.code(), Some(0));
}

#[test]
fn missing_flow_json_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // No write_flow_json — .flow.json is absent
    let stub_dir = create_default_gh_stub(&repo);

    let output = run_start_setup(&repo, "no flow json", &["--skip-pull"], &stub_dir);
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["step"], "flow_json");
}

#[test]
fn skip_pull_omits_git_pull_from_log() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "rails", None);
    let stub_dir = create_default_gh_stub(&repo);

    run_start_setup(&repo, "skip pull", &["--skip-pull"], &stub_dir);

    let log_path = repo.join(".flow-states").join("skip-pull.log");
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(!log.contains("git pull"));
    assert!(log.contains("git worktree add"));
}

#[test]
fn venv_symlink_created() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "rails", None);
    let stub_dir = create_default_gh_stub(&repo);

    // Create .venv dir
    let venv_dir = repo.join(".venv");
    fs::create_dir_all(venv_dir.join("bin")).unwrap();
    fs::write(venv_dir.join("bin").join("python3"), "fake").unwrap();

    let output = run_start_setup(&repo, "venv test", &["--skip-pull"], &stub_dir);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    let wt_venv = repo.join(".worktrees").join("venv-test").join(".venv");
    assert!(wt_venv.is_symlink());
    let target = fs::read_link(&wt_venv).unwrap();
    assert_eq!(target, PathBuf::from("../../.venv"));
}

#[test]
fn framework_propagated_to_state() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "python", None);
    let stub_dir = create_default_gh_stub(&repo);

    run_start_setup(&repo, "framework test", &["--skip-pull"], &stub_dir);

    let state_path = repo.join(".flow-states").join("framework-test.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["framework"], "python");
}

#[test]
fn auto_flag_overrides_skills() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "rails", None);
    let stub_dir = create_default_gh_stub(&repo);

    let output = run_start_setup(&repo, "auto test", &["--skip-pull", "--auto"], &stub_dir);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    let state_path = repo.join(".flow-states").join("auto-test.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert!(state.get("skills").is_some());
    assert_eq!(state["skills"]["flow-code"]["commit"], "auto");
    assert_eq!(state["skills"]["flow-code"]["continue"], "auto");
    assert_eq!(state["skills"]["flow-abort"], "auto");
}

#[test]
fn prompt_file_stores_content_in_state() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "rails", None);
    let stub_dir = create_default_gh_stub(&repo);

    let prompt_path = repo.join(".flow-start-prompt");
    fs::write(&prompt_path, "fix issue #228 with URLs https://github.com/org/repo").unwrap();

    let output = run_start_setup(
        &repo,
        "prompt file test",
        &["--skip-pull", "--prompt-file", &prompt_path.to_string_lossy()],
        &stub_dir,
    );
    assert_eq!(output.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    let state_path = repo.join(".flow-states").join("prompt-file-test.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(
        state["prompt"],
        "fix issue #228 with URLs https://github.com/org/repo"
    );
    assert!(!prompt_path.exists(), "Prompt file should be deleted");
}

#[test]
fn skills_from_flow_json_propagated() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let skills = json!({
        "flow-start": {"continue": "auto"},
        "flow-code": {"commit": "manual", "continue": "manual"},
    });
    write_flow_json(&repo, &current_plugin_version(), "rails", Some(&skills));
    let stub_dir = create_default_gh_stub(&repo);

    let output = run_start_setup(&repo, "skills test", &["--skip-pull"], &stub_dir);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    let state_path = repo.join(".flow-states").join("skills-test.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert!(state.get("skills").is_some());
    assert_eq!(state["skills"]["flow-code"]["commit"], "manual");
}

#[test]
fn no_skills_in_flow_json_omits_from_state() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "rails", None);
    let stub_dir = create_default_gh_stub(&repo);

    let output = run_start_setup(&repo, "no skills", &["--skip-pull"], &stub_dir);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    let state_path = repo.join(".flow-states").join("no-skills.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert!(state.get("skills").is_none());
}

#[test]
fn issue_title_used_for_branch_naming() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "rails", None);
    let stub_dir = create_default_gh_stub(&repo);

    // Pre-seed state file (simulating init_state with issue-title naming)
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join("organize-settings-allow-list.json"),
        json!({
            "schema_version": 1,
            "branch": "organize-settings-allow-list",
            "repo": null,
            "pr_number": null,
            "pr_url": null,
            "prompt": "work on issue #309",
            "current_phase": "flow-start",
            "phases": {},
        })
        .to_string(),
    )
    .unwrap();

    let prompt_path = repo.join(".flow-start-prompt");
    fs::write(&prompt_path, "work on issue #309").unwrap();

    let output = run_start_setup(
        &repo,
        "work-on-issue",
        &[
            "--skip-pull",
            "--prompt-file",
            &prompt_path.to_string_lossy(),
        ],
        &stub_dir,
    );
    assert_eq!(output.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let data = parse_output(&output);
    // start_setup reads the canonical branch from the state file, not feature_name
    assert_eq!(data["branch"], "organize-settings-allow-list");
    assert_eq!(data["feature"], "Organize Settings Allow List");
}

#[test]
fn frozen_phases_file_created() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "rails", None);
    let stub_dir = create_default_gh_stub(&repo);

    let output = run_start_setup(&repo, "frozen test", &["--skip-pull"], &stub_dir);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    let frozen = repo.join(".flow-states").join("frozen-test-phases.json");
    assert!(frozen.exists(), "Frozen phases file not created");

    // Verify it matches source
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source: Value = serde_json::from_str(
        &fs::read_to_string(manifest_dir.join("flow-phases.json")).unwrap(),
    )
    .unwrap();
    let frozen_data: Value =
        serde_json::from_str(&fs::read_to_string(&frozen).unwrap()).unwrap();
    assert_eq!(frozen_data, source);
}

// --- Backfill mode tests ---

#[test]
fn backfill_updates_pr_fields() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "rails", None);
    let stub_dir = create_default_gh_stub(&repo);

    // Pre-seed state file with null PR fields (simulating init-state)
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let pre_state = json!({
        "schema_version": 1,
        "branch": "backfill-test",
        "repo": null,
        "pr_number": null,
        "pr_url": null,
        "started_at": "2026-01-01T00:00:00-08:00",
        "current_phase": "flow-start",
        "framework": "rails",
        "files": {
            "plan": null,
            "dag": null,
            "log": ".flow-states/backfill-test.log",
            "state": ".flow-states/backfill-test.json"
        },
        "session_tty": null,
        "session_id": null,
        "transcript_path": null,
        "notes": [],
        "prompt": "original prompt",
        "phases": {},
        "phase_transitions": []
    });
    fs::write(
        state_dir.join("backfill-test.json"),
        serde_json::to_string_pretty(&pre_state).unwrap(),
    )
    .unwrap();

    let output = run_start_setup(&repo, "backfill test", &["--skip-pull"], &stub_dir);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    let post_state: Value = serde_json::from_str(
        &fs::read_to_string(state_dir.join("backfill-test.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(post_state["pr_number"], 42);
    assert_eq!(
        post_state["pr_url"],
        "https://github.com/test/repo/pull/42"
    );
    // Original fields preserved
    assert_eq!(post_state["started_at"], "2026-01-01T00:00:00-08:00");
}

#[test]
fn state_file_has_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "rails", None);
    let stub_dir = create_default_gh_stub(&repo);

    run_start_setup(&repo, "prompt test", &["--skip-pull"], &stub_dir);

    let state_path = repo.join(".flow-states").join("prompt-test.json");
    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["prompt"], "prompt test");
}

#[test]
fn git_pull_failure_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    // Init a repo without a remote
    Command::new("git")
        .args(["-c", "init.defaultBranch=main", "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    for (key, val) in [
        ("user.email", "test@test.com"),
        ("user.name", "Test"),
        ("commit.gpgsign", "false"),
    ] {
        Command::new("git")
            .args(["config", key, val])
            .current_dir(dir.path())
            .output()
            .unwrap();
    }
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    write_flow_json(dir.path(), &current_plugin_version(), "rails", None);
    let stub_dir = create_default_gh_stub(dir.path());

    // Don't pass --skip-pull, so it tries to pull and fails
    let output = run_start_setup(dir.path(), "pull fail", &[], &stub_dir);
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["step"], "git_pull");
}

// --- Branch resolution tests ---

#[test]
fn branch_flag_short_circuits_state_file_lookup() {
    // When --branch is passed, start-setup should use it directly
    // without needing the feature name to match any state file.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "rails", None);
    let stub_dir = create_default_gh_stub(&repo);

    // Pre-seed a state file whose name differs from the feature name
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join("my-custom-branch.json"),
        json!({
            "schema_version": 1,
            "branch": "my-custom-branch",
            "repo": null,
            "pr_number": null,
            "pr_url": null,
            "prompt": "some prompt",
            "current_phase": "flow-start",
            "phases": {},
        })
        .to_string(),
    )
    .unwrap();

    let output = run_start_setup(
        &repo,
        "unrelated-feature-name",
        &["--skip-pull", "--branch", "my-custom-branch"],
        &stub_dir,
    );
    assert_eq!(output.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let data = parse_output(&output);
    assert_eq!(data["branch"], "my-custom-branch");
    assert_eq!(data["feature"], "My Custom Branch");
}

#[test]
fn branch_flag_with_issue_derived_name() {
    // Simulates the real SKILL.md Step 11 flow: init-state derived
    // "organize-settings-allow-list" from issue title, but feature name
    // passed to start-setup is "work-on-issue-309".
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "rails", None);
    let stub_dir = create_default_gh_stub(&repo);

    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join("organize-settings-allow-list.json"),
        json!({
            "schema_version": 1,
            "branch": "organize-settings-allow-list",
            "repo": null,
            "pr_number": null,
            "pr_url": null,
            "prompt": "work on issue #309",
            "current_phase": "flow-start",
            "phases": {},
        })
        .to_string(),
    )
    .unwrap();

    let output = run_start_setup(
        &repo,
        "work-on-issue-309",
        &["--skip-pull", "--branch", "organize-settings-allow-list"],
        &stub_dir,
    );
    assert_eq!(output.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let data = parse_output(&output);
    assert_eq!(data["branch"], "organize-settings-allow-list");
    assert_eq!(data["feature"], "Organize Settings Allow List");
}

#[test]
fn multiple_state_files_without_branch_flag_picks_wrong_one() {
    // Documents the bug from issue #828: when the feature name doesn't
    // match any state file exactly, find_state_files returns ALL files
    // sorted alphabetically and state_files[0] picks the wrong one.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "rails", None);
    let stub_dir = create_default_gh_stub(&repo);

    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();

    // State file for another flow — sorts first alphabetically
    fs::write(
        state_dir.join("alpha-flow.json"),
        json!({
            "schema_version": 1,
            "branch": "alpha-flow",
            "repo": null,
            "pr_number": null,
            "pr_url": null,
            "prompt": "some other feature",
            "current_phase": "flow-start",
            "phases": {},
        })
        .to_string(),
    )
    .unwrap();

    // State file for this flow — correct one, sorts second
    fs::write(
        state_dir.join("port-issue-close.json"),
        json!({
            "schema_version": 1,
            "branch": "port-issue-close",
            "repo": null,
            "pr_number": null,
            "pr_url": null,
            "prompt": "work on issue #772",
            "current_phase": "flow-start",
            "phases": {},
        })
        .to_string(),
    )
    .unwrap();

    // Feature name doesn't match either state file exactly
    let output = run_start_setup(
        &repo,
        "work-on-issue-772",
        &["--skip-pull"],
        &stub_dir,
    );
    assert_eq!(output.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let data = parse_output(&output);
    // Bug: picks alpha-flow (first alphabetically) instead of port-issue-close
    assert_eq!(
        data["branch"], "alpha-flow",
        "Without --branch, start-setup picks the first state file alphabetically (bug #828)"
    );
}

// --- Tombstone tests: naming logic moved to init_state in PR #823 ---

#[test]
fn tombstone_no_fetch_issue_title_in_start_setup() {
    // Tombstone: removed in PR #823. Must not return.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let content = fs::read_to_string(manifest_dir.join("src/start_setup.rs")).unwrap();
    assert!(
        !content.contains("fetch_issue_title("),
        "fetch_issue_title was moved to init_state.rs — start_setup must not call it directly"
    );
}

#[test]
fn tombstone_no_check_duplicate_issue_in_start_setup() {
    // Tombstone: removed in PR #823. Must not return.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let content = fs::read_to_string(manifest_dir.join("src/start_setup.rs")).unwrap();
    assert!(
        !content.contains("check_duplicate_issue("),
        "check_duplicate_issue was moved to init_state.rs — start_setup must not call it directly"
    );
}

// --- Tombstone tests: Python files removed in PR #810 ---

#[test]
fn tombstone_python_start_setup_deleted() {
    // Tombstone: removed in PR #810. Must not return.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    assert!(
        !manifest_dir.join("lib/start-setup.py").exists(),
        "lib/start-setup.py was ported to Rust (src/start_setup.rs) and must not be re-added"
    );
}

#[test]
fn tombstone_python_test_start_setup_deleted() {
    // Tombstone: removed in PR #810. Must not return.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    assert!(
        !manifest_dir.join("tests/test_start_setup.py").exists(),
        "tests/test_start_setup.py was ported to Rust (tests/start_setup.rs) and must not be re-added"
    );
}
