//! Consolidated start-finalize: phase-transition complete + notify-slack +
//! set-timestamp + add-notification in a single command.
//!
//! Returns JSON with formatted_time and continue_action for the skill
//! to use in the COMPLETE banner and transition HARD-GATE.

use std::process;

use clap::Parser;
use serde_json::{json, Value};

use crate::commands::log::append_log;
use crate::commands::start_step::update_step;
use crate::git::project_root;
use crate::lock::mutate_state;
use crate::notify_slack;
use crate::output::json_error;
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

/// Testable entry point.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let root = project_root();
    let branch = &args.branch;
    let state_path = root.join(".flow-states").join(format!("{}.json", branch));

    if !state_path.exists() {
        return Ok(json!({
            "status": "error",
            "message": format!("No state file found: {}", state_path.display()),
        }));
    }

    // Update TUI step counter
    update_step(&state_path, 5);

    // Load frozen phase config if available
    let frozen_path = root
        .join(".flow-states")
        .join(format!("{}-phases.json", branch));
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
            "[Phase 1] start-finalize — phase-transition complete ({})",
            phase_result["status"]
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
    if let Some(ref pr_url) = args.pr_url {
        let message = format!("Phase 1: Start complete — PR created for {}", branch);
        let slack_args = notify_slack::Args {
            phase: "flow-start".to_string(),
            message: message.clone(),
            pr_url: Some(pr_url.clone()),
            thread_ts: None,
            feature: None,
        };
        slack_result = notify_slack::notify(&slack_args);

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
            &root,
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
