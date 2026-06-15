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
//!   against the worktree root. Candidates under this flow's own
//!   `<project_root>/.flow-states/<branch>/` subtree are carved out
//!   (allowed) so engaging the worktree gate does not hard-block the
//!   Review sub-agent launch, whose prompt carries the
//!   substantive-diff path there. The carve-out is scoped to the
//!   current flow's branch subdirectory, NOT the whole
//!   `.flow-states/` root that every concurrent flow shares.
//!   A second carve-out admits exactly the resolved
//!   `<plugin_root>/bin/flow` path (`plugin_root` is the
//!   caller-supplied `CLAUDE_PLUGIN_ROOT` value): the parent
//!   substitutes that resolved absolute path (from `bin/flow
//!   plugin-bin-flow`) into the adversarial / ci-fixer agent prompts,
//!   and it is outside the worktree. The carve-out is scoped to the
//!   single `bin/flow` path the resolver emits — NOT the whole plugin
//!   subtree, since a non-`bin/flow` plugin path is still
//!   out-of-worktree. The plugin-root value is validated non-empty and
//!   absolute and fails closed (no carve-out) when it is `None`, empty,
//!   or non-absolute. The caller — not this module — reads the env
//!   value so the carve-out stays env-race-free unit-testable.
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
///
/// `normalize_gate_input` (NUL strip + trim) is defense-in-depth:
/// this is a `pub` security predicate, and a future non-tokenizer
/// caller may pass raw, un-tokenized strings. Candidates produced by
/// `extract_path_candidates` already exclude whitespace and NUL by
/// the regex character class, so the normalization is a no-op for the
/// tokenizer path but keeps the validator robust for any direct
/// caller.
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
/// whose normalized form does not start with `worktree_root` —
/// EXCEPT candidates that normalize under this flow's own
/// `<project_root>/.flow-states/<branch>/` subtree, which are allowed
/// via the `.flow-states/` carve-out. project_root and branch are
/// derived by reusing `compute_worktree_paths` on `worktree_root` (no
/// disk touch); the carve-out is scoped to the current flow's branch
/// subdirectory — not the whole `.flow-states/` root, which every
/// concurrent flow shares — so engaging the worktree gate (the hook
/// now resolves the worktree from the payload cwd) does not
/// hard-block the Review sub-agent launch, whose prompt carries the
/// substantive-diff path under `.flow-states/<branch>/`. The `prompt`
/// is sliced at `AGENT_PROMPT_BYTE_CAP` along a UTF-8 char boundary
/// BEFORE the regex sweep so unbounded input cannot produce
/// unbounded I/O.
///
/// `plugin_root` is the caller-supplied `CLAUDE_PLUGIN_ROOT` value
/// (the caller reads the env so this function stays env-race-free
/// unit-testable). A candidate that normalizes under the resolved
/// `<plugin_root>/bin/flow` path is allowed — the parent substitutes
/// that path into the adversarial / ci-fixer agent prompts, and it is
/// outside the worktree. The carve-out admits only the `bin/flow` path
/// the resolver emits, not the whole plugin subtree, and fails closed
/// (candidate stays blocked) when `plugin_root` is `None`, empty after
/// trimming, or non-absolute.
pub fn validate_agent_prompt(
    prompt: Option<&str>,
    worktree_root: &Path,
    flow_active: bool,
    plugin_root: Option<&str>,
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
            // `.flow-states/` carve-out: the reviewer launch passes
            // the substantive-diff path (under
            // `<project_root>/.flow-states/<branch>/`, outside the
            // worktree) in the agent prompt. Once the hook resolves the
            // worktree from the payload cwd, engaging the gate would
            // otherwise hard-block every Review sub-agent launch. Allow
            // candidates that normalize under THIS flow's own
            // `<project_root>/.flow-states/<branch>/` subtree — scoped
            // to the current branch, NOT the whole `.flow-states/` root
            // that every concurrent flow shares. project_root and the
            // branch are derived by reusing `compute_worktree_paths` on
            // the worktree_root (no disk touch); its rightmost-occurrence
            // `rfind` means a project_root that itself contains
            // `.worktrees/` resolves correctly. When worktree_root lacks
            // the `/.worktrees/` anchor the derivation yields `None` and
            // the carve-out does not apply — the candidate stays blocked.
            let wt_str = worktree_root.to_string_lossy();
            if let Some((project_root, wt_root)) =
                crate::flow_paths::compute_worktree_paths(&wt_str)
            {
                // `wt_root` is `<project_root>/.worktrees/<branch>`; the
                // branch is the tail after the anchor. Slicing (not
                // `Path::file_name`) keeps the derivation branchless so
                // the 100% coverage gate has no unreachable arm.
                let branch = wt_root
                    .strip_prefix(project_root)
                    .and_then(|rest| rest.strip_prefix("/.worktrees/"))
                    .unwrap_or("");
                let flow_states_branch = normalize_path_lexical(
                    &Path::new(project_root).join(".flow-states").join(branch),
                );
                if resolved_norm.starts_with(&flow_states_branch) {
                    continue;
                }
            }
            // Plugin-root carve-out: the parent substitutes the
            // resolved absolute plugin `bin/flow` path (from
            // `bin/flow plugin-bin-flow`) into the adversarial /
            // ci-fixer agent prompts. That path lives outside the
            // worktree, so without this carve-out engaging the worktree
            // gate would hard-block every such launch. Admit ONLY the
            // resolved `<plugin_root>/bin/flow` path — the single shape
            // the resolver ever emits — NOT the whole plugin subtree: a
            // non-`bin/flow` path under the plugin root is still
            // out-of-worktree (in a target project the plugin lives
            // outside the project) and must stay blocked. The env value
            // is hygiene-stripped of NUL bytes and surrounding
            // whitespace (matching `plugin_bin_flow::run_impl`), but the
            // prefix comparison is case-preserving (paths are
            // case-sensitive) — both sides are compared as
            // lexically-normalized paths. Fail closed (no carve-out,
            // candidate stays blocked) when `plugin_root` is `None`,
            // empty after trimming, or non-absolute, per
            // `.claude/rules/security-gates.md` "Fail Closed" and the
            // env-validation requirement.
            if let Some(pr) = plugin_root {
                let pr_clean = pr.replace('\0', "");
                let pr_clean = pr_clean.trim();
                if !pr_clean.is_empty() {
                    let pr_path = Path::new(pr_clean);
                    if pr_path.is_absolute() {
                        let pr_bin_flow = normalize_path_lexical(&pr_path.join("bin").join("flow"));
                        if resolved_norm.starts_with(&pr_bin_flow) {
                            continue;
                        }
                    }
                }
            }
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
