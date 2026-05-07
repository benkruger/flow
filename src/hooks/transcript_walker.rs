//! Shared backward walker over the persisted Claude Code transcript
//! JSONL. Two consumers share this module:
//!
//! 1. `src/hooks/validate_skill.rs` — Layer 1 of the user-only skill
//!    enforcement chain. Calls
//!    `last_user_message_invokes_skill(transcript_path, skill)` to
//!    decide whether the most recent user turn typed the matching
//!    `<command-name>/<skill></command-name>` slash command. Without
//!    a match, the model invocation of a user-only skill is blocked.
//! 2. `src/hooks/validate_ask_user.rs` — Layer 2 carve-out. Calls
//!    `most_recent_skill_in_user_only_set(transcript_path)` to allow
//!    `AskUserQuestion` confirmation prompts during in-progress
//!    autonomous phases when the most recent assistant Skill tool
//!    invocation targets a user-only skill (`/flow:flow-abort` etc.).
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
//! paths, relative paths, and paths that do not normalize under
//! `<home>/.claude/projects/`. Reads are capped at
//! `TRANSCRIPT_BYTE_CAP` bytes via `BufReader::new(file.take(cap))`
//! so a hand-crafted oversized transcript cannot exhaust process
//! memory mid-session.
//!
//! ## JSONL turn shape
//!
//! Mirrors `crate::window_snapshot::read_transcript`. Each line is
//! a JSON object with a top-level `type` field whose value is
//! `"user"` or `"assistant"`. The line's payload lives under
//! `message.content` — a string for user turns, an array of
//! content blocks (`{"type": "tool_use", "name": "Skill",
//! "input": {"skill": "..."}}`, etc.) for assistant turns. Lines
//! that fail to parse as JSON are skipped silently.
//!
//! Tests live at `tests/transcript_walker.rs` (top-level rather
//! than the mirror `tests/hooks/transcript_walker.rs`) per the
//! deviation log entry on this branch — adding a `[[test]]`
//! stanza for the subdirectory test was blocked by the
//! validate-worktree-paths shared-config hook in autonomous mode.

use std::fs::File;
use std::io::{BufReader, Read};
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
pub const USER_ONLY_SKILLS: &[&str] = &[
    "flow:flow-abort",
    "flow:flow-reset",
    "flow:flow-release",
    "flow:flow-prime",
];

/// Maximum bytes read from the transcript file. Mirrors
/// `crate::window_snapshot::TRANSCRIPT_BYTE_CAP` (50 MB) so the
/// walker bounds I/O the same way the snapshot reader does. A
/// long-autonomous-flow transcript can exceed 100 MB; reading
/// every byte on every Skill / AskUserQuestion tool call would
/// dominate session latency. Capping at 50 MB still covers more
/// than the most recent ~10,000 turns of a typical session,
/// which is far more than the lookback the predicates need.
pub const TRANSCRIPT_BYTE_CAP: u64 = 50 * 1024 * 1024;

/// Return `true` when the most recent user-role turn in the
/// transcript at `path` contains a `<command-name>/<skill></command-name>`
/// substring. Returns `false` on any read, parse, or validation
/// failure (fail-open). Skips assistant turns until a user turn is
/// found; checks ONLY that user turn — older user turns do not
/// count.
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
    let lines = match read_capped(path) {
        Some(s) => s,
        None => return false,
    };
    let needle = format!("<command-name>/{}</command-name>", skill);
    for line in lines.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let turn: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let turn_type = turn.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if turn_type == "user" {
            return trimmed.contains(&needle);
        }
    }
    false
}

/// Return `true` when the most recent assistant tool_use Skill
/// invocation BEFORE the next-younger user turn targets a skill in
/// `USER_ONLY_SKILLS`. Returns `false` when no Skill invocation
/// occurred since the most recent user turn, when the most recent
/// Skill invocation targets a non-user-only skill, or on any read /
/// parse / validation failure (fail-open).
///
/// "Before the next-younger user turn" means: walking backward from
/// the file's tail, the walker stops the moment it encounters a
/// user turn. Older Skill invocations beyond that boundary are
/// invisible — only Skill calls fired since the most recent user
/// turn count.
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
        let turn_type = turn.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if turn_type == "user" {
            return false;
        }
        if turn_type != "assistant" {
            continue;
        }
        if let Some(skill) = extract_skill_invocation(&turn) {
            return USER_ONLY_SKILLS.contains(&skill.as_str());
        }
        // Assistant turn produced no Skill tool_use — keep walking
        // backward toward an older Skill call or the user boundary.
    }
    false
}

/// Read up to `TRANSCRIPT_BYTE_CAP` bytes from `path` as a UTF-8
/// String. Returns `None` on `File::open` error or non-UTF-8
/// content. Caps the read via `BufReader::new(file.take(cap))` per
/// `.claude/rules/external-input-path-construction.md` Rule 3 so a
/// hand-crafted oversized transcript cannot OOM the process.
fn read_capped(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file.take(TRANSCRIPT_BYTE_CAP));
    let mut buf = String::new();
    reader.read_to_string(&mut buf).ok()?;
    Some(buf)
}

/// Walk an assistant turn's `message.content` array and return the
/// first `tool_use` block whose `name == "Skill"`. The block's
/// `input.skill` is returned as a String. Returns `None` when the
/// content array is missing, non-array, contains no Skill tool_use,
/// or the Skill block lacks an `input.skill` string.
fn extract_skill_invocation(turn: &Value) -> Option<String> {
    let content = turn.get("message")?.get("content")?.as_array()?;
    for block in content {
        let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if block_type != "tool_use" {
            continue;
        }
        let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name != "Skill" {
            continue;
        }
        let skill = block
            .get("input")
            .and_then(|v| v.get("skill"))
            .and_then(|v| v.as_str())?;
        return Some(skill.to_string());
    }
    None
}
