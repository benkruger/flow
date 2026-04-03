use std::process::Command;

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
        stdout.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "Not valid lowercase hex: {}",
        stdout
    );
}

// --- log ---

#[test]
fn log_exits_0_and_writes_file() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
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
        "Unknown subcommand must produce no stdout (would mix with Python fallback). Got: {:?}",
        stdout
    );
}

#[test]
fn no_subcommand_exits_1() {
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "No subcommand should exit 1"
    );
}

#[test]
fn help_flag_exits_0() {
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("--help")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "--help should exit 0"
    );
}
