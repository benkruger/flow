use std::fs;
use std::process::Command;

fn make_state(current_phase: &str, phase_statuses: &[(&str, &str)]) -> String {
    let order = [
        "flow-start",
        "flow-plan",
        "flow-code",
        "flow-code-review",
        "flow-learn",
        "flow-complete",
    ];
    let names = [
        ("flow-start", "Start"),
        ("flow-plan", "Plan"),
        ("flow-code", "Code"),
        ("flow-code-review", "Code Review"),
        ("flow-learn", "Learn"),
        ("flow-complete", "Complete"),
    ];
    let name_map: std::collections::HashMap<&str, &str> = names.into_iter().collect();
    let status_map: std::collections::HashMap<&str, &str> = phase_statuses.iter().copied().collect();

    let mut phases = String::from("{");
    for (i, &p) in order.iter().enumerate() {
        if i > 0 {
            phases.push(',');
        }
        let status = status_map.get(p).copied().unwrap_or("pending");
        let name = name_map.get(p).unwrap_or(&p);
        let visit_count = if status == "complete" || status == "in_progress" {
            1
        } else {
            0
        };
        phases.push_str(&format!(
            r#""{}":{{"name":"{}","status":"{}","started_at":null,"completed_at":null,"session_started_at":null,"cumulative_seconds":0,"visit_count":{}}}"#,
            p, name, status, visit_count
        ));
    }
    phases.push('}');

    format!(
        r#"{{"branch":"test-feature","current_phase":"{}","phases":{}}}"#,
        current_phase, phases
    )
}

fn setup_state(dir: &std::path::Path, branch: &str, state_json: &str) {
    let state_dir = dir.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join(format!("{}.json", branch)), state_json).unwrap();
}

fn setup_git_repo(dir: &std::path::Path, branch: &str) {
    Command::new("git")
        .args(["-c", "init.defaultBranch=main", "init"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "commit.gpgsign", "false"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir)
        .output()
        .unwrap();
    // Create and switch to feature branch
    Command::new("git")
        .args(["checkout", "-b", branch])
        .current_dir(dir)
        .output()
        .unwrap();
}

#[test]
fn phase_1_always_exits_0() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["check-phase", "--required", "flow-start"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn no_state_file_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["check-phase", "--required", "flow-plan"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("/flow:flow-start"));
}

#[test]
fn previous_phase_pending_blocks() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");

    let state = make_state("flow-plan", &[("flow-start", "pending")]);
    setup_state(dir.path(), "test-feature", &state);

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["check-phase", "--required", "flow-plan"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("BLOCKED"));
    assert!(stdout.contains("pending"));
}

#[test]
fn previous_phase_complete_allows() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");

    let state = make_state("flow-plan", &[("flow-start", "complete")]);
    setup_state(dir.path(), "test-feature", &state);

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["check-phase", "--required", "flow-plan"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn branch_flag_uses_specified_state_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "main");

    let state = make_state("flow-plan", &[("flow-start", "complete")]);
    setup_state(dir.path(), "other-feature", &state);

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "check-phase",
            "--required",
            "flow-plan",
            "--branch",
            "other-feature",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn multiple_state_files_returns_ambiguity() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "main");

    for name in ["feat-a", "feat-b"] {
        let state = make_state("flow-plan", &[("flow-start", "complete")]);
        setup_state(dir.path(), name, &state);
    }

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["check-phase", "--required", "flow-plan"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Multiple active features"));
    assert!(stdout.contains("feat-a"));
    assert!(stdout.contains("feat-b"));
}

#[test]
fn frozen_phases_file_is_loaded() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");

    let state = make_state("flow-plan", &[("flow-start", "complete")]);
    setup_state(dir.path(), "test-feature", &state);

    // Copy flow-phases.json as frozen phases
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let source = std::path::PathBuf::from(manifest_dir).join("flow-phases.json");
    let dest = dir
        .path()
        .join(".flow-states")
        .join("test-feature-phases.json");
    fs::copy(source, dest).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["check-phase", "--required", "flow-plan"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
}
