//! Generic phase-finalize: phase_complete() + Slack notification + notification state record.
//!
//! A single command parameterized by `--phase` replaces the per-skill exit ceremony.
//! Handles both thread creation (Start phase, no --thread-ts) and thread replies
//! (all other phases, --thread-ts provided).
//!
//! Public entry point: [`run_impl`] — accepts `root`, `cwd`, and `args` so
//! tests drive it against a tempdir; the main-arm wrapper [`run_impl_main`]
//! resolves the real project root and cwd before delegating. Slack is always
//! routed through `notify_slack::notify` (which returns "skipped" when the
//! env-var config is absent, so library tests see a deterministic no-op).
//! The slack notification is posted BEFORE the single `mutate_state` call so
//! the phase-complete mutation and the slack-state-record happen in one
//! locked transaction — which means the module carries exactly one
//! non-object guard.

use clap::Parser;
use serde_json::{json, Value};

use crate::commands::log::append_log;
use crate::flow_paths::FlowPaths;
use crate::git::project_root;
use crate::lock::mutate_state;
use crate::notify_slack;
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

/// Main-arm wrapper: resolves real `root` and `cwd` then delegates to
/// [`run_impl`]. `dispatch::dispatch_ok_result_json` consumes the result.
pub fn run_impl_main(args: &Args) -> Result<Value, String> {
    let root = project_root();
    let cwd = std::env::current_dir().unwrap_or(std::path::PathBuf::from("."));
    run_impl(&root, &cwd, args)
}

/// Testable entry point. Accepts `root` and `cwd` so tests can drive it
/// against a tempdir without touching the host worktree.
///
/// Returns Ok(json) for both success and application-level errors
/// (status: error). Returns Err(string) only for infrastructure failures.
pub fn run_impl(
    root: &std::path::Path,
    cwd: &std::path::Path,
    args: &Args,
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

    // Slack call runs BEFORE state mutation so both the phase-complete
    // result and the slack-state record land in a single locked
    // transaction below. notify_slack::notify returns
    // `{"status":"skipped"}` when the env-var config is absent.
    let should_notify = args.thread_ts.is_some() || args.pr_url.is_some();
    let (slack_result, slack_message) = if should_notify {
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
        (notify_slack::notify(&slack_args), Some(message))
    } else {
        (json!({"status": "skipped"}), None)
    };

    // Single state mutation: phase_complete + optional slack record.
    let result_holder = std::cell::RefCell::new(Value::Null);
    let phase_name = args.phase.clone();
    let slack_status_is_ok = slack_result["status"] == "ok";
    let slack_ts = slack_result["ts"].as_str().unwrap_or("").to_string();
    let user_thread_ts = args.thread_ts.clone();

    let home = crate::window_snapshot::home_dir_or_empty();
    let mutate_result = mutate_state(&state_path, &mut |state| {
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

        // Capture window snapshot at finalize. phase_complete sets
        // phases.<phase_name>.status = "complete" but keeps the entry
        // as an object, so the IndexMut assignment below cannot panic.
        let snap = crate::window_snapshot::capture_for_active_state(&home, state, root);
        state["phases"][&phase_name]["window_at_complete"] =
            serde_json::to_value(&snap).expect("WindowSnapshot must serialize");

        if !slack_status_is_ok {
            return;
        }
        let thread_ts_for_state = match &user_thread_ts {
            Some(t) => t.clone(),
            None => slack_ts.clone(),
        };

        // Create mode: the new message's ts IS the thread_ts. Persist
        // it to state so reply-mode flows can read it later.
        if thread_ts_for_state == slack_ts {
            state["slack_thread_ts"] = json!(&slack_ts);
        }

        // Append to slack_notifications array. The `is_array` check
        // above normalizes non-array or missing values to `[]`, so the
        // subsequent `as_array_mut()` is guaranteed to return Some.
        if !state
            .get("slack_notifications")
            .map(|v| v.is_array())
            .unwrap_or(false)
        {
            state["slack_notifications"] = json!([]);
        }
        let arr = state["slack_notifications"]
            .as_array_mut()
            .expect("slack_notifications normalized to array above");
        arr.push(json!({
            "phase": &phase_name,
            "ts": &slack_ts,
            "thread_ts": &thread_ts_for_state,
            "message": slack_message.as_deref().unwrap_or(""),
        }));
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

    // `phase_complete` unconditionally returns `{"status":"ok"}` when
    // the transform ran; the only way `phase_result` is not ok here is
    // when the mutate-state guard returned early (array/non-object
    // state), in which case `phase_result` stays `null` and the
    // `unwrap_or` fallbacks below produce a default response.
    let formatted_time = phase_result["formatted_time"]
        .as_str()
        .unwrap_or("<1m")
        .to_string();
    let continue_action = phase_result["continue_action"]
        .as_str()
        .unwrap_or("ask")
        .to_string();

    if should_notify {
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
