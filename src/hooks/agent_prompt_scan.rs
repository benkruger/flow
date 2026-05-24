//! Parent-side Agent tool prompt-body scan.
//!
//! Closes the bypass surface where the parent model can route a
//! sub-agent toward out-of-worktree paths by embedding the path
//! verbatim in the Agent tool's `prompt` field. The sub-agent has its
//! own per-tool gates, but those gates run inside the child session;
//! the parent-side scan rejects the Agent call before the child
//! starts so an autonomous flow cannot silently surface a Claude Code
//! permission prompt for a Read on `~/.config/...` or any other
//! out-of-worktree target.
//!
//! Three helpers compose into the public entry point:
//!
//! - `extract_path_candidates` — pure tokenizer that pulls path-shape
//!   substrings out of arbitrary prompt prose. Matches an anchored
//!   regex (`[/.][A-Za-z0-9_./-]{2,}`), then runs a byte-boundary
//!   check on the preceding byte so option-flag pairs (`-l/--long`)
//!   and intra-token slashes do not produce false candidates. URL
//!   shapes (`https://example.com/path`) are filtered when the
//!   preceding byte is `:` — the scheme marker.
//! - `is_safe_path_candidate` — positive validator per
//!   `.claude/rules/external-input-path-construction.md` (added in
//!   Task 4).
//! - `validate_agent_prompt` — the parent-side entry point (added in
//!   Task 6). Composes the helpers, applies the byte cap, resolves
//!   relative candidates against the worktree root, lexically
//!   normalizes the result (no disk touch), and prefix-compares
//!   against the worktree root.
//!
//! The `Constructor Invariant Audit` for this module per
//! `.claude/rules/extract-helper-refactor.md`:
//! `Regex::captures`/`find_iter` return `Option`/`Iterator`,
//! `Path::join` is infallible, `str::split` is non-panicking, and the
//! validator helper is a pure predicate. No `Path::canonicalize` call
//! reaches the filesystem — every path comparison runs on lexically
//! normalized components.

use crate::hooks::transcript_walker::normalize_gate_input;
use regex::Regex;
use std::sync::OnceLock;

/// Compiled regex matching path-shape substrings.
///
/// The pattern requires a leading `/` or `.` followed by two or more
/// path characters (alphanumeric, `.`, `/`, `_`, `-`). The minimum
/// length of three characters keeps single-char anomalies (`./` /
/// `..`) from producing standalone candidates — those are caught
/// either by `is_safe_path_candidate` or by being too short for the
/// regex.
fn path_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"[/.][A-Za-z0-9_./\-]{2,}").expect("hard-coded literal regex compiles")
    })
}

/// Extract path-shape substrings from a prompt body.
///
/// Pure tokenizer with no filesystem access. For every match of the
/// path regex, applies a byte-boundary check on the preceding byte:
///
/// - Alphanumeric / `.` / `_` / `-` preceding → mid-token, skip.
/// - `:` preceding → URL scheme marker (`http:`, `https:`, `file:`),
///   skip.
///
/// Otherwise the match is captured as a candidate. The result vector
/// preserves match order. Duplicates are NOT deduplicated — the
/// downstream validator runs on each candidate individually.
/// Positive validator for a path-shape candidate.
///
/// Per `.claude/rules/external-input-path-construction.md` and
/// `.claude/rules/security-gates.md` "Normalize Before Comparing".
///
/// Rejects:
/// - Empty input (after `normalize_gate_input` trim).
/// - Embedded NUL bytes (defeats syscall path comparison in
///   implementation-defined ways — checked on the raw input).
/// - Leading `..` segment (`../foo`, `..`) — path traversal.
/// - Interior `/../` traversal.
///
/// Accepts every other shape: absolute paths, relative paths with
/// `.`/`-`/`_`-bearing segments, and surrounding whitespace
/// (normalized away by `normalize_gate_input` before the
/// empty-after-trim check).
pub fn is_safe_path_candidate(s: &str) -> bool {
    if s.contains('\0') {
        return false;
    }
    let normalized = normalize_gate_input(s);
    if normalized.is_empty() {
        return false;
    }
    if s.trim().starts_with("..") {
        return false;
    }
    if s.contains("/../") {
        return false;
    }
    true
}

pub fn extract_path_candidates(prompt: &str) -> Vec<String> {
    let bytes = prompt.as_bytes();
    let mut out = Vec::new();
    for m in path_regex().find_iter(prompt) {
        let start = m.start();
        if start > 0 {
            let prev = bytes[start - 1];
            if prev.is_ascii_alphanumeric()
                || prev == b'.'
                || prev == b'_'
                || prev == b'-'
                || prev == b':'
            {
                continue;
            }
        }
        out.push(m.as_str().to_string());
    }
    out
}
