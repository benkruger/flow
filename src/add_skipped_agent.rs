//! `bin/flow add-skipped-agent` — record a skipped review-agent in
//! `phases.<phase>.agents_skipped` so `phase-finalize` can gate
//! completion when one or more agents were skipped during the
//! phase.
//!
//! Invoked by `flow-review` Step 2's failure-classification logic
//! when an agent's response carries canonical external-failure
//! markers (rate-limit messages, API errors, etc.) and no
//! structured `**Finding` block. The Done handler in `flow-review`
//! then sees the field populated and surfaces the
//! `agents_skipped` error reason from `phase-finalize` to the user
//! for retry / accept / abort.
//!
//! Tests live at `tests/add_skipped_agent.rs` per
//! `.claude/rules/test-placement.md`.

use std::path::Path;

use clap::Parser;
use serde_json::{json, Value};

use crate::flow_paths::FlowPaths;
use crate::lock::mutate_state;
use crate::utils::now;

/// Reasons an agent may be marked as skipped. Positive allowlist per
/// `.claude/rules/security-gates.md` "Positive Allowlist, Not Negative
/// Denylist".
///
/// `exhausted_retries` records an agent that the calling skill
/// (flow-review Step 2, flow-learn Step 1) tried to invoke but gave
/// up on after exceeding the retry cap (3 attempts) — the agent
/// either truncated repeatedly, returned an external-failure marker,
/// or failed transcript verification through
/// `record-agent-return`. The phase-finalize required-agents gate
/// (added in a later task) reads both `agents_returned` and
/// `agents_skipped` so an exhausted-retry agent counts as
/// accounted-for in the same way an `api_error`-skipped agent does.
///
/// **v1 boundary — skip reasons are procedural, not
/// transcript-verified.** Unlike `record-agent-return` (which
/// verifies via the persisted JSONL that an `Agent` `tool_use` +
/// matching `tool_result` pair appears since the phase-enter
/// marker), the skip path here writes the entry unconditionally. A
/// model that calls `add-skipped-agent --reason exhausted_retries`
/// without having actually retried three times satisfies the
/// `agents_skipped` gate the same way a legitimate `api_error`
/// entry does. Exhausted and skipped agents surface to the user
/// in the Complete Done banner's Skipped Agents section
/// (`format_complete_summary::skipped_agents_section`), sourced
/// from `phases.<phase>.agents_skipped`. Procedural enforcement of
/// the retry cap lives in the calling SKILL.md's retry-3-then-skip
/// loop; a future PR may mechanically gate
/// `exhausted_retries` against
/// `phases.<phase>.agent_retry_counts.<agent> >= 3`, but that
/// tightening is out of scope for v1.
pub const ALLOWED_REASONS: &[&str] = &["rate_limit", "api_error", "other", "exhausted_retries"];

#[derive(Parser, Debug)]
#[command(
    name = "add-skipped-agent",
    about = "Record a skipped review-agent for the current phase"
)]
pub struct Args {
    /// Branch name. Validated through `FlowPaths::try_new` per
    /// `.claude/rules/branch-path-safety.md`.
    #[arg(long)]
    pub branch: String,
    /// Agent name (e.g., `reviewer`, `pre-mortem`, `adversarial`,
    /// `documentation`). Stored verbatim — agent-name validation is
    /// the calling skill's responsibility.
    #[arg(long)]
    pub agent: String,
    /// Reason the agent was skipped. Must normalize to one of
    /// `rate_limit`, `api_error`, `other`, or `exhausted_retries`
    /// per `ALLOWED_REASONS`.
    #[arg(long)]
    pub reason: String,
    /// Phase the agent belongs to. Defaults to `flow-review` since
    /// that is the only phase that spawns the four review agents
    /// today. Overridable for forward-compatibility.
    #[arg(long, default_value = "flow-review")]
    pub phase: String,
}

/// Normalize a gate input per `.claude/rules/security-gates.md`
/// "Normalize Before Comparing": strip NUL bytes, trim whitespace,
/// lowercase with ASCII semantics.
fn normalize_gate_input(s: &str) -> String {
    s.replace('\0', "").trim().to_ascii_lowercase()
}

/// Append `{agent, reason, timestamp}` to
/// `state.phases[phase].agents_skipped`. Initializes the array when
/// missing and resets non-object intermediate fields to empty maps
/// per `.claude/rules/rust-patterns.md` "State Mutation Object
/// Guards".
fn apply_skip_mutation(state: &mut Value, phase: &str, agent: &str, reason: &str, timestamp: &str) {
    if !(state.is_object() || state.is_null()) {
        return;
    }
    if !state["phases"].is_object() {
        state["phases"] = json!({});
    }
    if !state["phases"][phase].is_object() {
        state["phases"][phase] = json!({});
    }
    if !state["phases"][phase]["agents_skipped"].is_array() {
        state["phases"][phase]["agents_skipped"] = json!([]);
    }
    let arr = state["phases"][phase]["agents_skipped"]
        .as_array_mut()
        .expect("agents_skipped is an array after the guard above");
    arr.push(json!({
        "agent": agent,
        "reason": reason,
        "timestamp": timestamp,
    }));
}

/// Main-arm dispatcher. Returns `(value, exit_code)` where exit_code
/// is always `0` per the FLOW business-error convention; callers
/// parse the JSON `status` field.
pub fn run_impl_main(args: &Args, root: &Path) -> (Value, i32) {
    let reason_norm = normalize_gate_input(&args.reason);
    if !ALLOWED_REASONS.contains(&reason_norm.as_str()) {
        return (
            json!({
                "status": "error",
                "message": format!(
                    "reason must be one of {:?}; got {:?}",
                    ALLOWED_REASONS, args.reason
                ),
            }),
            0,
        );
    }

    let paths = match FlowPaths::try_new(root, &args.branch) {
        Some(p) => p,
        None => {
            return (
                json!({
                    "status": "error",
                    "message": format!("invalid branch name: {:?}", args.branch),
                }),
                0,
            );
        }
    };

    let state_path = paths.state_file();
    if !state_path.exists() {
        return (
            json!({
                "status": "error",
                "message": format!(
                    "state file not found: {}",
                    state_path.display()
                ),
            }),
            0,
        );
    }

    let timestamp = now();
    let agent = args.agent.clone();
    let phase = args.phase.clone();
    let result = mutate_state(&state_path, &mut |state| {
        apply_skip_mutation(state, &phase, &agent, &reason_norm, &timestamp);
    });

    match result {
        Ok(state) => {
            let count = state["phases"][&args.phase]["agents_skipped"]
                .as_array()
                .map(|a| a.len())
                .unwrap_or(0);
            (
                json!({
                    "status": "ok",
                    "agents_skipped_count": count,
                    "phase": args.phase,
                }),
                0,
            )
        }
        Err(e) => (
            json!({
                "status": "error",
                "message": format!("failed to add skipped-agent: {}", e),
            }),
            0,
        ),
    }
}
