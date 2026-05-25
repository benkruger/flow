//! Tests for `src/pricing.rs` — the per-token price table that converts
//! a model's `ModelTokens` into a USD cost via `cost_for`.
//!
//! `cost_for` is the only public surface; the price table (`price_for`,
//! `ModelPrice`, the per-family constants) is module-private, mirroring
//! the private `session_metrics::context_window_size` lookup. The table's
//! per-family / per-bucket / `[1m]`-premium branches are therefore
//! exercised through `cost_for` with single-bucket `ModelTokens` inputs
//! that isolate one price each.

use flow_rs::pricing::cost_for;
use flow_rs::state::ModelTokens;

/// Absolute tolerance for f64 cost comparisons. The renderer re-derives
/// each cost from tokens and compares within an epsilon rather than with
/// `==` (AC#5), because the multiply-sum in `cost_for` is not bit-exact.
const EPSILON: f64 = 1e-9;

/// Build a `ModelTokens` with the four buckets in declaration order.
fn tokens(input: i64, output: i64, cache_create: i64, cache_read: i64) -> ModelTokens {
    ModelTokens {
        input,
        output,
        cache_create,
        cache_read,
    }
}

// --- cost_for ---

#[test]
fn cost_for_opus_input_bucket_prices_at_list_rate() {
    // 1M input tokens at the Opus list rate ($15 / MTok) = $15.00.
    let c = cost_for("claude-opus-4-7", &tokens(1_000_000, 0, 0, 0)).expect("opus is priced");
    assert!((c - 15.0).abs() < EPSILON, "opus input bucket: got {c}");
}

#[test]
fn cost_for_opus_output_bucket_prices_at_list_rate() {
    // 1M output tokens at the Opus list rate ($75 / MTok) = $75.00.
    let c = cost_for("claude-opus-4-7", &tokens(0, 1_000_000, 0, 0)).expect("opus is priced");
    assert!((c - 75.0).abs() < EPSILON, "opus output bucket: got {c}");
}

#[test]
fn cost_for_opus_cache_write_bucket_prices_at_list_rate() {
    // 1M cache-creation tokens at the Opus 5-minute cache-write rate
    // (1.25x input = $18.75 / MTok) = $18.75.
    let c = cost_for("claude-opus-4-7", &tokens(0, 0, 1_000_000, 0)).expect("opus is priced");
    assert!(
        (c - 18.75).abs() < EPSILON,
        "opus cache-write bucket: got {c}"
    );
}

#[test]
fn cost_for_opus_cache_read_bucket_prices_at_list_rate() {
    // 1M cache-read tokens at the Opus cache-read rate
    // (0.1x input = $1.50 / MTok) = $1.50.
    let c = cost_for("claude-opus-4-7", &tokens(0, 0, 0, 1_000_000)).expect("opus is priced");
    assert!(
        (c - 1.50).abs() < EPSILON,
        "opus cache-read bucket: got {c}"
    );
}

#[test]
fn cost_for_opus_1m_suffix_applies_long_context_premium() {
    // The `[1m]` suffix selects the long-context premium tier:
    // 1M input tokens at Opus-1M ($30 / MTok) = $30.00, double the
    // standard Opus input rate. Proves the `[1m]` branch for opus.
    let c =
        cost_for("claude-opus-4-7[1m]", &tokens(1_000_000, 0, 0, 0)).expect("opus-1m is priced");
    assert!((c - 30.0).abs() < EPSILON, "opus-1m input bucket: got {c}");
}

#[test]
fn cost_for_sonnet_input_bucket_prices_at_list_rate() {
    // The `claude-` prefix + `sonnet` family selects the Sonnet table:
    // 1M input tokens at the Sonnet list rate ($3 / MTok) = $3.00.
    let c = cost_for("claude-sonnet-4-6", &tokens(1_000_000, 0, 0, 0)).expect("sonnet is priced");
    assert!((c - 3.0).abs() < EPSILON, "sonnet input bucket: got {c}");
}

#[test]
fn cost_for_sonnet_1m_suffix_applies_long_context_premium() {
    // 1M input tokens at Sonnet-1M ($6 / MTok) = $6.00, double the
    // standard Sonnet input rate. Proves the `[1m]` branch for sonnet.
    let c = cost_for("claude-sonnet-4-6[1m]", &tokens(1_000_000, 0, 0, 0))
        .expect("sonnet-1m is priced");
    assert!((c - 6.0).abs() < EPSILON, "sonnet-1m input bucket: got {c}");
}

#[test]
fn cost_for_haiku_input_bucket_prices_at_list_rate() {
    // The `haiku` family selects the Haiku table: 1M input tokens at
    // the Haiku list rate ($1 / MTok) = $1.00.
    let c = cost_for("claude-haiku-4-5", &tokens(1_000_000, 0, 0, 0)).expect("haiku is priced");
    assert!((c - 1.0).abs() < EPSILON, "haiku input bucket: got {c}");
}

#[test]
fn cost_for_non_claude_model_is_none() {
    // A model name that does not start with `claude-` is unknown to the
    // table; cost is unpriced (`None`), feeding the renderer's partial
    // `—` plumbing.
    assert!(cost_for("gpt-4", &tokens(1_000_000, 1_000_000, 0, 0)).is_none());
}

#[test]
fn cost_for_unknown_claude_family_is_none() {
    // A `claude-` model whose family is not opus/sonnet/haiku is unknown
    // to the table; cost is unpriced (`None`).
    assert!(cost_for("claude-future-9", &tokens(1_000_000, 1_000_000, 0, 0)).is_none());
}

/// Frozen-golden: the cost of a fixed `(model, ModelTokens)` pair pinned
/// to a value derived independently from the Opus per-bucket list rates.
///
/// Derivation (Opus standard tier, per-MTok rates / 1e6 = per-token):
///   input:       1000  * 15.00 / 1e6 = 0.015
///   output:       500  * 75.00 / 1e6 = 0.0375
///   cache_write:  200  * 18.75 / 1e6 = 0.00375
///   cache_read: 10000  *  1.50 / 1e6 = 0.015
///   total                              = 0.07125
///
/// Verified by hand-arithmetic above (not by copying production output),
/// per the frozen-golden bootstrapping discipline. Compared within
/// EPSILON because the f64 multiply-sum is not bit-exact (AC#5).
///
/// Update protocol: when the Opus list rates in `src/pricing.rs` change
/// intentionally, re-derive this total from the new rates in the same
/// commit and note the pricing change in the commit message.
#[test]
fn cost_for_frozen_golden_opus_mixed_buckets() {
    let c = cost_for("claude-opus-4-7", &tokens(1000, 500, 200, 10_000)).expect("opus is priced");
    assert!((c - 0.07125).abs() < EPSILON, "opus golden: got {c}");
}
