//! Mirror of `src/required_agents.rs` per `.claude/rules/test-placement.md`.
//!
//! Drives the helpers through the crate's public surface
//! (`flow_rs::required_agents::{REQUIRED_AGENTS, required_agents_for_phase}`)
//! so the file's 100/100/100 gate is satisfied through the same path
//! production callers take.

use flow_rs::required_agents::{required_agents_for_phase, REQUIRED_AGENTS};

// --- required_agents_for_phase ---

#[test]
fn required_agents_for_phase_returns_review_set() {
    let agents = required_agents_for_phase("flow-review");
    assert_eq!(
        agents,
        &["reviewer", "pre-mortem", "adversarial", "documentation"]
    );
}

#[test]
fn required_agents_for_phase_returns_learn_set() {
    let agents = required_agents_for_phase("flow-learn");
    assert_eq!(agents, &["learn-analyst"]);
}

#[test]
fn required_agents_for_phase_returns_empty_for_unknown_phase() {
    assert!(required_agents_for_phase("flow-code").is_empty());
    assert!(required_agents_for_phase("flow-start").is_empty());
    assert!(required_agents_for_phase("").is_empty());
    assert!(required_agents_for_phase("nonsense").is_empty());
}

// --- REQUIRED_AGENTS constant invariants ---

#[test]
fn required_agents_constant_contains_known_phases() {
    let phases: Vec<&str> = REQUIRED_AGENTS.iter().map(|(p, _)| *p).collect();
    assert!(phases.contains(&"flow-review"));
    assert!(phases.contains(&"flow-learn"));
}

#[test]
fn required_agents_constant_lists_no_duplicate_phases() {
    let mut phases: Vec<&str> = REQUIRED_AGENTS.iter().map(|(p, _)| *p).collect();
    let total = phases.len();
    phases.sort_unstable();
    phases.dedup();
    assert_eq!(
        phases.len(),
        total,
        "REQUIRED_AGENTS has duplicate phase keys"
    );
}
