use std::process::Command;

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
