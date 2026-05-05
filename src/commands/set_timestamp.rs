use serde_json::{json, Value};

use crate::flow_paths::FlowPaths;
use crate::git::resolve_branch;
use crate::lock::mutate_state;
use crate::utils::now;

/// A single path=value update that was applied.
#[derive(Debug)]
pub struct Update {
    pub path: String,
    pub value: Value,
}

/// Closed enumeration of the five step-counter fields that
/// trigger an account-window step snapshot capture per
/// `.claude/rules/scope-enumeration.md`. The single source of
/// truth — Task 11 reads this when deciding whether to append to
/// `phases.<n>.step_snapshots[]` after a `set-timestamp` call.
const STEP_COUNTER_FIELDS: &[&str] = &[
    "plan_step",
    "code_task",
    "code_review_step",
    "learn_step",
    "complete_step",
];

/// Returns `true` when `field` names one of the five step
/// counters. Whitespace and case are not normalized — callers pass
/// the field name straight from CLI argument parsing where
/// `--set <field>=<value>` already produced an exact slice of the
/// argument before the `=`.
pub fn is_step_counter_field(field: &str) -> bool {
    STEP_COUNTER_FIELDS.contains(&field)
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
            Value::Null => {
                return Err(format!("Cannot navigate into NoneType with key '{}'", part))
            }
            Value::Bool(_) => return Err(format!("Cannot navigate into bool with key '{}'", part)),
            Value::Number(_) => {
                return Err(format!("Cannot navigate into int with key '{}'", part))
            }
            Value::String(_) => {
                return Err(format!("Cannot navigate into str with key '{}'", part))
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
        Value::Null => return Err(format!("Cannot set key '{}' on NoneType", key)),
        Value::Bool(_) => return Err(format!("Cannot set key '{}' on bool", key)),
        Value::Number(_) => return Err(format!("Cannot set key '{}' on int", key)),
        Value::String(_) => return Err(format!("Cannot set key '{}' on str", key)),
    }

    Ok(())
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
/// `root`/`cwd` to bypass cwd-scope drift and the on-disk state file.
///
/// Branch resolution uses [`resolve_branch`] (consults
/// `FLOW_SIMULATE_BRANCH` and falls back to `git branch --show-current`
/// in the process cwd). The `None` branch is covered by subprocess
/// tests that spawn the binary with `current_dir` pointing at a
/// non-git tempdir.
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
    // restore the original — `apply_updates` mutates in place.
    let home = crate::window_snapshot::home_dir_or_empty();
    let result = mutate_state(&state_path, &mut |state| {
        let backup = state.clone();
        match apply_updates(state, set_args) {
            Ok(updates) => {
                // Per-step-counter snapshot: for each successful update
                // whose field is one of the five step counters, capture
                // a window snapshot and append it to
                // phases.<current_phase>.step_snapshots[].
                let current_phase = state
                    .get("current_phase")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if !current_phase.is_empty() {
                    for update in &updates {
                        if !is_step_counter_field(&update.path) {
                            continue;
                        }
                        let step = update.value.as_i64().unwrap_or(0);
                        let snap = crate::window_snapshot::capture_for_active_state(
                            &home, state, root,
                        );
                        crate::window_snapshot::append_step_snapshot(
                            state,
                            &current_phase,
                            step,
                            &update.path,
                            snap,
                        );
                    }
                }
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
