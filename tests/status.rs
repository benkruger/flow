//! Integration tests for `bin/flow status` — the presentation wrapper
//! around `format_status::run_impl_main` that adds a fenced-code panel
//! envelope.

use flow_rs::status::run_impl_main;
use serde_json::{json, Value};

mod common;

fn make_state(current_phase: &str, phase_statuses: &[(&str, &str)]) -> Value {
    let mut phases = serde_json::Map::new();
    let phase_names = flow_rs::phase_config::phase_names();
    let all_phases = [
        "flow-start",
        "flow-code",
        "flow-review",
        "flow-learn",
        "flow-complete",
    ];
    for &p in &all_phases {
        let status = phase_statuses
            .iter()
            .find(|(k, _)| *k == p)
            .map(|(_, v)| *v)
            .unwrap_or("pending");
        let name = phase_names.get(p).cloned().unwrap_or_default();
        phases.insert(
            p.to_string(),
            json!({
                "name": name,
                "status": status,
                "started_at": null,
                "completed_at": null,
                "session_started_at": null,
                "cumulative_seconds": 0,
                "visit_count": 0,
            }),
        );
    }

    json!({
        "schema_version": 1,
        "branch": "test-feature",
        "pr_url": "https://github.com/test/test/pull/1",
        "started_at": "2026-01-01T00:00:00-08:00",
        "current_phase": current_phase,
        "files": {
            "plan": "",
            "log": "",
            "state": ""
        },
        "notes": [],
        "phases": phases,
    })
}

fn write_state_file(root: &std::path::Path, branch: &str, state: &Value) {
    let branch_dir = root.join(".flow-states").join(branch);
    std::fs::create_dir_all(&branch_dir).unwrap();
    std::fs::write(branch_dir.join("state.json"), state.to_string()).unwrap();
}

// --- run_impl_main library-level tests ---

#[test]
fn status_run_impl_main_success_wraps_panel_with_fence() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let state = make_state("flow-start", &[("flow-start", "in_progress")]);
    write_state_file(&root, "only-feature", &state);

    let (text, code) = run_impl_main(Some("only-feature"), &root).expect("ok path");
    assert_eq!(code, 0);
    assert!(
        text.contains("```text"),
        "expected fenced text opener, got:\n{}",
        text
    );
    assert!(
        text.contains("Feature : Test Feature"),
        "expected wrapped panel content, got:\n{}",
        text
    );
    let fence_count = text.matches("```").count();
    assert!(
        fence_count >= 2,
        "expected at least two fence markers, got {} in:\n{}",
        fence_count,
        text
    );
}

#[test]
fn status_run_impl_main_no_state_returns_no_flow_message_exits_0() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();

    let (text, code) = run_impl_main(Some("absent-branch"), &root).expect("ok path");
    assert_eq!(code, 0);
    assert!(
        text.contains("No FLOW feature in progress"),
        "expected no-flow message on stdout, got:\n{}",
        text
    );
    assert!(
        text.contains("```text"),
        "expected fenced text opener, got:\n{}",
        text
    );
}

#[test]
fn status_run_impl_main_branch_resolution_err_renders_error_at_exit_0() {
    // When `format_status::run_impl_main` returns `Err((_, 2))` (no
    // git repo, no override → `resolve_branch` returns None), the
    // wrapper converts the error into a fenced "Status unavailable"
    // message at exit 0 so phase skills' bash blocks always surface
    // useful stdout content. Forces the Err arm by passing None and
    // a tempdir with no git repo and no `.flow-states/` entries.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    // Set GIT_CEILING_DIRECTORIES via env var unavailable in-process;
    // instead, ensure resolve_branch sees no git context by making
    // `root` a brand-new tempdir whose ancestors do not reach a git
    // repo via the in-process current_dir. The library-level call
    // operates on the explicit `root` argument for state-file
    // discovery (no state files → empty results) — but
    // resolve_branch still queries the host git context. The
    // subprocess test below covers the branch-resolution-Err path
    // through GIT_CEILING_DIRECTORIES; this in-process call may
    // exercise either branch depending on host state. Both branches
    // are valid: panel render OR no-flow message OR error-fenced
    // message. The assertion is that the wrapper never panics and
    // always returns Ok with fenced stdout.
    let result =
        run_impl_main(None, &root).expect("ok path — wrapper never returns Err for valid --branch");
    let (text, code) = result;
    assert_eq!(code, 0);
    assert!(
        text.contains("```text"),
        "expected fenced text envelope on every Ok path, got:\n{}",
        text
    );
}

#[test]
fn status_run_impl_main_with_branch_override_selects_named_branch() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut named = make_state("flow-code", &[("flow-code", "in_progress")]);
    named["branch"] = json!("named-target");
    write_state_file(&root, "named-target", &named);

    let mut other = make_state("flow-start", &[("flow-start", "in_progress")]);
    other["branch"] = json!("sibling-branch");
    write_state_file(&root, "sibling-branch", &other);

    let (text, code) = run_impl_main(Some("named-target"), &root).expect("ok path");
    assert_eq!(code, 0);
    assert!(
        text.contains("Branch  : named-target"),
        "expected named-target panel, got:\n{}",
        text
    );
    assert!(
        !text.contains("Multiple Features Active"),
        "expected single panel (not multi), got:\n{}",
        text
    );
}

#[test]
fn status_run_impl_main_multi_flow_wraps_multi_panel() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut a = make_state("flow-start", &[("flow-start", "in_progress")]);
    a["branch"] = json!("first-feature");
    let mut b = make_state(
        "flow-code",
        &[("flow-start", "complete"), ("flow-code", "in_progress")],
    );
    b["branch"] = json!("second-feature");
    write_state_file(&root, "first-feature", &a);
    write_state_file(&root, "second-feature", &b);

    let (text, code) = run_impl_main(Some("nonexistent"), &root).expect("ok path");
    assert_eq!(code, 0);
    assert!(
        text.contains("Multiple Features Active"),
        "expected multi-panel header, got:\n{}",
        text
    );
    assert!(
        text.contains("```text"),
        "expected fenced text opener around multi-panel, got:\n{}",
        text
    );
}

// --- Empty-panel / corrupted-state regression (F10, F11) ---

#[test]
fn status_run_impl_main_null_phases_returns_no_flow_message_not_empty_fence() {
    // format_status::format_panel returns String::new() when
    // state["phases"] is null or non-object. Without the empty-panel
    // guard, the wrapper would emit a fenced block with no content.
    // The wrapper now treats empty-panel as no-state and surfaces
    // the no-flow message.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("corrupted");
    std::fs::create_dir_all(&branch_dir).unwrap();
    let body = json!({
        "schema_version": 1,
        "branch": "corrupted",
        "current_phase": "flow-start",
        "phases": null,
    });
    std::fs::write(branch_dir.join("state.json"), body.to_string()).unwrap();

    let (text, code) = run_impl_main(Some("corrupted"), &root).expect("ok path");
    assert_eq!(code, 0);
    assert!(
        text.contains("No FLOW feature in progress"),
        "expected no-flow message for null phases, got:\n{}",
        text
    );
}

#[test]
fn status_run_impl_main_string_phases_returns_no_flow_message_not_empty_fence() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch_dir = root.join(".flow-states").join("corrupted");
    std::fs::create_dir_all(&branch_dir).unwrap();
    let body = json!({
        "schema_version": 1,
        "branch": "corrupted",
        "current_phase": "flow-start",
        "phases": "not-an-object",
    });
    std::fs::write(branch_dir.join("state.json"), body.to_string()).unwrap();

    let (text, code) = run_impl_main(Some("corrupted"), &root).expect("ok path");
    assert_eq!(code, 0);
    assert!(
        text.contains("No FLOW feature in progress"),
        "expected no-flow message for non-object phases, got:\n{}",
        text
    );
}

// --- Branch override validation (F4, F5) ---

#[test]
fn status_run_impl_main_rejects_empty_branch_override() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let result = run_impl_main(Some(""), &root);
    let (msg, code) = result.expect_err("empty --branch must reject");
    assert_eq!(code, 1);
    assert!(
        msg.contains("Invalid --branch"),
        "expected invalid-branch error message, got: {}",
        msg
    );
}

#[test]
fn status_run_impl_main_rejects_dot_branch_override() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let result = run_impl_main(Some("."), &root);
    let (_, code) = result.expect_err("'.' --branch must reject");
    assert_eq!(code, 1);
}

#[test]
fn status_run_impl_main_rejects_dotdot_branch_override() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let result = run_impl_main(Some(".."), &root);
    let (_, code) = result.expect_err("'..' --branch must reject");
    assert_eq!(code, 1);
}

#[test]
fn status_run_impl_main_rejects_slash_branch_override() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let result = run_impl_main(Some("feature/foo"), &root);
    let (_, code) = result.expect_err("slash-containing --branch must reject");
    assert_eq!(code, 1);
}

#[test]
fn status_run_impl_main_rejects_nul_byte_branch_override() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let result = run_impl_main(Some("foo\0bar"), &root);
    let (_, code) = result.expect_err("NUL-byte --branch must reject");
    assert_eq!(code, 1);
}

// --- Subprocess tests ---

#[test]
fn status_subprocess_exits_0_with_valid_state() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let state = make_state("flow-start", &[("flow-start", "in_progress")]);
    write_state_file(&root, "test-feature", &state);

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["status", "--branch", "test-feature"])
        .current_dir(&root)
        .env_remove("FLOW_CI_RUNNING")
        .env("GIT_CEILING_DIRECTORIES", &root)
        .output()
        .expect("spawn flow-rs status");
    assert_eq!(
        output.status.code(),
        Some(0),
        "expected exit 0 with valid state, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("```text"),
        "expected fenced text envelope, got: {}",
        stdout
    );
    assert!(
        stdout.contains("Current Status"),
        "expected inner panel header, got: {}",
        stdout
    );
}

#[test]
fn status_subprocess_no_state_exits_0_emits_no_flow_message() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::process::Command::new("git")
        .args(["init", "-b", "fresh"])
        .current_dir(&root)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&root)
        .output()
        .unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("status")
        .current_dir(&root)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("spawn flow-rs status");
    assert_eq!(
        output.status.code(),
        Some(0),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No FLOW feature in progress"),
        "expected no-flow message on stdout, got: {}",
        stdout
    );
}

#[test]
fn status_subprocess_branch_resolution_err_renders_error_on_stdout() {
    // Phase skills' bash blocks print stdout verbatim. The binary
    // converts branch-resolution failure into a fenced "Status
    // unavailable" message on stdout at exit 0 so the bash block
    // surfaces a useful notice instead of an empty fence.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("status")
        .current_dir(&root)
        .env_remove("FLOW_CI_RUNNING")
        .env("GIT_CEILING_DIRECTORIES", &root)
        .output()
        .expect("spawn flow-rs status");
    assert_eq!(
        output.status.code(),
        Some(0),
        "expected exit 0 (error hoisted to stdout), stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Status unavailable"),
        "expected fenced error message on stdout, got: {}",
        stdout
    );
    assert!(
        stdout.contains("```text"),
        "expected fenced text envelope around error, got: {}",
        stdout
    );
}

#[test]
fn status_subprocess_invalid_branch_override_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["status", "--branch", ".."])
        .current_dir(&root)
        .env_remove("FLOW_CI_RUNNING")
        .env("GIT_CEILING_DIRECTORIES", &root)
        .output()
        .expect("spawn flow-rs status");
    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit 1 for invalid --branch, stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Invalid --branch"),
        "expected invalid-branch error on stderr, got: {}",
        stderr
    );
}
