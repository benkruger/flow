//! Sentinel-based plan extractor for `bin/flow plan-from-issue`.
//!
//! The `plan-from-issue` subcommand replaces the heuristic
//! `plan-extract` path with a five-line scan over an issue body's
//! `<!-- FLOW-PLAN-BEGIN -->` / `<!-- FLOW-PLAN-END -->` markers.
//! The bytes between the first BEGIN and the first END after it are
//! the plan, returned verbatim.

use std::error::Error;
use std::fmt;

/// Maximum issue-body size accepted by `extract_plan`.
///
/// 1 MiB bounds the worst-case malicious or runaway issue body so a
/// single oversized fetch cannot exhaust process memory. Issue bodies
/// larger than the cap reject before any marker scan runs.
pub const PLAN_BODY_BYTE_CAP: usize = 1_048_576;

const BEGIN_MARKER: &str = "<!-- FLOW-PLAN-BEGIN -->";
const END_MARKER: &str = "<!-- FLOW-PLAN-END -->";

/// Reasons `extract_plan` rejects an issue body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractError {
    /// Neither sentinel marker appears in the body.
    MarkersMissing,
    /// One marker is present without its pair, or `END` appears with
    /// no following `BEGIN` predecessor.
    MarkersMalformed,
    /// Markers delimit a region that is empty or whitespace-only.
    Empty,
    /// Body exceeds `PLAN_BODY_BYTE_CAP` bytes.
    TooLarge,
}

impl fmt::Display for ExtractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            ExtractError::MarkersMissing => {
                "issue body contains neither FLOW-PLAN-BEGIN nor FLOW-PLAN-END marker"
            }
            ExtractError::MarkersMalformed => {
                "issue body has an unmatched or out-of-order FLOW-PLAN marker pair"
            }
            ExtractError::Empty => "issue body has empty content between FLOW-PLAN markers",
            ExtractError::TooLarge => "issue body exceeds the 1 MiB cap",
        };
        f.write_str(msg)
    }
}

impl Error for ExtractError {}

/// Extract the plan content delimited by FLOW-PLAN markers.
///
/// Returns the slice between the first `<!-- FLOW-PLAN-BEGIN -->` and
/// the first `<!-- FLOW-PLAN-END -->` after it. Rejects bodies with
/// missing markers, malformed pairs, empty content, or sizes over
/// `PLAN_BODY_BYTE_CAP`.
pub fn extract_plan(body: &str) -> Result<&str, ExtractError> {
    if body.len() > PLAN_BODY_BYTE_CAP {
        return Err(ExtractError::TooLarge);
    }

    let begin_pos = body.find(BEGIN_MARKER);
    let has_end = body.contains(END_MARKER);

    let begin_idx = match begin_pos {
        Some(i) => i,
        None => {
            return if has_end {
                Err(ExtractError::MarkersMalformed)
            } else {
                Err(ExtractError::MarkersMissing)
            };
        }
    };

    if !has_end {
        return Err(ExtractError::MarkersMalformed);
    }

    let plan_start = begin_idx + BEGIN_MARKER.len();
    let end_idx = match body[plan_start..].find(END_MARKER) {
        Some(rel) => plan_start + rel,
        None => return Err(ExtractError::MarkersMalformed),
    };

    let content = &body[plan_start..end_idx];
    if content.trim().is_empty() {
        return Err(ExtractError::Empty);
    }
    Ok(content)
}
