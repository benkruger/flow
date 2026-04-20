//! Tests for the JSON stdout helpers in `src/output.rs`. Migrated
//! from inline `#[cfg(test)]` per
//! `.claude/rules/test-placement.md`.
//!
//! All assertions drive through the public functions. The `*_string`
//! variants are the builders; the stdout-printing `json_ok` /
//! `json_error` delegate to them and are exercised here to cover the
//! println branches. Captured stdout during tests is suppressed by
//! the Rust test harness unless the test fails.

use flow_rs::output::{json_error, json_error_string, json_ok, json_ok_string};
use serde_json::{json, Value};

// --- json_ok_string ---

#[test]
fn json_ok_no_extra_fields() {
    let result = json_ok_string(&[]);
    let parsed: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["status"], "ok");
}

#[test]
fn json_ok_with_extra_fields() {
    let result = json_ok_string(&[("branch", json!("my-feature")), ("pr_number", json!(42))]);
    let parsed: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["status"], "ok");
    assert_eq!(parsed["branch"], "my-feature");
    assert_eq!(parsed["pr_number"], 42);
}

#[test]
fn json_ok_with_nested_value() {
    let result = json_ok_string(&[("data", json!({"key": "value"}))]);
    let parsed: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["status"], "ok");
    assert_eq!(parsed["data"]["key"], "value");
}

#[test]
fn json_ok_with_boolean_field() {
    let result = json_ok_string(&[("flaky", json!(true))]);
    let parsed: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["status"], "ok");
    assert_eq!(parsed["flaky"], true);
}

#[test]
fn json_ok_produces_valid_json() {
    let result = json_ok_string(&[
        ("count", json!(0)),
        ("items", json!([])),
        ("label", json!(null)),
    ]);
    let parsed: Value = serde_json::from_str(&result).unwrap();
    assert!(parsed.is_object());
}

// --- json_error_string ---

#[test]
fn json_error_basic() {
    let result = json_error_string("file not found", &[]);
    let parsed: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["status"], "error");
    assert_eq!(parsed["message"], "file not found");
}

#[test]
fn json_error_with_extra_fields() {
    let result = json_error_string("phase guard failed", &[("phase", json!("flow-code"))]);
    let parsed: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["status"], "error");
    assert_eq!(parsed["message"], "phase guard failed");
    assert_eq!(parsed["phase"], "flow-code");
}

#[test]
fn json_error_produces_valid_json() {
    let result = json_error_string("bad input: \"quotes\" and \\backslash", &[]);
    let parsed: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["status"], "error");
    assert!(parsed["message"].as_str().unwrap().contains("quotes"));
}

// --- json_ok / json_error (stdout printers) ---
//
// These delegate to the *_string builders and println. Calling them
// in tests is enough to cover the delegation line. Coverage-required
// per `.claude/rules/tests-guard-real-regressions.md` "Coverage-
// Required Tests" — the named consumer is the 100/100/100 gate.

#[test]
fn json_ok_prints_without_panicking() {
    json_ok(&[]);
}

#[test]
fn json_ok_prints_with_fields_without_panicking() {
    json_ok(&[("key", json!("value"))]);
}

#[test]
fn json_error_prints_without_panicking() {
    json_error("test error", &[]);
}

#[test]
fn json_error_prints_with_fields_without_panicking() {
    json_error("test", &[("field", json!("value"))]);
}
