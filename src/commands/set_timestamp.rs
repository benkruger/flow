use std::process;

use serde_json::{json, Value};

use crate::flow_paths::FlowPaths;
use crate::git::{project_root, resolve_branch};
use crate::lock::mutate_state;
use crate::output::{json_error, json_ok};
use crate::utils::now;

/// A single path=value update that was applied.
#[derive(Debug)]
pub struct Update {
    pub path: String,
    pub value: Value,
}

/// Navigate a nested JSON Value by dot-path parts and set the final value.
///
/// Numeric path segments are treated as array indexes (0-based).
pub fn set_nested(obj: &mut Value, path_parts: &[&str], value: Value) -> Result<(), String> {
    if path_parts.is_empty() {
        return Err("Empty path".to_string());
    }

    let (intermediate, final_key) = path_parts.split_at(path_parts.len() - 1);

    let mut current = obj;
    for part in intermediate {
        current = match current {
            Value::Array(arr) => {
                let index: usize = part
                    .parse()
                    .map_err(|_| format!("Expected numeric index for list, got '{}'", part))?;
                if index >= arr.len() {
                    return Err(format!(
                        "Index {} out of range (list has {} items)",
                        index,
                        arr.len()
                    ));
                }
                &mut arr[index]
            }
            Value::Object(map) => map
                .get_mut(*part)
                .ok_or_else(|| format!("Key '{}' not found", part))?,
            other => {
                return Err(format!(
                    "Cannot navigate into {} with key '{}'",
                    value_type_name(other),
                    part
                ))
            }
        };
    }

    let key = final_key[0];
    match current {
        Value::Array(arr) => {
            let index: usize = key
                .parse()
                .map_err(|_| format!("Expected numeric index for list, got '{}'", key))?;
            if index >= arr.len() {
                return Err(format!(
                    "Index {} out of range (list has {} items)",
                    index,
                    arr.len()
                ));
            }
            arr[index] = value;
        }
        Value::Object(map) => {
            map.insert(key.to_string(), value);
        }
        other => {
            return Err(format!(
                "Cannot set key '{}' on {}",
                key,
                value_type_name(other)
            ))
        }
    }

    Ok(())
}

fn value_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// Validate that code_task can only increment by 1 or reset to 0.
pub fn validate_code_task(state: &Value, new_value: i64) -> Result<(), String> {
    if new_value == 0 {
        return Ok(());
    }
    let current = state.get("code_task").and_then(|v| v.as_i64()).unwrap_or(0);
    if new_value != current.saturating_add(1) {
        let hint = if new_value == current.saturating_add(2) {
            format!(
                "--set code_task={} --set code_task={}",
                current.saturating_add(1),
                new_value
            )
        } else {
            format!(
                "--set code_task={} --set code_task={} ... --set code_task={}",
                current.saturating_add(1),
                current.saturating_add(2),
                new_value
            )
        };
        return Err(format!(
            "code_task can only increment by 1. Current: {}, attempted: {}. \
             Use multiple --set args in one call for atomic groups: {}",
            current, new_value, hint
        ));
    }
    Ok(())
}

/// Apply a list of path=value updates to the state Value.
///
/// Returns the list of updates that were applied.
pub fn apply_updates(state: &mut Value, set_args: &[String]) -> Result<Vec<Update>, String> {
    let mut updates = Vec::new();

    for assignment in set_args {
        let eq_pos = assignment
            .find('=')
            .ok_or_else(|| format!("Invalid format '{}' — expected path=value", assignment))?;

        let path = &assignment[..eq_pos];
        let raw_value = &assignment[eq_pos + 1..];

        let value: Value = if raw_value == "NOW" {
            Value::String(now())
        } else if let Ok(n) = raw_value.parse::<i64>() {
            json!(n)
        } else {
            Value::String(raw_value.to_string())
        };

        let path_parts: Vec<&str> = path.split('.').collect();

        if path_parts == ["code_task"] {
            let int_val = value
                .as_i64()
                .ok_or_else(|| format!("code_task must be an integer, got '{}'", raw_value))?;
            validate_code_task(state, int_val)?;
        }

        set_nested(state, &path_parts, value.clone())?;
        updates.push(Update {
            path: path.to_string(),
            value,
        });
    }

    Ok(updates)
}

/// Run the set-timestamp command.
pub fn run(set_args: Vec<String>, branch_override: Option<String>) {
    let root = project_root();

    // Drift guard: set-timestamp is the general-purpose state mutator
    // for mid-phase fields. Writing to the state file from the wrong
    // subdirectory of a mono-repo would silently record values
    // against the wrong assumed scope. See
    // [`crate::cwd_scope::enforce`].
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    if let Err(msg) = crate::cwd_scope::enforce(&cwd, &root) {
        json_error(&msg, &[]);
        process::exit(1);
    }

    let branch = match resolve_branch(branch_override.as_deref(), &root) {
        Some(b) => b,
        None => {
            json_error("Could not determine current branch", &[]);
            process::exit(1);
        }
    };

    let state_path = FlowPaths::new(&root, &branch).state_file();

    if !state_path.exists() {
        json_error(
            &format!("No state file found: {}", state_path.display()),
            &[],
        );
        process::exit(1);
    }

    let mut collected_updates: Vec<Update> = Vec::new();

    let result = mutate_state(&state_path, |state| match apply_updates(state, &set_args) {
        Ok(updates) => {
            collected_updates = updates;
        }
        Err(e) => {
            json_error(&e, &[]);
            process::exit(1);
        }
    });

    match result {
        Ok(_) => {
            let updates_json: Vec<Value> = collected_updates
                .iter()
                .map(|u| json!({"path": u.path, "value": u.value}))
                .collect();
            json_ok(&[("updates", json!(updates_json))]);
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("Invalid JSON") || msg.contains("JSON error") {
                json_error(&format!("Could not read state file: {}", msg), &[]);
            } else {
                json_error(&msg, &[]);
            }
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;
    use serde_json::json;

    fn iso_pattern() -> Regex {
        Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[Z+-]").unwrap()
    }

    // --- set_nested unit tests ---

    #[test]
    fn test_set_nested_simple_dict_key() {
        let mut obj = json!({"design": {"status": "pending"}});
        set_nested(&mut obj, &["design", "status"], json!("approved")).unwrap();
        assert_eq!(obj["design"]["status"], "approved");
    }

    #[test]
    fn test_set_nested_nested_path() {
        let mut obj = json!({"a": {"b": {"c": 1}}});
        set_nested(&mut obj, &["a", "b", "c"], json!(99)).unwrap();
        assert_eq!(obj["a"]["b"]["c"], 99);
    }

    #[test]
    fn test_set_nested_list_index() {
        let mut obj = json!({"items": [10, 20, 30]});
        set_nested(&mut obj, &["items", "1"], json!(99)).unwrap();
        assert_eq!(obj["items"][1], 99);
    }

    #[test]
    fn test_set_nested_list_non_numeric_intermediate() {
        let mut obj = json!({"items": [{"a": 1}]});
        let result = set_nested(&mut obj, &["items", "abc", "a"], json!("val"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Expected numeric index"));
    }

    #[test]
    fn test_set_nested_non_traversable_intermediate() {
        let mut obj = json!({"outer": {"name": "hello"}});
        let result = set_nested(&mut obj, &["outer", "name", "deep", "sub"], json!("val"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Cannot navigate into"));
    }

    #[test]
    fn test_set_nested_list_final_non_numeric() {
        let mut obj = json!({"items": [1, 2, 3]});
        let result = set_nested(&mut obj, &["items", "abc"], json!("val"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Expected numeric index"));
    }

    #[test]
    fn test_set_nested_list_final_out_of_range() {
        let mut obj = json!({"items": [1, 2, 3]});
        let result = set_nested(&mut obj, &["items", "99"], json!("val"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("out of range"));
    }

    #[test]
    fn test_set_nested_non_settable_final() {
        let mut obj = json!({"items": [1, 2]});
        let result = set_nested(&mut obj, &["items", "0", "sub"], json!("val"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Cannot set key"));
    }

    #[test]
    fn test_set_nested_list_intermediate_out_of_range() {
        let mut obj = json!({"items": [{"a": 1}]});
        let result = set_nested(&mut obj, &["items", "99", "a"], json!("val"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("out of range"));
    }

    #[test]
    fn test_set_nested_dict_key_not_found() {
        let mut obj = json!({"a": {"b": 1}});
        let result = set_nested(&mut obj, &["a", "missing", "x"], json!("val"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_set_nested_creates_new_dict_key() {
        let mut obj = json!({"a": {}});
        set_nested(&mut obj, &["a", "new_key"], json!("new_value")).unwrap();
        assert_eq!(obj["a"]["new_key"], "new_value");
    }

    #[test]
    fn test_set_nested_array_in_nested_path() {
        let mut obj = json!({"plan": {"tasks": [
            {"id": 1, "status": "pending", "started_at": null},
            {"id": 2, "status": "pending", "started_at": null}
        ]}});
        set_nested(
            &mut obj,
            &["plan", "tasks", "0", "status"],
            json!("in_progress"),
        )
        .unwrap();
        assert_eq!(obj["plan"]["tasks"][0]["status"], "in_progress");
        assert_eq!(obj["plan"]["tasks"][1]["status"], "pending");
    }

    // --- apply_updates tests ---

    #[test]
    fn test_apply_updates_simple_string() {
        let mut state = json!({"design": {"status": "pending"}});
        let updates = apply_updates(&mut state, &["design.status=approved".to_string()]).unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(state["design"]["status"], "approved");
    }

    #[test]
    fn test_apply_updates_now_magic_value() {
        let mut state = json!({"design": {"approved_at": null}});
        let updates = apply_updates(&mut state, &["design.approved_at=NOW".to_string()]).unwrap();
        assert_eq!(updates.len(), 1);
        assert!(iso_pattern().is_match(updates[0].value.as_str().unwrap()));
        assert!(iso_pattern().is_match(state["design"]["approved_at"].as_str().unwrap()));
    }

    #[test]
    fn test_apply_updates_integer_coercion() {
        let mut state = json!({"code_review_step": 0});
        let updates = apply_updates(&mut state, &["code_review_step=1".to_string()]).unwrap();
        assert_eq!(state["code_review_step"], 1);
        assert!(state["code_review_step"].is_i64());
        assert_eq!(updates[0].value, json!(1));
    }

    #[test]
    fn test_apply_updates_negative_integer() {
        let mut state = json!({"offset": 0});
        let updates = apply_updates(&mut state, &["offset=-5".to_string()]).unwrap();
        assert_eq!(state["offset"], -5);
        assert!(state["offset"].is_i64());
        assert_eq!(updates[0].value, json!(-5));
    }

    #[test]
    fn test_apply_updates_non_digit_stays_string() {
        let mut state = json!({"some_field": "old"});
        let updates = apply_updates(&mut state, &["some_field=in_progress".to_string()]).unwrap();
        assert_eq!(state["some_field"], "in_progress");
        assert!(state["some_field"].is_string());
        assert_eq!(updates[0].value, json!("in_progress"));
    }

    #[test]
    fn test_apply_updates_multiple_args() {
        let mut state = json!({"plan": {"tasks": [
            {"id": 1, "status": "pending", "started_at": null}
        ]}});
        let updates = apply_updates(
            &mut state,
            &[
                "plan.tasks.0.status=in_progress".to_string(),
                "plan.tasks.0.started_at=NOW".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(updates.len(), 2);
        assert_eq!(state["plan"]["tasks"][0]["status"], "in_progress");
        assert!(iso_pattern().is_match(state["plan"]["tasks"][0]["started_at"].as_str().unwrap()));
    }

    #[test]
    fn test_apply_updates_invalid_format() {
        let mut state = json!({"a": 1});
        let result = apply_updates(&mut state, &["design.approved_at".to_string()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid format"));
    }

    // --- validate_code_task tests ---

    #[test]
    fn test_code_task_increment_by_one() {
        let state = json!({"code_task": 0});
        assert!(validate_code_task(&state, 1).is_ok());
        let state = json!({"code_task": 1});
        assert!(validate_code_task(&state, 2).is_ok());
    }

    #[test]
    fn test_code_task_initial_set_to_one() {
        let state = json!({"branch": "test"}); // no code_task key
        assert!(validate_code_task(&state, 1).is_ok());
    }

    #[test]
    fn test_code_task_jump_blocked() {
        let state = json!({"code_task": 0});
        let result = validate_code_task(&state, 5);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("increment by 1"));
    }

    #[test]
    fn test_code_task_skip_blocked() {
        let state = json!({"code_task": 3});
        let result = validate_code_task(&state, 5);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("increment by 1"));
    }

    #[test]
    fn test_code_task_reset_to_zero() {
        let state = json!({"code_task": 3});
        assert!(validate_code_task(&state, 0).is_ok());
    }

    #[test]
    fn test_code_task_non_integer_blocked() {
        let mut state = json!({"code_task": 0});
        let result = apply_updates(&mut state, &["code_task=abc".to_string()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("must be an integer"));
    }

    #[test]
    fn test_code_task_cli_increment_blocked() {
        let mut state = json!({"code_task": 0});
        let result = apply_updates(&mut state, &["code_task=5".to_string()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("increment by 1"));
    }

    #[test]
    fn test_code_task_error_message_mentions_batch_set() {
        let state = json!({"code_task": 0});
        let result = validate_code_task(&state, 5);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(
            msg.contains("--set code_task="),
            "Error message should mention batch --set pattern, got: {}",
            msg
        );
    }

    // --- set_nested edge cases ---
    // Exercises value_type_name match arms and the empty-path guard.
    // Covered: empty path, Null intermediate, Bool intermediate,
    // Null final, Bool final. String and Number arms are covered by
    // the existing tests above (non_traversable_intermediate and
    // non_settable_final). Array and Object arms are handled by
    // set_nested's dedicated match arms and never reach value_type_name.

    #[test]
    fn set_nested_empty_path_errors() {
        let mut obj = json!({});
        let result = set_nested(&mut obj, &[], json!("v"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Empty path"));
    }

    #[test]
    fn set_nested_null_intermediate_errors() {
        let mut obj = json!({"a": null});
        let result = set_nested(&mut obj, &["a", "x", "y"], json!("v"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("NoneType"));
    }

    #[test]
    fn set_nested_bool_intermediate_errors() {
        let mut obj = json!({"a": true});
        let result = set_nested(&mut obj, &["a", "x", "y"], json!("v"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("bool"));
    }

    #[test]
    fn set_nested_null_final_errors() {
        let mut obj = json!({"a": null});
        let result = set_nested(&mut obj, &["a", "x"], json!("v"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("NoneType"));
    }

    #[test]
    fn set_nested_bool_final_errors() {
        let mut obj = json!({"a": true});
        let result = set_nested(&mut obj, &["a", "x"], json!("v"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("bool"));
    }

    #[test]
    fn test_code_task_batch_increment_in_single_call() {
        let mut state = json!({"code_task": 0});
        let updates = apply_updates(
            &mut state,
            &[
                "code_task=1".to_string(),
                "code_task=2".to_string(),
                "code_task=3".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(updates.len(), 3);
        assert_eq!(state["code_task"], 3);
    }
}
