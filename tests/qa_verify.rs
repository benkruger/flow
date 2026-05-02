//! Integration tests for `src/qa_verify.rs`. Drive through the public
//! `run_impl` entry and subprocess spawns of the compiled binary — no
//! private helpers imported per `.claude/rules/test-placement.md`.

use std::fs;
use std::path::Path;
use std::process::Command;

use flow_rs::qa_verify;
use serde_json::Value;

fn install_fake_gh(dir: &Path, stdout: &str, exit_code: u8) -> std::path::PathBuf {
    let bin_dir = dir.join("fakebin");
    fs::create_dir_all(&bin_dir).unwrap();
    let script = format!(
        "#!/usr/bin/env bash\ncat <<'EOF'\n{}\nEOF\nexit {}\n",
        stdout, exit_code
    );
    let gh = bin_dir.join("gh");
    fs::write(&gh, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin_dir
}

fn run_impl_with_fake_gh(fake_gh_dir: &Path, repo: &str, project_root: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    let path_with_fake = format!(
        "{}:{}",
        fake_gh_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    cmd.args([
        "qa-verify",
        "--repo",
        repo,
        "--project-root",
        project_root.to_str().unwrap(),
    ])
    .env_remove("FLOW_CI_RUNNING")
    .env("PATH", path_with_fake)
    .env("HOME", project_root);
    cmd
}

fn parse_last_json(output: &std::process::Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.trim().lines().last().unwrap_or("");
    serde_json::from_str(line).unwrap_or_else(|e| panic!("json parse failed: {} for {:?}", e, line))
}

// --- CLI integration ---

#[test]
fn qa_verify_cli_exits_zero_and_reports_check_failures() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "qa-verify",
            "--repo",
            "owner/nonexistent-qa-verify-test",
            "--project-root",
            root.to_str().unwrap(),
        ])
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("failed to spawn flow-rs");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"status\":\"ok\""));
    assert!(stdout.contains("\"checks\""));
}

#[test]
fn qa_verify_cli_reports_leftover_state_file_failure() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("leftover");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), r#"{"branch":"x"}"#).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "qa-verify",
            "--repo",
            "owner/nonexistent-qa-verify-test",
            "--project-root",
            root.to_str().unwrap(),
        ])
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("failed to spawn flow-rs");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("leftover"));
}

// --- Library-level via run_impl(&Args) with fake gh ---

fn args_for(repo: &str, root: &Path) -> qa_verify::Args {
    qa_verify::Args {
        repo: repo.to_string(),
        project_root: root.to_string_lossy().to_string(),
    }
}

#[test]
fn test_verify_all_pass() {
    // Empty dir + fake gh returning a merged-PR list → all checks pass.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let fake = install_fake_gh(&root, r#"[{"number": 1}]"#, 0);

    let output = run_impl_with_fake_gh(&fake, "owner/repo", &root)
        .output()
        .expect("spawn");
    assert_eq!(output.status.code(), Some(0));
    let data = parse_last_json(&output);
    let checks = data["checks"].as_array().unwrap();
    assert!(checks.iter().all(|c| c["passed"] == true));
}

#[test]
fn test_verify_leftover_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("leftover");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), r#"{"branch":"leftover"}"#).unwrap();
    let fake = install_fake_gh(&root, r#"[{"number": 1}]"#, 0);

    let output = run_impl_with_fake_gh(&fake, "owner/repo", &root)
        .output()
        .expect("spawn");
    let data = parse_last_json(&output);
    let checks = data["checks"].as_array().unwrap();
    let state_check = checks
        .iter()
        .find(|c| c["name"].as_str().unwrap().to_lowercase().contains("state"))
        .expect("state check");
    assert_eq!(state_check["passed"], false);
}

#[test]
fn test_verify_leftover_worktree() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let wt_dir = root.join(".worktrees").join("some-feature");
    fs::create_dir_all(&wt_dir).unwrap();
    let fake = install_fake_gh(&root, r#"[{"number": 1}]"#, 0);

    let output = run_impl_with_fake_gh(&fake, "owner/repo", &root)
        .output()
        .expect("spawn");
    let data = parse_last_json(&output);
    let checks = data["checks"].as_array().unwrap();
    let wt_check = checks
        .iter()
        .find(|c| {
            c["name"]
                .as_str()
                .unwrap()
                .to_lowercase()
                .contains("worktree")
        })
        .expect("worktree check");
    assert_eq!(wt_check["passed"], false);
}

#[test]
fn test_verify_no_merged_pr() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let fake = install_fake_gh(&root, "[]", 0);

    let output = run_impl_with_fake_gh(&fake, "owner/repo", &root)
        .output()
        .expect("spawn");
    let data = parse_last_json(&output);
    let checks = data["checks"].as_array().unwrap();
    let pr_check = checks
        .iter()
        .find(|c| c["name"].as_str().unwrap().contains("PR"))
        .expect("PR check");
    assert_eq!(pr_check["passed"], false);
    assert!(pr_check["detail"]
        .as_str()
        .unwrap()
        .contains("No merged PRs"));
}

#[test]
fn test_verify_pr_fetch_failure() {
    // Fake gh exits non-zero → subprocess_runner returns None.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let fake = install_fake_gh(&root, "error", 1);

    let output = run_impl_with_fake_gh(&fake, "owner/repo", &root)
        .output()
        .expect("spawn");
    let data = parse_last_json(&output);
    let checks = data["checks"].as_array().unwrap();
    let pr_check = checks
        .iter()
        .find(|c| c["name"].as_str().unwrap().contains("PR"))
        .expect("PR check");
    assert_eq!(pr_check["passed"], false);
    assert!(pr_check["detail"]
        .as_str()
        .unwrap()
        .contains("Could not fetch"));
}

#[test]
fn test_verify_no_flow_states_dir() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let fake = install_fake_gh(&root, r#"[{"number": 1}]"#, 0);

    let output = run_impl_with_fake_gh(&fake, "owner/repo", &root)
        .output()
        .expect("spawn");
    let data = parse_last_json(&output);
    let checks = data["checks"].as_array().unwrap();
    let state_check = checks
        .iter()
        .find(|c| c["name"].as_str().unwrap().to_lowercase().contains("state"))
        .expect("state check");
    assert_eq!(state_check["passed"], true);
}

#[test]
fn test_verify_excludes_orchestrate_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let state_dir = root.join(".flow-states");
    fs::create_dir(&state_dir).unwrap();
    fs::write(state_dir.join("orchestrate-queue.json"), "{}").unwrap();
    let fake = install_fake_gh(&root, r#"[{"number": 1}]"#, 0);

    let output = run_impl_with_fake_gh(&fake, "owner/repo", &root)
        .output()
        .expect("spawn");
    let data = parse_last_json(&output);
    let checks = data["checks"].as_array().unwrap();
    let state_check = checks
        .iter()
        .find(|c| c["name"].as_str().unwrap().to_lowercase().contains("state"))
        .expect("state check");
    assert_eq!(state_check["passed"], true);
}

#[test]
fn test_verify_excludes_phases_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let state_dir = root.join(".flow-states");
    fs::create_dir(&state_dir).unwrap();
    fs::write(state_dir.join("feature-phases.json"), "{}").unwrap();
    let fake = install_fake_gh(&root, r#"[{"number": 1}]"#, 0);

    let output = run_impl_with_fake_gh(&fake, "owner/repo", &root)
        .output()
        .expect("spawn");
    let data = parse_last_json(&output);
    let checks = data["checks"].as_array().unwrap();
    let state_check = checks
        .iter()
        .find(|c| c["name"].as_str().unwrap().to_lowercase().contains("state"))
        .expect("state check");
    assert_eq!(state_check["passed"], true);
}

#[test]
fn test_verify_excludes_dot_prefixed_json() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let state_dir = root.join(".flow-states");
    fs::create_dir(&state_dir).unwrap();
    fs::write(state_dir.join(".hidden-state.json"), "{}").unwrap();
    let fake = install_fake_gh(&root, r#"[{"number": 1}]"#, 0);

    let output = run_impl_with_fake_gh(&fake, "owner/repo", &root)
        .output()
        .expect("spawn");
    let data = parse_last_json(&output);
    let checks = data["checks"].as_array().unwrap();
    let state_check = checks
        .iter()
        .find(|c| c["name"].as_str().unwrap().to_lowercase().contains("state"))
        .expect("state check");
    assert_eq!(state_check["passed"], true);
}

/// A dot-prefixed subdirectory under `.flow-states/` (e.g. transient
/// per-machine tooling under `.flow-states/.local/`) is skipped by
/// the scanner so the state-cleanup check still passes.
#[test]
fn test_verify_excludes_dot_prefixed_subdir() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let local_dir = root.join(".flow-states").join(".local");
    fs::create_dir_all(&local_dir).unwrap();
    fs::write(local_dir.join("state.json"), r#"{"branch":".local"}"#).unwrap();
    let fake = install_fake_gh(&root, r#"[{"number": 1}]"#, 0);

    let output = run_impl_with_fake_gh(&fake, "owner/repo", &root)
        .output()
        .expect("spawn");
    let data = parse_last_json(&output);
    let checks = data["checks"].as_array().unwrap();
    let state_check = checks
        .iter()
        .find(|c| c["name"].as_str().unwrap().to_lowercase().contains("state"))
        .expect("state check");
    assert_eq!(state_check["passed"], true);
}

/// A subdirectory under `.flow-states/` without `state.json` (e.g.
/// transient cleanup remnant) is skipped by the scanner so the
/// state-cleanup check still passes.
#[test]
fn test_verify_excludes_subdir_without_state_json() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let empty_dir = root.join(".flow-states").join("empty-branch");
    fs::create_dir_all(&empty_dir).unwrap();
    let fake = install_fake_gh(&root, r#"[{"number": 1}]"#, 0);

    let output = run_impl_with_fake_gh(&fake, "owner/repo", &root)
        .output()
        .expect("spawn");
    let data = parse_last_json(&output);
    let checks = data["checks"].as_array().unwrap();
    let state_check = checks
        .iter()
        .find(|c| c["name"].as_str().unwrap().to_lowercase().contains("state"))
        .expect("state check");
    assert_eq!(state_check["passed"], true);
}

#[test]
fn test_verify_excludes_non_json_files() {
    // .flow-states/ contains a file that does NOT end in .json — the
    // extension filter excludes it, so the state check still passes.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let state_dir = root.join(".flow-states");
    fs::create_dir(&state_dir).unwrap();
    fs::write(state_dir.join("something.txt"), "x").unwrap();
    let fake = install_fake_gh(&root, r#"[{"number": 1}]"#, 0);

    let output = run_impl_with_fake_gh(&fake, "owner/repo", &root)
        .output()
        .expect("spawn");
    let data = parse_last_json(&output);
    let checks = data["checks"].as_array().unwrap();
    let state_check = checks
        .iter()
        .find(|c| c["name"].as_str().unwrap().to_lowercase().contains("state"))
        .expect("state check");
    assert_eq!(state_check["passed"], true);
}

/// Restricted PATH (no `gh` binary anywhere) → Command::new().output()
/// returns Err → subprocess_runner's `.ok()?` None branch fires →
/// verify_impl records "Could not fetch merged PRs".
#[test]
fn subprocess_runner_spawn_failure_reports_fetch_failure() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "qa-verify",
            "--repo",
            "owner/repo",
            "--project-root",
            root.to_str().unwrap(),
        ])
        .env_remove("FLOW_CI_RUNNING")
        .env("PATH", "/nonexistent-no-gh-here")
        .env("HOME", &root)
        .output()
        .expect("spawn");
    assert_eq!(output.status.code(), Some(0));
    let data = parse_last_json(&output);
    let checks = data["checks"].as_array().unwrap();
    let pr_check = checks
        .iter()
        .find(|c| c["name"].as_str().unwrap().contains("PR"))
        .expect("PR check");
    assert_eq!(pr_check["passed"], false);
    assert!(pr_check["detail"]
        .as_str()
        .unwrap()
        .contains("Could not fetch"));
}

// --- Library-level: run_impl signature coverage ---

#[test]
fn run_impl_returns_ok_value() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let result = qa_verify::run_impl(&args_for("owner/nonexistent-lib", &root))
        .expect("run_impl returns Ok");
    assert_eq!(result["status"], "ok");
    assert!(result["checks"].is_array());
}
