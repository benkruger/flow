//! Pure delta + by-model rollup helpers driven entirely by the
//! `WindowSnapshot` data captured in `state.window_at_*` and
//! `phase.window_at_*` / `phase.step_snapshots[]`.
//!
//! Three public functions cover the three reader needs:
//!
//! - `phase_delta` — per-phase token / cost / pct delta, used by
//!   `format_complete_summary` (Token Cost section), `format_status`
//!   (Tokens block), and `tui_data::phase_token_table`.
//! - `flow_total` — flow-level sum of the same fields, used by the
//!   Complete summary's totals row.
//! - `by_model_rollup` — flat `HashMap<String, ModelTokens>` of
//!   per-model totals across the entire flow, used wherever a
//!   reader wants "what models did this flow use, and how much."
//!
//! Cross-session math is the load-bearing semantic: a flow can
//! span multiple sessions (resume, hand-off after compaction), and
//! each new session resets transcript counters to zero. Naive
//! `complete - enter` deltas across a session boundary go negative.
//! Every helper here groups snapshots by `session_id`, sums deltas
//! within each group, and sums across groups so the reported delta
//! reflects actual usage and never goes negative from a session
//! boundary.

use indexmap::IndexMap;

use crate::state::{FlowState, ModelTokens, PhaseState, WindowSnapshot};

/// Per-phase delta computed from a phase's enter / step / complete
/// snapshots.
///
/// Token, cost, turn, and tool counters are simple non-negative
/// deltas (cross-session sum). Rate-limit pct deltas are
/// `Option<i64>` because a window reset (curr < prev) makes the
/// pct delta meaningless for that span — `window_reset_observed`
/// records whether any span observed a reset so a reader can
/// switch to displaying the latest absolute pct instead.
#[derive(Debug, Clone, PartialEq)]
pub struct DeltaReport {
    pub input_tokens_delta: i64,
    pub output_tokens_delta: i64,
    pub cache_creation_tokens_delta: i64,
    pub cache_read_tokens_delta: i64,
    pub cost_delta_usd: f64,
    pub five_hour_pct_delta: Option<i64>,
    pub seven_day_pct_delta: Option<i64>,
    pub window_reset_observed: bool,
    pub turn_count_delta: i64,
    pub tool_call_count_delta: i64,
    pub by_model_delta: IndexMap<String, ModelTokens>,
}

impl DeltaReport {
    /// All-zero / `None` report — a defined "no contribution" value
    /// that callers can fold into running totals.
    fn zero() -> Self {
        Self {
            input_tokens_delta: 0,
            output_tokens_delta: 0,
            cache_creation_tokens_delta: 0,
            cache_read_tokens_delta: 0,
            cost_delta_usd: 0.0,
            five_hour_pct_delta: Some(0),
            seven_day_pct_delta: Some(0),
            window_reset_observed: false,
            turn_count_delta: 0,
            tool_call_count_delta: 0,
            by_model_delta: IndexMap::new(),
        }
    }

    /// Fold `other` into `self` element-wise. Token / cost / turn
    /// counters add. Pct deltas Option-add — `None` is sticky so a
    /// reset in any folded report propagates to the total.
    fn add(&mut self, other: &Self) {
        self.input_tokens_delta = self.input_tokens_delta.saturating_add(other.input_tokens_delta);
        self.output_tokens_delta = self
            .output_tokens_delta
            .saturating_add(other.output_tokens_delta);
        self.cache_creation_tokens_delta = self
            .cache_creation_tokens_delta
            .saturating_add(other.cache_creation_tokens_delta);
        self.cache_read_tokens_delta = self
            .cache_read_tokens_delta
            .saturating_add(other.cache_read_tokens_delta);
        self.cost_delta_usd += other.cost_delta_usd;
        self.five_hour_pct_delta = match (self.five_hour_pct_delta, other.five_hour_pct_delta) {
            (Some(a), Some(b)) => Some(a.saturating_add(b)),
            _ => None,
        };
        self.seven_day_pct_delta = match (self.seven_day_pct_delta, other.seven_day_pct_delta) {
            (Some(a), Some(b)) => Some(a.saturating_add(b)),
            _ => None,
        };
        self.window_reset_observed |= other.window_reset_observed;
        self.turn_count_delta = self.turn_count_delta.saturating_add(other.turn_count_delta);
        self.tool_call_count_delta = self
            .tool_call_count_delta
            .saturating_add(other.tool_call_count_delta);
        for (model, tokens) in &other.by_model_delta {
            let entry = self.by_model_delta.entry(model.clone()).or_default();
            entry.input = entry.input.saturating_add(tokens.input);
            entry.output = entry.output.saturating_add(tokens.output);
            entry.cache_create = entry.cache_create.saturating_add(tokens.cache_create);
            entry.cache_read = entry.cache_read.saturating_add(tokens.cache_read);
        }
    }
}

/// Compute a `DeltaReport` for a single phase.
///
/// Returns `None` when the phase has no `window_at_enter` snapshot —
/// without an enter point there is no anchor to compute deltas from.
/// When `window_at_complete` is missing, falls back to the latest
/// `step_snapshots[]` entry as the endpoint so an in-progress phase
/// can still report progress.
pub fn phase_delta(phase: &PhaseState) -> Option<DeltaReport> {
    let enter = phase.window_at_enter.as_ref()?;
    let mut snapshots: Vec<&WindowSnapshot> = vec![enter];
    for step in &phase.step_snapshots {
        snapshots.push(&step.snapshot);
    }
    if let Some(complete) = phase.window_at_complete.as_ref() {
        snapshots.push(complete);
    }
    Some(deltas_from_snapshots(&snapshots))
}

/// Compute a `DeltaReport` covering the entire flow — every phase's
/// snapshots plus the top-level `window_at_start`/`window_at_complete`,
/// merged into a single time-ordered sequence and processed via the
/// same cross-session aggregation as `phase_delta`.
pub fn flow_total(state: &FlowState) -> DeltaReport {
    let mut snapshots: Vec<&WindowSnapshot> = Vec::new();
    if let Some(start) = state.window_at_start.as_ref() {
        snapshots.push(start);
    }
    for phase in state.phases.values() {
        if let Some(enter) = phase.window_at_enter.as_ref() {
            snapshots.push(enter);
        }
        for step in &phase.step_snapshots {
            snapshots.push(&step.snapshot);
        }
        if let Some(complete) = phase.window_at_complete.as_ref() {
            snapshots.push(complete);
        }
    }
    if let Some(complete) = state.window_at_complete.as_ref() {
        snapshots.push(complete);
    }
    deltas_from_snapshots(&snapshots)
}

/// Per-model rollup across every snapshot in the flow. Sums the
/// per-session deltas (same cross-session math as `flow_total`)
/// and returns just the by-model breakdown so consumers that
/// only need "which models did this flow use, and how much" can
/// avoid building a full `DeltaReport`.
pub fn by_model_rollup(state: &FlowState) -> IndexMap<String, ModelTokens> {
    flow_total(state).by_model_delta
}

/// Internal: walk a list of snapshots in order, group by
/// `session_id`, and compute per-session deltas summed into one
/// report. Empty input or single-snapshot input returns the zero
/// report — a single snapshot has no anchor pair to subtract.
fn deltas_from_snapshots(snapshots: &[&WindowSnapshot]) -> DeltaReport {
    let mut total = DeltaReport::zero();
    if snapshots.len() < 2 {
        return total;
    }
    // Walk consecutive snapshots looking for session boundaries.
    // Within each session: subtract the first session-snapshot from
    // the last session-snapshot to get that session's contribution.
    let mut session_start: usize = 0;
    let mut current_session = snapshots[0].session_id.as_deref().unwrap_or("");
    for i in 1..=snapshots.len() {
        let next_session = if i < snapshots.len() {
            snapshots[i].session_id.as_deref().unwrap_or("")
        } else {
            // Sentinel value to flush the final session.
            "\0__END__"
        };
        if next_session != current_session {
            // Flush the [session_start, i-1] span.
            if i - 1 > session_start {
                let segment = pair_delta(snapshots[session_start], snapshots[i - 1]);
                total.add(&segment);
            }
            session_start = i;
            if i < snapshots.len() {
                current_session = next_session;
            }
        }
    }
    total
}

/// Compute the delta between two snapshots that belong to the same
/// session. Token / cost / turn / tool counters are
/// `end - start` (saturating). Pct deltas observe the reset rule:
/// when end < start, the delta is `None` and `window_reset_observed`
/// is `true`.
fn pair_delta(start: &WindowSnapshot, end: &WindowSnapshot) -> DeltaReport {
    let input_tokens_delta = sub_opt(end.session_input_tokens, start.session_input_tokens);
    let output_tokens_delta = sub_opt(end.session_output_tokens, start.session_output_tokens);
    let cache_creation_tokens_delta = sub_opt(
        end.session_cache_creation_tokens,
        start.session_cache_creation_tokens,
    );
    let cache_read_tokens_delta = sub_opt(
        end.session_cache_read_tokens,
        start.session_cache_read_tokens,
    );
    let cost_delta_usd = match (end.session_cost_usd, start.session_cost_usd) {
        (Some(a), Some(b)) => a - b,
        (Some(a), None) => a,
        _ => 0.0,
    };
    let (five_hour_pct_delta, five_reset) =
        pct_delta_with_reset(start.five_hour_pct, end.five_hour_pct);
    let (seven_day_pct_delta, seven_reset) =
        pct_delta_with_reset(start.seven_day_pct, end.seven_day_pct);
    let window_reset_observed = five_reset || seven_reset;
    let turn_count_delta = sub_opt(end.turn_count, start.turn_count);
    let tool_call_count_delta = sub_opt(end.tool_call_count, start.tool_call_count);

    let mut by_model_delta: IndexMap<String, ModelTokens> = IndexMap::new();
    // For each model present in `end`, subtract the matching `start` entry
    // (or zero if the model is new in `end`).
    for (model, end_tokens) in &end.by_model {
        let start_tokens = start.by_model.get(model).cloned().unwrap_or_default();
        by_model_delta.insert(
            model.clone(),
            ModelTokens {
                input: end_tokens.input.saturating_sub(start_tokens.input),
                output: end_tokens.output.saturating_sub(start_tokens.output),
                cache_create: end_tokens.cache_create.saturating_sub(start_tokens.cache_create),
                cache_read: end_tokens.cache_read.saturating_sub(start_tokens.cache_read),
            },
        );
    }

    DeltaReport {
        input_tokens_delta,
        output_tokens_delta,
        cache_creation_tokens_delta,
        cache_read_tokens_delta,
        cost_delta_usd,
        five_hour_pct_delta,
        seven_day_pct_delta,
        window_reset_observed,
        turn_count_delta,
        tool_call_count_delta,
        by_model_delta,
    }
}

/// `end - start` with `None` treated as `0` when the other side is
/// populated. Saturating to keep overflow harmless.
fn sub_opt(end: Option<i64>, start: Option<i64>) -> i64 {
    end.unwrap_or(0).saturating_sub(start.unwrap_or(0))
}

/// Compute a pct delta with reset detection. Returns
/// `(delta_or_None, reset_observed)`. A reset is detected when both
/// pcts are populated and `end < start` — the rolling window has
/// reset between snapshots, so a positive delta cannot be inferred.
fn pct_delta_with_reset(start: Option<i64>, end: Option<i64>) -> (Option<i64>, bool) {
    match (start, end) {
        (Some(s), Some(e)) if e < s => (None, true),
        (Some(s), Some(e)) => (Some(e - s), false),
        // Either side missing → contribute 0 to the delta but no
        // reset observed (just no data yet).
        _ => (Some(0), false),
    }
}
