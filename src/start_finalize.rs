//! Consolidated start-finalize: phase-transition complete + notify-slack +
//! set-timestamp + add-notification in a single command.
//!
//! Returns JSON with formatted_time and continue_action for the skill
//! to use in the COMPLETE banner and transition HARD-GATE.
//!
//! Tests live in `tests/start_finalize.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` block
//! in this file.

use std::path::Path;

use clap::Parser;
use serde_json::{json, Value};

use crate::commands::log::append_log;
use crate::commands::start_step::update_step;
use crate::flow_paths::FlowPaths;
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

/// Main-arm entry point: runs the full start-finalize sequence.
/// Tests supply a `TempDir` `root` to drive every branch without
/// touching host state.
pub fn run_impl_main(args: &Args, root: &Path) -> (Value, i32) {
    let branch = &args.branch;
    let paths = FlowPaths::new(root, branch);
    let state_path = paths.state_file();

    if !state_path.exists() {
        return (
            json!({
                "status": "error",
                "message": format!("No state file found: {}", state_path.display()),
            }),
            0,
        );
    }

    // Update TUI step counter
    update_step(&state_path, 5);

    let frozen_path = paths.frozen_phases();
    let (frozen_order, frozen_commands) = match phase_config::load_phase_config(&frozen_path) {
        Ok(c) => (Some(c.order), Some(c.commands)),
        Err(_) => (None, None),
    };

    // Step 1: Phase transition complete
    let result_holder = std::cell::RefCell::new(Value::Null);

    let mutate_result = mutate_state(&state_path, &mut |state| {
        let result = phase_complete(
            state,
            "flow-start",
            None,
            frozen_order.as_deref(),
            frozen_commands.as_ref(),
        );
        *result_holder.borrow_mut() = result;
    });

    if let Err(e) = mutate_result {
        return (
            json!({
                "status": "error",
                "message": format!("State mutation failed: {}", e),
            }),
            0,
        );
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

    // Step 2: Slack notification (best-effort). notify_slack::notify
    // returns {"status":"skipped"} when the Slack env vars are unset.
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
            let ts = slack_result["ts"].as_str().unwrap_or("").to_string();
            let msg_clone = message.clone();
            let ts_clone = ts.clone();

            let _ = mutate_state(&state_path, &mut |state| {
                state["slack_thread_ts"] = json!(&ts_clone);

                if !state["notifications"].is_array() {
                    state["notifications"] = json!([]);
                }
                state["notifications"]
                    .as_array_mut()
                    .expect("notifications was just ensured to be an array")
                    .push(json!({
                        "phase": "flow-start",
                        "ts": &ts_clone,
                        "thread_ts": &ts_clone,
                        "message": &msg_clone,
                    }));
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

    let mut response = json!({
        "status": "ok",
        "formatted_time": formatted_time,
        "continue_action": continue_action,
    });

    if slack_result["status"] != "skipped" {
        response["slack"] = slack_result;
    }

    (response, 0)
}
