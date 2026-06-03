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
//! **Single locked read.** All of the decision — current phase, phase
//! status, required-agent match, set membership — and the write happen
//! inside one [`crate::lock::mutate_state`] closure
//! ([`apply_agent_return`]). The validation and the mutation therefore
//! observe one consistent locked state value; there is no
//! time-of-check/time-of-use gap between a separate pre-read and the
//! write. `mutate_state` opens the state file without `create`, so a
//! missing state file (the common no-active-flow case) returns `Err`
//! and records nothing — no state file is created for a branch with no
//! flow — and a corrupt/unparseable file returns `Err` before any
//! write, leaving it untouched.
//!
//! **External-input contract** (per
//! `.claude/rules/external-input-path-construction.md` and
//! `.claude/rules/branch-path-safety.md`): the branch is derived from
//! `cwd` via [`detect_branch_from_path`] and validated through
//! `FlowPaths::try_new` (rejects empty, `.`, `..`, slash-, and
//! NUL-containing branches) before any `.flow-states/` path is built.
//!
//! Tests live at `tests/hooks/agent_run_record.rs` per
//! `.claude/rules/test-placement.md`.

use std::path::Path;

use serde_json::{json, Value};

use crate::flow_paths::FlowPaths;
use crate::hooks::transcript_walker::normalize_gate_input;
use crate::hooks::{detect_branch_from_path, resolve_main_root};
use crate::lock::mutate_state;
use crate::required_agents::required_agents_for_phase;
use crate::utils::now;

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

/// Validate against the locked state and, when the launched subagent is
/// a required agent for the current in-progress phase and not already
/// recorded, append `{agent, timestamp}` to
/// `phases.<phase>.agents_returned` (set-semantics).
///
/// All phase/status reads use `get(...)` chains (never `IndexMut`), so a
/// wrong-type root, `phases`, or `phases.<phase>` reads as absent and
/// the guard returns without mutating — no panic. Crucially, the
/// `status == "in_progress"` check is what proves `phases` and
/// `phases.<phase>` are objects: it is unreadable otherwise, so the
/// guard returns before the write. The subsequent `IndexMut` write into
/// `agents_returned` therefore cannot index a non-object intermediate.
/// The only wrong-type case the write itself heals is a
/// present-but-non-array `agents_returned`, reset to `[]` before the
/// push per `.claude/rules/rust-patterns.md` "State Mutation Object
/// Guards".
fn apply_agent_return(st: &mut Value, sub_norm: &str, timestamp: &str) {
    if !(st.is_object() || st.is_null()) {
        return;
    }
    let phase = st
        .get("current_phase")
        .and_then(|v| v.as_str())
        .map(normalize_gate_input)
        .unwrap_or_default();
    if phase.is_empty() {
        return;
    }
    let status = st
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
    let Some(matched) = required.iter().find(|r| sub_norm == format!("flow:{}", r)) else {
        return;
    };
    let agent = matched.to_string();
    if agents_returned_contains(st, &phase, &agent) {
        return;
    }
    if !st["phases"][phase.as_str()]["agents_returned"].is_array() {
        st["phases"][phase.as_str()]["agents_returned"] = json!([]);
    }
    st["phases"][phase.as_str()]["agents_returned"]
        .as_array_mut()
        .expect("agents_returned is an array after the guard above")
        .push(json!({
            "agent": agent,
            "timestamp": timestamp,
        }));
}

/// Record the launched sub-agent into the current phase's
/// `agents_returned` set. Resolves the branch from `cwd`, builds the
/// branch-scoped state path, and hands the entire decision-and-write to
/// a single [`apply_agent_return`] closure under the state lock.
///
/// Best-effort and fail-open — every failure path returns without a
/// write and the caller never blocks the Agent launch. A missing or
/// corrupt state file causes `mutate_state` to return `Err`, which is
/// discarded. See the module doc for the full contract.
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
    let timestamp = now();
    let _ = mutate_state(&state_path, &mut |st| {
        apply_agent_return(st, &sub_norm, &timestamp);
    });
}
