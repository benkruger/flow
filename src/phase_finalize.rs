//! Generic phase-finalize: phase_complete() + Slack notification + notification state record.
//!
//! Replaces the per-skill exit ceremony with a single command parameterized by `--phase`.
//! Handles both thread creation (Start phase, no --thread-ts) and thread replies
//! (all other phases, --thread-ts provided).

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

/// Testable entry point.
///
/// Returns Ok(json) for both success and application-level errors (status: error).
/// Returns Err(string) only for infrastructure failures.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let root = project_root();
    let branch = &args.branch;
    let phase_num = phase_config::phase_number(&args.phase);
    let paths = FlowPaths::new(&root, branch);
    let state_path = paths.state_file();

    // Drift guard: phase transitions must happen from inside the
    // subdirectory the flow was started in. Running phase-finalize
    // from the wrong subdirectory of a mono-repo would mark the phase
    // complete against the wrong assumed scope. See
    // [`crate::cwd_scope::enforce`].
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    if let Err(msg) = crate::cwd_scope::enforce(&cwd, &root) {
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
        &root,
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
        slack_result = notify_slack::notify(&slack_args);

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
            &root,
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

/// CLI entry point.
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
