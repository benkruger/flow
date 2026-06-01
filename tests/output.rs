//! Tests for the JSON stdout helpers in `src/output.rs`. Migrated
//! from inline `#[cfg(test)]` per
//! `.claude/rules/test-placement.md`.
//!
//! All assertions drive through the public functions. The `*_string`
//! variants are the builders; the stdout-printing `json_ok` /
//! `json_error` delegate to them and are exercised here to cover the
//! println branches. Captured stdout during tests is suppressed by
//! the Rust test harness unless the test fails.

use flow_rs::output::{
    json_error, json_error_string, json_error_value, json_ok, json_ok_string, json_ok_value,
};
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

// --- json_ok_value / json_error_value ---
//
// The `*_value` helpers build the `Value::Object` directly; the
// `*_string` variants delegate to them and `.to_string()`. The
// status-first ordering and the delegation equivalence are the
// invariants callers (`cleanup`, `phase_transition`) depend on:
// they return the Value directly and never reparse a String.

#[test]
fn json_ok_value_is_status_first_object() {
    let v = json_ok_value(&[("branch", json!("my-feature")), ("pr_number", json!(42))]);
    assert!(v.is_object());
    let keys: Vec<&str> = v.as_object().unwrap().keys().map(|k| k.as_str()).collect();
    assert_eq!(keys.first(), Some(&"status"));
    assert_eq!(v["status"], "ok");
    assert_eq!(v["branch"], "my-feature");
    assert_eq!(v["pr_number"], 42);
}

#[test]
fn json_error_value_is_status_first_then_message() {
    let v = json_error_value("boom", &[("phase", json!("flow-code"))]);
    assert!(v.is_object());
    let keys: Vec<&str> = v.as_object().unwrap().keys().map(|k| k.as_str()).collect();
    assert_eq!(keys.first(), Some(&"status"));
    assert_eq!(keys.get(1), Some(&"message"));
    assert_eq!(v["status"], "error");
    assert_eq!(v["message"], "boom");
    assert_eq!(v["phase"], "flow-code");
}

#[test]
fn json_ok_string_equals_value_to_string() {
    let fields = [("branch", json!("my-feature")), ("pr_number", json!(42))];
    assert_eq!(json_ok_string(&fields), json_ok_value(&fields).to_string());
}

#[test]
fn json_ok_string_equals_value_to_string_empty() {
    assert_eq!(json_ok_string(&[]), json_ok_value(&[]).to_string());
}

#[test]
fn json_error_string_equals_value_to_string() {
    let fields = [("phase", json!("flow-code"))];
    assert_eq!(
        json_error_string("boom", &fields),
        json_error_value("boom", &fields).to_string()
    );
}

#[test]
fn json_error_string_equals_value_to_string_empty() {
    assert_eq!(
        json_error_string("boom", &[]),
        json_error_value("boom", &[]).to_string()
    );
}
