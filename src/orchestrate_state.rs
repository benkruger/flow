//! Manage orchestration queue state at `.flow-states/orchestrate.json`.
//!
//! Ported from `lib/orchestrate-state.py`. Orchestrate.json is a machine-level
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
        match std::fs::read_to_string(&state_path) {
            Ok(content) => {
                if let Ok(existing) = serde_json::from_str::<Value>(&content) {
                    if existing.get("completed_at").map_or(true, |v| v.is_null()) {
                        return json!({
                            "status": "error",
                            "message": "Orchestration already in progress. Complete or abort the current run first."
                        });
                    }
                }
            }
            Err(_) => {} // File unreadable — overwrite it
        }
    }

    let queue_items: Vec<Value> = queue.iter().map(|issue| build_queue_item(issue)).collect();

    let state = json!({
        "started_at": now(),
        "completed_at": null,
        "queue": queue_items,
        "current_index": null,
    });

    match serde_json::to_string_pretty(&state) {
        Ok(content) => {
            if let Err(e) = std::fs::write(&state_path, content) {
                return json!({"status": "error", "message": format!("Failed to write state: {}", e)});
            }
            json!({"status": "ok"})
        }
        Err(e) => json!({"status": "error", "message": format!("Failed to serialize: {}", e)}),
    }
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

    match mutate_state(state_path, |state| {
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

    match mutate_state(state_path, |state| {
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

    match mutate_state(state_path, |state| {
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
/// (matching Python's always-exit-0 behavior). Returns `Err(msg)` only
/// for infrastructure failures.
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

pub fn run(args: Args) {
    match run_impl(&args) {
        Ok(value) => {
            println!("{}", value);
        }
        Err(msg) => {
            println!("{}", json!({"status": "error", "message": msg}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sample_queue() -> Vec<Value> {
        vec![
            json!({"issue_number": 42, "title": "Add PDF export"}),
            json!({"issue_number": 43, "title": "Fix login timeout"}),
            json!({"issue_number": 44, "title": "Refactor auth middleware"}),
        ]
    }

    // --- create_state ---

    #[test]
    fn test_create_state() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");

        let result = create_state(&sample_queue(), &state_dir);
        assert_eq!(result["status"], "ok");

        let content = fs::read_to_string(state_dir.join("orchestrate.json")).unwrap();
        let state: Value = serde_json::from_str(&content).unwrap();

        assert!(state["started_at"].as_str().unwrap().contains("T"));
        assert!(state["completed_at"].is_null());
        assert!(state["current_index"].is_null());

        let queue = state["queue"].as_array().unwrap();
        assert_eq!(queue.len(), 3);
        assert_eq!(queue[0]["issue_number"], 42);
        assert_eq!(queue[0]["status"], "pending");
        assert!(queue[0]["started_at"].is_null());
        assert!(queue[0]["completed_at"].is_null());
        assert!(queue[0]["outcome"].is_null());
        assert!(queue[0]["pr_url"].is_null());
        assert!(queue[0]["branch"].is_null());
        assert!(queue[0]["reason"].is_null());
    }

    #[test]
    fn test_create_state_empty_queue() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");

        let result = create_state(&[], &state_dir);
        assert_eq!(result["status"], "ok");

        let content = fs::read_to_string(state_dir.join("orchestrate.json")).unwrap();
        let state: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(state["queue"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_create_state_already_exists_in_progress() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();

        let existing = json!({
            "started_at": "2026-03-20T20:00:00-07:00",
            "completed_at": null,
            "queue": [],
            "current_index": 0,
        });
        fs::write(
            state_dir.join("orchestrate.json"),
            serde_json::to_string_pretty(&existing).unwrap(),
        )
        .unwrap();

        let result = create_state(&sample_queue(), &state_dir);
        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("already in progress"));
    }

    #[test]
    fn test_create_state_overwrites_completed() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();

        let existing = json!({
            "started_at": "2026-03-20T20:00:00-07:00",
            "completed_at": "2026-03-20T21:00:00-07:00",
            "queue": [],
            "current_index": null,
        });
        fs::write(
            state_dir.join("orchestrate.json"),
            serde_json::to_string_pretty(&existing).unwrap(),
        )
        .unwrap();

        let result = create_state(&sample_queue(), &state_dir);
        assert_eq!(result["status"], "ok");

        let content = fs::read_to_string(state_dir.join("orchestrate.json")).unwrap();
        let state: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(state["queue"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_create_state_creates_directory() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        // Don't create state_dir — create_state should do it
        assert!(!state_dir.exists());

        let result = create_state(&sample_queue(), &state_dir);
        assert_eq!(result["status"], "ok");
        assert!(state_dir.join("orchestrate.json").exists());
    }

    // --- start_issue ---

    #[test]
    fn test_start_issue() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        create_state(&sample_queue(), &state_dir);

        let state_path = state_dir.join("orchestrate.json");
        let result = start_issue(&state_path, 0);
        assert_eq!(result["status"], "ok");

        let content = fs::read_to_string(&state_path).unwrap();
        let state: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(state["current_index"], 0);
        assert_eq!(state["queue"][0]["status"], "in_progress");
        assert!(state["queue"][0]["started_at"]
            .as_str()
            .unwrap()
            .contains("T"));
    }

    #[test]
    fn test_start_issue_out_of_range() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        create_state(&sample_queue(), &state_dir);

        let state_path = state_dir.join("orchestrate.json");
        let result = start_issue(&state_path, 10);
        assert_eq!(result["status"], "error");
        assert!(result["message"].as_str().unwrap().contains("out of range"));
    }

    #[test]
    fn test_start_issue_missing_state() {
        let dir = tempfile::tempdir().unwrap();
        let result = start_issue(&dir.path().join("missing.json"), 0);
        assert_eq!(result["status"], "error");
        assert!(result["message"].as_str().unwrap().contains("not found"));
    }

    #[test]
    fn test_start_issue_non_object_state() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let state_path = state_dir.join("orchestrate.json");
        fs::write(&state_path, "[1, 2, 3]").unwrap();

        let result = start_issue(&state_path, 0);
        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("not a JSON object"));
    }

    // --- record_outcome ---

    #[test]
    fn test_record_outcome_completed() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        create_state(&sample_queue(), &state_dir);

        let state_path = state_dir.join("orchestrate.json");
        start_issue(&state_path, 0);

        let result = record_outcome(
            &state_path,
            0,
            "completed",
            Some("https://github.com/test/test/pull/100"),
            Some("add-pdf-export"),
            None,
        );
        assert_eq!(result["status"], "ok");

        let content = fs::read_to_string(&state_path).unwrap();
        let state: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(state["queue"][0]["status"], "completed");
        assert_eq!(state["queue"][0]["outcome"], "completed");
        assert!(state["queue"][0]["completed_at"]
            .as_str()
            .unwrap()
            .contains("T"));
        assert_eq!(
            state["queue"][0]["pr_url"],
            "https://github.com/test/test/pull/100"
        );
        assert_eq!(state["queue"][0]["branch"], "add-pdf-export");
    }

    #[test]
    fn test_record_outcome_failed() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        create_state(&sample_queue(), &state_dir);

        let state_path = state_dir.join("orchestrate.json");
        start_issue(&state_path, 1);

        let result = record_outcome(
            &state_path,
            1,
            "failed",
            None,
            None,
            Some("CI failed after 3 attempts"),
        );
        assert_eq!(result["status"], "ok");

        let content = fs::read_to_string(&state_path).unwrap();
        let state: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(state["queue"][1]["status"], "failed");
        assert_eq!(state["queue"][1]["outcome"], "failed");
        assert_eq!(state["queue"][1]["reason"], "CI failed after 3 attempts");
    }

    #[test]
    fn test_record_outcome_out_of_range() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        create_state(&sample_queue(), &state_dir);

        let state_path = state_dir.join("orchestrate.json");
        let result = record_outcome(&state_path, 10, "completed", None, None, None);
        assert_eq!(result["status"], "error");
        assert!(result["message"].as_str().unwrap().contains("out of range"));
    }

    #[test]
    fn test_record_outcome_missing_state() {
        let dir = tempfile::tempdir().unwrap();
        let result = record_outcome(
            &dir.path().join("missing.json"),
            0,
            "completed",
            None,
            None,
            None,
        );
        assert_eq!(result["status"], "error");
        assert!(result["message"].as_str().unwrap().contains("not found"));
    }

    #[test]
    fn test_record_outcome_non_object_state() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let state_path = state_dir.join("orchestrate.json");
        fs::write(&state_path, "[1, 2, 3]").unwrap();

        let result = record_outcome(&state_path, 0, "completed", None, None, None);
        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("not a JSON object"));
    }

    // --- complete ---

    #[test]
    fn test_complete() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        create_state(&sample_queue(), &state_dir);

        let state_path = state_dir.join("orchestrate.json");
        let result = complete_orchestration(&state_path);
        assert_eq!(result["status"], "ok");

        let content = fs::read_to_string(&state_path).unwrap();
        let state: Value = serde_json::from_str(&content).unwrap();
        assert!(state["completed_at"].as_str().unwrap().contains("T"));
    }

    #[test]
    fn test_complete_missing_state() {
        let dir = tempfile::tempdir().unwrap();
        let result = complete_orchestration(&dir.path().join("missing.json"));
        assert_eq!(result["status"], "error");
        assert!(result["message"].as_str().unwrap().contains("not found"));
    }

    #[test]
    fn test_complete_non_object_state() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let state_path = state_dir.join("orchestrate.json");
        fs::write(&state_path, "[1, 2, 3]").unwrap();

        let result = complete_orchestration(&state_path);
        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("not a JSON object"));
    }

    // --- read_state ---

    #[test]
    fn test_read_state() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        create_state(&sample_queue(), &state_dir);

        let state_path = state_dir.join("orchestrate.json");
        let result = read_state(&state_path);
        assert_eq!(result["status"], "ok");
        assert!(result["state"]["started_at"]
            .as_str()
            .unwrap()
            .contains("T"));
        assert_eq!(result["state"]["queue"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_read_state_missing() {
        let dir = tempfile::tempdir().unwrap();
        let result = read_state(&dir.path().join("missing.json"));
        assert_eq!(result["status"], "error");
        assert!(result["message"].as_str().unwrap().contains("not found"));
    }

    // --- next_issue ---

    #[test]
    fn test_next_issue() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        create_state(&sample_queue(), &state_dir);

        let state_path = state_dir.join("orchestrate.json");
        let result = next_issue(&state_path);
        assert_eq!(result["status"], "ok");
        assert_eq!(result["index"], 0);
        assert_eq!(result["issue_number"], 42);
        assert_eq!(result["title"], "Add PDF export");
    }

    #[test]
    fn test_next_issue_skips_completed() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        create_state(&sample_queue(), &state_dir);

        let state_path = state_dir.join("orchestrate.json");
        start_issue(&state_path, 0);
        record_outcome(&state_path, 0, "completed", None, None, None);

        let result = next_issue(&state_path);
        assert_eq!(result["status"], "ok");
        assert_eq!(result["index"], 1);
        assert_eq!(result["issue_number"], 43);
    }

    #[test]
    fn test_next_issue_all_done() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        create_state(
            &[json!({"issue_number": 42, "title": "One issue"})],
            &state_dir,
        );

        let state_path = state_dir.join("orchestrate.json");
        start_issue(&state_path, 0);
        record_outcome(&state_path, 0, "completed", None, None, None);

        let result = next_issue(&state_path);
        assert_eq!(result["status"], "done");
    }

    #[test]
    fn test_next_issue_missing_state() {
        let dir = tempfile::tempdir().unwrap();
        let result = next_issue(&dir.path().join("missing.json"));
        assert_eq!(result["status"], "error");
        assert!(result["message"].as_str().unwrap().contains("not found"));
    }

    // --- CLI run_impl tests ---

    #[test]
    fn test_cli_create() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();

        let queue_file = dir.path().join("queue.json");
        fs::write(&queue_file, serde_json::to_string(&sample_queue()).unwrap()).unwrap();

        let args = Args {
            create: true,
            start_issue: None,
            record_outcome: None,
            complete: false,
            read: false,
            next: false,
            queue_file: Some(queue_file.to_string_lossy().to_string()),
            state_dir: Some(state_dir.to_string_lossy().to_string()),
            state_file: None,
            outcome: None,
            pr_url: None,
            branch: None,
            reason: None,
        };

        let result = run_impl(&args).unwrap();
        assert_eq!(result["status"], "ok");
        assert!(state_dir.join("orchestrate.json").exists());
    }

    #[test]
    fn test_cli_create_missing_queue_file() {
        let args = Args {
            create: true,
            start_issue: None,
            record_outcome: None,
            complete: false,
            read: false,
            next: false,
            queue_file: None,
            state_dir: None,
            state_file: None,
            outcome: None,
            pr_url: None,
            branch: None,
            reason: None,
        };

        let result = run_impl(&args).unwrap();
        assert_eq!(result["status"], "error");
        assert!(result["message"].as_str().unwrap().contains("--queue-file"));
    }

    #[test]
    fn test_cli_start_issue() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        create_state(&sample_queue(), &state_dir);

        let state_path = state_dir.join("orchestrate.json");
        let args = Args {
            create: false,
            start_issue: Some(0),
            record_outcome: None,
            complete: false,
            read: false,
            next: false,
            queue_file: None,
            state_dir: None,
            state_file: Some(state_path.to_string_lossy().to_string()),
            outcome: None,
            pr_url: None,
            branch: None,
            reason: None,
        };

        let result = run_impl(&args).unwrap();
        assert_eq!(result["status"], "ok");
    }

    #[test]
    fn test_cli_start_issue_missing_state_file() {
        let args = Args {
            create: false,
            start_issue: Some(0),
            record_outcome: None,
            complete: false,
            read: false,
            next: false,
            queue_file: None,
            state_dir: None,
            state_file: None,
            outcome: None,
            pr_url: None,
            branch: None,
            reason: None,
        };

        let result = run_impl(&args).unwrap();
        assert_eq!(result["status"], "error");
        assert!(result["message"].as_str().unwrap().contains("--state-file"));
    }

    #[test]
    fn test_cli_record_outcome() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        create_state(&sample_queue(), &state_dir);

        let state_path = state_dir.join("orchestrate.json");
        start_issue(&state_path, 0);

        let args = Args {
            create: false,
            start_issue: None,
            record_outcome: Some(0),
            complete: false,
            read: false,
            next: false,
            queue_file: None,
            state_dir: None,
            state_file: Some(state_path.to_string_lossy().to_string()),
            outcome: Some("completed".to_string()),
            pr_url: Some("https://github.com/test/test/pull/100".to_string()),
            branch: Some("add-pdf-export".to_string()),
            reason: None,
        };

        let result = run_impl(&args).unwrap();
        assert_eq!(result["status"], "ok");
    }

    #[test]
    fn test_cli_record_outcome_missing_state_file() {
        let args = Args {
            create: false,
            start_issue: None,
            record_outcome: Some(0),
            complete: false,
            read: false,
            next: false,
            queue_file: None,
            state_dir: None,
            state_file: None,
            outcome: Some("completed".to_string()),
            pr_url: None,
            branch: None,
            reason: None,
        };

        let result = run_impl(&args).unwrap();
        assert_eq!(result["status"], "error");
        assert!(result["message"].as_str().unwrap().contains("--state-file"));
    }

    #[test]
    fn test_cli_record_outcome_missing_outcome() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        create_state(&sample_queue(), &state_dir);

        let state_path = state_dir.join("orchestrate.json");

        let args = Args {
            create: false,
            start_issue: None,
            record_outcome: Some(0),
            complete: false,
            read: false,
            next: false,
            queue_file: None,
            state_dir: None,
            state_file: Some(state_path.to_string_lossy().to_string()),
            outcome: None,
            pr_url: None,
            branch: None,
            reason: None,
        };

        let result = run_impl(&args).unwrap();
        assert_eq!(result["status"], "error");
        assert!(result["message"].as_str().unwrap().contains("--outcome"));
    }

    #[test]
    fn test_cli_complete() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        create_state(&sample_queue(), &state_dir);

        let state_path = state_dir.join("orchestrate.json");
        let args = Args {
            create: false,
            start_issue: None,
            record_outcome: None,
            complete: true,
            read: false,
            next: false,
            queue_file: None,
            state_dir: None,
            state_file: Some(state_path.to_string_lossy().to_string()),
            outcome: None,
            pr_url: None,
            branch: None,
            reason: None,
        };

        let result = run_impl(&args).unwrap();
        assert_eq!(result["status"], "ok");
    }

    #[test]
    fn test_cli_complete_missing_state_file() {
        let args = Args {
            create: false,
            start_issue: None,
            record_outcome: None,
            complete: true,
            read: false,
            next: false,
            queue_file: None,
            state_dir: None,
            state_file: None,
            outcome: None,
            pr_url: None,
            branch: None,
            reason: None,
        };

        let result = run_impl(&args).unwrap();
        assert_eq!(result["status"], "error");
        assert!(result["message"].as_str().unwrap().contains("--state-file"));
    }

    #[test]
    fn test_cli_read() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        create_state(&sample_queue(), &state_dir);

        let state_path = state_dir.join("orchestrate.json");
        let args = Args {
            create: false,
            start_issue: None,
            record_outcome: None,
            complete: false,
            read: true,
            next: false,
            queue_file: None,
            state_dir: None,
            state_file: Some(state_path.to_string_lossy().to_string()),
            outcome: None,
            pr_url: None,
            branch: None,
            reason: None,
        };

        let result = run_impl(&args).unwrap();
        assert_eq!(result["status"], "ok");
        assert!(result.get("state").is_some());
    }

    #[test]
    fn test_cli_read_missing_state_file() {
        let args = Args {
            create: false,
            start_issue: None,
            record_outcome: None,
            complete: false,
            read: true,
            next: false,
            queue_file: None,
            state_dir: None,
            state_file: None,
            outcome: None,
            pr_url: None,
            branch: None,
            reason: None,
        };

        let result = run_impl(&args).unwrap();
        assert_eq!(result["status"], "error");
        assert!(result["message"].as_str().unwrap().contains("--state-file"));
    }

    #[test]
    fn test_cli_next() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        create_state(&sample_queue(), &state_dir);

        let state_path = state_dir.join("orchestrate.json");
        let args = Args {
            create: false,
            start_issue: None,
            record_outcome: None,
            complete: false,
            read: false,
            next: true,
            queue_file: None,
            state_dir: None,
            state_file: Some(state_path.to_string_lossy().to_string()),
            outcome: None,
            pr_url: None,
            branch: None,
            reason: None,
        };

        let result = run_impl(&args).unwrap();
        assert_eq!(result["status"], "ok");
        assert_eq!(result["index"], 0);
    }

    #[test]
    fn test_cli_next_missing_state_file() {
        let args = Args {
            create: false,
            start_issue: None,
            record_outcome: None,
            complete: false,
            read: false,
            next: true,
            queue_file: None,
            state_dir: None,
            state_file: None,
            outcome: None,
            pr_url: None,
            branch: None,
            reason: None,
        };

        let result = run_impl(&args).unwrap();
        assert_eq!(result["status"], "error");
        assert!(result["message"].as_str().unwrap().contains("--state-file"));
    }

    #[test]
    fn test_cli_read_missing_state() {
        let dir = tempfile::tempdir().unwrap();
        let args = Args {
            create: false,
            start_issue: None,
            record_outcome: None,
            complete: false,
            read: true,
            next: false,
            queue_file: None,
            state_dir: None,
            state_file: Some(
                dir.path()
                    .join("missing.json")
                    .to_string_lossy()
                    .to_string(),
            ),
            outcome: None,
            pr_url: None,
            branch: None,
            reason: None,
        };

        let result = run_impl(&args).unwrap();
        assert_eq!(result["status"], "error");
    }

    #[test]
    fn test_cli_exception_handling() {
        let dir = tempfile::tempdir().unwrap();
        let bad_file = dir.path().join("bad.json");
        fs::write(&bad_file, "{corrupt json").unwrap();

        let args = Args {
            create: false,
            start_issue: Some(0),
            record_outcome: None,
            complete: false,
            read: false,
            next: false,
            queue_file: None,
            state_dir: None,
            state_file: Some(bad_file.to_string_lossy().to_string()),
            outcome: None,
            pr_url: None,
            branch: None,
            reason: None,
        };

        let result = run_impl(&args).unwrap();
        assert_eq!(result["status"], "error");
    }
}
