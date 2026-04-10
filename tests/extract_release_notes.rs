//! Tests for `src/extract_release_notes.rs`.
//!
//! Pure-function tests call extract() directly. CLI tests call run_impl
//! with a fake repo fixture.

use std::fs;

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
