//! PreToolUse:Agent recorder — record a required Review/Learn agent
//! into `phases.<phase>.agents_returned` when the model launches it.
//!
//! The `PreToolUse:Agent` hook fires before a sub-agent starts. The
//! launch itself is the evidence the agent ran: only a real Agent tool
//! call reaches this code, so a model cannot fabricate the record by
//! synthesizing a CLI invocation. `phase-finalize`'s required-agents
//! gate reads `agents_returned` to confirm every required agent for the
//! phase was launched before the phase advances.
//!
//! [`record_agent_run`] is the public entry point, called from
//! [`crate::hooks::validate_pretool`]'s Agent branch after the prompt
//! scan passes and before the call is allowed. It is best-effort and
//! fail-open: any missing/corrupt state, invalid branch, non-matching
//! subagent, or non-`in_progress` phase yields no record, and the Agent
//! launch is never blocked.
//!
//! Set-semantics: each required agent appears at most once in
//! `agents_returned`. A re-launch of the same agent does not duplicate
//! the entry.
//!
//! **External-input contract** (per
//! `.claude/rules/external-input-path-construction.md`): the branch is
//! derived from `cwd` via [`detect_branch_from_path`] and validated
//! through `FlowPaths::try_new` (rejects empty, `.`, `..`, slash-, and
//! NUL-containing branches) before any `.flow-states/` path is built.
//! The decision read is bounded at `STATE_FILE_BYTE_CAP` so a corrupted
//! or maliciously-large state file cannot OOM the hook on the
//! per-launch hot path; the capped read also gates whether a
//! `mutate_state` write runs at all (mutate_state always rewrites the
//! file), so a non-recording launch never rewrites the state.
//!
//! Tests live at `tests/hooks/agent_run_record.rs` per
//! `.claude/rules/test-placement.md`.

use std::path::Path;

use serde_json::{json, Value};

use crate::flow_paths::FlowPaths;
use crate::hooks::stop_continue::STATE_FILE_BYTE_CAP;
use crate::hooks::transcript_walker::normalize_gate_input;
use crate::hooks::{detect_branch_from_path, resolve_main_root};
use crate::lock::mutate_state;
use crate::required_agents::required_agents_for_phase;
use crate::utils::now;

/// Read and parse the state file with a documented size cap. Returns
/// `None` on any failure (missing file, oversized-truncated-to-invalid
/// content, parse error) so callers fail open. The cap bounds the
/// per-launch hot path; see the module doc.
fn read_state_capped(path: &Path) -> Option<Value> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut buf = String::new();
    file.take(STATE_FILE_BYTE_CAP)
        .read_to_string(&mut buf)
        .ok()?;
    serde_json::from_str(&buf).ok()
}

/// Return `true` when `phases.<phase>.agents_returned` already carries
/// an entry whose `agent` field equals `agent`. A missing or wrong-type
/// field reads as "not present".
fn agents_returned_contains(state: &Value, phase: &str, agent: &str) -> bool {
    state
        .get("phases")
        .and_then(|p| p.get(phase))
        .and_then(|p| p.get("agents_returned"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .any(|e| e.get("agent").and_then(|a| a.as_str()) == Some(agent))
        })
        .unwrap_or(false)
}

/// Append `{agent, timestamp}` to `phases.<phase>.agents_returned`,
/// resetting a wrong-type or absent `agents_returned` to an empty array
/// first per `.claude/rules/rust-patterns.md` "State Mutation Object
/// Guards".
///
/// State-root shape is guaranteed an Object by the caller, mirroring
/// `record_agent_return::apply_return_mutation`: `record_agent_run`'s
/// capped pre-read confirms `phases.<phase>.status == "in_progress"`,
/// which is unreadable unless the state root, `phases`, and
/// `phases.<phase>` are all objects. The wrong-root-type and
/// wrong-`phases`-type guards would therefore be unreachable here and
/// are intentionally omitted — only `agents_returned` can be absent or
/// wrong-typed. The caller also pre-checks membership, so this helper
/// pushes unconditionally; set-semantics is owned by the caller's
/// `agents_returned_contains` short-circuit.
fn apply_set_add(state: &mut Value, phase: &str, agent: &str, timestamp: &str) {
    if !state["phases"][phase]["agents_returned"].is_array() {
        state["phases"][phase]["agents_returned"] = json!([]);
    }
    let arr = state["phases"][phase]["agents_returned"]
        .as_array_mut()
        .expect("agents_returned is an array after the guard above");
    arr.push(json!({
        "agent": agent,
        "timestamp": timestamp,
    }));
}

/// Record the launched sub-agent into the current phase's
/// `agents_returned` set when ALL guards hold: a branch resolves from
/// `cwd`, an active flow's state file exists, the state's
/// `current_phase` is `in_progress`, that phase has required agents,
/// and the (normalized) `subagent_type` matches one of them in the
/// canonical `flow:<name>` form.
///
/// Best-effort and fail-open — every failure path returns without a
/// write and the caller never blocks the Agent launch. See the module
/// doc for the full contract.
pub fn record_agent_run(cwd: Option<&Path>, subagent_type: Option<&str>) {
    let Some(cwd) = cwd else {
        return;
    };
    let Some(subagent_type) = subagent_type else {
        return;
    };
    let sub_norm = normalize_gate_input(subagent_type);
    if sub_norm.is_empty() {
        return;
    }
    let Some(branch) = detect_branch_from_path(cwd) else {
        return;
    };
    let main_root = resolve_main_root(cwd);
    let Some(paths) = FlowPaths::try_new(&main_root, &branch) else {
        return;
    };
    let state_path = paths.state_file();

    let Some(state) = read_state_capped(&state_path) else {
        return;
    };
    let phase = state
        .get("current_phase")
        .and_then(|v| v.as_str())
        .map(normalize_gate_input)
        .unwrap_or_default();
    if phase.is_empty() {
        return;
    }
    let status = state
        .get("phases")
        .and_then(|p| p.get(&phase))
        .and_then(|p| p.get("status"))
        .and_then(|v| v.as_str())
        .map(normalize_gate_input)
        .unwrap_or_default();
    if status != "in_progress" {
        return;
    }
    let required = required_agents_for_phase(&phase);
    // subagent_type is emitted as the namespaced `flow:<name>` form;
    // match it (full-string, normalized) against each required agent.
    // Single-form matching is correct per
    // `.claude/rules/skill-name-form-variance.md` because the Review/Learn
    // agents are launched only as `subagent_type: "flow:<name>"`.
    let Some(matched) = required.iter().find(|r| sub_norm == format!("flow:{}", r)) else {
        return;
    };
    let agent = matched.to_string();
    // Set-semantics: a re-launch of an already-recorded agent skips the
    // write entirely so mutate_state never rewrites the file needlessly.
    if agents_returned_contains(&state, &phase, &agent) {
        return;
    }
    let timestamp = now();
    let phase_for_mut = phase.clone();
    let _ = mutate_state(&state_path, &mut |st| {
        apply_set_add(st, &phase_for_mut, &agent, &timestamp);
    });
}
