use std::process::Command;

use indexmap::IndexMap;
use serde_json::{json, Value};

use crate::phase_config::{self, PHASE_ORDER};
use crate::utils::{elapsed_since, format_time, now, tolerant_i64};

/// Apply phase entry mutations to the state Value in-place.
///
/// Returns the result JSON to print to stdout.
pub fn phase_enter(state: &mut Value, phase: &str, reason: Option<&str>) -> Value {
    let prev_phase = state
        .get("current_phase")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Guard: reset "phases" to an empty object if it is not an object or null.
    // IndexMut panics on string key access to arrays, strings, bools, and numbers.
    if let Some(phases) = state.get("phases") {
        if !phases.is_object() && !phases.is_null() {
            state["phases"] = json!({});
        }
    }

    let phase_data = &mut state["phases"][phase];

    phase_data["status"] = json!("in_progress");
    if phase_data["started_at"].is_null() {
        phase_data["started_at"] = json!(now());
    }
    phase_data["session_started_at"] = json!(now());

    let visit_count = tolerant_i64(&phase_data["visit_count"]).saturating_add(1);
    phase_data["visit_count"] = json!(visit_count);

    state["current_phase"] = json!(phase);

    // Record phase transition
    let mut transition = json!({
        "from": prev_phase,
        "to": phase,
        "timestamp": now(),
    });
    if let Some(r) = reason {
        transition["reason"] = json!(r);
    }

    if !state.get("phase_transitions").is_some_and(|v| v.is_array()) {
        state["phase_transitions"] = json!([]);
    }
    state["phase_transitions"]
        .as_array_mut()
        .unwrap()
        .push(transition);

    // Clear auto-continue, discussion-mode, and stale continuation flags from previous phase
    if let Some(obj) = state.as_object_mut() {
        obj.remove("_auto_continue");
        obj.remove("_stop_instructed");
        obj.remove("_continue_pending");
        obj.remove("_continue_context");
    }

    let first_visit = visit_count == 1;

    json!({
        "status": "ok",
        "phase": phase,
        "action": "enter",
        "visit_count": visit_count,
        "first_visit": first_visit,
    })
}

/// Apply phase completion mutations to the state Value in-place.
///
/// Returns the result JSON to print to stdout.
pub fn phase_complete(
    state: &mut Value,
    phase: &str,
    next_phase: Option<&str>,
    phase_order: Option<&[String]>,
    phase_commands: Option<&IndexMap<String, String>>,
) -> Value {
    let default_order: Vec<String> = PHASE_ORDER.iter().map(|&s| s.to_string()).collect();
    let default_commands = phase_config::commands();

    let order = phase_order.unwrap_or(&default_order);
    let commands = phase_commands.unwrap_or(&default_commands);

    // Determine next phase
    let next = match next_phase {
        Some(np) => np.to_string(),
        None => {
            let phase_idx = order.iter().position(|p| p == phase).unwrap_or(0);
            if phase_idx + 1 < order.len() {
                order[phase_idx + 1].clone()
            } else {
                phase.to_string() // terminal phase points to itself
            }
        }
    };

    // Guard: reset "phases" to an empty object if it is not an object or null.
    // Mirrors the same guard in phase_enter — both functions access
    // state["phases"][phase] via IndexMut, which panics on non-object types.
    if let Some(phases) = state.get("phases") {
        if !phases.is_object() && !phases.is_null() {
            state["phases"] = json!({});
        }
    }

    // Compute elapsed time
    let session_started = state["phases"][phase]["session_started_at"]
        .as_str()
        .map(String::from);
    let elapsed = elapsed_since(session_started.as_deref(), None);

    let existing = tolerant_i64(&state["phases"][phase]["cumulative_seconds"]);
    let cumulative = existing.saturating_add(elapsed);

    // Update phase state
    state["phases"][phase]["cumulative_seconds"] = json!(cumulative);
    state["phases"][phase]["status"] = json!("complete");
    state["phases"][phase]["completed_at"] = json!(now());
    state["phases"][phase]["session_started_at"] = json!(null);
    state["current_phase"] = json!(&next);

    // Determine continue mode from skills config
    let continue_mode = state
        .get("skills")
        .and_then(|skills| skills.get(phase))
        .and_then(|cfg| {
            // String config (e.g. "auto")
            if let Some(s) = cfg.as_str() {
                return Some(s.to_string());
            }
            // Dict config (e.g. {"continue": "auto"})
            if let Some(obj) = cfg.as_object() {
                return obj
                    .get("continue")
                    .and_then(|v| v.as_str())
                    .map(String::from);
            }
            None
        });

    let next_command = commands.get(&next).cloned();

    let (continue_action, should_set_auto_continue) =
        if continue_mode.as_deref() == Some("auto") && next_command.is_some() {
            ("invoke", true)
        } else {
            ("ask", false)
        };

    if should_set_auto_continue {
        state["_auto_continue"] = json!(next_command.as_ref().unwrap());
    } else if let Some(obj) = state.as_object_mut() {
        obj.remove("_auto_continue");
    }

    // Capture diff stats for code phase
    if phase == "flow-code" {
        state["diff_stats"] = capture_diff_stats();
    }

    let mut result = json!({
        "status": "ok",
        "phase": phase,
        "action": "complete",
        "cumulative_seconds": cumulative,
        "formatted_time": format_time(cumulative),
        "next_phase": next,
        "continue_action": continue_action,
    });

    if continue_action == "invoke" {
        result["continue_target"] = json!(next_command.unwrap());
    }

    result
}

/// Capture git diff --stat summary for the current branch vs main.
///
/// Returns a JSON object with files_changed, insertions, deletions, captured_at.
/// Best-effort: returns zeros if git fails.
pub fn capture_diff_stats() -> Value {
    let zeros = || {
        json!({
            "files_changed": 0,
            "insertions": 0,
            "deletions": 0,
            "captured_at": now()
        })
    };

    let output = match Command::new("git")
        .args(["diff", "--stat", "main...HEAD"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return zeros(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return zeros();
    }

    let lines: Vec<&str> = trimmed.lines().collect();
    let summary = lines.last().unwrap_or(&"");

    let mut files_changed: i64 = 0;
    let mut insertions: i64 = 0;
    let mut deletions: i64 = 0;

    for part in summary.split(',') {
        let part = part.trim();
        if part.contains("file") {
            if let Some(n) = part.split_whitespace().next().and_then(|s| s.parse().ok()) {
                files_changed = n;
            }
        } else if part.contains("insertion") {
            if let Some(n) = part.split_whitespace().next().and_then(|s| s.parse().ok()) {
                insertions = n;
            }
        } else if part.contains("deletion") {
            if let Some(n) = part.split_whitespace().next().and_then(|s| s.parse().ok()) {
                deletions = n;
            }
        }
    }

    json!({
        "files_changed": files_changed,
        "insertions": insertions,
        "deletions": deletions,
        "captured_at": now()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phase_config;
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
            let visit_count: i64 = if status == "complete" || status == "in_progress" {
                1
            } else {
                0
            };
            let session = if status == "in_progress" {
                json!("2026-01-01T00:00:00Z")
            } else {
                json!(null)
            };
            phases.insert(
                p.to_string(),
                json!({
                    "name": phase_names.get(p).unwrap_or(&String::new()),
                    "status": status,
                    "started_at": null,
                    "completed_at": null,
                    "session_started_at": session,
                    "cumulative_seconds": 0,
                    "visit_count": visit_count,
                }),
            );
        }
        json!({
            "branch": "test-feature",
            "current_phase": current_phase,
            "phases": phases,
            "phase_transitions": [],
        })
    }

    // ===== phase_enter tests =====

    #[test]
    fn enter_sets_all_fields() {
        let mut state = make_state("flow-start", &[("flow-start", "complete")]);
        let result = phase_enter(&mut state, "flow-plan", None);

        assert_eq!(result["status"], "ok");
        assert_eq!(result["phase"], "flow-plan");
        assert_eq!(result["action"], "enter");
        assert_eq!(result["visit_count"], 1);
        assert_eq!(result["first_visit"], true);

        assert_eq!(state["phases"]["flow-plan"]["status"], "in_progress");
        assert!(state["phases"]["flow-plan"]["started_at"].is_string());
        assert!(state["phases"]["flow-plan"]["session_started_at"].is_string());
        assert_eq!(state["phases"]["flow-plan"]["visit_count"], 1);
        assert_eq!(state["current_phase"], "flow-plan");
    }

    #[test]
    fn enter_first_visit_sets_started_at() {
        let mut state = make_state("flow-start", &[("flow-start", "complete")]);
        assert!(state["phases"]["flow-plan"]["started_at"].is_null());

        phase_enter(&mut state, "flow-plan", None);

        assert!(state["phases"]["flow-plan"]["started_at"].is_string());
    }

    #[test]
    fn enter_reentry_preserves_started_at() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "complete")],
        );
        state["phases"]["flow-plan"]["started_at"] = json!("2026-01-15T10:00:00Z");
        state["phases"]["flow-plan"]["visit_count"] = json!(2);

        let result = phase_enter(&mut state, "flow-plan", None);

        assert_eq!(result["visit_count"], 3);
        assert_eq!(result["first_visit"], false);
        assert_eq!(
            state["phases"]["flow-plan"]["started_at"],
            "2026-01-15T10:00:00Z"
        );
        assert_eq!(state["phases"]["flow-plan"]["visit_count"], 3);
    }

    #[test]
    fn enter_flow_complete() {
        let mut state = make_state(
            "flow-learn",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
                ("flow-code-review", "complete"),
                ("flow-learn", "complete"),
            ],
        );
        let result = phase_enter(&mut state, "flow-complete", None);

        assert_eq!(result["status"], "ok");
        assert_eq!(result["phase"], "flow-complete");
        assert_eq!(result["visit_count"], 1);
        assert_eq!(result["first_visit"], true);
        assert_eq!(state["phases"]["flow-complete"]["status"], "in_progress");
        assert!(state["phases"]["flow-complete"]["started_at"].is_string());
        assert_eq!(state["current_phase"], "flow-complete");
    }

    #[test]
    fn enter_non_code_review_does_not_set_code_review_step() {
        let mut state = make_state("flow-start", &[("flow-start", "complete")]);
        phase_enter(&mut state, "flow-plan", None);

        assert!(state.get("code_review_step").is_none() || state["code_review_step"].is_null());
    }

    #[test]
    fn enter_clears_auto_continue() {
        let mut state = make_state("flow-start", &[("flow-start", "complete")]);
        state["_auto_continue"] = json!("/flow:flow-plan");

        phase_enter(&mut state, "flow-plan", None);

        assert!(state.get("_auto_continue").is_none() || state["_auto_continue"].is_null());
    }

    #[test]
    fn enter_clears_stop_instructed() {
        let mut state = make_state("flow-start", &[("flow-start", "complete")]);
        state["_stop_instructed"] = json!(true);

        phase_enter(&mut state, "flow-plan", None);

        assert!(
            state.get("_stop_instructed").is_none(),
            "_stop_instructed must be removed by phase_enter"
        );
    }

    #[test]
    fn enter_clears_continue_pending() {
        let mut state = make_state("flow-start", &[("flow-start", "complete")]);
        state["_continue_pending"] = json!("commit");
        state["_continue_context"] = json!("stale instructions");

        phase_enter(&mut state, "flow-plan", None);

        assert!(
            state.get("_continue_pending").is_none(),
            "_continue_pending must be removed by phase_enter"
        );
        assert!(
            state.get("_continue_context").is_none(),
            "_continue_context must be removed by phase_enter"
        );
    }

    #[test]
    fn enter_no_error_when_auto_continue_absent() {
        let mut state = make_state("flow-start", &[("flow-start", "complete")]);
        // No _auto_continue in state
        let result = phase_enter(&mut state, "flow-plan", None);

        assert_eq!(result["status"], "ok");
        assert!(state.get("_auto_continue").is_none() || state["_auto_continue"].is_null());
    }

    #[test]
    fn enter_records_phase_transition() {
        let mut state = make_state("flow-start", &[("flow-start", "complete")]);
        state["phase_transitions"] = json!([]);

        phase_enter(&mut state, "flow-plan", None);

        let transitions = state["phase_transitions"].as_array().unwrap();
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0]["from"], "flow-start");
        assert_eq!(transitions[0]["to"], "flow-plan");
        assert!(transitions[0]["timestamp"].is_string());
        assert!(transitions[0].get("reason").is_none() || transitions[0]["reason"].is_null());
    }

    #[test]
    fn enter_appends_to_existing_transitions() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "complete")],
        );
        state["phase_transitions"] = json!([
            {"from": "flow-start", "to": "flow-plan", "timestamp": "2026-01-01T00:00:00-08:00"}
        ]);

        phase_enter(&mut state, "flow-code", None);

        let transitions = state["phase_transitions"].as_array().unwrap();
        assert_eq!(transitions.len(), 2);
        assert_eq!(transitions[1]["from"], "flow-plan");
        assert_eq!(transitions[1]["to"], "flow-code");
    }

    #[test]
    fn enter_transition_has_no_reason_by_default() {
        let mut state = make_state("flow-start", &[("flow-start", "complete")]);
        state["phase_transitions"] = json!([]);

        phase_enter(&mut state, "flow-plan", None);

        let entry = &state["phase_transitions"][0];
        assert!(entry.get("reason").is_none() || entry["reason"].is_null());
    }

    #[test]
    fn enter_transition_with_reason() {
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
            ],
        );
        state["phase_transitions"] = json!([]);

        phase_enter(&mut state, "flow-plan", Some("approach was wrong"));

        assert_eq!(
            state["phase_transitions"][0]["reason"],
            "approach was wrong"
        );
    }

    #[test]
    fn enter_creates_transitions_array_if_missing() {
        let mut state = make_state("flow-start", &[("flow-start", "complete")]);
        // Remove phase_transitions key
        state.as_object_mut().unwrap().remove("phase_transitions");

        phase_enter(&mut state, "flow-plan", None);

        assert!(state["phase_transitions"].is_array());
        assert_eq!(state["phase_transitions"].as_array().unwrap().len(), 1);
    }

    // ===== phase_complete tests =====

    #[test]
    fn complete_sets_all_fields() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        let result = phase_complete(&mut state, "flow-plan", None, None, None);

        assert_eq!(result["status"], "ok");
        assert_eq!(result["phase"], "flow-plan");
        assert_eq!(result["action"], "complete");
        assert!(result.get("cumulative_seconds").is_some());
        assert!(result.get("formatted_time").is_some());
        assert_eq!(result["next_phase"], "flow-code");

        assert_eq!(state["phases"]["flow-plan"]["status"], "complete");
        assert!(state["phases"]["flow-plan"]["completed_at"].is_string());
        assert!(state["phases"]["flow-plan"]["session_started_at"].is_null());
        assert_eq!(state["current_phase"], "flow-code");
    }

    #[test]
    fn complete_adds_to_existing_cumulative() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(600);

        let result = phase_complete(&mut state, "flow-plan", None, None, None);

        assert!(result["cumulative_seconds"].as_i64().unwrap() >= 600);
    }

    #[test]
    fn complete_formatted_time_less_than_one_minute() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(0);
        state["phases"]["flow-plan"]["session_started_at"] = json!(null);

        let result = phase_complete(&mut state, "flow-plan", None, None, None);

        assert_eq!(result["formatted_time"], "<1m");
    }

    #[test]
    fn complete_next_phase_override() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );

        let result = phase_complete(
            &mut state,
            "flow-plan",
            Some("flow-code-review"),
            None,
            None,
        );

        assert_eq!(result["next_phase"], "flow-code-review");
        assert_eq!(state["current_phase"], "flow-code-review");
    }

    #[test]
    fn complete_null_session_started_at() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        state["phases"]["flow-plan"]["session_started_at"] = json!(null);
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(100);

        let result = phase_complete(&mut state, "flow-plan", None, None, None);

        assert_eq!(result["cumulative_seconds"], 100);
    }

    #[test]
    fn complete_formatted_time_minutes() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(300);
        state["phases"]["flow-plan"]["session_started_at"] = json!(null);

        let result = phase_complete(&mut state, "flow-plan", None, None, None);

        assert_eq!(result["formatted_time"], "5m");
    }

    #[test]
    fn complete_formatted_time_hours() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(3900);
        state["phases"]["flow-plan"]["session_started_at"] = json!(null);

        let result = phase_complete(&mut state, "flow-plan", None, None, None);

        assert_eq!(result["formatted_time"], "1h 5m");
    }

    #[test]
    fn complete_uses_custom_phase_order() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        let custom_order: Vec<String> = vec![
            "flow-start".into(),
            "flow-plan".into(),
            "flow-code-review".into(),
        ];

        let result = phase_complete(&mut state, "flow-plan", None, Some(&custom_order), None);

        assert_eq!(result["next_phase"], "flow-code-review");
        assert_eq!(state["current_phase"], "flow-code-review");
    }

    #[test]
    fn complete_terminal_phase_auto_next() {
        let mut state = make_state(
            "flow-complete",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
                ("flow-code-review", "complete"),
                ("flow-learn", "complete"),
                ("flow-complete", "in_progress"),
            ],
        );

        let result = phase_complete(&mut state, "flow-complete", None, None, None);

        assert_eq!(result["status"], "ok");
        assert_eq!(result["next_phase"], "flow-complete");
        assert_eq!(state["current_phase"], "flow-complete");
    }

    #[test]
    fn complete_flow_complete_with_next_phase() {
        let mut state = make_state(
            "flow-complete",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
                ("flow-code-review", "complete"),
                ("flow-learn", "complete"),
                ("flow-complete", "in_progress"),
            ],
        );

        let result = phase_complete(
            &mut state,
            "flow-complete",
            Some("flow-complete"),
            None,
            None,
        );

        assert_eq!(result["status"], "ok");
        assert_eq!(result["next_phase"], "flow-complete");
        assert_eq!(state["phases"]["flow-complete"]["status"], "complete");
        assert!(state["phases"]["flow-complete"]["completed_at"].is_string());
    }

    // ===== Auto-continue tests =====

    #[test]
    fn complete_sets_auto_continue_when_skills_continue_auto() {
        let mut state = make_state("flow-start", &[("flow-start", "in_progress")]);
        state["skills"] = json!({"flow-start": {"continue": "auto"}});

        let result = phase_complete(&mut state, "flow-start", None, None, None);

        assert_eq!(state["_auto_continue"], "/flow:flow-plan");
        assert_eq!(result["next_phase"], "flow-plan");
    }

    #[test]
    fn complete_sets_auto_continue_with_flat_string_config() {
        let mut state = make_state("flow-start", &[("flow-start", "in_progress")]);
        state["skills"] = json!({"flow-start": "auto"});

        phase_complete(&mut state, "flow-start", None, None, None);

        assert_eq!(state["_auto_continue"], "/flow:flow-plan");
    }

    #[test]
    fn complete_no_auto_continue_when_manual() {
        let mut state = make_state("flow-start", &[("flow-start", "in_progress")]);
        state["skills"] = json!({"flow-start": {"continue": "manual"}});

        phase_complete(&mut state, "flow-start", None, None, None);

        assert!(state.get("_auto_continue").is_none() || state["_auto_continue"].is_null());
    }

    #[test]
    fn complete_no_auto_continue_when_no_skills() {
        let mut state = make_state("flow-start", &[("flow-start", "in_progress")]);
        // No skills key

        phase_complete(&mut state, "flow-start", None, None, None);

        assert!(state.get("_auto_continue").is_none() || state["_auto_continue"].is_null());
    }

    #[test]
    fn complete_clears_auto_continue_when_switching_to_manual() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        state["skills"] = json!({"flow-plan": {"continue": "manual"}});
        state["_auto_continue"] = json!("/flow:flow-plan");

        phase_complete(&mut state, "flow-plan", None, None, None);

        assert!(state.get("_auto_continue").is_none() || state["_auto_continue"].is_null());
    }

    #[test]
    fn complete_no_auto_continue_when_skill_config_unexpected_type() {
        let mut state = make_state("flow-start", &[("flow-start", "in_progress")]);
        state["skills"] = json!({"flow-start": 42});

        phase_complete(&mut state, "flow-start", None, None, None);

        assert!(state.get("_auto_continue").is_none() || state["_auto_continue"].is_null());
    }

    #[test]
    fn complete_result_continue_action_invoke_when_auto() {
        let mut state = make_state("flow-start", &[("flow-start", "in_progress")]);
        state["skills"] = json!({"flow-start": {"continue": "auto"}});

        let result = phase_complete(&mut state, "flow-start", None, None, None);

        assert_eq!(result["continue_action"], "invoke");
        assert_eq!(result["continue_target"], "/flow:flow-plan");
    }

    #[test]
    fn complete_result_continue_action_ask_when_manual() {
        let mut state = make_state("flow-start", &[("flow-start", "in_progress")]);
        state["skills"] = json!({"flow-start": {"continue": "manual"}});

        let result = phase_complete(&mut state, "flow-start", None, None, None);

        assert_eq!(result["continue_action"], "ask");
        assert!(result.get("continue_target").is_none());
    }

    #[test]
    fn complete_result_continue_action_ask_when_absent() {
        let mut state = make_state("flow-start", &[("flow-start", "in_progress")]);

        let result = phase_complete(&mut state, "flow-start", None, None, None);

        assert_eq!(result["continue_action"], "ask");
        assert!(result.get("continue_target").is_none());
    }

    #[test]
    fn complete_result_continue_action_invoke_with_flat_string() {
        let mut state = make_state("flow-start", &[("flow-start", "in_progress")]);
        state["skills"] = json!({"flow-start": "auto"});

        let result = phase_complete(&mut state, "flow-start", None, None, None);

        assert_eq!(result["continue_action"], "invoke");
        assert_eq!(result["continue_target"], "/flow:flow-plan");
    }

    #[test]
    fn complete_result_continue_action_ask_with_unexpected_type() {
        let mut state = make_state("flow-start", &[("flow-start", "in_progress")]);
        state["skills"] = json!({"flow-start": 42});

        let result = phase_complete(&mut state, "flow-start", None, None, None);

        assert_eq!(result["continue_action"], "ask");
        assert!(result.get("continue_target").is_none());
    }

    #[test]
    fn complete_result_continue_action_ask_when_auto_but_no_command() {
        let mut state = make_state("flow-start", &[("flow-start", "in_progress")]);
        state["skills"] = json!({"flow-start": {"continue": "auto"}});

        // Pass phase_commands that omits flow-plan
        let mut cmds = IndexMap::new();
        cmds.insert("flow-start".to_string(), "/flow:flow-start".to_string());

        let result = phase_complete(&mut state, "flow-start", None, None, Some(&cmds));

        assert_eq!(result["continue_action"], "ask");
        assert!(result.get("continue_target").is_none());
        assert!(state.get("_auto_continue").is_none() || state["_auto_continue"].is_null());
    }

    #[test]
    fn complete_future_session_started_clamps_to_zero() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        state["phases"]["flow-plan"]["session_started_at"] = json!("2099-12-31T23:59:59Z");
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(50);

        let result = phase_complete(&mut state, "flow-plan", None, None, None);

        assert_eq!(result["cumulative_seconds"], 50);
    }

    // ===== counter type tolerance tests =====

    #[test]
    fn enter_visit_count_string_tolerance() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "complete")],
        );
        // Simulate a state file where visit_count was stored as a string
        state["phases"]["flow-plan"]["visit_count"] = json!("3");

        let result = phase_enter(&mut state, "flow-plan", None);

        // Should read "3" as 3 and increment to 4
        assert_eq!(result["visit_count"], 4);
        assert_eq!(state["phases"]["flow-plan"]["visit_count"], 4);
    }

    #[test]
    fn enter_visit_count_float_tolerance() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "complete")],
        );
        // Simulate a state file where visit_count was stored as a float
        state["phases"]["flow-plan"]["visit_count"] = json!(3.0);

        let result = phase_enter(&mut state, "flow-plan", None);

        // Should read 3.0 as 3 and increment to 4
        assert_eq!(result["visit_count"], 4);
        assert_eq!(state["phases"]["flow-plan"]["visit_count"], 4);
    }

    #[test]
    fn complete_cumulative_seconds_string_tolerance() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        // Simulate a state file where cumulative_seconds was stored as a string
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!("120");
        state["phases"]["flow-plan"]["session_started_at"] = json!("2099-12-31T23:59:59Z");

        let result = phase_complete(&mut state, "flow-plan", None, None, None);

        // Should read "120" as 120 and preserve it (future session clamps elapsed to 0)
        assert_eq!(result["cumulative_seconds"], 120);
    }

    #[test]
    fn complete_cumulative_seconds_float_tolerance() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        // Simulate a state file where cumulative_seconds was stored as a float
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(120.0);
        state["phases"]["flow-plan"]["session_started_at"] = json!("2099-12-31T23:59:59Z");

        let result = phase_complete(&mut state, "flow-plan", None, None, None);

        // Should read 120.0 as 120 and preserve it (future session clamps elapsed to 0)
        assert_eq!(result["cumulative_seconds"], 120);
    }

    // ===== phase_enter schema robustness tests =====

    #[test]
    fn enter_phases_key_absent() {
        // State has no "phases" key at all — auto-vivification should handle it
        let mut state = json!({
            "branch": "test-feature",
            "current_phase": "flow-start",
        });
        let result = phase_enter(&mut state, "flow-plan", None);
        assert_eq!(result["status"], "ok");
        assert_eq!(state["phases"]["flow-plan"]["status"], "in_progress");
    }

    #[test]
    fn enter_phases_key_null() {
        // State has "phases": null — auto-vivification should handle it
        let mut state = json!({
            "branch": "test-feature",
            "current_phase": "flow-start",
            "phases": null,
        });
        let result = phase_enter(&mut state, "flow-plan", None);
        assert_eq!(result["status"], "ok");
        assert_eq!(state["phases"]["flow-plan"]["status"], "in_progress");
    }

    #[test]
    fn enter_phases_wrong_type_string() {
        // State has "phases" as a string — must not panic
        let mut state = json!({
            "branch": "test-feature",
            "current_phase": "flow-start",
            "phases": "corrupted",
        });
        let result = phase_enter(&mut state, "flow-plan", None);
        // After the guard resets the wrong type, entry should succeed
        assert_eq!(result["status"], "ok");
    }

    #[test]
    fn enter_phases_wrong_type_array() {
        // State has "phases" as an array — must not panic
        let mut state = json!({
            "branch": "test-feature",
            "current_phase": "flow-start",
            "phases": [1, 2, 3],
        });
        let result = phase_enter(&mut state, "flow-plan", None);
        assert_eq!(result["status"], "ok");
    }

    // ===== capture_diff_stats tests =====

    #[test]
    fn capture_diff_stats_returns_zeros_structure() {
        // Just verify the structure — detailed parsing tested in integration
        let stats = capture_diff_stats();
        assert!(stats.get("files_changed").is_some());
        assert!(stats.get("insertions").is_some());
        assert!(stats.get("deletions").is_some());
        assert!(stats.get("captured_at").is_some());
    }
}
