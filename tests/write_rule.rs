//! Integration tests for `bin/flow write-rule`.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::{create_git_repo_with_remote, parse_output};
use flow_rs::write_rule::{
    canonical_path, classify_path, read_content_file, write_rule, ManagedArtifact,
};

fn run_write_rule(repo: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("write-rule")
        .args(args)
        .current_dir(repo)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

#[test]
fn write_rule_writes_content_and_deletes_source() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let content_file = dir.path().join("content.md");
    fs::write(&content_file, "# Rule Body\n\nContent here.\n").unwrap();
    let target = dir.path().join(".claude").join("rules").join("test.md");

    let output = run_write_rule(
        &repo,
        &[
            "--path",
            target.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["path"], target.to_string_lossy().as_ref());

    // Content written
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "# Rule Body\n\nContent here.\n"
    );
    // Source file deleted
    assert!(!content_file.exists());
}

#[test]
fn write_rule_missing_content_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let missing = dir.path().join("no-such.md");
    let target = dir.path().join("target.md");

    let output = run_write_rule(
        &repo,
        &[
            "--path",
            target.to_str().unwrap(),
            "--content-file",
            missing.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Could not read content file"));
}

#[test]
fn write_rule_overwrites_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let content_file = dir.path().join("new.md");
    fs::write(&content_file, "new body").unwrap();
    let target = dir.path().join("existing.md");
    fs::write(&target, "old body").unwrap();

    let output = run_write_rule(
        &repo,
        &[
            "--path",
            target.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read_to_string(&target).unwrap(), "new body");
}

#[test]
fn write_rule_creates_nested_parent_directories() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let content_file = dir.path().join("c.md");
    fs::write(&content_file, "nested").unwrap();
    let target = dir.path().join("a").join("b").join("c").join("rule.md");

    let output = run_write_rule(
        &repo,
        &[
            "--path",
            target.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(target.exists());
    assert_eq!(fs::read_to_string(&target).unwrap(), "nested");
}

#[test]
fn write_rule_target_parent_blocked_by_file_errors() {
    // Drives the write_rule Err arm of run(): create_dir_all fails when
    // a regular file occupies the parent path that needs to be a dir.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let content_file = dir.path().join("c.md");
    fs::write(&content_file, "body").unwrap();
    let blocker = dir.path().join("blocker");
    fs::write(&blocker, "I am a file, not a directory").unwrap();
    let target = blocker.join("nested").join("rule.md");

    let output = run_write_rule(
        &repo,
        &[
            "--path",
            target.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Could not create directories"));
}

// --- Library-level tests (migrated from src/write_rule.rs) ---

// --- read_content_file ---

#[test]
fn read_content_file_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    let content_file = dir.path().join("content.md");
    fs::write(&content_file, "# My Rule\n\nDo the thing.\n").unwrap();

    let content = read_content_file(content_file.to_str().unwrap()).unwrap();
    assert_eq!(content, "# My Rule\n\nDo the thing.\n");
    assert!(!content_file.exists());
}

#[test]
fn read_content_file_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nonexistent.md");

    let result = read_content_file(missing.to_str().unwrap());
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Could not read content file"));
}

// --- write_rule ---

#[test]
fn write_rule_happy_path_lib() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("rules").join("topic.md");
    fs::create_dir_all(target.parent().unwrap()).unwrap();

    let result = write_rule(target.to_str().unwrap(), "# Topic\n\nRule text.\n");
    assert!(result.is_ok());
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "# Topic\n\nRule text.\n"
    );
}

#[test]
fn write_rule_creates_parent_dirs_lib() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir
        .path()
        .join("deep")
        .join("nested")
        .join("dir")
        .join("rule.md");

    let result = write_rule(target.to_str().unwrap(), "content");
    assert!(result.is_ok());
    assert_eq!(fs::read_to_string(&target).unwrap(), "content");
}

#[test]
fn write_rule_overwrites_existing_lib() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("rule.md");
    fs::write(&target, "old content").unwrap();

    let result = write_rule(target.to_str().unwrap(), "new content");
    assert!(result.is_ok());
    assert_eq!(fs::read_to_string(&target).unwrap(), "new content");
}

#[test]
fn write_rule_write_error_lib() {
    let dir = tempfile::tempdir().unwrap();
    let readonly = dir.path().join("readonly");
    fs::create_dir_all(&readonly).unwrap();

    // Make the directory read-only
    let mut perms = fs::metadata(&readonly).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&readonly, perms).unwrap();

    let target = readonly.join("rule.md");
    let result = write_rule(target.to_str().unwrap(), "content");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Could not write"));

    // Restore permissions for cleanup
    let mut perms = fs::metadata(&readonly).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(&readonly, perms).unwrap();
}

#[test]
fn write_rule_create_dir_error_lib() {
    let dir = tempfile::tempdir().unwrap();
    // Place a regular file where the parent directory needs to be.
    let blocker = dir.path().join("blocker");
    fs::write(&blocker, "I am a file").unwrap();

    let target = blocker.join("nested").join("rule.md");
    let result = write_rule(target.to_str().unwrap(), "content");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Could not create directories"));
}

#[test]
fn write_rule_empty_path_errors_lib() {
    // Empty string path: parent() returns None so create_dir_all is
    // skipped, and fs::write on an empty path returns an OS error.
    let result = write_rule("", "content");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Could not write"));
}

// --- classify_path ---

#[test]
fn classify_path_matches_plan_md_basename() {
    let p = Path::new("/some/where/.flow-states/feat/plan.md");
    assert_eq!(classify_path(p), Some(ManagedArtifact::PlanMd));
}

#[test]
fn classify_path_matches_dag_md_basename() {
    let p = Path::new("/some/where/.flow-states/feat/dag.md");
    assert_eq!(classify_path(p), Some(ManagedArtifact::DagMd));
}

#[test]
fn classify_path_matches_commit_msg_txt_basename() {
    let p = Path::new("/some/where/.flow-states/feat/commit-msg.txt");
    assert_eq!(classify_path(p), Some(ManagedArtifact::CommitMsgTxt));
}

#[test]
fn classify_path_matches_flow_issue_body_basename() {
    let p = Path::new("/some/where/.flow-issue-body");
    assert_eq!(classify_path(p), Some(ManagedArtifact::FlowIssueBody));
}

#[test]
fn classify_path_matches_orchestrate_queue_json_basename() {
    let p = Path::new("/some/where/.flow-states/orchestrate-queue.json");
    assert_eq!(classify_path(p), Some(ManagedArtifact::OrchestrateQueue));
}

#[test]
fn classify_path_returns_none_for_non_managed_basename() {
    assert_eq!(
        classify_path(Path::new("/some/.claude/rules/rule.md")),
        None
    );
    assert_eq!(classify_path(Path::new("/some/CLAUDE.md")), None);
    assert_eq!(classify_path(Path::new("/some/foo.txt")), None);
}

#[test]
fn classify_path_returns_none_when_path_has_no_file_name() {
    // `Path::file_name()` returns None for paths that end with `..`
    // or that are pure roots like `/`. The `?` propagation in
    // classify_path must short-circuit these to None, not panic.
    assert_eq!(classify_path(Path::new("/")), None);
    assert_eq!(classify_path(Path::new("..")), None);
}

#[test]
fn classify_path_returns_none_for_non_utf8_basename() {
    // `path.file_name().to_str()` returns None when the basename
    // contains non-UTF-8 bytes. Construct one via OsStrExt to drive
    // the second `?` branch in classify_path.
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let bytes = b"\xff\xfe.md";
    let osstr = OsStr::from_bytes(bytes);
    assert_eq!(classify_path(Path::new(osstr)), None);
}

// --- canonical_path ---

#[test]
fn canonical_path_branch_scoped_returns_main_repo_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let branch = Some("feat-x");

    assert_eq!(
        canonical_path(ManagedArtifact::PlanMd, root, branch),
        Some(root.join(".flow-states").join("feat-x").join("plan.md"))
    );
    assert_eq!(
        canonical_path(ManagedArtifact::DagMd, root, branch),
        Some(root.join(".flow-states").join("feat-x").join("dag.md"))
    );
    assert_eq!(
        canonical_path(ManagedArtifact::CommitMsgTxt, root, branch),
        Some(
            root.join(".flow-states")
                .join("feat-x")
                .join("commit-msg.txt")
        )
    );
}

#[test]
fn canonical_path_project_root_returns_main_repo_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // FlowIssueBody is project-root-scoped; branch availability does not matter.
    assert_eq!(
        canonical_path(ManagedArtifact::FlowIssueBody, root, None),
        Some(root.join(".flow-issue-body"))
    );
    assert_eq!(
        canonical_path(ManagedArtifact::FlowIssueBody, root, Some("feat-x")),
        Some(root.join(".flow-issue-body"))
    );
}

#[test]
fn canonical_path_machine_level_returns_main_repo_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // OrchestrateQueue is a machine-level singleton at .flow-states/orchestrate-queue.json
    // — not branch-scoped. Branch availability does not matter.
    assert_eq!(
        canonical_path(ManagedArtifact::OrchestrateQueue, root, None),
        Some(root.join(".flow-states").join("orchestrate-queue.json"))
    );
    assert_eq!(
        canonical_path(ManagedArtifact::OrchestrateQueue, root, Some("feat-x")),
        Some(root.join(".flow-states").join("orchestrate-queue.json"))
    );
}

#[test]
fn canonical_path_returns_none_when_branch_unavailable_for_branch_scoped() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // Branch-scoped variants return None when branch is unavailable
    // (detached HEAD, or invalid branch like one containing '/').
    assert_eq!(canonical_path(ManagedArtifact::PlanMd, root, None), None);
    assert_eq!(canonical_path(ManagedArtifact::DagMd, root, None), None);
    assert_eq!(
        canonical_path(ManagedArtifact::CommitMsgTxt, root, None),
        None
    );
    // Invalid branch (slash) is also None — FlowPaths::try_new rejects it.
    assert_eq!(
        canonical_path(ManagedArtifact::PlanMd, root, Some("feature/foo")),
        None
    );
    // Project-root and machine-level variants still return Some when branch is None.
    assert!(canonical_path(ManagedArtifact::FlowIssueBody, root, None).is_some());
    assert!(canonical_path(ManagedArtifact::OrchestrateQueue, root, None).is_some());
}

// --- subprocess canonicalization matrix ---

/// Setup helper: create a git repo on a feature branch, return its path.
/// The returned path is canonicalized (macOS /var → /private/var stable).
/// Subprocess hygiene: `run_wr_canon` neutralizes FLOW_CI_RUNNING and HOME.
fn setup_branch_repo(parent: &Path, branch: &str) -> PathBuf {
    let canonical_parent = parent.canonicalize().expect("canonicalize tempdir");
    let repo = create_git_repo_with_remote(&canonical_parent);
    let repo = repo.canonicalize().expect("canonicalize repo");
    Command::new("git")
        .args(["checkout", "-b", branch])
        .current_dir(&repo)
        .output()
        .unwrap();
    repo
}

/// Spawn `bin/flow write-rule` with subprocess hygiene per
/// `.claude/rules/subprocess-test-hygiene.md`. `cwd` is the working
/// directory write-rule runs from; the binary detects project_root and
/// current_branch from there.
fn run_wr_canon(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("write-rule")
        .args(args)
        .current_dir(cwd)
        .env_remove("FLOW_CI_RUNNING")
        .env("HOME", cwd)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

#[test]
fn write_rule_subprocess_canonical_path_succeeds_plan_md() {
    let dir = tempfile::tempdir().unwrap();
    let repo = setup_branch_repo(dir.path(), "feat-x");
    let content_file = repo.join("content.md");
    fs::write(&content_file, "plan body").unwrap();
    let canonical = repo.join(".flow-states").join("feat-x").join("plan.md");

    let output = run_wr_canon(
        &repo,
        &[
            "--path",
            canonical.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(fs::read_to_string(&canonical).unwrap(), "plan body");
}

#[test]
fn write_rule_subprocess_worktree_root_path_rejects_plan_md() {
    let dir = tempfile::tempdir().unwrap();
    let repo = setup_branch_repo(dir.path(), "feat-x");
    let content_file = repo.join("content.md");
    fs::write(&content_file, "plan body").unwrap();
    // Simulate the bug: model substituted <project_root> to a worktree
    // root, so --path lands at <main_repo>/.worktrees/feat-x/.flow-states/feat-x/plan.md.
    let wrong = repo
        .join(".worktrees")
        .join("feat-x")
        .join(".flow-states")
        .join("feat-x")
        .join("plan.md");

    let output = run_wr_canon(
        &repo,
        &[
            "--path",
            wrong.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["step"], "path_canonicalization");
    assert_eq!(data["artifact_kind"], "PlanMd");
    assert!(!wrong.exists(), "rejected path must not be written");
}

#[test]
fn write_rule_subprocess_service_subdir_path_rejects_plan_md() {
    let dir = tempfile::tempdir().unwrap();
    let repo = setup_branch_repo(dir.path(), "feat-x");
    let content_file = repo.join("content.md");
    fs::write(&content_file, "plan body").unwrap();
    // Simulate the mono-repo bug: model substituted <project_root> to a
    // service subdirectory, so --path lands at <main_repo>/api/.flow-states/feat-x/plan.md.
    let wrong = repo
        .join("api")
        .join(".flow-states")
        .join("feat-x")
        .join("plan.md");

    let output = run_wr_canon(
        &repo,
        &[
            "--path",
            wrong.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["step"], "path_canonicalization");
    assert!(!wrong.exists(), "rejected path must not be written");
}

#[test]
fn write_rule_subprocess_canonical_path_succeeds_dag_md() {
    let dir = tempfile::tempdir().unwrap();
    let repo = setup_branch_repo(dir.path(), "feat-x");
    let content_file = repo.join("content.md");
    fs::write(&content_file, "dag body").unwrap();
    let canonical = repo.join(".flow-states").join("feat-x").join("dag.md");

    let output = run_wr_canon(
        &repo,
        &[
            "--path",
            canonical.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read_to_string(&canonical).unwrap(), "dag body");
}

#[test]
fn write_rule_subprocess_worktree_root_path_rejects_dag_md() {
    let dir = tempfile::tempdir().unwrap();
    let repo = setup_branch_repo(dir.path(), "feat-x");
    let content_file = repo.join("content.md");
    fs::write(&content_file, "dag body").unwrap();
    let wrong = repo
        .join(".worktrees")
        .join("feat-x")
        .join(".flow-states")
        .join("feat-x")
        .join("dag.md");

    let output = run_wr_canon(
        &repo,
        &[
            "--path",
            wrong.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["artifact_kind"], "DagMd");
}

#[test]
fn write_rule_subprocess_canonical_path_succeeds_commit_msg() {
    let dir = tempfile::tempdir().unwrap();
    let repo = setup_branch_repo(dir.path(), "feat-x");
    let content_file = repo.join("content.txt");
    fs::write(&content_file, "commit message").unwrap();
    let canonical = repo
        .join(".flow-states")
        .join("feat-x")
        .join("commit-msg.txt");

    let output = run_wr_canon(
        &repo,
        &[
            "--path",
            canonical.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read_to_string(&canonical).unwrap(), "commit message");
}

#[test]
fn write_rule_subprocess_worktree_root_path_rejects_commit_msg() {
    let dir = tempfile::tempdir().unwrap();
    let repo = setup_branch_repo(dir.path(), "feat-x");
    let content_file = repo.join("content.txt");
    fs::write(&content_file, "commit message").unwrap();
    let wrong = repo
        .join(".worktrees")
        .join("feat-x")
        .join(".flow-states")
        .join("feat-x")
        .join("commit-msg.txt");

    let output = run_wr_canon(
        &repo,
        &[
            "--path",
            wrong.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["artifact_kind"], "CommitMsgTxt");
}

#[test]
fn write_rule_subprocess_canonical_path_succeeds_flow_issue_body() {
    let dir = tempfile::tempdir().unwrap();
    let repo = setup_branch_repo(dir.path(), "feat-x");
    let content_file = repo.join("content.md");
    fs::write(&content_file, "issue body").unwrap();
    let canonical = repo.join(".flow-issue-body");

    let output = run_wr_canon(
        &repo,
        &[
            "--path",
            canonical.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read_to_string(&canonical).unwrap(), "issue body");
}

#[test]
fn write_rule_subprocess_worktree_path_rejects_flow_issue_body() {
    let dir = tempfile::tempdir().unwrap();
    let repo = setup_branch_repo(dir.path(), "feat-x");
    let content_file = repo.join("content.md");
    fs::write(&content_file, "issue body").unwrap();
    // Wrong: under a worktree subdirectory, not at project root.
    let wrong = repo
        .join(".worktrees")
        .join("feat-x")
        .join(".flow-issue-body");

    let output = run_wr_canon(
        &repo,
        &[
            "--path",
            wrong.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["artifact_kind"], "FlowIssueBody");
}

#[test]
fn write_rule_subprocess_canonical_path_succeeds_orchestrate_queue() {
    let dir = tempfile::tempdir().unwrap();
    // Orchestrate queue is machine-level — no branch needed.
    let canonical_parent = dir.path().canonicalize().unwrap();
    let repo = create_git_repo_with_remote(&canonical_parent);
    let repo = repo.canonicalize().unwrap();
    let content_file = repo.join("content.json");
    fs::write(&content_file, "{}").unwrap();
    let canonical = repo.join(".flow-states").join("orchestrate-queue.json");

    let output = run_wr_canon(
        &repo,
        &[
            "--path",
            canonical.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read_to_string(&canonical).unwrap(), "{}");
}

#[test]
fn write_rule_subprocess_worktree_path_rejects_orchestrate_queue() {
    let dir = tempfile::tempdir().unwrap();
    let repo = setup_branch_repo(dir.path(), "feat-x");
    let content_file = repo.join("content.json");
    fs::write(&content_file, "{}").unwrap();
    // Wrong: under a worktree subdirectory, not at project root's .flow-states.
    let wrong = repo
        .join(".worktrees")
        .join("feat-x")
        .join(".flow-states")
        .join("orchestrate-queue.json");

    let output = run_wr_canon(
        &repo,
        &[
            "--path",
            wrong.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["artifact_kind"], "OrchestrateQueue");
}

#[test]
fn write_rule_subprocess_non_managed_path_passes_through() {
    let dir = tempfile::tempdir().unwrap();
    let repo = setup_branch_repo(dir.path(), "feat-x");
    let content_file = repo.join("content.md");
    fs::write(&content_file, "rule body").unwrap();
    // Non-managed basenames pass the gate unchanged.
    let target = repo.join(".claude").join("rules").join("topic.md");

    let output = run_wr_canon(
        &repo,
        &[
            "--path",
            target.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read_to_string(&target).unwrap(), "rule body");

    // Same for CLAUDE.md.
    let content_file2 = repo.join("content2.md");
    fs::write(&content_file2, "claude body").unwrap();
    let target2 = repo.join("CLAUDE.md");
    let output2 = run_wr_canon(
        &repo,
        &[
            "--path",
            target2.to_str().unwrap(),
            "--content-file",
            content_file2.to_str().unwrap(),
        ],
    );
    assert_eq!(output2.status.code(), Some(0));
    assert_eq!(fs::read_to_string(&target2).unwrap(), "claude body");
}

#[test]
fn write_rule_subprocess_relative_canonical_path_succeeds() {
    // Drives the relative-path branch (root.join(provided)) of the
    // gate — a relative --path resolves against project_root.
    let dir = tempfile::tempdir().unwrap();
    let repo = setup_branch_repo(dir.path(), "feat-x");
    let content_file = repo.join("content.md");
    fs::write(&content_file, "plan body").unwrap();
    // Relative --path: project_root() must resolve it to the canonical
    // destination for the gate to accept.
    let rel = ".flow-states/feat-x/plan.md";

    let output = run_wr_canon(
        &repo,
        &[
            "--path",
            rel,
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(repo.join(".flow-states/feat-x/plan.md")).unwrap(),
        "plan body"
    );
}

#[test]
fn write_rule_subprocess_path_with_parent_dir_normalizes_to_canonical() {
    // Drives the Component::ParentDir branch of normalize_lexical.
    // A `..` traversal in the middle of an otherwise-canonical path
    // must normalize to the canonical destination, not be rejected.
    let dir = tempfile::tempdir().unwrap();
    let repo = setup_branch_repo(dir.path(), "feat-x");
    let content_file = repo.join("content.md");
    fs::write(&content_file, "plan body").unwrap();
    // <repo>/.flow-states/other/../feat-x/plan.md normalizes to
    // <repo>/.flow-states/feat-x/plan.md (the canonical destination).
    let provided = repo
        .join(".flow-states")
        .join("other")
        .join("..")
        .join("feat-x")
        .join("plan.md");

    let output = run_wr_canon(
        &repo,
        &[
            "--path",
            provided.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(repo.join(".flow-states/feat-x/plan.md")).unwrap(),
        "plan body"
    );
}

#[test]
fn write_rule_subprocess_rejection_preserves_content_file() {
    // Gate must run BEFORE read_content_file so a rejection does not
    // destroy the caller's input. Regression: prior gate ordering ran
    // read_content_file first; on reject the user lost their content.
    let dir = tempfile::tempdir().unwrap();
    let repo = setup_branch_repo(dir.path(), "feat-x");
    let content_file = repo.join("preserve-me.md");
    fs::write(&content_file, "important content").unwrap();
    let wrong = repo
        .join(".worktrees")
        .join("feat-x")
        .join(".flow-states")
        .join("feat-x")
        .join("plan.md");

    let output = run_wr_canon(
        &repo,
        &[
            "--path",
            wrong.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["step"], "path_canonicalization");
    // The load-bearing assertion: content file survives the rejection.
    assert!(
        content_file.exists(),
        "content file must survive a gate rejection so the caller can retry"
    );
    assert_eq!(
        fs::read_to_string(&content_file).unwrap(),
        "important content"
    );
}

#[test]
fn write_rule_subprocess_subdir_cwd_relative_path_lands_at_canonical() {
    // Gate-vs-write divergence regression: the gate joins the relative
    // --path against project_root, but the actual fs::write must use
    // the same resolved absolute path — otherwise the write resolves
    // the relative string against the process cwd instead, landing at
    // a misplaced state copy. Drive the bug from a mono-repo
    // subdirectory cwd.
    let dir = tempfile::tempdir().unwrap();
    let repo = setup_branch_repo(dir.path(), "feat-x");
    let subdir = repo.join("api");
    fs::create_dir_all(&subdir).unwrap();
    let content_file = subdir.join("content.md");
    fs::write(&content_file, "plan body").unwrap();

    let output = run_wr_canon(
        &subdir,
        &[
            "--path",
            ".flow-states/feat-x/plan.md",
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    let canonical = repo.join(".flow-states/feat-x/plan.md");
    let misplaced = subdir.join(".flow-states/feat-x/plan.md");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        canonical.exists(),
        "write must land at gate-validated canonical path: {}",
        canonical.display()
    );
    assert!(
        !misplaced.exists(),
        "write must NOT land at the subdir-local resolution: {}",
        misplaced.display()
    );
    assert_eq!(fs::read_to_string(&canonical).unwrap(), "plan body");
}

#[test]
fn write_rule_subprocess_detached_head_no_op_for_branch_scoped() {
    let dir = tempfile::tempdir().unwrap();
    let canonical_parent = dir.path().canonicalize().unwrap();
    let repo = create_git_repo_with_remote(&canonical_parent);
    let repo = repo.canonicalize().unwrap();
    // Detach HEAD: checkout the commit by SHA.
    let sha_out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&repo)
        .output()
        .unwrap();
    let sha = String::from_utf8(sha_out.stdout)
        .unwrap()
        .trim()
        .to_string();
    Command::new("git")
        .args(["checkout", &sha])
        .current_dir(&repo)
        .output()
        .unwrap();

    let content_file = repo.join("content.md");
    fs::write(&content_file, "plan body").unwrap();
    // Branch is None in detached HEAD. Branch-scoped canonical_path
    // returns None → gate is no-op pass-through. Even an unusual --path
    // is accepted; we exercise the canonical path here.
    let target = repo
        .join(".flow-states")
        .join("placeholder")
        .join("plan.md");

    let output = run_wr_canon(
        &repo,
        &[
            "--path",
            target.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "detached HEAD must not block branch-scoped writes; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "plan body");
}

// --- worktree-path guard for protected basenames ---

/// Setup helper: create the active-flow fixture for the worktree-path
/// guard tests. Mirrors `seed_active_flow_fixture` from
/// tests/hooks/validate_claude_paths.rs — a `.git` marker file at the
/// worktree level lets `detect_branch_from_path` resolve the branch
/// without invoking git, so no real git worktree is required. Returns
/// `(main_root, worktree_path)` both canonicalized for stable
/// `starts_with` comparisons on macOS (/var → /private/var).
fn seed_active_flow(parent: &Path, branch: &str) -> (PathBuf, PathBuf) {
    let main_root = parent.canonicalize().expect("canonicalize tempdir");
    let branch_dir = main_root.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), "{}").unwrap();
    let worktree = main_root.join(".worktrees").join(branch);
    fs::create_dir_all(&worktree).unwrap();
    fs::write(worktree.join(".git"), "gitdir: fake\n").unwrap();
    (main_root, worktree)
}

#[test]
fn write_rule_subprocess_protected_main_repo_path_rejects() {
    // Protected basename + main-repo destination + active flow → reject
    // with step:worktree_path_validation. This is the load-bearing
    // regression: without the guard, a model can call write-rule with a
    // main-repo path during a flow and the subprocess writes to main.
    let dir = tempfile::tempdir().unwrap();
    let (main_root, worktree) = seed_active_flow(dir.path(), "feat-x");
    let content_file = worktree.join("content.md");
    fs::write(&content_file, "rule body").unwrap();
    // Main-repo destination — outside the worktree.
    let main_target = main_root.join(".claude").join("rules").join("foo.md");

    let output = run_wr_canon(
        &worktree,
        &[
            "--path",
            main_target.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["step"], "worktree_path_validation");
    assert!(
        !main_target.exists(),
        "rejected path must not be written to main repo"
    );
    // Content file preserved across rejection (gate-before-read invariant).
    assert!(content_file.exists(), "content file must survive rejection");
}

#[test]
fn write_rule_subprocess_protected_worktree_path_passes() {
    // Protected basename + worktree destination + active flow → pass.
    // The guard fires only when the path lands outside the worktree.
    let dir = tempfile::tempdir().unwrap();
    let (_main_root, worktree) = seed_active_flow(dir.path(), "feat-x");
    let content_file = worktree.join("content.md");
    fs::write(&content_file, "rule body").unwrap();
    let worktree_target = worktree.join(".claude").join("rules").join("foo.md");

    let output = run_wr_canon(
        &worktree,
        &[
            "--path",
            worktree_target.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(&worktree_target).unwrap(), "rule body");
}

#[test]
fn write_rule_subprocess_protected_no_active_flow_passes() {
    // Protected basename + no active flow (no state.json) → pass.
    // The guard is conditional on flow-active state — outside a flow,
    // `bin/flow write-rule` keeps its current pass-through behavior so
    // prime-time and one-off invocations are not blocked.
    let dir = tempfile::tempdir().unwrap();
    // No seed_active_flow — just create a worktree-shaped dir without
    // .flow-states/<branch>/state.json so is_flow_active returns false.
    let main_root = dir.path().canonicalize().unwrap();
    let worktree = main_root.join(".worktrees").join("feat-x");
    fs::create_dir_all(&worktree).unwrap();
    fs::write(worktree.join(".git"), "gitdir: fake\n").unwrap();
    let content_file = worktree.join("content.md");
    fs::write(&content_file, "rule body").unwrap();
    let main_target = main_root.join(".claude").join("rules").join("foo.md");

    let output = run_wr_canon(
        &worktree,
        &[
            "--path",
            main_target.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(&main_target).unwrap(), "rule body");
}

#[test]
fn write_rule_subprocess_unprotected_main_repo_path_passes_in_flow() {
    // Non-protected path + main-repo destination + active flow → pass.
    // The guard's purpose is to protect CLAUDE.md / .claude/rules /
    // .claude/skills only — other paths under main repo (e.g.
    // .flow-issue-body, .flow-states/*) are governed by the existing
    // managed-artifact canonicalization gate and unrelated paths pass
    // through unchanged.
    let dir = tempfile::tempdir().unwrap();
    let (main_root, worktree) = seed_active_flow(dir.path(), "feat-x");
    let content_file = worktree.join("content.txt");
    fs::write(&content_file, "unrelated body").unwrap();
    let main_target = main_root.join("docs").join("notes.txt");

    let output = run_wr_canon(
        &worktree,
        &[
            "--path",
            main_target.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(&main_target).unwrap(), "unrelated body");
}

#[test]
fn write_rule_subprocess_protected_claude_md_main_repo_rejects() {
    // CLAUDE.md is also a protected basename (alongside .claude/rules and
    // .claude/skills). The guard must fire on a main-repo CLAUDE.md
    // target as well — not only on the .claude/* directory cases above.
    let dir = tempfile::tempdir().unwrap();
    let (main_root, worktree) = seed_active_flow(dir.path(), "feat-x");
    let content_file = worktree.join("content.md");
    fs::write(&content_file, "claude body").unwrap();
    let main_target = main_root.join("CLAUDE.md");

    let output = run_wr_canon(
        &worktree,
        &[
            "--path",
            main_target.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["step"], "worktree_path_validation");
    assert!(!main_target.exists());
}

#[test]
fn write_rule_subprocess_protected_skills_main_repo_rejects() {
    // .claude/skills/ is protected just like .claude/rules/.
    let dir = tempfile::tempdir().unwrap();
    let (main_root, worktree) = seed_active_flow(dir.path(), "feat-x");
    let content_file = worktree.join("content.md");
    fs::write(&content_file, "skill body").unwrap();
    let main_target = main_root
        .join(".claude")
        .join("skills")
        .join("my-skill")
        .join("SKILL.md");

    let output = run_wr_canon(
        &worktree,
        &[
            "--path",
            main_target.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["step"], "worktree_path_validation");
}

#[test]
fn write_rule_subprocess_protected_relative_path_passes_inside_worktree() {
    // Drives the `cwd.join(provided)` branch of worktree_path_guard:
    // a relative protected `--path` resolves against cwd (the
    // worktree), which lands inside the worktree → pass. Without this
    // test the relative-resolution branch is uncovered.
    let dir = tempfile::tempdir().unwrap();
    let (_main_root, worktree) = seed_active_flow(dir.path(), "feat-x");
    let content_file = worktree.join("content.md");
    fs::write(&content_file, "rule body").unwrap();

    let output = run_wr_canon(
        &worktree,
        &[
            "--path",
            ".claude/rules/foo.md",
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(worktree.join(".claude/rules/foo.md")).unwrap(),
        "rule body"
    );
}

#[test]
fn write_rule_subprocess_protected_inactive_flow_for_branch_passes() {
    // Drives the `is_flow_active = false` branch of worktree_path_guard:
    // `.flow-states/` exists at main_root (so find_main_root succeeds)
    // but the BRANCH-specific state.json is missing, so the gate
    // returns None and the write proceeds. Without this test the
    // is_flow_active=false branch is uncovered.
    let dir = tempfile::tempdir().unwrap();
    let main_root = dir.path().canonicalize().unwrap();
    // Create .flow-states/ at main_root for an UNRELATED branch so
    // find_main_root_from succeeds but is_flow_active("feat-x", ...)
    // returns false.
    fs::create_dir_all(main_root.join(".flow-states").join("other-branch")).unwrap();
    fs::write(
        main_root
            .join(".flow-states")
            .join("other-branch")
            .join("state.json"),
        "{}",
    )
    .unwrap();
    let worktree = main_root.join(".worktrees").join("feat-x");
    fs::create_dir_all(&worktree).unwrap();
    fs::write(worktree.join(".git"), "gitdir: fake\n").unwrap();
    let content_file = worktree.join("content.md");
    fs::write(&content_file, "rule body").unwrap();
    // Target main-repo destination — without an active flow on this
    // branch, the gate must NOT block.
    let main_target = main_root.join(".claude").join("rules").join("foo.md");

    let output = run_wr_canon(
        &worktree,
        &[
            "--path",
            main_target.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(&main_target).unwrap(), "rule body");
}

#[test]
fn write_rule_subprocess_protected_branch_undetectable_passes() {
    // Drives the `detect_branch_from_path = None` branch of
    // worktree_path_guard: cwd is at main_root (no .worktrees/ marker)
    // and there is no git repo here, so detect_branch_from_path falls
    // through to the git subprocess fallback which fails. The gate
    // returns None — no branch means no flow correlation — and the
    // write proceeds.
    let dir = tempfile::tempdir().unwrap();
    let main_root = dir.path().canonicalize().unwrap();
    // .flow-states/ exists so find_main_root_from succeeds at main_root.
    fs::create_dir_all(main_root.join(".flow-states")).unwrap();
    let content_file = main_root.join("content.md");
    fs::write(&content_file, "rule body").unwrap();
    // Protected basename so worktree_path_guard's first short-circuit
    // (is_protected_path=false) does NOT fire.
    let target = main_root.join(".claude").join("rules").join("foo.md");

    let output = run_wr_canon(
        &main_root,
        &[
            "--path",
            target.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "rule body");
}

#[test]
fn write_rule_subprocess_submodule_subdirectory_does_not_bypass_gate() {
    // Adversarial regression: a git submodule (or any subdirectory
    // carrying its own `.git` file) inside a worktree previously
    // tricked `detect_branch_from_path` into returning `<branch>/<sub>`
    // — a slash-containing branch that `is_flow_active` rejected,
    // silently disabling the gate. The fix in `worktree_branch_from_path`
    // bypasses the `.git` walk-up and extracts the first `.worktrees/<X>/`
    // segment, restoring the active-flow correlation.
    let dir = tempfile::tempdir().unwrap();
    let (main_root, worktree) = seed_active_flow(dir.path(), "feat-x");
    let sub = worktree.join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join(".git"), "gitdir: submodule\n").unwrap();
    let content_file = sub.join("content.md");
    fs::write(&content_file, "exfiltrated").unwrap();
    let main_target = main_root.join("CLAUDE.md");

    let output = run_wr_canon(
        &sub,
        &[
            "--path",
            main_target.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["step"], "worktree_path_validation");
    assert!(
        !main_target.exists(),
        "main repo CLAUDE.md must NOT have been written from a submodule cwd"
    );
}

#[test]
#[cfg(target_os = "macos")]
fn write_rule_subprocess_non_canonical_macos_path_lands_inside_worktree() {
    // Adversarial regression: on macOS the seed worktree path resolves
    // through a /var/ → /private/var/ symlink. A `--path` passed in
    // non-canonical /var/ form against a canonical cwd in /private/var/
    // form previously tripped the lexical `starts_with` comparison and
    // false-rejected a legitimate worktree write. The fix in
    // `canonicalize_with_fallback` resolves both sides to the same
    // /private/var/ representation before the prefix match.
    let dir = tempfile::tempdir().unwrap();
    let (_main_root, worktree) = seed_active_flow(dir.path(), "feat-x");
    if !worktree.to_string_lossy().starts_with("/private/var/") {
        return; // Non-/var tempdir layout — gate skipped silently.
    }
    let canonical_str = worktree.to_string_lossy().to_string();
    let non_canonical_str = canonical_str.replacen("/private/var/", "/var/", 1);
    let non_canonical_worktree = PathBuf::from(&non_canonical_str);
    assert!(non_canonical_worktree.exists());

    let content_file = worktree.join("content.md");
    fs::write(&content_file, "rule body").unwrap();
    let target = non_canonical_worktree
        .join(".claude")
        .join("rules")
        .join("foo.md");

    let output = run_wr_canon(
        &worktree,
        &[
            "--path",
            target.to_str().unwrap(),
            "--content-file",
            content_file.to_str().unwrap(),
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "non-canonical absolute --path resolving to a legitimate \
         worktree file must NOT be rejected. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Verify the canonical destination has the bytes (not a phantom write).
    let canonical_target = worktree.join(".claude/rules/foo.md");
    assert!(canonical_target.exists());
    assert_eq!(fs::read_to_string(&canonical_target).unwrap(), "rule body");
}

// --- end-to-end ---

#[test]
fn end_to_end_write_lib() {
    let dir = tempfile::tempdir().unwrap();
    let content_file = dir.path().join("content.md");
    fs::write(&content_file, "# Rule\n\nDo it.\n").unwrap();
    let target = dir.path().join(".claude").join("rules").join("topic.md");

    let content = read_content_file(content_file.to_str().unwrap()).unwrap();
    let result = write_rule(target.to_str().unwrap(), &content);

    assert!(result.is_ok());
    assert_eq!(fs::read_to_string(&target).unwrap(), "# Rule\n\nDo it.\n");
    assert!(!content_file.exists());
}
