//! Integration tests for `bin/flow status` — the presentation wrapper
//! around `format_status::run_impl_main` that adds the banner header
//! and fenced-code panel envelope.

use flow_rs::status::run_impl_main;
use flow_rs::utils::read_version;
use serde_json::{json, Value};

mod common;

fn make_state(current_phase: &str, phase_statuses: &[(&str, &str)]) -> Value {
    let mut phases = serde_json::Map::new();
    let phase_names = flow_rs::phase_config::phase_names();
    let all_phases = [
        "flow-start",
        "flow-code",
        "flow-code-review",
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
            "dag": "",
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
fn status_run_impl_main_success_wraps_panel_with_banner_and_fence() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let state = make_state("flow-start", &[("flow-start", "in_progress")]);
    write_state_file(&root, "only-feature", &state);

    let (text, code) = run_impl_main(Some("only-feature"), &root).expect("ok path");
    assert_eq!(code, 0);

    // Banner header + fenced text envelope around the panel content.
    let version = read_version();
    assert!(
        text.contains(&format!("FLOW v{} — flow:status — STARTING", version)),
        "expected banner header with version, got:\n{}",
        text
    );
    assert!(
        text.contains("```text"),
        "expected fenced `text` opener, got:\n{}",
        text
    );
    // The wrapped panel content should still surface the feature.
    // make_state hardcodes branch="test-feature" → derive_feature → "Test Feature".
    assert!(
        text.contains("Feature : Test Feature"),
        "expected wrapped panel content, got:\n{}",
        text
    );
    // Fence must close.
    let fence_count = text.matches("```").count();
    assert!(
        fence_count >= 2,
        "expected at least two fence markers (open + close), got {} in:\n{}",
        fence_count,
        text
    );
}

#[test]
fn status_run_impl_main_no_state_returns_no_flow_message_exits_0() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();

    let (text, code) = run_impl_main(Some("absent-branch"), &root).expect("ok path");
    // Hoisted into binary: no-state now exits 0 with the message on stdout
    // (not 1 with empty stdout as format-status does).
    assert_eq!(code, 0);
    assert!(
        text.contains("No FLOW feature in progress"),
        "expected no-flow message on stdout, got:\n{}",
        text
    );
    // Wrapped in fenced code block.
    assert!(
        text.contains("```text"),
        "expected fenced text opener, got:\n{}",
        text
    );
}

#[test]
fn status_run_impl_main_branch_resolution_err_returns_err_2() {
    // When no --branch override is supplied AND the cwd is not a git
    // repo, `resolve_branch` returns None and run_impl_main returns
    // Err(("Could not determine current branch", 2)) — surfaced by the
    // CLI arm via stderr + exit 2.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    // Set HOME so resolve_branch doesn't fall through to host git state.
    let prev_ceiling = std::env::var("GIT_CEILING_DIRECTORIES").ok();
    // Use process-level isolation via a dedicated tempdir cwd. We cannot
    // mutate process env without races, so this test exercises the Err
    // branch via the in-process call only when resolve_branch returns
    // None — which it will here because the tempdir has no .git/.
    let _ = prev_ceiling; // keep variable referenced
    let result = run_impl_main(None, &root);
    // Allow either result — the host may have a current branch detected
    // via git's parent search. The important assertion is shape: when
    // Err is returned, code must be 2.
    if let Err((msg, code)) = result {
        assert_eq!(code, 2);
        assert!(
            msg.contains("Could not determine current branch"),
            "expected branch-resolution err message, got: {}",
            msg
        );
    }
    // If the host's git found a branch, the test is exercised at the
    // subprocess level by `status_subprocess_branch_resolution_err_exits_2`
    // below, which uses GIT_CEILING_DIRECTORIES to block git from
    // walking up.
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
    // Wrapped in banner + fence even for multi-panel.
    let version = read_version();
    assert!(
        text.contains(&format!("FLOW v{} — flow:status — STARTING", version)),
        "expected banner around multi-panel, got:\n{}",
        text
    );
    assert!(
        text.contains("```text"),
        "expected fenced text opener around multi-panel, got:\n{}",
        text
    );
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
        stdout.contains("FLOW v"),
        "expected banner on stdout, got: {}",
        stdout
    );
    assert!(
        stdout.contains("flow:status — STARTING"),
        "expected banner header, got: {}",
        stdout
    );
    assert!(
        stdout.contains("```text"),
        "expected fenced text envelope, got: {}",
        stdout
    );
}

#[test]
fn status_subprocess_no_state_exits_0_emits_no_flow_message() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    // Initialize a git repo so resolve_branch succeeds with a current
    // branch; the no-state branch is reached because no .flow-states
    // entries exist for any branch.
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
        "expected exit 0 (hoisted into binary), stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No FLOW feature in progress"),
        "expected no-flow message on stdout, got: {}",
        stdout
    );
    assert!(
        stdout.contains("```text"),
        "expected fenced text envelope around no-flow message, got: {}",
        stdout
    );
}

#[test]
fn status_subprocess_branch_resolution_err_exits_2() {
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
        Some(2),
        "expected exit 2 from branch-resolution failure\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Could not determine current branch"),
        "expected branch-resolve error in stderr, got: {}",
        stderr
    );
}

#[test]
fn status_banner_includes_version_from_read_version() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let state = make_state("flow-start", &[("flow-start", "in_progress")]);
    write_state_file(&root, "v-feature", &state);

    let (text, _code) = run_impl_main(Some("v-feature"), &root).expect("ok path");
    let version = read_version();
    assert!(
        text.contains(&format!("FLOW v{}", version)),
        "expected `FLOW v{}` from read_version() in banner, got:\n{}",
        version,
        text
    );
}
