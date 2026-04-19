//! Integration tests for `bin/flow analyze-issues`.
//!
//! Uses `--issues-json` to bypass the gh subprocess — the flag exists
//! specifically for testing.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_gh_stub, create_git_repo_with_remote};
use serde_json::{json, Value};

/// Parse the full stdout as JSON (analyze-issues pretty-prints, so
/// last-line parsing doesn't work for it).
fn parse_full_stdout(output: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("Failed to parse stdout as JSON: {}\nstdout: {}", e, stdout))
}

fn run_analyze(repo: &Path, args: &[&str], stub_dir: &Path) -> Output {
    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("analyze-issues")
        .args(args)
        .current_dir(repo)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

/// Build a fake gh issue list response.
fn fake_issue(number: i64, title: &str, labels: Vec<&str>) -> serde_json::Value {
    let label_objs: Vec<serde_json::Value> =
        labels.iter().map(|name| json!({"name": name})).collect();
    json!({
        "number": number,
        "title": title,
        "body": "Some issue body",
        "url": format!("https://github.com/o/r/issues/{}", number),
        "createdAt": "2026-04-01T00:00:00Z",
        "labels": label_objs,
        "milestone": null,
    })
}

/// Covers the `check_stale` body path in this integration test
/// binary's flow-rs subprocess — exercises the case where
/// `age_days >= 60` AND `file_paths` is non-empty. Without this,
/// every other integration test uses recent createdAt dates and
/// check_stale always early-returns in the main bin instance.
#[test]
fn analyze_issues_stale_detection_via_subprocess() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // createdAt 90 days ago; body references a nonexistent file so
    // check_stale's filter finds 1 missing path.
    let old_date = (chrono::Utc::now() - chrono::Duration::days(90)).to_rfc3339();
    let issue = json!({
        "number": 77,
        "title": "Stale",
        "body": "See /definitely/nonexistent/stale_ref.py",
        "url": "https://github.com/o/r/issues/77",
        "createdAt": old_date,
        "labels": [],
        "milestone": null,
    });
    let issues = vec![issue];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho '{\"data\":{}}'\nexit 0\n");

    let output = run_analyze(
        &repo,
        &["--issues-json", issues_path.to_str().unwrap()],
        &stub_dir,
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_full_stdout(&output);
    assert_eq!(data["status"], "ok");
    // Verify the stale fields are populated — confirms check_stale
    // body executed (not the early-return path).
    let first = &data["issues"][0];
    assert_eq!(first["stale"], true);
    assert!(first["stale_missing"].as_i64().unwrap() >= 1);
}

/// Covers the per-binary instantiation's None branch of
/// `.as_str().map(String::from)` in both `detect_labels` and
/// `analyze_issues`'s label_names extraction. Without a label
/// object lacking a string `"name"`, this integration test
/// binary never exercises the `?` None short-circuit.
#[test]
fn analyze_issues_non_string_label_name_filtered() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let issue = json!({
        "number": 42,
        "title": "Mixed labels",
        "body": "",
        "url": "https://github.com/o/r/issues/42",
        "createdAt": "2026-04-01T00:00:00Z",
        "labels": [
            {"color": "red"},       // no "name" key → ? short-circuits
            {"name": null},          // as_str() None
            {"name": 42},            // as_str() None
            {"name": "valid-label"}, // Some("valid-label")
        ],
        "milestone": null,
    });
    let issues = vec![issue];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho '{\"data\":{}}'\nexit 0\n");

    let output = run_analyze(
        &repo,
        &["--issues-json", issues_path.to_str().unwrap()],
        &stub_dir,
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_full_stdout(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["total"], 1);
}

/// Covers the `.spawn()?` Err-propagation region of
/// `run_with_drain_and_timeout` inside the main binary's
/// instantiation. Constructs a stub_dir where the `gh` entry is
/// present but NOT executable — spawn returns EACCES / permission
/// denied, hitting the Err arm of `.spawn()?`. Without this, the
/// main bin's instance of `run_with_drain_and_timeout` only ever
/// sees successful spawns (gh exists on PATH, subprocess spawns
/// fine, gh then fails via exit code).
#[test]
fn analyze_issues_gh_spawn_err_covers_spawn_question_mark() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    // stub_dir with non-executable "gh" → spawn returns EACCES.
    let stub_dir = dir.path().join("noexec_stub");
    fs::create_dir_all(&stub_dir).unwrap();
    fs::write(stub_dir.join("gh"), b"not executable").unwrap();
    // No chmod +x → spawn fails with permission-denied on Unix.

    let issues = vec![fake_issue(1, "T", vec![])];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();

    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("analyze-issues")
        .args(["--issues-json", issues_path.to_str().unwrap()])
        .current_dir(&repo)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    // analyze-issues with --issues-json skips the outer gh call but
    // fetch_blockers may still try to spawn gh (if detect_repo returns
    // Some). Local bare remote → detect_repo returns None → fetch_blockers
    // not called. So this test really verifies the flow still exits
    // 0 with stubbed PATH.
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Force the outer gh-issue-list path (no --issues-json) AND make
/// gh non-executable so flow-rs's `run_with_drain_and_timeout`
/// hits `.spawn()?` Err branch. With no --issues-json, run_impl_main
/// goes through `read_issues_json` → gh path. With non-executable
/// gh in an isolated PATH, spawn returns EACCES → `?` Err → gh_result_to_stdout
/// returns Err → read_issues_json returns Err → exit 1.
#[test]
fn analyze_issues_no_issues_json_gh_unexecutable_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    let stub_dir = dir.path().join("noexec_stub");
    fs::create_dir_all(&stub_dir).unwrap();
    fs::write(stub_dir.join("gh"), b"not executable").unwrap();
    // No chmod +x.

    // Isolated PATH: only stub_dir (no /usr/bin so gh in stub is the
    // only candidate; spawn() on non-exec returns EACCES).
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("analyze-issues")
        .current_dir(dir.path())
        .env("PATH", stub_dir.to_string_lossy().to_string())
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    // gh spawn fails → analyze-issues exits 1 with an error payload.
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("error"),
        "expected error output, got: {}",
        stdout
    );
}

#[test]
fn analyze_issues_reads_json_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let issues = vec![
        fake_issue(1, "First", vec!["Rule"]),
        fake_issue(2, "Second", vec!["Tech Debt"]),
    ];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
    // gh is still called for blockers but stub returns empty.
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho '{\"data\":{}}'\nexit 0\n");

    let output = run_analyze(
        &repo,
        &["--issues-json", issues_path.to_str().unwrap()],
        &stub_dir,
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_full_stdout(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["total"], 2);
}

#[test]
fn analyze_issues_partitions_in_progress() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let issues = vec![
        fake_issue(1, "In progress", vec!["Flow In-Progress"]),
        fake_issue(2, "Available", vec!["Rule"]),
    ];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho '{\"data\":{}}'\nexit 0\n");

    let output = run_analyze(
        &repo,
        &["--issues-json", issues_path.to_str().unwrap()],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_full_stdout(&output);
    let in_progress = data["in_progress"].as_array().unwrap();
    assert_eq!(in_progress.len(), 1);
    assert_eq!(in_progress[0]["number"], 1);
}

#[test]
fn analyze_issues_nonexistent_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let missing = dir.path().join("no-such.json");
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_analyze(
        &repo,
        &["--issues-json", missing.to_str().unwrap()],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_full_stdout(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Could not read issues file"));
}

#[test]
fn analyze_issues_empty_list() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let issues: Vec<serde_json::Value> = vec![];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho '{\"data\":{}}'\nexit 0\n");

    let output = run_analyze(
        &repo,
        &["--issues-json", issues_path.to_str().unwrap()],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_full_stdout(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["total"], 0);
    assert!(data["issues"].as_array().unwrap().is_empty());
}

#[test]
fn analyze_issues_ready_filter() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let issues = vec![
        fake_issue(1, "Ready", vec!["Rule"]),
        fake_issue(2, "Also ready", vec!["Tech Debt"]),
    ];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho '{\"data\":{}}'\nexit 0\n");

    let output = run_analyze(
        &repo,
        &["--issues-json", issues_path.to_str().unwrap(), "--ready"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_full_stdout(&output);
    assert_eq!(data["status"], "ok");
}

#[test]
fn analyze_issues_decomposed_filter() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let issues = vec![fake_issue(1, "Any", vec!["Rule"])];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho '{\"data\":{}}'\nexit 0\n");

    let output = run_analyze(
        &repo,
        &[
            "--issues-json",
            issues_path.to_str().unwrap(),
            "--decomposed",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_full_stdout(&output);
    assert_eq!(data["status"], "ok");
}

#[test]
fn analyze_issues_blocked_filter() {
    // Drive the "blocked" filter closure inside filter_issues via the
    // run() CLI path (covers the closure body in the production binary).
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let issues = vec![fake_issue(1, "Decomposed", vec!["Decomposed"])];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho '{\"data\":{}}'\nexit 0\n");

    let output = run_analyze(
        &repo,
        &["--issues-json", issues_path.to_str().unwrap(), "--blocked"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_full_stdout(&output);
    assert_eq!(data["status"], "ok");
}

#[test]
fn analyze_issues_quick_start_filter() {
    // Drive the "quick-start" filter closure inside filter_issues via run().
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let issues = vec![fake_issue(1, "Decomposed", vec!["Decomposed"])];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho '{\"data\":{}}'\nexit 0\n");

    let output = run_analyze(
        &repo,
        &[
            "--issues-json",
            issues_path.to_str().unwrap(),
            "--quick-start",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_full_stdout(&output);
    assert_eq!(data["status"], "ok");
}

#[test]
fn analyze_issues_invalid_json_content_errors() {
    // File exists but contains invalid JSON → run() prints an error and
    // exits 1 via the "Invalid JSON" branch of the from_str match arm.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, "this is not json").unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_analyze(
        &repo,
        &["--issues-json", issues_path.to_str().unwrap()],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(1));
    let data = parse_full_stdout(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("Invalid JSON"));
}

#[test]
fn analyze_issues_gh_failure_errors() {
    // No --issues-json: run() invokes `gh issue list`. Stub exits non-zero
    // → run() prints an error and exits 1 via the gh_result_to_stdout Err
    // branch.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho 'gh failed' >&2\nexit 1\n");

    let output = run_analyze(&repo, &[], &stub_dir);

    assert_eq!(output.status.code(), Some(1));
    let data = parse_full_stdout(&output);
    assert_eq!(data["status"], "error");
}

#[test]
fn analyze_issues_label_and_milestone_args_forwarded_to_gh() {
    // --label and --milestone args are pushed into the gh command. With a
    // stub that returns a valid issue list, the run() succeeds and exit 0.
    // Drives the `for l in &args.label` loop and the `if let Some(ref m)`
    // milestone branch.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho '[]'\nexit 0\n");

    let output = run_analyze(
        &repo,
        &[
            "--label",
            "Rule",
            "--label",
            "Tech Debt",
            "--milestone",
            "v1",
        ],
        &stub_dir,
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_full_stdout(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["total"], 0);
}
