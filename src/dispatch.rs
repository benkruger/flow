//! Shared CLI dispatch helpers for `main.rs` match arms.
//!
//! Match arms whose owning module exposes a `run_impl_main` pure
//! function call one of these helpers to print the result and exit.
//! Centralizing the print-then-exit pair keeps match arms short and
//! ensures a uniform output contract: JSON for structured commands,
//! plain text for human-readable output.

use serde_json::Value;

/// Serialize `result` as JSON to stdout, then exit with `code`.
pub fn dispatch_json(result: Value, code: i32) -> ! {
    println!("{}", serde_json::to_string(&result).unwrap());
    std::process::exit(code)
}

/// Print `text` to stdout when non-empty, then exit with `code`.
/// Empty strings produce no output so callers can represent a
/// success-with-no-text result without an extra blank line.
pub fn dispatch_text(text: &str, code: i32) -> ! {
    if !text.is_empty() {
        println!("{}", text);
    }
    std::process::exit(code)
}

/// Convert a `Result<Value, String>` into the `(Value, i32)` contract.
///
/// `Ok(value)` with `status: "error"` maps to exit 1, all other
/// `Ok(value)` map to exit 0. `Err(e)` wraps `e` in a
/// `{"status": "error", "message": ...}` payload with exit 1.
pub fn result_to_value_code(result: Result<Value, String>) -> (Value, i32) {
    match result {
        Ok(v) => {
            let code = if v.get("status").and_then(|s| s.as_str()) == Some("error") {
                1
            } else {
                0
            };
            (v, code)
        }
        Err(e) => (serde_json::json!({"status": "error", "message": e}), 1),
    }
}

/// Combines [`result_to_value_code`] with [`dispatch_json`] for
/// `run()` entry points whose only job is to print the JSON result
/// and exit.
pub fn dispatch_result_json(result: Result<Value, String>) -> ! {
    let (value, code) = result_to_value_code(result);
    dispatch_json(value, code)
}

/// Variant of [`result_to_value_code`] that treats every `Ok(value)`
/// as exit 0, even when the value carries `status: "error"`. Used by
/// commands that surface application errors as JSON payloads with a
/// zero exit code (phase gates, plan-check, tombstone-audit, etc.).
pub fn ok_result_to_value_code(result: Result<Value, String>) -> (Value, i32) {
    match result {
        Ok(v) => (v, 0),
        Err(e) => (serde_json::json!({"status": "error", "message": e}), 1),
    }
}

/// Combines [`ok_result_to_value_code`] with [`dispatch_json`] for
/// `run()` entry points that print the Ok JSON as-is (exit 0) and
/// route infrastructure Err to `{status: error, message}` exit 1.
pub fn dispatch_ok_result_json(result: Result<Value, String>) -> ! {
    let (value, code) = ok_result_to_value_code(result);
    dispatch_json(value, code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn result_to_value_code_ok_non_error_returns_code_zero() {
        let (v, code) = result_to_value_code(Ok(json!({"status": "ok", "data": 42})));
        assert_eq!(code, 0);
        assert_eq!(v["status"], "ok");
        assert_eq!(v["data"], 42);
    }

    #[test]
    fn result_to_value_code_ok_error_status_returns_code_one() {
        let (v, code) = result_to_value_code(Ok(json!({"status": "error", "message": "bad"})));
        assert_eq!(code, 1);
        assert_eq!(v["status"], "error");
    }

    #[test]
    fn result_to_value_code_err_wraps_message_with_code_one() {
        let (v, code) = result_to_value_code(Err("infra failure".to_string()));
        assert_eq!(code, 1);
        assert_eq!(v["status"], "error");
        assert_eq!(v["message"], "infra failure");
    }

    #[test]
    fn result_to_value_code_ok_without_status_field_is_success() {
        let (v, code) = result_to_value_code(Ok(json!({"data": 1})));
        assert_eq!(code, 0);
        assert_eq!(v["data"], 1);
    }

    #[test]
    fn ok_result_to_value_code_ok_error_status_still_exits_zero() {
        let (v, code) = ok_result_to_value_code(Ok(json!({"status": "error", "message": "gate"})));
        assert_eq!(code, 0);
        assert_eq!(v["status"], "error");
    }

    #[test]
    fn ok_result_to_value_code_err_wraps_message_with_code_one() {
        let (v, code) = ok_result_to_value_code(Err("infra".to_string()));
        assert_eq!(code, 1);
        assert_eq!(v["status"], "error");
        assert_eq!(v["message"], "infra");
    }

    #[test]
    fn ok_result_to_value_code_ok_without_error_status_exits_zero() {
        let (v, code) = ok_result_to_value_code(Ok(json!({"status": "ok"})));
        assert_eq!(code, 0);
        assert_eq!(v["status"], "ok");
    }
}
