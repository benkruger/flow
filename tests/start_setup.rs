//! Integration tests for start-setup subcommand (port of test_start_setup.py).

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

    // Configure git
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

/// Write .flow.json with version, framework, and optional skills.
fn write_flow_json(repo: &Path, version: &str, framework: &str, skills: Option<&Value>) {
    let mut data = json!({
        "flow_version": version,
        "framework": framework,
    });
    if let Some(sk) = skills {
        data["skills"] = sk.clone();
    }
    fs::write(repo.join(".flow.json"), data.to_string()).unwrap();
}

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

/// Create a custom gh stub script. Returns the stub directory.
fn create_gh_stub(repo: &Path, script: &str) -> PathBuf {
    let stub_dir = repo.join(".stub-bin");
    fs::create_dir_all(&stub_dir).unwrap();
    let gh_stub = stub_dir.join("gh");
    fs::write(&gh_stub, script).unwrap();
    fs::set_permissions(&gh_stub, fs::Permissions::from_mode(0o755)).unwrap();
    stub_dir
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

/// Parse JSON from stdout.
fn parse_output(output: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim()).unwrap_or_else(|_| json!({"raw": stdout.trim()}))
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
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [[ \"$1\" == \"issue\" && \"$2\" == \"view\" ]]; then\n\
           echo \"Organize settings allow list\"\n\
         else\n\
           echo \"https://github.com/test/repo/pull/42\"\n\
         fi\n",
    );

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

#[test]
fn duplicate_issue_guard_integration() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    write_flow_json(&repo, &current_plugin_version(), "rails", None);
    let stub_dir = create_default_gh_stub(&repo);

    // Create existing state file referencing issue #999
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join("existing-flow.json"),
        json!({
            "prompt": "work on issue #999",
            "branch": "existing-flow",
            "current_phase": "flow-code",
            "pr_url": "https://github.com/test/repo/pull/50",
        })
        .to_string(),
    )
    .unwrap();

    let prompt_path = repo.join(".flow-start-prompt");
    fs::write(&prompt_path, "work on issue #999").unwrap();

    let output = run_start_setup(
        &repo,
        "new-attempt",
        &[
            "--skip-pull",
            "--prompt-file",
            &prompt_path.to_string_lossy(),
        ],
        &stub_dir,
    );
    assert_ne!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["step"], "duplicate_issue");
    assert!(data["message"].as_str().unwrap().contains("existing-flow"));
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
