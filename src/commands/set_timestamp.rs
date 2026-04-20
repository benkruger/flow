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
                let index: usize = match part.parse() {
                    Ok(n) => n,
                    Err(_) => {
                        return Err(format!("Expected numeric index for list, got '{}'", part))
                    }
                };
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
            let index: usize = match key.parse() {
                Ok(n) => n,
                Err(_) => return Err(format!("Expected numeric index for list, got '{}'", key)),
            };
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
            let int_val = match value.as_i64() {
                Some(n) => n,
                None => return Err(format!("code_task must be an integer, got '{}'", raw_value)),
            };
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

/// Outcome of [`run_impl_main`]: a JSON payload (success or error
/// shape) and a paired exit code.
pub type RunOutcome = (Value, i32);

/// Testable core of the set-timestamp command. Returns the payload the
/// CLI wrapper would print plus the exit code. Tests pass tempdir
/// `root`/`cwd` to bypass git resolution and the on-disk state file.
pub fn run_impl_main(
    set_args: &[String],
    branch_override: Option<&str>,
    root: &std::path::Path,
    cwd: &std::path::Path,
) -> RunOutcome {
    // Drift guard: set-timestamp is the general-purpose state mutator
    // for mid-phase fields. Writing to the state file from the wrong
    // subdirectory of a mono-repo would silently record values
    // against the wrong assumed scope. See
    // [`crate::cwd_scope::enforce`].
    if let Err(msg) = crate::cwd_scope::enforce(cwd, root) {
        return (json!({"status": "error", "message": msg}), 1);
    }

    let branch = match resolve_branch(branch_override, root) {
        Some(b) => b,
        None => {
            return (
                json!({
                    "status": "error",
                    "message": "Could not determine current branch"
                }),
                1,
            );
        }
    };

    let state_path = FlowPaths::new(root, &branch).state_file();

    if !state_path.exists() {
        return (
            json!({
                "status": "error",
                "message": format!("No state file found: {}", state_path.display())
            }),
            1,
        );
    }

    let mut collected_updates: Vec<Update> = Vec::new();
    let mut apply_error: Option<String> = None;

    // Snapshot state before applying updates so a mid-way failure can
    // restore the original — `apply_updates` mutates in place, so the
    // pre-extraction implementation relied on `process::exit` inside
    // the closure to abort the post-closure `mutate_state` write. The
    // testable extraction returns errors instead of exiting, so we
    // restore the snapshot here to preserve "no partial mutation on
    // error" semantics.
    let result = mutate_state(&state_path, &mut |state| {
        let backup = state.clone();
        match apply_updates(state, set_args) {
            Ok(updates) => {
                collected_updates = updates;
            }
            Err(e) => {
                *state = backup;
                apply_error = Some(e);
            }
        }
    });

    if let Some(msg) = apply_error {
        return (json!({"status": "error", "message": msg}), 1);
    }

    match result {
        Ok(_) => {
            let updates_json: Vec<Value> = collected_updates
                .iter()
                .map(|u| json!({"path": u.path, "value": u.value}))
                .collect();
            (json!({"status": "ok", "updates": updates_json}), 0)
        }
        Err(e) => {
            let msg = e.to_string();
            let message = if msg.contains("Invalid JSON") || msg.contains("JSON error") {
                format!("Could not read state file: {}", msg)
            } else {
                msg
            };
            (json!({"status": "error", "message": message}), 1)
        }
    }
}

/// Run the set-timestamp command.
pub fn run(set_args: Vec<String>, branch_override: Option<String>) {
    let root = project_root();
    let cwd = std::env::current_dir().unwrap_or(std::path::PathBuf::from("."));
    let (value, code) = run_impl_main(&set_args, branch_override.as_deref(), &root, &cwd);
    if code == 0 {
        // Emit the success shape via the existing json_ok helper to
        // preserve the historical key ordering and pretty-printing.
        let updates = value.get("updates").cloned().unwrap_or(json!([]));
        json_ok(&[("updates", updates)]);
    } else {
        let msg = value
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("error");
        json_error(msg, &[]);
        process::exit(code);
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

    /// Exercises the no-state-file path: a valid branch is provided but
    /// no `.flow-states/<branch>.json` exists.
    #[test]
    fn run_impl_main_no_state_file_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (value, code) = run_impl_main(
            &["foo=bar".to_string()],
            Some("set-ts-no-state"),
            &root,
            &root,
        );
        assert_eq!(code, 1);
        assert_eq!(value["status"], "error");
        assert!(value["message"]
            .as_str()
            .unwrap()
            .contains("No state file found"));
    }

    /// Exercises the apply_updates error path: a malformed `--set` arg
    /// (missing `=`) causes apply_updates to return Err. The state file
    /// must remain unchanged because the closure restores its snapshot
    /// before mutate_state writes.
    #[test]
    fn run_impl_main_invalid_set_arg_returns_error_and_preserves_state() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let state_dir = root.join(".flow-states");
        std::fs::create_dir_all(&state_dir).unwrap();
        let state_path = state_dir.join("set-ts-invalid.json");
        let original = r#"{"branch":"set-ts-invalid","existing":"value"}"#;
        std::fs::write(&state_path, original).unwrap();

        let (value, code) = run_impl_main(
            &["no_equals_sign".to_string()],
            Some("set-ts-invalid"),
            &root,
            &root,
        );
        assert_eq!(code, 1);
        assert_eq!(value["status"], "error");
        assert!(value["message"]
            .as_str()
            .unwrap()
            .contains("expected path=value"));

        let after: Value =
            serde_json::from_str(&std::fs::read_to_string(&state_path).unwrap()).unwrap();
        assert_eq!(after["existing"], "value");
    }

    /// Exercises the mutate_state error path: corrupt JSON in the state
    /// file → mutate_state returns Err which is rewritten to "Could not
    /// read state file: <details>".
    #[test]
    fn run_impl_main_corrupt_state_file_returns_could_not_read_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let state_dir = root.join(".flow-states");
        std::fs::create_dir_all(&state_dir).unwrap();
        let state_path = state_dir.join("set-ts-corrupt.json");
        std::fs::write(&state_path, "{not-json").unwrap();

        let (value, code) = run_impl_main(
            &["foo=bar".to_string()],
            Some("set-ts-corrupt"),
            &root,
            &root,
        );
        assert_eq!(code, 1);
        assert_eq!(value["status"], "error");
        assert!(value["message"]
            .as_str()
            .unwrap()
            .contains("Could not read state file"));
    }

    /// Direct unit test for `value_type_name` covering the Array and
    /// Object arms (lines 97-98). Both production callsites match Array
    /// and Object before falling into the catch-all that calls this
    /// function, so the arms are unreachable from production but kept
    /// for forward-compat if a new callsite ever reaches them.
    #[test]
    fn value_type_name_covers_all_variants() {
        assert_eq!(value_type_name(&Value::Null), "NoneType");
        assert_eq!(value_type_name(&json!(true)), "bool");
        assert_eq!(value_type_name(&json!(7)), "int");
        assert_eq!(value_type_name(&json!("hello")), "str");
        assert_eq!(value_type_name(&json!([1, 2])), "list");
        assert_eq!(value_type_name(&json!({"k": "v"})), "dict");
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
