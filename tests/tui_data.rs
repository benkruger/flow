//! Integration tests for `src/tui_data.rs`.

use chrono::{DateTime, FixedOffset};
use flow_rs::phase_config::{self, PHASE_ORDER};
use flow_rs::tui_data::{
    flow_summary, load_account_metrics, load_all_flows, load_orchestration, orchestration_summary,
    parse_log_entries, phase_timeline, run_impl_main, status_icon, step_annotation, step_names,
};
use serde_json::{json, Value};

// --- Test helper: make_state ---

fn make_state(current_phase: &str, phase_statuses: &[(&str, &str)]) -> Value {
    let mut phases = serde_json::Map::new();
    let names_map = phase_config::phase_names();

    for &key in PHASE_ORDER {
        let status = phase_statuses
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, s)| *s)
            .unwrap_or("pending");
        let name = names_map.get(key).cloned().unwrap_or_default();
        phases.insert(
            key.to_string(),
            json!({
                "name": name,
                "status": status,
                "started_at": null,
                "completed_at": null,
                "session_started_at": null,
                "cumulative_seconds": 0,
                "visit_count": 0,
            }),
        );
    }

    json!({
        "branch": "test-feature",
        "repo": "test/test",
        "pr_number": 1,
        "pr_url": "https://github.com/test/test/pull/1",
        "started_at": "2026-01-01T00:00:00-08:00",
        "current_phase": current_phase,
        "files": {
            "plan": null,
            "dag": null,
            "log": null,
            "state": null,
        },
        "phases": phases,
        "prompt": "",
    })
}

// --- step_annotation ---

#[test]
fn test_step_annotation_zero_step() {
    assert_eq!(step_annotation(0, 0, ""), "");
}

#[test]
fn test_step_annotation_negative_step() {
    assert_eq!(step_annotation(-1, 5, ""), "");
}

#[test]
fn test_step_annotation_with_total() {
    assert_eq!(step_annotation(3, 11, ""), "step 3 of 11");
}

#[test]
fn test_step_annotation_without_total() {
    assert_eq!(step_annotation(3, 0, ""), "step 3");
}

#[test]
fn test_step_annotation_with_name() {
    assert_eq!(
        step_annotation(5, 5, "finalizing"),
        "finalizing - step 5 of 5"
    );
}

#[test]
fn test_step_annotation_with_name_no_total() {
    assert_eq!(
        step_annotation(3, 0, "creating workspace"),
        "creating workspace - step 3"
    );
}

// --- step_names ---

#[test]
fn test_step_names_start_has_entries() {
    let names = step_names();
    let start = names.get("flow-start").unwrap();
    for key in 1..=5 {
        assert!(
            start.contains_key(&key),
            "missing key {} in flow-start",
            key
        );
    }
    assert_eq!(start.len(), 5);
}

#[test]
fn test_step_names_plan_has_entries() {
    let names = step_names();
    let plan = names.get("flow-plan").unwrap();
    for key in 1..=4 {
        assert!(plan.contains_key(&key), "missing key {} in flow-plan", key);
    }
    assert_eq!(plan.len(), 4);
}

#[test]
fn test_step_names_code_review_has_entries() {
    let names = step_names();
    let cr = names.get("flow-code-review").unwrap();
    for key in 1..=4 {
        assert!(
            cr.contains_key(&key),
            "missing key {} in flow-code-review",
            key
        );
    }
    assert_eq!(cr.len(), 4);
}

#[test]
fn test_step_names_learn_has_entries() {
    let names = step_names();
    let learn = names.get("flow-learn").unwrap();
    for key in 1..=7 {
        assert!(
            learn.contains_key(&key),
            "missing key {} in flow-learn",
            key
        );
    }
    assert_eq!(learn.len(), 7);
}

#[test]
fn test_step_names_complete_has_entries() {
    let names = step_names();
    let complete = names.get("flow-complete").unwrap();
    for key in 1..=6 {
        assert!(
            complete.contains_key(&key),
            "missing key {} in flow-complete",
            key
        );
    }
    assert_eq!(complete.len(), 6);
}

// --- status_icon ---

#[test]
fn test_status_icon_completed() {
    assert_eq!(status_icon("completed"), "\u{2713}");
}

#[test]
fn test_status_icon_failed() {
    assert_eq!(status_icon("failed"), "\u{2717}");
}

#[test]
fn test_status_icon_in_progress() {
    assert_eq!(status_icon("in_progress"), "\u{25b6}");
}

#[test]
fn test_status_icon_pending() {
    assert_eq!(status_icon("pending"), "\u{00b7}");
}

#[test]
fn test_status_icon_unknown() {
    assert_eq!(status_icon("whatever"), "\u{00b7}");
}

// --- phase_timeline ---

fn pacific(s: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(s).unwrap()
}

#[test]
fn test_phase_timeline_all_pending() {
    let state = make_state("flow-start", &[]);
    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline.len(), PHASE_ORDER.len());
    assert!(timeline.iter().all(|e| e.status == "pending"));
}

#[test]
fn test_phase_timeline_mixed() {
    let now = pacific("2026-01-01T00:02:00-08:00");
    let mut state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    state["phases"]["flow-start"]["cumulative_seconds"] = json!(120);
    state["phases"]["flow-plan"]["cumulative_seconds"] = json!(480);
    state["phases"]["flow-code"]["session_started_at"] = json!("2026-01-01T00:00:00-08:00");

    let timeline = phase_timeline(&state, Some(now));

    assert_eq!(timeline[0].status, "complete");
    assert_eq!(timeline[0].time, "2m");
    assert_eq!(timeline[0].number, 1);
    assert_eq!(timeline[1].status, "complete");
    assert_eq!(timeline[1].time, "8m");
    assert_eq!(timeline[2].status, "in_progress");
    assert_eq!(timeline[2].name, "Code");
    assert_eq!(timeline[2].time, "2m");
    assert_eq!(timeline[3].status, "pending");
}

// --- phase_timeline: Start ---

#[test]
fn test_phase_timeline_start_annotation() {
    let mut state = make_state("flow-start", &[("flow-start", "in_progress")]);
    state["start_step"] = json!(3);
    state["start_steps_total"] = json!(5);

    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    let start_entry = &timeline[0];
    assert_eq!(start_entry.annotation, "creating workspace - step 3 of 5");
    assert_eq!(start_entry.name, "Start");
}

#[test]
fn test_phase_timeline_start_step_zero() {
    let state = make_state("flow-start", &[("flow-start", "in_progress")]);
    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[0].annotation, "");
}

#[test]
fn test_phase_timeline_start_no_total() {
    let mut state = make_state("flow-start", &[("flow-start", "in_progress")]);
    state["start_step"] = json!(3);

    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[0].annotation, "creating workspace - step 3");
}

// --- phase_timeline: Plan ---

#[test]
fn test_phase_timeline_plan_annotation() {
    let mut state = make_state(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    state["plan_step"] = json!(2);
    state["plan_steps_total"] = json!(4);

    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[1].annotation, "decomposing - step 2 of 4");
}

#[test]
fn test_phase_timeline_plan_step_zero() {
    let state = make_state(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[1].annotation, "");
}

#[test]
fn test_phase_timeline_plan_no_total() {
    let mut state = make_state(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    state["plan_step"] = json!(2);

    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[1].annotation, "decomposing - step 2");
}

// --- phase_timeline: Code ---

#[test]
fn test_phase_timeline_code_with_task_annotation() {
    let mut state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    state["code_task"] = json!(3);
    state["diff_stats"] = json!({"files_changed": 5, "insertions": 127, "deletions": 48});

    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    let code_entry = &timeline[2];
    assert!(code_entry.annotation.contains("task 4"));
    assert!(code_entry.annotation.contains("+127"));
    assert!(code_entry.annotation.contains("-48"));
}

#[test]
fn test_phase_timeline_code_first_task_annotation() {
    let mut state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    state["code_tasks_total"] = json!(3);

    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[2].annotation, "task 1 of 3");
}

#[test]
fn test_phase_timeline_code_with_total() {
    let mut state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    state["code_task"] = json!(3);
    state["code_tasks_total"] = json!(8);

    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert!(timeline[2].annotation.contains("task 4 of 8"));
}

#[test]
fn test_phase_timeline_code_total_absent_fallback() {
    let mut state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    state["code_task"] = json!(3);

    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[2].annotation, "task 4");
    assert!(!timeline[2].annotation.contains("of"));
}

#[test]
fn test_phase_timeline_code_total_with_diff_stats() {
    let mut state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    state["code_task"] = json!(3);
    state["code_tasks_total"] = json!(8);
    state["diff_stats"] = json!({"insertions": 127, "deletions": 48});

    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[2].annotation, "task 4 of 8, +127 -48");
}

#[test]
fn test_phase_timeline_code_total_zero_ignored() {
    let mut state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    state["code_task"] = json!(3);
    state["code_tasks_total"] = json!(0);

    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[2].annotation, "task 4");
    assert!(!timeline[2].annotation.contains("of"));
}

// --- phase_timeline: Code overflow cap ---

#[test]
fn test_phase_timeline_code_task_overflow_capped() {
    let mut state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    state["code_task"] = json!(3);
    state["code_tasks_total"] = json!(3);

    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[2].annotation, "task 3 of 3");
}

#[test]
fn test_phase_timeline_code_task_overflow_exceeds_total() {
    let mut state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    state["code_task"] = json!(5);
    state["code_tasks_total"] = json!(3);

    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[2].annotation, "task 3 of 3");
}

// --- phase_timeline: Code task name ---

#[test]
fn test_phase_timeline_code_with_task_name() {
    let mut state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    state["code_task"] = json!(1);
    state["code_tasks_total"] = json!(3);
    state["code_task_name"] = json!("Update contract tests");

    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(
        timeline[2].annotation,
        "Update contract tests - task 2 of 3"
    );
}

#[test]
fn test_phase_timeline_code_task_name_absent() {
    let mut state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    state["code_task"] = json!(1);
    state["code_tasks_total"] = json!(3);

    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[2].annotation, "task 2 of 3");
}

#[test]
fn test_phase_timeline_code_task_name_with_diff_stats() {
    let mut state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    state["code_task"] = json!(1);
    state["code_tasks_total"] = json!(3);
    state["code_task_name"] = json!("Update contract tests");
    state["diff_stats"] = json!({"insertions": 127, "deletions": 48});

    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(
        timeline[2].annotation,
        "Update contract tests - task 2 of 3, +127 -48"
    );
}

#[test]
fn test_phase_timeline_code_task_name_truncated() {
    let mut state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    state["code_task"] = json!(0);
    state["code_tasks_total"] = json!(3);
    state["code_task_name"] = json!("Implement the very long task description that exceeds limit");

    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    let name_part = timeline[2].annotation.split(" - task ").next().unwrap();
    assert_eq!(name_part.chars().count(), 30);
    assert!(name_part.ends_with("..."));
}

#[test]
fn test_phase_timeline_code_task_name_empty_string() {
    let mut state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    state["code_task"] = json!(1);
    state["code_tasks_total"] = json!(3);
    state["code_task_name"] = json!("");

    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[2].annotation, "task 2 of 3");
}

// --- phase_timeline: Code Review ---

#[test]
fn test_phase_timeline_code_review_step_zero() {
    let state = make_state(
        "flow-code-review",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "complete"),
            ("flow-code-review", "in_progress"),
        ],
    );
    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[3].annotation, "simplifying - step 1 of 4");
}

#[test]
fn test_phase_timeline_code_review_annotation() {
    let mut state = make_state(
        "flow-code-review",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "complete"),
            ("flow-code-review", "in_progress"),
        ],
    );
    state["code_review_step"] = json!(2);
    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[3].annotation, "security review - step 3 of 4");
}

#[test]
fn test_phase_timeline_code_review_complete() {
    let mut state = make_state(
        "flow-code-review",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "complete"),
            ("flow-code-review", "in_progress"),
        ],
    );
    state["code_review_step"] = json!(4);
    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[3].annotation, "");
}

#[test]
fn test_phase_timeline_code_review_step_four() {
    let mut state = make_state(
        "flow-code-review",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "complete"),
            ("flow-code-review", "in_progress"),
        ],
    );
    state["code_review_step"] = json!(3);
    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[3].annotation, "agent reviews - step 4 of 4");
}

// --- phase_timeline: step name fallback ---

#[test]
fn test_phase_timeline_unknown_step_falls_back() {
    let mut state = make_state("flow-start", &[("flow-start", "in_progress")]);
    state["start_step"] = json!(99);
    state["start_steps_total"] = json!(5);

    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[0].annotation, "step 99 of 5");
}

// --- phase_timeline: Learn ---

#[test]
fn test_phase_timeline_learn_annotation() {
    let mut state = make_state(
        "flow-learn",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "complete"),
            ("flow-code-review", "complete"),
            ("flow-learn", "in_progress"),
        ],
    );
    state["learn_step"] = json!(4);
    state["learn_steps_total"] = json!(7);
    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[4].annotation, "committing - step 5 of 7");
}

#[test]
fn test_phase_timeline_learn_step_zero() {
    let mut state = make_state(
        "flow-learn",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "complete"),
            ("flow-code-review", "complete"),
            ("flow-learn", "in_progress"),
        ],
    );
    state["learn_steps_total"] = json!(7);
    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[4].annotation, "gathering sources - step 1 of 7");
}

// --- phase_timeline: Complete ---

#[test]
fn test_phase_timeline_complete_annotation() {
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
    state["complete_step"] = json!(5);
    state["complete_steps_total"] = json!(6);
    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[5].annotation, "merging PR - step 5 of 6");
}

#[test]
fn test_phase_timeline_complete_step_zero() {
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
    state["complete_steps_total"] = json!(6);
    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[5].annotation, "");
}

#[test]
fn test_phase_timeline_complete_step_one() {
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
    state["complete_step"] = json!(1);
    state["complete_steps_total"] = json!(6);
    let timeline = phase_timeline(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(timeline[5].annotation, "running checks - step 1 of 6");
}

// --- phase_timeline: live elapsed for in-progress ---

#[test]
fn test_phase_timeline_in_progress_live_time() {
    let now = pacific("2026-01-01T00:05:00-08:00");
    let mut state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    state["phases"]["flow-code"]["session_started_at"] = json!("2026-01-01T00:00:00-08:00");

    let timeline = phase_timeline(&state, Some(now));
    let code_entry = timeline.iter().find(|e| e.key == "flow-code").unwrap();
    assert_eq!(code_entry.time, "5m");
}

#[test]
fn test_phase_timeline_in_progress_cumulative_plus_live() {
    let now = pacific("2026-01-01T00:03:00-08:00");
    let mut state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    state["phases"]["flow-code"]["session_started_at"] = json!("2026-01-01T00:00:00-08:00");
    state["phases"]["flow-code"]["cumulative_seconds"] = json!(120);

    let timeline = phase_timeline(&state, Some(now));
    let code_entry = timeline.iter().find(|e| e.key == "flow-code").unwrap();
    assert_eq!(code_entry.time, "5m");
}

#[test]
fn test_phase_timeline_in_progress_no_session_started() {
    let now = pacific("2026-01-01T00:05:00-08:00");
    let mut state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    state["phases"]["flow-code"]["session_started_at"] = json!(null);
    state["phases"]["flow-code"]["cumulative_seconds"] = json!(60);

    let timeline = phase_timeline(&state, Some(now));
    let code_entry = timeline.iter().find(|e| e.key == "flow-code").unwrap();
    assert_eq!(code_entry.time, "1m");
}

// --- parse_log_entries ---

#[test]
fn test_parse_log_entries_basic() {
    let log = "2026-01-01T10:15:00-08:00 [Phase 1] git worktree add (exit 0)\n\
               2026-01-01T10:20:00-08:00 [Phase 2] Plan approved\n";
    let entries = parse_log_entries(log, 20);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].time, "10:15");
    assert_eq!(entries[0].message, "[Phase 1] git worktree add (exit 0)");
    assert_eq!(entries[1].time, "10:20");
}

#[test]
fn test_parse_log_entries_limit() {
    let lines: Vec<String> = (0..30)
        .map(|i| format!("2026-01-01T10:{:02}:00-08:00 entry {}", i, i))
        .collect();
    let log = lines.join("\n");
    let entries = parse_log_entries(&log, 5);
    assert_eq!(entries.len(), 5);
    assert_eq!(entries[0].message, "entry 25");
    assert_eq!(entries[4].message, "entry 29");
}

#[test]
fn test_parse_log_entries_empty() {
    let entries = parse_log_entries("", 20);
    assert_eq!(entries.len(), 0);
}

#[test]
fn test_parse_log_entries_malformed_lines() {
    let log = "2026-01-01T10:15:00-08:00 valid entry\n\
               this line has no timestamp\n\
               2026-01-01T10:20:00-08:00 another valid entry\n";
    let entries = parse_log_entries(log, 20);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].message, "valid entry");
    assert_eq!(entries[1].message, "another valid entry");
}

#[test]
fn test_parse_log_entries_blank_lines() {
    let log = "2026-01-01T10:15:00-08:00 first entry\n\n\
               2026-01-01T10:20:00-08:00 second entry\n";
    let entries = parse_log_entries(log, 20);
    assert_eq!(entries.len(), 2);
}

#[test]
fn test_parse_log_entries_invalid_timestamp() {
    let log = "9999-99-99T99:99:99-08:00 bad timestamp\n";
    let entries = parse_log_entries(log, 20);
    assert_eq!(entries.len(), 0);
}

// --- flow_summary ---

#[test]
fn test_flow_summary_basic() {
    let now = pacific("2026-01-01T01:00:00-08:00");
    let state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    let summary = flow_summary(&state, Some(now));
    assert_eq!(summary.feature, "Test Feature");
    assert_eq!(summary.branch, "test-feature");
    assert_eq!(summary.worktree, ".worktrees/test-feature");
    assert_eq!(summary.pr_number, Some(1));
    assert_eq!(
        summary.pr_url.as_deref(),
        Some("https://github.com/test/test/pull/1")
    );
    assert_eq!(summary.phase_number, 3);
    assert_eq!(summary.phase_name, "Code");
}

#[test]
fn test_flow_summary_elapsed_time() {
    let now = pacific("2026-01-01T00:42:00-08:00");
    let mut state = make_state("flow-start", &[]);
    state["started_at"] = json!("2026-01-01T00:00:00-08:00");
    let summary = flow_summary(&state, Some(now));
    assert_eq!(summary.elapsed, "42m");
}

#[test]
fn test_flow_summary_code_task_present() {
    let mut state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    state["code_task"] = json!(3);
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(summary.code_task, 3);
}

#[test]
fn test_flow_summary_code_task_absent() {
    let state = make_state("flow-start", &[]);
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(summary.code_task, 0);
}

#[test]
fn test_flow_summary_diff_stats_present() {
    let mut state = make_state("flow-start", &[]);
    state["diff_stats"] = json!({"files_changed": 5, "insertions": 100, "deletions": 20});
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert!(summary.diff_stats.is_some());
}

#[test]
fn test_flow_summary_diff_stats_absent() {
    let state = make_state("flow-start", &[]);
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert!(summary.diff_stats.is_none());
}

#[test]
fn test_flow_summary_notes_count() {
    let mut state = make_state("flow-start", &[]);
    state["notes"] = json!([{"text": "note1"}, {"text": "note2"}]);
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(summary.notes_count, 2);
}

#[test]
fn test_flow_summary_issues_count() {
    let mut state = make_state("flow-start", &[]);
    state["issues_filed"] = json!([{"url": "http://example.com/1"}]);
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(summary.issues_count, 1);
}

#[test]
fn test_flow_summary_no_notes_or_issues() {
    let state = make_state("flow-start", &[]);
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(summary.notes_count, 0);
    assert_eq!(summary.issues_count, 0);
}

#[test]
fn test_flow_summary_issues_populated() {
    let mut state = make_state("flow-start", &[]);
    state["issues_filed"] = json!([
        {
            "label": "Tech Debt",
            "title": "Extract helper for date parsing",
            "url": "https://github.com/test/test/issues/42",
            "phase": "flow-code-review",
            "phase_name": "Code Review",
        },
        {
            "label": "Flaky Test",
            "title": "test_timeout flakes on CI",
            "url": "https://github.com/test/test/issues/55",
            "phase": "flow-code",
            "phase_name": "Code",
        },
    ]);
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(summary.issues.len(), 2);
    assert_eq!(summary.issues[0].label, "Tech Debt");
    assert_eq!(summary.issues[0].title, "Extract helper for date parsing");
    assert_eq!(
        summary.issues[0].url,
        "https://github.com/test/test/issues/42"
    );
    assert_eq!(summary.issues[0].ref_str, "#42");
    assert_eq!(summary.issues[0].phase_name, "Code Review");
    assert_eq!(summary.issues[1].ref_str, "#55");
}

#[test]
fn test_flow_summary_issues_empty() {
    let mut state = make_state("flow-start", &[]);
    state["issues_filed"] = json!([]);
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert!(summary.issues.is_empty());
}

#[test]
fn test_flow_summary_issues_url_fallback() {
    let mut state = make_state("flow-start", &[]);
    state["issues_filed"] = json!([{
        "label": "Flow",
        "title": "Process gap",
        "url": "https://example.com/custom/path",
        "phase": "flow-learn",
        "phase_name": "Learn",
    }]);
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(summary.issues[0].ref_str, "https://example.com/custom/path");
}

#[test]
fn test_flow_summary_blocked_true() {
    let mut state = make_state("flow-code", &[("flow-code", "in_progress")]);
    state["_blocked"] = json!("2026-01-01T10:00:00-08:00");
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert!(summary.blocked);
}

#[test]
fn test_flow_summary_blocked_false() {
    let state = make_state("flow-code", &[("flow-code", "in_progress")]);
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert!(!summary.blocked);
}

#[test]
fn test_flow_summary_blocked_empty_string() {
    let mut state = make_state("flow-code", &[("flow-code", "in_progress")]);
    state["_blocked"] = json!("");
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert!(!summary.blocked);
}

#[test]
fn test_flow_summary_issue_numbers() {
    let mut state = make_state("flow-start", &[]);
    state["prompt"] = json!("work on #83 and #89");
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert!(summary.issue_numbers.contains(&83));
    assert!(summary.issue_numbers.contains(&89));
}

#[test]
fn test_flow_summary_plan_path_from_files() {
    let mut state = make_state("flow-start", &[]);
    state["files"]["plan"] = json!(".flow-states/test-feature-plan.md");
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(
        summary.plan_path.as_deref(),
        Some(".flow-states/test-feature-plan.md")
    );
}

#[test]
fn test_flow_summary_plan_path_fallback_plan_file() {
    let mut state = make_state("flow-start", &[]);
    state["files"]["plan"] = json!(null);
    state["plan_file"] = json!(".flow-states/test-feature-plan.md");
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(
        summary.plan_path.as_deref(),
        Some(".flow-states/test-feature-plan.md")
    );
}

#[test]
fn test_flow_summary_plan_path_absent() {
    let state = make_state("flow-start", &[]);
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert!(summary.plan_path.is_none());
}

#[test]
fn test_flow_summary_phase_elapsed() {
    let now = pacific("2026-01-01T00:05:00-08:00");
    let mut state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    state["phases"]["flow-code"]["session_started_at"] = json!("2026-01-01T00:00:00-08:00");
    let summary = flow_summary(&state, Some(now));
    assert_eq!(summary.phase_elapsed, "5m");
}

#[test]
fn test_flow_summary_phase_elapsed_no_in_progress() {
    let now = pacific("2026-01-01T01:00:00-08:00");
    let state = make_state(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "pending")],
    );
    let summary = flow_summary(&state, Some(now));
    assert_eq!(summary.phase_elapsed, "");
}

#[test]
fn test_flow_summary_annotation_code_phase() {
    let mut state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    state["code_task"] = json!(2);
    state["code_tasks_total"] = json!(5);
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(summary.annotation, "task 3 of 5");
}

#[test]
fn test_flow_summary_annotation_no_step_set() {
    let state = make_state("flow-start", &[("flow-start", "in_progress")]);
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(summary.annotation, "");
}

#[test]
fn test_flow_summary_annotation_start_phase() {
    let mut state = make_state("flow-start", &[("flow-start", "in_progress")]);
    state["start_step"] = json!(5);
    state["start_steps_total"] = json!(5);
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert_eq!(summary.annotation, "finalizing - step 5 of 5");
}

// --- load_all_flows ---

#[test]
fn test_load_all_flows_empty() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".flow-states")).unwrap();
    let result = load_all_flows(dir.path());
    assert!(result.is_empty());
}

#[test]
fn test_load_all_flows_single() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    std::fs::create_dir(&state_dir).unwrap();
    let state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    std::fs::write(
        state_dir.join("test-feature.json"),
        serde_json::to_string(&state).unwrap(),
    )
    .unwrap();
    let result = load_all_flows(dir.path());
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].branch, "test-feature");
}

#[test]
fn test_load_all_flows_multiple() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    std::fs::create_dir(&state_dir).unwrap();
    for name in ["charlie-feature", "alpha-feature", "bravo-feature"] {
        let mut state = make_state("flow-start", &[]);
        state["branch"] = json!(name);
        std::fs::write(
            state_dir.join(format!("{}.json", name)),
            serde_json::to_string(&state).unwrap(),
        )
        .unwrap();
    }
    let result = load_all_flows(dir.path());
    assert_eq!(result.len(), 3);
    let names: Vec<&str> = result.iter().map(|f| f.branch.as_str()).collect();
    assert_eq!(
        names,
        vec!["alpha-feature", "bravo-feature", "charlie-feature"]
    );
}

#[test]
fn test_load_all_flows_skips_corrupt_json() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    std::fs::create_dir(&state_dir).unwrap();
    let state = make_state("flow-start", &[]);
    std::fs::write(
        state_dir.join("good-feature.json"),
        serde_json::to_string(&state).unwrap(),
    )
    .unwrap();
    std::fs::write(state_dir.join("bad-feature.json"), "{invalid json").unwrap();
    let result = load_all_flows(dir.path());
    assert_eq!(result.len(), 1);
}

#[test]
fn test_load_all_flows_skips_phases_json() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    std::fs::create_dir(&state_dir).unwrap();
    let mut state = make_state("flow-start", &[]);
    state["branch"] = json!("my-feature");
    std::fs::write(
        state_dir.join("my-feature.json"),
        serde_json::to_string(&state).unwrap(),
    )
    .unwrap();
    std::fs::write(state_dir.join("my-feature-phases.json"), r#"{"order": []}"#).unwrap();
    let result = load_all_flows(dir.path());
    assert_eq!(result.len(), 1);
}

#[test]
fn test_load_all_flows_no_state_dir() {
    let dir = tempfile::tempdir().unwrap();
    let result = load_all_flows(dir.path());
    assert!(result.is_empty());
}

#[test]
fn test_load_all_flows_skips_json_without_branch() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    std::fs::create_dir(&state_dir).unwrap();
    std::fs::write(state_dir.join("no-branch.json"), r#"{"some": "data"}"#).unwrap();
    let state = make_state("flow-start", &[]);
    std::fs::write(
        state_dir.join("real-feature.json"),
        serde_json::to_string(&state).unwrap(),
    )
    .unwrap();
    let result = load_all_flows(dir.path());
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].branch, "test-feature");
}

// --- load_orchestration ---

#[test]
fn test_load_orchestration_no_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".flow-states")).unwrap();
    assert!(load_orchestration(dir.path()).is_none());
}

#[test]
fn test_load_orchestration_with_state() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    std::fs::create_dir(&state_dir).unwrap();
    let orch = json!({
        "started_at": "2026-03-20T22:00:00-07:00",
        "completed_at": null,
        "queue": [{"issue_number": 42, "title": "Add PDF export", "status": "pending"}],
    });
    std::fs::write(
        state_dir.join("orchestrate.json"),
        serde_json::to_string(&orch).unwrap(),
    )
    .unwrap();
    let result = load_orchestration(dir.path());
    assert!(result.is_some());
    let r = result.unwrap();
    assert_eq!(
        r.get("started_at").unwrap().as_str().unwrap(),
        "2026-03-20T22:00:00-07:00"
    );
}

#[test]
fn test_load_orchestration_corrupt_json() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    std::fs::create_dir(&state_dir).unwrap();
    std::fs::write(state_dir.join("orchestrate.json"), "{corrupt json").unwrap();
    assert!(load_orchestration(dir.path()).is_none());
}

#[test]
fn test_load_orchestration_no_state_dir() {
    let dir = tempfile::tempdir().unwrap();
    assert!(load_orchestration(dir.path()).is_none());
}

// --- orchestration_summary ---

#[test]
fn test_orchestration_summary_no_state() {
    assert!(orchestration_summary(None, None).is_none());
}

#[test]
fn test_orchestration_summary_basic() {
    let now = pacific("2026-03-21T00:00:00-07:00");
    let orch = json!({
        "started_at": "2026-03-20T22:00:00-07:00",
        "completed_at": null,
        "queue": [
            {
                "issue_number": 42, "title": "Add PDF export",
                "status": "completed", "outcome": "completed",
                "started_at": "2026-03-20T22:00:00-07:00",
                "completed_at": "2026-03-20T23:24:00-07:00",
                "pr_url": "https://github.com/test/test/pull/58",
            },
            {
                "issue_number": 43, "title": "Fix login timeout",
                "status": "pending", "outcome": null,
                "started_at": null, "completed_at": null,
            },
        ],
    });
    let summary = orchestration_summary(Some(&orch), Some(now)).unwrap();
    assert_eq!(summary.total, 2);
    assert_eq!(summary.completed_count, 1);
    assert_eq!(summary.failed_count, 0);
    assert!(summary.is_running);
    assert_eq!(summary.items[0].icon, "\u{2713}");
    assert_eq!(summary.items[0].issue_number, Some(42));
    assert_eq!(summary.items[1].icon, "\u{00b7}");
}

#[test]
fn test_orchestration_summary_with_completed_and_failed() {
    let now = pacific("2026-03-21T02:00:00-07:00");
    let orch = json!({
        "started_at": "2026-03-20T22:00:00-07:00",
        "completed_at": null,
        "queue": [
            {"issue_number": 42, "title": "A", "status": "completed", "outcome": "completed",
             "started_at": "2026-03-20T22:00:00-07:00", "completed_at": "2026-03-20T23:00:00-07:00"},
            {"issue_number": 43, "title": "B", "status": "failed", "outcome": "failed",
             "started_at": "2026-03-20T23:00:00-07:00", "completed_at": "2026-03-21T00:00:00-07:00",
             "reason": "CI failed after 3 attempts"},
            {"issue_number": 44, "title": "C", "status": "pending", "outcome": null},
        ],
    });
    let summary = orchestration_summary(Some(&orch), Some(now)).unwrap();
    assert_eq!(summary.completed_count, 1);
    assert_eq!(summary.failed_count, 1);
    assert_eq!(summary.total, 3);
    assert_eq!(summary.items[1].icon, "\u{2717}");
    assert_eq!(
        summary.items[1].reason.as_deref(),
        Some("CI failed after 3 attempts")
    );
}

#[test]
fn test_orchestration_summary_in_progress_elapsed() {
    let now = pacific("2026-03-21T00:38:00-07:00");
    let orch = json!({
        "started_at": "2026-03-20T22:00:00-07:00",
        "completed_at": null,
        "queue": [
            {"issue_number": 45, "title": "Update hooks",
             "status": "in_progress",
             "started_at": "2026-03-21T00:00:00-07:00"},
        ],
    });
    let summary = orchestration_summary(Some(&orch), Some(now)).unwrap();
    assert_eq!(summary.items[0].icon, "\u{25b6}");
    assert_eq!(summary.items[0].elapsed, "38m");
}

#[test]
fn test_orchestration_summary_no_queue() {
    let now = pacific("2026-03-21T00:00:00-07:00");
    let orch = json!({
        "started_at": "2026-03-20T22:00:00-07:00",
        "completed_at": null,
        "queue": [],
    });
    let summary = orchestration_summary(Some(&orch), Some(now)).unwrap();
    assert_eq!(summary.total, 0);
    assert!(summary.items.is_empty());
    assert!(summary.is_running);
}

#[test]
fn test_orchestration_summary_not_running() {
    let now = pacific("2026-03-21T06:00:00-07:00");
    let orch = json!({
        "started_at": "2026-03-20T22:00:00-07:00",
        "completed_at": "2026-03-20T23:00:00-07:00",
        "queue": [
            {"issue_number": 42, "title": "Done", "status": "completed", "outcome": "completed",
             "started_at": "2026-03-20T22:00:00-07:00", "completed_at": "2026-03-20T23:00:00-07:00"},
        ],
    });
    let summary = orchestration_summary(Some(&orch), Some(now)).unwrap();
    assert!(!summary.is_running);
    assert_eq!(summary.elapsed, "1h 0m");
}

#[test]
fn test_queue_item_display_icons() {
    let now = pacific("2026-03-21T00:00:00-07:00");
    let orch = json!({
        "started_at": "2026-03-20T22:00:00-07:00",
        "completed_at": null,
        "queue": [
            {"issue_number": 1, "title": "A", "status": "completed", "outcome": "completed",
             "started_at": "2026-03-20T22:00:00-07:00", "completed_at": "2026-03-20T23:00:00-07:00"},
            {"issue_number": 2, "title": "B", "status": "failed", "outcome": "failed",
             "started_at": "2026-03-20T22:00:00-07:00", "completed_at": "2026-03-20T23:00:00-07:00"},
            {"issue_number": 3, "title": "C", "status": "in_progress",
             "started_at": "2026-03-20T23:00:00-07:00"},
            {"issue_number": 4, "title": "D", "status": "pending"},
        ],
    });
    let summary = orchestration_summary(Some(&orch), Some(now)).unwrap();
    assert_eq!(summary.items[0].icon, "\u{2713}");
    assert_eq!(summary.items[1].icon, "\u{2717}");
    assert_eq!(summary.items[2].icon, "\u{25b6}");
    assert_eq!(summary.items[3].icon, "\u{00b7}");
}

// --- load_account_metrics ---

#[test]
fn test_load_account_metrics_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    let repo_root = dir.path().join("repo");
    std::fs::create_dir(&repo_root).unwrap();

    let year_month = chrono::Local::now().format("%Y-%m").to_string();
    let cost_dir = repo_root.join(".claude").join("cost").join(&year_month);
    std::fs::create_dir_all(&cost_dir).unwrap();
    std::fs::write(cost_dir.join("session-a"), "1.50").unwrap();
    std::fs::write(cost_dir.join("session-b"), "2.75").unwrap();

    let home_dir = dir.path().join("home");
    let claude_dir = home_dir.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(
        claude_dir.join("rate-limits.json"),
        r#"{"five_hour_pct": 45, "seven_day_pct": 32}"#,
    )
    .unwrap();

    let result = load_account_metrics(&repo_root, Some(&home_dir));
    assert_eq!(result.cost_monthly, "4.25");
    assert_eq!(result.rl_5h, Some(45));
    assert_eq!(result.rl_7d, Some(32));
    assert!(!result.stale);
}

#[test]
fn test_load_account_metrics_no_cost_directory() {
    let dir = tempfile::tempdir().unwrap();
    let repo_root = dir.path().join("repo");
    std::fs::create_dir(&repo_root).unwrap();

    let home_dir = dir.path().join("home");
    let claude_dir = home_dir.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(
        claude_dir.join("rate-limits.json"),
        r#"{"five_hour_pct": 10, "seven_day_pct": 20}"#,
    )
    .unwrap();

    let result = load_account_metrics(&repo_root, Some(&home_dir));
    assert_eq!(result.cost_monthly, "0.00");
}

#[test]
fn test_load_account_metrics_no_rate_limits_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo_root = dir.path().join("repo");
    std::fs::create_dir(&repo_root).unwrap();

    let home_dir = dir.path().join("home");
    std::fs::create_dir(&home_dir).unwrap();

    let result = load_account_metrics(&repo_root, Some(&home_dir));
    assert!(result.stale);
    assert!(result.rl_5h.is_none());
    assert!(result.rl_7d.is_none());
}

#[test]
fn test_load_account_metrics_stale_rate_limits() {
    let dir = tempfile::tempdir().unwrap();
    let repo_root = dir.path().join("repo");
    std::fs::create_dir(&repo_root).unwrap();

    let home_dir = dir.path().join("home");
    let claude_dir = home_dir.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let rl_path = claude_dir.join("rate-limits.json");
    std::fs::write(&rl_path, r#"{"five_hour_pct": 55, "seven_day_pct": 40}"#).unwrap();
    // Set mtime to 15 minutes ago
    let old_time = std::time::SystemTime::now() - std::time::Duration::from_secs(900);
    filetime::set_file_mtime(&rl_path, filetime::FileTime::from_system_time(old_time)).unwrap();

    let result = load_account_metrics(&repo_root, Some(&home_dir));
    assert!(result.stale);
    assert!(result.rl_5h.is_none());
    assert!(result.rl_7d.is_none());
}

#[test]
fn test_load_account_metrics_malformed_cost_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo_root = dir.path().join("repo");
    std::fs::create_dir(&repo_root).unwrap();

    let year_month = chrono::Local::now().format("%Y-%m").to_string();
    let cost_dir = repo_root.join(".claude").join("cost").join(&year_month);
    std::fs::create_dir_all(&cost_dir).unwrap();
    std::fs::write(cost_dir.join("good-session"), "3.00").unwrap();
    std::fs::write(cost_dir.join("bad-session"), "not-a-number").unwrap();

    let home_dir = dir.path().join("home");
    std::fs::create_dir(&home_dir).unwrap();

    let result = load_account_metrics(&repo_root, Some(&home_dir));
    assert_eq!(result.cost_monthly, "3.00");
}

#[test]
fn test_load_account_metrics_malformed_rate_limits() {
    let dir = tempfile::tempdir().unwrap();
    let repo_root = dir.path().join("repo");
    std::fs::create_dir(&repo_root).unwrap();

    let home_dir = dir.path().join("home");
    let claude_dir = home_dir.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("rate-limits.json"), "{invalid json").unwrap();

    let result = load_account_metrics(&repo_root, Some(&home_dir));
    assert!(result.stale);
    assert!(result.rl_5h.is_none());
    assert!(result.rl_7d.is_none());
}

#[test]
fn test_load_all_flows_sorted_by_phase_then_feature() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    std::fs::create_dir(&state_dir).unwrap();

    // Flow in Code phase (phase 3) — branch "alpha" sorts first alphabetically
    let mut code_state = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    code_state["branch"] = json!("alpha-feature");
    std::fs::write(
        state_dir.join("alpha-feature.json"),
        serde_json::to_string(&code_state).unwrap(),
    )
    .unwrap();

    // Flow in Start phase (phase 1) — branch "beta" sorts second alphabetically
    let mut start_state = make_state("flow-start", &[("flow-start", "in_progress")]);
    start_state["branch"] = json!("beta-feature");
    std::fs::write(
        state_dir.join("beta-feature.json"),
        serde_json::to_string(&start_state).unwrap(),
    )
    .unwrap();

    // Flow in Plan phase (phase 2)
    let mut plan_state = make_state(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    plan_state["branch"] = json!("gamma-feature");
    std::fs::write(
        state_dir.join("gamma-feature.json"),
        serde_json::to_string(&plan_state).unwrap(),
    )
    .unwrap();

    // Second flow in Start phase (phase 1) — tiebreaker: "delta" > "beta"
    let mut start_state2 = make_state("flow-start", &[("flow-start", "in_progress")]);
    start_state2["branch"] = json!("delta-feature");
    std::fs::write(
        state_dir.join("delta-feature.json"),
        serde_json::to_string(&start_state2).unwrap(),
    )
    .unwrap();

    let flows = load_all_flows(dir.path());

    assert_eq!(flows.len(), 4);
    assert_eq!(flows[0].branch, "beta-feature");
    assert_eq!(flows[0].phase_number, 1);
    assert_eq!(flows[1].branch, "delta-feature");
    assert_eq!(flows[1].phase_number, 1);
    assert_eq!(flows[2].branch, "gamma-feature");
    assert_eq!(flows[2].phase_number, 2);
    assert_eq!(flows[3].branch, "alpha-feature");
    assert_eq!(flows[3].phase_number, 3);
}

#[test]
fn test_load_all_flows_unknown_phase_sorts_last() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join(".flow-states");
    std::fs::create_dir(&state_dir).unwrap();

    let mut start_state = make_state("flow-start", &[("flow-start", "in_progress")]);
    start_state["branch"] = json!("known-feature");
    std::fs::write(
        state_dir.join("known-feature.json"),
        serde_json::to_string(&start_state).unwrap(),
    )
    .unwrap();

    let mut unknown_state = make_state("flow-nonexistent", &[]);
    unknown_state["branch"] = json!("unknown-feature");
    std::fs::write(
        state_dir.join("unknown-feature.json"),
        serde_json::to_string(&unknown_state).unwrap(),
    )
    .unwrap();

    let flows = load_all_flows(dir.path());

    assert_eq!(flows.len(), 2);
    assert_eq!(flows[0].branch, "known-feature");
    assert_eq!(flows[0].phase_number, 1);
    assert_eq!(flows[1].branch, "unknown-feature");
    assert_eq!(flows[1].phase_number, usize::MAX);
}

#[test]
fn test_load_account_metrics_null_rate_limit_values() {
    let dir = tempfile::tempdir().unwrap();
    let repo_root = dir.path().join("repo");
    std::fs::create_dir(&repo_root).unwrap();

    let home_dir = dir.path().join("home");
    let claude_dir = home_dir.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(
        claude_dir.join("rate-limits.json"),
        r#"{"five_hour_pct": null, "seven_day_pct": null}"#,
    )
    .unwrap();

    let result = load_account_metrics(&repo_root, Some(&home_dir));
    assert!(result.stale);
    assert!(result.rl_5h.is_none());
    assert!(result.rl_7d.is_none());
}

// --- Coverage gap closures ---

#[test]
fn test_phase_timeline_now_none_uses_real_clock() {
    // Covers the now-fallback closure when callers pass None.
    let state = serde_json::json!({"phases": {}});
    let result = phase_timeline(&state, None);
    assert!(result.is_empty());
}

#[test]
fn test_phase_order_keys_all_present_in_phase_names() {
    // Contract test for the invariant phase_timeline's `.expect()`
    // relies on: every PHASE_ORDER key must resolve to an entry in
    // phase_names(). A violation would panic inside the TUI refresh
    // loop, so this locks it mechanically.
    let names = phase_config::phase_names();
    for &key in PHASE_ORDER {
        assert!(
            names.contains_key(key),
            "PHASE_ORDER key '{}' missing from phase_names()",
            key
        );
    }
}

#[test]
fn test_load_account_metrics_none_home_override_falls_back_to_env() {
    // Covers the None => env::var("HOME") arm.
    let dir = tempfile::tempdir().unwrap();
    let repo_root = dir.path();
    let result = load_account_metrics(repo_root, None);
    assert_eq!(result.cost_monthly, "0.00");
}

// --- _blocked field variants ---

#[test]
fn test_flow_summary_blocked_null_value() {
    let state = serde_json::json!({
        "branch": "test",
        "_blocked": serde_json::Value::Null,
        "phases": {},
    });
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert!(!summary.blocked);
}

#[test]
fn test_flow_summary_blocked_bool_true() {
    let state = serde_json::json!({
        "branch": "test",
        "_blocked": true,
        "phases": {},
    });
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert!(summary.blocked);
}

#[test]
fn test_flow_summary_blocked_bool_false() {
    let state = serde_json::json!({
        "branch": "test",
        "_blocked": false,
        "phases": {},
    });
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert!(!summary.blocked);
}

#[test]
fn test_flow_summary_blocked_compound_value() {
    let state = serde_json::json!({
        "branch": "test",
        "_blocked": {"reason": "ci_failed"},
        "phases": {},
    });
    let summary = flow_summary(&state, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert!(summary.blocked);
}

// --- load_all_flows directory failure ---

#[cfg(unix)]
#[test]
fn test_load_all_flows_unreadable_state_dir_returns_empty() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let state_dir = tmp.path().join(".flow-states");
    std::fs::create_dir(&state_dir).unwrap();

    let mut perms = std::fs::metadata(&state_dir).unwrap().permissions();
    perms.set_mode(0o000);
    std::fs::set_permissions(&state_dir, perms).unwrap();

    let result = load_all_flows(tmp.path());

    let mut perms = std::fs::metadata(&state_dir).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&state_dir, perms).unwrap();

    assert!(result.is_empty());
}

#[test]
fn test_load_all_flows_skips_non_json_files() {
    let tmp = tempfile::tempdir().unwrap();
    let state_dir = tmp.path().join(".flow-states");
    std::fs::create_dir(&state_dir).unwrap();
    std::fs::write(state_dir.join("ignore-me.txt"), "not json").unwrap();
    std::fs::write(state_dir.join("noise.log"), "log entry").unwrap();
    let result = load_all_flows(tmp.path());
    assert!(result.is_empty());
}

#[test]
fn test_orchestration_summary_item_with_invalid_completed_at_uses_empty_elapsed() {
    let now = pacific("2026-01-01T00:00:00-08:00");
    let orch = serde_json::json!({
        "started_at": "2026-01-01T00:00:00-08:00",
        "queue": [
            {
                "issue_number": 1,
                "title": "X",
                "status": "completed",
                "started_at": "2026-01-01T00:00:00-08:00",
                "completed_at": "not-a-real-timestamp",
            }
        ],
    });
    let summary = orchestration_summary(Some(&orch), Some(now)).unwrap();
    assert_eq!(summary.items.len(), 1);
    assert_eq!(summary.items[0].elapsed, "");
}

#[test]
fn test_orchestration_summary_with_invalid_completed_at_falls_back_to_now() {
    let now = pacific("2026-01-01T00:01:00-08:00");
    let orch = serde_json::json!({
        "started_at": "2026-01-01T00:00:00-08:00",
        "completed_at": "not-a-real-timestamp",
        "queue": [],
    });
    let summary = orchestration_summary(Some(&orch), Some(now)).unwrap();
    assert_eq!(summary.elapsed, "1m");
}

#[test]
fn test_orchestration_summary_with_valid_completed_at_uses_parsed_dt() {
    let orch = serde_json::json!({
        "started_at": "2026-01-01T00:00:00-08:00",
        "completed_at": "2026-01-01T00:02:00-08:00",
        "queue": [],
    });
    let now = pacific("2026-01-01T05:00:00-08:00");
    let summary = orchestration_summary(Some(&orch), Some(now)).unwrap();
    assert_eq!(summary.elapsed, "2m");
    assert!(!summary.is_running);
}

#[cfg(unix)]
#[test]
fn test_load_account_metrics_with_directory_in_cost_dir_skips_via_read_err() {
    let dir = tempfile::tempdir().unwrap();
    let repo_root = dir.path();
    let now = chrono::Local::now();
    let year_month = now.format("%Y-%m").to_string();
    let cost_dir = repo_root.join(".claude").join("cost").join(&year_month);
    std::fs::create_dir_all(&cost_dir).unwrap();
    std::fs::write(cost_dir.join("session1"), "1.50").unwrap();
    std::fs::create_dir(cost_dir.join("subdir")).unwrap();

    let home_dir = dir.path().join("home");
    std::fs::create_dir(&home_dir).unwrap();
    let result = load_account_metrics(repo_root, Some(&home_dir));
    assert_eq!(result.cost_monthly, "1.50");
}

#[cfg(unix)]
#[test]
fn test_load_account_metrics_with_unreadable_cost_dir_skips_via_read_dir_err() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let repo_root = dir.path();
    let now = chrono::Local::now();
    let year_month = now.format("%Y-%m").to_string();
    let cost_dir = repo_root.join(".claude").join("cost").join(&year_month);
    std::fs::create_dir_all(&cost_dir).unwrap();
    std::fs::write(cost_dir.join("session1"), "1.50").unwrap();
    let mut perms = std::fs::metadata(&cost_dir).unwrap().permissions();
    perms.set_mode(0o000);
    std::fs::set_permissions(&cost_dir, perms).unwrap();

    let home_dir = dir.path().join("home");
    std::fs::create_dir(&home_dir).unwrap();
    let result = load_account_metrics(repo_root, Some(&home_dir));

    let mut perms = std::fs::metadata(&cost_dir).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&cost_dir, perms).unwrap();

    assert_eq!(result.cost_monthly, "0.00");
}

#[test]
fn test_load_all_flows_with_directory_named_json_skips_via_read_err() {
    let tmp = tempfile::tempdir().unwrap();
    let state_dir = tmp.path().join(".flow-states");
    std::fs::create_dir(&state_dir).unwrap();
    std::fs::create_dir(state_dir.join("dir-with-json-suffix.json")).unwrap();
    let result = load_all_flows(tmp.path());
    assert!(result.is_empty());
}

#[cfg(unix)]
#[test]
fn test_load_account_metrics_with_rate_limits_as_directory_skips_via_read_err() {
    let dir = tempfile::tempdir().unwrap();
    let repo_root = dir.path();
    let home_dir = dir.path().join("home");
    let claude_dir = home_dir.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::create_dir(claude_dir.join("rate-limits.json")).unwrap();

    let result = load_account_metrics(repo_root, Some(&home_dir));
    assert!(result.stale);
    assert!(result.rl_5h.is_none());
    assert!(result.rl_7d.is_none());
}

#[test]
fn test_load_account_metrics_with_future_mtime_treated_as_stale() {
    let dir = tempfile::tempdir().unwrap();
    let repo_root = dir.path();
    let home_dir = dir.path().join("home");
    let claude_dir = home_dir.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let rl_path = claude_dir.join("rate-limits.json");
    std::fs::write(&rl_path, r#"{"five_hour_pct": 50, "seven_day_pct": 30}"#).unwrap();

    let future = filetime::FileTime::from_system_time(
        std::time::SystemTime::now() + std::time::Duration::from_secs(3600),
    );
    filetime::set_file_mtime(&rl_path, future).unwrap();

    let result = load_account_metrics(repo_root, Some(&home_dir));
    assert!(result.stale);
    assert!(result.rl_5h.is_none());
    assert!(result.rl_7d.is_none());
}

// --- phase_timeline early-return when phases is missing ---

#[test]
fn test_phase_timeline_with_no_phases_field_returns_empty() {
    let state_no_phases = serde_json::json!({});
    let result = phase_timeline(&state_no_phases, Some(pacific("2026-01-01T00:00:00-08:00")));
    assert!(result.is_empty());
}

#[test]
fn test_phase_timeline_with_non_object_phases_returns_empty() {
    let state_array_phases = serde_json::json!({"phases": []});
    let result = phase_timeline(
        &state_array_phases,
        Some(pacific("2026-01-01T00:00:00-08:00")),
    );
    assert!(result.is_empty());
}

#[test]
fn test_load_orchestration_with_orchestrate_as_directory_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    let state_dir = tmp.path().join(".flow-states");
    std::fs::create_dir(&state_dir).unwrap();
    std::fs::create_dir(state_dir.join("orchestrate.json")).unwrap();
    let result = load_orchestration(tmp.path());
    assert!(result.is_none());
}

// --- run_impl_main (main.rs TuiData arm driver) ---

#[test]
fn run_impl_main_no_flag_returns_err_exit_1() {
    let dir = tempfile::tempdir().unwrap();
    let (msg, code) = run_impl_main(false, false, false, dir.path())
        .expect_err("no-flag invocation must return Err");
    assert_eq!(code, 1);
    assert!(msg.contains("--load-all-flows"));
    assert!(msg.contains("--load-orchestration"));
    assert!(msg.contains("--load-account-metrics"));
}

#[test]
fn run_impl_main_load_all_flows_returns_array_exit_0() {
    let dir = tempfile::tempdir().unwrap();
    let (value, code) = run_impl_main(true, false, false, dir.path()).expect("ok path");
    assert_eq!(code, 0);
    assert!(value.is_array(), "expected array, got {:?}", value);
}

#[test]
fn run_impl_main_load_orchestration_no_state_returns_null_exit_0() {
    let dir = tempfile::tempdir().unwrap();
    let (value, code) = run_impl_main(false, true, false, dir.path()).expect("ok path");
    assert_eq!(code, 0);
    assert_eq!(value, Value::Null);
}

#[test]
fn run_impl_main_load_orchestration_with_state_returns_state_and_summary() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".flow-states")).unwrap();
    std::fs::write(
        dir.path().join(".flow-states").join("orchestrate.json"),
        serde_json::json!({
            "issue_queue": [],
            "started_at": "2026-04-14T00:00:00-07:00",
            "completed_at": null,
            "status": "running",
        })
        .to_string(),
    )
    .unwrap();
    let (value, code) = run_impl_main(false, true, false, dir.path()).expect("ok path");
    assert_eq!(code, 0);
    assert!(
        value.get("state").is_some(),
        "expected state key: {}",
        value
    );
    assert!(
        value.get("summary").is_some(),
        "expected summary key: {}",
        value
    );
}

#[test]
fn run_impl_main_load_account_metrics_returns_object_exit_0() {
    let dir = tempfile::tempdir().unwrap();
    let (value, code) = run_impl_main(false, false, true, dir.path()).expect("ok path");
    assert_eq!(code, 0);
    assert!(value.is_object(), "expected object, got {:?}", value);
}
