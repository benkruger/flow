//! Integration tests for `src/hooks/validate_pretool.rs`.

use std::io::Write;
use std::process::{Command, Stdio};

use flow_rs::hooks::validate_pretool::{should_block_background, validate, validate_agent};
use serde_json::{json, Value};

fn sample_settings() -> Value {
    json!({
        "permissions": {
            "allow": [
                "Bash(git status)",
                "Bash(git diff *)",
                "Bash(*bin/*)",
            ],
            "deny": []
        }
    })
}

fn deny_settings() -> Value {
    json!({
        "permissions": {
            "allow": ["Bash(git *)"],
            "deny": [
                "Bash(git rebase *)",
                "Bash(git push --force *)",
                "Bash(git push -f *)",
                "Bash(git reset --hard *)",
                "Bash(git stash *)",
                "Bash(git checkout *)",
                "Bash(git clean *)",
            ]
        }
    })
}

// --- Basic allow tests ---

#[test]
fn test_allows_bin_flow_ci() {
    let (allowed, msg) = validate("bin/flow ci", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_bin_ci() {
    let (allowed, msg) = validate("bin/ci", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_git_add() {
    let (allowed, msg) = validate("git add -A", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_git_diff() {
    let (allowed, msg) = validate("git diff HEAD", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_empty_command() {
    let (allowed, msg) = validate("", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

// --- Compound command blocking ---

#[test]
fn test_blocks_compound_and() {
    let (allowed, msg) = validate("cd .worktrees/test && git status", None, true);
    assert!(!allowed);
    assert!(msg.contains("Compound commands"));
    assert!(msg.contains("separate Bash calls"));
}

#[test]
fn test_blocks_compound_semicolon() {
    let (allowed, msg) = validate("bin/ci; echo done", None, true);
    assert!(!allowed);
    assert!(msg.contains("Compound commands"));
}

#[test]
fn test_blocks_pipe() {
    let (allowed, msg) = validate("git show HEAD:file.py | sed 's/foo/bar/'", None, true);
    assert!(!allowed);
    assert!(msg.contains("Compound commands"));
    assert!(msg.contains("separate Bash calls"));
}

#[test]
fn test_blocks_or_operator() {
    let (allowed, msg) = validate("bin/ci || echo failed", None, true);
    assert!(!allowed);
    assert!(msg.contains("Compound commands"));
}

// --- File-read command blocking ---

#[test]
fn test_blocks_cat() {
    let (allowed, msg) = validate("cat lib/foo.py", None, true);
    assert!(!allowed);
    assert!(msg.contains("Read"));
}

#[test]
fn test_blocks_grep() {
    let (allowed, msg) = validate("grep -r 'pattern' lib/", None, true);
    assert!(!allowed);
    assert!(msg.contains("Grep"));
}

#[test]
fn test_blocks_rg() {
    let (allowed, msg) = validate("rg 'pattern' lib/", None, true);
    assert!(!allowed);
    assert!(msg.contains("Grep"));
}

#[test]
fn test_blocks_find() {
    let (allowed, msg) = validate("find . -name '*.py'", None, true);
    assert!(!allowed);
    assert!(msg.contains("Glob"));
}

#[test]
fn test_blocks_ls() {
    let (allowed, msg) = validate("ls -la lib/", None, true);
    assert!(!allowed);
    assert!(msg.contains("Glob"));
}

#[test]
fn test_blocks_head() {
    let (allowed, msg) = validate("head -20 lib/foo.py", None, true);
    assert!(!allowed);
    assert!(msg.contains("Read"));
}

#[test]
fn test_blocks_tail() {
    let (allowed, msg) = validate("tail -f log.txt", None, true);
    assert!(!allowed);
    assert!(msg.contains("Read"));
}

// --- Exec prefix ---

#[test]
fn test_blocks_exec_prefix() {
    let (allowed, msg) = validate("exec /Users/ben/code/flow/bin/flow ci", None, true);
    assert!(!allowed);
    assert!(msg.contains("exec"));
    assert!(msg.contains("permission prompt"));
}

#[test]
fn test_blocks_exec_bare_command() {
    let (allowed, msg) = validate("exec bin/flow ci", None, true);
    assert!(!allowed);
    assert!(msg.contains("exec"));
}

#[test]
fn test_allows_command_without_exec() {
    let (allowed, msg) = validate("/Users/ben/code/flow/bin/flow ci", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

// --- Blanket restore ---

#[test]
fn test_blocks_git_restore_dot() {
    let (allowed, msg) = validate("git restore .", None, true);
    assert!(!allowed);
    assert!(msg.contains("git restore ."));
    assert!(msg.contains("individually"));
}

#[test]
fn test_allows_git_restore_specific_file() {
    let (allowed, msg) = validate("git restore lib/foo.py", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

// --- Git diff with file args ---

#[test]
fn test_blocks_git_diff_with_file_args() {
    let (allowed, msg) = validate("git diff origin/main..HEAD -- file.py", None, true);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("Read"));
}

#[test]
fn test_blocks_git_diff_head_with_file_args() {
    let (allowed, msg) = validate("git diff HEAD -- src/lib/foo.py", None, true);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_git_diff_cached_with_file_args() {
    let (allowed, msg) = validate("git diff --cached -- file.py", None, true);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_allows_git_diff_without_file_args() {
    let (allowed, msg) = validate("git diff origin/main..HEAD", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_git_diff_stat() {
    let (allowed, msg) = validate("git diff --stat", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

// --- Whitelist ---

#[test]
fn test_whitelist_allows_matching_command() {
    let s = sample_settings();
    let (allowed, msg) = validate("git status", Some(&s), true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_whitelist_allows_glob_match() {
    let s = sample_settings();
    let (allowed, msg) = validate("git diff HEAD", Some(&s), true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_whitelist_allows_bin_glob() {
    let s = sample_settings();
    let (allowed, _) = validate("bin/ci", Some(&s), true);
    assert!(allowed);
}

#[test]
fn test_whitelist_allows_leading_glob() {
    let s = sample_settings();
    let (allowed, _) = validate("/usr/local/bin/flow ci", Some(&s), true);
    assert!(allowed);
}

#[test]
fn test_whitelist_allows_chmod_absolute_path() {
    let s = json!({"permissions": {"allow": ["Bash(chmod +x *)"], "deny": []}});
    let (allowed, msg) = validate(
        "chmod +x /Users/ben/code/hh/.worktrees/feature/bin/qa",
        Some(&s),
        true,
    );
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_whitelist_blocks_unmatched_command() {
    let s = sample_settings();
    let (allowed, msg) = validate("curl http://example.com", Some(&s), true);
    assert!(!allowed);
    assert!(msg.contains("not in allow list"));
    assert!(msg.contains("curl http://example.com"));
}

#[test]
fn test_whitelist_blocks_rm_rf() {
    let s = sample_settings();
    let (allowed, msg) = validate("rm -rf /", Some(&s), true);
    assert!(!allowed);
    assert!(msg.contains("not in allow list"));
}

#[test]
fn test_whitelist_skipped_when_no_settings() {
    let (allowed, msg) = validate("curl http://example.com", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_whitelist_skipped_when_empty_allow() {
    let s = json!({"permissions": {"allow": []}});
    let (allowed, _) = validate("curl http://example.com", Some(&s), true);
    assert!(allowed);
}

// --- flow_active parameter ---

#[test]
fn test_flow_active_false_allows_unlisted_command() {
    let s = sample_settings();
    let (allowed, msg) = validate("npm test", Some(&s), false);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_flow_active_true_blocks_unlisted_command() {
    let s = sample_settings();
    let (allowed, msg) = validate("npm test", Some(&s), true);
    assert!(!allowed);
    assert!(msg.contains("not in allow list"));
}

#[test]
fn test_flow_active_false_still_blocks_compound() {
    let s = sample_settings();
    let (allowed, msg) = validate("git status && git diff", Some(&s), false);
    assert!(!allowed);
    assert!(msg.contains("Compound commands"));
}

#[test]
fn test_flow_active_false_still_blocks_file_read() {
    let s = sample_settings();
    let (allowed, msg) = validate("cat README.md", Some(&s), false);
    assert!(!allowed);
    assert!(msg.contains("Read"));
}

#[test]
fn test_flow_active_false_still_blocks_deny() {
    let s = deny_settings();
    let (allowed, msg) = validate("git rebase main", Some(&s), false);
    assert!(!allowed);
    assert!(msg.to_lowercase().contains("deny"));
}

#[test]
fn test_flow_active_false_still_blocks_redirect() {
    let s = sample_settings();
    let (allowed, msg) = validate("git log > /tmp/out.txt", Some(&s), false);
    assert!(!allowed);
    assert!(msg.to_lowercase().contains("redirection"));
}

#[test]
fn test_flow_active_default_blocks_unlisted() {
    let s = sample_settings();
    let (allowed, msg) = validate("npm test", Some(&s), true);
    assert!(!allowed);
    assert!(msg.contains("not in allow list"));
}

#[test]
fn test_compound_blocked_before_whitelist() {
    let s = sample_settings();
    let (allowed, msg) = validate("git status && git diff", Some(&s), true);
    assert!(!allowed);
    assert!(msg.contains("Compound commands"));
}

#[test]
fn test_file_read_blocked_before_whitelist() {
    let s = sample_settings();
    let (allowed, msg) = validate("cat README.md", Some(&s), true);
    assert!(!allowed);
    assert!(msg.contains("Read"));
}

// --- Deny list ---

#[test]
fn test_deny_blocks_matching_command() {
    let s = deny_settings();
    let (allowed, msg) = validate("git rebase main", Some(&s), true);
    assert!(!allowed);
    assert!(msg.to_lowercase().contains("deny"));
}

#[test]
fn test_deny_overrides_allow() {
    let s = deny_settings();
    let (allowed, msg) = validate("git checkout feature-branch", Some(&s), true);
    assert!(!allowed);
    assert!(msg.to_lowercase().contains("deny"));
}

#[test]
fn test_deny_blocks_force_push() {
    let s = deny_settings();
    let (allowed, msg) = validate("git push --force origin main", Some(&s), true);
    assert!(!allowed);
    assert!(msg.to_lowercase().contains("deny"));
}

#[test]
fn test_deny_blocks_hard_reset() {
    let s = deny_settings();
    let (allowed, msg) = validate("git reset --hard HEAD~1", Some(&s), true);
    assert!(!allowed);
    assert!(msg.to_lowercase().contains("deny"));
}

#[test]
fn test_deny_allows_non_matching_command() {
    let s = deny_settings();
    let (allowed, msg) = validate("git status", Some(&s), true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_deny_skipped_when_no_settings() {
    let (allowed, msg) = validate("git rebase main", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_deny_skipped_when_empty_deny() {
    let s = json!({"permissions": {"allow": ["Bash(git status)"], "deny": []}});
    let (allowed, msg) = validate("git status", Some(&s), true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_deny_skipped_when_no_deny_key() {
    let s = json!({"permissions": {"allow": ["Bash(git status)"]}});
    let (allowed, msg) = validate("git status", Some(&s), true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_deny_runs_before_allow() {
    let s = json!({
        "permissions": {
            "allow": ["Bash(git stash *)"],
            "deny": ["Bash(git stash *)"]
        }
    });
    let (allowed, msg) = validate("git stash save", Some(&s), true);
    assert!(!allowed);
    assert!(msg.to_lowercase().contains("deny"));
}

// --- Redirect blocking ---

#[test]
fn test_blocks_redirect_output() {
    let (allowed, msg) = validate("git show HEAD:file.py > /tmp/out.py", None, true);
    assert!(!allowed);
    assert!(msg.contains("Read tool"));
    assert!(msg.contains("Write tool"));
}

#[test]
fn test_blocks_redirect_append() {
    let (allowed, msg) = validate("git log >> /tmp/out.txt", None, true);
    assert!(!allowed);
    assert!(msg.to_lowercase().contains("redirection"));
}

#[test]
fn test_blocks_redirect_stderr() {
    let (allowed, msg) = validate("git status 2> /tmp/err.txt", None, true);
    assert!(!allowed);
    assert!(msg.to_lowercase().contains("redirection"));
}

#[test]
fn test_blocks_redirect_no_space() {
    let (allowed, msg) = validate("git show HEAD:file.py>/tmp/out.py", None, true);
    assert!(!allowed);
    assert!(msg.to_lowercase().contains("redirection"));
}

#[test]
fn test_allows_no_redirect() {
    let (allowed, msg) = validate("git diff --diff-filter=M", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_arrow_in_flag() {
    let (allowed, msg) = validate("git log --format=>%s", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

// --- run_in_background blocking ---

#[test]
fn test_blocks_background_bin_flow_ci_outside_flow() {
    let msg = should_block_background("bin/flow ci", false);
    assert!(msg.is_some());
    let text = msg.unwrap();
    assert!(text.contains("bin/flow"));
    assert!(text.contains("bin/ci"));
}

#[test]
fn test_blocks_background_bin_flow_ci_with_args_outside_flow() {
    let msg = should_block_background("bin/flow ci --retry 3", false);
    assert!(msg.is_some());
}

#[test]
fn test_blocks_background_bin_ci_outside_flow() {
    let msg = should_block_background("bin/ci", false);
    assert!(msg.is_some());
    assert!(msg.unwrap().contains("bin/ci"));
}

#[test]
fn test_blocks_background_absolute_bin_flow_ci_outside_flow() {
    let msg = should_block_background("/Users/ben/code/flow/bin/flow ci", false);
    assert!(msg.is_some());
}

#[test]
fn test_blocks_background_absolute_bin_ci_outside_flow() {
    let msg = should_block_background("/Users/ben/code/flow/bin/ci", false);
    assert!(msg.is_some());
}

#[test]
fn test_blocks_background_bin_flow_finalize_commit() {
    let msg = should_block_background("bin/flow finalize-commit .flow-commit-msg main", false);
    assert!(msg.is_some());
    assert!(msg.unwrap().contains("bin/flow"));
}

#[test]
fn test_blocks_background_bin_flow_phase_transition() {
    let msg = should_block_background("bin/flow phase-transition --action complete", false);
    assert!(msg.is_some());
}

#[test]
fn test_blocks_background_absolute_bin_flow_finalize_commit() {
    let msg = should_block_background(
        "/Users/ben/code/flow/bin/flow finalize-commit .flow-commit-msg main",
        false,
    );
    assert!(msg.is_some());
}

#[test]
fn test_blocks_background_bare_bin_flow() {
    let msg = should_block_background("bin/flow", false);
    assert!(msg.is_some());
}

#[test]
fn test_blocks_background_any_command_inside_flow() {
    let msg = should_block_background("echo hi", true);
    assert!(msg.is_some());
    assert!(msg.unwrap().contains("FLOW phase"));
}

#[test]
fn test_allows_background_non_flow_outside_flow() {
    let msg = should_block_background("echo hi", false);
    assert!(msg.is_none());
}

#[test]
fn test_does_not_false_positive_on_commands_containing_flow() {
    assert!(should_block_background("npm run ci", false).is_none());
    assert!(should_block_background("git commit", false).is_none());
    assert!(should_block_background("npm run flow", false).is_none());
}

#[test]
fn test_is_flow_command_empty_returns_false() {
    assert!(should_block_background("", false).is_none());
}

#[test]
fn test_is_flow_command_whitespace_only_returns_false() {
    assert!(should_block_background("   \t", false).is_none());
}

// --- is_bg_truthy: defensive JSON type handling (subprocess tests) ---
//
// `is_bg_truthy` is a private helper called inside `run()` against the
// `tool_input.run_in_background` field. We drive it by spawning the
// compiled binary and feeding JSON via stdin:
//   - When `is_bg_truthy` returns true → `should_block_background` runs
//     against `command = "bin/flow ci"` and the process exits 2 with a
//     block message on stderr.
//   - When `is_bg_truthy` returns false → the background path is skipped
//     and `validate("bin/flow ci", ...)` allows the command → exit 0.
// Command `bin/flow ci` is deliberately chosen: it's on FLOW's own
// whitelist (allowed by `validate`) AND it's a CI-tier command that
// `should_block_background` always blocks when `is_bg_truthy` is true
// (regardless of flow_active).

fn run_hook_with_bg(bg: Value) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["hook", "validate-pretool"])
        .env_remove("FLOW_CI_RUNNING")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn flow-rs");
    {
        let stdin = child.stdin.as_mut().unwrap();
        let input = json!({
            "tool_input": {
                "command": "bin/flow ci",
                "run_in_background": bg,
            }
        });
        stdin
            .write_all(serde_json::to_string(&input).unwrap().as_bytes())
            .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn is_bg_truthy_bool_true_blocks() {
    let (code, _stdout, stderr) = run_hook_with_bg(json!(true));
    assert_eq!(code, 2, "bool true should block; stderr={stderr}");
    assert!(stderr.contains("bin/flow"));
}

#[test]
fn is_bg_truthy_bool_false_allows() {
    let (code, _stdout, stderr) = run_hook_with_bg(json!(false));
    assert_eq!(code, 0, "bool false should allow; stderr={stderr}");
}

#[test]
fn is_bg_truthy_string_true_case_insensitive_blocks() {
    let (code, _, stderr) = run_hook_with_bg(json!("True"));
    assert_eq!(code, 2, "\"True\" should block; stderr={stderr}");
    let (code, _, stderr) = run_hook_with_bg(json!("TRUE"));
    assert_eq!(code, 2, "\"TRUE\" should block; stderr={stderr}");
}

#[test]
fn is_bg_truthy_string_one_blocks() {
    let (code, _, stderr) = run_hook_with_bg(json!("1"));
    assert_eq!(code, 2, "\"1\" should block; stderr={stderr}");
}

#[test]
fn is_bg_truthy_string_other_allows() {
    // Non-truthy strings: "false", "0", "yes", "", "foreground"
    for s in &["false", "0", "yes", "", "foreground"] {
        let (code, _, stderr) = run_hook_with_bg(json!(s));
        assert_eq!(
            code, 0,
            "string {s:?} should not block; got exit={code} stderr={stderr}"
        );
    }
}

#[test]
fn is_bg_truthy_integer_nonzero_blocks() {
    for n in &[1_i64, 42, -1] {
        let (code, _, stderr) = run_hook_with_bg(json!(n));
        assert_eq!(
            code, 2,
            "integer {n} should block; got exit={code} stderr={stderr}"
        );
    }
}

#[test]
fn is_bg_truthy_integer_zero_allows() {
    let (code, _, stderr) = run_hook_with_bg(json!(0_i64));
    assert_eq!(code, 0, "integer 0 should allow; stderr={stderr}");
}

#[test]
fn is_bg_truthy_f64_nonzero_blocks() {
    // serde_json::Number stores float literals as Float variant; as_i64
    // returns None so evaluation falls through to the as_f64 arm.
    let (code, _, stderr) = run_hook_with_bg(json!(1.5_f64));
    assert_eq!(code, 2, "f64 1.5 should block; stderr={stderr}");
}

#[test]
fn is_bg_truthy_f64_zero_allows() {
    let (code, _, stderr) = run_hook_with_bg(json!(0.0_f64));
    assert_eq!(code, 0, "f64 0.0 should allow; stderr={stderr}");
}

#[test]
fn is_bg_truthy_null_allows() {
    let (code, _, stderr) = run_hook_with_bg(Value::Null);
    assert_eq!(code, 0, "null should allow; stderr={stderr}");
}

#[test]
fn is_bg_truthy_array_allows() {
    let (code, _, stderr) = run_hook_with_bg(json!([true, 1]));
    assert_eq!(code, 0, "array should allow; stderr={stderr}");
}

#[test]
fn is_bg_truthy_object_allows() {
    let (code, _, stderr) = run_hook_with_bg(json!({"x": 1}));
    assert_eq!(code, 0, "object should allow; stderr={stderr}");
}

// --- run() branch coverage via subprocess ---
//
// Each test drives a distinct branch of `run()` that cannot be reached
// through the library surface: stdin parsing, settings/project-root
// discovery, Agent-tool dispatch, and the validate() exit-2 fall-through.

fn run_hook_with_input(input: &str, cwd: Option<&std::path::Path>) -> (i32, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.args(["hook", "validate-pretool"])
        .env_remove("FLOW_CI_RUNNING")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let mut child = cmd.spawn().expect("spawn flow-rs");
    {
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(input.as_bytes()).unwrap();
    }
    let output = child.wait_with_output().unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

/// Covers `None => exit(0)` in `match read_hook_input()` — non-JSON
/// stdin makes `read_hook_input` return None.
#[test]
fn run_rejects_malformed_stdin_and_exits_zero() {
    let (code, _, _) = run_hook_with_input("not valid json", None);
    assert_eq!(code, 0, "malformed stdin must exit 0");
}

/// Covers the `else { None }` branch of `branch = if settings.is_some()`
/// and the `_ => false` flow_active arm: running from a cwd with no
/// .claude/settings.json makes `find_settings_and_root` return
/// `(None, None)`, so settings.is_none() and the (&branch, &main_root)
/// match both take the wildcard arm.
#[test]
fn run_without_settings_falls_through_branch_and_main_root() {
    let dir = tempfile::tempdir().unwrap();
    let input = r#"{"tool_input": {"command": "git status"}}"#;
    let (code, _, _) = run_hook_with_input(input, Some(dir.path()));
    assert_eq!(code, 0, "allowed command with no settings must exit 0");
}

/// Covers the `should_block_background(...)` fall-through when the
/// command is NOT a flow command and flow_active is false:
/// is_bg_truthy=true, should_block_background returns None, so execution
/// falls past the background block and continues.
#[test]
fn run_with_bg_true_non_flow_command_falls_through() {
    let dir = tempfile::tempdir().unwrap();
    let input = r#"{"tool_input": {"command": "git status", "run_in_background": true}}"#;
    let (code, _, _) = run_hook_with_input(input, Some(dir.path()));
    assert_eq!(
        code, 0,
        "bg=true on non-flow command outside flow must fall through"
    );
}

/// Covers the Agent-tool allow path: empty command + !flow_active →
/// validate_agent returns (true, ""), so we hit `exit(0)` inside the
/// `if command.is_empty()` block.
#[test]
fn run_agent_path_allowed_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let input = r#"{"tool_input": {}}"#;
    let (code, _, _) = run_hook_with_input(input, Some(dir.path()));
    assert_eq!(code, 0, "empty command outside flow must exit 0");
}

/// Covers the validate()-rejected exit-2 path: a file-read command is
/// blocked regardless of flow-active state, so validate() returns
/// (false, msg) and run() eprintlns the message and exits 2.
#[test]
fn run_validate_rejection_exits_two() {
    let dir = tempfile::tempdir().unwrap();
    let input = r#"{"tool_input": {"command": "cat foo.py"}}"#;
    let (code, _, stderr) = run_hook_with_input(input, Some(dir.path()));
    assert_eq!(code, 2, "cat must be blocked; stderr={stderr}");
    assert!(stderr.contains("BLOCKED"));
}

/// Covers the Agent-tool block path (eprintln + exit 2) when
/// flow_active is true. Builds a fake worktree layout under a tempdir:
///   root/.claude/settings.json         — satisfies find_settings_and_root
///   root/.flow-states/<branch>.json    — makes is_flow_active return true
///   root/.worktrees/<branch>/.git      — makes detect_branch_from_path
///                                        identify the branch from cwd
/// Then spawns the hook with cwd=root/.worktrees/<branch>/ and a
/// general-purpose subagent payload, which must exit 2 with a BLOCKED
/// message.
#[test]
fn run_agent_path_blocked_exits_two_when_flow_active() {
    let root = tempfile::tempdir().unwrap();
    let root_path = root.path().canonicalize().unwrap();

    let claude_dir = root_path.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let flow_states = root_path.join(".flow-states");
    std::fs::create_dir_all(&flow_states).unwrap();
    std::fs::write(flow_states.join("feat.json"), "{}").unwrap();

    let worktree = root_path.join(".worktrees").join("feat");
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(worktree.join(".git"), "gitdir: ../../.git/worktrees/feat").unwrap();

    let input = r#"{"tool_input": {"subagent_type": "general-purpose"}}"#;
    let (code, _, stderr) = run_hook_with_input(input, Some(&worktree));
    assert_eq!(
        code, 2,
        "general-purpose agent during active flow must exit 2; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("general-purpose"));
}

// --- Agent validation ---

#[test]
fn test_validate_agent_blocks_general_purpose_when_flow_active() {
    let (allowed, msg) = validate_agent(Some("general-purpose"), true);
    assert!(!allowed);
    assert!(msg.contains("general-purpose"));
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_validate_agent_blocks_absent_type_when_flow_active() {
    let (allowed, msg) = validate_agent(None, true);
    assert!(!allowed);
    assert!(msg.contains("general-purpose"));
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_validate_agent_allows_flow_namespace_when_flow_active() {
    let (allowed, msg) = validate_agent(Some("flow:ci-fixer"), true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_validate_agent_allows_explore_when_flow_active() {
    let (allowed, msg) = validate_agent(Some("Explore"), true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_validate_agent_allows_plan_when_flow_active() {
    let (allowed, msg) = validate_agent(Some("Plan"), true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_validate_agent_allows_general_purpose_when_no_flow() {
    let (allowed, msg) = validate_agent(Some("general-purpose"), false);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_validate_agent_allows_absent_type_when_no_flow() {
    let (allowed, msg) = validate_agent(None, false);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_validate_agent_blocks_case_variants_when_flow_active() {
    let (allowed, _) = validate_agent(Some("General-Purpose"), true);
    assert!(!allowed);
    let (allowed, _) = validate_agent(Some("GENERAL-PURPOSE"), true);
    assert!(!allowed);
}

#[test]
fn test_validate_agent_blocks_empty_string_when_flow_active() {
    let (allowed, msg) = validate_agent(Some(""), true);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_validate_agent_blocks_whitespace_padded_when_flow_active() {
    let (allowed, _) = validate_agent(Some(" general-purpose "), true);
    assert!(!allowed);
}

// --- quote_aware_scan ---

#[test]
fn test_allows_pipe_in_single_quoted_arg() {
    let cmd = "bin/flow add-finding --reason 'describes | operator'";
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "pipe inside single quotes should be inert; got: {msg}"
    );
}

#[test]
fn test_allows_pipe_in_double_quoted_arg() {
    let cmd = "bin/flow add-finding --reason \"describes | operator\"";
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "pipe inside double quotes should be inert; got: {msg}"
    );
}

#[test]
fn test_allows_semicolon_in_single_quoted_arg() {
    let cmd = "bin/flow add-finding --reason 'a; b'";
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "semicolon inside single quotes should be inert; got: {msg}"
    );
}

#[test]
fn test_allows_semicolon_in_double_quoted_arg() {
    let cmd = "bin/flow add-finding --reason \"a; b\"";
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "semicolon inside double quotes should be inert; got: {msg}"
    );
}

#[test]
fn test_allows_ampersand_in_single_quoted_arg() {
    let cmd = "bin/flow add-finding --reason 'foo && bar'";
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "&& inside single quotes should be inert; got: {msg}"
    );
}

#[test]
fn test_allows_ampersand_in_double_quoted_arg() {
    let cmd = "bin/flow add-finding --reason \"foo && bar\"";
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "&& inside double quotes should be inert; got: {msg}"
    );
}

#[test]
fn test_allows_or_operator_in_quoted_arg() {
    let cmd = "bin/flow add-finding --reason 'a || b'";
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "|| inside single quotes should be inert; got: {msg}"
    );
}

#[test]
fn test_allows_redirect_char_in_single_quoted_arg() {
    let cmd = "bin/flow add-finding --reason 'a > b'";
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "> inside single quotes should be inert; got: {msg}"
    );
}

#[test]
fn test_allows_redirect_char_in_double_quoted_arg() {
    let cmd = "bin/flow add-finding --reason \"a > b\"";
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "> inside double quotes should be inert; got: {msg}"
    );
}

#[test]
fn test_still_blocks_unquoted_pipe() {
    let (allowed, msg) = validate("rg foo src | head", None, true);
    assert!(!allowed, "unquoted | must still be blocked");
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_still_blocks_unquoted_compound_and() {
    let (allowed, msg) = validate("cd foo && git status", None, true);
    assert!(!allowed, "unquoted && must still be blocked");
    assert!(msg.contains("Compound") || msg.contains("&&"));
}

#[test]
fn test_still_blocks_unquoted_semicolon() {
    let (allowed, msg) = validate("bin/ci; echo done", None, true);
    assert!(!allowed, "unquoted ; must still be blocked");
    assert!(msg.contains("Compound") || msg.contains(";"));
}

#[test]
fn test_still_blocks_unquoted_redirect() {
    let (allowed, msg) = validate("git log > /tmp/out", None, true);
    assert!(!allowed, "unquoted > must still be blocked");
    assert!(msg.to_lowercase().contains("redirection"));
}

#[test]
fn test_blocks_operator_after_closing_quote() {
    let (allowed, msg) = validate("echo 'foo' | grep bar", None, true);
    assert!(!allowed, "| after closed quote must be blocked");
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_unclosed_single_quote_with_operator() {
    let (allowed, msg) = validate("echo 'foo | bar", None, true);
    assert!(!allowed, "unclosed single quote must be blocked");
    assert!(
        msg.to_lowercase().contains("unclosed"),
        "error message should name the unclosed-quote case; got: {msg}"
    );
}

#[test]
fn test_blocks_unclosed_double_quote_with_operator() {
    let (allowed, msg) = validate("echo \"foo | bar", None, true);
    assert!(!allowed, "unclosed double quote must be blocked");
    assert!(
        msg.to_lowercase().contains("unclosed"),
        "error message should name the unclosed-quote case; got: {msg}"
    );
}

#[test]
fn test_allows_escaped_pipe_outside_quotes() {
    let (allowed, msg) = validate("echo foo\\|bar", None, true);
    assert!(allowed, "backslash-escaped | must be inert; got: {msg}");
}

#[test]
fn test_allows_mixed_quotes_with_operators() {
    let (allowed, msg) = validate("echo 'a|b' \"c;d\"", None, true);
    assert!(
        allowed,
        "mixed quotes with operators must be inert; got: {msg}"
    );
}

#[test]
fn test_blocks_dollar_paren_command_substitution() {
    let (allowed, msg) = validate("echo $(date)", None, true);
    assert!(!allowed, "unquoted $() must be blocked");
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_dollar_paren_inside_double_quoted_arg() {
    let (allowed, msg) = validate("echo \"the $(cmd) pattern\"", None, true);
    assert!(
        !allowed,
        "$() inside double quotes must be blocked — bash expands it; got: {msg}"
    );
}

#[test]
fn test_blocks_backtick_command_substitution() {
    let (allowed, msg) = validate("echo `date`", None, true);
    assert!(!allowed, "unquoted backtick must be blocked");
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_backtick_inside_double_quoted_arg() {
    let (allowed, msg) = validate("echo \"look: `date`\"", None, true);
    assert!(
        !allowed,
        "backtick inside double quotes must be blocked — bash expands it; got: {msg}"
    );
}

#[test]
fn test_allows_escaped_double_quote_inside_double_quoted_arg() {
    let cmd = r#"echo "hello \"world\"""#;
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "escaped double quote inside double-quoted arg must be literal; got: {msg}"
    );
}

#[test]
fn test_allows_escaped_redirect_inside_double_quoted_arg() {
    let cmd = r#"echo "result \> output""#;
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "escaped redirect char inside double-quoted arg must be literal; got: {msg}"
    );
}

#[test]
fn test_allows_dollar_paren_inside_single_quoted_arg() {
    let cmd = "echo 'literal $(cmd) text'";
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "$() inside single quotes must be inert; got: {msg}"
    );
}

#[test]
fn test_allows_backtick_inside_single_quoted_arg() {
    let (allowed, msg) = validate("echo 'look: `tick`'", None, true);
    assert!(
        allowed,
        "backtick inside single quotes must be inert; got: {msg}"
    );
}

#[test]
fn test_allows_quoted_arg_with_redirect_char_after_equals() {
    let (allowed, msg) = validate("git log --format=\"%s > %h\"", None, true);
    assert!(
        allowed,
        "> inside a double-quoted format string must be inert; got: {msg}"
    );
}

// --- adversarial_scan_gaps ---

#[test]
fn test_blocks_input_redirect() {
    let (allowed, msg) = validate("python3 < /etc/passwd", None, true);
    assert!(!allowed, "input redirect must be blocked");
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_here_string() {
    let (allowed, msg) = validate("python3 <<< 'code'", None, true);
    assert!(!allowed, "here-string must be blocked");
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_heredoc() {
    let (allowed, msg) = validate("python3 <<EOF\ncode\nEOF", None, true);
    assert!(!allowed, "heredoc must be blocked");
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_process_substitution_input() {
    let (allowed, msg) = validate("diff <(echo a) <(echo b)", None, true);
    assert!(!allowed, "input process substitution must be blocked");
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_trailing_ampersand_background() {
    let (allowed, msg) = validate("sleep 100 &", None, true);
    assert!(
        !allowed,
        "trailing & background operator must be blocked; got: {msg}"
    );
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_double_dash_redirect() {
    let (allowed, msg) = validate("echo foo-->/tmp/out", None, true);
    assert!(
        !allowed,
        "foo-->/tmp/out must be blocked — the dash carve-out was a bypass vector; got: {msg}"
    );
    assert!(msg.to_lowercase().contains("redirection"));
}

#[test]
fn test_allows_input_redirect_char_in_single_quoted_arg() {
    let (allowed, msg) = validate("echo 'hello <world>'", None, true);
    assert!(allowed, "< inside single quotes must be inert; got: {msg}");
}

#[test]
fn test_allows_input_redirect_char_in_double_quoted_arg() {
    let (allowed, msg) = validate("echo \"hello <world>\"", None, true);
    assert!(allowed, "< inside double quotes must be inert; got: {msg}");
}

#[test]
fn test_allows_ampersand_in_flag_name() {
    let (allowed, msg) = validate("mysql -u root -p'p&w0rd'", None, true);
    assert!(allowed, "& inside single quotes must be inert; got: {msg}");
}
