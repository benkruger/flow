use std::path::Path;

use clap::Parser;
use serde_json::{json, Value};

use crate::flow_paths::FlowPaths;
use crate::git::{project_root, resolve_branch};
use crate::lock::mutate_state;
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

/// Main-arm dispatcher with injected root. Returns `(value, exit_code)`:
/// `(ok+notification_count, 0)` on success, `(no_state, 0)` when the
/// state file is missing, `(error+message, 1)` on resolve-branch failure
/// or mutate_state failure.
pub fn run_impl_main(args: Args, root: &Path) -> (Value, i32) {
    let branch = match resolve_branch(args.branch.as_deref(), root) {
        Some(b) => b,
        None => {
            return (
                json!({"status": "error", "message": "Could not determine current branch"}),
                1,
            );
        }
    };
    // Branch reaches us either from `current_branch()` (raw git output)
    // or from `--branch` CLI override (raw user input). Both are
    // external inputs per `.claude/rules/external-input-validation.md`,
    // so use the fallible constructor to reject slash-containing or
    // empty branches as a structured error rather than a panic.
    let state_path = match FlowPaths::try_new(root, &branch) {
        Some(p) => p.state_file(),
        None => {
            return (
                json!({"status": "error", "message": format!("Invalid branch '{}'", branch)}),
                1,
            );
        }
    };

    if !state_path.exists() {
        return (json!({"status": "no_state"}), 0);
    }

    let preview = truncate_preview(&args.message);
    let names = phase_names();
    let phase_name = match names.get(&args.phase) {
        Some(n) => n.clone(),
        None => args.phase.clone(),
    };
    let timestamp = now();

    match mutate_state(&state_path, |state| {
        // Corruption resilience: skip mutation when state root is wrong
        // type (e.g. array from interrupted write) to prevent IndexMut
        // panics. See .claude/rules/rust-patterns.md "State Mutation
        // Object Guards".
        if !(state.is_object() || state.is_null()) {
            return;
        }
        if state.get("slack_notifications").is_none() || !state["slack_notifications"].is_array() {
            state["slack_notifications"] = json!([]);
        }
        // The block above guarantees state["slack_notifications"] is an
        // array, so as_array_mut returns Some unconditionally.
        let arr = state["slack_notifications"]
            .as_array_mut()
            .expect("slack_notifications is always an array here");
        arr.push(json!({
            "phase": args.phase,
            "phase_name": phase_name,
            "ts": args.ts,
            "thread_ts": args.thread_ts,
            "message_preview": preview,
            "timestamp": timestamp,
        }));
    }) {
        Ok(state) => {
            let count = match state["slack_notifications"].as_array() {
                Some(a) => a.len(),
                None => 0,
            };
            (json!({"status": "ok", "notification_count": count}), 0)
        }
        Err(e) => (
            json!({"status": "error", "message": format!("Failed to add notification: {}", e)}),
            1,
        ),
    }
}

pub fn run(args: Args) -> ! {
    let root = project_root();
    let (value, code) = run_impl_main(args, &root);
    crate::dispatch::dispatch_json(value, code)
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

    /// Verify that an array-root state file triggers the object guard's
    /// early return, leaving the file unchanged and preventing an
    /// IndexMut panic on non-object root types.
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

    // --- run_impl_main ---

    fn make_args(branch: Option<&str>) -> Args {
        Args {
            phase: "flow-code".to_string(),
            ts: "5555555555.555555".to_string(),
            thread_ts: "1111111111.111111".to_string(),
            message: "test message".to_string(),
            branch: branch.map(|s| s.to_string()),
        }
    }

    #[test]
    fn add_notification_run_impl_main_no_state_returns_no_state_tuple() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let args = make_args(Some("missing-branch"));
        let (value, code) = run_impl_main(args, &root);
        assert_eq!(value["status"], "no_state");
        assert_eq!(code, 0);
    }

    #[test]
    fn add_notification_run_impl_main_success_returns_count_tuple() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(
            state_dir.join("present-branch.json"),
            r#"{"current_phase":"flow-code","slack_notifications":[]}"#,
        )
        .unwrap();
        let args = make_args(Some("present-branch"));
        let (value, code) = run_impl_main(args, &root);
        assert_eq!(code, 0);
        assert_eq!(value["status"], "ok");
        assert_eq!(value["notification_count"], 1);
    }

    #[test]
    fn add_notification_run_impl_main_mutate_state_failure_returns_error_tuple() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(state_dir.join("present-branch.json"), "{not json").unwrap();
        let args = make_args(Some("present-branch"));
        let (value, code) = run_impl_main(args, &root);
        assert_eq!(value["status"], "error");
        assert_eq!(code, 1);
        assert!(value["message"]
            .as_str()
            .unwrap()
            .contains("Failed to add notification"));
    }

    #[test]
    fn add_notification_run_impl_main_array_root_returns_ok_zero_count() {
        // State root is an array — closure guard fires early return,
        // leaving slack_notifications as Value::Null. as_array() None
        // branch returns count 0.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(state_dir.join("array-root.json"), "[1, 2, 3]").unwrap();
        let args = make_args(Some("array-root"));
        let (value, code) = run_impl_main(args, &root);
        assert_eq!(value["status"], "ok");
        assert_eq!(value["notification_count"], 0);
        assert_eq!(code, 0);
    }

    #[test]
    fn add_notification_run_impl_main_unknown_phase_falls_back_to_phase_string() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(
            state_dir.join("unknown-phase.json"),
            r#"{"current_phase":"flow-code","slack_notifications":[]}"#,
        )
        .unwrap();
        let mut args = make_args(Some("unknown-phase"));
        args.phase = "custom-unknown-phase".to_string();
        let (value, code) = run_impl_main(args, &root);
        assert_eq!(value["status"], "ok");
        assert_eq!(code, 0);
        let on_disk: Value = serde_json::from_str(
            &fs::read_to_string(state_dir.join("unknown-phase.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            on_disk["slack_notifications"][0]["phase_name"],
            "custom-unknown-phase"
        );
    }

    #[test]
    fn add_notification_run_impl_main_findings_wrong_type_resets_to_array() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(
            state_dir.join("wrong-type.json"),
            r#"{"current_phase":"flow-code","slack_notifications":"not-an-array"}"#,
        )
        .unwrap();
        let args = make_args(Some("wrong-type"));
        let (value, code) = run_impl_main(args, &root);
        assert_eq!(value["status"], "ok");
        assert_eq!(value["notification_count"], 1);
        assert_eq!(code, 0);
    }

    #[test]
    fn add_notification_run_impl_main_slash_branch_returns_structured_error_no_panic() {
        // Regression: --branch feature/foo previously panicked via
        // FlowPaths::new. Per .claude/rules/external-input-validation.md
        // CLI subcommand entry callsite discipline, --branch is external
        // input and must use FlowPaths::try_new with a structured error
        // return.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let args = make_args(Some("feature/foo"));
        let (value, code) = run_impl_main(args, &root);
        assert_eq!(code, 1);
        assert_eq!(value["status"], "error");
        assert!(value["message"]
            .as_str()
            .unwrap()
            .contains("Invalid branch 'feature/foo'"));
    }
}
