//! Integration tests for `src/hooks/validate_worktree_paths.rs`.

use std::io::Write;
use std::process::{Command, Stdio};

use flow_rs::hooks::validate_worktree_paths::{
    get_file_path, is_shared_config, run_impl_main, validate, validate_shared_config,
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

// --- run_impl_main decision core ---

#[test]
fn run_impl_main_no_hook_input_allows() {
    let (code, msg) = run_impl_main(None, Some("/project/.worktrees/feat".to_string()));
    assert_eq!(code, 0);
    assert!(msg.is_none());
}

#[test]
fn run_impl_main_empty_file_path_allows() {
    let input = json!({"tool_input": {}});
    let (code, msg) = run_impl_main(Some(input), Some("/project/.worktrees/feat".to_string()));
    assert_eq!(code, 0);
    assert!(msg.is_none());
}

#[test]
fn run_impl_main_no_cwd_allows() {
    let input = json!({"tool_input": {"file_path": "/some/path"}});
    let (code, msg) = run_impl_main(Some(input), None);
    assert_eq!(code, 0);
    assert!(msg.is_none());
}

#[test]
fn run_impl_main_blocks_main_repo_path_from_worktree() {
    let input = json!({
        "tool_input": {"file_path": "/project/lib/foo.py"}
    });
    let (code, msg) = run_impl_main(Some(input), Some("/project/.worktrees/feat".to_string()));
    assert_eq!(code, 2);
    assert!(msg.unwrap().contains("BLOCKED"));
}

#[test]
fn run_impl_main_blocks_shared_config_edit() {
    let input = json!({
        "tool_name": "Edit",
        "tool_input": {"file_path": "/project/.worktrees/feat/.gitignore"}
    });
    let (code, msg) = run_impl_main(Some(input), Some("/project/.worktrees/feat".to_string()));
    assert_eq!(code, 2);
    assert!(msg.unwrap().contains("shared configuration"));
}

#[test]
fn run_impl_main_allows_worktree_internal_edit() {
    let input = json!({
        "tool_name": "Edit",
        "tool_input": {"file_path": "/project/.worktrees/feat/src/lib.rs"}
    });
    let (code, msg) = run_impl_main(Some(input), Some("/project/.worktrees/feat".to_string()));
    assert_eq!(code, 0);
    assert!(msg.is_none());
}

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
