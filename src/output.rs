//! JSON stdout helpers used by `bin/flow` subcommand main arms.
//!
//! Tests live at `tests/output.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` in
//! this file.

use serde_json::Value;

/// Print a JSON success response to stdout.
///
/// Produces `{"status": "ok", ...extra_fields}`.
pub fn json_ok(fields: &[(&str, Value)]) {
    println!("{}", json_ok_string(fields));
}

/// Print a JSON error response to stdout.
///
/// Produces `{"status": "error", "message": "...", ...extra_fields}`.
pub fn json_error(message: &str, fields: &[(&str, Value)]) {
    println!("{}", json_error_string(message, fields));
}

/// Build a JSON success response as a `Value`.
///
/// Produces a status-first `Value::Object` — `{"status": "ok",
/// ...extra_fields}`. Callers that return `(Value, i32)` use this
/// directly instead of reparsing the `*_string` form. `serde_json::Map`
/// preserves insertion order (`preserve_order` feature), so `status`
/// is always the first key.
pub fn json_ok_value(fields: &[(&str, Value)]) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("status".to_string(), Value::String("ok".to_string()));
    for (key, value) in fields {
        map.insert((*key).to_string(), value.clone());
    }
    Value::Object(map)
}

/// Build a JSON error response as a `Value`.
///
/// Produces a status-first `Value::Object` — `{"status": "error",
/// "message": "...", ...extra_fields}`. Callers that return
/// `(Value, i32)` use this directly instead of reparsing the
/// `*_string` form.
pub fn json_error_value(message: &str, fields: &[(&str, Value)]) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("status".to_string(), Value::String("error".to_string()));
    map.insert("message".to_string(), Value::String(message.to_string()));
    for (key, value) in fields {
        map.insert((*key).to_string(), value.clone());
    }
    Value::Object(map)
}

/// Build a JSON success response as a String (for testing or capture).
///
/// Delegates to `json_ok_value` and serializes — byte-identical to
/// the `Value` form's `.to_string()`.
pub fn json_ok_string(fields: &[(&str, Value)]) -> String {
    json_ok_value(fields).to_string()
}

/// Build a JSON error response as a String (for testing or capture).
///
/// Delegates to `json_error_value` and serializes — byte-identical to
/// the `Value` form's `.to_string()`.
pub fn json_error_string(message: &str, fields: &[(&str, Value)]) -> String {
    json_error_value(message, fields).to_string()
}
