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

/// Closed enumeration of the step-counter fields that
/// trigger an account-window step snapshot capture. The single
/// source of truth — code reads this when deciding whether to
/// append to `phases.<n>.step_snapshots[]` after a
/// `set-timestamp` call.
const STEP_COUNTER_FIELDS: &[&str] = &["code_task", "review_step", "complete_step"];

/// Returns `true` when `field` names one of the recognized step
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
/// Missing intermediate object keys are auto-vivified as empty
/// objects, so a dot-path can create the nesting it needs even when
/// an intermediate segment is absent from the state file until its
/// first write — a path of the shape `phases.<phase>.<section>.<key>`
/// creates the `<section>` object before setting `<key>`.
/// Present-but-wrong-type intermediates (a string, number, bool, or
/// null where an object or array is expected) and out-of-range array
/// indices still error.
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
            Value::Object(map) => map.entry(part.to_string()).or_insert_with(|| json!({})),
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

/// Closed set of state fields the model is denied writing through
/// the `bin/flow set-timestamp` CLI boundary. These fields carry
/// trust signals that the autonomous `continue: auto` contract
/// relies on — the model must not be able to counterfeit them
/// through the CLI.
///
/// `_halt_pending` is set by the Stop hook
/// (`stop_continue::check_autonomous_stop`) in response to a real
/// user message and read by every subsequent Stop event to refuse
/// the turn-end. The hook writes the field via in-process
/// `mutate_state` calls — never via the CLI — so denying the CLI
/// path leaves the legitimate writer untouched. See
/// `.claude/rules/autonomous-phase-discipline.md` "Mechanical
/// halt-pause contract".
///
/// `_last_observed_code_task` and `_consecutive_unchanged_count`
/// are the hook-managed counter-tracking fields the Stop hook uses
/// to detect autonomous-mode stalling. The only legitimate way to
/// reset `_consecutive_unchanged_count` is for the model to advance
/// `code_task` via the existing monotonic-+1 validator. Allowing
/// CLI writes would let the model reset the counter directly,
/// silently bypassing the pointed refusal text that catches the
/// stalling loop. See `.claude/rules/autonomous-phase-discipline.md`
/// "Forbidden Stalling Frames".
const MODEL_DENIED_FIELDS: &[&str] = &[
    "_halt_pending",
    "_last_observed_code_task",
    "_consecutive_unchanged_count",
];

/// Strip NULs, trim, and ASCII-lowercase the input so case,
/// whitespace, and embedded-NUL bypass shapes are folded before
/// the equality check in [`validate_model_denied_field`]. See
/// `.claude/rules/security-gates.md` "Normalize Before Comparing".
fn normalize_field_name(s: &str) -> String {
    s.replace('\0', "").trim().to_ascii_lowercase()
}

/// Reject writes to fields in [`MODEL_DENIED_FIELDS`]. The deny
/// applies to both truthy and falsy writes — the model has no
/// business setting OR clearing these fields. The normalization
/// closes case, whitespace, and embedded-NUL bypass shapes.
fn validate_model_denied_field(field: &str) -> Result<(), String> {
    let normalized = normalize_field_name(field);
    if MODEL_DENIED_FIELDS.contains(&normalized.as_str()) {
        return Err(format!(
            "Field '{}' cannot be written via set-timestamp — it is owned by FLOW \
             hooks. See .claude/rules/autonomous-phase-discipline.md — \
             'Mechanical halt-pause contract' for _halt_pending, \
             'Forbidden Stalling Frames' for the counter-tracking fields.",
            field
        ));
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
///
/// **Non-atomic across multiple `--set` args.** Updates are
/// applied sequentially in place; an Err return from a later
/// arg (deny check, `code_task` validation, or `set_nested`
/// type error) propagates via `?` WITHOUT rolling back prior
/// in-place mutations to `state`. Callers that need
/// transactional semantics MUST snapshot `state` before
/// invocation and restore from the snapshot on Err — this is
/// what [`run_impl_main`] does via the `backup` / `*state =
/// backup` pattern inside the `mutate_state` closure. A direct
/// library caller that omits the snapshot will observe partial
/// mutation (e.g. `state["code_task"] == 1` even when the call
/// returns Err from a later denied arg). The current production
/// caller chain is the only consumer; future cross-module
/// consumers must follow the same pattern.
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

        // Reject CLI writes to model-denied top-level fields before
        // any state mutation. Scoped to single-segment paths because
        // the protected fields (e.g. `_halt_pending`) are read by
        // hooks at the top level only; a nested attempt is a no-op
        // for those readers.
        if path_parts.len() == 1 {
            validate_model_denied_field(path_parts[0])?;
        }

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

    // Per `.claude/rules/external-input-validation.md` "CLI subcommand
    // entry callsite discipline" + `.claude/rules/branch-path-safety.md`:
    // `--branch` is an external input. Slash-containing branches
    // (`feature/foo`, `dependabot/*`) and the empty string flow raw
    // from clap, so the panicking constructor would crash the CLI
    // with a backtrace. `try_new` returns `None` for invalid inputs;
    // translate that into a structured error.
    let state_path = match FlowPaths::try_new(root, &branch) {
        Some(p) => p.state_file(),
        None => {
            return (
                json!({
                    "status": "error",
                    "message": format!("Invalid branch name: {:?}", branch)
                }),
                1,
            );
        }
    };

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
    let home = crate::session_metrics::home_dir_or_empty();
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
                        let snap =
                            crate::per_flow_capture::capture_for_active_state(&home, state, root);
                        crate::session_metrics::append_step_snapshot(
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
