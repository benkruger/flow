use indexmap::IndexMap;
use serde_json::Value;

use crate::phase_config::{self, PhaseConfig, PHASE_ORDER};

/// Check if entry into `phase` is allowed given the state JSON.
///
/// Returns `Ok((allowed, message))` where message may be empty if allowed
/// with no note. Returns `Err` if the phase name is invalid.
pub fn check_phase(
    state: &Value,
    phase: &str,
    phase_config: Option<&PhaseConfig>,
) -> Result<(bool, String), String> {
    let default_order: Vec<String> = PHASE_ORDER.iter().map(|&s| s.to_string()).collect();
    let default_names: IndexMap<String, String> = phase_config::phase_names();
    let default_numbers: IndexMap<String, usize> = phase_config::phase_numbers();
    let default_commands: IndexMap<String, String> = phase_config::commands();

    #[allow(clippy::type_complexity)]
    let (order, names, numbers, commands): (
        &Vec<String>,
        &IndexMap<String, String>,
        &IndexMap<String, usize>,
        &IndexMap<String, String>,
    ) = match phase_config {
        Some(cfg) => (&cfg.order, &cfg.names, &cfg.numbers, &cfg.commands),
        None => (
            &default_order,
            &default_names,
            &default_numbers,
            &default_commands,
        ),
    };

    let phase_idx = order.iter().position(|p| p == phase).ok_or_else(|| {
        format!(
            "Invalid phase: {}. Must be one of: {}",
            phase,
            order.join(", ")
        )
    })?;

    // First phase has no prerequisites
    if phase_idx == 0 {
        return Ok((true, String::new()));
    }

    let prev = &order[phase_idx - 1];
    let phases = state.get("phases").and_then(|v| v.as_object());

    let prev_data = phases.and_then(|p| p.get(prev.as_str()));
    let prev_status = prev_data
        .and_then(|d| d.get("status"))
        .and_then(|s| s.as_str())
        .unwrap_or("pending");

    let prev_name = names
        .get(prev.as_str())
        .cloned()
        .unwrap_or_else(|| prev.clone());
    let prev_num = numbers.get(prev.as_str()).copied().unwrap_or(0);
    let prev_cmd = commands
        .get(prev.as_str())
        .cloned()
        .unwrap_or_else(|| format!("/flow:{}", prev));

    let phase_name = names
        .get(phase)
        .cloned()
        .unwrap_or_else(|| phase.to_string());
    let phase_num = numbers.get(phase).copied().unwrap_or(0);

    if prev_status != "complete" {
        let msg = format!(
            "BLOCKED: Phase {}: {} must be complete before entering Phase {}: {}.\n\
             Phase {} current status: {}\n\
             Complete it first with: {}",
            prev_num, prev_name, phase_num, phase_name, prev_num, prev_status, prev_cmd
        );
        return Ok((false, msg));
    }

    // Allowed — check if revisiting
    let this_data = phases.and_then(|p| p.get(phase));
    let this_status = this_data
        .and_then(|d| d.get("status"))
        .and_then(|s| s.as_str())
        .unwrap_or("pending");

    if this_status == "complete" {
        let visits = this_data
            .and_then(|d| d.get("visit_count"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let msg = format!(
            "NOTE: Phase {}: {} was previously completed ({} visit(s)). Re-entering.",
            phase_num, phase_name, visits
        );
        return Ok((true, msg));
    }

    Ok((true, String::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phase_config::{self, PHASE_ORDER};
    use indexmap::IndexMap;
    use serde_json::json;

    /// Build a minimal state Value matching conftest.py make_state().
    fn make_state(current_phase: &str, phase_statuses: &[(&str, &str)]) -> Value {
        let phase_names = phase_config::phase_names();
        let mut phases = serde_json::Map::new();
        for &p in PHASE_ORDER {
            let status = phase_statuses
                .iter()
                .find(|(k, _)| *k == p)
                .map(|(_, v)| *v)
                .unwrap_or("pending");
            let visit_count = if status == "complete" || status == "in_progress" {
                1
            } else {
                0
            };
            phases.insert(
                p.to_string(),
                json!({
                    "name": phase_names.get(p).unwrap_or(&String::new()),
                    "status": status,
                    "started_at": null,
                    "completed_at": null,
                    "session_started_at": if status == "in_progress" { json!("2026-01-01T00:00:00Z") } else { json!(null) },
                    "cumulative_seconds": 0,
                    "visit_count": visit_count,
                }),
            );
        }
        json!({
            "branch": "test-feature",
            "current_phase": current_phase,
            "phases": phases,
        })
    }

    // --- Phase status checks ---

    #[test]
    fn previous_phase_pending_blocks() {
        let state = make_state("flow-plan", &[("flow-start", "pending")]);
        let (allowed, output) = check_phase(&state, "flow-plan", None).unwrap();
        assert!(!allowed);
        assert!(output.contains("BLOCKED"));
        assert!(output.contains("pending"));
    }

    #[test]
    fn previous_phase_in_progress_blocks() {
        let state = make_state("flow-plan", &[("flow-start", "in_progress")]);
        let (allowed, output) = check_phase(&state, "flow-plan", None).unwrap();
        assert!(!allowed);
        assert!(output.contains("BLOCKED"));
        assert!(output.contains("in_progress"));
    }

    #[test]
    fn previous_phase_complete_allows() {
        let state = make_state("flow-plan", &[("flow-start", "complete")]);
        let (allowed, _output) = check_phase(&state, "flow-plan", None).unwrap();
        assert!(allowed);
    }

    #[test]
    fn sequential_chain_phase_4_with_1_to_3_complete() {
        let state = make_state(
            "flow-code-review",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
            ],
        );
        let (allowed, _) = check_phase(&state, "flow-code-review", None).unwrap();
        assert!(allowed);
    }

    // --- Re-entry ---

    #[test]
    fn re_entering_completed_phase_shows_note() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "complete")],
        );
        state["phases"]["flow-plan"]["visit_count"] = json!(2);
        let (allowed, output) = check_phase(&state, "flow-plan", None).unwrap();
        assert!(allowed);
        assert!(output.contains("previously completed"));
        assert!(output.contains("2 visit(s)"));
    }

    #[test]
    fn first_visit_no_previously_completed_message() {
        let state = make_state("flow-plan", &[("flow-start", "complete")]);
        let (allowed, output) = check_phase(&state, "flow-plan", None).unwrap();
        assert!(allowed);
        assert!(!output.contains("previously completed"));
    }

    #[test]
    fn phase_5_requires_phase_4_complete() {
        let state = make_state(
            "flow-learn",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
                ("flow-code-review", "pending"),
            ],
        );
        let (allowed, output) = check_phase(&state, "flow-learn", None).unwrap();
        assert!(!allowed);
        assert!(output.contains("Phase 4"));
    }

    #[test]
    fn phase_6_requires_phase_5_complete() {
        let state = make_state(
            "flow-complete",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
                ("flow-code-review", "complete"),
                ("flow-learn", "pending"),
            ],
        );
        let (allowed, output) = check_phase(&state, "flow-complete", None).unwrap();
        assert!(!allowed);
        assert!(output.contains("Phase 5"));
    }

    #[test]
    fn missing_phases_key_blocks() {
        let state = json!({"branch": "test", "current_phase": "flow-plan"});
        let (allowed, output) = check_phase(&state, "flow-plan", None).unwrap();
        assert!(!allowed);
        assert!(output.contains("BLOCKED"));
    }

    #[test]
    fn blocked_message_includes_correct_command() {
        let state = make_state(
            "flow-code-review",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "pending"),
            ],
        );
        let (allowed, output) = check_phase(&state, "flow-code-review", None).unwrap();
        assert!(!allowed);
        assert!(output.contains("/flow:flow-code"));
    }

    #[test]
    fn invalid_phase_name_errors() {
        let state = make_state("flow-start", &[("flow-start", "complete")]);
        let result = check_phase(&state, "nonexistent", None);
        assert!(result.is_err());
    }

    // --- Frozen config ---

    #[test]
    fn check_phase_uses_frozen_config() {
        let config = PhaseConfig {
            order: vec![
                "flow-start".into(),
                "flow-plan".into(),
                "flow-code-review".into(),
            ],
            names: {
                let mut m = IndexMap::new();
                m.insert("flow-start".into(), "Start".into());
                m.insert("flow-plan".into(), "Plan".into());
                m.insert("flow-code-review".into(), "Review".into());
                m
            },
            numbers: {
                let mut m = IndexMap::new();
                m.insert("flow-start".into(), 1);
                m.insert("flow-plan".into(), 2);
                m.insert("flow-code-review".into(), 3);
                m
            },
            commands: {
                let mut m = IndexMap::new();
                m.insert("flow-start".into(), "/t:a".into());
                m.insert("flow-plan".into(), "/t:b".into());
                m.insert("flow-code-review".into(), "/t:c".into());
                m
            },
        };
        let state = make_state(
            "flow-code-review",
            &[("flow-start", "complete"), ("flow-plan", "complete")],
        );
        let (allowed, _) = check_phase(&state, "flow-code-review", Some(&config)).unwrap();
        assert!(allowed);
    }

    #[test]
    fn check_phase_frozen_config_uses_correct_predecessor() {
        // Custom order: start, code, plan — so plan's predecessor is code
        let config = PhaseConfig {
            order: vec!["flow-start".into(), "flow-code".into(), "flow-plan".into()],
            names: {
                let mut m = IndexMap::new();
                m.insert("flow-start".into(), "Start".into());
                m.insert("flow-code".into(), "Code".into());
                m.insert("flow-plan".into(), "Plan".into());
                m
            },
            numbers: {
                let mut m = IndexMap::new();
                m.insert("flow-start".into(), 1);
                m.insert("flow-code".into(), 2);
                m.insert("flow-plan".into(), 3);
                m
            },
            commands: {
                let mut m = IndexMap::new();
                m.insert("flow-start".into(), "/t:a".into());
                m.insert("flow-code".into(), "/t:b".into());
                m.insert("flow-plan".into(), "/t:c".into());
                m
            },
        };
        // In default order, plan's predecessor is start (complete) → allowed
        // In custom order, plan's predecessor is code (pending) → blocked
        let state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-code", "pending")],
        );
        let (allowed, output) = check_phase(&state, "flow-plan", Some(&config)).unwrap();
        assert!(!allowed);
        assert!(output.contains("BLOCKED"));
    }
}
