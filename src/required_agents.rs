//! Authoritative per-phase required-agent set.
//!
//! The `phase-finalize` required-agents gate checks the set returned
//! by [`required_agents_for_phase`] against
//! `phases.<phase>.agents_returned` (recorded by FLOW's
//! `PreToolUse:Agent` hook when the model launches each required
//! agent — see `crate::hooks::agent_run_record`). A phase with any
//! required agent absent from `agents_returned` fails finalize with
//! `reason: "required_agent_not_returned"`.
//!
//! The constant is bound to the matching SKILL.md agent-invocation set
//! by `tests/skill_contracts.rs::required_agents_matches_skill_invocations`.
//! A SKILL.md edit that adds, removes, or renames an
//! `subagent_type: "flow:<name>"` invocation without updating this
//! constant fails the contract test.

/// Phase keyed slice of required agent names. Each agent name matches
/// the `<name>` portion of a `subagent_type: "flow:<name>"` invocation
/// in the phase's SKILL.md. Agent names are stored lowercase so a
/// `normalize_gate_input`-normalized `flow:<name>` comparison matches.
pub const REQUIRED_AGENTS: &[(&str, &[&str])] = &[
    (
        "flow-review",
        &["reviewer", "pre-mortem", "adversarial", "documentation"],
    ),
    ("flow-learn", &["learn-analyst"]),
];

/// Return the required-agent slice for `phase`, or an empty slice when
/// the phase has no required agents (e.g. flow-start, flow-code,
/// flow-complete).
pub fn required_agents_for_phase(phase: &str) -> &'static [&'static str] {
    for (key, agents) in REQUIRED_AGENTS {
        if *key == phase {
            return agents;
        }
    }
    &[]
}
