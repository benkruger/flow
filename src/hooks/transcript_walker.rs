//! Shared backward walker over the persisted Claude Code transcript
//! JSONL. Two consumers share this module:
//!
//! 1. `src/hooks/validate_skill.rs` — Layer 1 of the user-only skill
//!    enforcement chain. Calls
//!    `last_user_message_invokes_skill(transcript_path, skill, home)`
//!    to decide whether the most recent user turn typed the matching
//!    `<command-name>/<skill></command-name>` slash command. Without
//!    a match, the model invocation of a user-only skill is blocked.
//! 2. `src/hooks/validate_ask_user.rs` — Layer 2 carve-out. Calls
//!    `most_recent_skill_in_user_only_set(transcript_path, home)` to
//!    allow `AskUserQuestion` confirmation prompts during in-progress
//!    autonomous phases when the most recent assistant turn fires a
//!    Skill tool_use targeting a user-only skill.
//!
//! Both helpers are read-only over a JSONL transcript file. They
//! never mutate state, never spawn subprocesses, and fail-open
//! (return `false`) on any I/O, parse, or validation error. The
//! `false` return surfaces as "no match" at every consumer, which
//! routes through to the consumer's safe default (block for Layer
//! 1, fall through to existing autonomous block for Layer 2).
//!
//! ## Validation contract
//!
//! Per `.claude/rules/external-input-path-construction.md`, the
//! `path` argument is validated through
//! `crate::window_snapshot::is_safe_transcript_path` before any
//! filesystem read. The validator rejects empty paths, NUL-byte
//! paths, relative paths, paths containing a `..` component, and
//! paths that do not normalize under `<home>/.claude/projects/`.
//!
//! ## Tail-bounded read
//!
//! `read_capped` seeks to the LAST `TRANSCRIPT_BYTE_CAP` bytes of
//! the file and reads forward to EOF. The walker iterates the
//! resulting buffer in reverse line order, so the file's tail (the
//! most recent turns) is always visible regardless of total file
//! size. Reading from the head would silently omit the most recent
//! turns on transcripts larger than the cap, defeating the entire
//! enforcement chain on long autonomous flows.
//!
//! ## Gate normalization
//!
//! Per `.claude/rules/security-gates.md` "Normalize Before
//! Comparing", every gate-relevant string input is normalized
//! through `normalize_gate_input` (NUL strip + trim + ASCII
//! lowercase) before comparison. This applies to `skill` values,
//! transcript-extracted skill names, and turn-type discriminants.
//! Both sides of every comparison run through the same normalizer.
//!
//! ## Slash-command anchoring
//!
//! Layer 1's user-turn check parses `message.content` as a string
//! and requires the `<command-name>/<skill></command-name>`
//! marker at the START of the trimmed content. A user typing
//! "what does <command-name>/flow:flow-abort</command-name> do?"
//! produces a content string where the marker appears mid-text —
//! that is prose mention, not a slash-command invocation, and is
//! rejected. Tool-result-wrapped user turns (where `content` is an
//! array of blocks rather than a string) are also rejected
//! because echoed assistant text in a tool_result would otherwise
//! authorize invocation of a user-only skill.
//!
//! ## JSONL turn shape
//!
//! Mirrors `crate::window_snapshot::read_transcript`. Each line is
//! a JSON object with a top-level `type` field whose value is
//! `"user"` or `"assistant"`. The line's payload lives under
//! `message.content` — a string for user-typed turns, an array of
//! content blocks (`{"type": "tool_use", "name": "Skill",
//! "input": {"skill": "..."}}`, etc.) for assistant turns and
//! tool_result-wrapped user turns. Lines that fail to parse as
//! JSON are skipped silently.
//!
//! Tests live at `tests/transcript_walker.rs` (top-level rather
//! than the mirror `tests/hooks/transcript_walker.rs`) per the
//! deviation log entry on this branch — adding a `[[test]]`
//! stanza for the subdirectory test was blocked by the
//! validate-worktree-paths shared-config hook in autonomous mode.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;

use crate::window_snapshot::is_safe_transcript_path;

/// The four FLOW skills the model must never invoke. Each requires
/// explicit user initiative — typing `/flow:flow-<name>` directly —
/// because the action is destructive (`flow-abort`, `flow-reset`),
/// resource-shipping (`flow-release`), or environment-mutating
/// (`flow-prime`).
///
/// Re-exported by `validate_skill` and `validate_ask_user` so a
/// single authoritative list governs both Layer 1 (block model
/// invocation) and Layer 2 (carve-out for confirmation prompts).
/// Entries are stored ASCII-lowercased; gate comparisons normalize
/// caller input through `normalize_gate_input` before checking
/// membership.
pub const USER_ONLY_SKILLS: &[&str] = &[
    "flow:flow-abort",
    "flow:flow-reset",
    "flow:flow-release",
    "flow:flow-prime",
];

/// Maximum bytes read from the transcript file (50 MB). Bounds I/O
/// across long autonomous flows — a session transcript can exceed
/// 100 MB, and reading every byte on every Skill /
/// AskUserQuestion tool call would dominate session latency.
/// `read_capped` reads the LAST `TRANSCRIPT_BYTE_CAP` bytes (not
/// the first), so the most recent ~10,000 turns are always
/// reachable regardless of total file size.
pub const TRANSCRIPT_BYTE_CAP: u64 = 50 * 1024 * 1024;

/// Normalize a gate-relevant string for comparison: strip NUL
/// bytes, trim leading/trailing whitespace, and ASCII-lowercase.
/// Per `.claude/rules/security-gates.md` "Normalize Before
/// Comparing", every gate input runs through this helper before
/// comparison so a NUL-padded, whitespace-padded, or case-variant
/// caller cannot bypass the membership check.
pub fn normalize_gate_input(s: &str) -> String {
    s.replace('\0', "").trim().to_ascii_lowercase()
}

/// Return `true` when the most recent user-role turn in the
/// transcript at `path` invokes `skill` as a Claude Code slash
/// command. Returns `false` on any read, parse, or validation
/// failure (fail-open).
///
/// Slash-command anchoring: the marker
/// `<command-name>/<skill></command-name>` must appear at the
/// START of the trimmed `message.content` string. A user typing
/// the marker mid-prose, or a tool_result-wrapped user turn whose
/// content is an array of blocks (containing assistant-echoed
/// text), does NOT satisfy the check.
///
/// `home` is passed in (rather than read from `$HOME` internally)
/// so the validator can run against a fixture-controlled prefix in
/// tests without `set_var` env races. Hook callers
/// (`validate_skill::run`, `validate_ask_user::run`) read `$HOME`
/// via `crate::window_snapshot::home_dir_or_empty()` and pass it
/// through.
pub fn last_user_message_invokes_skill(path: &Path, skill: &str, home: &Path) -> bool {
    if !is_safe_transcript_path(path, home) {
        return false;
    }
    let skill_norm = normalize_gate_input(skill);
    if skill_norm.is_empty() {
        return false;
    }
    let lines = match read_capped(path) {
        Some(s) => s,
        None => return false,
    };
    let needle = format!("<command-name>/{}</command-name>", skill_norm);
    for line in lines.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let turn: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let turn_type =
            normalize_gate_input(turn.get("type").and_then(|v| v.as_str()).unwrap_or(""));
        if turn_type != "user" {
            continue;
        }
        // Stop at the most recent user turn. Older user turns are
        // invisible regardless of what they contain.
        let content = match turn.get("message").and_then(|m| m.get("content")) {
            Some(c) => c,
            None => return false,
        };
        // Only string content authorizes — tool_result-wrapped user
        // turns (content as array) carry assistant-generated text
        // that must not be treated as user intent.
        let content_str = match content.as_str() {
            Some(s) => s,
            None => return false,
        };
        let content_norm = content_str.trim_start().to_ascii_lowercase();
        return content_norm.starts_with(&needle);
    }
    false
}

/// Return `true` when the most recent assistant turn in the
/// transcript fires at least one Skill tool_use whose `input.skill`
/// (after normalization) is in `USER_ONLY_SKILLS`. Returns `false`
/// when the most recent turn is a user turn, when the most recent
/// assistant turn carries no user-only Skill invocations, or on
/// any read / parse / validation failure (fail-open).
///
/// Walking backward from the file's tail, the walker stops at the
/// first user turn or the first assistant turn that carries any
/// Skill tool_use. Older turns beyond either boundary are
/// invisible. Multi-tool assistant turns are scanned in full — a
/// turn fires `[Bash, Skill(flow:flow-status), Skill(flow:flow-abort)]`
/// satisfies the check because the user-only Skill is present in
/// the same turn.
///
/// `home` is passed in for the same testability reason as
/// `last_user_message_invokes_skill`.
pub fn most_recent_skill_in_user_only_set(path: &Path, home: &Path) -> bool {
    if !is_safe_transcript_path(path, home) {
        return false;
    }
    let lines = match read_capped(path) {
        Some(s) => s,
        None => return false,
    };
    for line in lines.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let turn: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let turn_type =
            normalize_gate_input(turn.get("type").and_then(|v| v.as_str()).unwrap_or(""));
        if turn_type == "user" {
            return false;
        }
        if turn_type != "assistant" {
            continue;
        }
        let skills = extract_skill_invocations(&turn);
        if skills.is_empty() {
            // Assistant turn produced no Skill tool_use — keep
            // walking backward toward an older Skill call or the
            // user boundary.
            continue;
        }
        return skills
            .iter()
            .map(|s| normalize_gate_input(s))
            .any(|s| USER_ONLY_SKILLS.contains(&s.as_str()));
    }
    false
}

/// Read the LAST `TRANSCRIPT_BYTE_CAP` bytes of `path` as a UTF-8
/// String. Returns `None` on `File::open` error or non-UTF-8
/// content.
///
/// The function seeks to `max(0, file_len - TRANSCRIPT_BYTE_CAP)`
/// and reads forward to EOF. The buffer is the file's tail, which
/// is what the backward walker needs — reading from the head
/// silently omits recent turns on transcripts larger than the cap.
/// A partial JSONL line at the buffer's start (mid-line truncation
/// at the seek point) fails to parse and is silently skipped by
/// the walker's `Err(_) => continue` branch.
///
/// `metadata()` and `seek()` on a freshly-opened regular file are
/// genuinely TOCTOU-only failure modes per the
/// `.claude/rules/external-input-path-construction.md` "No `.expect()`
/// on Filesystem Reads" carve-out — `.expect()` is acceptable here
/// because the `.ok()?` branch above on `File::open` is the only
/// reachable failure surface for the open-file-then-stat-then-seek
/// sequence. A test cannot reproduce metadata or seek failure on a
/// freshly-opened regular file without root-level interference.
fn read_capped(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let file_len = file
        .metadata()
        .expect("metadata succeeds on freshly-opened regular file (TOCTOU-only)")
        .len();
    let start = file_len.saturating_sub(TRANSCRIPT_BYTE_CAP);
    file.seek(SeekFrom::Start(start))
        .expect("seek to non-negative absolute offset succeeds on regular file (TOCTOU-only)");
    let mut reader = BufReader::new(file);
    let mut buf = String::new();
    reader.read_to_string(&mut buf).ok()?;
    Some(buf)
}

/// Walk an assistant turn's `message.content` array and return
/// every `tool_use` block whose `name == "Skill"` — extracted from
/// `input.skill` as a String. The walker examines all blocks (not
/// just the first), so a multi-tool turn whose user-only Skill
/// appears second or later is still visible to the caller. Returns
/// an empty Vec when the content array is missing, non-array,
/// contains no Skill tool_use, or every Skill block lacks an
/// `input.skill` string.
fn extract_skill_invocations(turn: &Value) -> Vec<String> {
    let content = match turn
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
    {
        Some(c) => c,
        None => return Vec::new(),
    };
    let mut skills = Vec::new();
    for block in content {
        let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if block_type != "tool_use" {
            continue;
        }
        let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name != "Skill" {
            continue;
        }
        if let Some(skill) = block
            .get("input")
            .and_then(|v| v.get("skill"))
            .and_then(|v| v.as_str())
        {
            skills.push(skill.to_string());
        }
    }
    skills
}
