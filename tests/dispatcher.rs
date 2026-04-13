mod common;

use std::process::Command;

use common::flow_states_dir;

// --- generate-id ---

#[test]
fn generate_id_exits_0() {
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("generate-id")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "generate-id should exit 0");
}

#[test]
fn generate_id_stdout_is_8_char_hex() {
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("generate-id")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(stdout.len(), 8, "Expected 8 chars, got: {}", stdout);
    assert!(
        stdout
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "Not valid lowercase hex: {}",
        stdout
    );
}

// --- log ---

#[test]
fn log_exits_0_and_writes_file() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = flow_states_dir(dir.path());
    std::fs::create_dir(&state_dir).unwrap();

    // Initialize a git repo so project_root() works
    Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["log", "test-branch", "[Phase 1] Step 5 — test (exit 0)"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0), "log should exit 0");

    let log_file = state_dir.join("test-branch.log");
    assert!(log_file.exists(), "Log file should exist");
    let content = std::fs::read_to_string(&log_file).unwrap();
    assert!(
        content.contains("[Phase 1] Step 5 — test (exit 0)"),
        "Log should contain message. Got: {}",
        content
    );
}

#[test]
fn log_missing_args_exits_nonzero() {
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("log")
        .output()
        .unwrap();
    assert_ne!(
        output.status.code(),
        Some(0),
        "log with missing args should exit non-zero"
    );
}

// --- unknown subcommand ---

#[test]
fn unknown_subcommand_exits_127() {
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("nonexistent-command")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(127),
        "Unknown subcommand should exit 127 for hybrid dispatcher fallback"
    );
}

#[test]
fn unknown_subcommand_no_stdout() {
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("nonexistent-command")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.is_empty(),
        "Unknown subcommand must produce no stdout — callers parse stdout for JSON results, so any extra output here would corrupt the result. Got: {:?}",
        stdout
    );
}

#[test]
fn no_subcommand_exits_1() {
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "No subcommand should exit 1");
}

// --- format-status ---

#[test]
fn format_status_no_state_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    // Set branch name
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("format-status")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "format-status with no state file should exit 1"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.is_empty(), "No stdout expected. Got: {}", stdout);
}

/// Regression guard: `format-status` must not panic when the user is
/// on a slash-containing git branch. Before `FlowPaths::try_new`,
/// FlowPaths::new panicked via assert! on slash-containing branches,
/// crashing `run_format_status` for users with standard git branch
/// naming conventions (`feature/foo`, `fix/*`, `user/*`).
#[test]
fn format_status_does_not_panic_on_slash_branch() {
    let dir = tempfile::tempdir().unwrap();
    Command::new("git")
        .args(["init", "-b", "feature/foo"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("format-status")
        .current_dir(dir.path())
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_ne!(
        output.status.code(),
        Some(101),
        "format-status must not panic on slash-branch.\nstderr: {}\nstdout: {}",
        stderr,
        stdout
    );
    assert!(
        !stderr.contains("must not contain"),
        "format-status must not emit the slash-branch assert message.\nstderr: {}",
        stderr
    );
}

/// Regression guard: same as above for multi-slash (dependabot-style)
/// branch names, which are produced by automated dependency bots.
#[test]
fn format_status_does_not_panic_on_multi_slash_branch() {
    let dir = tempfile::tempdir().unwrap();
    Command::new("git")
        .args(["init", "-b", "dependabot/npm_and_yarn/acme-1.2.3"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("format-status")
        .current_dir(dir.path())
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_ne!(
        output.status.code(),
        Some(101),
        "format-status must not panic on multi-slash branch.\nstderr: {}",
        stderr
    );
}

/// Regression guard: the stop-continue hook fires on every Claude Code
/// Stop event. A panic here would break the session for any user on a
/// slash-containing branch, since the hook runs after every model turn.
#[test]
fn stop_continue_hook_does_not_panic_on_slash_branch() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    Command::new("git")
        .args(["init", "-b", "feature/foo"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["hook", "stop-continue"])
        .current_dir(dir.path())
        .env_remove("FLOW_CI_RUNNING")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(b"{}").unwrap();
    }
    let output = child.wait_with_output().unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_ne!(
        output.status.code(),
        Some(101),
        "stop-continue hook must not panic on slash-branch.\nstderr: {}",
        stderr
    );
    assert!(
        !stderr.contains("must not contain"),
        "stop-continue hook must not emit the slash-branch assert message.\nstderr: {}",
        stderr
    );
}

#[test]
fn format_status_valid_state_exits_0() {
    let dir = tempfile::tempdir().unwrap();
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

    // Get the branch name
    let branch_out = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let branch = String::from_utf8_lossy(&branch_out.stdout)
        .trim()
        .to_string();

    let state_dir = flow_states_dir(dir.path());
    std::fs::create_dir(&state_dir).unwrap();
    let state = serde_json::json!({
        "branch": branch,
        "pr_url": "https://github.com/test/test/pull/1",
        "started_at": "2026-01-01T00:00:00-08:00",
        "current_phase": "flow-plan",
        "notes": [],
        "phases": {
            "flow-start": {"name": "Start", "status": "complete", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 60, "visit_count": 1},
            "flow-plan": {"name": "Plan", "status": "in_progress", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 1},
            "flow-code": {"name": "Code", "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0},
            "flow-code-review": {"name": "Code Review", "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0},
            "flow-learn": {"name": "Learn", "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0},
            "flow-complete": {"name": "Complete", "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0},
        }
    });
    std::fs::write(
        state_dir.join(format!("{}.json", branch)),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("format-status")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "format-status should exit 0. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("FLOW v"),
        "Panel should contain version. Got: {}",
        stdout
    );
    assert!(
        stdout.contains("Phase 1:"),
        "Panel should contain phases. Got: {}",
        stdout
    );
    assert!(
        stdout.contains("YOU ARE HERE"),
        "Panel should mark current phase. Got: {}",
        stdout
    );
}

#[test]
fn format_status_branch_flag() {
    let dir = tempfile::tempdir().unwrap();
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

    let state_dir = flow_states_dir(dir.path());
    std::fs::create_dir(&state_dir).unwrap();
    let state = serde_json::json!({
        "branch": "other-feature",
        "pr_url": "https://github.com/test/test/pull/2",
        "started_at": "2026-01-01T00:00:00-08:00",
        "current_phase": "flow-code",
        "notes": [],
        "phases": {
            "flow-start": {"name": "Start", "status": "complete", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 60, "visit_count": 1},
            "flow-plan": {"name": "Plan", "status": "complete", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 120, "visit_count": 1},
            "flow-code": {"name": "Code", "status": "in_progress", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 1},
            "flow-code-review": {"name": "Code Review", "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0},
            "flow-learn": {"name": "Learn", "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0},
            "flow-complete": {"name": "Complete", "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0},
        }
    });
    std::fs::write(
        state_dir.join("other-feature.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["format-status", "--branch", "other-feature"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "Should exit 0 with --branch flag"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("FLOW v"), "Panel should contain version");
}

#[test]
fn format_status_corrupt_json_exits_1() {
    let dir = tempfile::tempdir().unwrap();
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

    let branch_out = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let branch = String::from_utf8_lossy(&branch_out.stdout)
        .trim()
        .to_string();

    let state_dir = flow_states_dir(dir.path());
    std::fs::create_dir(&state_dir).unwrap();
    std::fs::write(state_dir.join(format!("{}.json", branch)), "{bad json").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("format-status")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "Corrupt JSON should exit 1");
}

#[test]
fn help_flag_exits_0() {
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("--help")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "--help should exit 0");
}
