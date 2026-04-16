use std::process;

use clap::Parser;
use serde_json::json;

use crate::flow_paths::FlowPaths;
use crate::git::{project_root, resolve_branch};
use crate::lock::mutate_state;
use crate::output::{json_error, json_ok};
use crate::phase_config::phase_names;
use crate::utils::now;

const MAX_PREVIEW_LENGTH: usize = 100;

#[derive(Parser, Debug)]
#[command(
    name = "add-notification",
    about = "Record a Slack notification in FLOW state"
)]
pub struct Args {
    /// Phase that sent the notification
    #[arg(long)]
    pub phase: String,

    /// Slack message timestamp
    #[arg(long)]
    pub ts: String,

    /// Slack thread timestamp
    #[arg(long)]
    pub thread_ts: String,

    /// Message text (truncated for preview)
    #[arg(long)]
    pub message: String,

    /// Override branch for state file lookup
    #[arg(long)]
    pub branch: Option<String>,
}

pub fn run(args: Args) {
    let root = project_root();
    let branch = match resolve_branch(args.branch.as_deref(), &root) {
        Some(b) => b,
        None => {
            json_error("Could not determine current branch", &[]);
            process::exit(1);
        }
    };
    let state_path = FlowPaths::new(&root, &branch).state_file();

    if !state_path.exists() {
        println!(r#"{{"status":"no_state"}}"#);
        process::exit(0);
    }

    let preview = truncate_preview(&args.message);
    let names = phase_names();
    let phase_name = names
        .get(&args.phase)
        .cloned()
        .unwrap_or_else(|| args.phase.clone());
    let timestamp = now();

    match mutate_state(&state_path, |state| {
        if !(state.is_object() || state.is_null()) {
            return;
        }
        if state.get("slack_notifications").is_none() || !state["slack_notifications"].is_array() {
            state["slack_notifications"] = json!([]);
        }
        if let Some(arr) = state["slack_notifications"].as_array_mut() {
            arr.push(json!({
                "phase": args.phase,
                "phase_name": phase_name,
                "ts": args.ts,
                "thread_ts": args.thread_ts,
                "message_preview": preview,
                "timestamp": timestamp,
            }));
        }
    }) {
        Ok(state) => {
            let count = state["slack_notifications"]
                .as_array()
                .map(|a| a.len())
                .unwrap_or(0);
            json_ok(&[("notification_count", json!(count))]);
        }
        Err(e) => {
            json_error(&format!("Failed to add notification: {}", e), &[]);
            process::exit(1);
        }
    }
}

fn truncate_preview(message: &str) -> String {
    if message.chars().count() > MAX_PREVIEW_LENGTH {
        let truncated: String = message.chars().take(MAX_PREVIEW_LENGTH).collect();
        format!("{}...", truncated)
    } else {
        message.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::fs;
    use std::path::Path;

    fn make_state(branch: &str) -> Value {
        json!({
            "schema_version": 1,
            "branch": branch,
            "current_phase": "flow-code",
            "slack_notifications": []
        })
    }

    fn write_state(dir: &Path, branch: &str, state: &Value) -> std::path::PathBuf {
        let state_dir = dir.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let path = state_dir.join(format!("{}.json", branch));
        fs::write(&path, serde_json::to_string_pretty(state).unwrap()).unwrap();
        path
    }

    #[test]
    fn add_notification_to_empty_array() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("test-feature");
        let path = write_state(dir.path(), "test-feature", &state);

        let result = mutate_state(&path, |s| {
            let names = phase_names();
            let phase = "flow-code";
            let phase_name = names.get(phase).cloned().unwrap_or_default();
            if let Some(arr) = s["slack_notifications"].as_array_mut() {
                arr.push(json!({
                    "phase": phase,
                    "phase_name": phase_name,
                    "ts": "5555555555.555555",
                    "thread_ts": "1111111111.111111",
                    "message_preview": "short msg",
                    "timestamp": now(),
                }));
            }
        })
        .unwrap();

        let notifs = result["slack_notifications"].as_array().unwrap();
        assert_eq!(notifs.len(), 1);
        assert_eq!(notifs[0]["phase"], "flow-code");
        assert_eq!(notifs[0]["phase_name"], "Code");
        assert_eq!(notifs[0]["ts"], "5555555555.555555");
        assert_eq!(notifs[0]["thread_ts"], "1111111111.111111");
        assert_eq!(notifs[0]["message_preview"], "short msg");
        assert!(notifs[0]["timestamp"].as_str().unwrap().contains("T"));
    }

    #[test]
    fn add_notification_preserves_existing() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = make_state("test-feature");
        state["slack_notifications"] = json!([
            {"phase": "flow-start", "message_preview": "existing"}
        ]);
        let path = write_state(dir.path(), "test-feature", &state);

        mutate_state(&path, |s| {
            if let Some(arr) = s["slack_notifications"].as_array_mut() {
                arr.push(json!({"phase": "flow-code", "message_preview": "new"}));
            }
        })
        .unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let on_disk: Value = serde_json::from_str(&content).unwrap();
        let notifs = on_disk["slack_notifications"].as_array().unwrap();
        assert_eq!(notifs.len(), 2);
        assert_eq!(notifs[0]["message_preview"], "existing");
        assert_eq!(notifs[1]["message_preview"], "new");
    }

    #[test]
    fn add_notification_creates_array_if_missing() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let path = state_dir.join("test.json");
        fs::write(&path, r#"{"current_phase": "flow-code"}"#).unwrap();

        mutate_state(&path, |s| {
            if s.get("slack_notifications").is_none() || !s["slack_notifications"].is_array() {
                s["slack_notifications"] = json!([]);
            }
            if let Some(arr) = s["slack_notifications"].as_array_mut() {
                arr.push(json!({"phase": "flow-code", "message_preview": "test"}));
            }
        })
        .unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let on_disk: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(on_disk["slack_notifications"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn truncate_preview_short_message() {
        assert_eq!(truncate_preview("hello"), "hello");
    }

    #[test]
    fn truncate_preview_exactly_100_chars() {
        let msg = "a".repeat(100);
        assert_eq!(truncate_preview(&msg), msg);
    }

    #[test]
    fn truncate_preview_over_100_chars() {
        let msg = "a".repeat(150);
        let result = truncate_preview(&msg);
        assert_eq!(result.len(), 103); // 100 + "..."
        assert!(result.ends_with("..."));
        assert_eq!(&result[..100], &msg[..100]);
    }

    #[test]
    fn truncate_preview_101_chars() {
        let msg = "a".repeat(101);
        let result = truncate_preview(&msg);
        assert_eq!(result.len(), 103);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn persists_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("test-feature");
        let path = write_state(dir.path(), "test-feature", &state);

        mutate_state(&path, |s| {
            if let Some(arr) = s["slack_notifications"].as_array_mut() {
                arr.push(json!({"phase": "flow-code", "message_preview": "persisted"}));
            }
        })
        .unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let on_disk: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(
            on_disk["slack_notifications"][0]["message_preview"],
            "persisted"
        );
    }

    #[test]
    fn add_notification_array_root_state_noop() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let path = state_dir.join("test.json");
        let content = "[1, 2, 3]";
        fs::write(&path, content).unwrap();

        mutate_state(&path, |state| {
            if !(state.is_object() || state.is_null()) {
                return;
            }
            if state.get("slack_notifications").is_none()
                || !state["slack_notifications"].is_array()
            {
                state["slack_notifications"] = json!([]);
            }
            if let Some(arr) = state["slack_notifications"].as_array_mut() {
                arr.push(json!({"phase": "flow-code", "message_preview": "should not appear"}));
            }
        })
        .unwrap();

        let after = fs::read_to_string(&path).unwrap();
        let parsed: Value = serde_json::from_str(&after).unwrap();
        assert!(parsed.is_array(), "Root should still be an array");
        assert_eq!(parsed.as_array().unwrap().len(), 3);
    }

    #[test]
    fn corrupt_state_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let path = state_dir.join("test.json");
        fs::write(&path, "{corrupt").unwrap();

        let result = mutate_state(&path, |_| {});
        assert!(result.is_err());
    }
}
