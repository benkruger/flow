//! Integration tests for `src/format_complete_summary.rs`. Migrated
//! from inline `#[cfg(test)]` in `src/format_complete_summary.rs` per
//! `.claude/rules/test-placement.md`.
//!
//! `truncate_prompt`, `outcome_marker`, and `outcome_label` are
//! private helpers driven through the public `format_complete_summary`
//! entry point; coverage comes from crafted state fixtures that force
//! each branch.

use std::path::{Path, PathBuf};

use flow_rs::format_complete_summary::{
    compute_cost_breakdown, format_complete_summary, format_findings_markdown, run_impl,
    run_impl_main, Args,
};
use serde_json::{json, Value};

mod common;

const PHASE_NAMES_LIST: [&str; 4] = ["Start", "Code", "Review", "Complete"];

fn all_complete_state() -> Value {
    let mut phases = serde_json::Map::new();
    let all_phases = ["flow-start", "flow-code", "flow-review", "flow-complete"];
    let timings = [20, 2700, 720, 45];
    for (i, &p) in all_phases.iter().enumerate() {
        phases.insert(
            p.to_string(),
            json!({
                "name": PHASE_NAMES_LIST[i],
                "status": "complete",
                "started_at": "2026-01-01T00:00:00-08:00",
                "completed_at": "2026-01-01T01:00:00-08:00",
                "session_started_at": null,
                "cumulative_seconds": timings[i],
                "visit_count": 1,
            }),
        );
    }
    json!({
        "branch": "test-feature",
        "pr_url": "https://github.com/test/test/pull/1",
        "started_at": "2026-01-01T00:00:00-08:00",
        "current_phase": "flow-complete",
        "prompt": "Add invoice PDF export with watermark support",
        "issues_filed": [],
        "notes": [],
        "phases": phases,
    })
}

fn write_state_file(dir: &Path) -> PathBuf {
    let state = all_complete_state();
    let state_file = dir.join("state.json");
    std::fs::write(&state_file, serde_json::to_string(&state).unwrap()).unwrap();
    state_file
}

// --- Token Cost section ---

use common::{add_phase_snapshots, snapshot_value};

/// Integration check: when every phase has both enter and complete
/// snapshots — the state shape the fixed `start_init`,
/// `complete_finalize`, and `complete_fast` writers produce —
/// `phase_delta` returns `Some(_)` for every phase, and the Token
/// Cost section renders real token + cost data for all four rows.
/// Without the dual write, the renderer's None branch produced a
/// placeholder row with tokens=0 and cost=`—` for any phase whose
/// snapshot pair was missing.
///
/// Depends on the fixes in Tasks 2 (start_init), 4 (complete_finalize),
/// and 6 (complete_fast); locks in the end-to-end summary contract.
#[test]
fn summary_shows_data_for_all_four_phases_when_capture_complete() {
    let mut state = all_complete_state();
    // Populate every phase with enter+complete snapshots so
    // `phase_delta` returns `Some(_)` for each one. The scaling
    // factor (enter_n -> complete_n) produces a positive token
    // delta and a positive cost delta per `snapshot_value`'s
    // schema.
    add_phase_snapshots(&mut state, "flow-start", 0, 5);
    add_phase_snapshots(&mut state, "flow-code", 5, 15);
    add_phase_snapshots(&mut state, "flow-review", 15, 20);
    add_phase_snapshots(&mut state, "flow-complete", 25, 30);

    let result = format_complete_summary(&state, None);

    // Scope assertions to the Token Cost subsection. The summary
    // also contains a Timeline section with phase name + duration
    // rows ("Start:  <1m") that share the "Start:" prefix, so a
    // raw search would match the timing row instead of the cost
    // row. The bounded slice ensures the cost-row assertions
    // target the intended section only.
    let tail_at_header = result
        .summary
        .split_once("Token Cost")
        .map(|(_, tail)| tail)
        .unwrap_or_else(|| panic!("Token Cost header missing; summary:\n{}", result.summary));
    let token_cost_section = tail_at_header
        .split_once("\n  Artifacts")
        .map(|(section, _)| section)
        .unwrap_or(tail_at_header);

    // Every named phase row must render with a real cost cell, not
    // the missing-snapshot placeholder `—`. The None branch in
    // `token_cost_section` produces `(0, None, false, true)`,
    // which renders the row's cost column as `—` and no `$` sign.
    for &name in &PHASE_NAMES_LIST {
        let row_marker = format!("{}:", name);
        let line = token_cost_section
            .lines()
            .find(|l| l.trim_start().starts_with(&row_marker))
            .unwrap_or_else(|| {
                panic!(
                    "missing Token Cost row for {}; section:\n{}",
                    name, token_cost_section
                )
            });
        assert!(
            line.contains('$'),
            "{} row missing cost cell — phase_delta likely returned None; line: {:?}",
            name,
            line
        );
        // The em-dash placeholder lives at the cost column position.
        // Stripping the leading "  <Name>:" prefix isolates the
        // tokens + cost suffix; assert no em-dash appears there.
        let suffix = line.trim_start_matches(|c: char| c.is_whitespace());
        let after_name = suffix.trim_start_matches(&*row_marker);
        assert!(
            !after_name.contains('—'),
            "{} row carries placeholder em-dash from missing-snapshot path; line: {:?}",
            name,
            line
        );
    }
}

/// Full data: every phase carries enter+complete snapshots.
/// The Token Cost section renders header + per-phase rows + total.
#[test]
fn token_cost_section_with_full_data_renders_per_phase_and_total() {
    let mut state = all_complete_state();
    add_phase_snapshots(&mut state, "flow-start", 0, 5);
    add_phase_snapshots(&mut state, "flow-code", 5, 10);
    add_phase_snapshots(&mut state, "flow-code", 10, 50);

    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("Token Cost"), "section header");
    assert!(result.summary.contains("Total:"));
    // 5 -> tokens grew by (5*100 + 5*50) = 750 from enter=0 to complete=5
    // for Start phase. Across 3 phases the combined Total should be > 0.
}

// Pre-fix tests `token_cost_section_with_no_snapshots_returns_empty`
// and `token_cost_section_with_partial_data_skips_unpopulated_phases`
// were removed per `.claude/rules/supersession.md` — both asserted
// the buggy "skip silently when no snapshots" behavior the plan
// (issue #1410) explicitly replaces. The new contract is exercised
// by the plan-named `token_cost_section_*` tests in the
// "Renderer placeholder behavior" block below.

/// Window reset observed: pct delta drops between enter and complete
/// for a phase → reset marker (↻) appears next to that phase row and
/// a footer note explains the marker.
#[test]
fn token_cost_section_with_window_reset_marks_observed() {
    let mut state = all_complete_state();
    let mut enter = snapshot_value("S1", 80, "claude-opus-4-7");
    let mut complete = snapshot_value("S1", 5, "claude-opus-4-7");
    // Force token growth so the row isn't filtered out
    enter["session_input_tokens"] = json!(100);
    complete["session_input_tokens"] = json!(500);
    state["phases"]["flow-code"]["window_at_enter"] = enter;
    state["phases"]["flow-code"]["window_at_complete"] = complete;
    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("Token Cost"));
    assert!(
        result.summary.contains("↻"),
        "reset marker must appear when window_reset_observed is true"
    );
    assert!(
        result.summary.contains("rate-limit window reset"),
        "footer note must explain the marker"
    );
}

/// Single-model flow: by_model rollup has one entry → the By Model
/// table is suppressed (no point showing a one-row breakdown).
#[test]
fn token_cost_section_single_model_skips_by_model_table() {
    let mut state = all_complete_state();
    add_phase_snapshots(&mut state, "flow-code", 10, 50);
    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("Token Cost"));
    assert!(
        !result.summary.contains("By Model"),
        "single-model rollup should suppress the By Model table"
    );
}

/// Multi-model flow: by_model rollup has 2+ entries → the By Model
/// table is rendered with one row per model.
#[test]
fn token_cost_section_multi_model_renders_by_model_table() {
    let mut state = all_complete_state();
    state["phases"]["flow-code"]["window_at_enter"] = snapshot_value("S1", 0, "claude-opus-4-7");
    let mut complete_v = snapshot_value("S1", 50, "claude-opus-4-7");
    // Add a second model's bucket to the by_model map.
    complete_v["by_model"]["claude-sonnet-4-6"] = json!({
        "input": 1000, "output": 500, "cache_create": 0, "cache_read": 0
    });
    state["phases"]["flow-code"]["window_at_complete"] = complete_v;
    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("By Model"));
    assert!(result.summary.contains("claude-opus-4-7"));
    assert!(result.summary.contains("claude-sonnet-4-6"));
}

/// `state.phases` is null → no phase entries to walk; section
/// short-circuits via the `state.get("phases").and_then` chain.
#[test]
fn token_cost_section_with_null_phases_value_returns_empty() {
    let mut state = all_complete_state();
    state["phases"] = json!(null);
    let result = format_complete_summary(&state, None);
    assert!(!result.summary.contains("Token Cost"));
}

// Pre-fix test `token_cost_section_with_zero_delta_phase_is_skipped`
// was removed per `.claude/rules/supersession.md` — the new contract
// renders a row whenever a phase's status is not "pending", even when
// the delta is zero. See the plan-named tests below for the new
// "section omitted only when every phase is pending" semantics.

/// Phase has a zero token delta (enter and complete carry identical
/// per-model token counts) → cost is a priced `Some(0.0)` and the
/// row still renders, because every non-pending phase produces a row.
#[test]
fn token_cost_section_with_zero_token_delta_renders_row() {
    let mut state = all_complete_state();
    let enter = snapshot_value("S1", 0, "claude-opus-4-7");
    let complete = snapshot_value("S1", 0, "claude-opus-4-7");
    state["phases"]["flow-code"]["window_at_enter"] = enter;
    state["phases"]["flow-code"]["window_at_complete"] = complete;
    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("Token Cost"));
    assert!(result.summary.contains("$0.000"));
}

/// Phases value missing from state → token_cost_section short-
/// circuits per phase via the `let Some(phase_v) = ...` else-arm.
/// Coverage of the None branch.
#[test]
fn token_cost_section_with_missing_phases_object_returns_empty() {
    let mut state = all_complete_state();
    // Drop the entire phases object so each PHASE_ORDER lookup fails.
    state.as_object_mut().unwrap().remove("phases");
    let result = format_complete_summary(&state, None);
    assert!(!result.summary.contains("Token Cost"));
}

// Pre-fix test `token_cost_section_with_unparseable_phase_skips_silently`
// was removed per `.claude/rules/supersession.md` — the new contract
// renders a placeholder row (cost = "—") for phases whose PhaseState
// fails to deserialize, instead of silently dropping them. The plan-
// named tests below (`token_cost_section_renders_em_dash_for_unknown_cost`,
// `token_cost_section_renders_phase_with_missing_window_at_enter`)
// cover the placeholder shape.

/// `format_tokens` boundary cases: < 1000 (raw integer), >= 1M
/// (megaformat). Drives both branches via crafted snapshots that
/// produce token counts in those ranges.
#[test]
fn token_cost_section_format_tokens_covers_small_and_million_ranges() {
    let mut state = all_complete_state();
    // Tiny token delta (< 1000): enter=0 → complete=1 produces
    // 100 input + 50 output = 150 tokens.
    add_phase_snapshots(&mut state, "flow-start", 0, 1);
    // Million-range delta: scale `n` so n*100 + n*50 > 1_000_000.
    // n=10000 → 1_500_000 tokens.
    add_phase_snapshots(&mut state, "flow-code", 0, 10000);
    let result = format_complete_summary(&state, None);
    // Raw integer for the small phase.
    assert!(result.summary.contains("150"));
    // M suffix for the million-range phase.
    assert!(result.summary.contains("M"));
}

// --- Renderer placeholder behavior (issue #1410) ---
//
// The pre-fix renderer silently dropped the entire Token Cost
// section when no phase had a complete snapshot pair. The fix:
// any phase whose status is not "pending" produces a row, with
// `—` placeholders for unknown values. The five tests below
// cover the new contract per Task 8 of the plan.

/// All phases pending → no row contributes → section omitted
/// (only path that still hides the header).
#[test]
fn token_cost_section_omitted_only_when_no_phases_have_run() {
    let mut state = all_complete_state();
    // Reset every phase to pending so no phase has run.
    for key in ["flow-start", "flow-code", "flow-review", "flow-complete"] {
        state["phases"][key]["status"] = json!("pending");
    }
    let result = format_complete_summary(&state, None);
    assert!(
        !result.summary.contains("Token Cost"),
        "section header must not appear when every phase is pending"
    );
}

/// Any phase with status != "pending" forces the section header,
/// even when no phase has snapshots and every per-phase delta
/// would be unknown.
#[test]
fn token_cost_section_renders_header_when_any_phase_has_run() {
    let state = all_complete_state();
    // No snapshots added — every phase has run (status="complete")
    // but no phase contributes a delta.
    let result = format_complete_summary(&state, None);
    assert!(
        result.summary.contains("Token Cost"),
        "section header must render when at least one phase has run"
    );
    assert!(
        result.summary.contains("Total:"),
        "total row must render alongside the header"
    );
}

/// A phase whose per-phase `by_model_delta` references only an
/// unpriced model produces an em-dash cost row instead of `$0.000`.
/// Rendering `$0.000` for both "not priceable" and "computed zero
/// cost" would erase the distinction; unknown cost renders as an
/// em-dash. Cost is token-derived (`pricing::cost_for` over the
/// by_model_delta), so an unpriced model is the way to drive the
/// unknown-cost branch.
#[test]
fn token_cost_section_renders_em_dash_for_unknown_cost() {
    let mut state = all_complete_state();
    // Non-`claude-` model → `pricing::price_for` returns None, so
    // the by_model_delta is unpriceable and the cost column is
    // unknown (em-dash) even though token deltas are present.
    let enter = snapshot_value("S1", 1, "gpt-4o-unpriced");
    let complete = snapshot_value("S1", 5, "gpt-4o-unpriced");
    state["phases"]["flow-code"]["window_at_enter"] = enter;
    state["phases"]["flow-code"]["window_at_complete"] = complete;
    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("Token Cost"));
    assert!(
        result.summary.contains("—"),
        "em-dash placeholder must appear when cost is not priceable"
    );
}

/// A phase with only `window_at_complete` (no `window_at_enter`)
/// renders a placeholder row marked partial. The pre-fix code
/// silently skipped the phase via the `phase_delta` returns-None
/// branch.
#[test]
fn token_cost_section_renders_phase_with_missing_window_at_enter() {
    let mut state = all_complete_state();
    // Only window_at_complete is set; window_at_enter is left
    // unpopulated (state has no enter snapshot).
    state["phases"]["flow-code"]["window_at_complete"] = snapshot_value("S1", 5, "claude-opus-4-7");
    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("Token Cost"));
    // Code phase row still renders even without an enter anchor.
    let token_section_start = result.summary.find("Token Cost").expect("section");
    let after_token = &result.summary[token_section_start..];
    assert!(
        after_token.contains("Code:"),
        "Code row must render even when window_at_enter is missing"
    );
    assert!(
        after_token.contains("—"),
        "missing-enter row must use em-dash for unknown cost"
    );
}

/// State with only a subset of PHASE_ORDER keys present in
/// `phases` exercises the `let Some(phase_v) = phases_obj.get(key)
/// else { continue }` branch — older state files or hand-edited
/// state may omit phase entries entirely.
#[test]
fn token_cost_section_skips_phase_keys_missing_from_state() {
    let mut state = all_complete_state();
    let phases_obj = state["phases"].as_object().unwrap().clone();
    let flow_code_entry = phases_obj.get("flow-code").cloned().unwrap();
    let mut new_phases = serde_json::Map::new();
    new_phases.insert("flow-code".to_string(), flow_code_entry);
    state["phases"] = json!(new_phases);
    add_phase_snapshots(&mut state, "flow-code", 0, 5);
    let result = format_complete_summary(&state, None);
    // The four PHASE_ORDER entries missing from the phases map hit
    // the `continue` arm; only flow-code contributes a row inside
    // the Token Cost section. Bound the assertion to that section
    // (other sections — phase timing, by-model — may still mention
    // "Start" via PHASE_NAMES_LIST).
    let token_section_start = result
        .summary
        .find("Token Cost")
        .expect("Token Cost section must render");
    let after_token = &result.summary[token_section_start..];
    let token_block_end = after_token.find("\n\n").unwrap_or(after_token.len());
    let token_block = &after_token[..token_block_end];
    assert!(
        token_block.contains("Code:"),
        "token block:\n{}",
        token_block
    );
    assert!(
        !token_block.contains("Start:"),
        "missing PHASE_ORDER keys must not produce rows:\n{}",
        token_block
    );
}

/// Total `total_partial=false` branch: every running phase
/// contributed `Some` cost, so the total has no `*` partial marker.
#[test]
fn token_cost_section_total_not_partial_when_every_phase_has_cost() {
    // Set every phase except flow-code to status=pending so they
    // skip; flow-code is the only running phase and has populated
    // snapshots → cost is Some → total_partial stays false.
    let mut state = all_complete_state();
    for key in ["flow-start", "flow-review", "flow-complete"] {
        state["phases"][key]["status"] = json!("pending");
    }
    add_phase_snapshots(&mut state, "flow-code", 0, 5);
    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("Token Cost"));
    assert!(
        !result.summary.contains("cost partial"),
        "footnote must NOT appear when every running phase has populated cost"
    );
}

/// When at least one phase contributes `None` cost, the total row
/// is marked partial via a `*` suffix so the user sees the total
/// is approximate.
#[test]
fn token_cost_section_total_marks_partial_when_any_unknown() {
    let mut state = all_complete_state();
    // flow-start has both cost endpoints populated → contributes
    // a known value to the total.
    add_phase_snapshots(&mut state, "flow-start", 0, 5);
    // flow-code has only window_at_complete → contributes None.
    state["phases"]["flow-code"]["window_at_complete"] = snapshot_value("S1", 5, "claude-opus-4-7");
    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("Token Cost"));
    // The total line carries the partial marker; the footnote
    // explains it.
    assert!(
        result.summary.contains("Total:"),
        "Total: header must render"
    );
    assert!(
        result.summary.contains("*"),
        "partial marker must appear when any phase contributed None cost"
    );
    assert!(
        result.summary.contains("cost partial"),
        "footnote must explain the partial marker"
    );
}

/// End-to-end render of an unpriced-cost phase. Cost is
/// token-derived: a phase whose by_model delta carries an unknown
/// model family cannot be priced, so `pair_delta` emits `None` and
/// this renderer prints `—` instead of a fabricated `$0.000`. The
/// phase row falls through the `None` cost arm in the Token Cost
/// section, and the `* cost partial` footnote appears because at
/// least one phase contributed `None` cost.
#[test]
fn format_complete_summary_renders_dash_for_unpriced_cost() {
    let mut state = all_complete_state();
    // Code phase: priced opus model — pair_delta returns Some(cost).
    add_phase_snapshots(&mut state, "flow-code", 0, 5);
    // Review phase: an unpriced model family. The token delta
    // is real (non-zero) but unprice-able, so pair_delta returns
    // None for cost and the row renders `—`.
    let phase = "flow-review";
    state["phases"][phase]["window_at_enter"] = snapshot_value("S1", 7, "gpt-4o-unpriced");
    state["phases"][phase]["window_at_complete"] = snapshot_value("S1", 12, "gpt-4o-unpriced");

    let result = format_complete_summary(&state, None);

    assert!(result.summary.contains("Token Cost"), "section header");
    // The summary contains both a phase timeline (with rows like
    // `Review: 12m`) and the Token Cost section (with rows like
    // `Review:  <tokens>  <cost>`). The cost row is the one we
    // need to inspect, so bound the search to the Token Cost
    // section between its header and the closing separator.
    let after_header = result
        .summary
        .split_once("Token Cost")
        .map(|(_, rest)| rest)
        .expect("Token Cost header must appear");
    let token_section = after_header
        .split_once("\n\n")
        .map(|(section, _)| section)
        .unwrap_or(after_header);
    let phase_label = "Review:";
    let row = token_section
        .lines()
        .find(|line| line.contains(phase_label))
        .unwrap_or_else(|| {
            panic!(
                "missing `{}` row in Token Cost section:\n{}",
                phase_label, token_section
            )
        });
    assert!(
        row.ends_with('—'),
        "row for `{}` must end in the em-dash that signals frozen cost; got `{}`",
        phase_label,
        row
    );
    // The frozen rows contributed None cost, so the total carries
    // the partial marker and the footnote appears.
    assert!(
        result.summary.contains("cost partial"),
        "footnote must appear when any phase contributes None cost; summary:\n{}",
        result.summary
    );
}

// --- compute_cost_breakdown structural API ---
//
// The structured intermediate `compute_cost_breakdown` returns the
// Token Cost data needed by two formatters: the terminal banner
// (existing `token_cost_section`) and the new PR-body markdown table
// (Task 4 `format_cost_table`). Tests below drive each branch of the
// extracted helper directly so coverage of the structural API is
// independent of the formatter that consumes it.

/// Full state with snapshots for every phase produces four rows and
/// totals that match the per-phase sums.
#[test]
fn compute_cost_breakdown_full_data_returns_all_four_phase_rows() {
    let mut state = all_complete_state();
    add_phase_snapshots(&mut state, "flow-start", 0, 5);
    add_phase_snapshots(&mut state, "flow-code", 5, 15);
    add_phase_snapshots(&mut state, "flow-review", 15, 20);
    add_phase_snapshots(&mut state, "flow-complete", 25, 30);

    let breakdown = compute_cost_breakdown(&state).expect("breakdown");
    assert_eq!(breakdown.rows.len(), 4, "four phase rows");

    let summed_tokens: i64 = breakdown.rows.iter().map(|r| r.tokens).sum();
    assert_eq!(breakdown.total_tokens, summed_tokens, "totals match sum");

    let summed_cost: f64 = breakdown.rows.iter().filter_map(|r| r.cost).sum();
    assert!(
        (breakdown.total_cost.unwrap_or(0.0) - summed_cost).abs() < 1e-9,
        "total_cost matches sum of per-row costs"
    );
}

/// AC#5: every rendered row's cost equals its token delta priced
/// through the pricing table, and the total equals the summed rows —
/// compared within a float epsilon, never with `==` (prices are
/// stored as $/token and re-multiplied, so binary float rounding
/// makes exact equality unreliable). Locks the cost↔token
/// reconciliation the token-derived cost source guarantees.
#[test]
fn compute_cost_breakdown_each_row_cost_matches_tokens_priced() {
    use flow_rs::pricing::cost_for;
    use flow_rs::state::ModelTokens;

    // (phase key, rendered name, enter_n, complete_n). Each phase uses
    // the single opus model from `add_phase_snapshots`, so its delta
    // is ((Δn)*100 input, (Δn)*50 output) per `snapshot_value`.
    let phases = [
        ("flow-start", "Start", 0, 5),
        ("flow-code", "Code", 5, 15),
        ("flow-review", "Review", 15, 20),
        ("flow-complete", "Complete", 25, 30),
    ];
    let mut state = all_complete_state();
    for (key, _, a, b) in phases {
        add_phase_snapshots(&mut state, key, a, b);
    }
    let breakdown = compute_cost_breakdown(&state).expect("breakdown");

    for (_, name, a, b) in phases {
        let dn = b - a;
        let expected = cost_for(
            "claude-opus-4-7",
            &ModelTokens {
                input: dn * 100,
                output: dn * 50,
                cache_create: 0,
                cache_read: 0,
            },
        )
        .expect("opus is priced");
        let row = breakdown
            .rows
            .iter()
            .find(|r| r.phase_name == name)
            .unwrap_or_else(|| panic!("row for {name}"));
        let got = row.cost.expect("priced row carries Some cost");
        assert!(
            (got - expected).abs() < 1e-9,
            "{name}: rendered cost {got} must equal its tokens priced ({expected})"
        );
    }

    // The total equals the summed per-row costs (epsilon, never ==).
    let summed: f64 = breakdown.rows.iter().filter_map(|r| r.cost).sum();
    assert!(
        (breakdown.total_cost.expect("total") - summed).abs() < 1e-9,
        "total must equal the summed rows"
    );
    // ...and equals the by-model aggregate re-priced through the table.
    let agg: f64 = breakdown
        .by_model
        .iter()
        .filter_map(|(m, t)| cost_for(m, t))
        .sum();
    assert!(
        (breakdown.total_cost.expect("total") - agg).abs() < 1e-9,
        "total must equal the by-model aggregate priced through the table"
    );
}

/// Phases map is `{}` → no rows accumulate → return None.
#[test]
fn compute_cost_breakdown_returns_none_when_phases_empty() {
    let mut state = all_complete_state();
    state["phases"] = json!({});
    assert!(compute_cost_breakdown(&state).is_none());
}

/// Every phase status is "pending" → no row contributes → return None.
#[test]
fn compute_cost_breakdown_returns_none_when_all_phases_pending() {
    let mut state = all_complete_state();
    for key in ["flow-start", "flow-code", "flow-review", "flow-complete"] {
        state["phases"][key]["status"] = json!("pending");
    }
    assert!(compute_cost_breakdown(&state).is_none());
}

/// A phase whose by_model delta carries an unpriced model family
/// produces a row with `cost = None` and flips `total_partial` —
/// cost is token-derived, so an unknown model is the "cost unknown"
/// signal that marks the total approximate.
#[test]
fn compute_cost_breakdown_marks_partial_when_cost_missing() {
    let mut state = all_complete_state();
    let enter = snapshot_value("S1", 1, "gpt-4o-unpriced");
    let complete = snapshot_value("S1", 5, "gpt-4o-unpriced");
    state["phases"]["flow-code"]["window_at_enter"] = enter;
    state["phases"]["flow-code"]["window_at_complete"] = complete;
    // Drop the other phases so flow-code is the only contributor.
    for key in ["flow-start", "flow-review", "flow-complete"] {
        state["phases"][key]["status"] = json!("pending");
    }

    let breakdown = compute_cost_breakdown(&state).expect("breakdown");
    assert!(breakdown.total_partial, "partial marker set");
    let row = breakdown
        .rows
        .iter()
        .find(|r| r.phase_name == "Code")
        .expect("Code row present");
    assert!(row.cost.is_none(), "Code row cost is None");
}

/// A phase whose snapshot pair shows the 5h pct dropping between
/// enter and complete records a window reset.
#[test]
fn compute_cost_breakdown_records_reset_observed() {
    let mut state = all_complete_state();
    let mut enter = snapshot_value("S1", 80, "claude-opus-4-7");
    let mut complete = snapshot_value("S1", 5, "claude-opus-4-7");
    // Force token growth so the row carries a non-trivial delta.
    enter["session_input_tokens"] = json!(100);
    complete["session_input_tokens"] = json!(500);
    state["phases"]["flow-code"]["window_at_enter"] = enter;
    state["phases"]["flow-code"]["window_at_complete"] = complete;

    let breakdown = compute_cost_breakdown(&state).expect("breakdown");
    assert!(breakdown.reset_observed_anywhere, "anywhere flag set");
    let row = breakdown
        .rows
        .iter()
        .find(|r| r.phase_name == "Code")
        .expect("Code row present");
    assert!(row.reset_observed, "Code row reset flag set");
}

/// Multi-model snapshots accumulate per-model tokens in `by_model`.
#[test]
fn compute_cost_breakdown_accumulates_by_model() {
    let mut state = all_complete_state();
    state["phases"]["flow-code"]["window_at_enter"] = snapshot_value("S1", 0, "claude-opus-4-7");
    let mut complete_v = snapshot_value("S1", 50, "claude-opus-4-7");
    complete_v["by_model"]["claude-sonnet-4-6"] = json!({
        "input": 1000, "output": 500, "cache_create": 0, "cache_read": 0
    });
    state["phases"]["flow-code"]["window_at_complete"] = complete_v;

    let breakdown = compute_cost_breakdown(&state).expect("breakdown");
    assert!(
        breakdown.by_model.contains_key("claude-opus-4-7"),
        "opus key present"
    );
    assert!(
        breakdown.by_model.contains_key("claude-sonnet-4-6"),
        "sonnet key present"
    );
}

/// Per `.claude/rules/security-gates.md` "Normalize Before
/// Comparing", the status comparison in `compute_cost_breakdown`
/// must accept case- and whitespace-drifted variants of
/// `"pending"` (`"PENDING"`, `"Pending"`, `" pending"`,
/// `"pending "`). State files can be hand-edited or carry
/// case-drifted values; an un-normalized comparison would flip
/// those phases from "pending" to "active" and produce phantom
/// rows.
#[test]
fn compute_cost_breakdown_normalizes_status_case_and_whitespace() {
    let mut state = all_complete_state();
    state["phases"]["flow-start"]["status"] = json!("PENDING");
    state["phases"]["flow-code"]["status"] = json!("Pending");
    state["phases"]["flow-review"]["status"] = json!("pending ");
    state["phases"]["flow-complete"]["status"] = json!(" pending");

    assert!(
        compute_cost_breakdown(&state).is_none(),
        "every phase is a case/whitespace variant of pending; breakdown should be None"
    );
}

/// Empty-string status is semantically equivalent to "missing
/// field" — it must be treated as pending so no row is produced.
/// Without this collapse, a hand-edited state file that clears
/// the status field to `""` flips the phase to "active" and
/// produces a phantom row.
#[test]
fn compute_cost_breakdown_treats_empty_string_status_as_pending() {
    let mut state = all_complete_state();
    // Only flow-code carries the empty-string status; the other
    // phases stay "complete" so they would contribute rows if
    // the gate is correct. We want to verify the empty-string
    // status specifically skips flow-code.
    for key in ["flow-start", "flow-review", "flow-complete"] {
        state["phases"][key]["status"] = json!("pending");
    }
    state["phases"]["flow-code"]["status"] = json!("");

    assert!(
        compute_cost_breakdown(&state).is_none(),
        "empty-string status must be treated as pending; got non-empty breakdown"
    );
}

// --- basic summary ---

#[test]
fn basic_summary() {
    let state = all_complete_state();
    let result = format_complete_summary(&state, None);

    assert!(result.summary.contains("Test Feature"));
    assert!(result
        .summary
        .contains("Add invoice PDF export with watermark support"));
    assert!(result
        .summary
        .contains("https://github.com/test/test/pull/1"));
    for name in &PHASE_NAMES_LIST {
        assert!(
            result.summary.contains(&format!("{}:", name)),
            "Missing phase {} in summary:\n{}",
            name,
            result.summary
        );
    }
    assert!(result.summary.contains("Total:"));
    assert_eq!(result.total_seconds, 20 + 2700 + 720 + 45);
}

#[test]
fn summary_with_issues() {
    let mut state = all_complete_state();
    state["issues_filed"] = json!([
        {
            "label": "Rule",
            "title": "Test rule",
            "url": "https://github.com/test/test/issues/1",
            "phase": "flow-review",
            "phase_name": "Review",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
        {
            "label": "Tech Debt",
            "title": "Refactor X",
            "url": "https://github.com/test/test/issues/2",
            "phase": "flow-review",
            "phase_name": "Review",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
    ]);

    let result = format_complete_summary(&state, None);

    assert!(result.summary.contains("Issues filed: 2"));
    assert!(!result
        .summary
        .contains("https://github.com/test/test/issues/1"));
    assert!(!result
        .summary
        .contains("https://github.com/test/test/issues/2"));
    assert!(result.issues_links.contains("[Rule] #1 Test rule"));
    assert!(result
        .issues_links
        .contains("https://github.com/test/test/issues/1"));
    assert!(result.issues_links.contains("[Tech Debt] #2 Refactor X"));
    assert!(result
        .issues_links
        .contains("https://github.com/test/test/issues/2"));
}

#[test]
fn summary_with_single_issue() {
    let mut state = all_complete_state();
    state["issues_filed"] = json!([
        {
            "label": "Tech Debt",
            "title": "Fix routing logic",
            "url": "https://github.com/test/test/issues/42",
            "phase": "flow-review",
            "phase_name": "Review",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
    ]);

    let result = format_complete_summary(&state, None);

    assert!(result.summary.contains("Issues filed: 1"));
    assert!(!result
        .summary
        .contains("https://github.com/test/test/issues/42"));
    assert!(result
        .issues_links
        .contains("[Tech Debt] #42 Fix routing logic"));
    assert!(result
        .issues_links
        .contains("https://github.com/test/test/issues/42"));
}

#[test]
fn summary_with_issues_url_without_number() {
    let mut state = all_complete_state();
    state["issues_filed"] = json!([
        {
            "label": "Rule",
            "title": "Some rule",
            "url": "https://example.com/custom-path",
            "phase": "flow-review",
            "phase_name": "Review",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
    ]);

    let result = format_complete_summary(&state, None);

    assert!(result.summary.contains("Issues filed: 1"));
    assert!(!result.summary.contains("https://example.com/custom-path"));
    assert!(result.issues_links.contains("[Rule] Some rule"));
    assert!(result
        .issues_links
        .contains("https://example.com/custom-path"));
}

// --- resolved / closed_issues ---

#[test]
fn summary_with_resolved_issues() {
    let state = all_complete_state();
    let closed = vec![json!({
        "number": 407,
        "url": "https://github.com/test/test/issues/407",
    })];

    let result = format_complete_summary(&state, Some(&closed));

    assert!(result.summary.contains("Resolved"));
    assert!(result.summary.contains("#407"));
    assert!(result
        .summary
        .contains("https://github.com/test/test/issues/407"));
}

#[test]
fn summary_with_multiple_resolved_issues() {
    let state = all_complete_state();
    let closed = vec![
        json!({"number": 83, "url": "https://github.com/test/test/issues/83"}),
        json!({"number": 89, "url": "https://github.com/test/test/issues/89"}),
    ];

    let result = format_complete_summary(&state, Some(&closed));

    assert!(result.summary.contains("#83"));
    assert!(result.summary.contains("#89"));
}

#[test]
fn summary_no_resolved_issues() {
    let state = all_complete_state();

    let result_none = format_complete_summary(&state, None);
    let result_empty = format_complete_summary(&state, Some(&[]));

    assert!(!result_none.summary.contains("Resolved"));
    assert!(!result_empty.summary.contains("Resolved"));
}

#[test]
fn summary_with_resolved_and_filed() {
    let mut state = all_complete_state();
    state["issues_filed"] = json!([
        {
            "label": "Tech Debt",
            "title": "Refactor X",
            "url": "https://github.com/test/test/issues/50",
            "phase": "flow-review",
            "phase_name": "Review",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
    ]);
    let closed = vec![json!({
        "number": 407,
        "url": "https://github.com/test/test/issues/407",
    })];

    let result = format_complete_summary(&state, Some(&closed));

    assert!(result.summary.contains("Resolved"));
    assert!(result.summary.contains("#407"));
    assert!(result.summary.contains("Issues filed: 1"));
    assert!(result.issues_links.contains("[Tech Debt] #50 Refactor X"));
    assert!(result
        .issues_links
        .contains("https://github.com/test/test/issues/50"));
}

#[test]
fn summary_resolved_without_url() {
    let state = all_complete_state();
    let closed = vec![json!({"number": 42})];

    let result = format_complete_summary(&state, Some(&closed));

    assert!(result.summary.contains("Resolved"));
    assert!(result.summary.contains("#42"));
}

#[test]
fn summary_with_filed_issue_without_url() {
    let mut state = all_complete_state();
    state["issues_filed"] = json!([
        {
            "label": "Rule",
            "title": "URL-less rule",
            "phase": "flow-review",
            "phase_name": "Review",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
    ]);

    let result = format_complete_summary(&state, None);
    assert!(result.issues_links.contains("[Rule] URL-less rule"));
    assert!(!result.issues_links.contains(" — "));
}

// --- outcome marker / label (private helpers driven through findings) ---

#[test]
fn summary_with_unknown_outcome_falls_back_to_question_marker() {
    let mut state = all_complete_state();
    state["findings"] = json!([
        {
            "finding": "future-outcome finding",
            "reason": "uses a not-yet-handled outcome",
            "outcome": "deferred",
            "phase": "flow-review",
            "phase_name": "Review",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
    ]);

    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("?"));
    assert!(result.summary.contains("Unknown"));
}

// --- notes / issue counts ---

#[test]
fn summary_with_notes() {
    let mut state = all_complete_state();
    state["notes"] = json!([
        {
            "phase": "flow-code",
            "phase_name": "Code",
            "timestamp": "2026-01-01T00:00:00-08:00",
            "type": "correction",
            "note": "Test note",
        },
    ]);

    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("Notes captured: 1"));
}

#[test]
fn summary_no_issues_no_notes() {
    let mut state = all_complete_state();
    state["issues_filed"] = json!([]);
    state["notes"] = json!([]);

    let result = format_complete_summary(&state, None);

    assert!(!result.summary.contains("Issues filed"));
    assert!(!result.summary.contains("Notes captured"));
    assert_eq!(result.issues_links, "");
}

#[test]
fn summary_issues_filed_key_absent_renders_empty_links() {
    let mut state = all_complete_state();
    state.as_object_mut().unwrap().remove("issues_filed");
    let result = format_complete_summary(&state, None);
    assert_eq!(result.issues_links, "");
}

#[test]
fn summary_issues_filed_wrong_type_renders_empty_links() {
    let mut state = all_complete_state();
    state["issues_filed"] = json!("not-an-array");
    let result = format_complete_summary(&state, None);
    assert_eq!(result.issues_links, "");
}

#[test]
fn issues_links_without_url() {
    let mut state = all_complete_state();
    state["issues_filed"] = json!([
        {
            "label": "Tech Debt",
            "title": "Missing test",
            "url": "",
            "phase": "flow-review",
            "phase_name": "Review",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
    ]);

    let result = format_complete_summary(&state, None);
    assert!(result.issues_links.contains("[Tech Debt] Missing test"));
    assert!(!result.issues_links.contains("—"));
}

// --- truncate_prompt coverage via format_complete_summary ---

#[test]
fn summary_truncates_long_prompt() {
    let mut state = all_complete_state();
    let long_prompt = "A".repeat(100);
    state["prompt"] = json!(long_prompt);

    let result = format_complete_summary(&state, None);

    assert!(!result.summary.contains(&long_prompt));
    assert!(result.summary.contains("..."));
    let expected = format!("{}...", "A".repeat(80));
    assert!(result.summary.contains(&expected));
}

#[test]
fn summary_short_prompt_not_truncated() {
    let mut state = all_complete_state();
    state["prompt"] = json!("Fix login bug");

    let result = format_complete_summary(&state, None);

    assert!(result.summary.contains("Fix login bug"));
    assert!(!result.summary.contains("..."));
}

#[test]
fn summary_prompt_exactly_at_limit_not_truncated() {
    // Covers the `<= MAX_PROMPT_LENGTH` boundary path of truncate_prompt
    // (80 chars exactly returns the prompt as-is).
    let mut state = all_complete_state();
    let exactly_80 = "A".repeat(80);
    state["prompt"] = json!(exactly_80.clone());

    let result = format_complete_summary(&state, None);

    assert!(result.summary.contains(&exactly_80));
    // No ellipsis when no truncation.
    assert!(!result.summary.contains("AAA..."));
}

#[test]
fn summary_prompt_multibyte_truncates_by_code_points() {
    // Covers the multi-byte code-point branch: 81 "日" chars is 81 code
    // points (> 80 limit) but 243 bytes — truncate_prompt must count
    // chars not bytes, taking 80 and appending "...".
    let mut state = all_complete_state();
    state["prompt"] = json!("日".repeat(81));

    let result = format_complete_summary(&state, None);

    let truncated = format!("{}...", "日".repeat(80));
    assert!(
        result.summary.contains(&truncated),
        "expected 80 chars + ... in summary, got:\n{}",
        result.summary
    );
}

// --- formatting chrome ---

#[test]
fn summary_uses_format_time() {
    let state = all_complete_state();
    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("<1m"));
    assert!(result.summary.contains("45m"));
    assert!(result.summary.contains("5m"));
}

#[test]
fn summary_heavy_borders() {
    let state = all_complete_state();
    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("━━"));
}

#[test]
fn summary_check_mark() {
    let state = all_complete_state();
    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("✓"));
}

#[test]
fn summary_version() {
    let state = all_complete_state();
    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("FLOW v"));
}

// --- run_impl ---

#[test]
fn cli_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = write_state_file(dir.path());
    let args = Args {
        state_file: state_file.to_string_lossy().to_string(),
        closed_issues_file: None,
    };
    let result = run_impl(&args).unwrap();
    assert!(result.summary.contains("Test Feature"));
    assert!(result.total_seconds > 0);
}

#[test]
fn cli_with_closed_issues_file() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = write_state_file(dir.path());
    let closed = vec![json!({
        "number": 407,
        "url": "https://github.com/test/test/issues/407",
    })];
    let closed_file = dir.path().join("closed.json");
    std::fs::write(&closed_file, serde_json::to_string(&closed).unwrap()).unwrap();

    let args = Args {
        state_file: state_file.to_string_lossy().to_string(),
        closed_issues_file: Some(closed_file.to_string_lossy().to_string()),
    };
    let result = run_impl(&args).unwrap();
    assert!(result.summary.contains("Resolved"));
    assert!(result.summary.contains("#407"));
}

#[test]
fn cli_missing_closed_issues_file() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = write_state_file(dir.path());
    let args = Args {
        state_file: state_file.to_string_lossy().to_string(),
        closed_issues_file: Some(
            dir.path()
                .join("nonexistent.json")
                .to_string_lossy()
                .to_string(),
        ),
    };
    let result = run_impl(&args).unwrap();
    assert!(!result.summary.contains("Resolved"));
}

#[test]
fn cli_missing_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        state_file: dir
            .path()
            .join("missing.json")
            .to_string_lossy()
            .to_string(),
        closed_issues_file: None,
    };
    let result = run_impl(&args);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not found"));
}

#[test]
fn cli_state_file_unreadable_returns_read_error() {
    // Covers the `map_err(|e| format!("Failed to read state file"))?`
    // closure on line 254 — exists() is true but read_to_string fails
    // because the path is a directory.
    let dir = tempfile::tempdir().unwrap();
    let state_as_dir = dir.path().join("state.json");
    std::fs::create_dir_all(&state_as_dir).unwrap();
    let args = Args {
        state_file: state_as_dir.to_string_lossy().to_string(),
        closed_issues_file: None,
    };
    let result = run_impl(&args);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Failed to read state file"));
}

#[test]
fn cli_corrupt_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let bad_file = dir.path().join("bad.json");
    std::fs::write(&bad_file, "{bad json").unwrap();
    let args = Args {
        state_file: bad_file.to_string_lossy().to_string(),
        closed_issues_file: None,
    };
    let result = run_impl(&args);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Failed to parse"));
}

#[test]
fn run_impl_closed_content_unreadable_omits_resolved() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = write_state_file(dir.path());
    let closed_dir = dir.path().join("closed_as_dir");
    std::fs::create_dir_all(&closed_dir).unwrap();
    let args = Args {
        state_file: state_file.to_string_lossy().to_string(),
        closed_issues_file: Some(closed_dir.to_string_lossy().to_string()),
    };
    let result = run_impl(&args).unwrap();
    assert!(!result.summary.contains("Resolved"));
    assert!(result.summary.contains("Test Feature"));
}

#[test]
fn run_impl_closed_content_malformed_omits_resolved() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = write_state_file(dir.path());
    let closed_file = dir.path().join("malformed.json");
    std::fs::write(&closed_file, "{not valid json").unwrap();
    let args = Args {
        state_file: state_file.to_string_lossy().to_string(),
        closed_issues_file: Some(closed_file.to_string_lossy().to_string()),
    };
    let result = run_impl(&args).unwrap();
    assert!(!result.summary.contains("Resolved"));
    assert!(result.summary.contains("Test Feature"));
}

// --- findings ---

#[test]
fn summary_with_review_findings() {
    let mut state = all_complete_state();
    state["findings"] = json!([
        {
            "finding": "Unused variable in handler",
            "reason": "False positive from macro expansion",
            "outcome": "dismissed",
            "phase": "flow-review",
            "phase_name": "Review",
            "timestamp": "2026-01-01T00:30:00-08:00",
        },
        {
            "finding": "Missing null check in parser",
            "reason": "Could panic on malformed input",
            "outcome": "fixed",
            "phase": "flow-review",
            "phase_name": "Review",
            "timestamp": "2026-01-01T00:31:00-08:00",
        },
    ]);

    let result = format_complete_summary(&state, None);

    assert!(result.summary.contains("Review Findings"));
    assert!(result.summary.contains("Unused variable in handler"));
    assert!(result.summary.contains("Missing null check in parser"));
    assert!(result.summary.contains("✗"));
    assert!(result.summary.contains("✓"));
    assert!(result
        .summary
        .contains("False positive from macro expansion"));
}

/// `outcome_marker` and `outcome_label` are general finding-rendering
/// infrastructure: they map every outcome the add-finding CLI accepts
/// (`VALID_OUTCOMES` in `src/add_finding.rs`) to a marker + label,
/// independent of which phase recorded the finding. This exercises the
/// `rule_written` / `rule_clarified` arms (marker `+`, labels
/// "Rule written" / "Rule clarified") through the Review Findings
/// section so the mapping stays covered.
#[test]
fn summary_renders_rule_outcome_marker_and_labels() {
    let mut state = all_complete_state();
    state["findings"] = json!([
        {
            "finding": "Added a rule",
            "reason": "New constraint",
            "outcome": "rule_written",
            "phase": "flow-review",
            "phase_name": "Review",
            "path": ".claude/rules/new.md",
            "timestamp": "2026-01-01T00:30:00-08:00",
        },
        {
            "finding": "Clarified a rule",
            "reason": "Tightened wording",
            "outcome": "rule_clarified",
            "phase": "flow-review",
            "phase_name": "Review",
            "path": ".claude/rules/existing.md",
            "timestamp": "2026-01-01T00:31:00-08:00",
        },
    ]);

    let result = format_complete_summary(&state, None);

    assert!(result.summary.contains("Review Findings"));
    assert!(result.summary.contains("+"));
    assert!(result.summary.contains("Rule written"));
    assert!(result.summary.contains("Rule clarified"));
}

#[test]
fn summary_review_findings_only_no_learn_section() {
    let mut state = all_complete_state();
    state["findings"] = json!([
        {
            "finding": "Bug in parser",
            "reason": "Fixed inline",
            "outcome": "fixed",
            "phase": "flow-review",
            "phase_name": "Review",
            "timestamp": "2026-01-01T00:30:00-08:00",
        },
    ]);

    let result = format_complete_summary(&state, None);

    assert!(result.summary.contains("Review Findings"));
    assert!(!result.summary.contains("Learn Findings"));
}

#[test]
fn summary_no_findings_hides_sections() {
    let mut state = all_complete_state();
    state["findings"] = json!([]);

    let result_empty = format_complete_summary(&state, None);
    assert!(!result_empty.summary.contains("Review Findings"));

    let state_no_key = all_complete_state();
    let result_missing = format_complete_summary(&state_no_key, None);
    assert!(!result_missing.summary.contains("Review Findings"));
}

#[test]
fn summary_findings_all_outcomes() {
    let mut state = all_complete_state();
    state["findings"] = json!([
        {
            "finding": "f1",
            "reason": "r1",
            "outcome": "fixed",
            "phase": "flow-review",
            "phase_name": "Review",
            "timestamp": "2026-01-01T00:30:00-08:00",
        },
        {
            "finding": "f2",
            "reason": "r2",
            "outcome": "dismissed",
            "phase": "flow-review",
            "phase_name": "Review",
            "timestamp": "2026-01-01T00:31:00-08:00",
        },
        {
            "finding": "f3",
            "reason": "r3",
            "outcome": "filed",
            "phase": "flow-review",
            "phase_name": "Review",
            "issue_url": "https://github.com/test/test/issues/99",
            "timestamp": "2026-01-01T00:32:00-08:00",
        },
    ]);

    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("✓"));
    assert!(result.summary.contains("✗"));
    assert!(result.summary.contains("→"));
    assert!(result.summary.contains("Review Findings"));
}

#[test]
fn summary_findings_with_existing_artifacts() {
    let mut state = all_complete_state();
    state["findings"] = json!([
        {
            "finding": "Bug found",
            "reason": "Fixed it",
            "outcome": "fixed",
            "phase": "flow-review",
            "phase_name": "Review",
            "timestamp": "2026-01-01T00:30:00-08:00",
        },
    ]);
    state["issues_filed"] = json!([
        {
            "label": "Tech Debt",
            "title": "Refactor X",
            "url": "https://github.com/test/test/issues/50",
            "phase": "flow-review",
            "phase_name": "Review",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
    ]);

    let result = format_complete_summary(&state, None);

    assert!(result.summary.contains("Review Findings"));
    assert!(result.summary.contains("Issues filed: 1"));
}

// --- format_findings_markdown ---

#[test]
fn format_findings_markdown_empty_returns_empty_string() {
    let out = format_findings_markdown(&[], "flow-review");
    assert_eq!(out, "");
}

#[test]
fn format_findings_markdown_no_phase_match_returns_empty_string() {
    let findings = vec![json!({
        "finding": "Missing rule for X",
        "reason": "Captured during analysis",
        "outcome": "rule_written",
        "phase": "flow-code",
    })];
    let out = format_findings_markdown(&findings, "flow-review");
    assert_eq!(out, "");
}

#[test]
fn format_findings_markdown_renders_fixed_finding() {
    let findings = vec![json!({
        "finding": "Missing null check in parser",
        "reason": "Could panic on malformed input",
        "outcome": "fixed",
        "phase": "flow-review",
    })];
    let out = format_findings_markdown(&findings, "flow-review");
    assert!(
        out.contains("- ✓ **Missing null check in parser**"),
        "expected fixed marker + bold finding; got:\n{}",
        out
    );
    assert!(
        out.contains("  - Fixed — Could panic on malformed input"),
        "expected indented Fixed label + reason; got:\n{}",
        out
    );
}

#[test]
fn format_findings_markdown_renders_dismissed_finding() {
    let findings = vec![json!({
        "finding": "Unused import",
        "reason": "False positive from macro expansion",
        "outcome": "dismissed",
        "phase": "flow-review",
    })];
    let out = format_findings_markdown(&findings, "flow-review");
    assert!(
        out.contains("- ✗ **Unused import**"),
        "expected dismissed marker + bold finding; got:\n{}",
        out
    );
    assert!(
        out.contains("  - Dismissed — False positive from macro expansion"),
        "expected indented Dismissed label + reason; got:\n{}",
        out
    );
}

#[test]
fn format_findings_markdown_renders_mixed_outcomes_in_order() {
    let findings = vec![
        json!({
            "finding": "first",
            "reason": "r1",
            "outcome": "fixed",
            "phase": "flow-review",
        }),
        json!({
            "finding": "second",
            "reason": "r2",
            "outcome": "dismissed",
            "phase": "flow-review",
        }),
        json!({
            "finding": "third",
            "reason": "r3",
            "outcome": "fixed",
            "phase": "flow-review",
        }),
    ];
    let out = format_findings_markdown(&findings, "flow-review");
    let p1 = out.find("**first**").expect("first finding rendered");
    let p2 = out.find("**second**").expect("second finding rendered");
    let p3 = out.find("**third**").expect("third finding rendered");
    assert!(
        p1 < p2 && p2 < p3,
        "findings must render in input order; got:\n{}",
        out
    );
    assert!(out.contains("- ✓ **first**"));
    assert!(out.contains("- ✗ **second**"));
    assert!(out.contains("- ✓ **third**"));
}

#[test]
fn format_findings_markdown_filters_by_phase() {
    let findings = vec![
        json!({
            "finding": "review-only",
            "reason": "r1",
            "outcome": "fixed",
            "phase": "flow-review",
        }),
        json!({
            "finding": "code-only",
            "reason": "r2",
            "outcome": "rule_written",
            "phase": "flow-code",
        }),
    ];
    let out = format_findings_markdown(&findings, "flow-review");
    assert!(out.contains("**review-only**"), "review entry must appear");
    assert!(
        !out.contains("**code-only**"),
        "non-review entry must NOT appear when filtering for flow-review; got:\n{}",
        out
    );
}

#[test]
fn format_findings_markdown_sanitizes_newlines_in_finding() {
    let findings = vec![json!({
        "finding": "first line\nsecond line",
        "reason": "r1",
        "outcome": "fixed",
        "phase": "flow-review",
    })];
    let out = format_findings_markdown(&findings, "flow-review");
    assert!(
        out.contains("**first line second line**"),
        "newline in finding must become a single space; got:\n{}",
        out
    );
    // The literal newline inside the bold span would break the
    // nested list structure on GitHub — assert it was stripped.
    assert!(
        !out.contains("first line\nsecond line"),
        "raw newline inside finding must be replaced; got:\n{}",
        out
    );
}

#[test]
fn format_findings_markdown_sanitizes_newlines_in_reason() {
    let findings = vec![json!({
        "finding": "f1",
        "reason": "reason line one\nreason line two",
        "outcome": "fixed",
        "phase": "flow-review",
    })];
    let out = format_findings_markdown(&findings, "flow-review");
    assert!(
        out.contains("Fixed — reason line one reason line two"),
        "newline in reason must become a single space; got:\n{}",
        out
    );
    assert!(
        !out.contains("reason line one\nreason line two"),
        "raw newline inside reason must be replaced; got:\n{}",
        out
    );
}

#[test]
fn format_findings_markdown_handles_missing_outcome_field() {
    let findings = vec![json!({
        "finding": "no outcome key",
        "reason": "r1",
        "phase": "flow-review",
    })];
    // The helper must not panic when outcome is absent. It falls
    // back to outcome_marker("") / outcome_label("") which the
    // existing private helpers map to "?" / "Unknown".
    let out = format_findings_markdown(&findings, "flow-review");
    assert!(out.contains("**no outcome key**"));
    assert!(out.contains("?"));
    assert!(out.contains("Unknown"));
}

#[test]
fn format_findings_markdown_normalizes_uppercase_phase() {
    let findings = vec![json!({
        "finding": "Uppercase phase finding",
        "reason": "Phase value was FLOW-REVIEW",
        "outcome": "fixed",
        "phase": "FLOW-REVIEW",
    })];
    let out = format_findings_markdown(&findings, "flow-review");
    assert!(
        out.contains("**Uppercase phase finding**"),
        "uppercase phase value must normalize to lowercase before filter equality; got:\n{}",
        out
    );
}

#[test]
fn format_findings_markdown_normalizes_whitespace_padded_phase() {
    let findings = vec![json!({
        "finding": "Padded phase finding",
        "reason": "Phase value carried surrounding whitespace",
        "outcome": "fixed",
        "phase": " flow-review ",
    })];
    let out = format_findings_markdown(&findings, "flow-review");
    assert!(
        out.contains("**Padded phase finding**"),
        "whitespace-padded phase value must trim before filter equality; got:\n{}",
        out
    );
}

#[test]
fn format_findings_markdown_normalizes_phase_with_embedded_nul() {
    let findings = vec![json!({
        "finding": "NUL phase finding",
        "reason": "Phase value carried an embedded NUL",
        "outcome": "fixed",
        "phase": "flow-review\u{0}",
    })];
    let out = format_findings_markdown(&findings, "flow-review");
    assert!(
        out.contains("**NUL phase finding**"),
        "NUL-bearing phase value must strip NUL before filter equality; got:\n{}",
        out
    );
}

#[test]
fn format_findings_markdown_collapses_crlf_to_single_space() {
    let findings = vec![json!({
        "finding": "line1\r\nline2",
        "reason": "r1",
        "outcome": "fixed",
        "phase": "flow-review",
    })];
    let out = format_findings_markdown(&findings, "flow-review");
    assert!(
        out.contains("**line1 line2**"),
        "CRLF must collapse to a single space; got:\n{}",
        out
    );
    assert!(
        !out.contains("line1  line2"),
        "CRLF must not leave a double-space gap; got:\n{}",
        out
    );
}

#[test]
fn format_findings_markdown_collapses_lone_cr_to_single_space() {
    // Lone `\r` without a following `\n` (classic Mac line ending,
    // or a corrupted CRLF where the LF was stripped) exercises the
    // false branch of the CRLF-peek inside escape_markdown_list_value.
    let findings = vec![json!({
        "finding": "line1\rline2",
        "reason": "r1",
        "outcome": "fixed",
        "phase": "flow-review",
    })];
    let out = format_findings_markdown(&findings, "flow-review");
    assert!(
        out.contains("**line1 line2**"),
        "lone CR must collapse to a single space; got:\n{}",
        out
    );
}

#[test]
fn format_findings_markdown_sanitizes_tab_in_reason() {
    let findings = vec![json!({
        "finding": "f1",
        "reason": "before\tafter",
        "outcome": "fixed",
        "phase": "flow-review",
    })];
    let out = format_findings_markdown(&findings, "flow-review");
    assert!(
        out.contains("Fixed — before after"),
        "tab in reason must become a single space; got:\n{}",
        out
    );
    assert!(
        !out.contains("before\tafter"),
        "raw tab in reason must be replaced; got:\n{}",
        out
    );
}

#[test]
fn format_findings_markdown_escapes_bold_markers_in_finding() {
    let findings = vec![json!({
        "finding": "Inner **bold** marker",
        "reason": "r1",
        "outcome": "fixed",
        "phase": "flow-review",
    })];
    let out = format_findings_markdown(&findings, "flow-review");
    assert!(
        out.contains("**Inner \\*\\*bold\\*\\* marker**"),
        "asterisks in finding must be backslash-escaped; got:\n{}",
        out
    );
}

#[test]
fn format_findings_markdown_escapes_backtick_in_reason() {
    let findings = vec![json!({
        "finding": "f1",
        "reason": "Run `bin/flow ci` before commit",
        "outcome": "fixed",
        "phase": "flow-review",
    })];
    let out = format_findings_markdown(&findings, "flow-review");
    assert!(
        out.contains("Fixed — Run \\`bin/flow ci\\` before commit"),
        "backticks in reason must be backslash-escaped; got:\n{}",
        out
    );
}

#[test]
fn format_findings_markdown_escapes_html_angle_brackets() {
    let findings = vec![json!({
        "finding": "Renders <script>alert(1)</script>",
        "reason": "Untrusted HTML must not pass through",
        "outcome": "dismissed",
        "phase": "flow-review",
    })];
    let out = format_findings_markdown(&findings, "flow-review");
    assert!(
        out.contains("&lt;script&gt;"),
        "`<` and `>` must be HTML-entity escaped; got:\n{}",
        out
    );
    assert!(
        !out.contains("<script>"),
        "raw `<script>` must NOT appear in output; got:\n{}",
        out
    );
}

#[test]
fn format_findings_markdown_escapes_backslash_first() {
    let findings = vec![json!({
        "finding": "trailing backslash\\",
        "reason": "r1",
        "outcome": "fixed",
        "phase": "flow-review",
    })];
    let out = format_findings_markdown(&findings, "flow-review");
    // A trailing single backslash would otherwise escape the
    // closing `**` markdown marker. The escape pass doubles each
    // backslash so the value cannot escape the closing wrapper.
    assert!(
        out.contains("**trailing backslash\\\\**"),
        "backslash must be doubled to neutralize trailing-escape on closing markers; got:\n{}",
        out
    );
}

// --- run_impl_main ---

#[test]
fn run_impl_main_happy_path_returns_ok_value() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = write_state_file(dir.path());
    let args = Args {
        state_file: state_file.to_string_lossy().to_string(),
        closed_issues_file: None,
    };
    let (value, code) = run_impl_main(&args);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert!(value["summary"].as_str().unwrap().contains("Test Feature"));
    assert!(value["total_seconds"].as_i64().unwrap() > 0);
}

#[test]
fn run_impl_main_missing_state_file_returns_err_exit_1() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        state_file: dir
            .path()
            .join("missing.json")
            .to_string_lossy()
            .to_string(),
        closed_issues_file: None,
    };
    let (value, code) = run_impl_main(&args);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    assert!(value["message"].as_str().unwrap().contains("not found"));
}
