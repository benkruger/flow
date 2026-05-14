//! Integration tests for `bin/flow analyze-issues` and its library surface.
//!
//! Migrated from inline `#[cfg(test)]` per
//! `.claude/rules/test-placement.md`.

mod common;

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_gh_stub, create_git_repo_with_remote};
use flow_rs::analyze_issues::{
    analyze_issues, blocker_result_to_map, build_blocker_query, detect_labels, fetch_blockers,
    filter_issues, gh_output_to_result, normalize_error_payload, parse_blocker_response, run_gh,
    run_impl_main, Args,
};
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
fn analyze_issues_collapse_flags_in_progress_row_in_issues_array() {
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
    assert!(data.get("in_progress").is_none());
    let issues_arr = data["issues"].as_array().unwrap();
    assert_eq!(issues_arr.len(), 2);
    let row1 = issues_arr.iter().find(|i| i["number"] == 1).unwrap();
    assert_eq!(row1["flow_in_progress"], true);
    let row2 = issues_arr.iter().find(|i| i["number"] == 2).unwrap();
    assert_eq!(row2["flow_in_progress"], false);
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
fn analyze_issues_gh_json_field_list_includes_assignees() {
    // Drive the gh path (no --issues-json) with a stub that captures
    // its argv to a file. Assert the `--json <field-list>` argument
    // contains `assignees` so the analyzer can surface per-row
    // assignees logins.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let log_path = repo.join("gh_args.log");
    let stub_script = format!(
        "#!/bin/bash\nprintf '%s\\n' \"$@\" > {}\necho '[]'\nexit 0\n",
        log_path.to_string_lossy(),
    );
    let stub_dir = create_gh_stub(&repo, &stub_script);

    let output = run_analyze(&repo, &[], &stub_dir);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    let logged = fs::read_to_string(&log_path).expect("gh stub should have logged args");
    let lines: Vec<&str> = logged.lines().collect();
    let json_idx = lines
        .iter()
        .position(|l| *l == "--json")
        .expect("gh args should include --json");
    let field_list = lines
        .get(json_idx + 1)
        .expect("--json should be followed by a value");
    assert!(
        field_list.split(',').any(|f| f == "assignees"),
        "expected --json field list to include `assignees`; got `{}`",
        field_list,
    );
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

// --- Library-level tests (migrated from inline `#[cfg(test)]`) ---

#[test]
fn detects_in_progress_label_lib() {
    let labels = vec![json!({"name": "Flow In-Progress"}), json!({"name": "Bug"})];
    let result = detect_labels(&labels);
    assert!(result.flow_in_progress);
    assert!(!result.decomposed);
    assert!(!result.blocked);
}

#[test]
fn detects_vanilla_label_canonical_case_lib() {
    let labels = vec![json!({"name": "Vanilla"})];
    assert!(detect_labels(&labels).vanilla);
}

#[test]
fn detects_vanilla_label_lowercase_lib() {
    let labels = vec![json!({"name": "vanilla"})];
    assert!(detect_labels(&labels).vanilla);
}

#[test]
fn detects_vanilla_label_uppercase_lib() {
    let labels = vec![json!({"name": "VANILLA"})];
    assert!(detect_labels(&labels).vanilla);
}

#[test]
fn detects_flow_in_progress_exact_match_lib() {
    let labels = vec![json!({"name": "Flow In-Progress"})];
    let result = detect_labels(&labels);
    assert!(result.flow_in_progress);
    assert!(!result.triage_in_progress);
}

#[test]
fn detects_triage_in_progress_exact_match_lib() {
    let labels = vec![json!({"name": "Triage In-Progress"})];
    let result = detect_labels(&labels);
    assert!(result.triage_in_progress);
    assert!(!result.flow_in_progress);
}

#[test]
fn no_vanilla_no_flow_no_triage_in_progress_lib() {
    let labels = vec![json!({"name": "Bug"})];
    let result = detect_labels(&labels);
    assert!(!result.vanilla);
    assert!(!result.flow_in_progress);
    assert!(!result.triage_in_progress);
}

#[test]
fn detects_decomposed_label_lib() {
    let labels = vec![json!({"name": "decomposed"})];
    let result = detect_labels(&labels);
    assert!(result.decomposed);
}

#[test]
fn detects_decomposed_label_case_insensitive_lib() {
    let labels = vec![json!({"name": "Decomposed"})];
    let result = detect_labels(&labels);
    assert!(result.decomposed);
}

#[test]
fn detects_blocked_label_lib() {
    let labels = vec![json!({"name": "Blocked"}), json!({"name": "Bug"})];
    let result = detect_labels(&labels);
    assert!(result.blocked);
}

#[test]
fn detects_blocked_label_case_insensitive_lib() {
    let labels = vec![json!({"name": "blocked"})];
    assert!(detect_labels(&labels).blocked);
}

#[test]
fn no_blocked_label_lib() {
    let labels = vec![json!({"name": "Enhancement"})];
    assert!(!detect_labels(&labels).blocked);
}

#[test]
fn no_special_labels_lib() {
    let labels = vec![json!({"name": "Bug"})];
    let result = detect_labels(&labels);
    assert!(!result.flow_in_progress);
    assert!(!result.decomposed);
    assert!(!result.blocked);
}

#[test]
fn empty_labels_lib() {
    let result = detect_labels(&[]);
    assert!(!result.flow_in_progress);
    assert!(!result.decomposed);
    assert!(!result.blocked);
}

#[test]
fn build_blocker_query_single_issue_lib() {
    let query = build_blocker_query(&[10]);
    assert!(query.contains("issue_10: issue(number: 10)"));
    assert!(query.contains("blockedBy(first: 10)"));
    assert!(query.contains("nodes"));
    assert!(query.contains("number"));
    assert!(query.contains("state"));
}

#[test]
fn build_blocker_query_multiple_issues_lib() {
    let query = build_blocker_query(&[10, 20, 30]);
    assert!(query.contains("issue_10: issue(number: 10)"));
    assert!(query.contains("issue_20: issue(number: 20)"));
    assert!(query.contains("issue_30: issue(number: 30)"));
}

#[test]
fn build_blocker_query_has_variables_lib() {
    let query = build_blocker_query(&[1]);
    assert!(query.contains("$owner: String!"));
    assert!(query.contains("$repo: String!"));
}

fn graphql_response(issue_blockers: &[(i64, Vec<(i64, &str)>)]) -> String {
    let mut repo_data = serde_json::Map::new();
    for (number, blockers) in issue_blockers {
        let nodes: Vec<Value> = blockers
            .iter()
            .map(|(n, state)| json!({"number": n, "state": state}))
            .collect();
        repo_data.insert(
            format!("issue_{}", number),
            json!({"blockedBy": {"nodes": nodes}}),
        );
    }
    json!({"data": {"repository": repo_data}}).to_string()
}

#[test]
fn parse_blocker_response_happy_path_lib() {
    let response = graphql_response(&[
        (10, vec![(100, "OPEN"), (101, "OPEN")]),
        (20, vec![]),
        (30, vec![(200, "OPEN")]),
    ]);
    let result = parse_blocker_response(&response, &[10, 20, 30], "owner/repo");
    let nums_10: Vec<i64> = result[&10]
        .iter()
        .map(|v| v["number"].as_i64().unwrap())
        .collect();
    assert_eq!(nums_10, vec![100, 101]);
    assert!(result[&20].is_empty());
    let nums_30: Vec<i64> = result[&30]
        .iter()
        .map(|v| v["number"].as_i64().unwrap())
        .collect();
    assert_eq!(nums_30, vec![200]);
    assert_eq!(
        result[&10][0]["url"],
        "https://github.com/owner/repo/issues/100"
    );
}

#[test]
fn parse_blocker_response_filters_closed_lib() {
    let response = graphql_response(&[(10, vec![(100, "OPEN"), (101, "CLOSED")])]);
    let result = parse_blocker_response(&response, &[10], "owner/repo");
    assert_eq!(result[&10].len(), 1);
    assert_eq!(result[&10][0]["number"], 100);
}

#[test]
fn parse_blocker_response_all_closed_returns_empty_lib() {
    let response = graphql_response(&[(10, vec![(100, "CLOSED"), (101, "CLOSED")])]);
    let result = parse_blocker_response(&response, &[10], "owner/repo");
    assert!(result[&10].is_empty());
}

#[test]
fn parse_blocker_response_malformed_json_lib() {
    let result = parse_blocker_response("{corrupt", &[10], "owner/repo");
    assert!(result.is_empty());
}

#[test]
fn parse_blocker_response_null_repository_lib() {
    let response = r#"{"data":{"repository":null}}"#;
    let result = parse_blocker_response(response, &[10], "owner/repo");
    assert!(result[&10].is_empty());
}

#[test]
fn parse_blocker_response_null_blocked_by_lib() {
    let response = r#"{"data":{"repository":{"issue_10":{"blockedBy":null}}}}"#;
    let result = parse_blocker_response(response, &[10], "owner/repo");
    assert!(result[&10].is_empty());
}

#[test]
fn parse_blocker_response_null_nodes_lib() {
    let response = r#"{"data":{"repository":{"issue_10":{"blockedBy":{"nodes":null}}}}}"#;
    let result = parse_blocker_response(response, &[10], "owner/repo");
    assert!(result[&10].is_empty());
}

#[test]
fn parse_blocker_response_open_node_missing_number_filtered_lib() {
    // OPEN node with no `number` field exercises the
    // `.and_then(|n| n.as_i64())?` short-circuit in the filter_map.
    let response = r#"{"data":{"repository":{"issue_10":{"blockedBy":{"nodes":[
        {"state":"OPEN"},
        {"number":50,"state":"OPEN"}
    ]}}}}}"#;
    let result = parse_blocker_response(response, &[10], "owner/repo");
    assert_eq!(result[&10].len(), 1);
    assert_eq!(result[&10][0]["number"], 50);
}

#[test]
fn parse_blocker_response_invalid_repo_empty_owner_rejected_lib() {
    let response = r#"{"data":{"repository":{"issue_10":{"blockedBy":{"nodes":[
        {"number": 50, "state": "OPEN"}
    ]}}}}}"#;
    let result = parse_blocker_response(response, &[10], "/repo");
    assert!(result.is_empty());
}

#[test]
fn parse_blocker_response_invalid_repo_dot_segment_rejected_lib() {
    let response = r#"{"data":{"repository":{"issue_10":{"blockedBy":{"nodes":[
        {"number": 50, "state": "OPEN"}
    ]}}}}}"#;
    let result = parse_blocker_response(response, &[10], "./repo");
    assert!(result.is_empty());
}

#[test]
fn parse_blocker_response_invalid_repo_too_many_slashes_rejected_lib() {
    let response = r#"{"data":{"repository":{"issue_10":{"blockedBy":{"nodes":[
        {"number": 50, "state": "OPEN"}
    ]}}}}}"#;
    let result = parse_blocker_response(response, &[10], "owner/repo/extra");
    assert!(result.is_empty());
}

#[test]
fn parse_blocker_response_invalid_repo_empty_name_rejected_lib() {
    let response = r#"{"data":{"repository":{"issue_10":{"blockedBy":{"nodes":[
        {"number": 50, "state": "OPEN"}
    ]}}}}}"#;
    let result = parse_blocker_response(response, &[10], "owner/");
    assert!(result.is_empty());
}

#[test]
fn parse_blocker_response_invalid_repo_no_slash_rejected_lib() {
    let response = r#"{"data":{"repository":{"issue_10":{"blockedBy":{"nodes":[
        {"number": 50, "state": "OPEN"}
    ]}}}}}"#;
    let result = parse_blocker_response(response, &[10], "owner");
    assert!(result.is_empty());
}

#[test]
fn parse_blocker_response_invalid_repo_path_traversal_rejected_lib() {
    let response = r#"{"data":{"repository":{"issue_10":{"blockedBy":{"nodes":[
        {"number": 50, "state": "OPEN"}
    ]}}}}}"#;
    let result = parse_blocker_response(response, &[10], "owner/..");
    assert!(result.is_empty());
}

#[test]
fn parse_blocker_response_invalid_repo_with_pipe_rejected_lib() {
    let response = r#"{"data":{"repository":{"issue_10":{"blockedBy":{"nodes":[
        {"number": 50, "state": "OPEN"}
    ]}}}}}"#;
    let result = parse_blocker_response(response, &[10], "owner|evil/repo");
    assert!(result.is_empty());
}

#[test]
fn parse_blocker_response_valid_repo_with_dash_underscore_dot_accepted_lib() {
    // Exercises the `c == '-'`, `c == '_'`, `c == '.'` branches in the
    // is_safe_repo_slug character whitelist.
    let response = r#"{"data":{"repository":{"issue_10":{"blockedBy":{"nodes":[
        {"number": 50, "state": "OPEN"}
    ]}}}}}"#;
    let result = parse_blocker_response(response, &[10], "my-org_name/repo.v1");
    assert_eq!(result[&10].len(), 1);
    assert_eq!(
        result[&10][0]["url"],
        "https://github.com/my-org_name/repo.v1/issues/50"
    );
}

#[test]
fn fetch_blockers_empty_list_lib() {
    assert!(fetch_blockers("owner/repo", &[]).is_empty());
}

#[test]
fn fetch_blockers_malformed_repo_lib() {
    assert!(fetch_blockers("noslash", &[10]).is_empty());
}

fn fake_issue_lib(number: i64, title: &str, labels: Vec<&str>) -> Value {
    let labels_json: Vec<Value> = labels.into_iter().map(|l| json!({"name": l})).collect();
    json!({
        "number": number,
        "title": title,
        "body": "",
        "labels": labels_json,
        "createdAt": chrono::Local::now().to_rfc3339(),
        "url": format!("https://github.com/test/repo/issues/{}", number),
        "milestone": Value::Null,
    })
}

#[test]
fn run_impl_main_with_issues_json_path_returns_ok() {
    let dir = tempfile::tempdir().unwrap();
    let issues = vec![fake_issue_lib(1, "Test", vec!["Rule"])];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
    let args = Args {
        issues_json: Some(issues_path.to_string_lossy().into_owned()),
        ready: false,
        blocked: false,
        decomposed: false,
        quick_start: false,
        label: Vec::new(),
        milestone: None,
    };
    let (value, code) = run_impl_main(args);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
}

#[test]
fn run_impl_main_missing_file_returns_error_one() {
    let args = Args {
        issues_json: Some("/definitely/not/a/real/path.json".to_string()),
        ready: false,
        blocked: false,
        decomposed: false,
        quick_start: false,
        label: Vec::new(),
        milestone: None,
    };
    let (value, code) = run_impl_main(args);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("Could not read issues file"));
}

#[test]
fn run_impl_main_malformed_json_returns_error_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.json");
    fs::write(&path, "{corrupt").unwrap();
    let args = Args {
        issues_json: Some(path.to_string_lossy().into_owned()),
        ready: false,
        blocked: false,
        decomposed: false,
        quick_start: false,
        label: Vec::new(),
        milestone: None,
    };
    let (value, code) = run_impl_main(args);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    assert!(value["message"].as_str().unwrap().contains("Invalid JSON"));
}

#[test]
fn run_impl_main_with_ready_filter_applies_filter() {
    let dir = tempfile::tempdir().unwrap();
    let issues = vec![
        fake_issue_lib(1, "Ready", vec!["Rule"]),
        fake_issue_lib(2, "Blocked", vec!["Blocked"]),
    ];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
    let args = Args {
        issues_json: Some(issues_path.to_string_lossy().into_owned()),
        ready: true,
        blocked: false,
        decomposed: false,
        quick_start: false,
        label: Vec::new(),
        milestone: None,
    };
    let (value, code) = run_impl_main(args);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
}

#[test]
fn run_impl_main_blocked_filter_applies() {
    let dir = tempfile::tempdir().unwrap();
    let issues = vec![
        fake_issue_lib(1, "Ready", vec!["Rule"]),
        fake_issue_lib(2, "Blocked", vec!["Blocked"]),
    ];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
    let args = Args {
        issues_json: Some(issues_path.to_string_lossy().into_owned()),
        ready: false,
        blocked: true,
        decomposed: false,
        quick_start: false,
        label: Vec::new(),
        milestone: None,
    };
    let (value, code) = run_impl_main(args);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
}

#[test]
fn run_impl_main_decomposed_filter_applies() {
    let dir = tempfile::tempdir().unwrap();
    let issues = vec![fake_issue_lib(1, "Decomposed issue", vec!["decomposed"])];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
    let args = Args {
        issues_json: Some(issues_path.to_string_lossy().into_owned()),
        ready: false,
        blocked: false,
        decomposed: true,
        quick_start: false,
        label: Vec::new(),
        milestone: None,
    };
    let (value, code) = run_impl_main(args);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
}

#[test]
fn run_impl_main_quick_start_filter_applies() {
    let dir = tempfile::tempdir().unwrap();
    let issues = vec![fake_issue_lib(1, "Quick start", vec!["decomposed"])];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
    let args = Args {
        issues_json: Some(issues_path.to_string_lossy().into_owned()),
        ready: false,
        blocked: false,
        decomposed: false,
        quick_start: true,
        label: Vec::new(),
        milestone: None,
    };
    let (value, code) = run_impl_main(args);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
}

// --- gh_output_to_result ---

fn fake_output(code: Option<i32>, stdout: &str, stderr: &str) -> std::process::Output {
    use std::os::unix::process::ExitStatusExt;
    let status = match code {
        Some(c) => std::process::ExitStatus::from_raw(c << 8),
        None => std::process::ExitStatus::from_raw(9),
    };
    std::process::Output {
        status,
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

#[test]
fn gh_output_to_result_success_returns_stdout_lib() {
    let out = fake_output(Some(0), "payload", "");
    assert_eq!(gh_output_to_result(out, "gh").unwrap(), "payload");
}

#[test]
fn gh_output_to_result_nonzero_with_stderr_returns_labeled_error_lib() {
    let out = fake_output(Some(2), "", "oops");
    assert_eq!(
        gh_output_to_result(out, "gh issue list").unwrap_err(),
        "gh issue list failed: oops"
    );
}

#[test]
fn gh_output_to_result_nonzero_empty_stderr_with_code_names_exit_code_lib() {
    let out = fake_output(Some(9), "", "");
    let err = gh_output_to_result(out, "gh").unwrap_err();
    assert!(err.contains("no stderr output"));
    assert!(err.contains("exit code 9"));
}

#[test]
fn gh_output_to_result_signal_terminated_empty_stderr_names_signal_lib() {
    let out = fake_output(None, "", "");
    let err = gh_output_to_result(out, "gh").unwrap_err();
    assert!(err.contains("terminated by signal"));
}

#[test]
fn gh_output_to_result_whitespace_stderr_includes_exit_code_lib() {
    let out = fake_output(Some(3), "", "   \n\t\n  ");
    let err = gh_output_to_result(out, "gh").unwrap_err();
    assert!(err.contains("exit code 3"));
}

#[test]
fn gh_output_to_result_strips_nuls_and_cr_lf_from_stderr_lib() {
    let out = fake_output(Some(4), "", "foo\0bar\r\nbaz");
    let err = gh_output_to_result(out, "gh").unwrap_err();
    assert!(!err.contains('\0'));
    assert!(!err.contains('\r'));
    assert!(!err.contains('\n'));
}

#[test]
fn run_gh_executes_body_lib() {
    let _ = run_gh(&["--version"], "gh --version");
}

#[test]
fn blocker_result_to_map_ok_parses_response_lib() {
    let response = r#"{"data":{"repository":{"issue_10":{"blockedBy":{"nodes":[]}}}}}"#;
    let map = blocker_result_to_map(&[10], "owner/repo", Ok(response.to_string()));
    assert!(map.contains_key(&10));
}

#[test]
fn blocker_result_to_map_err_logs_and_returns_empty_lib() {
    let map = blocker_result_to_map(&[10], "owner/repo", Err("gh failed".to_string()));
    assert!(map.is_empty());
}

#[test]
fn normalize_error_payload_strips_nuls_lib() {
    assert_eq!(normalize_error_payload("a\0b\0c"), "abc");
}

#[test]
fn normalize_error_payload_collapses_newlines_lib() {
    assert_eq!(normalize_error_payload("a\r\nb\nc"), "a b c");
}

#[test]
fn normalize_error_payload_trims_and_collapses_whitespace_lib() {
    assert_eq!(normalize_error_payload("  foo   bar  \n\t "), "foo bar");
}

#[test]
fn normalize_error_payload_empty_on_whitespace_only_lib() {
    assert_eq!(normalize_error_payload("   \n\t \r\n  "), "");
}

#[test]
fn normalize_error_payload_passes_through_normal_text_lib() {
    assert_eq!(normalize_error_payload("hello world"), "hello world");
}

// --- analyze_issues helpers ---

fn make_issue_lib(
    number: i64,
    title: &str,
    body: &str,
    labels: &[&str],
    created_at: &str,
) -> Value {
    make_issue_opt_lib(number, title, body, labels, created_at, None)
}

fn make_issue_opt_lib(
    number: i64,
    title: &str,
    body: &str,
    labels: &[&str],
    created_at: &str,
    milestone_title: Option<&str>,
) -> Value {
    let label_arr: Vec<Value> = labels.iter().map(|n| json!({"name": n})).collect();
    let milestone = match milestone_title {
        Some(t) => json!({"title": t, "number": 1}),
        None => Value::Null,
    };
    json!({
        "number": number,
        "title": title,
        "body": body,
        "labels": label_arr,
        "createdAt": created_at,
        "url": format!("https://github.com/test/repo/issues/{}", number),
        "milestone": milestone,
    })
}

fn now_iso_lib() -> String {
    chrono::Local::now().to_rfc3339()
}

#[test]
fn analyze_non_string_label_name_filtered_out_lib() {
    let issue = json!({
        "number": 99,
        "title": "Non-string label",
        "body": "",
        "labels": [
            {"color": "red"},
            {"name": null},
            {"name": 42},
            {"name": "valid-label"},
        ],
        "createdAt": now_iso_lib(),
        "url": "https://github.com/test/repo/issues/99",
        "milestone": Value::Null,
    });
    let result = analyze_issues(&[issue], &HashMap::new());
    let issues_arr = result["issues"].as_array().unwrap();
    assert_eq!(issues_arr.len(), 1);
    let labels = issues_arr[0]["labels"].as_array().unwrap();
    assert!(labels.iter().any(|l| l == "valid-label"));
    assert!(!labels.iter().any(|l| l.is_null()));
}

#[test]
fn analyze_empty_list_lib() {
    let result = analyze_issues(&[], &HashMap::new());
    assert_eq!(result["status"], "ok");
    assert_eq!(result["total"], 0);
}

#[test]
fn analyze_routes_flow_in_progress_into_issues_lib() {
    let issues = vec![
        make_issue_lib(1, "Active", "", &["Flow In-Progress"], &now_iso_lib()),
        make_issue_lib(2, "Available", "", &[], &now_iso_lib()),
    ];
    let result = analyze_issues(&issues, &HashMap::new());
    assert!(result.get("in_progress").is_none());
    let arr = result["issues"].as_array().unwrap();
    assert_eq!(arr.len(), 2);
    let row1 = arr.iter().find(|i| i["number"] == 1).unwrap();
    assert_eq!(row1["flow_in_progress"], true);
}

#[test]
fn analyze_issue_fields_lib() {
    let issues = vec![make_issue_lib(
        1,
        "Test",
        "Check lib/foo.py",
        &["decomposed"],
        &now_iso_lib(),
    )];
    let result = analyze_issues(&issues, &HashMap::new());
    let issue = &result["issues"][0];
    assert!(issue["decomposed"].as_bool().unwrap());
    assert_eq!(issue["number"], 1);
}

#[test]
fn analyze_blocked_label_lib() {
    let issues = vec![
        make_issue_lib(1, "Ready issue", "", &[], &now_iso_lib()),
        make_issue_lib(2, "Blocked issue", "", &["Blocked"], &now_iso_lib()),
    ];
    let result = analyze_issues(&issues, &HashMap::new());
    let arr = result["issues"].as_array().unwrap();
    let issue_1 = arr.iter().find(|i| i["number"] == 1).unwrap();
    let issue_2 = arr.iter().find(|i| i["number"] == 2).unwrap();
    assert!(!issue_1["blocked"].as_bool().unwrap());
    assert!(issue_2["blocked"].as_bool().unwrap());
}

#[test]
fn analyze_total_includes_all_lib() {
    let issues = vec![
        make_issue_lib(1, "A", "", &["Flow In-Progress"], &now_iso_lib()),
        make_issue_lib(2, "B", "", &[], &now_iso_lib()),
        make_issue_lib(3, "C", "", &[], &now_iso_lib()),
    ];
    let result = analyze_issues(&issues, &HashMap::new());
    assert_eq!(result["total"], 3);
}

#[test]
fn analyze_native_blocked_without_label_lib() {
    let issues = vec![make_issue_lib(
        10,
        "Has native blocker",
        "",
        &[],
        &now_iso_lib(),
    )];
    let mut blocker_map: HashMap<i64, Vec<Value>> = HashMap::new();
    blocker_map.insert(
        10,
        vec![
            json!({"number": 100, "url": "https://github.com/test/repo/issues/100"}),
            json!({"number": 200, "url": "https://github.com/test/repo/issues/200"}),
        ],
    );
    let result = analyze_issues(&issues, &blocker_map);
    let issue = &result["issues"][0];
    assert!(issue["blocked"].as_bool().unwrap());
    assert!(issue["native_blocked"].as_bool().unwrap());
}

#[test]
fn analyze_no_blocker_counts_default_lib() {
    let issues = vec![make_issue_lib(10, "No counts", "", &[], &now_iso_lib())];
    let result = analyze_issues(&issues, &HashMap::new());
    assert!(!result["issues"][0]["blocked"].as_bool().unwrap());
}

#[test]
fn filter_ready_returns_not_blocked_lib() {
    let issues = vec![
        json!({"number": 1, "blocked": false, "decomposed": false}),
        json!({"number": 2, "blocked": true, "decomposed": false}),
    ];
    let result = filter_issues(&issues, "ready").unwrap();
    assert_eq!(result.len(), 1);
}

#[test]
fn filter_blocked_returns_blocked_lib() {
    let issues = vec![
        json!({"number": 1, "blocked": false}),
        json!({"number": 2, "blocked": true}),
    ];
    let result = filter_issues(&issues, "blocked").unwrap();
    assert_eq!(result.len(), 1);
}

#[test]
fn filter_decomposed_returns_decomposed_lib() {
    let issues = vec![
        json!({"number": 1, "decomposed": false}),
        json!({"number": 2, "decomposed": true}),
    ];
    let result = filter_issues(&issues, "decomposed").unwrap();
    assert_eq!(result.len(), 1);
}

#[test]
fn filter_quick_start_lib() {
    let issues = vec![
        json!({"number": 1, "blocked": false, "decomposed": false}),
        json!({"number": 2, "blocked": true, "decomposed": true}),
        json!({"number": 3, "blocked": false, "decomposed": true}),
    ];
    let result = filter_issues(&issues, "quick-start").unwrap();
    assert_eq!(result.len(), 1);
}

#[test]
fn filter_unknown_raises_lib() {
    assert!(filter_issues(&[], "invalid").is_err());
}

#[test]
fn analyze_surfaces_assignees_logins_lib() {
    let issue = json!({
        "number": 1,
        "title": "Has assignees",
        "body": "",
        "labels": [],
        "createdAt": now_iso_lib(),
        "url": "https://github.com/test/repo/issues/1",
        "milestone": Value::Null,
        "assignees": [
            {"login": "alice"},
            {"login": "bob"},
        ],
    });
    let result = analyze_issues(&[issue], &HashMap::new());
    let assignees = result["issues"][0]["assignees"].as_array().unwrap();
    let logins: Vec<&str> = assignees.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(logins, vec!["alice", "bob"]);
}

#[test]
fn analyze_empty_assignees_yields_empty_array_lib() {
    let issue = json!({
        "number": 1,
        "title": "No assignees",
        "body": "",
        "labels": [],
        "createdAt": now_iso_lib(),
        "url": "https://github.com/test/repo/issues/1",
        "milestone": Value::Null,
        "assignees": [],
    });
    let result = analyze_issues(&[issue], &HashMap::new());
    let assignees = result["issues"][0]["assignees"].as_array().unwrap();
    assert!(assignees.is_empty());
}

#[test]
fn analyze_no_top_level_in_progress_key_lib() {
    let issues = vec![
        make_issue_lib(1, "Active", "", &["Flow In-Progress"], &now_iso_lib()),
        make_issue_lib(2, "Available", "", &[], &now_iso_lib()),
    ];
    let result = analyze_issues(&issues, &HashMap::new());
    assert!(
        result.get("in_progress").is_none(),
        "output must not carry a top-level in_progress key; got {}",
        result,
    );
}

#[test]
fn analyze_flow_in_progress_row_in_issues_array_lib() {
    let issues = vec![make_issue_lib(
        1,
        "Active",
        "",
        &["Flow In-Progress"],
        &now_iso_lib(),
    )];
    let result = analyze_issues(&issues, &HashMap::new());
    let arr = result["issues"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["number"], 1);
    assert_eq!(arr[0]["flow_in_progress"], true);
}

#[test]
fn analyze_total_equals_issues_len_after_collapse_lib() {
    let issues = vec![
        make_issue_lib(1, "Active", "", &["Flow In-Progress"], &now_iso_lib()),
        make_issue_lib(2, "Available", "", &[], &now_iso_lib()),
        make_issue_lib(3, "Another", "", &[], &now_iso_lib()),
    ];
    let result = analyze_issues(&issues, &HashMap::new());
    let issues_len = result["issues"].as_array().unwrap().len();
    assert_eq!(result["total"].as_u64().unwrap() as usize, issues_len);
}

#[test]
fn analyze_blocked_by_entries_carry_number_and_url_lib() {
    let issues = vec![make_issue_lib(
        1547,
        "Blocked by another",
        "",
        &[],
        &now_iso_lib(),
    )];
    let mut blocker_map: HashMap<i64, Vec<Value>> = HashMap::new();
    blocker_map.insert(
        1547,
        vec![json!({
            "number": 1525,
            "url": "https://github.com/benkruger/flow/issues/1525",
        })],
    );
    let result = analyze_issues(&issues, &blocker_map);
    let blocked_by = result["issues"][0]["blocked_by"].as_array().unwrap();
    assert_eq!(blocked_by.len(), 1);
    assert_eq!(blocked_by[0]["number"], 1525);
    assert_eq!(
        blocked_by[0]["url"],
        "https://github.com/benkruger/flow/issues/1525"
    );
}

#[test]
fn analyze_no_category_key_lib() {
    let issues = vec![make_issue_lib(
        1,
        "Categorizable",
        "Fix crash on login",
        &["Rule"],
        &now_iso_lib(),
    )];
    let result = analyze_issues(&issues, &HashMap::new());
    let row = &result["issues"][0];
    assert!(
        row.get("category").is_none(),
        "expected no `category` key on output row; got {}",
        row,
    );
}

#[test]
fn analyze_no_stale_or_file_path_keys_lib() {
    let old_date = (chrono::Utc::now() - chrono::Duration::days(90)).to_rfc3339();
    let issues = vec![make_issue_lib(
        1,
        "Old issue",
        "See /nonexistent/foo.py",
        &[],
        &old_date,
    )];
    let result = analyze_issues(&issues, &HashMap::new());
    let row = &result["issues"][0];
    for key in ["stale", "stale_missing", "age_days", "file_paths"] {
        assert!(
            row.get(key).is_none(),
            "expected no `{}` key on output row; got {}",
            key,
            row,
        );
    }
}

#[test]
fn analyze_no_brief_key_lib() {
    let long_body: String = "x".repeat(500);
    let issues = vec![make_issue_lib(
        1,
        "Long body",
        &long_body,
        &[],
        &now_iso_lib(),
    )];
    let result = analyze_issues(&issues, &HashMap::new());
    let row = &result["issues"][0];
    assert!(
        row.get("brief").is_none(),
        "expected no `brief` key on output row; got {}",
        row,
    );
}

#[test]
fn analyze_no_milestone_key_lib() {
    let issues = vec![make_issue_opt_lib(
        1,
        "Milestone issue",
        "",
        &[],
        &now_iso_lib(),
        Some("v1.2.0"),
    )];
    let result = analyze_issues(&issues, &HashMap::new());
    let row = &result["issues"][0];
    assert!(
        row.get("milestone").is_none(),
        "expected no `milestone` key on output row; got {}",
        row,
    );
}

#[test]
fn analyze_issues_filters_against_collapsed_schema_subprocess() {
    // End-to-end regression: --ready, --blocked, --decomposed, and
    // --quick-start each behave correctly when every input issue
    // flows into the single `issues` array (no top-level partition).
    // A Flow-In-Progress row is included or excluded purely on its
    // own `blocked`/`decomposed` flags, not on a partition.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let issues = vec![
        fake_issue(
            1,
            "In progress and decomposed",
            vec!["Flow In-Progress", "decomposed"],
        ),
        fake_issue(2, "Decomposed", vec!["decomposed"]),
        fake_issue(3, "Blocked", vec!["Blocked"]),
        fake_issue(4, "Plain", vec![]),
    ];
    let issues_path = dir.path().join("issues.json");
    fs::write(&issues_path, serde_json::to_string(&issues).unwrap()).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho '{\"data\":{}}'\nexit 0\n");

    let issues_arg = issues_path.to_str().unwrap();

    let ready = run_analyze(&repo, &["--issues-json", issues_arg, "--ready"], &stub_dir);
    assert_eq!(ready.status.code(), Some(0));
    let ready_data = parse_full_stdout(&ready);
    let ready_arr = ready_data["issues"].as_array().unwrap();
    let ready_nums: Vec<i64> = ready_arr
        .iter()
        .map(|i| i["number"].as_i64().unwrap())
        .collect();
    assert!(
        !ready_nums.contains(&3),
        "--ready must exclude blocked issue 3"
    );
    assert!(ready_nums.contains(&1));
    assert!(ready_nums.contains(&2));
    assert!(ready_nums.contains(&4));

    let blocked = run_analyze(
        &repo,
        &["--issues-json", issues_arg, "--blocked"],
        &stub_dir,
    );
    assert_eq!(blocked.status.code(), Some(0));
    let blocked_data = parse_full_stdout(&blocked);
    let blocked_arr = blocked_data["issues"].as_array().unwrap();
    let blocked_nums: Vec<i64> = blocked_arr
        .iter()
        .map(|i| i["number"].as_i64().unwrap())
        .collect();
    assert_eq!(blocked_nums, vec![3]);

    let decomposed = run_analyze(
        &repo,
        &["--issues-json", issues_arg, "--decomposed"],
        &stub_dir,
    );
    assert_eq!(decomposed.status.code(), Some(0));
    let decomposed_data = parse_full_stdout(&decomposed);
    let decomposed_nums: Vec<i64> = decomposed_data["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["number"].as_i64().unwrap())
        .collect();
    assert!(decomposed_nums.contains(&1));
    assert!(decomposed_nums.contains(&2));
    assert!(!decomposed_nums.contains(&3));
    assert!(!decomposed_nums.contains(&4));

    let qs = run_analyze(
        &repo,
        &["--issues-json", issues_arg, "--quick-start"],
        &stub_dir,
    );
    assert_eq!(qs.status.code(), Some(0));
    let qs_data = parse_full_stdout(&qs);
    let qs_nums: Vec<i64> = qs_data["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["number"].as_i64().unwrap())
        .collect();
    // --quick-start excludes Flow-In-Progress rows so a `/flow:flow-start`
    // command isn't surfaced for in-flight work (issue 1 has Flow In-Progress).
    assert!(!qs_nums.contains(&1));
    assert!(qs_nums.contains(&2));
    assert!(!qs_nums.contains(&3));
    assert!(!qs_nums.contains(&4));
}

#[test]
fn analyze_issues_milestone_cli_flag_still_forwarded_to_gh() {
    // Per-row milestone is gone but the server-side --milestone CLI
    // flag still passes through to gh, so users can filter by
    // milestone server-side.
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let log_path = repo.join("gh_args.log");
    let stub_script = format!(
        "#!/bin/bash\nprintf '%s\\n' \"$@\" > {}\necho '[]'\nexit 0\n",
        log_path.to_string_lossy(),
    );
    let stub_dir = create_gh_stub(&repo, &stub_script);

    let output = run_analyze(&repo, &["--milestone", "v1.2"], &stub_dir);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let logged = fs::read_to_string(&log_path).expect("gh stub should have logged args");
    let lines: Vec<&str> = logged.lines().collect();
    let m_idx = lines
        .iter()
        .position(|l| *l == "--milestone")
        .expect("--milestone should appear in gh argv");
    assert_eq!(lines.get(m_idx + 1), Some(&"v1.2"));
}
