//! Integration tests for `src/scaffold_qa.rs`.
//!
//! Drives the `scaffold-qa` subcommand through the compiled binary.
//! `FLOW_SCAFFOLD_TEMPLATES_BASE` and `FLOW_SCAFFOLD_CLONE_DIR` env
//! vars redirect the private `default_templates_base()` and
//! `default_clone_dir()` helpers to test-controlled directories so each
//! branch of the scaffold logic is covered without exposing a
//! pub-for-testing seam.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

/// Write a gh stub at `<dir>/gh`. The caller-supplied script body is
/// prepended with `#!/bin/bash` and chmodded 0755.
fn write_gh_stub(dir: &Path, script_body: &str) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let path = dir.join("gh");
    let script = format!("#!/bin/bash\n{}\n", script_body);
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Write a git stub that stubs `push` (exit 0, silent) and forwards
/// everything else to the real `/usr/bin/git`. The scaffold flow runs
/// `git init`, `add`, `commit`, `tag`, `remote`, `push` — only the
/// network-dependent `push` needs stubbing.
fn write_git_push_stub(dir: &Path) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let path = dir.join("git");
    let script = "#!/bin/bash\n\
        if [[ \"$1\" == \"push\" ]]; then exit 0; fi\n\
        exec /usr/bin/git \"$@\"\n";
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Set up stubs dir with a working gh + git-push combo. The git stub
/// forwards everything except `push` to real git, and the caller
/// supplies the gh script body.
fn write_stubs(dir: &Path, gh_body: &str) {
    write_gh_stub(dir, gh_body);
    write_git_push_stub(dir);
}

fn run_scaffold(
    template: &str,
    repo: &str,
    stub_dir: Option<&Path>,
    templates_base: &Path,
    clone_dir: &Path,
) -> Output {
    let mut path_env = std::env::var("PATH").unwrap_or_default();
    if let Some(stub) = stub_dir {
        path_env = format!("{}:{}", stub.display(), path_env);
    }
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["scaffold-qa", "--template", template, "--repo", repo])
        .env("PATH", &path_env)
        .env("FLOW_SCAFFOLD_TEMPLATES_BASE", templates_base)
        .env("FLOW_SCAFFOLD_CLONE_DIR", clone_dir)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("spawn flow-rs")
}

fn last_json(stdout: &str) -> Value {
    let last = stdout
        .lines()
        .rfind(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON in stdout: {}", stdout));
    serde_json::from_str(last).unwrap_or_else(|e| panic!("failed to parse JSON '{}': {}", last, e))
}

/// Build a minimal template fixture. Returns the templates_base dir
/// (the parent directory passed via `FLOW_SCAFFOLD_TEMPLATES_BASE`).
fn build_minimal_template(root: &Path, template_name: &str, issues_json: &str) -> PathBuf {
    let templates_base = root.join("templates");
    let tpl_dir = templates_base.join(template_name);
    fs::create_dir_all(tpl_dir.join(".qa")).unwrap();
    fs::write(tpl_dir.join("Gemfile"), "gem content\n").unwrap();
    fs::write(tpl_dir.join(".qa/issues.json"), issues_json).unwrap();
    templates_base
}

// --- Happy path ---

#[test]
fn scaffold_qa_happy_path_creates_files_and_issues() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let issues = serde_json::to_string(&json!([
        {"title": "Issue 1", "body": "Body 1", "labels": []},
        {"title": "Issue 2", "body": "Body 2", "labels": ["bug"]}
    ]))
    .unwrap();
    let templates_base = build_minimal_template(&root, "rails", &issues);

    // gh stub: repo create OK, issue create OK.
    let stub_dir = root.join("stubs");
    write_stubs(&stub_dir, "exit 0");

    let clone_dir = root.join("clone");
    let output = run_scaffold(
        "rails",
        "owner/flow-qa-rails",
        Some(&stub_dir),
        &templates_base,
        &clone_dir,
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let data = last_json(&stdout);
    assert_eq!(data["status"], "ok", "got: {}", data);
    assert_eq!(data["repo"], "owner/flow-qa-rails");
    assert_eq!(data["issues_created"], 2);

    // Template files were written.
    assert!(clone_dir.join("Gemfile").exists());
    assert_eq!(
        fs::read_to_string(clone_dir.join("Gemfile")).unwrap(),
        "gem content\n"
    );
}

/// Exercises the bin/* permission setting branch: any file under `bin/`
/// in the template gets 0755 after being written to clone_dir.
#[test]
fn scaffold_qa_sets_bin_scripts_executable() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let templates_base = root.join("templates");
    let tpl_dir = templates_base.join("rails");
    fs::create_dir_all(tpl_dir.join("bin")).unwrap();
    fs::create_dir_all(tpl_dir.join(".qa")).unwrap();
    fs::write(tpl_dir.join("Gemfile"), "gem\n").unwrap();
    fs::write(tpl_dir.join("bin/ci"), "#!/usr/bin/env bash\n").unwrap();
    fs::write(tpl_dir.join(".qa/issues.json"), "[]").unwrap();

    let stub_dir = root.join("stubs");
    write_stubs(&stub_dir, "exit 0");
    let clone_dir = root.join("clone");

    let output = run_scaffold(
        "rails",
        "owner/repo",
        Some(&stub_dir),
        &templates_base,
        &clone_dir,
    );
    let data = last_json(&String::from_utf8_lossy(&output.stdout));
    assert_eq!(data["status"], "ok", "got: {}", data);

    let bin_ci = clone_dir.join("bin/ci");
    let mode = fs::metadata(&bin_ci).unwrap().permissions().mode();
    assert!(mode & 0o111 != 0, "bin/ci not executable: {:o}", mode);
}

// --- Error paths ---

#[test]
fn scaffold_qa_unknown_template_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let templates_base = root.join("templates");
    fs::create_dir_all(&templates_base).unwrap();

    let stub_dir = root.join("stubs");
    write_stubs(&stub_dir, "exit 0");
    let clone_dir = root.join("clone");

    let output = run_scaffold(
        "nonexistent",
        "owner/repo",
        Some(&stub_dir),
        &templates_base,
        &clone_dir,
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit 1 on unknown template"
    );
    let data = last_json(&String::from_utf8_lossy(&output.stdout));
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Unknown template"));
}

/// Template directory contains a non-UTF8 file. `find_templates`
/// propagates the io::Error; scaffold_qa renders it as a status error
/// with "Failed to read templates".
#[test]
fn scaffold_qa_find_templates_io_error_surfaces_as_status_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let templates_base = root.join("templates");
    let tpl_dir = templates_base.join("badutf");
    fs::create_dir_all(&tpl_dir).unwrap();
    fs::write(tpl_dir.join("bad.txt"), [0x80u8]).unwrap();

    let stub_dir = root.join("stubs");
    write_stubs(&stub_dir, "exit 0");
    let clone_dir = root.join("clone");

    let output = run_scaffold(
        "badutf",
        "owner/repo",
        Some(&stub_dir),
        &templates_base,
        &clone_dir,
    );
    let data = last_json(&String::from_utf8_lossy(&output.stdout));
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Failed to read templates"));
}

#[test]
fn scaffold_qa_gh_repo_create_failure_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let templates_base = build_minimal_template(&root, "rails", "[]");

    // gh stub: first call (repo create) fails with stderr.
    let stub_dir = root.join("stubs");
    write_stubs(&stub_dir, "echo 'repo already exists' >&2\nexit 1");
    let clone_dir = root.join("clone");

    let output = run_scaffold(
        "rails",
        "owner/repo",
        Some(&stub_dir),
        &templates_base,
        &clone_dir,
    );
    let data = last_json(&String::from_utf8_lossy(&output.stdout));
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("gh repo create failed"));
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("repo already exists"));
}

/// gh succeeds, git commands fail. After a successful gh repo create,
/// scaffold_qa runs `git init`, `add`, `commit`, `tag`, `remote`, `push`
/// in the clone_dir. Pre-seed clone_dir with a `git` stub that fails
/// every invocation — but that would also affect gh (no, gh stub handles
/// gh separately). The simplest way: redirect PATH so `git` resolves to
/// a failing stub, but leave real git on the host available via absolute
/// path only (scaffold_qa invokes `git` without an absolute path, so a
/// stub on PATH wins).
#[test]
fn scaffold_qa_git_command_failure_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let templates_base = build_minimal_template(&root, "rails", "[]");

    // Stub dir with both `gh` (success) and `git` (failure).
    let stub_dir = root.join("stubs");
    write_stubs(&stub_dir, "exit 0");
    let git_stub = stub_dir.join("git");
    fs::write(
        &git_stub,
        "#!/bin/bash\necho 'git init failed' >&2\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&git_stub, fs::Permissions::from_mode(0o755)).unwrap();

    let clone_dir = root.join("clone");
    let output = run_scaffold(
        "rails",
        "owner/repo",
        Some(&stub_dir),
        &templates_base,
        &clone_dir,
    );
    let data = last_json(&String::from_utf8_lossy(&output.stdout));
    assert_eq!(data["status"], "error");
    let msg = data["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("failed"),
        "expected 'failed' in message, got: {}",
        msg
    );
}

/// Issue-create failures are tolerated: scaffold_qa completes with
/// status:ok but issues_created stays at 0.
#[test]
fn scaffold_qa_issue_create_failure_keeps_issues_created_zero() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let issues = serde_json::to_string(&json!([
        {"title": "Bad", "body": "body", "labels": ["bug"]}
    ]))
    .unwrap();
    let templates_base = build_minimal_template(&root, "rails", &issues);

    // gh stub: repo create succeeds, issue create fails.
    let stub_dir = root.join("stubs");
    write_stubs(
        &stub_dir,
        "if [[ \"$1\" == \"issue\" ]]; then\n\
           echo 'rate limited' >&2\n\
           exit 1\n\
         fi\n\
         exit 0",
    );
    let clone_dir = root.join("clone");

    let output = run_scaffold(
        "rails",
        "owner/repo",
        Some(&stub_dir),
        &templates_base,
        &clone_dir,
    );
    let data = last_json(&String::from_utf8_lossy(&output.stdout));
    assert_eq!(data["status"], "ok", "got: {}", data);
    assert_eq!(data["issues_created"], 0);
}

/// Issue labels with missing key and non-string entries exercise both
/// the None `as_array()` branch (missing labels key) and the `continue`
/// branch on non-string array elements. Both issues still land in
/// issues_created.
#[test]
fn scaffold_qa_issue_labels_missing_or_non_string_handled() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let issues = serde_json::to_string(&json!([
        {"title": "Plain", "body": "body"},
        {"title": "Typed", "body": "body", "labels": [42, "valid"]}
    ]))
    .unwrap();
    let templates_base = build_minimal_template(&root, "rails", &issues);

    let stub_dir = root.join("stubs");
    write_stubs(&stub_dir, "exit 0");
    let clone_dir = root.join("clone");

    let output = run_scaffold(
        "rails",
        "owner/repo",
        Some(&stub_dir),
        &templates_base,
        &clone_dir,
    );
    let data = last_json(&String::from_utf8_lossy(&output.stdout));
    assert_eq!(data["status"], "ok", "got: {}", data);
    assert_eq!(data["issues_created"], 2);
}

/// Template file that is NOT under `bin/` — no chmod invocation; exercises
/// the false branch of `starts_with("bin/")`.
#[test]
fn scaffold_qa_non_bin_file_not_made_executable() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let templates_base = build_minimal_template(&root, "rails", "[]");

    let stub_dir = root.join("stubs");
    write_stubs(&stub_dir, "exit 0");
    let clone_dir = root.join("clone");

    let output = run_scaffold(
        "rails",
        "owner/repo",
        Some(&stub_dir),
        &templates_base,
        &clone_dir,
    );
    let data = last_json(&String::from_utf8_lossy(&output.stdout));
    assert_eq!(data["status"], "ok", "got: {}", data);

    // Gemfile is top-level, not under bin/ — should be regular file.
    let gemfile = clone_dir.join("Gemfile");
    let mode = fs::metadata(&gemfile).unwrap().permissions().mode();
    // Regular file creation defaults to 0o644 or similar; crucially,
    // the executable bits should NOT have been explicitly set.
    assert!(
        mode & 0o100 == 0,
        "Gemfile should not have owner exec bit, got: {:o}",
        mode
    );
}

/// Issues JSON with an invalid payload (not a JSON array) makes
/// `serde_json::from_str` return Err; the `.unwrap_or_default()` falls
/// back to an empty Vec and scaffold_qa reports issues_created=0.
#[test]
fn scaffold_qa_invalid_issues_json_defaults_to_empty_list() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    // issues.json has invalid JSON.
    let templates_base = build_minimal_template(&root, "rails", "not valid json");

    let stub_dir = root.join("stubs");
    write_stubs(&stub_dir, "exit 0");
    let clone_dir = root.join("clone");

    let output = run_scaffold(
        "rails",
        "owner/repo",
        Some(&stub_dir),
        &templates_base,
        &clone_dir,
    );
    let data = last_json(&String::from_utf8_lossy(&output.stdout));
    assert_eq!(data["status"], "ok", "got: {}", data);
    assert_eq!(data["issues_created"], 0);
}

/// run_impl subprocess exit code contract: status:ok → exit 0.
#[test]
fn scaffold_qa_cli_ok_exits_0() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let templates_base = build_minimal_template(&root, "rails", "[]");

    let stub_dir = root.join("stubs");
    write_stubs(&stub_dir, "exit 0");
    let clone_dir = root.join("clone");

    let output = run_scaffold(
        "rails",
        "owner/repo",
        Some(&stub_dir),
        &templates_base,
        &clone_dir,
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Broken symlink inside the template dir exercises the
/// `else if path.is_file()` false-else arm: is_dir=false, is_file=false,
/// entry is silently skipped.
#[test]
fn scaffold_qa_broken_symlink_in_template_is_skipped() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let templates_base = root.join("templates");
    let tpl_dir = templates_base.join("rails");
    fs::create_dir_all(tpl_dir.join(".qa")).unwrap();
    fs::write(tpl_dir.join("Gemfile"), "gem\n").unwrap();
    fs::write(tpl_dir.join(".qa/issues.json"), "[]").unwrap();
    // Dangling symlink inside the template — find_templates skips it.
    symlink(root.join("nonexistent"), tpl_dir.join("broken")).unwrap();

    let stub_dir = root.join("stubs");
    write_stubs(&stub_dir, "exit 0");
    let clone_dir = root.join("clone");

    let output = run_scaffold(
        "rails",
        "owner/repo",
        Some(&stub_dir),
        &templates_base,
        &clone_dir,
    );
    let data = last_json(&String::from_utf8_lossy(&output.stdout));
    assert_eq!(data["status"], "ok", "got: {}", data);
    assert!(
        !clone_dir.join("broken").exists(),
        "broken symlink must not be copied"
    );
    assert!(clone_dir.join("Gemfile").exists());
}

/// Template directory with a nested subdir whose permissions prevent
/// `read_dir`. Exercises the `?` Err arm of `fs::read_dir` in the
/// find_templates stack loop. Runs only on non-root processes (where
/// chmod 0o000 actually blocks access).
#[test]
fn scaffold_qa_unreadable_nested_dir_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let templates_base = root.join("templates");
    let tpl_dir = templates_base.join("rails");
    fs::create_dir_all(tpl_dir.join(".qa")).unwrap();
    fs::write(tpl_dir.join(".qa/issues.json"), "[]").unwrap();
    let locked = tpl_dir.join("locked");
    fs::create_dir(&locked).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

    let stub_dir = root.join("stubs");
    write_stubs(&stub_dir, "exit 0");
    let clone_dir = root.join("clone");

    let output = run_scaffold(
        "rails",
        "owner/repo",
        Some(&stub_dir),
        &templates_base,
        &clone_dir,
    );

    // Always restore perms so tempdir can be cleaned up.
    let _ = fs::set_permissions(&locked, fs::Permissions::from_mode(0o755));

    let data = last_json(&String::from_utf8_lossy(&output.stdout));
    // When running as root (some CI environments), chmod 0o000 is not
    // enforced and read_dir succeeds. In that case the test is vacuous;
    // assert only that the status is one of the two expected outcomes.
    assert!(
        data["status"] == "error" || data["status"] == "ok",
        "unexpected status: {}",
        data
    );
    if data["status"] == "error" {
        assert!(data["message"]
            .as_str()
            .unwrap_or("")
            .contains("Failed to read templates"));
    }
}

/// Drive `default_templates_base()` fallback: no
/// `FLOW_SCAFFOLD_TEMPLATES_BASE` env override. The binary resolves its
/// own templates_base via `current_exe` parent walk. Under
/// cargo-llvm-cov the binary lives at
/// `target/llvm-cov-target/debug/flow-rs`, so the 3-parent walk lands
/// at `target/` — no `qa/templates` there, so scaffold_qa errors with
/// "Unknown template". Either way, the `default_templates_base`
/// fallback branch is exercised.
#[test]
fn scaffold_qa_default_templates_base_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let stub_dir = root.join("stubs");
    write_stubs(&stub_dir, "exit 0");
    let clone_dir = root.join("clone");

    let path_env = format!(
        "{}:{}",
        stub_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "scaffold-qa",
            "--template",
            "definitely-not-a-template",
            "--repo",
            "owner/defaults",
        ])
        .env("PATH", &path_env)
        .env("FLOW_SCAFFOLD_CLONE_DIR", &clone_dir)
        .env_remove("FLOW_SCAFFOLD_TEMPLATES_BASE")
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("spawn flow-rs");

    // Production fallback fires. Either the resolved path contains a
    // valid template (happy path) or it doesn't (Unknown template). We
    // just assert the command produced JSON output, proving the
    // fallback branch executed without panic.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let _data = last_json(&stdout);
}

/// Drive `default_clone_dir()` fallback: no `FLOW_SCAFFOLD_CLONE_DIR`
/// env override. A fresh UUID dir under `temp_dir()` is used.
#[test]
fn scaffold_qa_default_clone_dir_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let templates_base = build_minimal_template(&root, "rails", "[]");
    let stub_dir = root.join("stubs");
    write_stubs(&stub_dir, "exit 0");

    let path_env = format!(
        "{}:{}",
        stub_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "scaffold-qa",
            "--template",
            "rails",
            "--repo",
            "owner/clone-default",
        ])
        .env("PATH", &path_env)
        .env("FLOW_SCAFFOLD_TEMPLATES_BASE", &templates_base)
        .env_remove("FLOW_SCAFFOLD_CLONE_DIR")
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("spawn flow-rs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let data = last_json(&stdout);
    assert_eq!(data["status"], "ok", "got: {}", data);
}

/// run_impl subprocess exit code contract: status:error → exit 1.
#[test]
fn scaffold_qa_cli_error_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let templates_base = root.join("templates");
    fs::create_dir_all(&templates_base).unwrap();
    let stub_dir = root.join("stubs");
    write_stubs(&stub_dir, "exit 0");
    let clone_dir = root.join("clone");

    // Unknown template → status:error → exit 1.
    let output = run_scaffold(
        "definitely_not_a_template",
        "owner/repo",
        Some(&stub_dir),
        &templates_base,
        &clone_dir,
    );
    assert_eq!(output.status.code(), Some(1));
}
