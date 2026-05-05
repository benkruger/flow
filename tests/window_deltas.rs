//! Integration tests for `src/window_deltas.rs` — pure delta math
//! against in-memory `FlowState` / `PhaseState` fixtures. No
//! filesystem, no subprocess.

use indexmap::IndexMap;

use flow_rs::state::{
    FlowState, ModelTokens, Phase, PhaseState, PhaseStatus, StateFiles, StepSnapshot,
    WindowSnapshot,
};
use flow_rs::window_deltas::{by_model_rollup, flow_total, phase_delta};

// --- fixture helpers ---

fn empty_state() -> FlowState {
    FlowState {
        schema_version: 1,
        branch: "test".to_string(),
        relative_cwd: String::new(),
        repo: None,
        pr_number: None,
        pr_url: None,
        started_at: "2026-05-04T10:00:00-07:00".to_string(),
        current_phase: "flow-start".to_string(),
        files: StateFiles {
            plan: None,
            dag: None,
            log: ".flow-states/test/log".to_string(),
            state: ".flow-states/test/state.json".to_string(),
        },
        session_tty: None,
        session_id: None,
        transcript_path: None,
        notes: vec![],
        prompt: None,
        phases: IndexMap::new(),
        phase_transitions: vec![],
        plan_file: None,
        dag_file: None,
        skills: None,
        issues_filed: vec![],
        slack_thread_ts: None,
        slack_notifications: vec![],
        start_step: None,
        start_steps_total: None,
        plan_step: None,
        plan_steps_total: None,
        code_task: None,
        code_tasks_total: None,
        code_task_name: None,
        code_review_step: None,
        learn_step: None,
        learn_steps_total: None,
        complete_step: None,
        complete_steps_total: None,
        auto_continue: None,
        continue_pending: None,
        continue_context: None,
        blocked: None,
        last_failure: None,
        compact_summary: None,
        compact_cwd: None,
        compact_count: None,
        window_at_start: None,
        window_at_complete: None,
    }
}

/// Build a snapshot where every numeric field is set from a single
/// `n` value plus a session id. Convenient for tests that vary one
/// dimension while keeping others stable.
fn snap(session: &str, n: i64) -> WindowSnapshot {
    let mut by_model = IndexMap::new();
    by_model.insert(
        "claude-opus-4-7".to_string(),
        ModelTokens {
            input: n,
            output: n,
            cache_create: 0,
            cache_read: 0,
        },
    );
    WindowSnapshot {
        captured_at: format!("2026-05-04T{:02}:00:00-07:00", n.min(23)),
        session_id: Some(session.to_string()),
        model: Some("claude-opus-4-7".to_string()),
        five_hour_pct: Some(n),
        seven_day_pct: Some(n),
        session_input_tokens: Some(n),
        session_output_tokens: Some(n),
        session_cache_creation_tokens: Some(0),
        session_cache_read_tokens: Some(0),
        session_cost_usd: Some(n as f64 * 0.01),
        by_model,
        turn_count: Some(n),
        tool_call_count: Some(n * 2),
        context_at_last_turn_tokens: Some(n * 100),
        context_window_pct: Some((n * 100) as f64 / 200_000.0 * 100.0),
    }
}

fn phase_with_snapshots(
    enter: Option<WindowSnapshot>,
    steps: Vec<(i64, &str, WindowSnapshot)>,
    complete: Option<WindowSnapshot>,
) -> PhaseState {
    PhaseState {
        name: "Test".to_string(),
        status: PhaseStatus::Complete,
        started_at: None,
        completed_at: None,
        session_started_at: None,
        cumulative_seconds: 0,
        visit_count: 0,
        window_at_enter: enter,
        window_at_complete: complete,
        step_snapshots: steps
            .into_iter()
            .map(|(step, field, snapshot)| StepSnapshot {
                step,
                field: field.to_string(),
                snapshot,
            })
            .collect(),
    }
}

// --- DeltaReport derive coverage ---

/// Exercise the `#[derive]`'d Debug, Clone, and PartialEq impls on
/// `DeltaReport`. These trait derives generate covered-code regions
/// in cargo-llvm-cov; without a consumer test the derives appear as
/// missed regions even though every produced report is otherwise
/// exercised.
#[test]
fn delta_report_derives_debug_clone_partial_eq() {
    let phase = phase_with_snapshots(Some(snap("S1", 0)), vec![], Some(snap("S1", 5)));
    let a = phase_delta(&phase).expect("populated");
    let b = a.clone();
    assert_eq!(a, b);
    let dbg = format!("{:?}", a);
    assert!(dbg.contains("input_tokens_delta"));
}

// --- phase_delta ---

/// Single session, enter→complete: simple subtraction yields the
/// expected delta across every counter.
#[test]
fn phase_delta_same_session_subtracts_endpoints() {
    let phase = phase_with_snapshots(Some(snap("S1", 5)), vec![], Some(snap("S1", 12)));
    let report = phase_delta(&phase).expect("populated");
    assert_eq!(report.input_tokens_delta, 7);
    assert_eq!(report.output_tokens_delta, 7);
    assert_eq!(report.turn_count_delta, 7);
    assert_eq!(report.tool_call_count_delta, 14);
    // 12 - 5 = 7 for both pcts; no reset observed.
    assert_eq!(report.five_hour_pct_delta, Some(7));
    assert_eq!(report.seven_day_pct_delta, Some(7));
    assert!(!report.window_reset_observed);
    assert_eq!(
        report.by_model_delta.get("claude-opus-4-7").unwrap().input,
        7
    );
}

/// Multi-session phase: snapshots span two session_ids. Each
/// session's contribution is computed independently and summed.
/// Without grouping the naive `complete - enter` would go negative
/// across the boundary.
#[test]
fn phase_delta_cross_session_groups_then_sums() {
    // S1: enter=5, step=8 → S1 contributes 8-5 = 3
    // S2: step=2, complete=10 → S2 contributes 10-2 = 8
    // Total: 11
    let phase = phase_with_snapshots(
        Some(snap("S1", 5)),
        vec![
            (1, "code_task", snap("S1", 8)),
            (2, "code_task", snap("S2", 2)),
        ],
        Some(snap("S2", 10)),
    );
    let report = phase_delta(&phase).expect("populated");
    assert_eq!(report.input_tokens_delta, 11);
    assert_eq!(report.turn_count_delta, 11);
}

/// Step snapshots between enter and complete contribute through
/// the cross-session aggregation. With one session, the result is
/// identical to a no-step phase between the same endpoints.
#[test]
fn phase_delta_with_step_snapshots_aggregates_through_steps() {
    let with_steps = phase_with_snapshots(
        Some(snap("S1", 5)),
        vec![
            (1, "code_task", snap("S1", 7)),
            (2, "code_task", snap("S1", 9)),
        ],
        Some(snap("S1", 12)),
    );
    let without_steps = phase_with_snapshots(Some(snap("S1", 5)), vec![], Some(snap("S1", 12)));
    let a = phase_delta(&with_steps).expect("populated");
    let b = phase_delta(&without_steps).expect("populated");
    assert_eq!(a.input_tokens_delta, b.input_tokens_delta);
}

/// When complete.five_hour_pct < enter.five_hour_pct, the rolling
/// window reset between snapshots: pct delta becomes `None` and
/// `window_reset_observed` is set so readers can switch to the
/// absolute current value.
#[test]
fn phase_delta_with_window_reset_marks_observed_and_uses_absolute() {
    let mut enter = snap("S1", 80);
    let mut complete = snap("S1", 5);
    enter.session_input_tokens = Some(100);
    complete.session_input_tokens = Some(200);
    enter.five_hour_pct = Some(80);
    complete.five_hour_pct = Some(5);
    enter.seven_day_pct = Some(50);
    complete.seven_day_pct = Some(50);
    let phase = phase_with_snapshots(Some(enter), vec![], Some(complete));
    let report = phase_delta(&phase).expect("populated");
    assert_eq!(report.five_hour_pct_delta, None);
    assert!(report.window_reset_observed);
    // Other deltas still computed normally.
    assert_eq!(report.input_tokens_delta, 100);
}

/// Phase missing `window_at_enter` cannot anchor a delta — return
/// None so callers can render "no data yet" rather than zero.
#[test]
fn phase_delta_missing_enter_snapshot_returns_none() {
    let phase = phase_with_snapshots(None, vec![], Some(snap("S1", 10)));
    assert!(phase_delta(&phase).is_none());
}

/// Phase missing `window_at_complete` falls back to the latest
/// step snapshot as the endpoint so an in-progress phase can
/// still report what it has done so far.
#[test]
fn phase_delta_missing_complete_uses_latest_step_snapshot() {
    let phase = phase_with_snapshots(
        Some(snap("S1", 0)),
        vec![
            (1, "code_task", snap("S1", 3)),
            (2, "code_task", snap("S1", 7)),
        ],
        None,
    );
    let report = phase_delta(&phase).expect("populated");
    assert_eq!(report.input_tokens_delta, 7);
}

// --- flow_total ---

/// `flow_total` aggregates every phase's snapshots into one report.
/// Two phases each contributing 5 input tokens → total 10.
#[test]
fn flow_total_aggregates_every_phase() {
    let mut state = empty_state();
    state.phases.insert(
        Phase::FlowStart,
        phase_with_snapshots(Some(snap("S1", 0)), vec![], Some(snap("S1", 5))),
    );
    state.phases.insert(
        Phase::FlowCode,
        phase_with_snapshots(Some(snap("S1", 5)), vec![], Some(snap("S1", 10))),
    );
    let report = flow_total(&state);
    // FlowStart: 5-0 = 5; FlowCode: 10-5 = 5; Total = 10
    assert_eq!(report.input_tokens_delta, 10);
}

// --- by_model_rollup ---

/// Rollup walks every phase's snapshots and sums the per-model
/// counters across the entire flow.
#[test]
fn by_model_rollup_sums_across_all_snapshots() {
    let mut state = empty_state();
    state.phases.insert(
        Phase::FlowStart,
        phase_with_snapshots(Some(snap("S1", 0)), vec![], Some(snap("S1", 5))),
    );
    state.phases.insert(
        Phase::FlowCode,
        phase_with_snapshots(Some(snap("S1", 5)), vec![], Some(snap("S1", 12))),
    );
    let rollup = by_model_rollup(&state);
    // 5 (start phase) + 7 (code phase) = 12 input tokens for opus
    assert_eq!(rollup.get("claude-opus-4-7").unwrap().input, 12);
}

/// State with phases that have no snapshots → rollup is empty
/// without panicking.
#[test]
fn by_model_rollup_handles_phases_with_no_snapshots() {
    let mut state = empty_state();
    state
        .phases
        .insert(Phase::FlowStart, phase_with_snapshots(None, vec![], None));
    let rollup = by_model_rollup(&state);
    assert!(rollup.is_empty());
}

// --- additional branch coverage ---

/// `flow_total` walks each phase's step_snapshots[] alongside
/// the enter / complete pair, so progress recorded by mid-phase
/// counter increments contributes to the flow-level total.
#[test]
fn flow_total_walks_each_phase_step_snapshots() {
    let mut state = empty_state();
    state.phases.insert(
        Phase::FlowCode,
        phase_with_snapshots(
            Some(snap("S1", 0)),
            vec![
                (1, "code_task", snap("S1", 3)),
                (2, "code_task", snap("S1", 7)),
            ],
            Some(snap("S1", 12)),
        ),
    );
    let report = flow_total(&state);
    // 12-0 across same session: 12.
    assert_eq!(report.input_tokens_delta, 12);
}

/// `flow_total` includes top-level `window_at_start` and
/// `window_at_complete` in addition to per-phase snapshots.
#[test]
fn flow_total_includes_top_level_start_complete() {
    let mut state = empty_state();
    state.window_at_start = Some(snap("S1", 0));
    state.window_at_complete = Some(snap("S1", 50));
    let report = flow_total(&state);
    assert_eq!(report.input_tokens_delta, 50);
}

/// Empty flow with no snapshots returns an all-zero, no-reset
/// report rather than panicking.
#[test]
fn flow_total_empty_state_returns_zero_report() {
    let state = empty_state();
    let report = flow_total(&state);
    assert_eq!(report.input_tokens_delta, 0);
    assert_eq!(report.cost_delta_usd, 0.0);
    assert_eq!(report.five_hour_pct_delta, Some(0));
    assert!(!report.window_reset_observed);
    assert!(report.by_model_delta.is_empty());
}

/// Reset observed in any folded report is sticky — `flow_total`
/// over phases where one phase observed a reset propagates the
/// reset flag to the total.
#[test]
fn flow_total_sticky_reset_flag_propagates() {
    let mut state = empty_state();
    let mut enter = snap("S1", 80);
    let mut complete = snap("S1", 5);
    enter.five_hour_pct = Some(80);
    complete.five_hour_pct = Some(5);
    state.phases.insert(
        Phase::FlowStart,
        phase_with_snapshots(Some(enter), vec![], Some(complete)),
    );
    let report = flow_total(&state);
    assert_eq!(report.five_hour_pct_delta, None);
    assert!(report.window_reset_observed);
}

/// Cost delta with `start` as `None` and `end` populated treats
/// the start as zero and reports the end's value as the delta.
#[test]
fn phase_delta_cost_with_none_start_uses_end_value() {
    let mut enter = snap("S1", 0);
    let mut complete = snap("S1", 0);
    enter.session_cost_usd = None;
    complete.session_cost_usd = Some(0.42);
    let phase = phase_with_snapshots(Some(enter), vec![], Some(complete));
    let report = phase_delta(&phase).expect("populated");
    assert!((report.cost_delta_usd - 0.42).abs() < 1e-9);
}

/// Cost delta where both endpoints are `None` contributes zero.
#[test]
fn phase_delta_cost_with_both_none_contributes_zero() {
    let mut enter = snap("S1", 0);
    let mut complete = snap("S1", 0);
    enter.session_cost_usd = None;
    complete.session_cost_usd = None;
    let phase = phase_with_snapshots(Some(enter), vec![], Some(complete));
    let report = phase_delta(&phase).expect("populated");
    assert_eq!(report.cost_delta_usd, 0.0);
}

/// pct_delta_with_reset: when `start` is None and `end` is Some,
/// returns Some(0) without marking a reset (no anchor to compare
/// against).
#[test]
fn phase_delta_pct_with_missing_start_contributes_zero_no_reset() {
    let mut enter = snap("S1", 0);
    let mut complete = snap("S1", 0);
    enter.five_hour_pct = None;
    complete.five_hour_pct = Some(50);
    let phase = phase_with_snapshots(Some(enter), vec![], Some(complete));
    let report = phase_delta(&phase).expect("populated");
    assert_eq!(report.five_hour_pct_delta, Some(0));
    assert!(!report.window_reset_observed);
}

/// `by_model_delta` includes new models present only in the end
/// snapshot — start treated as zero baseline.
#[test]
fn phase_delta_by_model_new_in_end_uses_zero_start() {
    let mut enter = snap("S1", 0);
    enter.by_model.clear(); // No models seen yet at enter
    let complete = snap("S1", 5); // by_model has opus with input=5
    let phase = phase_with_snapshots(Some(enter), vec![], Some(complete));
    let report = phase_delta(&phase).expect("populated");
    assert_eq!(
        report.by_model_delta.get("claude-opus-4-7").unwrap().input,
        5
    );
}

/// Phase with only an enter snapshot (no steps, no complete) →
/// single snapshot returns the zero report (no delta possible).
#[test]
fn phase_delta_only_enter_returns_zero_report() {
    let phase = phase_with_snapshots(Some(snap("S1", 5)), vec![], None);
    let report = phase_delta(&phase).expect("populated");
    assert_eq!(report.input_tokens_delta, 0);
    assert_eq!(report.five_hour_pct_delta, Some(0));
}

/// pct_delta_with_reset: when `end` is None and `start` is Some,
/// the catch-all path returns `(Some(0), false)` — no anchor
/// available so we contribute zero without a false reset.
#[test]
fn phase_delta_pct_with_missing_end_contributes_zero_no_reset() {
    let mut enter = snap("S1", 0);
    let mut complete = snap("S1", 0);
    enter.five_hour_pct = Some(50);
    complete.five_hour_pct = None;
    let phase = phase_with_snapshots(Some(enter), vec![], Some(complete));
    let report = phase_delta(&phase).expect("populated");
    assert_eq!(report.five_hour_pct_delta, Some(0));
    assert!(!report.window_reset_observed);
}

/// Single-snapshot session sandwiched between others: the
/// `deltas_from_snapshots` algorithm skips the lone snapshot's
/// "session" because there's no pair to subtract. Subsequent
/// multi-snapshot sessions still contribute normally.
#[test]
fn phase_delta_with_single_snapshot_session_in_middle_skips_lone_snapshot() {
    // S1 has 1 snapshot (enter), S2 has 2 snapshots.
    // S1 contributes 0 (no pair). S2 contributes 12-3=9.
    let phase = phase_with_snapshots(
        Some(snap("S1", 5)),
        vec![
            (1, "code_task", snap("S2", 3)),
            (2, "code_task", snap("S2", 12)),
        ],
        None,
    );
    let report = phase_delta(&phase).expect("populated");
    assert_eq!(report.input_tokens_delta, 9);
}

/// Cross-session deltas when `session_id` is missing on a snapshot:
/// missing session id is treated as the empty-string sentinel,
/// grouping consecutive None snapshots together — they share a
/// "no session" context.
#[test]
fn phase_delta_with_missing_session_ids_groups_as_one_session() {
    let mut s_a = snap("ignored", 5);
    let mut s_b = snap("ignored", 12);
    s_a.session_id = None;
    s_b.session_id = None;
    let phase = phase_with_snapshots(Some(s_a), vec![], Some(s_b));
    let report = phase_delta(&phase).expect("populated");
    assert_eq!(report.input_tokens_delta, 7);
}
