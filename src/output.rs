use serde_json::Value;

/// Print a JSON success response to stdout.
///
/// Produces `{"status": "ok", ...extra_fields}`.
pub fn json_ok(fields: &[(&str, Value)]) {
    let mut map = serde_json::Map::new();
    map.insert("status".to_string(), Value::String("ok".to_string()));
    for (key, value) in fields {
        map.insert((*key).to_string(), value.clone());
    }
    println!("{}", Value::Object(map));
}

/// Print a JSON error response to stdout.
///
/// Produces `{"status": "error", "message": "...", ...extra_fields}`.
pub fn json_error(message: &str, fields: &[(&str, Value)]) {
    let mut map = serde_json::Map::new();
    map.insert("status".to_string(), Value::String("error".to_string()));
    map.insert("message".to_string(), Value::String(message.to_string()));
    for (key, value) in fields {
        map.insert((*key).to_string(), value.clone());
    }
    println!("{}", Value::Object(map));
}

/// Build a JSON success response as a String (for testing or capture).
pub fn json_ok_string(fields: &[(&str, Value)]) -> String {
    let mut map = serde_json::Map::new();
    map.insert("status".to_string(), Value::String("ok".to_string()));
    for (key, value) in fields {
        map.insert((*key).to_string(), value.clone());
    }
    Value::Object(map).to_string()
}

/// Build a JSON error response as a String (for testing or capture).
pub fn json_error_string(message: &str, fields: &[(&str, Value)]) -> String {
    let mut map = serde_json::Map::new();
    map.insert("status".to_string(), Value::String("error".to_string()));
    map.insert("message".to_string(), Value::String(message.to_string()));
    for (key, value) in fields {
        map.insert((*key).to_string(), value.clone());
    }
    Value::Object(map).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_ok_no_extra_fields() {
        let result = json_ok_string(&[]);
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["status"], "ok");
    }

    #[test]
    fn json_ok_with_extra_fields() {
        let result = json_ok_string(&[
            ("branch", json!("my-feature")),
            ("pr_number", json!(42)),
        ]);
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
    fn json_ok_produces_valid_json() {
        let result = json_ok_string(&[
            ("count", json!(0)),
            ("items", json!([])),
            ("label", json!(null)),
        ]);
        // Must parse without error
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert!(parsed.is_object());
    }

    #[test]
    fn json_error_produces_valid_json() {
        let result = json_error_string("bad input: \"quotes\" and \\backslash", &[]);
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["status"], "error");
        assert!(parsed["message"].as_str().unwrap().contains("quotes"));
    }
}
