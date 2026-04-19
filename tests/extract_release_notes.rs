//! Tests for `src/extract_release_notes.rs`.
//!
//! Pure-function tests call extract() directly. CLI tests call run_impl
//! with a fake repo fixture. Subprocess tests at the bottom drive the
//! `pub fn run` wrapper end-to-end via the compiled binary.

use std::fs;
use std::process::Command;

use flow_rs::extract_release_notes::{extract, run_impl, Args};

const SAMPLE_NOTES: &str = "\
# Release Notes

## v0.3.0 — Third release

- Feature C

---

## v0.2.0 — Second release

- Feature B
- Fix B

---

## v0.1.0 — Initial Release

- Feature A
";

// --- extract ---

#[test]
fn test_extract_middle_version() {
    let result = extract("v0.2.0", SAMPLE_NOTES);
    assert!(result.starts_with("## v0.2.0"));
    assert!(result.contains("Feature B"));
    assert!(!result.contains("Feature A"));
    assert!(!result.contains("Feature C"));
}

#[test]
fn test_extract_first_version() {
    let result = extract("v0.3.0", SAMPLE_NOTES);
    assert!(result.starts_with("## v0.3.0"));
    assert!(result.contains("Feature C"));
}

#[test]
fn test_extract_last_version() {
    let result = extract("v0.1.0", SAMPLE_NOTES);
    assert!(result.starts_with("## v0.1.0"));
    assert!(result.contains("Feature A"));
}

#[test]
fn test_missing_version_returns_empty() {
    let result = extract("v9.9.9", SAMPLE_NOTES);
    assert_eq!(result, "");
}

#[test]
fn test_version_in_body_text_not_matched() {
    let notes = "\
# Release Notes

## v1.0.0 — First release

- Upgraded from v0.9.0 to v1.0.0

---

## v0.8.0 — Earlier release

- Feature
";
    let result = extract("v0.9.0", notes);
    assert_eq!(result, "");
}

#[test]
fn test_substring_version_not_matched() {
    let notes = "\
# Release Notes

## v0.10.0 — Tenth release

- Feature X

---

## v0.1.0 — First release

- Feature A
";
    // v0.1.0 must NOT match v0.10.0 header (substring match bug)
    let result = extract("v0.1.0", notes);
    assert!(
        result.starts_with("## v0.1.0"),
        "Expected v0.1.0 section, got: {}",
        result
    );
    assert!(result.contains("Feature A"));
    assert!(!result.contains("Feature X"));
}

// --- CLI integration tests via run_impl ---

#[test]
fn test_cli_writes_output_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("RELEASE-NOTES.md"), SAMPLE_NOTES).unwrap();

    let args = Args {
        version: Some("v0.2.0".to_string()),
    };
    let result = run_impl(&args, dir.path());
    assert!(result.is_ok(), "run_impl failed: {:?}", result.err());

    let out_file = dir.path().join("tmp").join("release-notes-v0.2.0.md");
    assert!(out_file.exists());
    let content = fs::read_to_string(&out_file).unwrap();
    assert!(content.contains("v0.2.0"));
    assert!(content.contains("Feature B"));
}

#[test]
fn test_cli_invalid_version_format() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("RELEASE-NOTES.md"), SAMPLE_NOTES).unwrap();

    let args = Args {
        version: Some("../../etc/passwd".to_string()),
    };
    let result = run_impl(&args, dir.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("invalid version format"));
}

#[test]
fn test_cli_missing_version_section() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("RELEASE-NOTES.md"), SAMPLE_NOTES).unwrap();

    let args = Args {
        version: Some("v99.99.99".to_string()),
    };
    let result = run_impl(&args, dir.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("no section found"));
}

#[test]
fn test_cli_no_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args { version: None };
    let result = run_impl(&args, dir.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Usage"));
}

#[test]
fn test_cli_file_not_found() {
    let dir = tempfile::tempdir().unwrap();
    // No RELEASE-NOTES.md

    let args = Args {
        version: Some("v1.0.0".to_string()),
    };
    let result = run_impl(&args, dir.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not found"));
}

// --- Subprocess tests covering the `pub fn run` wrapper ---

/// Set up a fake plugin root: a directory with a `flow-phases.json`
/// file (so `plugin_root()` resolves) and a `RELEASE-NOTES.md` so
/// `run_impl` can read content. Returns the tempdir + its path.
fn setup_plugin_root_with_notes(notes: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    fs::write(root.join("flow-phases.json"), "{}").unwrap();
    fs::write(root.join("RELEASE-NOTES.md"), notes).unwrap();
    (dir, root)
}

#[test]
fn run_subprocess_success_prints_written_to_and_exits_zero() {
    let (_dir, root) = setup_plugin_root_with_notes(SAMPLE_NOTES);
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["extract-release-notes", "v0.2.0"])
        .env("CLAUDE_PLUGIN_ROOT", &root)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("spawn flow-rs");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Written to"),
        "expected success message, stdout: {}",
        stdout
    );
    // Verify the file was actually written.
    let written = root.join("tmp").join("release-notes-v0.2.0.md");
    assert!(written.exists(), "expected output file at {:?}", written);
}

#[test]
fn run_subprocess_missing_version_prints_error_and_exits_one() {
    let (_dir, root) = setup_plugin_root_with_notes(SAMPLE_NOTES);
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["extract-release-notes", "v9.9.9"])
        .env("CLAUDE_PLUGIN_ROOT", &root)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("spawn flow-rs");
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("no section found"),
        "expected error message, stdout: {}",
        stdout
    );
}

// REMOVED: `run_subprocess_no_plugin_root_prints_error_and_exits_one`.
//
// Same shape as the deleted bump-version equivalent: setting
// `CLAUDE_PLUGIN_ROOT` to an empty tempdir does NOT make
// `plugin_root()` return None — the helper falls through to a
// `current_exe` walk-up which reaches the real flow repo's
// `flow-phases.json` from the test binary location. The subprocess
// then runs `extract-release-notes` against the real repo. While
// extract-release-notes only writes to `tmp/` (gitignored) so the
// blast radius is smaller than bump-version's, it still spawns a
// production code path against unintended state.
//
// The "plugin_root None" branch is unreachable from any subprocess
// test launched from inside the flow repo. Coverage for that arm
// must come from inline unit tests of the helper. Do NOT re-add
// this test shape.
