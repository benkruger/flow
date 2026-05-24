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
//!   preceding byte is `:` AND the match begins with `//` (the
//!   scheme delimiter), so plain colon-prefixed paths like
//!   `time:/etc/hosts` still reach the validator.
//! - `is_safe_path_candidate` — positive validator per
//!   `.claude/rules/external-input-path-construction.md`.
//! - `validate_agent_prompt` — the parent-side entry point.
//!   Composes the helpers, applies the byte cap, resolves
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
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

/// Maximum bytes of the Agent tool's `prompt` field this module
/// inspects. 1 MB comfortably covers every prompt the parent model
/// produces in practice (typical Review-phase agent prompts run
/// 5-30 KB, the largest observed compose review findings plus a
/// full diff at ~200 KB). The cap exists per
/// `.claude/rules/external-input-path-construction.md` so a
/// corrupted or maliciously-large `tool_input.prompt` cannot OOM
/// the hook.
pub const AGENT_PROMPT_BYTE_CAP: usize = 1_048_576;

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

/// Lexically normalize a path by resolving `..` components against
/// the input itself. No filesystem access — `Path::canonicalize`
/// is deliberately NOT used per the Constructor Invariant Audit
/// (it would touch the disk and could surface a permission prompt
/// on a dangling symlink target).
///
/// `Path::components()` automatically normalizes `Component::CurDir`
/// out of non-leading positions, and production callers pass only
/// absolute paths (worktree_root from `compute_worktree_root`, or
/// `worktree.join(...)` joined results, or absolute candidate
/// paths). The match therefore only needs to handle `ParentDir`
/// and the catch-all normal/root components.
///
/// A root-adjacent `ParentDir` (where nothing remains to pop) is
/// discarded: `is_safe_path_candidate` rejects leading `..` and
/// interior `/../` upstream, so the only `ParentDir` that can reach
/// here is a trailing one whose parent pops cleanly — and a token
/// like `/..` normalizes to `/`, which fails the worktree-prefix
/// check identically to any other out-of-worktree path.
fn normalize_path_lexical(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Validate the `prompt` field of an Agent tool call.
///
/// Returns `(allowed, message)`. Message is empty on allow.
///
/// Skipped silently when:
/// - `flow_active` is false (outside a FLOW worktree)
/// - `prompt` is `None` or empty
///
/// Otherwise extracts path candidates, runs the safety validator
/// on each, resolves relative candidates against `worktree_root`,
/// lexically normalizes the result, and rejects any candidate
/// whose normalized form does not start with `worktree_root`. The
/// `prompt` is sliced at `AGENT_PROMPT_BYTE_CAP` along a UTF-8
/// char boundary BEFORE the regex sweep so unbounded input cannot
/// produce unbounded I/O.
pub fn validate_agent_prompt(
    prompt: Option<&str>,
    worktree_root: &Path,
    flow_active: bool,
) -> (bool, String) {
    if !flow_active {
        return (true, String::new());
    }
    let prompt = match prompt {
        Some(p) if !p.is_empty() => p,
        _ => return (true, String::new()),
    };

    let sliced = if prompt.len() <= AGENT_PROMPT_BYTE_CAP {
        prompt
    } else {
        let mut end = AGENT_PROMPT_BYTE_CAP;
        while end > 0 && !prompt.is_char_boundary(end) {
            end -= 1;
        }
        &prompt[..end]
    };

    let candidates = extract_path_candidates(sliced);
    let worktree_norm = normalize_path_lexical(worktree_root);
    for candidate in candidates {
        if !is_safe_path_candidate(&candidate) {
            return (
                false,
                format!(
                    "BLOCKED: Agent prompt contains malformed path token `{}`. \
                     Remove traversal segments and NUL bytes from the prompt.",
                    candidate
                ),
            );
        }
        let candidate_path = Path::new(&candidate);
        let resolved = if candidate_path.is_absolute() {
            candidate_path.to_path_buf()
        } else {
            worktree_root.join(&candidate)
        };
        let resolved_norm = normalize_path_lexical(&resolved);
        if !resolved_norm.starts_with(&worktree_norm) {
            return (
                false,
                format!(
                    "BLOCKED: Agent prompt references path `{}` outside the worktree `{}`. \
                     Out-of-worktree paths surface Claude Code permission prompts in \
                     autonomous flows; drop the requirement from the prompt instead of \
                     redirecting the agent toward a different out-of-worktree path. See \
                     .claude/rules/cognitive-isolation.md \"Context Budget + Truncation \
                     Recovery\".",
                    candidate,
                    worktree_root.display()
                ),
            );
        }
    }
    (true, String::new())
}

/// Extract path-shape substrings from a prompt body.
///
/// Pure tokenizer with no filesystem access. For every match of the
/// path regex, applies a byte-boundary check on the preceding byte:
///
/// - Alphanumeric / `.` / `_` / `-` preceding → mid-token, skip.
/// - `:` preceding AND match begins with `//` → URL scheme marker
///   (`http://`, `https://`, `file://`, `gs://`), skip. Plain
///   `:`-preceded paths without the `//` prefix (e.g.,
///   `time:/etc/hosts`) reach the validator.
///
/// Otherwise the match is captured as a candidate. The result vector
/// preserves match order. Duplicates are NOT deduplicated — the
/// downstream validator runs on each candidate individually.
pub fn extract_path_candidates(prompt: &str) -> Vec<String> {
    let bytes = prompt.as_bytes();
    let mut out = Vec::new();
    for m in path_regex().find_iter(prompt) {
        let start = m.start();
        if start > 0 {
            let prev = bytes[start - 1];
            if prev.is_ascii_alphanumeric() || prev == b'.' || prev == b'_' || prev == b'-' {
                continue;
            }
            // URL scheme post-filter: a `:` immediately before the
            // match is a URL boundary ONLY when the match begins
            // with `//` (the scheme-delimiter shape of
            // `https://`, `file://`, `gs://`). Plain `:`-preceded
            // paths like `time:/etc/hosts` or `log:/etc/passwd`
            // are not URL schemes and must reach the validator —
            // the prior filter rejected every `:`-preceded match
            // unconditionally, which let a model bypass the
            // worktree prefix check by composing prompts like
            // `Read time:/etc/hosts`. The remaining URL coverage
            // still rejects `https://example.com/path` because
            // the candidate after the colon begins with `//`.
            if prev == b':' && m.as_str().as_bytes().get(1) == Some(&b'/') {
                continue;
            }
        }
        out.push(m.as_str().to_string());
    }
    out
}
