//! Tests for `bin/flow capture-diff` — writes the full and substantive
//! diffs of the current worktree relative to `origin/<base>` to canonical
//! files under `.flow-states/<branch>/`. Replaces the inline `git diff`
//! the flow-review skill previously embedded in agent prompts; the
//! file-handoff form keeps the diff out of the parent skill's prompt
//! budget so larger PRs do not exhaust agent context.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_git_repo_with_remote, parse_output};

fn flow_rs_no_recursion() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd
}

/// Run `bin/flow capture-diff` against `repo` with the given args.
///
/// Returns the raw `Output` so callers can assert on exit code and stdout
/// JSON. Sets `current_dir(repo)` so git resolves against the fixture
/// repo, and neutralizes ambient env per
/// `.claude/rules/subprocess-test-hygiene.md`.
fn run_capture_diff(repo: &Path, args: &[&str]) -> Output {
    flow_rs_no_recursion()
        .arg("capture-diff")
        .args(args)
        .current_dir(repo)
        .env("HOME", repo)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

/// Create a fixture worktree with one commit beyond `origin/main` so
/// `git diff origin/main...HEAD` returns a non-empty patch.
fn fixture_with_feature_commit(repo: &Path) {
    fs::write(repo.join("feature.rs"), "// feature\n").unwrap();
    Command::new("git")
        .args(["add", "feature.rs"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "feature commit"])
        .current_dir(repo)
        .output()
        .unwrap();
}

// --- canonical-path writes ---

#[test]
fn capture_diff_writes_full_diff_file_to_canonical_path() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_feature_commit(&repo);

    let output = run_capture_diff(&repo, &["--branch", "feat-test", "--base", "main"]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let full_path = repo.join(".flow-states/feat-test/full-diff.diff");
    assert!(
        full_path.exists(),
        "full-diff.diff missing at {:?}",
        full_path
    );
    let content = fs::read_to_string(&full_path).unwrap();
    assert!(content.contains("feature.rs"));
}

#[test]
fn capture_diff_writes_substantive_diff_file_to_canonical_path() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_feature_commit(&repo);

    let output = run_capture_diff(&repo, &["--branch", "feat-sub", "--base", "main"]);

    assert_eq!(output.status.code(), Some(0));
    let sub_path = repo.join(".flow-states/feat-sub/substantive-diff.diff");
    assert!(
        sub_path.exists(),
        "substantive-diff.diff missing at {:?}",
        sub_path
    );
    let content = fs::read_to_string(&sub_path).unwrap();
    assert!(content.contains("feature.rs"));
}

#[test]
fn capture_diff_creates_branch_subdirectory_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_feature_commit(&repo);

    let branch_dir = repo.join(".flow-states/fresh-branch");
    assert!(!branch_dir.exists(), "precondition: branch dir absent");

    let output = run_capture_diff(&repo, &["--branch", "fresh-branch", "--base", "main"]);

    assert_eq!(output.status.code(), Some(0));
    assert!(
        branch_dir.exists(),
        "capture-diff did not create branch dir"
    );
    assert!(branch_dir.join("full-diff.diff").exists());
    assert!(branch_dir.join("substantive-diff.diff").exists());
}

// --- invalid branch rejection (FlowPaths::try_new) ---

#[test]
fn capture_diff_rejects_slash_branch() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_feature_commit(&repo);

    let output = run_capture_diff(&repo, &["--branch", "feat/slash", "--base", "main"]);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
}

#[test]
fn capture_diff_rejects_empty_branch() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_feature_commit(&repo);

    let output = run_capture_diff(&repo, &["--branch", "", "--base", "main"]);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
}

#[test]
fn capture_diff_rejects_dot_branch() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_feature_commit(&repo);

    let output = run_capture_diff(&repo, &["--branch", "..", "--base", "main"]);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
}

/// `is_safe_base` rejects values that would either interpolate
/// hostile bytes into the diff range or escape the simple-branch
/// expectation. An empty `--base` is the simplest rejection variant
/// — it produces `origin/...HEAD` which has no valid meaning and
/// the gate must short-circuit before the subprocess runs.
#[test]
fn capture_diff_rejects_empty_base() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_feature_commit(&repo);

    let output = run_capture_diff(&repo, &["--branch", "feat-empty-base", "--base", ""]);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap()
        .contains("invalid base ref"));
}

/// `is_safe_base` rejects whitespace in the base value because it
/// would split into multiple shell-style tokens once interpolated
/// into the diff range. A base like `main with spaces` must be
/// rejected by the validator before any subprocess fires.
#[test]
fn capture_diff_rejects_base_with_space() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_feature_commit(&repo);

    let output = run_capture_diff(
        &repo,
        &["--branch", "feat-space-base", "--base", "main staging"],
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap()
        .contains("invalid base ref"));
}

// --- Task 2: success envelope shape ---

#[test]
fn capture_diff_success_envelope_returns_both_paths() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_feature_commit(&repo);

    let output = run_capture_diff(&repo, &["--branch", "envelope-test", "--base", "main"]);

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    let full = data["full"].as_str().expect("full path field");
    let sub = data["substantive"]
        .as_str()
        .expect("substantive path field");
    assert!(full.ends_with(".flow-states/envelope-test/full-diff.diff"));
    assert!(sub.ends_with(".flow-states/envelope-test/substantive-diff.diff"));
}

#[test]
fn capture_diff_returns_error_envelope_when_git_diff_fails() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // No feature commit, AND --base names a ref that does not exist on origin.
    // git diff origin/nonexistent...HEAD will fail with "unknown revision".
    let output = run_capture_diff(
        &repo,
        &["--branch", "git-error", "--base", "nonexistent-base"],
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"].is_string());
}

#[test]
fn capture_diff_exit_code_zero_on_success() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_feature_commit(&repo);

    let output = run_capture_diff(&repo, &["--branch", "exit-ok", "--base", "main"]);

    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn capture_diff_exit_code_zero_on_business_error() {
    // Business errors (invalid branch, git diff failure) return JSON
    // with status:error AND exit code 0 per the FLOW convention.
    // Exit code 1 is reserved for infrastructure failures.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_feature_commit(&repo);

    let output = run_capture_diff(&repo, &["--branch", "feat/slash", "--base", "main"]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "business errors must use status:error + exit 0"
    );
}

// --- error paths: ensure_branch_dir, fs::write, git spawn ---

#[test]
fn capture_diff_returns_error_when_branch_dir_blocked_by_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_feature_commit(&repo);

    // Place a regular file at .flow-states so create_dir_all fails
    // when ensure_branch_dir tries to create the .flow-states subtree.
    fs::write(repo.join(".flow-states"), "blocking file").unwrap();

    let output = run_capture_diff(&repo, &["--branch", "blocked", "--base", "main"]);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap()
        .contains("create branch dir"));
}

#[test]
fn capture_diff_returns_error_when_full_diff_write_path_is_directory() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_feature_commit(&repo);

    // Pre-create full-diff.diff as a directory so fs::write fails AFTER
    // ensure_branch_dir succeeds — exercises the post-mkdir write Err
    // arm for the full diff.
    fs::create_dir_all(repo.join(".flow-states/full-write-fail/full-diff.diff")).unwrap();

    let output = run_capture_diff(&repo, &["--branch", "full-write-fail", "--base", "main"]);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap()
        .contains("write full-diff"));
}

#[test]
fn capture_diff_returns_error_when_substantive_diff_write_path_is_directory() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_feature_commit(&repo);

    // Pre-create substantive-diff.diff as a directory so fs::write
    // fails AFTER the full diff write succeeds — exercises the
    // post-full-write Err arm for the substantive diff.
    fs::create_dir_all(repo.join(".flow-states/sub-write-fail/substantive-diff.diff")).unwrap();

    let output = run_capture_diff(&repo, &["--branch", "sub-write-fail", "--base", "main"]);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap()
        .contains("write substantive-diff"));
}

#[test]
fn capture_diff_returns_spawn_error_when_git_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_feature_commit(&repo);

    // Empty PATH so the git binary cannot be located. The first
    // git_diff call's spawn step fails, producing the
    // "spawn git: <io error>" message.
    let output = flow_rs_no_recursion()
        .args(["capture-diff", "--branch", "no-git", "--base", "main"])
        .current_dir(&repo)
        .env("PATH", "")
        .env("HOME", &repo)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("spawn"));
}
