//! Crate-wide error type.
//!
//! Tests live at `tests/error.rs` per
//! `.claude/rules/test-placement.md` — no inline `#[cfg(test)]` in
//! this file.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum FlowError {
    #[error("State file not found: {0}")]
    NoState(String),

    #[error("Phase guard: {0}")]
    PhaseGuard(String),

    #[error("Git error: {0}")]
    Git(String),

    #[error("GitHub error: {0}")]
    GitHub(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
