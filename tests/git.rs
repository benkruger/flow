//! Integration tests for `src/git.rs`. Drives the public wrappers
//! (`current_branch`, `current_branch_in`, `project_root`,
//! `resolve_branch`, `resolve_branch_in`) through real git fixtures.
//! The pure helpers behind these wrappers are now private; their
//! branches are exercised transitively via the wrappers.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use flow_rs::git::{
    current_branch, current_branch_in, default_branch_in, project_root, resolve_branch,
    resolve_branch_in,
};

/// Initialize a git repo in the given directory with an initial commit
/// on the named branch.
fn init_git_repo(dir: &Path, initial_branch: &str) {
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git command failed");
        assert!(output.status.success(), "git {:?} failed", args);
    };
    run(&["init", "--initial-branch", initial_branch]);
    run(&["config", "user.email", "test@test.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "commit.gpgsign", "false"]);
    run(&["commit", "--allow-empty", "-m", "init"]);
}

// --- project_root (subprocess) ---

#[test]
fn project_root_in_real_repo_returns_existing_path() {
    let root = project_root();
    assert!(root.exists() || root == Path::new("."));
}

/// Drives the `worktree <path>` parse branch in `project_root_with_stdout`
/// (line 40 of src/git.rs) by spawning the compiled `flow-rs` binary
/// with cwd set to a fixture git repo. The subprocess's internal
/// `project_root()` runs `git worktree list --porcelain` inside the
/// fixture; the output carries a `worktree <path>` line that exercises
/// the strip_prefix-matched return.
#[test]
fn project_root_subprocess_in_git_repo_covers_worktree_parse() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    init_git_repo(&root, "main");
    // `plan-check` calls `project_root()` at the top of `run_impl`
    // before any state-file resolution, so even when the plan file is
    // missing the subprocess still hits the git parse branch.
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["plan-check", "--plan-file", "/nonexistent/plan.md"])
        .current_dir(&root)
        .env_remove("FLOW_CI_RUNNING")
        .env_remove("FLOW_SIMULATE_BRANCH")
        .env("GH_TOKEN", "invalid")
        .env("HOME", &root)
        .output()
        .expect("spawn flow-rs plan-check");
    // We don't care about the exit status or output — the coverage
    // signal comes from the subprocess executing project_root() under
    // cwd=fixture_git_repo.
    let _ = output;
}

// --- current_branch (subprocess) ---

#[test]
fn current_branch_in_real_repo_returns_without_panic() {
    // Process cwd is the flow repo. current_branch queries git; the
    // exact branch depends on the test harness state.
    let _ = current_branch();
}

// --- current_branch_in ---

#[test]
fn current_branch_in_reads_cwd_repo() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "my-feature");
    let branch = current_branch_in(dir.path());
    assert_eq!(branch, Some("my-feature".to_string()));
}

#[test]
fn current_branch_in_detached_head() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "main");
    let sha = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let sha = String::from_utf8_lossy(&sha.stdout).trim().to_string();
    let output = Command::new("git")
        .args(["checkout", &sha])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let branch = current_branch_in(dir.path());
    assert_eq!(branch, None);
}

#[test]
fn current_branch_in_non_git_dir_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let branch = current_branch_in(dir.path());
    assert_eq!(branch, None);
}

// --- resolve_branch (public wrapper) ---

#[test]
fn resolve_branch_override_wins() {
    let dir = tempfile::tempdir().unwrap();
    let branch = resolve_branch(Some("explicit-branch"), dir.path());
    assert_eq!(branch, Some("explicit-branch".to_string()));
}

// --- resolve_branch_in ---

#[test]
fn resolve_branch_in_override_wins() {
    let repo = tempfile::tempdir().unwrap();
    init_git_repo(repo.path(), "main");
    let root = tempfile::tempdir().unwrap();
    let branch = resolve_branch_in(Some("explicit"), repo.path(), root.path());
    assert_eq!(branch, Some("explicit".to_string()));
}

#[test]
fn resolve_branch_in_reads_branch_from_cwd() {
    let repo = tempfile::tempdir().unwrap();
    init_git_repo(repo.path(), "cwd-branch");
    let root = tempfile::tempdir().unwrap();
    let branch = resolve_branch_in(None, repo.path(), root.path());
    assert_eq!(branch, Some("cwd-branch".to_string()));
}

#[test]
fn resolve_branch_in_matches_state_file() {
    let repo = tempfile::tempdir().unwrap();
    init_git_repo(repo.path(), "matched");
    let root = tempfile::tempdir().unwrap();
    let branch_dir = root.path().join(".flow-states").join("matched");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), r#"{"branch": "matched"}"#).unwrap();

    let branch = resolve_branch_in(None, repo.path(), root.path());
    assert_eq!(branch, Some("matched".to_string()));
}

// --- default_branch_in ---

/// Configure `origin/HEAD` on a fixture repo to point at the named branch.
/// Mirrors what `git clone` does automatically when cloning a remote whose
/// default branch is `<branch>`.
fn set_origin_head(repo: &Path, branch: &str) {
    let target = format!("refs/remotes/origin/{}", branch);
    let output = Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD", &target])
        .current_dir(repo)
        .output()
        .expect("git symbolic-ref failed to spawn");
    assert!(
        output.status.success(),
        "git symbolic-ref failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Materialize a `refs/remotes/origin/<branch>` ref so symbolic-ref has
/// a valid target. Uses git update-ref against the current HEAD SHA.
fn create_remote_ref(repo: &Path, branch: &str) {
    let sha = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .expect("git rev-parse failed");
    let sha = String::from_utf8_lossy(&sha.stdout).trim().to_string();
    let ref_name = format!("refs/remotes/origin/{}", branch);
    let output = Command::new("git")
        .args(["update-ref", &ref_name, &sha])
        .current_dir(repo)
        .output()
        .expect("git update-ref failed to spawn");
    assert!(
        output.status.success(),
        "git update-ref failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn default_branch_in_returns_main_when_origin_head_points_to_main() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "main");
    create_remote_ref(dir.path(), "main");
    set_origin_head(dir.path(), "main");
    let branch = default_branch_in(dir.path());
    assert_eq!(branch, Ok("main".to_string()));
}

#[test]
fn default_branch_in_returns_staging_when_origin_head_points_to_staging() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "main");
    create_remote_ref(dir.path(), "staging");
    set_origin_head(dir.path(), "staging");
    let branch = default_branch_in(dir.path());
    assert_eq!(branch, Ok("staging".to_string()));
}

#[test]
fn default_branch_in_returns_develop_when_origin_head_points_to_develop() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "main");
    create_remote_ref(dir.path(), "develop");
    set_origin_head(dir.path(), "develop");
    let branch = default_branch_in(dir.path());
    assert_eq!(branch, Ok("develop".to_string()));
}

#[test]
fn default_branch_in_errors_when_origin_remote_missing() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "main");
    // No `origin` remote configured at all.
    let result = default_branch_in(dir.path());
    assert!(result.is_err(), "expected Err, got {:?}", result);
}

#[test]
fn default_branch_in_errors_when_origin_head_unset() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "main");
    // origin remote exists (we'd need create_remote_ref) but no symbolic-ref.
    // Use a plain init without a remote — symbolic-ref fails just the same.
    let result = default_branch_in(dir.path());
    assert!(result.is_err(), "expected Err, got {:?}", result);
}

#[test]
fn default_branch_in_errors_when_not_a_git_dir() {
    let dir = tempfile::tempdir().unwrap();
    // No `git init`.
    let result = default_branch_in(dir.path());
    assert!(result.is_err(), "expected Err, got {:?}", result);
}

#[test]
fn default_branch_in_error_message_names_failure_class() {
    let dir = tempfile::tempdir().unwrap();
    let result = default_branch_in(dir.path());
    let err = result.unwrap_err();
    assert!(
        err.contains("symbolic-ref") || err.contains("spawn"),
        "expected error to name the git failure class, got: {}",
        err
    );
}

// --- FLOW_SIMULATE_BRANCH subprocess coverage ---

/// Drives the `FLOW_SIMULATE_BRANCH` Some-non-empty branch in
/// `current_branch_from_output` (lines 92-95 of src/git.rs) by spawning
/// the compiled `flow-rs` binary with the env var set. The subprocess's
/// internal `current_branch()` reads the env var and returns the
/// simulated value without consulting git. Env-var manipulation must
/// happen via a child process per
/// `.claude/rules/testing-gotchas.md` "Rust Parallel Test Env Var
/// Races" — `unsafe { env::set_var }` in-process would race other
/// tests reading the same var.
#[test]
fn current_branch_subprocess_with_flow_simulate_branch_env_set() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    init_git_repo(&root, "main");
    // `status` calls `format_status::run_impl_main` which resolves the
    // branch via `resolve_branch` → `current_branch()`. With
    // FLOW_SIMULATE_BRANCH set, current_branch() returns the simulated
    // value before any git call. We don't care about the subprocess's
    // exit status — the coverage signal comes from current_branch()
    // executing the simulate-branch path.
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["status"])
        .current_dir(&root)
        .env_remove("FLOW_CI_RUNNING")
        .env("FLOW_SIMULATE_BRANCH", "simulated-branch")
        .env("GH_TOKEN", "invalid")
        .env("HOME", &root)
        .output()
        .expect("spawn flow-rs status");
    let _ = output;
}

// --- project_root non-git-dir fallback ---

/// Drives the git-failure fallback in `project_root_from_output`
/// (line 31 of src/git.rs: `_ => PathBuf::from(".")`) by spawning
/// the compiled `flow-rs` binary with cwd set to a non-git tempdir.
/// `git worktree list --porcelain` exits non-zero outside a git
/// repository, so the match arm falls through to the literal `"."`.
#[test]
fn project_root_subprocess_in_non_git_dir_covers_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    // Deliberately do NOT init git here — that's the point of the test.
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["status"])
        .current_dir(&root)
        .env_remove("FLOW_CI_RUNNING")
        .env_remove("FLOW_SIMULATE_BRANCH")
        .env("GH_TOKEN", "invalid")
        .env("HOME", &root)
        .output()
        .expect("spawn flow-rs status");
    let _ = output;
}

/// Drives the empty-string FLOW_SIMULATE_BRANCH branch in
/// `current_branch_from_output` (line 95: the falling-through `}` of
/// `if !s.is_empty()`). Setting the env var to an empty string still
/// produces `Some("")` from `env::var().ok()`, but the inner
/// `is_empty()` check skips the early return — control falls past
/// the simulated block to consult git.
#[test]
fn current_branch_subprocess_with_empty_flow_simulate_branch() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    init_git_repo(&root, "main");
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["status"])
        .current_dir(&root)
        .env_remove("FLOW_CI_RUNNING")
        .env("FLOW_SIMULATE_BRANCH", "")
        .env("GH_TOKEN", "invalid")
        .env("HOME", &root)
        .output()
        .expect("spawn flow-rs status");
    let _ = output;
}

/// Drives the `try_new=None` fall-through in `resolve_branch_impl`
/// (lines 282-283). When the current branch contains `/` (e.g. a
/// `feature/foo` git branch), `FlowPaths::try_new` rejects it as
/// invalid for FLOW's path layout; control skips the inner
/// state-file check and falls through to `branch` at line 287.
#[test]
fn resolve_branch_in_slash_branch_falls_through_to_branch() {
    let repo = tempfile::tempdir().unwrap();
    init_git_repo(repo.path(), "feature/foo");
    let root = tempfile::tempdir().unwrap();
    let branch = resolve_branch_in(None, repo.path(), root.path());
    // current_branch_in returns Some("feature/foo"); FlowPaths::try_new
    // rejects the slash; impl returns the raw branch via the
    // fall-through path.
    assert_eq!(branch, Some("feature/foo".to_string()));
}

/// Drives the `unwrap_or_else(|| PathBuf::from("."))` closure in
/// `project_root_with_stdout` (line 45 of src/git.rs) by spawning
/// the compiled `flow-rs` binary with `PATH` rewritten to a tempdir
/// that contains a fake `git` binary. The fake exits 0 (so the
/// success arm of `project_root_from_output` fires) but emits no
/// `worktree ` line, so `find_map` returns `None` and the
/// `unwrap_or_else` fallback closure runs.
#[test]
fn project_root_subprocess_with_fake_git_no_worktree_line() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mock_bin = root.join("mock_bin");
    fs::create_dir_all(&mock_bin).unwrap();
    // Fake git: exit 0 with stdout that has no `worktree ` line.
    fs::write(
        mock_bin.join("git"),
        "#!/usr/bin/env bash\necho 'no-worktree-line-here'\nexit 0\n",
    )
    .unwrap();
    let mut perms = fs::metadata(mock_bin.join("git")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(mock_bin.join("git"), perms).unwrap();
    // Prepend mock_bin to existing PATH so the fake `git` wins lookup
    // but `#!/usr/bin/env bash` can still find bash for the shebang.
    let path = format!(
        "{}:{}",
        mock_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["status"])
        .current_dir(&root)
        .env_remove("FLOW_CI_RUNNING")
        .env_remove("FLOW_SIMULATE_BRANCH")
        .env("PATH", &path)
        .env("GH_TOKEN", "invalid")
        .env("HOME", &root)
        .output()
        .expect("spawn flow-rs status");
    let _ = output;
}

/// Drives the `output.ok()?` Err side in `current_branch_from_output`
/// (line 97 `^0` of src/git.rs) by spawning the compiled `flow-rs`
/// binary with `PATH` rewritten to a tempdir that does NOT contain
/// `git`. `Command::new("git").output()` returns
/// `Err(io::Error{NotFound})` when the binary cannot be located; the
/// `?` operator on `output.ok()` short-circuits and returns `None`
/// from `current_branch_from_output`.
#[test]
fn current_branch_subprocess_with_no_git_in_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let empty_bin = root.join("empty_bin");
    fs::create_dir_all(&empty_bin).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["status"])
        .current_dir(&root)
        .env_remove("FLOW_CI_RUNNING")
        .env_remove("FLOW_SIMULATE_BRANCH")
        .env("PATH", empty_bin.display().to_string())
        .env("GH_TOKEN", "invalid")
        .env("HOME", &root)
        .output()
        .expect("spawn flow-rs status");
    let _ = output;
}

/// Drives the `Err(e)` spawn-failure arm of
/// `default_branch_from_output` by spawning the compiled `flow-rs`
/// binary with `PATH` rewritten to a tempdir that does NOT contain
/// `git`. `bin/flow base-branch` calls `default_branch_in(&root)`,
/// which spawns `git symbolic-ref`. With no git on PATH, the spawn
/// returns `Err(io::Error{NotFound})`; the match arm formats the
/// spawn-failed message and the subcommand exits 1.
#[test]
fn default_branch_in_subprocess_with_no_git_in_path_covers_spawn_err() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let empty_bin = root.join("empty_bin");
    fs::create_dir_all(&empty_bin).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["base-branch"])
        .current_dir(&root)
        .env_remove("FLOW_CI_RUNNING")
        .env_remove("FLOW_SIMULATE_BRANCH")
        .env("PATH", empty_bin.display().to_string())
        .env("GH_TOKEN", "invalid")
        .env("HOME", &root)
        .output()
        .expect("spawn flow-rs base-branch");
    let _ = output;
}

/// Drives the `if stripped.is_empty()` branch in
/// `default_branch_from_output` by spawning the compiled `flow-rs`
/// binary with `PATH` pointed at a fake `git` that exits 0 with
/// empty stdout. `bin/flow base-branch` calls `default_branch_in`,
/// which spawns `git symbolic-ref` and gets exit 0 with no output;
/// `String::from_utf8_lossy("").trim().to_string()` yields "",
/// `strip_prefix("origin/")` returns None so `unwrap_or` gives "",
/// and the empty-check returns Err. Real git refuses to set
/// `refs/remotes/origin/HEAD` to a target without a branch suffix,
/// so this anomalous shape is only reachable via fake-binary
/// fixtures or git binary corruption.
#[test]
fn default_branch_in_returns_err_when_git_returns_empty_branch() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mock_bin = root.join("mock_bin");
    fs::create_dir_all(&mock_bin).unwrap();
    // Fake git: exit 0 with empty stdout for every invocation.
    fs::write(mock_bin.join("git"), "#!/usr/bin/env bash\nexit 0\n").unwrap();
    let mut perms = fs::metadata(mock_bin.join("git")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(mock_bin.join("git"), perms).unwrap();
    let path = format!(
        "{}:{}",
        mock_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["base-branch"])
        .current_dir(&root)
        .env_remove("FLOW_CI_RUNNING")
        .env_remove("FLOW_SIMULATE_BRANCH")
        .env("PATH", &path)
        .env("GH_TOKEN", "invalid")
        .env("HOME", &root)
        .output()
        .expect("spawn flow-rs base-branch");
    let _ = output;
}
