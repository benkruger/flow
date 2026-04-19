//! Consolidated start-finalize: phase-transition complete + notify-slack +
//! set-timestamp + add-notification in a single command.
//!
//! Returns JSON with formatted_time and continue_action for the skill
//! to use in the COMPLETE banner and transition HARD-GATE.
//!
//! # Dependency-injected core
//!
//! [`run_impl_with_deps`] is the fully-testable core: it accepts the
//! project root as a `&Path` and the Slack notifier as an injectable
//! closure, so inline tests can drive every branch against a `TempDir`
//! fixture without touching host state or spawning `curl`. Production
//! [`run_impl`] is a one-line binder that passes the real
//! [`git::project_root`] and [`notify_slack::notify`]. [`run`] adapts
//! the result into a printed JSON line and process exit code.

use std::path::Path;

use clap::Parser;
use serde_json::{json, Value};

use crate::commands::log::append_log;
use crate::commands::start_step::update_step;
use crate::flow_paths::FlowPaths;
use crate::git::project_root;
use crate::lock::mutate_state;
use crate::notify_slack;
use crate::phase_config;
use crate::phase_transition::phase_complete;

#[derive(Parser, Debug)]
#[command(
    name = "start-finalize",
    about = "Complete Start phase and send notifications"
)]
pub struct Args {
    /// Branch name for state file lookup
    #[arg(long)]
    pub branch: String,

    /// PR URL for Slack notification
    #[arg(long = "pr-url")]
    pub pr_url: Option<String>,

    /// Override all skills to fully autonomous preset
    #[arg(long)]
    pub auto: bool,
}

/// Testable core with injected project root and Slack notifier.
///
/// Production `run_impl` binds `root` to [`project_root`] and `notifier`
/// to [`notify_slack::notify`]. Tests supply a `TempDir` path and a
/// stub closure returning canned `Value` responses.
pub fn run_impl_with_deps(
    args: &Args,
    root: &Path,
    notifier: &dyn Fn(&notify_slack::Args) -> Value,
) -> Value {
    let branch = &args.branch;
    let paths = FlowPaths::new(root, branch);
    let state_path = paths.state_file();

    if !state_path.exists() {
        return json!({
            "status": "error",
            "message": format!("No state file found: {}", state_path.display()),
        });
    }

    // Update TUI step counter
    update_step(&state_path, 5);

    // Load frozen phase config if available
    let frozen_path = paths.frozen_phases();
    let frozen_config = if frozen_path.exists() {
        phase_config::load_phase_config(&frozen_path).ok()
    } else {
        None
    };

    let frozen_order: Option<Vec<String>> = frozen_config.as_ref().map(|c| c.order.clone());
    let frozen_commands = frozen_config.as_ref().map(|c| c.commands.clone());

    // Step 1: Phase transition complete
    let result_holder = std::cell::RefCell::new(Value::Null);

    let mutate_result = mutate_state(&state_path, |state| {
        let result = phase_complete(
            state,
            "flow-start",
            None,
            frozen_order.as_deref(),
            frozen_commands.as_ref(),
        );
        *result_holder.borrow_mut() = result;
    });

    match mutate_result {
        Ok(_) => {}
        Err(e) => {
            return json!({
                "status": "error",
                "message": format!("State mutation failed: {}", e),
            });
        }
    }

    let phase_result = result_holder.into_inner();
    let _ = append_log(
        root,
        branch,
        &format!(
            "[Phase 1] start-finalize — phase-transition complete ({})",
            phase_result["status"]
        ),
    );

    let formatted_time = phase_result["formatted_time"]
        .as_str()
        .unwrap_or("<1m")
        .to_string();
    let continue_action = phase_result["continue_action"]
        .as_str()
        .unwrap_or("ask")
        .to_string();

    // Step 2: Slack notification (best-effort)
    let mut slack_result = json!({"status": "skipped"});
    if let Some(ref pr_url) = args.pr_url {
        let message = format!("Phase 1: Start complete — PR created for {}", branch);
        let slack_args = notify_slack::Args {
            phase: "flow-start".to_string(),
            message: message.clone(),
            pr_url: Some(pr_url.clone()),
            thread_ts: None,
            feature: None,
        };
        slack_result = notifier(&slack_args);

        if slack_result["status"] == "ok" {
            // Store thread_ts and notification in state
            let ts = slack_result["ts"].as_str().unwrap_or("").to_string();
            let msg_clone = message.clone();
            let ts_clone = ts.clone();

            let _ = mutate_state(&state_path, move |state| {
                if !(state.is_object() || state.is_null()) {
                    return;
                }
                state["slack_thread_ts"] = json!(ts_clone);

                // Append to notifications array
                if !state
                    .get("notifications")
                    .map(|v| v.is_array())
                    .unwrap_or(false)
                {
                    state["notifications"] = json!([]);
                }
                if let Some(arr) = state["notifications"].as_array_mut() {
                    arr.push(json!({
                        "phase": "flow-start",
                        "ts": ts_clone,
                        "thread_ts": ts_clone,
                        "message": msg_clone,
                    }));
                }
            });
        }

        let _ = append_log(
            root,
            branch,
            &format!(
                "[Phase 1] start-finalize — notify-slack ({})",
                slack_result["status"]
            ),
        );
    }

    // Build response
    let mut response = json!({
        "status": "ok",
        "formatted_time": formatted_time,
        "continue_action": continue_action,
    });

    if slack_result["status"] != "skipped" {
        response["slack"] = slack_result;
    }

    response
}

/// Production entry point: binds [`run_impl_with_deps`] to the real
/// [`project_root`] and [`notify_slack::notify`].
pub fn run_impl(args: &Args) -> Value {
    run_impl_with_deps(args, &project_root(), &notify_slack::notify)
}

/// Main-arm entry point: returns the `(Value, i32)` contract that
/// `dispatch::dispatch_json` consumes. Takes `root: &Path` per
/// `.claude/rules/rust-patterns.md` "Main-arm dispatch" so inline
/// tests can pass a `TempDir` fixture instead of the host
/// `project_root()`. `start_finalize::run_impl_with_deps` always
/// returns `Value` — business errors appear in the `status: "error"`
/// payload with exit code `0`.
pub fn run_impl_main(args: &Args, root: &Path) -> (Value, i32) {
    (run_impl_with_deps(args, root, &notify_slack::notify), 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::fs;
    use std::path::PathBuf;

    // --- run_impl_with_deps ---

    /// Seed a minimal state file with `flow-start` in_progress so
    /// `phase_complete` has legal input. Returns the project root.
    fn seed_state(branch: &str, skills_continue: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let state = json!({
            "schema_version": 1,
            "branch": branch,
            "current_phase": "flow-start",
            "phases": {
                "flow-start": {
                    "name": "Start",
                    "status": "in_progress",
                    "session_started_at": "2026-01-01T00:00:00-08:00",
                    "cumulative_seconds": 0,
                    "visit_count": 1,
                },
                "flow-plan": {"name": "Plan", "status": "pending", "cumulative_seconds": 0, "visit_count": 0},
                "flow-code": {"name": "Code", "status": "pending", "cumulative_seconds": 0, "visit_count": 0},
                "flow-code-review": {"name": "Code Review", "status": "pending", "cumulative_seconds": 0, "visit_count": 0},
                "flow-learn": {"name": "Learn", "status": "pending", "cumulative_seconds": 0, "visit_count": 0},
                "flow-complete": {"name": "Complete", "status": "pending", "cumulative_seconds": 0, "visit_count": 0},
            },
            "skills": {
                "flow-start": {"continue": skills_continue},
                "flow-plan": {"continue": skills_continue, "dag": "auto"},
            },
            "phase_transitions": [],
            "notifications": [],
        });
        fs::write(
            state_dir.join(format!("{}.json", branch)),
            serde_json::to_string_pretty(&state).unwrap(),
        )
        .unwrap();
        (dir, root)
    }

    fn panicking_notifier(_args: &notify_slack::Args) -> Value {
        panic!("notifier must not be called when pr_url is None");
    }

    #[test]
    fn finalize_no_pr_url_skips_slack() {
        let (_dir, root) = seed_state("no-url-branch", "auto");
        let args = Args {
            branch: "no-url-branch".to_string(),
            pr_url: None,
            auto: false,
        };

        let result = run_impl_with_deps(&args, &root, &panicking_notifier);
        assert_eq!(result["status"], "ok");
        assert!(
            result.get("slack").is_none(),
            "response must not include slack field when pr_url is None"
        );

        let state_path = root.join(".flow-states/no-url-branch.json");
        let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
        assert!(
            state.get("slack_thread_ts").is_none(),
            "state must not record slack_thread_ts without pr_url"
        );
    }

    #[test]
    fn finalize_notifier_skipped_leaves_state_untouched() {
        let (_dir, root) = seed_state("skipped-branch", "auto");
        let args = Args {
            branch: "skipped-branch".to_string(),
            pr_url: Some("https://github.com/test/repo/pull/42".to_string()),
            auto: false,
        };
        let notifier = |_: &notify_slack::Args| -> Value { json!({"status": "skipped"}) };

        let result = run_impl_with_deps(&args, &root, &notifier);
        assert_eq!(result["status"], "ok");
        // Response omits slack when the notifier returned "skipped" —
        // the response-building check is `!= "skipped"`.
        assert!(
            result.get("slack").is_none(),
            "response must not include slack field when notifier returns skipped"
        );

        let state_path = root.join(".flow-states/skipped-branch.json");
        let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
        assert!(
            state.get("slack_thread_ts").is_none(),
            "skipped notifier must not write slack_thread_ts"
        );
        assert!(
            state["notifications"].as_array().unwrap().is_empty(),
            "skipped notifier must not append to notifications"
        );
    }

    #[test]
    fn finalize_notifier_ok_writes_thread_ts_and_notification() {
        let (_dir, root) = seed_state("ok-branch", "auto");
        let args = Args {
            branch: "ok-branch".to_string(),
            pr_url: Some("https://github.com/test/repo/pull/42".to_string()),
            auto: false,
        };
        let notifier =
            |_: &notify_slack::Args| -> Value { json!({"status": "ok", "ts": "1234.5678"}) };

        let result = run_impl_with_deps(&args, &root, &notifier);
        assert_eq!(result["status"], "ok");
        assert_eq!(result["slack"]["status"], "ok");
        assert_eq!(result["slack"]["ts"], "1234.5678");

        let state_path = root.join(".flow-states/ok-branch.json");
        let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
        assert_eq!(state["slack_thread_ts"], "1234.5678");
        let notifications = state["notifications"].as_array().unwrap();
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0]["phase"], "flow-start");
        assert_eq!(notifications[0]["ts"], "1234.5678");
        assert_eq!(notifications[0]["thread_ts"], "1234.5678");
    }

    #[test]
    fn finalize_notifier_error_continues_best_effort() {
        let (_dir, root) = seed_state("err-branch", "auto");
        let args = Args {
            branch: "err-branch".to_string(),
            pr_url: Some("https://github.com/test/repo/pull/42".to_string()),
            auto: false,
        };
        let notifier = |_: &notify_slack::Args| -> Value {
            json!({"status": "error", "message": "curl failed"})
        };

        let result = run_impl_with_deps(&args, &root, &notifier);
        // Top-level status stays "ok" — Slack is best-effort.
        assert_eq!(result["status"], "ok");
        assert_eq!(result["slack"]["status"], "error");

        let state_path = root.join(".flow-states/err-branch.json");
        let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
        assert!(
            state.get("slack_thread_ts").is_none(),
            "error response must not write slack_thread_ts"
        );
    }

    #[test]
    fn finalize_notifier_ok_with_wrong_notifications_type_heals() {
        let (_dir, root) = seed_state("heal-branch", "auto");
        // Corrupt notifications to a string so the auto-heal path fires.
        let state_path = root.join(".flow-states/heal-branch.json");
        let mut state: Value =
            serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
        state["notifications"] = json!("not-an-array");
        fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();

        let args = Args {
            branch: "heal-branch".to_string(),
            pr_url: Some("https://github.com/test/repo/pull/42".to_string()),
            auto: false,
        };
        let notifier = |_: &notify_slack::Args| -> Value { json!({"status": "ok", "ts": "9.9"}) };

        let result = run_impl_with_deps(&args, &root, &notifier);
        assert_eq!(result["status"], "ok");

        let healed: Value =
            serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
        let arr = healed["notifications"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["ts"], "9.9");
    }

    #[test]
    fn finalize_missing_state_returns_error_with_deps() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // No state file seeded.
        let args = Args {
            branch: "nope-branch".to_string(),
            pr_url: None,
            auto: false,
        };
        let result = run_impl_with_deps(&args, &root, &panicking_notifier);
        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("No state file"));
    }

    #[test]
    fn finalize_corrupt_state_returns_error_with_deps() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(state_dir.join("corrupt-branch.json"), "not json{{{").unwrap();

        let args = Args {
            branch: "corrupt-branch".to_string(),
            pr_url: None,
            auto: false,
        };
        let result = run_impl_with_deps(&args, &root, &panicking_notifier);
        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("State mutation failed"));
    }

    #[test]
    fn finalize_with_deps_notifier_called_once() {
        // Regression: ensure the notifier is invoked exactly once per
        // pr_url-set call, not zero (missed branch) or twice (accidental
        // retry).
        let (_dir, root) = seed_state("call-count-branch", "auto");
        let args = Args {
            branch: "call-count-branch".to_string(),
            pr_url: Some("https://github.com/test/repo/pull/42".to_string()),
            auto: false,
        };
        let calls: RefCell<usize> = RefCell::new(0);
        let notifier = |_: &notify_slack::Args| -> Value {
            *calls.borrow_mut() += 1;
            json!({"status": "ok", "ts": "42.0"})
        };

        let _ = run_impl_with_deps(&args, &root, &notifier);
        assert_eq!(*calls.borrow(), 1);
    }

    // --- run_impl_main ---

    #[test]
    fn finalize_run_impl_main_err_path() {
        // Drive the missing-state-file scenario through run_impl_main
        // against a TempDir so the injected root scopes the FlowPaths
        // resolution to the fixture. Asserts the `(Value, 0)`
        // contract: business errors appear as `status:"error"` in the
        // Value with exit code 0.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let args = Args {
            branch: "main-err-branch".to_string(),
            pr_url: None,
            auto: false,
        };
        let (v, code) = run_impl_main(&args, &root);
        assert_eq!(code, 0, "exit code is 0 for business errors");
        assert_eq!(v["status"], "error");
        assert!(v["message"]
            .as_str()
            .unwrap_or("")
            .contains("No state file found"));
    }

    #[test]
    fn finalize_run_impl_main_happy_wraps_with_exit_zero() {
        // Happy path via run_impl_main directly. pr_url=None so the
        // production notify_slack binder is never invoked.
        let (_dir, root) = seed_state("happy-main-branch", "auto");
        let args = Args {
            branch: "happy-main-branch".to_string(),
            pr_url: None,
            auto: false,
        };
        let (v, code) = run_impl_main(&args, &root);
        assert_eq!(code, 0);
        assert_eq!(v["status"], "ok");
    }
}
