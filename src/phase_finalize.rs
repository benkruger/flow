//! Generic phase-finalize: phase_complete() + Slack notification + notification state record.
//!
//! A single command parameterized by `--phase` replaces the per-skill exit ceremony.
//! Handles both thread creation (Start phase, no --thread-ts) and thread replies
//! (all other phases, --thread-ts provided).
//!
//! Public entry points: `run()` is the CLI dispatch, `run_impl(args)` is the
//! testable surface for infrastructure failures. `run_impl_with_deps(root, cwd,
//! args, notifier)` accepts injected `root`/`cwd` paths plus a
//! `&dyn Fn(&notify_slack::Args) -> Value` notifier so inline tests can drive
//! the Slack-success, Slack-error, and state-error branches against a tempdir
//! without touching the real worktree or spawning curl.

use std::process;

use clap::Parser;
use serde_json::{json, Value};

use crate::commands::log::append_log;
use crate::flow_paths::FlowPaths;
use crate::git::project_root;
use crate::lock::mutate_state;
use crate::notify_slack;
use crate::output::json_error;
use crate::phase_config;
use crate::phase_transition::phase_complete;

#[derive(Parser, Debug)]
#[command(
    name = "phase-finalize",
    about = "Generic phase exit: complete + Slack + notification"
)]
pub struct Args {
    /// Phase name (e.g. flow-start, flow-code, flow-code-review, flow-learn)
    #[arg(long)]
    pub phase: String,

    /// Branch name for state file lookup
    #[arg(long)]
    pub branch: String,

    /// Slack thread timestamp (if provided, replies to thread; if absent, creates new thread)
    #[arg(long = "thread-ts")]
    pub thread_ts: Option<String>,

    /// PR URL for Slack notification (used when creating a new thread, i.e. Start phase)
    #[arg(long = "pr-url")]
    pub pr_url: Option<String>,
}

/// Notifier closure shape: Slack notify function — takes notify_slack::Args
/// and returns the JSON Value result.
pub type NotifierFn = dyn Fn(&notify_slack::Args) -> Value;

/// Testable entry point.
///
/// Returns Ok(json) for both success and application-level errors (status: error).
/// Returns Err(string) only for infrastructure failures. Resolves the real
/// project root, current directory, and production notifier, then delegates to
/// `run_impl_with_deps`.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let root = project_root();
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    run_impl_with_deps(&root, &cwd, args, &notify_slack::notify)
}

/// Dependency-injected core. Tests pass a tempdir for `root`/`cwd` and a
/// closure notifier; production `run_impl` wires real paths and
/// `notify_slack::notify`.
pub fn run_impl_with_deps(
    root: &std::path::Path,
    cwd: &std::path::Path,
    args: &Args,
    notifier: &NotifierFn,
) -> Result<Value, String> {
    let branch = &args.branch;
    let phase_num = phase_config::phase_number(&args.phase);
    // `args.branch` is a raw clap `--branch` CLI arg — accepts any string
    // the shell passes, including slashes (`feature/foo`) and empty values.
    // `.claude/rules/external-input-validation.md` requires `try_new` on the
    // CLI-override path so the caller sees a structured error rather than a
    // Rust panic (issue #1137 reference incident).
    let paths = match FlowPaths::try_new(root, branch) {
        Some(p) => p,
        None => {
            return Ok(json!({
                "status": "error",
                "message": format!(
                    "Invalid branch name: '{}' (must be non-empty and contain no '/')",
                    branch
                ),
            }));
        }
    };
    let state_path = paths.state_file();

    // Drift guard: phase transitions must happen from inside the
    // subdirectory the flow was started in. Running phase-finalize
    // from the wrong subdirectory of a mono-repo would mark the phase
    // complete against the wrong assumed scope. See
    // [`crate::cwd_scope::enforce`].
    if let Err(msg) = crate::cwd_scope::enforce(cwd, root) {
        return Ok(json!({"status": "error", "message": msg}));
    }

    if !state_path.exists() {
        return Ok(json!({
            "status": "error",
            "message": format!("No state file found: {}", state_path.display()),
        }));
    }

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
    let phase_name = args.phase.clone();

    let mutate_result = mutate_state(&state_path, |state| {
        if !(state.is_object() || state.is_null()) {
            return;
        }
        let result = phase_complete(
            state,
            &phase_name,
            None,
            frozen_order.as_deref(),
            frozen_commands.as_ref(),
        );
        *result_holder.borrow_mut() = result;
    });

    match mutate_result {
        Ok(_) => {}
        Err(e) => {
            return Ok(json!({
                "status": "error",
                "message": format!("State mutation failed: {}", e),
            }));
        }
    }

    let phase_result = result_holder.into_inner();
    let _ = append_log(
        root,
        branch,
        &format!(
            "[Phase {}] phase-finalize --phase {} ({})",
            phase_num, args.phase, phase_result["status"]
        ),
    );

    if phase_result["status"] == "error" {
        return Ok(json!({
            "status": "error",
            "message": phase_result["message"],
        }));
    }

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

    // Determine if we should send a Slack notification
    let should_notify = args.thread_ts.is_some() || args.pr_url.is_some();

    if should_notify {
        let message = format!(
            "Phase {}: {} complete",
            phase_config::phase_numbers()
                .get(&args.phase)
                .copied()
                .unwrap_or(0),
            phase_config::phase_names()
                .get(&args.phase)
                .cloned()
                .unwrap_or_else(|| args.phase.clone()),
        );

        let slack_args = notify_slack::Args {
            phase: args.phase.clone(),
            message: message.clone(),
            pr_url: args.pr_url.clone(),
            thread_ts: args.thread_ts.clone(),
            feature: None,
        };
        slack_result = notifier(&slack_args);

        if slack_result["status"] == "ok" {
            let ts = slack_result["ts"].as_str().unwrap_or("").to_string();
            let msg_clone = message.clone();
            let ts_clone = ts.clone();
            let thread_ts_for_state = if args.thread_ts.is_some() {
                // Reply mode: thread_ts is the existing thread
                args.thread_ts.clone().unwrap_or_default()
            } else {
                // Create mode: the new message's ts IS the thread_ts
                ts.clone()
            };

            let _ = mutate_state(&state_path, move |state| {
                if !(state.is_object() || state.is_null()) {
                    return;
                }

                // If creating a new thread (no --thread-ts), store thread_ts
                if thread_ts_for_state == ts_clone {
                    state["slack_thread_ts"] = json!(ts_clone);
                }

                // Append to slack_notifications array
                if !state
                    .get("slack_notifications")
                    .map(|v| v.is_array())
                    .unwrap_or(false)
                {
                    state["slack_notifications"] = json!([]);
                }
                if let Some(arr) = state["slack_notifications"].as_array_mut() {
                    arr.push(json!({
                        "phase": phase_name,
                        "ts": ts_clone,
                        "thread_ts": thread_ts_for_state,
                        "message": msg_clone,
                    }));
                }
            });
        }

        let _ = append_log(
            root,
            branch,
            &format!(
                "[Phase {}] phase-finalize --phase {} — notify-slack ({})",
                phase_num, args.phase, slack_result["status"]
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

    Ok(response)
}

/// CLI entry point. Thin dispatcher over [`run_impl`]: prints the
/// success JSON on `Ok`, emits a `json_error` and calls
/// `process::exit(1)` on infrastructure failure. All Slack-path and
/// state-file logic lives in [`run_impl_with_deps`].
pub fn run(args: Args) {
    match run_impl(&args) {
        Ok(result) => {
            println!("{}", serde_json::to_string(&result).unwrap());
        }
        Err(e) => {
            json_error(&e, &[]);
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    // --- run_impl_with_deps ---

    fn test_args(phase: &str, branch: &str, thread_ts: Option<&str>, pr_url: Option<&str>) -> Args {
        Args {
            phase: phase.to_string(),
            branch: branch.to_string(),
            thread_ts: thread_ts.map(|s| s.to_string()),
            pr_url: pr_url.map(|s| s.to_string()),
        }
    }

    /// Write a minimal state file with the named phase `in_progress` and all
    /// prior phases marked `complete` so `phase_complete` can advance.
    fn write_state(root: &std::path::Path, branch: &str, current_phase: &str) {
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();

        // Build each phase status based on its position relative to current_phase.
        // Order mirrors phase_config::PHASE_ORDER.
        let phase_order = [
            "flow-start",
            "flow-plan",
            "flow-code",
            "flow-code-review",
            "flow-learn",
            "flow-complete",
        ];
        let cur_idx = phase_order
            .iter()
            .position(|p| *p == current_phase)
            .expect("current_phase must be a known phase");

        let mut phases = serde_json::Map::new();
        for (idx, p) in phase_order.iter().enumerate() {
            let status = match idx.cmp(&cur_idx) {
                std::cmp::Ordering::Less => "complete",
                std::cmp::Ordering::Equal => "in_progress",
                std::cmp::Ordering::Greater => "pending",
            };
            phases.insert(
                p.to_string(),
                json!({
                    "name": p,
                    "status": status,
                    "started_at": if status != "pending" { Some("2026-01-01T00:00:00-08:00") } else { None },
                    "completed_at": if status == "complete" { Some("2026-01-01T00:01:00-08:00") } else { None },
                    "session_started_at": if status == "in_progress" { Some("2026-01-01T00:00:00-08:00") } else { None },
                    "cumulative_seconds": if status == "complete" { 60 } else { 0 },
                    "visit_count": if status == "pending" { 0 } else { 1 }
                }),
            );
        }

        let state = json!({
            "schema_version": 1,
            "branch": branch,
            "current_phase": current_phase,
            "started_at": "2026-01-01T00:00:00-08:00",
            "phases": Value::Object(phases),
            "phase_transitions": [],
            "prompt": "test feature",
            "notes": [],
        });

        fs::write(
            state_dir.join(format!("{}.json", branch)),
            serde_json::to_string_pretty(&state).unwrap(),
        )
        .unwrap();
    }

    fn read_state(root: &std::path::Path, branch: &str) -> Value {
        let path = root.join(".flow-states").join(format!("{}.json", branch));
        let content = fs::read_to_string(&path).unwrap();
        serde_json::from_str(&content).unwrap()
    }

    #[test]
    fn finalize_with_notifier_slack_thread_reply_success() {
        // thread_ts provided + notifier returns ok:
        // - slack_notifications appended with the input thread_ts
        // - slack_thread_ts NOT set (reply, not create)
        let dir = tempdir().unwrap();
        let root = dir.path();
        write_state(root, "branch-a", "flow-code");

        let notifier =
            |_: &notify_slack::Args| -> Value { json!({"status": "ok", "ts": "5555.6666"}) };
        let args = test_args("flow-code", "branch-a", Some("1111.2222"), None);

        let result = run_impl_with_deps(root, root, &args, &notifier).unwrap();
        assert_eq!(result["status"], "ok");
        assert_eq!(result["slack"]["status"], "ok");

        let state = read_state(root, "branch-a");
        assert!(
            state.get("slack_thread_ts").is_none() || state["slack_thread_ts"].is_null(),
            "reply branch must not set slack_thread_ts: got {}",
            state["slack_thread_ts"]
        );
        let notifs = state["slack_notifications"].as_array().unwrap();
        assert_eq!(notifs.len(), 1);
        assert_eq!(notifs[0]["ts"], "5555.6666");
        assert_eq!(notifs[0]["thread_ts"], "1111.2222");
        assert_eq!(notifs[0]["phase"], "flow-code");
    }

    #[test]
    fn finalize_with_notifier_slack_thread_create_success() {
        // No thread_ts, pr_url present, notifier returns ok:
        // - slack_thread_ts SET to returned ts
        // - slack_notifications[0].thread_ts equals the new ts
        let dir = tempdir().unwrap();
        let root = dir.path();
        write_state(root, "branch-b", "flow-start");

        let notifier =
            |_: &notify_slack::Args| -> Value { json!({"status": "ok", "ts": "7777.8888"}) };
        let args = test_args(
            "flow-start",
            "branch-b",
            None,
            Some("https://github.com/org/repo/pull/42"),
        );

        let result = run_impl_with_deps(root, root, &args, &notifier).unwrap();
        assert_eq!(result["status"], "ok");

        let state = read_state(root, "branch-b");
        assert_eq!(
            state["slack_thread_ts"], "7777.8888",
            "create branch must set slack_thread_ts to returned ts"
        );
        let notifs = state["slack_notifications"].as_array().unwrap();
        assert_eq!(notifs.len(), 1);
        assert_eq!(notifs[0]["thread_ts"], "7777.8888");
    }

    #[test]
    fn finalize_with_notifier_slack_error_skips_state_record() {
        // Notifier returns error:
        // - slack_notifications NOT appended
        // - response includes "slack" key with the error
        let dir = tempdir().unwrap();
        let root = dir.path();
        write_state(root, "branch-c", "flow-code");

        let notifier =
            |_: &notify_slack::Args| -> Value { json!({"status": "error", "message": "boom"}) };
        let args = test_args("flow-code", "branch-c", Some("1111.2222"), None);

        let result = run_impl_with_deps(root, root, &args, &notifier).unwrap();
        assert_eq!(result["status"], "ok");
        assert_eq!(result["slack"]["status"], "error");

        let state = read_state(root, "branch-c");
        // slack_notifications either absent or empty — never appended on error
        let notifs_empty = state
            .get("slack_notifications")
            .map(|v| v.as_array().map(|a| a.is_empty()).unwrap_or(true))
            .unwrap_or(true);
        assert!(
            notifs_empty,
            "slack_notifications must not be populated on notifier error"
        );
    }

    #[test]
    fn finalize_with_notifier_slash_branch_returns_structured_error_no_panic() {
        // `--branch feature/foo` from the CLI must not panic at
        // FlowPaths::new. The `try_new` guard returns a structured error
        // JSON so the caller sees an "Invalid branch name" message instead
        // of a Rust panic (issue #1137 reference pattern).
        let dir = tempdir().unwrap();
        let root = dir.path();

        let notifier = |_: &notify_slack::Args| -> Value { json!({"status": "skipped"}) };
        let args = test_args("flow-code", "feature/foo", None, None);

        let result = run_impl_with_deps(root, root, &args, &notifier).unwrap();
        assert_eq!(result["status"], "error");
        assert!(
            result["message"]
                .as_str()
                .unwrap()
                .contains("Invalid branch name"),
            "slash-containing branch must return Invalid branch name error: got {}",
            result["message"]
        );
    }

    #[test]
    fn finalize_with_notifier_empty_branch_returns_structured_error_no_panic() {
        // `--branch ""` (empty string) from the CLI must not panic at
        // FlowPaths::new. Same try_new guard covers the empty case.
        let dir = tempdir().unwrap();
        let root = dir.path();

        let notifier = |_: &notify_slack::Args| -> Value { json!({"status": "skipped"}) };
        let args = test_args("flow-code", "", None, None);

        let result = run_impl_with_deps(root, root, &args, &notifier).unwrap();
        assert_eq!(result["status"], "error");
        assert!(
            result["message"]
                .as_str()
                .unwrap()
                .contains("Invalid branch name"),
            "empty branch must return Invalid branch name error: got {}",
            result["message"]
        );
    }

    #[test]
    fn finalize_with_notifier_state_file_missing() {
        // No state file at expected path — returns the "No state file found" error.
        let dir = tempdir().unwrap();
        let root = dir.path();
        // Intentionally do NOT call write_state.

        let notifier = |_: &notify_slack::Args| -> Value { json!({"status": "skipped"}) };
        let args = test_args("flow-code", "branch-missing", None, None);

        let result = run_impl_with_deps(root, root, &args, &notifier).unwrap();
        assert_eq!(result["status"], "error");
        assert!(
            result["message"]
                .as_str()
                .unwrap()
                .contains("No state file found"),
            "message must name the missing state file: got {}",
            result["message"]
        );
    }

    #[test]
    fn finalize_with_notifier_no_slack_args_response_omits_slack_key() {
        // Neither thread_ts nor pr_url — should_notify=false, slack_result
        // stays "skipped", response omits the "slack" key. We route a
        // panicking notifier through the function to prove the
        // should_notify=false branch short-circuits before calling it.
        let dir = tempdir().unwrap();
        let root = dir.path();
        write_state(root, "branch-d", "flow-code");

        let notifier = |_: &notify_slack::Args| -> Value {
            panic!("notifier must not be called when neither thread_ts nor pr_url is set");
        };
        let args = test_args("flow-code", "branch-d", None, None);

        let result = run_impl_with_deps(root, root, &args, &notifier).unwrap();
        assert_eq!(result["status"], "ok");
        assert!(
            result.get("slack").is_none(),
            "skipped slack results must be omitted from response: {}",
            result
        );

        // Slack state fields must not be written when notifier short-circuits.
        let state = read_state(root, "branch-d");
        assert!(state.get("slack_thread_ts").is_none() || state["slack_thread_ts"].is_null());
        let notifs_empty = state
            .get("slack_notifications")
            .map(|v| v.as_array().map(|a| a.is_empty()).unwrap_or(true))
            .unwrap_or(true);
        assert!(notifs_empty);
    }
}
