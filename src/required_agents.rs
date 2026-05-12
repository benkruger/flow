//! Authoritative per-phase required-agent set.
//!
//! The `phase-finalize` required-agents gate composes the set returned
//! by [`required_agents_for_phase`] against
//! `phases.<phase>.agents_returned` (recorded by
//! `bin/flow record-agent-return` after the calling skill verifies an
//! agent's clean return) and `phases.<phase>.agents_skipped`. A phase
//! with any required agent that neither returned nor was skipped fails
//! finalize with `reason: "required_agent_not_returned"`.
//!
//! The constant is bound to the matching SKILL.md agent-invocation set
//! by `tests/skill_contracts.rs::required_agents_matches_skill_invocations`.
//! A SKILL.md edit that adds, removes, or renames an
//! `subagent_type: "flow:<name>"` invocation without updating this
//! constant fails the contract test.

/// Phase keyed slice of required agent names. Each agent name matches
/// the `<name>` portion of a `subagent_type: "flow:<name>"` invocation
/// in the phase's SKILL.md. Agent names are stored lowercase so the
/// `is_known_agent` lookup matches `normalize_gate_input` callers.
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

/// Return `true` when `agent` appears in the union of every phase's
/// required-agent slice. Consumed by `record_agent_return` to reject
/// unknown agent names via the positive-allowlist pattern from
/// `.claude/rules/security-gates.md`.
pub fn is_known_agent(agent: &str) -> bool {
    for (_, agents) in REQUIRED_AGENTS {
        if agents.contains(&agent) {
            return true;
        }
    }
    false
}
