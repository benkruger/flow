//! StopFailure hook: capture error type/message into the state file.
//!
//! Tests live at tests/stop_failure.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use std::path::Path;

use serde_json::{json, Value};

use crate::git::project_root;
use crate::lock::mutate_state;
use crate::utils::now;

/// Capture StopFailure event data into the state file.
///
/// Writes `_last_failure` object with type, message, and timestamp.
/// Requires error_type key in hook_input to confirm this is a real
/// StopFailure event.
pub fn capture_failure_data(hook_input: &Value, state_path: &Path) {
    if hook_input.get("error_type").is_none() {
        return;
    }

    let error_type = hook_input
        .get("error_type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let error_message = hook_input
        .get("error_message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let timestamp = now();

    let _ = mutate_state(state_path, &mut |state| {
        // Guard: state must be an object (or Null, which auto-converts)
        // for string-key mutations. Arrays/bools/numbers/strings would
        // panic on `state["_last_failure"] = v`. Fail-open.
        if !(state.is_object() || state.is_null()) {
            return;
        }
        state["_last_failure"] = json!({
            "type": error_type,
            "message": error_message,
            "timestamp": timestamp,
        });
    });
}

/// Run the stop-failure hook (entry point).
///
/// Resolves the stdin payload and the active flow's state file via the
/// shared `read_hook_input_and_state` helper, then captures failure
/// data. `--branch` override support and the no-active-flow fail-open
/// posture live in the helper; this entry point owns only the
/// `project_root()` call and the `capture_failure_data` dispatch.
pub fn run() {
    let root = project_root();
    if let Some((hook_input, state_path)) = crate::hooks::read_hook_input_and_state(&root) {
        capture_failure_data(&hook_input, &state_path);
    }
}
