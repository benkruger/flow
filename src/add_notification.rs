//! `bin/flow add-notification` — record a Slack notification in FLOW state.
//!
//! Tests live at `tests/add_notification.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` in
//! this file.

use std::path::Path;

use clap::Parser;
use serde_json::{json, Value};

use crate::flow_paths::FlowPaths;
use crate::git::resolve_branch;
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

/// Applies the slack_notifications append transform to the in-memory
/// state. Extracted to a named function so cargo-llvm-cov measures a
/// single monomorphization of the mutation logic regardless of how
/// many tests or production paths call [`run_impl_main`]. See
/// `add_issue::apply_issue_mutation` for the mirror pattern.
fn apply_notification_mutation(
    state: &mut Value,
    args: &Args,
    phase_name: &str,
    preview: &str,
    timestamp: &str,
) {
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
        apply_notification_mutation(state, &args, &phase_name, &preview, &timestamp);
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

/// Truncate a message to at most `MAX_PREVIEW_LENGTH` characters, appending
/// "..." when truncation occurs. Tests drive this directly via the public
/// surface.
pub fn truncate_preview(message: &str) -> String {
    if message.chars().count() > MAX_PREVIEW_LENGTH {
        let truncated: String = message.chars().take(MAX_PREVIEW_LENGTH).collect();
        format!("{}...", truncated)
    } else {
        message.to_string()
    }
}
