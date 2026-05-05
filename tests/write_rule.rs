//! Integration tests for `bin/flow write-rule`.

mod common;

use std::fs;
use std::path::Path;
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
