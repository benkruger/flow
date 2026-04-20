//! StopFailure hook: capture error type/message into the state file.
//!
//! Tests live at tests/stop_failure.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use std::io::Read;
use std::path::Path;

use serde_json::{json, Value};

use crate::flow_paths::FlowPaths;
use crate::git::{project_root, resolve_branch};
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

    let _ = mutate_state(state_path, |state| {
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
/// Uses `resolve_branch` for `--branch` override support. Calls
/// `current_branch()` internally — does not scan `.flow-states/`.
pub fn run() {
    let mut stdin_buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut stdin_buf);

    let hook_input: Value = match serde_json::from_str(&stdin_buf) {
        Ok(v) => v,
        Err(_) => return,
    };

    let root = project_root();
    let branch = resolve_branch(None, &root);
    let branch = match branch {
        Some(b) => b,
        None => return,
    };

    // Slash-containing git branches are not valid FLOW branches —
    // treat as "no active flow" and return rather than panicking.
    let state_path = match FlowPaths::try_new(&root, &branch) {
        Some(p) => p.state_file(),
        None => return,
    };

    if !state_path.exists() {
        return;
    }

    capture_failure_data(&hook_input, &state_path);
}
