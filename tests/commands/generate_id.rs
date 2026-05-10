//! Integration tests for `flow_rs::commands::generate_id` and the
//! `bin/flow generate-id` subcommand.

use std::process::Command;

use flow_rs::commands::generate_id::generate_id;

#[test]
fn returns_8_chars() {
    let result = generate_id();
    assert_eq!(result.len(), 8);
}

#[test]
fn is_lowercase_hex() {
    let result = generate_id();
    assert!(
        result
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "Not valid lowercase hex: {}",
        result
    );
}

#[test]
fn two_calls_produce_different_values() {
    let a = generate_id();
    let b = generate_id();
    assert_ne!(a, b);
}

#[test]
fn cli_generate_id_prints_8_char_id_and_exits_zero() {
    // Spawn the compiled binary so the `run()` entry point is exercised
    // end-to-end. Covers the line `println!("{}", generate_id())` in
    // the production `run()` function.
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.arg("generate-id").env_remove("FLOW_CI_RUNNING");
    let output = cmd.output().expect("spawn flow-rs generate-id");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let id = stdout.trim();
    assert_eq!(id.len(), 8, "expected 8-char id, got: {:?}", id);
    assert!(
        id.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "not valid lowercase hex: {}",
        id
    );
}
