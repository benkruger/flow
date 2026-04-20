//! Integration tests for `src/scaffold_qa.rs`.
//!
//! Drives every branch of the public `find_templates`, `scaffold_impl`,
//! and `run_impl` surface through the library crate plus the compiled
//! binary. Inline `#[cfg(test)]` blocks are prohibited in `src/*.rs`
//! per `.claude/rules/test-placement.md` — this file owns every
//! behavior test for the module.

use std::cell::RefCell;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::json;

use flow_rs::qa_reset::CmdResult;
use flow_rs::scaffold_qa::{self, find_templates, scaffold_impl};

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

/// Resolve the qa/templates base directory from this repo's root.
fn templates_base() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("qa").join("templates")
}

// --- find_templates ---

#[test]
fn find_templates_rails_collects_expected_files() {
    let templates = find_templates(&templates_base().join("rails")).unwrap();
    assert!(templates.contains_key("Gemfile"));
    assert!(templates.contains_key("lib/calculator.rb"));
    assert!(templates.contains_key("test/calculator_test.rb"));
    assert!(templates.contains_key(".qa/issues.json"));
}

#[test]
fn find_templates_python_collects_expected_files() {
    let templates = find_templates(&templates_base().join("python")).unwrap();
    assert!(templates.contains_key("bin/dependencies"));
    assert!(templates.contains_key("src/calculator.py"));
    assert!(templates.contains_key("tests/test_calculator.py"));
    assert!(templates.contains_key(".qa/issues.json"));
}

#[test]
fn find_templates_ios_collects_expected_files() {
    let templates = find_templates(&templates_base().join("ios")).unwrap();
    assert!(templates.contains_key("FlowQA.xcodeproj/project.pbxproj"));
    assert!(templates.contains_key("FlowQA.xcodeproj/xcshareddata/xcschemes/FlowQA.xcscheme"));
    assert!(templates.contains_key("FlowQA/Calculator.swift"));
    assert!(templates.contains_key("FlowQA/FlowQAApp.swift"));
    assert!(templates.contains_key("FlowQATests/CalculatorTests.swift"));
    assert!(templates.contains_key(".qa/issues.json"));
}

#[test]
fn find_templates_preserves_file_content() {
    let templates = find_templates(&templates_base().join("rails")).unwrap();
    let actual = fs::read_to_string(templates_base().join("rails").join("Gemfile")).unwrap();
    assert_eq!(templates["Gemfile"], actual);
}

#[test]
fn find_templates_empty_dir_returns_empty_map() {
    let dir = tempfile::tempdir().unwrap();
    let templates = find_templates(dir.path()).unwrap();
    assert!(templates.is_empty());
}

/// Exercises the `read_dir` error propagation path. A subdir with no
/// permissions is statted successfully (is_dir holds) but its own
/// `read_dir` call returns EACCES. That forces the `?` in the stack
/// loop to propagate an `io::Error`.
#[test]
fn find_templates_unreadable_nested_dir_returns_io_error() {
    let dir = tempfile::tempdir().unwrap();
    let tpl = dir.path().join("tpl");
    fs::create_dir_all(&tpl).unwrap();
    fs::write(tpl.join("readme.md"), "content").unwrap();
    let locked = tpl.join("locked");
    fs::create_dir(&locked).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

    let result = find_templates(&tpl);
    let _ = fs::set_permissions(&locked, fs::Permissions::from_mode(0o755));

    // When the test runs as root (some CI containers), the chmod is
    // not enforced and read_dir succeeds. In that environment the test
    // is vacuous — assert only when the expected error occurs.
    if let Err(e) = result {
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied);
    }
}

/// Exercises the branch where a dir entry is neither a regular file
/// nor a directory — a broken symlink whose target does not exist
/// returns `false` from both `is_dir` and `is_file`, so the entry is
/// silently skipped. This covers the implicit-else fall-through in
/// `find_templates`' entry-classification chain.
#[test]
fn find_templates_broken_symlink_is_skipped() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let tpl = dir.path().join("tpl");
    fs::create_dir_all(&tpl).unwrap();
    fs::write(tpl.join("real.txt"), "content").unwrap();
    symlink(dir.path().join("nonexistent_target"), tpl.join("broken")).unwrap();

    let templates = find_templates(&tpl).unwrap();
    assert!(templates.contains_key("real.txt"));
    assert!(!templates.contains_key("broken"));
}

/// Exercises `read_to_string`'s Err path: a template file whose bytes
/// are not valid UTF-8. `find_templates` propagates the io::Error via
/// `?`, so this covers the final `?` branch in the stack loop.
#[test]
fn find_templates_non_utf8_file_returns_io_error() {
    let dir = tempfile::tempdir().unwrap();
    let tpl = dir.path().join("badutf");
    fs::create_dir_all(&tpl).unwrap();
    fs::write(tpl.join("bad.txt"), [0x80u8]).unwrap();

    let err = find_templates(&tpl).expect_err("expected io::Error on non-utf8 file");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

// --- scaffold_impl ---

#[test]
fn scaffold_impl_creates_repo_and_issues() {
    let dir = tempfile::tempdir().unwrap();
    let tpl_dir = dir.path().join("templates").join("rails");
    fs::create_dir_all(tpl_dir.join(".qa")).unwrap();
    fs::create_dir_all(tpl_dir.join("bin")).unwrap();
    fs::write(tpl_dir.join("Gemfile"), "source 'https://rubygems.org'\n").unwrap();
    fs::write(tpl_dir.join("bin/ci"), "#!/usr/bin/env ruby\nexit 0\n").unwrap();
    let issues = json!([
        {"title": "Issue 1", "body": "Body 1", "labels": []},
        {"title": "Issue 2", "body": "Body 2", "labels": ["bug"]}
    ]);
    fs::write(
        tpl_dir.join(".qa/issues.json"),
        serde_json::to_string(&issues).unwrap(),
    )
    .unwrap();

    let calls = RefCell::new(Vec::new());
    let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
        calls
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        ok_result("")
    };

    let clone_dir = dir.path().join("clone");
    let result = scaffold_impl(
        "rails",
        "owner/flow-qa-rails",
        &dir.path().join("templates"),
        &clone_dir,
        &runner,
    );

    assert_eq!(result["status"], "ok");
    assert_eq!(result["repo"], "owner/flow-qa-rails");
    assert_eq!(result["issues_created"], 2);

    let captured = calls.borrow();
    assert!(captured
        .iter()
        .any(|c| c.contains(&"repo".to_string()) && c.contains(&"create".to_string())));
    let issue_creates: Vec<_> = captured
        .iter()
        .filter(|c| c.contains(&"issue".to_string()) && c.contains(&"create".to_string()))
        .collect();
    assert_eq!(issue_creates.len(), 2);
}

#[test]
fn scaffold_impl_writes_template_files() {
    let dir = tempfile::tempdir().unwrap();
    let tpl_dir = dir.path().join("templates").join("rails");
    fs::create_dir_all(tpl_dir.join("bin")).unwrap();
    fs::create_dir_all(tpl_dir.join(".qa")).unwrap();
    fs::write(tpl_dir.join("Gemfile"), "gem content\n").unwrap();
    fs::write(tpl_dir.join("bin/ci"), "#!/usr/bin/env ruby\n").unwrap();
    fs::write(tpl_dir.join(".qa/issues.json"), "[]").unwrap();

    let runner = |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { ok_result("") };

    let clone_dir = dir.path().join("clone");
    scaffold_impl(
        "rails",
        "owner/repo",
        &dir.path().join("templates"),
        &clone_dir,
        &runner,
    );

    assert_eq!(
        fs::read_to_string(clone_dir.join("Gemfile")).unwrap(),
        "gem content\n"
    );
    assert_eq!(
        fs::read_to_string(clone_dir.join("bin/ci")).unwrap(),
        "#!/usr/bin/env ruby\n"
    );
}

#[test]
fn scaffold_impl_unknown_template_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("templates")).unwrap();
    let runner = |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { ok_result("") };

    let result = scaffold_impl(
        "nonexistent",
        "owner/repo",
        &dir.path().join("templates"),
        &dir.path().join("clone"),
        &runner,
    );
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("Unknown template"));
}

/// A template directory that contains a non-UTF8 file causes
/// find_templates to propagate an io::Error, which scaffold_impl
/// renders as a status "error" message.
#[test]
fn scaffold_impl_find_templates_io_error_surfaced_as_status_error() {
    let dir = tempfile::tempdir().unwrap();
    let tpl_dir = dir.path().join("templates").join("badutf");
    fs::create_dir_all(&tpl_dir).unwrap();
    fs::write(tpl_dir.join("bad.txt"), [0x80u8]).unwrap();

    let runner = |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { ok_result("") };
    let result = scaffold_impl(
        "badutf",
        "owner/repo",
        &dir.path().join("templates"),
        &dir.path().join("clone"),
        &runner,
    );
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("Failed to read templates"));
}

#[test]
fn scaffold_impl_gh_create_failure_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let tpl_dir = dir.path().join("templates").join("rails");
    fs::create_dir_all(tpl_dir.join(".qa")).unwrap();
    fs::write(tpl_dir.join(".qa/issues.json"), "[]").unwrap();

    let runner =
        |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { err_result("already exists") };

    let result = scaffold_impl(
        "rails",
        "owner/repo",
        &dir.path().join("templates"),
        &dir.path().join("clone"),
        &runner,
    );

    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("gh repo create failed"));
}

#[test]
fn scaffold_impl_git_command_failure_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let tpl_dir = dir.path().join("templates").join("rails");
    fs::create_dir_all(tpl_dir.join(".qa")).unwrap();
    fs::write(tpl_dir.join(".qa/issues.json"), "[]").unwrap();

    let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
        if args[0] == "gh" {
            ok_result("")
        } else {
            err_result("git init failed")
        }
    };

    let clone_dir = dir.path().join("clone");
    let result = scaffold_impl(
        "rails",
        "owner/repo",
        &dir.path().join("templates"),
        &clone_dir,
        &runner,
    );

    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("failed"));
}

#[test]
fn scaffold_impl_sets_bin_scripts_executable() {
    let dir = tempfile::tempdir().unwrap();
    let tpl_dir = dir.path().join("templates").join("ios");
    fs::create_dir_all(tpl_dir.join("bin")).unwrap();
    fs::create_dir_all(tpl_dir.join(".qa")).unwrap();
    fs::write(tpl_dir.join("bin/ci"), "#!/usr/bin/env bash\n").unwrap();
    fs::write(tpl_dir.join("bin/test"), "#!/usr/bin/env bash\n").unwrap();
    fs::write(tpl_dir.join("bin/build"), "#!/usr/bin/env bash\n").unwrap();
    fs::write(tpl_dir.join(".qa/issues.json"), "[]").unwrap();

    let runner = |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { ok_result("") };

    let clone_dir = dir.path().join("clone");
    scaffold_impl(
        "ios",
        "owner/repo",
        &dir.path().join("templates"),
        &clone_dir,
        &runner,
    );

    for script in &["ci", "test", "build"] {
        let path = clone_dir.join("bin").join(script);
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert!(mode & 0o111 != 0, "bin/{} not executable", script);
    }
}

/// Exercises the branch where the caller-provided clone_dir already
/// exists: `create_dir_all` succeeds as a noop (idempotent) and the
/// pre-existing contents are preserved.
#[test]
fn scaffold_impl_clone_dir_already_exists_reuses_it() {
    let dir = tempfile::tempdir().unwrap();
    let tpl_dir = dir.path().join("templates").join("rails");
    fs::create_dir_all(tpl_dir.join(".qa")).unwrap();
    fs::write(tpl_dir.join("Gemfile"), "gem\n").unwrap();
    fs::write(tpl_dir.join(".qa/issues.json"), "[]").unwrap();

    let clone_dir = dir.path().join("clone");
    fs::create_dir_all(&clone_dir).unwrap();
    fs::write(clone_dir.join("marker"), "preserved").unwrap();

    let runner = |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { ok_result("") };
    let result = scaffold_impl(
        "rails",
        "owner/repo",
        &dir.path().join("templates"),
        &clone_dir,
        &runner,
    );
    assert_eq!(result["status"], "ok");
    assert_eq!(
        fs::read_to_string(clone_dir.join("marker")).unwrap(),
        "preserved"
    );
    assert!(clone_dir.join("Gemfile").exists());
}

/// Exercises the issue-create failure branch: gh and git succeed, but
/// the gh issue-create runner returns failure. `issues_created` must
/// stay zero and the overall status must remain "ok" (issue failures
/// do not abort the scaffold).
#[test]
fn scaffold_impl_issue_create_failure_leaves_issues_created_zero() {
    let dir = tempfile::tempdir().unwrap();
    let tpl_dir = dir.path().join("templates").join("rails");
    fs::create_dir_all(tpl_dir.join(".qa")).unwrap();
    let issues = json!([
        {"title": "Bad", "body": "Body", "labels": ["bug"]}
    ]);
    fs::write(
        tpl_dir.join(".qa/issues.json"),
        serde_json::to_string(&issues).unwrap(),
    )
    .unwrap();

    let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
        if args.len() >= 2 && args[0] == "gh" && args[1] == "issue" {
            err_result("rate limited")
        } else {
            ok_result("")
        }
    };

    let clone_dir = dir.path().join("clone");
    let result = scaffold_impl(
        "rails",
        "owner/repo",
        &dir.path().join("templates"),
        &clone_dir,
        &runner,
    );

    assert_eq!(result["status"], "ok");
    assert_eq!(result["issues_created"], 0);
}

/// Exercises the clone_dir create-dir-all failure branch: the caller
/// passes a path whose ancestor is actually an existing regular file,
/// so `create_dir_all` fails with ENOTDIR.
#[test]
fn scaffold_impl_clone_dir_create_failure_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let tpl_dir = dir.path().join("templates").join("rails");
    fs::create_dir_all(tpl_dir.join(".qa")).unwrap();
    fs::write(tpl_dir.join(".qa/issues.json"), "[]").unwrap();

    let blocker = dir.path().join("blocker");
    fs::write(&blocker, "").unwrap();
    let clone_dir = blocker.join("nested");

    let runner = |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { ok_result("") };
    let result = scaffold_impl(
        "rails",
        "owner/repo",
        &dir.path().join("templates"),
        &clone_dir,
        &runner,
    );
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("Failed to create clone dir"));
}

/// Exercises the `fs::write` failure branch inside the template-write
/// loop: a pre-seeded clone_dir with a regular file where a nested
/// subdir is expected forces the write to fail with ENOTDIR.
#[test]
fn scaffold_impl_template_write_failure_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let tpl_dir = dir.path().join("templates").join("rails");
    fs::create_dir_all(tpl_dir.join(".qa")).unwrap();
    fs::create_dir_all(tpl_dir.join("sub")).unwrap();
    fs::write(tpl_dir.join("sub/foo"), "content\n").unwrap();
    fs::write(tpl_dir.join(".qa/issues.json"), "[]").unwrap();

    let clone_dir = dir.path().join("clone");
    fs::create_dir_all(&clone_dir).unwrap();
    fs::write(clone_dir.join("sub"), "blocker").unwrap();

    let runner = |_args: &[&str], _cwd: Option<&Path>| -> CmdResult { ok_result("") };
    let result = scaffold_impl(
        "rails",
        "owner/repo",
        &dir.path().join("templates"),
        &clone_dir,
        &runner,
    );
    assert_eq!(result["status"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("Failed to write"));
}

/// Exercises the issue-create path with a non-string label entry. The
/// scaffold must skip the non-string and still issue the --label arg
/// for the valid entry. Also covers the `None`-labels (missing key)
/// branch on another issue in the same fixture.
#[test]
fn scaffold_impl_issue_labels_handle_missing_key_and_non_string_entries() {
    let dir = tempfile::tempdir().unwrap();
    let tpl_dir = dir.path().join("templates").join("rails");
    fs::create_dir_all(tpl_dir.join(".qa")).unwrap();
    let issues = json!([
        {"title": "Plain", "body": "body"},
        {"title": "Typed", "body": "body", "labels": [42, "valid"]}
    ]);
    fs::write(
        tpl_dir.join(".qa/issues.json"),
        serde_json::to_string(&issues).unwrap(),
    )
    .unwrap();

    let captured_cmds: RefCell<Vec<Vec<String>>> = RefCell::new(Vec::new());
    let runner = |args: &[&str], _cwd: Option<&Path>| -> CmdResult {
        if args.len() >= 2 && args[0] == "gh" && args[1] == "issue" {
            captured_cmds
                .borrow_mut()
                .push(args.iter().map(|s| s.to_string()).collect());
        }
        ok_result("")
    };

    let clone_dir = dir.path().join("clone");
    let result = scaffold_impl(
        "rails",
        "owner/repo",
        &dir.path().join("templates"),
        &clone_dir,
        &runner,
    );
    assert_eq!(result["status"], "ok");
    assert_eq!(result["issues_created"], 2);

    let cmds = captured_cmds.borrow();
    assert!(!cmds[0].iter().any(|s| s == "--label"));
    let label_count = cmds[1].iter().filter(|s| *s == "--label").count();
    assert_eq!(label_count, 1);
    assert!(cmds[1].iter().any(|s| s == "valid"));
}

// --- run_impl / CLI ---

/// Subprocess invocation of `bin/flow scaffold-qa` with an unknown
/// template name. Exercises `run()`'s `Ok(result)` arm when the result
/// carries `status == "error"` — the path that prints the JSON and
/// calls `process::exit(1)`. Transitively exercises
/// `default_templates_base()` and `default_clone_dir()` in the
/// production binary (they resolve qa/templates and a fresh temp dir).
#[test]
fn scaffold_qa_cli_unknown_template_exits_nonzero_with_error_json() {
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "scaffold-qa",
            "--template",
            "definitely_not_a_real_template_name",
            "--repo",
            "owner/nonexistent",
        ])
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("failed to spawn flow-rs");

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit 1 on unknown template, got {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":\"error\""),
        "expected error status in stdout, got: {}",
        stdout
    );
    assert!(
        stdout.contains("Unknown template"),
        "expected 'Unknown template' in stdout, got: {}",
        stdout
    );
}

/// Directly drives `run_impl` with an unknown template. Exercises the
/// `Ok(scaffold_impl(...))` wrapper inside `run_impl`, the
/// `default_templates_base()` resolution, `default_clone_dir()`, and
/// the early-return where `scaffold_impl` emits the "Unknown template"
/// error as JSON.
#[test]
fn scaffold_qa_run_impl_unknown_template_returns_error_status() {
    let args = scaffold_qa::Args {
        template: "nonexistent_scaffold_qa_template_for_test".to_string(),
        repo: "owner/repo".to_string(),
    };

    let result = scaffold_qa::run_impl(&args).expect("run_impl returns Ok wrapping scaffold_impl");
    assert_eq!(
        result["status"], "error",
        "expected status=error on unknown template"
    );
    let message = result["message"]
        .as_str()
        .expect("error response carries message");
    assert!(
        message.contains("Unknown template"),
        "expected 'Unknown template' in message, got: {}",
        message
    );
}
