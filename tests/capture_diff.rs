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
use flow_rs::capture_diff::{run_impl, Args};

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

// --- Task 3: --family per-family diff slicing ---
//
// The read-overflow remediation in flow-review Step 2 re-invokes the
// documentation agent once per file family under a bounded-read
// protocol. To keep each re-invocation's diff read bounded, the skill
// slices the substantive diff per file family via `capture-diff
// --family <pathspec>`, which writes one `substantive-diff-<family>.diff`
// file per family under the branch dir. Each `--family` pathspec is
// external CLI input reaching two sinks (the `git diff -- <pathspec>`
// subprocess arg and the output filename), so the validator rejects
// empty, NUL, newline/CR, `..` traversal components, and leading-`:`
// pathspec magic per `.claude/rules/external-input-path-construction.md`.

/// Create a fixture worktree with committed changes under `src/` and
/// `tests/` subdirectories so a `--family <subdir>/` pathspec produces
/// a non-empty per-family slice.
fn fixture_with_subdir_commits(repo: &Path) {
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::create_dir_all(repo.join("tests")).unwrap();
    fs::write(repo.join("src/lib.rs"), "// src change\n").unwrap();
    fs::write(repo.join("tests/foo.rs"), "// test change\n").unwrap();
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "subdir changes"])
        .current_dir(repo)
        .output()
        .unwrap();
}

#[test]
fn capture_diff_family_writes_per_family_slice_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_subdir_commits(&repo);

    let output = run_capture_diff(
        &repo,
        &["--branch", "fam-one", "--base", "main", "--family", "src/"],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let slice = repo.join(".flow-states/fam-one/substantive-diff-src.diff");
    assert!(slice.exists(), "per-family slice missing at {:?}", slice);
    let content = fs::read_to_string(&slice).unwrap();
    assert!(
        content.contains("src/lib.rs"),
        "slice should contain the src/ change"
    );
    assert!(
        !content.contains("tests/foo.rs"),
        "src family slice must not contain the tests/ change"
    );
}

#[test]
fn capture_diff_family_multiple_writes_all_slices() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_subdir_commits(&repo);

    let output = run_capture_diff(
        &repo,
        &[
            "--branch",
            "fam-multi",
            "--base",
            "main",
            "--family",
            "src/",
            "--family",
            "tests/",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    let src_slice = repo.join(".flow-states/fam-multi/substantive-diff-src.diff");
    let tests_slice = repo.join(".flow-states/fam-multi/substantive-diff-tests.diff");
    assert!(
        src_slice.exists() && tests_slice.exists(),
        "both per-family slices must be written"
    );
    assert!(fs::read_to_string(&src_slice)
        .unwrap()
        .contains("src/lib.rs"));
    assert!(fs::read_to_string(&tests_slice)
        .unwrap()
        .contains("tests/foo.rs"));
}

#[test]
fn capture_diff_family_envelope_lists_slices() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_subdir_commits(&repo);

    let output = run_capture_diff(
        &repo,
        &["--branch", "fam-env", "--base", "main", "--family", "src/"],
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    let slices = data["family_slices"]
        .as_array()
        .expect("family_slices array");
    assert_eq!(slices.len(), 1);
    assert_eq!(slices[0]["family"], "src/");
    assert!(slices[0]["path"]
        .as_str()
        .unwrap()
        .ends_with(".flow-states/fam-env/substantive-diff-src.diff"));
}

#[test]
fn capture_diff_no_family_omits_slices_field() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_subdir_commits(&repo);

    let output = run_capture_diff(&repo, &["--branch", "no-fam", "--base", "main"]);

    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert!(
        data["family_slices"].is_null(),
        "family_slices must be absent when no --family is passed (byte-compatible with flow-learn's parse)"
    );
}

#[test]
fn capture_diff_family_rejects_traversal() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_subdir_commits(&repo);

    let output = run_capture_diff(
        &repo,
        &[
            "--branch",
            "fam-trav",
            "--base",
            "main",
            "--family",
            "../../etc",
        ],
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"].as_str().unwrap().contains("invalid family"));
    // No slice file escaped the branch dir.
    assert!(
        !dir.path().join("etc").exists(),
        "traversal pathspec must not write outside the branch dir"
    );
}

#[test]
fn capture_diff_family_rejects_pathspec_magic() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_subdir_commits(&repo);

    let output = run_capture_diff(
        &repo,
        &["--branch", "fam-magic", "--base", "main", "--family", ":/"],
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"].as_str().unwrap().contains("invalid family"));
}

#[test]
fn capture_diff_family_rejects_newline() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_subdir_commits(&repo);

    let output = run_capture_diff(
        &repo,
        &[
            "--branch", "fam-nl", "--base", "main", "--family", "src\nfoo",
        ],
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"].as_str().unwrap().contains("invalid family"));
}

#[test]
fn capture_diff_family_rejects_empty() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_subdir_commits(&repo);

    let output = run_capture_diff(
        &repo,
        &["--branch", "fam-empty", "--base", "main", "--family", ""],
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"].as_str().unwrap().contains("invalid family"));
}

#[test]
fn capture_diff_returns_error_when_family_diff_write_path_is_directory() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_subdir_commits(&repo);

    // Pre-create the per-family slice path as a directory so fs::write
    // fails AFTER the full and substantive writes succeed — exercises
    // the post-write Err arm for the family diff.
    fs::create_dir_all(repo.join(".flow-states/fam-write-fail/substantive-diff-src.diff")).unwrap();

    let output = run_capture_diff(
        &repo,
        &[
            "--branch",
            "fam-write-fail",
            "--base",
            "main",
            "--family",
            "src/",
        ],
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap()
        .contains("write family diff"));
}

// --- Review #1845: --family validator + collision hardening ---
//
// Review's adversarial agent proved (with failing probes) that the
// original --family validator accepted a backslash and a leading-slash
// (absolute / degenerate `/`) pathspec, and that family_filename_component
// is non-injective so two distinct families collide on one slice
// filename and silently clobber each other. These regression tests lock
// the fixes: is_safe_family now rejects `\` and leading `/`, and capture()
// rejects a slice-filename collision (and the exact-duplicate degenerate)
// before any write.

#[test]
fn capture_diff_family_rejects_backslash() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_subdir_commits(&repo);

    let output = run_capture_diff(
        &repo,
        &[
            "--branch",
            "fam-bslash",
            "--base",
            "main",
            "--family",
            "a\\b",
        ],
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"].as_str().unwrap().contains("invalid family"));
}

#[test]
fn capture_diff_family_rejects_absolute_path() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_subdir_commits(&repo);

    let output = run_capture_diff(
        &repo,
        &["--branch", "fam-abs", "--base", "main", "--family", "/etc"],
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"].as_str().unwrap().contains("invalid family"));
}

#[test]
fn capture_diff_family_rejects_filename_collision() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_subdir_commits(&repo);

    // `a/b` and `a_b` both fold to the slice name `a_b`; rejecting the
    // collision prevents the second write from clobbering the first slice.
    let output = run_capture_diff(
        &repo,
        &[
            "--branch",
            "fam-collide",
            "--base",
            "main",
            "--family",
            "a/b",
            "--family",
            "a_b",
        ],
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    let msg = data["message"].as_str().unwrap();
    assert!(msg.contains("invalid family"));
    assert!(msg.contains("collides"));
}

#[test]
fn capture_diff_family_rejects_duplicate() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    fixture_with_subdir_commits(&repo);

    // An exact-duplicate --family value is the degenerate case of the
    // filename collision and is rejected by the same guard.
    let output = run_capture_diff(
        &repo,
        &[
            "--branch", "fam-dup", "--base", "main", "--family", "src/", "--family", "src/",
        ],
    );

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"].as_str().unwrap().contains("collides"));
}

/// NUL bytes cannot be passed through argv to the child binary (the OS
/// rejects them at spawn), so the NUL rejection is exercised via the
/// library entry point directly. Validating every family BEFORE any git
/// subprocess runs means the NUL family returns the business-error
/// envelope without spawning git.
#[test]
fn capture_diff_family_rejects_nul_via_run_impl() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        branch: "fam-nul".to_string(),
        base: "main".to_string(),
        family: vec!["a\0b".to_string()],
    };
    let (val, code) = run_impl(&args, dir.path(), dir.path());
    assert_eq!(code, 0);
    assert_eq!(val["status"], "error");
    assert!(val["message"].as_str().unwrap().contains("invalid family"));
}
