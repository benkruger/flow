//! Integration tests for `src/hooks/validate_worktree_paths.rs`.

use std::io::Write;
use std::process::{Command, Stdio};

use flow_rs::hooks::validate_worktree_paths::{
    detect_misplaced_flow_states, get_file_path, is_shared_config, validate, validate_shared_config,
};
use serde_json::json;

// --- validate tests ---

#[test]
fn test_allows_when_not_in_worktree() {
    let (allowed, msg) = validate("/Users/ben/code/flow/lib/foo.py", "/Users/ben/code/flow");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_file_inside_worktree() {
    let (allowed, msg) = validate(
        "/Users/ben/code/flow/.worktrees/my-feature/lib/foo.py",
        "/Users/ben/code/flow/.worktrees/my-feature",
    );
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_blocks_main_repo_path_from_worktree() {
    let cwd = "/Users/ben/code/flow/.worktrees/my-feature";
    let (allowed, msg) = validate("/Users/ben/code/flow/lib/foo.py", cwd);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains(cwd));
}

#[test]
fn test_allows_flow_states_path() {
    let (allowed, msg) = validate(
        "/Users/ben/code/flow/.flow-states/my-feature.json",
        "/Users/ben/code/flow/.worktrees/my-feature",
    );
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_home_directory_paths() {
    let (allowed, msg) = validate(
        "/Users/ben/.claude/plans/some-plan.md",
        "/Users/ben/code/flow/.worktrees/my-feature",
    );
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_plugin_cache_paths() {
    let (allowed, msg) = validate(
        "/Users/ben/.claude/plugins/cache/flow/0.28.5/skills/flow-code/SKILL.md",
        "/Users/ben/code/flow/.worktrees/my-feature",
    );
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_error_message_includes_corrected_path() {
    let cwd = "/Users/ben/code/flow/.worktrees/my-feature";
    let file_path = "/Users/ben/code/flow/skills/flow-prime/SKILL.md";
    let (allowed, msg) = validate(file_path, cwd);
    assert!(!allowed);
    let corrected = "/Users/ben/code/flow/.worktrees/my-feature/skills/flow-prime/SKILL.md";
    assert!(msg.contains(corrected));
    assert!(msg.contains(file_path));
}

#[test]
fn test_allows_empty_file_path() {
    let (allowed, _) = validate("", "/Users/ben/code/flow/.worktrees/my-feature");
    assert!(allowed);
}

#[test]
fn test_allows_worktree_root_path_exactly() {
    let cwd = "/Users/ben/code/flow/.worktrees/my-feature";
    let (allowed, _) = validate(cwd, cwd);
    assert!(allowed);
}

// --- get_file_path tests ---

#[test]
fn test_get_file_path_prefers_file_path() {
    let tool_input = json!({"file_path": "/some/path.py", "path": "/other/path"});
    assert_eq!(get_file_path(&tool_input), "/some/path.py");
}

#[test]
fn test_get_file_path_falls_back_to_path() {
    let tool_input = json!({"path": "/some/dir"});
    assert_eq!(get_file_path(&tool_input), "/some/dir");
}

#[test]
fn test_get_file_path_returns_empty_for_neither() {
    let tool_input = json!({"command": "something"});
    assert_eq!(get_file_path(&tool_input), "");
}

// --- is_shared_config tests ---

#[test]
fn test_shared_config_gitignore() {
    assert!(is_shared_config("/project/.worktrees/feat/.gitignore"));
}

#[test]
fn test_shared_config_gitattributes() {
    assert!(is_shared_config("/project/.worktrees/feat/.gitattributes"));
}

#[test]
fn test_shared_config_makefile() {
    assert!(is_shared_config("/project/.worktrees/feat/Makefile"));
}

#[test]
fn test_shared_config_rakefile() {
    assert!(is_shared_config("/project/.worktrees/feat/Rakefile"));
}

#[test]
fn test_shared_config_justfile() {
    assert!(is_shared_config("/project/.worktrees/feat/justfile"));
}

#[test]
fn test_shared_config_package_json() {
    assert!(is_shared_config("/project/.worktrees/feat/package.json"));
}

#[test]
fn test_shared_config_requirements_txt() {
    assert!(is_shared_config(
        "/project/.worktrees/feat/requirements.txt"
    ));
}

#[test]
fn test_shared_config_go_mod() {
    assert!(is_shared_config("/project/.worktrees/feat/go.mod"));
}

#[test]
fn test_shared_config_cargo_toml() {
    assert!(is_shared_config("/project/.worktrees/feat/Cargo.toml"));
}

#[test]
fn test_shared_config_github_directory() {
    assert!(is_shared_config(
        "/project/.worktrees/feat/.github/workflows/ci.yml"
    ));
}

#[test]
fn test_shared_config_github_codeowners() {
    assert!(is_shared_config(
        "/project/.worktrees/feat/.github/CODEOWNERS"
    ));
}

#[test]
fn test_shared_config_not_regular_file() {
    assert!(!is_shared_config("/project/.worktrees/feat/src/lib.rs"));
}

#[test]
fn test_shared_config_not_readme() {
    assert!(!is_shared_config("/project/.worktrees/feat/README.md"));
}

#[test]
fn test_shared_config_empty_path() {
    assert!(!is_shared_config(""));
}

#[test]
fn test_shared_config_case_sensitive_makefile() {
    assert!(!is_shared_config("/project/.worktrees/feat/makefile"));
}

#[test]
fn test_shared_config_github_directory_itself() {
    assert!(!is_shared_config("/project/.worktrees/feat/.github"));
}

// --- validate_shared_config tests ---

#[test]
fn test_shared_config_edit_gitignore_blocked() {
    let cwd = "/project/.worktrees/feat";
    let file_path = "/project/.worktrees/feat/.gitignore";
    let (allowed, msg) = validate_shared_config(file_path, cwd, "Edit");
    assert!(!allowed);
    assert!(msg.contains("shared configuration"));
    assert!(msg.contains("permissions.md"));
}

#[test]
fn test_shared_config_write_cargo_toml_blocked() {
    let cwd = "/project/.worktrees/feat";
    let file_path = "/project/.worktrees/feat/Cargo.toml";
    let (allowed, msg) = validate_shared_config(file_path, cwd, "Write");
    assert!(!allowed);
    assert!(msg.contains("shared configuration"));
}

#[test]
fn test_shared_config_read_gitignore_allowed() {
    let cwd = "/project/.worktrees/feat";
    let file_path = "/project/.worktrees/feat/.gitignore";
    let (allowed, msg) = validate_shared_config(file_path, cwd, "Read");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_shared_config_grep_github_allowed() {
    let cwd = "/project/.worktrees/feat";
    let file_path = "/project/.worktrees/feat/.github/workflows/ci.yml";
    let (allowed, msg) = validate_shared_config(file_path, cwd, "Grep");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_shared_config_edit_outside_worktree_allowed() {
    let cwd = "/project";
    let file_path = "/project/.gitignore";
    let (allowed, msg) = validate_shared_config(file_path, cwd, "Edit");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_shared_config_edit_regular_file_allowed() {
    let cwd = "/project/.worktrees/feat";
    let file_path = "/project/.worktrees/feat/src/lib.rs";
    let (allowed, msg) = validate_shared_config(file_path, cwd, "Edit");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_shared_config_empty_path_allowed() {
    let cwd = "/project/.worktrees/feat";
    let (allowed, msg) = validate_shared_config("", cwd, "Edit");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_shared_config_edit_main_repo_shared_allowed() {
    let cwd = "/project/.worktrees/feat";
    let file_path = "/project/.gitignore";
    let (allowed, msg) = validate_shared_config(file_path, cwd, "Edit");
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_shared_config_edit_github_workflow_blocked() {
    let cwd = "/project/.worktrees/feat";
    let file_path = "/project/.worktrees/feat/.github/workflows/ci.yml";
    let (allowed, msg) = validate_shared_config(file_path, cwd, "Edit");
    assert!(!allowed);
    assert!(msg.contains("shared configuration"));
}

#[test]
fn test_shared_config_empty_tool_name_allowed() {
    let cwd = "/project/.worktrees/feat";
    let file_path = "/project/.worktrees/feat/.gitignore";
    let (allowed, msg) = validate_shared_config(file_path, cwd, "");
    assert!(allowed);
    assert!(msg.is_empty());
}

// Direct `run_impl_main` tests removed — decision core is now
// private. Its branches are driven through the subprocess tests
// below that spawn `bin/flow hook validate-worktree-paths`.

// --- run() subprocess smoke test ---

fn run_hook(stdin_input: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["hook", "validate-worktree-paths"])
        .env_remove("FLOW_CI_RUNNING")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn flow-rs");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(stdin_input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().expect("wait");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn run_subprocess_exits_0_with_no_hook_input() {
    // Empty stdin → read_hook_input returns None → exit 0.
    let (code, _, _) = run_hook("");
    assert_eq!(code, 0);
}

#[test]
fn run_subprocess_exits_0_with_empty_file_path() {
    let (code, _, _) = run_hook("{\"tool_input\": {}}");
    assert_eq!(code, 0);
}

/// Covers the `file_path == cwd` short-circuit path in
/// `validate_shared_config`: when the file_path is exactly the worktree
/// root (no `/` suffix), it should NOT be treated as a main-repo path
/// nor as a shared config file.
#[test]
fn test_shared_config_file_path_equals_cwd_allowed() {
    let cwd = "/project/.worktrees/feat";
    // file_path equals cwd — not starts_with cwd_prefix but equals cwd.
    let (allowed, _) = validate_shared_config(cwd, cwd, "Edit");
    // The cwd path itself is not a shared-config filename (it's a dir),
    // so allowed = true.
    assert!(allowed);
}

/// Drives the `None => return (0, None)` branch in `run_impl_main`'s
/// cwd match. The production wrapper reads cwd from
/// `std::env::current_dir().ok().map(...)`; forcing the subprocess's
/// cwd inode to be unlinked via `pre_exec` + `rmdir` makes
/// `getcwd(3)` return `ENOENT`, so the hook sees `cwd = None`.
#[cfg(unix)]
#[test]
fn run_subprocess_exits_0_when_current_dir_fails() {
    use std::os::unix::process::CommandExt;

    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let cwd = root.join("doomed");
    std::fs::create_dir(&cwd).expect("mkdir doomed");

    let preexec_path = std::ffi::CString::new(cwd.to_str().expect("utf8").as_bytes())
        .expect("CString from cwd path");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.args(["hook", "validate-worktree-paths"])
        .env_remove("FLOW_CI_RUNNING")
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // SAFETY: `libc::rmdir` is POSIX async-signal-safe; the closure
    // allocates no memory and does not panic.
    unsafe {
        cmd.pre_exec(move || {
            libc::rmdir(preexec_path.as_ptr());
            Ok(())
        });
    }

    let mut child = cmd.spawn().expect("spawn flow-rs");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(br#"{"tool_input":{"file_path":"/tmp/x"}}"#)
        .unwrap();
    let output = child.wait_with_output().expect("wait");
    // With cwd unresolvable, the hook exits 0 (no-op) because it
    // cannot validate a path it can't contextualize against a
    // worktree root.
    assert_eq!(output.status.code(), Some(0));
}

// --- detect_misplaced_flow_states tests ---

#[test]
fn detect_misplaced_returns_none_for_canonical_path() {
    let result = detect_misplaced_flow_states(
        "/Users/ben/code/flow/.flow-states/foo.md",
        "/Users/ben/code/flow",
    );
    assert!(result.is_none());
}

#[test]
fn detect_misplaced_returns_canonical_for_worktree_root_flow_states() {
    let result = detect_misplaced_flow_states(
        "/Users/ben/code/flow/.worktrees/feat/.flow-states/foo.md",
        "/Users/ben/code/flow",
    );
    assert_eq!(
        result,
        Some("/Users/ben/code/flow/.flow-states/foo.md".to_string())
    );
}

#[test]
fn detect_misplaced_returns_canonical_for_service_subdir_flow_states() {
    let result = detect_misplaced_flow_states(
        "/Users/ben/code/flow/.worktrees/feat/api/.flow-states/foo.md",
        "/Users/ben/code/flow",
    );
    assert_eq!(
        result,
        Some("/Users/ben/code/flow/.flow-states/foo.md".to_string())
    );
}

#[test]
fn detect_misplaced_returns_canonical_for_deep_nested_flow_states() {
    let result = detect_misplaced_flow_states(
        "/Users/ben/code/flow/.worktrees/feat/api/sub/.flow-states/foo.md",
        "/Users/ben/code/flow",
    );
    assert_eq!(
        result,
        Some("/Users/ben/code/flow/.flow-states/foo.md".to_string())
    );
}

#[test]
fn detect_misplaced_returns_none_for_paths_without_flow_states() {
    let result = detect_misplaced_flow_states(
        "/Users/ben/code/flow/.worktrees/feat/src/main.rs",
        "/Users/ben/code/flow",
    );
    assert!(result.is_none());
}

#[test]
fn detect_misplaced_returns_none_for_paths_outside_project() {
    let result = detect_misplaced_flow_states("/home/user/.claude/foo", "/Users/ben/code/flow");
    assert!(result.is_none());
}

#[test]
fn detect_misplaced_returns_none_for_substring_match() {
    let result = detect_misplaced_flow_states(
        "/Users/ben/code/flow/.worktrees/feat/foo-flow-states-bar",
        "/Users/ben/code/flow",
    );
    assert!(result.is_none());
}

#[test]
fn detect_misplaced_returns_none_for_main_repo_flow_states() {
    let result = detect_misplaced_flow_states(
        "/Users/ben/code/flow/.flow-states/foo.md",
        "/Users/ben/code/flow",
    );
    assert!(result.is_none());
}

#[test]
fn detect_misplaced_returns_none_when_no_slash_after_worktrees_prefix() {
    // Path matches `<root>/.worktrees/` literally but has no `/` after
    // the branch token, so `after_worktrees.find('/')` returns None
    // and the helper short-circuits without trying to detect the
    // `.flow-states/` segment.
    let result = detect_misplaced_flow_states(
        "/Users/ben/code/flow/.worktrees/lonely-branch",
        "/Users/ben/code/flow",
    );
    assert!(result.is_none());
}

// --- validate() .flow-states/ canonicalization tests ---

#[test]
fn validate_rejects_worktree_flow_states_write() {
    let cwd = "/Users/ben/code/flow/.worktrees/feat/api";
    let file_path = "/Users/ben/code/flow/.worktrees/feat/.flow-states/plan.md";
    let (allowed, msg) = validate(file_path, cwd);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains(".flow-states/"));
    assert!(msg.contains("/Users/ben/code/flow/.flow-states/plan.md"));
    assert!(msg.contains(file_path));
}

#[test]
fn validate_rejects_service_subdir_flow_states_write() {
    let cwd = "/Users/ben/code/flow/.worktrees/feat/api";
    let file_path = "/Users/ben/code/flow/.worktrees/feat/api/.flow-states/plan.md";
    let (allowed, msg) = validate(file_path, cwd);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("/Users/ben/code/flow/.flow-states/plan.md"));
}

#[test]
fn validate_accepts_main_repo_flow_states_write_from_subdir_cwd() {
    let cwd = "/Users/ben/code/flow/.worktrees/feat/api";
    let file_path = "/Users/ben/code/flow/.flow-states/feat/plan.md";
    let (allowed, msg) = validate(file_path, cwd);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn validate_accepts_worktree_non_flow_states_write() {
    let cwd = "/Users/ben/code/flow/.worktrees/feat/api";
    let file_path = "/Users/ben/code/flow/.worktrees/feat/api/src/main.rs";
    let (allowed, msg) = validate(file_path, cwd);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn validate_plain_repo_no_op() {
    // No .worktrees/ in cwd — the new check is bypassed via the
    // pre-existing "not in a worktree" early-return.
    let cwd = "/Users/ben/code/flow";
    let file_path = "/Users/ben/code/flow/.flow-states/feat/plan.md";
    let (allowed, msg) = validate(file_path, cwd);
    assert!(allowed);
    assert!(msg.is_empty());
}

// --- subprocess matrix: .flow-states/ canonicalization per tool ---

/// Spawn the hook with stdin matching `tool_name` + `path_field`,
/// targeted at `file_path`, with cwd set to `worktree_cwd`.
///
/// Per `.claude/rules/subprocess-test-hygiene.md`: removes
/// `FLOW_CI_RUNNING` (the `bin/flow hook` family inherits the parent's
/// CI guard) and pins `HOME` to a tempdir so the child reads no user
/// dotfiles. Returns `(exit_code, stdout, stderr)`.
fn spawn_hook_with_cwd(
    worktree_cwd: &std::path::Path,
    home: &std::path::Path,
    tool_name: &str,
    path_field: &str,
    file_path: &str,
) -> (i32, String, String) {
    let stdin_input = format!(
        r#"{{"tool_name":"{}","tool_input":{{"{}":"{}"}}}}"#,
        tool_name, path_field, file_path
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["hook", "validate-worktree-paths"])
        .env_remove("FLOW_CI_RUNNING")
        .env("HOME", home)
        .current_dir(worktree_cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn flow-rs");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(stdin_input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().expect("wait");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

/// Build a fixture worktree at `<canonical_tmp>/.worktrees/feat` and
/// return `(root, worktree_cwd)`. Per
/// `.claude/rules/testing-gotchas.md` "macOS Subprocess Path
/// Canonicalization": canonicalize the tempdir root before any
/// descendant `join()` so the child's `current_dir()` and the
/// production `starts_with` prefix check agree.
fn worktree_fixture(tmp: &tempfile::TempDir) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = tmp.path().canonicalize().expect("canonicalize");
    let worktree_cwd = root.join(".worktrees").join("feat");
    std::fs::create_dir_all(&worktree_cwd).expect("mkdir worktree");
    (root, worktree_cwd)
}

#[test]
fn validate_subprocess_rejects_worktree_flow_states_write() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (root, worktree_cwd) = worktree_fixture(&tmp);
    let target = worktree_cwd.join(".flow-states/plan.md");
    let canonical = root.join(".flow-states/plan.md");
    let (code, _, stderr) = spawn_hook_with_cwd(
        &worktree_cwd,
        &root,
        "Write",
        "file_path",
        target.to_str().unwrap(),
    );
    assert_eq!(code, 2, "stderr: {}", stderr);
    assert!(stderr.contains("BLOCKED"), "stderr: {}", stderr);
    assert!(stderr.contains(".flow-states/"), "stderr: {}", stderr);
    assert!(
        stderr.contains(canonical.to_str().unwrap()),
        "stderr: {}",
        stderr
    );
}

#[test]
fn validate_subprocess_rejects_worktree_flow_states_read() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (root, worktree_cwd) = worktree_fixture(&tmp);
    let target = worktree_cwd.join(".flow-states/plan.md");
    let canonical = root.join(".flow-states/plan.md");
    let (code, _, stderr) = spawn_hook_with_cwd(
        &worktree_cwd,
        &root,
        "Read",
        "file_path",
        target.to_str().unwrap(),
    );
    assert_eq!(code, 2, "stderr: {}", stderr);
    assert!(stderr.contains("BLOCKED"), "stderr: {}", stderr);
    assert!(
        stderr.contains(canonical.to_str().unwrap()),
        "stderr: {}",
        stderr
    );
}

#[test]
fn validate_subprocess_rejects_worktree_flow_states_edit() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (root, worktree_cwd) = worktree_fixture(&tmp);
    let target = worktree_cwd.join(".flow-states/plan.md");
    let canonical = root.join(".flow-states/plan.md");
    let (code, _, stderr) = spawn_hook_with_cwd(
        &worktree_cwd,
        &root,
        "Edit",
        "file_path",
        target.to_str().unwrap(),
    );
    assert_eq!(code, 2, "stderr: {}", stderr);
    assert!(stderr.contains("BLOCKED"), "stderr: {}", stderr);
    assert!(
        stderr.contains(canonical.to_str().unwrap()),
        "stderr: {}",
        stderr
    );
}

#[test]
fn validate_subprocess_rejects_worktree_flow_states_glob() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (root, worktree_cwd) = worktree_fixture(&tmp);
    // Glob/Grep use the `path` field rather than `file_path`.
    let target = worktree_cwd.join(".flow-states/plan.md");
    let canonical = root.join(".flow-states/plan.md");
    let (code, _, stderr) = spawn_hook_with_cwd(
        &worktree_cwd,
        &root,
        "Glob",
        "path",
        target.to_str().unwrap(),
    );
    assert_eq!(code, 2, "stderr: {}", stderr);
    assert!(stderr.contains("BLOCKED"), "stderr: {}", stderr);
    assert!(
        stderr.contains(canonical.to_str().unwrap()),
        "stderr: {}",
        stderr
    );
}

#[test]
fn validate_subprocess_rejects_worktree_flow_states_grep() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (root, worktree_cwd) = worktree_fixture(&tmp);
    let target = worktree_cwd.join(".flow-states/plan.md");
    let canonical = root.join(".flow-states/plan.md");
    let (code, _, stderr) = spawn_hook_with_cwd(
        &worktree_cwd,
        &root,
        "Grep",
        "path",
        target.to_str().unwrap(),
    );
    assert_eq!(code, 2, "stderr: {}", stderr);
    assert!(stderr.contains("BLOCKED"), "stderr: {}", stderr);
    assert!(
        stderr.contains(canonical.to_str().unwrap()),
        "stderr: {}",
        stderr
    );
}

#[test]
fn validate_subprocess_accepts_main_repo_flow_states_write() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (root, worktree_cwd) = worktree_fixture(&tmp);
    let target = root.join(".flow-states/feat/plan.md");
    let (code, _, stderr) = spawn_hook_with_cwd(
        &worktree_cwd,
        &root,
        "Write",
        "file_path",
        target.to_str().unwrap(),
    );
    assert_eq!(code, 0, "stderr: {}", stderr);
}

#[test]
fn validate_subprocess_accepts_main_repo_flow_states_read() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (root, worktree_cwd) = worktree_fixture(&tmp);
    let target = root.join(".flow-states/feat/plan.md");
    let (code, _, stderr) = spawn_hook_with_cwd(
        &worktree_cwd,
        &root,
        "Read",
        "file_path",
        target.to_str().unwrap(),
    );
    assert_eq!(code, 0, "stderr: {}", stderr);
}

#[test]
fn validate_subprocess_accepts_main_repo_flow_states_edit() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (root, worktree_cwd) = worktree_fixture(&tmp);
    let target = root.join(".flow-states/feat/plan.md");
    let (code, _, stderr) = spawn_hook_with_cwd(
        &worktree_cwd,
        &root,
        "Edit",
        "file_path",
        target.to_str().unwrap(),
    );
    assert_eq!(code, 0, "stderr: {}", stderr);
}

#[test]
fn validate_subprocess_accepts_main_repo_flow_states_glob() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (root, worktree_cwd) = worktree_fixture(&tmp);
    let target = root.join(".flow-states/feat/plan.md");
    let (code, _, stderr) = spawn_hook_with_cwd(
        &worktree_cwd,
        &root,
        "Glob",
        "path",
        target.to_str().unwrap(),
    );
    assert_eq!(code, 0, "stderr: {}", stderr);
}

#[test]
fn validate_subprocess_accepts_main_repo_flow_states_grep() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (root, worktree_cwd) = worktree_fixture(&tmp);
    let target = root.join(".flow-states/feat/plan.md");
    let (code, _, stderr) = spawn_hook_with_cwd(
        &worktree_cwd,
        &root,
        "Grep",
        "path",
        target.to_str().unwrap(),
    );
    assert_eq!(code, 0, "stderr: {}", stderr);
}
