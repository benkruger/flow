//! Per-token USD price table for Claude models.
//!
//! `cost_for` converts a model's captured `ModelTokens` into a USD cost
//! by multiplying each token bucket (input, output, cache-write,
//! cache-read) by its per-token list price and summing. It is the single
//! public surface; `price_for`, `ModelPrice`, and the per-family
//! constants are module-private, mirroring the private
//! `session_metrics::context_window_size` lookup-table pattern: a `claude-`
//! prefix plus a `[1m]` long-context suffix select a table entry, and an
//! unknown model returns `None` so callers fall back to a partial (`—`)
//! cost rather than presenting a guessed figure.
//!
//! Prices are Anthropic published list rates expressed as USD per token
//! (per-MTok rate / 1_000_000). The standard cache-write rate is the
//! 5-minute ephemeral tier (1.25x input); the cache-read rate is 0.1x
//! input. The `[1m]` long-context tier doubles input and multiplies
//! output by 1.5, with cache rates re-derived from the tier's input.
//!
//! Maintenance path: when Anthropic changes published pricing, update the
//! per-family constants below (the `<rate> / 1_000_000.0` literals show
//! each per-MTok rate) and re-derive the frozen-golden total in
//! `tests/pricing.rs` in the same commit. Adding a new model family is a
//! new `model.contains(...)` arm plus its constant.

use crate::state::ModelTokens;

/// Per-token USD prices for one model, split by token bucket.
///
/// Each field is USD per single token (per-MTok list rate / 1e6).
/// `cache_write` prices `ModelTokens.cache_create`; `cache_read` prices
/// `ModelTokens.cache_read`.
struct ModelPrice {
    input: f64,
    output: f64,
    cache_write: f64,
    cache_read: f64,
}

const OPUS: ModelPrice = ModelPrice {
    input: 15.0 / 1_000_000.0,
    output: 75.0 / 1_000_000.0,
    cache_write: 18.75 / 1_000_000.0,
    cache_read: 1.50 / 1_000_000.0,
};

const OPUS_1M: ModelPrice = ModelPrice {
    input: 30.0 / 1_000_000.0,
    output: 112.50 / 1_000_000.0,
    cache_write: 37.50 / 1_000_000.0,
    cache_read: 3.00 / 1_000_000.0,
};

const SONNET: ModelPrice = ModelPrice {
    input: 3.0 / 1_000_000.0,
    output: 15.0 / 1_000_000.0,
    cache_write: 3.75 / 1_000_000.0,
    cache_read: 0.30 / 1_000_000.0,
};

const SONNET_1M: ModelPrice = ModelPrice {
    input: 6.0 / 1_000_000.0,
    output: 22.50 / 1_000_000.0,
    cache_write: 7.50 / 1_000_000.0,
    cache_read: 0.60 / 1_000_000.0,
};

const HAIKU: ModelPrice = ModelPrice {
    input: 1.0 / 1_000_000.0,
    output: 5.0 / 1_000_000.0,
    cache_write: 1.25 / 1_000_000.0,
    cache_read: 0.10 / 1_000_000.0,
};

/// Look up the per-token price table for a model name.
///
/// Returns `Some(ModelPrice)` for a `claude-` model in a known family
/// (opus / sonnet / haiku); `None` for any other name. The `[1m]`
/// long-context suffix selects the premium tier for opus and sonnet
/// (haiku has no published 1M tier). Mirrors the private
/// `session_metrics::context_window_size` matcher.
fn price_for(model: &str) -> Option<ModelPrice> {
    if !model.starts_with("claude-") {
        return None;
    }
    let is_1m = model.contains("[1m]");
    if model.contains("opus") {
        return Some(if is_1m { OPUS_1M } else { OPUS });
    }
    if model.contains("sonnet") {
        return Some(if is_1m { SONNET_1M } else { SONNET });
    }
    if model.contains("haiku") {
        return Some(HAIKU);
    }
    None
}

/// Compute the USD cost of a model's captured token usage.
///
/// Returns `None` when the model is not in the price table (see
/// `price_for`); callers render an unpriced bucket as a partial `—`.
pub fn cost_for(model: &str, tokens: &ModelTokens) -> Option<f64> {
    let p = price_for(model)?;
    Some(
        tokens.input as f64 * p.input
            + tokens.output as f64 * p.output
            + tokens.cache_create as f64 * p.cache_write
            + tokens.cache_read as f64 * p.cache_read,
    )
}
