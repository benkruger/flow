//! Integration tests for `src/qa_reset.rs`.
//!
//! Covers the CLI wrapper surface and the production `default_runner`
//! that inline unit tests cannot reach. Inline tests in
//! `src/qa_reset.rs` drive `reset_git`, `close_prs`,
//! `delete_remote_branches`, `load_issue_template`, `reset_issues`,
//! `clean_local`, and `reset_impl` with injected runner closures.
//! This file covers `run()`'s process-exit paths and drives
//! `default_runner` against a real subprocess.

use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::process::Command;

use flow_rs::qa_reset::{
    self, clean_local, close_prs, delete_remote_branches, load_issue_template, reset_git,
    reset_issues, CmdResult,
};
use serde_json::json;

/// Subprocess: `bin/flow qa-reset --repo owner/repo --local-path
/// <nonexistent>` exercises `run()`'s `Ok(result)` arm when the
/// underlying `reset_git` fails — the result carries
/// `status=error` and `run()` calls `process::exit(1)`.
#[test]
fn qa_reset_cli_nonexistent_local_path_exits_nonzero_with_error_json() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let missing = root.join("not-a-repo");

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "qa-reset",
            "--repo",
            "owner/nonexistent",
            "--local-path",
            missing.to_str().unwrap(),
        ])
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("failed to spawn flow-rs");

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit 1 on missing local path, got {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":\"error\""),
        "expected status=error in stdout, got: {}",
        stdout
    );
}

/// Library-level: drives `qa_reset::default_runner` against a real
/// subprocess that succeeds. The production runner captures stdout,
/// stderr, and the exit status into a `CmdResult`; inline tests only
/// cover the mock-runner path, so this test ensures the real runner
/// invariant holds.
#[test]
fn qa_reset_default_runner_captures_stdout_on_success() {
    let result: CmdResult = qa_reset::default_runner(&["echo", "hello"], None);
    assert!(
        result.success,
        "expected success=true for `echo hello`, got stderr: {}",
        result.stderr
    );
    assert!(
        result.stdout.contains("hello"),
        "expected 'hello' in stdout, got: {}",
        result.stdout
    );
}

/// Library-level: drives `qa_reset::default_runner` against a command
/// that does not exist. The runner catches the spawn error and
/// returns `success=false` with the error message in `stderr`.
#[test]
fn qa_reset_default_runner_spawn_failure_returns_error_in_stderr() {
    let result: CmdResult =
        qa_reset::default_runner(&["definitely_not_a_real_command_for_qa_reset_test"], None);
    assert!(
        !result.success,
        "expected success=false for missing command, got stdout: {}",
        result.stdout
    );
    assert!(
        !result.stderr.is_empty(),
        "expected non-empty stderr for missing command, got empty"
    );
}

/// Library-level: drives `qa_reset::default_runner` with a command
/// that exits non-zero. The runner reports `success=false` and
/// preserves stdout/stderr captured from the child.
#[test]
fn qa_reset_default_runner_nonzero_exit_reports_failure() {
    // `false` is a POSIX command that exits 1 with no output.
    let result: CmdResult = qa_reset::default_runner(&["false"], None);
    assert!(
        !result.success,
        "expected success=false for `false`, got stdout: {}, stderr: {}",
        result.stdout, result.stderr
    );
}

/// Library-level: drives `qa_reset::default_runner` with an explicit
/// cwd so the `Some(dir)` branch of the internal cwd-setter fires.
/// Previous tests only hit the `None` branch.
#[test]
fn qa_reset_default_runner_with_cwd_runs_in_target_directory() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let marker_name = "qa_reset_cwd_marker";
    fs::write(root.join(marker_name), "hello").unwrap();

    let result: CmdResult = qa_reset::default_runner(&["ls"], Some(Path::new(&root)));
    assert!(
        result.success,
        "expected success=true for `ls` in tempdir, got stderr: {}",
        result.stderr
    );
    assert!(
        result.stdout.contains(marker_name),
        "expected marker file in ls output, got: {}",
        result.stdout
    );
}

// --- library-level tests (migrated from inline) ---

fn ok_result(stdout: &str) -> CmdResult {
    CmdResult {
        success: true,
        stdout: stdout.to_string(),
        stderr: String::new(),
    }
}

fn err_result(stderr: &str) -> CmdResult {
    CmdResult {
        success: false,
        stdout: String::new(),
        stderr: stderr.to_string(),
    }
}

/// Simple base64 encoder for test use only.
fn simple_base64_encode(input: &[u8]) -> String {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(alphabet[(n >> 18 & 63) as usize] as char);
        result.push(alphabet[(n >> 12 & 63) as usize] as char);
        if chunk.len() > 1 {
            result.push(alphabet[(n >> 6 & 63) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(alphabet[(n & 63) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

// --- reset_git ---

#[test]
fn test_reset_git_runs_correct_commands() {
    let cmds = RefCell::new(Vec::new());
    let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
        cmds.borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        ok_result("")
    };

    let result = reset_git(Path::new("/tmp/repo"), &runner);
    assert_eq!(result["status"], "ok");
    let captured = cmds.borrow();
    assert!(captured.iter().any(|c| c.contains(&"reset".to_string())));
    assert!(captured.iter().any(|c| c.contains(&"push".to_string())));
}

#[test]
fn test_reset_git_failure() {
    let runner =
        |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { err_result("fatal: not a repo") };
    let result = reset_git(Path::new("/tmp/repo"), &runner);
    assert_eq!(result["status"], "error");
}

// --- close_prs ---

#[test]
fn test_close_prs_closes_all_open() {
    let pr_list = serde_json::to_string(&json!([{"number": 1}, {"number": 2}])).unwrap();
    let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
        if args.contains(&"list") {
            ok_result(&pr_list)
        } else {
            ok_result("")
        }
    };
    let result = close_prs("owner/repo", &runner);
    assert_eq!(result, 2);
}

#[test]
fn test_close_prs_no_open() {
    let runner = |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { ok_result("[]") };
    let result = close_prs("owner/repo", &runner);
    assert_eq!(result, 0);
}

#[test]
fn test_close_prs_individual_close_failure_not_counted() {
    // Each close returns failure → the `if r.success` condition is
    // false, so `closed += 1` is skipped (exercises the else arm).
    let pr_list = serde_json::to_string(&json!([{"number": 1}])).unwrap();
    let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
        if args.contains(&"list") {
            ok_result(&pr_list)
        } else if args.contains(&"close") {
            err_result("close failed")
        } else {
            ok_result("")
        }
    };
    let result = close_prs("owner/repo", &runner);
    assert_eq!(result, 0);
}

#[test]
fn test_close_prs_skips_entries_missing_number_field() {
    // PR list entry without a "number" field — the `if let Some(num)`
    // check returns None so the close call is skipped.
    let pr_list = serde_json::to_string(&json!([{"title": "no number"}])).unwrap();
    let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
        if args.contains(&"list") {
            ok_result(&pr_list)
        } else {
            ok_result("")
        }
    };
    let result = close_prs("owner/repo", &runner);
    assert_eq!(result, 0);
}

#[test]
fn test_close_prs_gh_failure() {
    let runner = |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { err_result("error") };
    let result = close_prs("owner/repo", &runner);
    assert_eq!(result, 0);
}

// --- delete_remote_branches ---

#[test]
fn test_delete_remote_branches() {
    let branch_output = "  origin/main\n  origin/feature-1\n  origin/feature-2\n";
    let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
        if args.contains(&"-r") {
            ok_result(branch_output)
        } else {
            ok_result("")
        }
    };
    let result = delete_remote_branches("owner/repo", Path::new("/tmp/repo"), &runner);
    assert_eq!(result, 2);
}

#[test]
fn test_delete_remote_branches_only_main() {
    let runner =
        |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { ok_result("  origin/main\n") };
    let result = delete_remote_branches("owner/repo", Path::new("/tmp/repo"), &runner);
    assert_eq!(result, 0);
}

#[test]
fn test_delete_remote_branches_git_failure() {
    let runner = |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { err_result("error") };
    let result = delete_remote_branches("owner/repo", Path::new("/tmp/repo"), &runner);
    assert_eq!(result, 0);
}

#[test]
fn test_delete_remote_branches_individual_push_failure_not_counted() {
    // Every individual push returns failure → `deleted += 1` is
    // skipped. Returns 0 despite having branches to delete.
    let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
        if args.contains(&"-r") {
            ok_result("  origin/feature-1\n  origin/feature-2\n")
        } else {
            err_result("push failed")
        }
    };
    let result = delete_remote_branches("owner/repo", Path::new("/tmp/repo"), &runner);
    assert_eq!(result, 0);
}

#[test]
fn test_delete_remote_branches_branch_without_slash_falls_back_to_bare_name() {
    // `git branch -r` can emit entries without "origin/" prefix in
    // exotic configurations — the fallback path takes the bare name.
    let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
        if args.contains(&"-r") {
            ok_result("  oddbranch\n  origin/main\n")
        } else {
            ok_result("")
        }
    };
    let result = delete_remote_branches("owner/repo", Path::new("/tmp/repo"), &runner);
    assert_eq!(result, 1);
}

#[test]
fn test_delete_remote_branches_empty_line() {
    let branch_output = "  origin/main\n\n  origin/feature-1\n";
    let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
        if args.contains(&"-r") {
            ok_result(branch_output)
        } else {
            ok_result("")
        }
    };
    let result = delete_remote_branches("owner/repo", Path::new("/tmp/repo"), &runner);
    assert_eq!(result, 1);
}

// --- load_issue_template ---

#[test]
fn test_load_issue_template_success() {
    let content =
        serde_json::to_string(&json!([{"title": "Test", "body": "Body", "labels": []}])).unwrap();
    let encoded = simple_base64_encode(content.as_bytes());
    let runner = |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { ok_result(&encoded) };
    let result = load_issue_template("owner/repo", &runner);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["title"], "Test");
}

#[test]
fn test_load_issue_template_failure() {
    let runner = |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { err_result("not found") };
    let result = load_issue_template("owner/repo", &runner);
    assert!(result.is_empty());
}

#[test]
fn test_load_issue_template_corrupt() {
    let runner = |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { ok_result("not-base64!!!") };
    let result = load_issue_template("owner/repo", &runner);
    assert!(result.is_empty());
}

// --- reset_issues ---

#[test]
fn test_reset_issues_closes_and_recreates() {
    let issue_list = serde_json::to_string(&json!([{"number": 1}, {"number": 2}])).unwrap();
    let close_count = RefCell::new(0usize);
    let create_count = RefCell::new(0usize);

    let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
        if args.contains(&"list") {
            ok_result(&issue_list)
        } else if args.contains(&"close") {
            *close_count.borrow_mut() += 1;
            ok_result("")
        } else if args.contains(&"create") {
            *create_count.borrow_mut() += 1;
            ok_result("")
        } else {
            ok_result("")
        }
    };

    let template = vec![json!({"title": "New issue", "body": "Body", "labels": []})];
    let result = reset_issues("owner/repo", &template, &runner);
    assert_eq!(result, 1);
    assert_eq!(*close_count.borrow(), 2);
    assert_eq!(*create_count.borrow(), 1);
}

#[test]
fn test_reset_issues_individual_create_failure_not_counted() {
    // Runner returns failure for create calls → `created += 1` is
    // skipped. Returns 0 despite a template being provided.
    let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
        if args.contains(&"list") {
            ok_result("[]")
        } else if args.contains(&"create") {
            err_result("create failed")
        } else {
            ok_result("")
        }
    };
    let template = vec![json!({"title": "A", "body": "B", "labels": []})];
    let result = reset_issues("owner/repo", &template, &runner);
    assert_eq!(result, 0);
}

#[test]
fn test_reset_issues_skips_existing_without_number_field() {
    // An existing issue without a "number" field — the inner check
    // skips it. The branch is needed for coverage of the None arm
    // inside the close loop.
    let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
        if args.contains(&"list") {
            ok_result(&serde_json::to_string(&json!([{"title": "x"}])).unwrap())
        } else {
            ok_result("")
        }
    };
    let template: Vec<serde_json::Value> = vec![];
    let result = reset_issues("owner/repo", &template, &runner);
    assert_eq!(result, 0);
}

#[test]
fn test_reset_issues_invalid_json_skips_close_and_creates_only() {
    // Runner returns non-JSON → `if let Ok(issues)` takes Err arm;
    // the close loop is skipped. Template issues are still created.
    let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
        if args.contains(&"list") {
            ok_result("not valid json")
        } else {
            ok_result("")
        }
    };
    let template = vec![json!({"title": "A", "body": "B", "labels": []})];
    let result = reset_issues("owner/repo", &template, &runner);
    assert_eq!(result, 1);
}

#[test]
fn test_reset_issues_empty_stdout_skips_close() {
    // Runner succeeds with empty stdout → outer `!stdout.is_empty()`
    // guard takes the false arm and the close loop is skipped.
    let runner = |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { ok_result("") };
    let template = vec![json!({"title": "A", "body": "B", "labels": []})];
    let result = reset_issues("owner/repo", &template, &runner);
    assert_eq!(result, 1);
}

#[test]
fn test_reset_issues_with_labels() {
    let calls = RefCell::new(Vec::new());
    let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
        calls
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        if args.contains(&"list") {
            ok_result("[]")
        } else {
            ok_result("")
        }
    };

    let template = vec![json!({"title": "Bug", "body": "Fix it", "labels": ["bug", "urgent"]})];
    let result = reset_issues("owner/repo", &template, &runner);
    assert_eq!(result, 1);
    let captured = calls.borrow();
    let create_call = captured
        .iter()
        .find(|c| c.contains(&"create".to_string()))
        .unwrap();
    assert!(create_call.contains(&"--label".to_string()));
    assert!(create_call.contains(&"bug".to_string()));
    assert!(create_call.contains(&"urgent".to_string()));
}

// --- clean_local ---

#[test]
fn test_clean_local_removes_flow_artifacts() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".flow-states")).unwrap();
    fs::write(dir.path().join(".flow-states").join("test.json"), "{}").unwrap();
    fs::write(dir.path().join(".flow.json"), "{}").unwrap();
    fs::create_dir(dir.path().join(".claude")).unwrap();
    fs::write(dir.path().join(".claude").join("settings.json"), "{}").unwrap();

    clean_local(dir.path());

    assert!(!dir.path().join(".flow-states").exists());
    assert!(!dir.path().join(".flow.json").exists());
    assert!(!dir.path().join(".claude").exists());
}

#[test]
fn test_clean_local_missing_artifacts() {
    let dir = tempfile::tempdir().unwrap();
    clean_local(dir.path());
}

// --- reset_impl: exercised end-to-end via `bin/flow qa-reset` subprocess tests ---
//
// These tests spawn the compiled `flow-rs qa-reset` binary with a
// fake `gh`/`git` on PATH so the orchestration in `reset_impl`
// (local_path branching, early-error propagation, success aggregation)
// runs through the real production path. Library-level `reset_impl`
// is no longer `pub` per `.claude/rules/test-placement.md`; the
// runner-level tests above cover each helper's branches.

fn install_fake_bin(dir: &Path, name: &str, script: &str) -> std::path::PathBuf {
    let bin_dir = dir.join("fakebin");
    fs::create_dir_all(&bin_dir).unwrap();
    let bin = bin_dir.join(name);
    fs::write(&bin, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin_dir
}

/// Subprocess: `--local-path` provided, `reset_git`/`close_prs`/etc.
/// all succeed. reset_impl's aggregation returns ok.
#[test]
fn subprocess_reset_full_workflow_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    // Fake gh and git that always exit 0 with empty output.
    let bin_dir = install_fake_bin(&root, "gh", "#!/usr/bin/env bash\necho '[]'\nexit 0\n");
    install_fake_bin(&root, "git", "#!/usr/bin/env bash\nexit 0\n");

    // Create a local-path dir so reset_git can "operate".
    let local = root.join("local");
    fs::create_dir_all(&local).unwrap();

    let path_with_fake = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "qa-reset",
            "--repo",
            "owner/repo",
            "--local-path",
            local.to_str().unwrap(),
        ])
        .env_remove("FLOW_CI_RUNNING")
        .env("PATH", path_with_fake)
        .env("HOME", &root)
        .env("GH_TOKEN", "invalid")
        .output()
        .expect("spawn flow-rs qa-reset");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":\"ok\""),
        "expected ok status, got: {}",
        stdout
    );
}

/// Subprocess: no `--local-path` flag. reset_impl skips the
/// local-path-dependent steps.
#[test]
fn subprocess_reset_without_local_path_skips_git_ops() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let bin_dir = install_fake_bin(&root, "gh", "#!/usr/bin/env bash\necho '[]'\nexit 0\n");
    install_fake_bin(&root, "git", "#!/usr/bin/env bash\nexit 0\n");

    let path_with_fake = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["qa-reset", "--repo", "owner/repo"])
        .env_remove("FLOW_CI_RUNNING")
        .env("PATH", path_with_fake)
        .env("HOME", &root)
        .env("GH_TOKEN", "invalid")
        .output()
        .expect("spawn flow-rs qa-reset");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"branches_deleted\":0"));
}

/// Subprocess: `reset_git` fails — the early-error branch in
/// reset_impl returns the error and skips downstream ops.
#[test]
fn subprocess_reset_git_failure_early_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    // Fake git always exits 1.
    let bin_dir = install_fake_bin(&root, "git", "#!/usr/bin/env bash\nexit 1\n");
    install_fake_bin(&root, "gh", "#!/usr/bin/env bash\nexit 0\n");
    let local = root.join("local");
    fs::create_dir_all(&local).unwrap();

    let path_with_fake = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "qa-reset",
            "--repo",
            "owner/repo",
            "--local-path",
            local.to_str().unwrap(),
        ])
        .env_remove("FLOW_CI_RUNNING")
        .env("PATH", path_with_fake)
        .env("HOME", &root)
        .env("GH_TOKEN", "invalid")
        .output()
        .expect("spawn flow-rs qa-reset");
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":\"error\""),
        "expected error, got: {}",
        stdout
    );
}
