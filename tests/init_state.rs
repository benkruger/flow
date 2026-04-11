use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use serde_json::{json, Value};

fn flow_rs() -> Command {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
}

fn setup_project(dir: &std::path::Path, framework: &str, skills: Option<Value>) {
    // Init git repo (needed for project_root())
    Command::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir)
        .output()
        .unwrap();

    // Write .flow.json
    let mut data = json!({"flow_version": "1.1.0", "framework": framework});
    if let Some(s) = skills {
        data["skills"] = s;
    }
    fs::write(
        dir.join(".flow.json"),
        serde_json::to_string(&data).unwrap(),
    )
    .unwrap();

    // Copy flow-phases.json so freeze_phases can find it
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let phases_src = std::path::PathBuf::from(manifest_dir).join("flow-phases.json");
    fs::copy(&phases_src, dir.join("flow-phases.json")).unwrap();
}

fn run_init_state(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    flow_rs()
        .arg("init-state")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

fn parse_stdout(output: &std::process::Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "Failed to parse JSON: {}\nstdout: {}\nstderr: {}",
            e,
            stdout,
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn read_state_file(dir: &std::path::Path, branch: &str) -> Value {
    let path = dir.join(".flow-states").join(format!("{}.json", branch));
    let content = fs::read_to_string(&path).unwrap();
    serde_json::from_str(&content).unwrap()
}

// --- Happy path ---

#[test]
fn happy_path_returns_ok_json() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);
    let output = run_init_state(dir.path(), &["test feature"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_stdout(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["branch"], "test-feature");
    assert_eq!(data["state_file"], ".flow-states/test-feature.json");
}

// --- State file fields ---

#[test]
fn state_file_has_null_pr_fields() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);
    run_init_state(dir.path(), &["pr null test"]);
    let state = read_state_file(dir.path(), "pr-null-test");
    assert!(state["pr_number"].is_null());
    assert!(state["pr_url"].is_null());
    assert!(state["repo"].is_null());
}

#[test]
fn state_file_has_all_6_phases() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);
    run_init_state(dir.path(), &["six phases test"]);
    let state = read_state_file(dir.path(), "six-phases-test");
    let phases = state["phases"].as_object().unwrap();
    assert_eq!(phases.len(), 6);
    assert_eq!(phases["flow-start"]["name"], "Start");
    assert_eq!(phases["flow-code-review"]["name"], "Code Review");
}

#[test]
fn state_file_phase_1_in_progress() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);
    run_init_state(dir.path(), &["phase status test"]);
    let state = read_state_file(dir.path(), "phase-status-test");
    let start = &state["phases"]["flow-start"];
    assert_eq!(start["status"], "in_progress");
    assert!(start["started_at"].is_string());
    assert!(start["session_started_at"].is_string());
    assert_eq!(start["visit_count"], 1);
}

#[test]
fn state_file_other_phases_pending() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);
    run_init_state(dir.path(), &["pending phases test"]);
    let state = read_state_file(dir.path(), "pending-phases-test");
    for key in [
        "flow-plan",
        "flow-code",
        "flow-code-review",
        "flow-learn",
        "flow-complete",
    ] {
        let phase = &state["phases"][key];
        assert_eq!(phase["status"], "pending");
        assert!(phase["started_at"].is_null());
        assert_eq!(phase["visit_count"], 0);
    }
}

// --- Subdirectory scope (relative_cwd) ---

#[test]
fn relative_cwd_persisted_to_state_file() {
    // When --relative-cwd is passed, init-state writes it to the state file.
    // start-init computes this from cwd.strip_prefix(project_root()) at
    // flow-start time and forwards it via this flag.
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);
    let output = run_init_state(dir.path(), &["subdir test", "--relative-cwd", "api"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let state = read_state_file(dir.path(), "subdir-test");
    assert_eq!(state["relative_cwd"], "api");
}

#[test]
fn relative_cwd_defaults_to_empty_when_flag_omitted() {
    // Backwards-compatible default: when --relative-cwd is omitted,
    // the state file gets an empty string. Existing flow-start callers
    // that don't yet pass the flag continue to work.
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);
    run_init_state(dir.path(), &["empty rel test"]);
    let state = read_state_file(dir.path(), "empty-rel-test");
    assert_eq!(state["relative_cwd"], "");
}

#[test]
fn relative_cwd_supports_nested_paths() {
    // Mono-repos with nested package layouts (e.g. packages/api) need
    // multi-segment relative paths. The flag passes them through verbatim.
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);
    let output = run_init_state(
        dir.path(),
        &["nested test", "--relative-cwd", "packages/api"],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let state = read_state_file(dir.path(), "nested-test");
    assert_eq!(state["relative_cwd"], "packages/api");
}

// --- Framework ---

#[test]
fn framework_from_flow_json() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "python", None);
    run_init_state(dir.path(), &["python framework"]);
    let state = read_state_file(dir.path(), "python-framework");
    assert_eq!(state["framework"], "python");
}

// --- Skills ---

#[test]
fn skills_from_flow_json() {
    let dir = tempfile::tempdir().unwrap();
    let skills = json!({"flow-start": {"continue": "manual"}});
    setup_project(dir.path(), "rails", Some(skills));
    run_init_state(dir.path(), &["skills config"]);
    let state = read_state_file(dir.path(), "skills-config");
    assert_eq!(state["skills"]["flow-start"]["continue"], "manual");
}

#[test]
fn skills_omitted_when_not_in_flow_json() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);
    run_init_state(dir.path(), &["no skills"]);
    let state = read_state_file(dir.path(), "no-skills");
    assert!(state.get("skills").is_none());
}

#[test]
fn auto_flag_overrides_skills() {
    let dir = tempfile::tempdir().unwrap();
    let skills = json!({"flow-start": {"continue": "manual"}});
    setup_project(dir.path(), "rails", Some(skills));
    run_init_state(dir.path(), &["auto override", "--auto"]);
    let state = read_state_file(dir.path(), "auto-override");
    assert_eq!(state["skills"]["flow-start"]["continue"], "auto");
    assert_eq!(state["skills"]["flow-code"]["commit"], "auto");
    assert_eq!(state["skills"]["flow-code-review"]["commit"], "auto");
}

// --- Prompt ---

#[test]
fn prompt_from_prompt_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);
    let prompt_path = dir.path().join(".flow-states");
    fs::create_dir_all(&prompt_path).unwrap();
    let prompt_file = prompt_path.join("test-prompt-file");
    fs::write(&prompt_file, "fix login timeout with special chars: && | ;").unwrap();
    let output = run_init_state(
        dir.path(),
        &[
            "prompt file test",
            "--prompt-file",
            prompt_file.to_str().unwrap(),
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let state = read_state_file(dir.path(), "prompt-file-test");
    assert_eq!(
        state["prompt"],
        "fix login timeout with special chars: && | ;"
    );
    assert!(
        !prompt_file.exists(),
        "Prompt file should be deleted after read"
    );
}

#[test]
fn prompt_defaults_to_feature_name() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);
    run_init_state(dir.path(), &["default prompt"]);
    let state = read_state_file(dir.path(), "default-prompt");
    assert_eq!(state["prompt"], "default prompt");
}

// --- Error cases ---

#[test]
fn missing_feature_name_fails() {
    let output = flow_rs().arg("init-state").output().unwrap();
    assert_ne!(output.status.code(), Some(0));
}

#[test]
fn missing_flow_json_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    // Init git but no .flow.json
    Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let output = run_init_state(dir.path(), &["no flow json"]);
    assert_ne!(output.status.code(), Some(0));
    let data = parse_stdout(&output);
    assert_eq!(data["status"], "error");
}

// --- Branch name derivation ---

#[test]
fn branch_name_derived_from_feature() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);
    let output = run_init_state(dir.path(), &["Invoice Pdf Export"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_stdout(&output);
    assert_eq!(data["branch"], "invoice-pdf-export");
}

#[test]
fn branch_name_truncated_at_32() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);
    let output = run_init_state(
        dir.path(),
        &["this is a very long feature name that exceeds limit"],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_stdout(&output);
    let branch = data["branch"].as_str().unwrap();
    assert!(
        branch.chars().count() <= 32,
        "Branch too long: {} ({} chars)",
        branch,
        branch.chars().count()
    );
    assert!(!branch.ends_with('-'));
}

// --- Start step tracking ---

#[test]
fn start_step_fields_set_when_flags_passed() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);
    let output = run_init_state(
        dir.path(),
        &[
            "step tracking test",
            "--start-step",
            "3",
            "--start-steps-total",
            "11",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let state = read_state_file(dir.path(), "step-tracking-test");
    assert_eq!(state["start_step"], 3);
    assert_eq!(state["start_steps_total"], 11);
}

#[test]
fn start_step_fields_absent_when_flags_omitted() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);
    run_init_state(dir.path(), &["no step fields"]);
    let state = read_state_file(dir.path(), "no-step-fields");
    assert!(state.get("start_step").is_none());
    assert!(state.get("start_steps_total").is_none());
}

// --- Log file ---

#[test]
fn log_file_created() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);
    run_init_state(dir.path(), &["log test"]);
    let log_path = dir.path().join(".flow-states").join("log-test.log");
    assert!(log_path.exists());
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains("[Phase 1]"));
}

// --- Frozen phases file ---

#[test]
fn frozen_phases_file_created() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);
    run_init_state(dir.path(), &["frozen phases"]);
    let frozen = dir
        .path()
        .join(".flow-states")
        .join("frozen-phases-phases.json");
    assert!(frozen.exists());
}

#[test]
fn frozen_phases_file_matches_source() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);
    run_init_state(dir.path(), &["phases match"]);
    let frozen = dir
        .path()
        .join(".flow-states")
        .join("phases-match-phases.json");
    let frozen_data: Value = serde_json::from_str(&fs::read_to_string(&frozen).unwrap()).unwrap();
    let source_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("flow-phases.json");
    let source_data: Value =
        serde_json::from_str(&fs::read_to_string(&source_path).unwrap()).unwrap();
    assert_eq!(frozen_data, source_data);
}

// --- Files block ---

#[test]
fn state_file_has_files_block() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);
    run_init_state(dir.path(), &["files block test"]);
    let state = read_state_file(dir.path(), "files-block-test");
    let files = &state["files"];
    assert!(files["plan"].is_null());
    assert!(files["dag"].is_null());
    assert_eq!(files["log"], ".flow-states/files-block-test.log");
    assert_eq!(files["state"], ".flow-states/files-block-test.json");
}

// --- Required top-level fields ---

#[test]
fn state_file_has_required_top_level_fields() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);
    run_init_state(dir.path(), &["fields test"]);
    let state = read_state_file(dir.path(), "fields-test");
    assert_eq!(state["schema_version"], 1);
    assert_eq!(state["branch"], "fields-test");
    assert_eq!(state["current_phase"], "flow-start");
    assert_eq!(state["notes"], json!([]));
    assert_eq!(state["phase_transitions"], json!([]));
    assert!(state["session_tty"].is_null() || state["session_tty"].is_string());
    assert!(state["session_id"].is_null());
    assert!(state["transcript_path"].is_null());
}

// --- Issue-title naming and duplicate detection (PR #823) ---

#[test]
fn fetch_issue_title_failure_returns_error() {
    // When prompt contains #N and gh is not available, init_state should
    // return a hard error instead of silently falling back to feature_name.
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);

    let prompt_path = dir.path().join(".flow-states");
    fs::create_dir_all(&prompt_path).unwrap();
    let prompt_file = prompt_path.join("test-prompt");
    fs::write(&prompt_file, "work on issue #999").unwrap();

    // Run with empty PATH so gh cannot be found
    let output = flow_rs()
        .arg("init-state")
        .args([
            "fetch failure test",
            "--prompt-file",
            prompt_file.to_str().unwrap(),
        ])
        .current_dir(dir.path())
        .env("PATH", "")
        .output()
        .unwrap();

    assert_ne!(
        output.status.code(),
        Some(0),
        "Should fail when fetch_issue_title cannot reach GitHub"
    );
    let data = parse_stdout(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["step"], "fetch_issue_title");

    // No state file should be created
    let state_path = dir
        .path()
        .join(".flow-states")
        .join("fetch-failure-test.json");
    assert!(
        !state_path.exists(),
        "State file should not be created when fetch fails"
    );
}

#[test]
fn duplicate_issue_detected_before_state_creation() {
    // When an existing state file references the same issue, init_state should
    // exit with duplicate_issue error before creating a new state file.
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);

    // Pre-create an existing state file referencing issue #777
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join("existing-flow.json"),
        serde_json::json!({
            "prompt": "work on issue #777",
            "branch": "existing-flow",
            "current_phase": "flow-code",
            "pr_url": "https://github.com/test/repo/pull/50",
        })
        .to_string(),
    )
    .unwrap();

    // Run init_state with a prompt that also references #777.
    // The gh stub must emit the JSON shape that fetch_issue_info now parses
    // (issue #887): `{title, labels}`. An empty labels array bypasses the
    // Flow In-Progress guard so the test still reaches the duplicate-issue
    // guard, which is what this test is exercising.
    let stub_dir = write_gh_stub(dir.path(), r#"{"title": "Some Issue Title", "labels": []}"#);
    let prompt_file = write_prompt_file(&state_dir, "work on issue #777");

    let output = flow_rs()
        .arg("init-state")
        .args(["dup test", "--prompt-file", prompt_file.to_str().unwrap()])
        .current_dir(dir.path())
        .env("PATH", format!("{}:/usr/bin:/bin", stub_dir.display()))
        .output()
        .unwrap();

    assert_ne!(
        output.status.code(),
        Some(0),
        "Should fail on duplicate issue"
    );
    let data = parse_stdout(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["step"], "duplicate_issue");
    let msg = data["message"].as_str().unwrap();
    assert!(
        msg.contains("existing-flow"),
        "Error should reference the existing branch"
    );
}

// --- Flow In-Progress label guard (issue #887) ---

fn write_gh_stub(dir: &std::path::Path, json_body: &str) -> std::path::PathBuf {
    let stub_dir = dir.join("stubs");
    fs::create_dir_all(&stub_dir).unwrap();
    let stub_path = stub_dir.join("gh");
    // Escape single quotes in the JSON body so the bash echo does not break
    // on titles like "It's broken". The '\\'' idiom ends the single-quoted
    // string, inserts a literal quote, and reopens the single-quoted string.
    let escaped = json_body.replace('\'', "'\\''");
    let script = format!("#!/bin/bash\necho '{}'\n", escaped);
    fs::write(&stub_path, script).unwrap();
    let mut perms = fs::metadata(&stub_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&stub_path, perms).unwrap();
    stub_dir
}

fn write_prompt_file(state_dir: &std::path::Path, body: &str) -> std::path::PathBuf {
    fs::create_dir_all(state_dir).unwrap();
    let prompt_file = state_dir.join("test-prompt");
    fs::write(&prompt_file, body).unwrap();
    prompt_file
}

#[test]
fn flow_in_progress_label_blocks_start() {
    // When a referenced issue carries the Flow In-Progress label, init-state
    // must exit with status=error, step=flow_in_progress_label, and NOT
    // create a state file. The error message must name the issue number, name
    // the label, and direct the user to resume the existing flow in its
    // worktree. Issue #887.
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);

    let stub_dir = write_gh_stub(
        dir.path(),
        r#"{"title": "Some Issue", "labels": ["Flow In-Progress"]}"#,
    );
    let prompt_file = write_prompt_file(&dir.path().join(".flow-states"), "work on issue #100");

    let output = flow_rs()
        .arg("init-state")
        .args([
            "label blocks test",
            "--prompt-file",
            prompt_file.to_str().unwrap(),
        ])
        .current_dir(dir.path())
        .env("PATH", format!("{}:/usr/bin:/bin", stub_dir.display()))
        .output()
        .unwrap();

    assert_ne!(
        output.status.code(),
        Some(0),
        "should fail when label is present"
    );
    let data = parse_stdout(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["step"], "flow_in_progress_label");
    let msg = data["message"].as_str().unwrap();
    assert!(
        msg.contains("#100"),
        "message should name the issue: {}",
        msg
    );
    assert!(
        msg.contains("Flow In-Progress"),
        "message should name the label so users can search for it: {}",
        msg
    );
    assert!(
        msg.contains("Resume the existing flow"),
        "message should direct the user to resume the existing flow: {}",
        msg
    );

    // No state file should be created when the guard fires
    let state_path = dir.path().join(".flow-states").join("some-issue.json");
    assert!(
        !state_path.exists(),
        "state file must not be created when label guard fires"
    );
}

#[test]
fn flow_in_progress_label_absent_allows_start() {
    // When the referenced issue has no Flow In-Progress label, init-state
    // proceeds normally and creates the state file with the branch derived
    // from the issue title.
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);

    let stub_dir = write_gh_stub(dir.path(), r#"{"title": "Some Issue", "labels": []}"#);
    let prompt_file = write_prompt_file(&dir.path().join(".flow-states"), "work on issue #100");

    let output = flow_rs()
        .arg("init-state")
        .args([
            "label absent test",
            "--prompt-file",
            prompt_file.to_str().unwrap(),
        ])
        .current_dir(dir.path())
        .env("PATH", format!("{}:/usr/bin:/bin", stub_dir.display()))
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_stdout(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["branch"], "some-issue");

    let state_path = dir.path().join(".flow-states").join("some-issue.json");
    assert!(
        state_path.exists(),
        "state file should be created when label is absent"
    );
}

#[test]
fn flow_in_progress_label_case_sensitive_match() {
    // The label guard uses exact string comparison. A lowercase label must
    // NOT block — the canonical label is "Flow In-Progress" byte-for-byte.
    // This test locks the semantic against an accidental .to_lowercase()
    // refactor.
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);

    let stub_dir = write_gh_stub(
        dir.path(),
        r#"{"title": "Some Issue", "labels": ["flow in-progress"]}"#,
    );
    let prompt_file = write_prompt_file(&dir.path().join(".flow-states"), "work on issue #100");

    let output = flow_rs()
        .arg("init-state")
        .args([
            "case sensitive test",
            "--prompt-file",
            prompt_file.to_str().unwrap(),
        ])
        .current_dir(dir.path())
        .env("PATH", format!("{}:/usr/bin:/bin", stub_dir.display()))
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "lowercase label must NOT block; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_stdout(&output);
    assert_eq!(data["status"], "ok");
}

#[test]
fn flow_in_progress_label_with_other_labels() {
    // The guard must use .any() semantics — a label array containing
    // "Flow In-Progress" alongside other labels must still block.
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);

    let stub_dir = write_gh_stub(
        dir.path(),
        r#"{"title": "Multi Label", "labels": ["bug", "Flow In-Progress", "decomposed"]}"#,
    );
    let prompt_file = write_prompt_file(&dir.path().join(".flow-states"), "work on issue #100");

    let output = flow_rs()
        .arg("init-state")
        .args([
            "multi label test",
            "--prompt-file",
            prompt_file.to_str().unwrap(),
        ])
        .current_dir(dir.path())
        .env("PATH", format!("{}:/usr/bin:/bin", stub_dir.display()))
        .output()
        .unwrap();

    assert_ne!(
        output.status.code(),
        Some(0),
        "should block when label is among others"
    );
    let data = parse_stdout(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["step"], "flow_in_progress_label");
}

#[test]
fn flow_in_progress_label_checked_before_duplicate_issue() {
    // Ordering invariant: the label guard fires BEFORE check_duplicate_issue.
    // When both conditions are true (label present AND a local state file
    // already targets the same issue), the reported step must be
    // flow_in_progress_label, not duplicate_issue. The label is the broader
    // (cross-machine) guard and must catch the conflict first.
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "rails", None);

    // Pre-create a local state file targeting the same issue
    let state_dir = dir.path().join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join("existing-flow.json"),
        serde_json::json!({
            "prompt": "work on issue #100",
            "branch": "existing-flow",
            "current_phase": "flow-code",
            "pr_url": "https://github.com/test/repo/pull/50",
        })
        .to_string(),
    )
    .unwrap();

    // gh stub returns the label
    let stub_dir = write_gh_stub(
        dir.path(),
        r#"{"title": "Ordering Test", "labels": ["Flow In-Progress"]}"#,
    );
    let prompt_file = write_prompt_file(&state_dir, "work on issue #100");

    let output = flow_rs()
        .arg("init-state")
        .args([
            "ordering test",
            "--prompt-file",
            prompt_file.to_str().unwrap(),
        ])
        .current_dir(dir.path())
        .env("PATH", format!("{}:/usr/bin:/bin", stub_dir.display()))
        .output()
        .unwrap();

    assert_ne!(output.status.code(), Some(0));
    let data = parse_stdout(&output);
    assert_eq!(
        data["step"], "flow_in_progress_label",
        "label guard must run before duplicate_issue guard"
    );
}

// --- --branch override tests ---

#[test]
fn branch_override_skips_derivation() {
    // When --branch is provided, init-state uses it directly without
    // running issue extraction or branch_name derivation.
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "python", None);

    let output = run_init_state(
        dir.path(),
        &["ignored-feature-name", "--branch", "my-custom-branch"],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let data = parse_stdout(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(
        data["branch"], "my-custom-branch",
        "Branch must match --branch override, not derived from feature name"
    );
    assert_eq!(
        data["state_file"], ".flow-states/my-custom-branch.json",
        "State file path must use the overridden branch name"
    );

    // State file must exist under the overridden name
    let state = read_state_file(dir.path(), "my-custom-branch");
    assert_eq!(state["branch"], "my-custom-branch");
}

#[test]
fn branch_override_does_not_call_fetch_issue_info() {
    // When --branch is provided AND the prompt contains #N references,
    // init-state must NOT call fetch_issue_info (which would fail without
    // a gh stub). This verifies the skip path works end-to-end.
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path(), "python", None);

    // Write a prompt with issue reference — without --branch this would
    // try to call `gh issue view` and fail (no gh stub in PATH)
    let prompt_path = dir.path().join("prompt-with-issue");
    fs::write(&prompt_path, "fix bug in #999").unwrap();

    let output = run_init_state(
        dir.path(),
        &[
            "ignored",
            "--branch",
            "pre-derived-branch",
            "--prompt-file",
            prompt_path.to_str().unwrap(),
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "With --branch, issue references in prompt must be ignored.\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let data = parse_stdout(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["branch"], "pre-derived-branch");
}
