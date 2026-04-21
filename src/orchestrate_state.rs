//! Manage orchestration queue state at `.flow-states/orchestrate.json`.
//!
//! Orchestrate.json is a machine-level
//! singleton (not branch-scoped) that tracks overnight autonomous execution.
//!
//! Six mutually exclusive operations:
//! - `--create` — build queue from `--queue-file`, write new state
//! - `--start-issue INDEX` — mark queue item as in_progress
//! - `--record-outcome INDEX` — record completed/failed with optional metadata
//! - `--complete` — set completed_at
//! - `--read` — return current state
//! - `--next` — find next pending issue

use std::path::Path;

use clap::{ArgGroup, Parser};
use serde_json::{json, Value};

use crate::lock::mutate_state;
use crate::utils::now;

/// Build a normalized queue item from an issue dict.
///
/// Input must have `issue_number` (integer) and `title` (string).
/// Returns a queue item with pending status and null metadata fields.
fn build_queue_item(issue: &Value) -> Value {
    json!({
        "issue_number": issue.get("issue_number").and_then(|v| v.as_i64()).unwrap_or(0),
        "title": issue.get("title").and_then(|v| v.as_str()).unwrap_or(""),
        "status": "pending",
        "started_at": null,
        "completed_at": null,
        "outcome": null,
        "pr_url": null,
        "branch": null,
        "reason": null,
    })
}

/// Create orchestrate.json with the given issue queue.
///
/// Returns `{"status": "ok"}` on success.
/// Returns `{"status": "error", "message": "..."}` if an in-progress
/// orchestration already exists (completed_at is null).
/// Overwrites completed orchestrations. Creates the state directory
/// if it does not exist.
pub fn create_state(queue: &[Value], state_dir: &Path) -> Value {
    if let Err(e) = std::fs::create_dir_all(state_dir) {
        return json!({"status": "error", "message": format!("Failed to create directory: {}", e)});
    }

    let state_path = state_dir.join("orchestrate.json");

    if state_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&state_path) {
            if let Ok(existing) = serde_json::from_str::<Value>(&content) {
                if existing.get("completed_at").is_none_or(|v| v.is_null()) {
                    return json!({
                        "status": "error",
                        "message": "Orchestration already in progress. Complete or abort the current run first."
                    });
                }
            }
        }
    }

    let queue_items: Vec<Value> = queue.iter().map(build_queue_item).collect();

    let state = json!({
        "started_at": now(),
        "completed_at": null,
        "queue": queue_items,
        "current_index": null,
    });

    // serde_json::to_string_pretty on a `json!({...})` literal is infallible.
    let content = serde_json::to_string_pretty(&state).expect("json! literal serializes");
    if let Err(e) = std::fs::write(&state_path, content) {
        return json!({"status": "error", "message": format!("Failed to write state: {}", e)});
    }
    json!({"status": "ok"})
}

/// Mark queue item at index as in_progress.
///
/// Returns `{"status": "ok"}` on success.
/// Returns `{"status": "error", "message": "..."}` if the state file
/// is missing or the index is out of range.
pub fn start_issue(state_path: &Path, index: i64) -> Value {
    if !state_path.exists() {
        return json!({
            "status": "error",
            "message": format!("State file not found: {}", state_path.display())
        });
    }

    let mut error_result: Option<Value> = None;

    match mutate_state(state_path, &mut |state| {
        if !(state.is_object() || state.is_null()) {
            error_result =
                Some(json!({"status": "error", "message": "State file is not a JSON object"}));
            return;
        }
        let queue_len = state
            .get("queue")
            .and_then(|v| v.as_array())
            .map_or(0, |a| a.len());
        if index < 0 || (index as usize) >= queue_len {
            error_result = Some(json!({
                "status": "error",
                "message": format!("Index {} out of range (queue has {} items)", index, queue_len)
            }));
            return;
        }
        let idx = index as usize;
        if !state["queue"][idx].is_object() {
            error_result =
                Some(json!({"status": "error", "message": "Queue item is not a JSON object"}));
            return;
        }
        state["current_index"] = json!(index);
        state["queue"][idx]["status"] = json!("in_progress");
        state["queue"][idx]["started_at"] = json!(now());
    }) {
        Ok(_) => error_result.unwrap_or_else(|| json!({"status": "ok"})),
        Err(e) => json!({"status": "error", "message": format!("{}", e)}),
    }
}

/// Record the outcome for a queue item.
///
/// `outcome` should be `"completed"` or `"failed"`.
/// Optional `pr_url`, `branch`, and `reason` are set only when provided
/// (non-empty).
pub fn record_outcome(
    state_path: &Path,
    index: i64,
    outcome: &str,
    pr_url: Option<&str>,
    branch: Option<&str>,
    reason: Option<&str>,
) -> Value {
    if !state_path.exists() {
        return json!({
            "status": "error",
            "message": format!("State file not found: {}", state_path.display())
        });
    }

    let mut error_result: Option<Value> = None;

    match mutate_state(state_path, &mut |state| {
        if !(state.is_object() || state.is_null()) {
            error_result =
                Some(json!({"status": "error", "message": "State file is not a JSON object"}));
            return;
        }
        let queue_len = state
            .get("queue")
            .and_then(|v| v.as_array())
            .map_or(0, |a| a.len());
        if index < 0 || (index as usize) >= queue_len {
            error_result = Some(json!({
                "status": "error",
                "message": format!("Index {} out of range (queue has {} items)", index, queue_len)
            }));
            return;
        }
        let idx = index as usize;
        if !state["queue"][idx].is_object() {
            error_result =
                Some(json!({"status": "error", "message": "Queue item is not a JSON object"}));
            return;
        }
        state["queue"][idx]["status"] = json!(outcome);
        state["queue"][idx]["outcome"] = json!(outcome);
        state["queue"][idx]["completed_at"] = json!(now());
        if let Some(url) = pr_url.filter(|s| !s.is_empty()) {
            state["queue"][idx]["pr_url"] = json!(url);
        }
        if let Some(b) = branch.filter(|s| !s.is_empty()) {
            state["queue"][idx]["branch"] = json!(b);
        }
        if let Some(r) = reason.filter(|s| !s.is_empty()) {
            state["queue"][idx]["reason"] = json!(r);
        }
    }) {
        Ok(_) => error_result.unwrap_or_else(|| json!({"status": "ok"})),
        Err(e) => json!({"status": "error", "message": format!("{}", e)}),
    }
}

/// Mark orchestration as complete by setting completed_at.
pub fn complete_orchestration(state_path: &Path) -> Value {
    if !state_path.exists() {
        return json!({
            "status": "error",
            "message": format!("State file not found: {}", state_path.display())
        });
    }

    let mut error_result: Option<Value> = None;

    match mutate_state(state_path, &mut |state| {
        if !(state.is_object() || state.is_null()) {
            error_result =
                Some(json!({"status": "error", "message": "State file is not a JSON object"}));
            return;
        }
        state["completed_at"] = json!(now());
    }) {
        Ok(_) => error_result.unwrap_or_else(|| json!({"status": "ok"})),
        Err(e) => json!({"status": "error", "message": format!("{}", e)}),
    }
}

/// Read and return the current orchestration state.
pub fn read_state(state_path: &Path) -> Value {
    if !state_path.exists() {
        return json!({
            "status": "error",
            "message": format!("State file not found: {}", state_path.display())
        });
    }

    match std::fs::read_to_string(state_path) {
        Ok(content) => match serde_json::from_str::<Value>(&content) {
            Ok(state) => json!({"status": "ok", "state": state}),
            Err(e) => json!({"status": "error", "message": format!("Invalid JSON: {}", e)}),
        },
        Err(e) => json!({"status": "error", "message": format!("Failed to read: {}", e)}),
    }
}

/// Find the next pending issue in the queue.
///
/// Returns `{"status": "ok", "index": N, "issue_number": N, "title": "..."}`,
/// or `{"status": "done"}` when all issues are processed.
pub fn next_issue(state_path: &Path) -> Value {
    if !state_path.exists() {
        return json!({
            "status": "error",
            "message": format!("State file not found: {}", state_path.display())
        });
    }

    match std::fs::read_to_string(state_path) {
        Ok(content) => match serde_json::from_str::<Value>(&content) {
            Ok(state) => {
                let queue = state.get("queue").and_then(|v| v.as_array());
                if let Some(items) = queue {
                    for (i, item) in items.iter().enumerate() {
                        if item.get("status").and_then(|v| v.as_str()) == Some("pending") {
                            return json!({
                                "status": "ok",
                                "index": i,
                                "issue_number": item.get("issue_number").and_then(|v| v.as_i64()).unwrap_or(0),
                                "title": item.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                            });
                        }
                    }
                }
                json!({"status": "done"})
            }
            Err(e) => json!({"status": "error", "message": format!("Invalid JSON: {}", e)}),
        },
        Err(e) => json!({"status": "error", "message": format!("Failed to read: {}", e)}),
    }
}

// --- CLI ---

#[derive(Parser, Debug)]
#[command(name = "orchestrate-state", about = "Manage orchestration queue state")]
#[command(group(ArgGroup::new("action").required(true).args([
    "create", "start_issue", "record_outcome", "complete", "read", "next"
])))]
pub struct Args {
    /// Create orchestrate.json
    #[arg(long)]
    pub create: bool,

    /// Mark issue at INDEX as in_progress
    #[arg(long, value_name = "INDEX")]
    pub start_issue: Option<i64>,

    /// Record outcome for issue at INDEX
    #[arg(long, value_name = "INDEX")]
    pub record_outcome: Option<i64>,

    /// Mark orchestration complete
    #[arg(long)]
    pub complete: bool,

    /// Read current state
    #[arg(long)]
    pub read: bool,

    /// Get next pending issue
    #[arg(long)]
    pub next: bool,

    /// Path to JSON file with issue queue (for --create)
    #[arg(long)]
    pub queue_file: Option<String>,

    /// Path to .flow-states/ directory (for --create)
    #[arg(long)]
    pub state_dir: Option<String>,

    /// Path to orchestrate.json
    #[arg(long)]
    pub state_file: Option<String>,

    /// Outcome for --record-outcome (completed or failed)
    #[arg(long, value_parser = ["completed", "failed"])]
    pub outcome: Option<String>,

    /// PR URL for completed issues
    #[arg(long)]
    pub pr_url: Option<String>,

    /// Branch name for completed issues
    #[arg(long)]
    pub branch: Option<String>,

    /// Failure reason for failed issues
    #[arg(long)]
    pub reason: Option<String>,
}

/// Testable implementation — returns the JSON Value to print.
///
/// Returns `Ok(value)` for both success and application-error responses
/// so the CLI prints the result and exits 0 in either case; orchestration
/// state mutations are best-effort and a malformed queue entry must not
/// halt the orchestrator. `Err(msg)` is reserved for infrastructure
/// failures (filesystem errors, unreadable plugin root) that should
/// exit 1.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    if args.create {
        let queue_path = match &args.queue_file {
            Some(p) => p,
            None => {
                return Ok(
                    json!({"status": "error", "message": "--queue-file required with --create"}),
                )
            }
        };
        let state_dir = args.state_dir.as_deref().unwrap_or(".flow-states");
        let content = std::fs::read_to_string(queue_path)
            .map_err(|e| format!("Failed to read queue file: {}", e))?;
        let queue: Vec<Value> =
            serde_json::from_str(&content).map_err(|e| format!("Invalid queue JSON: {}", e))?;
        Ok(create_state(&queue, Path::new(state_dir)))
    } else if let Some(index) = args.start_issue {
        match &args.state_file {
            Some(sf) => Ok(start_issue(Path::new(sf), index)),
            None => Ok(json!({"status": "error", "message": "--state-file required"})),
        }
    } else if let Some(index) = args.record_outcome {
        match &args.state_file {
            Some(sf) => match &args.outcome {
                Some(outcome) => Ok(record_outcome(
                    Path::new(sf),
                    index,
                    outcome,
                    args.pr_url.as_deref(),
                    args.branch.as_deref(),
                    args.reason.as_deref(),
                )),
                None => Ok(
                    json!({"status": "error", "message": "--outcome required with --record-outcome"}),
                ),
            },
            None => Ok(json!({"status": "error", "message": "--state-file required"})),
        }
    } else if args.complete {
        match &args.state_file {
            Some(sf) => Ok(complete_orchestration(Path::new(sf))),
            None => Ok(json!({"status": "error", "message": "--state-file required"})),
        }
    } else if args.read {
        match &args.state_file {
            Some(sf) => Ok(read_state(Path::new(sf))),
            None => Ok(json!({"status": "error", "message": "--state-file required"})),
        }
    } else if args.next {
        match &args.state_file {
            Some(sf) => Ok(next_issue(Path::new(sf))),
            None => Ok(json!({"status": "error", "message": "--state-file required"})),
        }
    } else {
        Ok(json!({"status": "error", "message": "No action specified"}))
    }
}

/// Main-arm dispatch: returns (value, 0). Err is wrapped into an error JSON.
pub fn run_impl_main(args: &Args) -> (Value, i32) {
    let value = match run_impl(args) {
        Ok(v) => v,
        Err(msg) => json!({"status": "error", "message": msg}),
    };
    (value, 0)
}
