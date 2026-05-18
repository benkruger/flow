//! Shared backward walker over the persisted Claude Code transcript
//! JSONL. Four consumers share this module:
//!
//! 1. `src/hooks/validate_skill.rs` — Layer 1 of the user-only skill
//!    enforcement chain. Calls
//!    `last_user_message_invokes_skill(transcript_path, skill, home)`
//!    to decide whether the most recent user turn typed the matching
//!    `<command-name>/<skill></command-name>` slash command. Without
//!    a match, the model invocation of a user-only skill is blocked.
//! 2. `src/hooks/validate_ask_user.rs` — Layer 2 user-only-skill
//!    carve-out. Calls
//!    `most_recent_skill_in_user_only_set(transcript_path, home)` to
//!    allow `AskUserQuestion` confirmation prompts during in-progress
//!    autonomous phases when the most recent assistant turn fires a
//!    Skill tool_use targeting a user-only skill.
//! 3. `src/hooks/validate_ask_user.rs` — shared-config carve-out.
//!    Calls `recent_edit_blocked_on_shared_config(transcript_path, home)`
//!    to allow `AskUserQuestion` confirmation prompts during
//!    in-progress autonomous phases when the most recent user-role
//!    turn carries a `validate_worktree_paths` shared-config edit
//!    block. The shared-config block's BLOCKED message itself
//!    instructs the model to call `AskUserQuestion` to confirm — the
//!    carve-out lets the prompt fire instead of deadlocking.
//! 4. `src/hooks/validate_pretool.rs` — Layer 10 bootstrap-skill
//!    carve-out for the integration-branch commit gate. Calls
//!    `any_skill_in_set_since_user(transcript_path, home, &[
//!    "flow:flow-start", "flow:flow-prime", "flow-release"])` to
//!    recognize the bootstrap commit windows where the commit runs
//!    while cwd is on the integration branch — flow-start Step 2
//!    (deps repair), flow-prime Step 6 (setup writes), and
//!    flow-release's version-bump commit. The sanctioned parent is
//!    recognized either as an assistant `Skill` tool_use OR as the
//!    user-typed slash-command boundary turn since the most recent
//!    real user turn — `flow:flow-prime` and `flow-release` are
//!    user-only skills that Claude Code records only as user-role
//!    turns, never as assistant `Skill` tool_use, so the user-turn
//!    recognition is required for those two.
//!
//! All helpers are read-only over a JSONL transcript file. They
//! never mutate state, never spawn subprocesses, and fail-open
//! (return `false` / `None`) on any I/O, parse, or validation error.
//! The fail-open return surfaces as "no match" at every consumer,
//! which routes through to the consumer's safe default (block for
//! Layer 1, fall through to existing autonomous block for Layer 2,
//! block for Layer 10 commit-gate carve-outs).
//!
//! ## Validation contract
//!
//! Per `.claude/rules/external-input-path-construction.md`, the
//! `path` argument is validated through
//! `crate::session_metrics::is_safe_transcript_path` before any
//! filesystem read. The validator rejects empty paths, NUL-byte
//! paths, relative paths, paths containing a `..` component, and
//! paths that do not normalize under `<home>/.claude/projects/`.
//!
//! ## Three lookback windows
//!
//! Walkers declare their lookback semantics by name. Three public
//! wrappers route every file read:
//!
//! - `read_full` — uncapped. Loads the entire transcript. Used by
//!   phase-boundary verifiers (`verify_agent_returned_in_phase`)
//!   whose marker may sit arbitrarily far back in a long autonomous
//!   flow's transcript.
//! - `read_recency_window` — capped at `TRANSCRIPT_BYTE_CAP` (50 MB).
//!   Used by per-turn recency walkers (`last_user_message_invokes_skill`,
//!   `most_recent_skill_in_user_only_set`,
//!   `most_recent_user_message_since_skill_action`,
//!   `most_recent_skill_since_user`, `any_skill_in_set_since_user`)
//!   where the marker of interest is among the most recent ~10,000
//!   turns and the hot path must stay bounded.
//! - `read_recent_turns` — capped at `SHARED_CONFIG_BLOCK_BYTE_CAP`
//!   (4 MB). Used by `recent_edit_blocked_on_shared_config` which
//!   only needs the latest assistant tool call and its paired
//!   tool_result.
//!
//! The private `read_capped` is the seek-and-take primitive both
//! `read_recency_window` and `read_recent_turns` wrap. Direct calls
//! to `read_capped` from production code outside this module are
//! forbidden — the contract test
//! `read_capped_only_called_inside_named_helpers` in
//! `tests/hooks/transcript_walker.rs` locks the invariant in, and
//! `.claude/rules/transcript-walker-cap.md` documents the API for
//! future authors.
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
//! and requires the trimmed content to START with one of two
//! emission shapes Claude Code uses for user-typed slash commands:
//! the two-line shape
//! `<command-message><skill></command-message>\n<command-name>/<skill></command-name>`
//! (Claude Code 2.1.140+) OR the legacy one-line shape
//! `<command-name>/<skill></command-name>`. Both shapes are
//! checked via `starts_with` disjunction. A user typing
//! "what does <command-name>/flow:flow-abort</command-name> do?"
//! produces a content string where the marker appears mid-text —
//! that is prose mention, not a slash-command invocation, and is
//! rejected because the prose's leading bytes are neither
//! `<command-message>` nor `<command-name>`. Tool-result-wrapped
//! user turns (where `content` is an array of blocks rather than
//! a string) are also rejected because echoed assistant text in a
//! tool_result would otherwise authorize invocation of a
//! user-only skill.
//!
//! ## JSONL turn shape
//!
//! Mirrors `crate::session_metrics::read_transcript`. Each line is
//! a JSON object with a top-level `type` field whose value is
//! `"user"` or `"assistant"`. The line's payload lives under
//! `message.content` — a string for user-typed turns, an array of
//! content blocks (`{"type": "tool_use", "name": "Skill",
//! "input": {"skill": "..."}}`, etc.) for assistant turns and
//! tool_result-wrapped user turns. Lines that fail to parse as
//! JSON are skipped silently.
//!
//! ## Real vs synthetic user turns
//!
//! Claude Code emits two shapes of synthetic user turns alongside
//! the user-typed prose: tool_result-wrapped turns (array content)
//! and hook-injected feedback turns (string content with
//! `isMeta:true`). Walkers that need to find the most recent REAL
//! user turn must call `is_real_user_turn` and `continue` past
//! synthetic turns rather than stop at them. A walker that stops
//! at any user turn — or filters only on array content — silently
//! fails the moment a Stop-hook refusal lands ahead of the real
//! invocation. See `.claude/rules/transcript-shape.md` for the
//! catalog of synthetic shapes and the mechanical contract each
//! walker must satisfy.
//!
//! ## Imperative vs conversational real user turns
//!
//! Real (non-synthetic) user turns split further into two classes:
//! conversational prose (the user is talking to the model) and
//! imperative slash-command input (the user is invoking a slash
//! command via `<command-name>/<skill></command-name>` or the
//! two-line `<command-message>...</command-message>` shape). The
//! distinction matters only for
//! `most_recent_user_message_since_skill_action`, whose consumer
//! (`stop_continue::check_autonomous_stop`) uses the walker's
//! return to detect whether the user has typed a conversational
//! halt trigger. A slash-command turn is user-direction input,
//! not a conversation — treating it as halt-trigger prose would
//! permanently re-arm `_halt_pending` after every
//! `/flow:flow-continue` and trap the autonomous flow.
//!
//! Within imperative slash commands, `/flow:flow-continue` is the
//! universal resume directive: the walker also watermarks
//! preceding conversational prose to `None` so a user who first
//! paused with prose and then resumed with the slash command sees
//! the next Stop fire Rule 1 (encouraging refusal), not Rule 2 or
//! a fresh conversation pass-through.
//!
//! Every other walker in this module uses real-user-turn as a
//! *boundary* (where to stop scanning) rather than as a
//! *conversation signal* (whether the user wants to talk). The
//! imperative-vs-conversational discrimination lives in
//! `most_recent_user_message_since_skill_action` alone — see the
//! function's doc for the full rationale and
//! `.claude/rules/transcript-shape.md` "Real User Turns:
//! Imperative vs Conversational Shapes" for the discipline
//! authored at the rule layer.
//! `.claude/rules/autonomous-phase-discipline.md` "Conversation
//! pass-through" carries the consumer-side picture.
//!
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;

use crate::session_metrics::is_safe_transcript_path;

/// The five FLOW skills the model must never invoke. Each requires
/// explicit user initiative — the user types the slash command
/// directly — because the action is destructive (`flow-abort`,
/// `flow-reset`), resource-shipping (`flow-release`),
/// environment-mutating (`flow-prime`), or autonomy-resuming
/// (`flow-continue`).
///
/// Re-exported by `validate_skill` and `validate_ask_user` so a
/// single authoritative list governs both Layer 1 (block model
/// invocation) and Layer 2 (carve-out for confirmation prompts).
/// Entries are stored ASCII-lowercased; gate comparisons normalize
/// caller input through `normalize_gate_input` before checking
/// membership.
///
/// Namespacing asymmetry: `flow-abort`, `flow-reset`,
/// `flow-prime`, and `flow-continue` are plugin-marketplace skills
/// at `skills/<name>/SKILL.md`, so Claude Code emits the
/// namespaced `flow:<name>` form when the user types
/// `/flow:<name>`. `flow-release` and `flow-qa` are project-local
/// maintainer skills at `.claude/skills/<name>/`, so Claude Code
/// emits the bare names `flow-release` and `flow-qa` when the user
/// types `/flow-release` or `/flow-qa`. The constant reflects the
/// literal `input.skill` values the `validate-skill` PreToolUse
/// hook observes; mixing the two shapes is intentional and load-
/// bearing.
pub const USER_ONLY_SKILLS: &[&str] = &[
    "flow:flow-abort",
    "flow:flow-reset",
    "flow-release",
    "flow-qa",
    "flow:flow-prime",
    "flow:flow-continue",
];

/// Recency-window cap (50 MB) for per-turn walkers that need to
/// find a marker among the most recent ~10,000 turns. Wrapped by
/// `read_recency_window`, which seeks to the LAST `TRANSCRIPT_BYTE_CAP`
/// bytes of the transcript and reads forward to EOF. Bounds I/O on
/// the per-turn hot path so a session transcript that grows past
/// 100 MB cannot dominate latency on every Skill / AskUserQuestion
/// tool call.
///
/// Phase-boundary verifiers that may need to look past this window
/// (`verify_agent_returned_in_phase`) use `read_full` instead — the
/// uncapped path. The cap is therefore the *recency-window* limit,
/// not a global transcript-read limit.
pub const TRANSCRIPT_BYTE_CAP: u64 = 50 * 1024 * 1024;

/// Smaller tail-bounded cap (4 MB) for shared-config block detection.
/// `recent_edit_blocked_on_shared_config` only needs the last 1-2
/// turns since the most recent real user turn — the most recent
/// assistant tool call and its paired tool_result. 4 MB comfortably
/// holds those turns even when they include large file contents in
/// `tool_use.input` or `tool_result.content`. Using a smaller cap
/// here than `TRANSCRIPT_BYTE_CAP` keeps the AskUserQuestion-blocked
/// hot path fast — the helper runs synchronously inside the
/// `validate-ask-user` hook and adds latency to every blocked
/// AskUserQuestion call during in-progress autonomous phases.
pub const SHARED_CONFIG_BLOCK_BYTE_CAP: u64 = 4 * 1024 * 1024;

/// Normalize a gate-relevant string for comparison: strip NUL
/// bytes, trim leading/trailing whitespace, and ASCII-lowercase.
/// Per `.claude/rules/security-gates.md` "Normalize Before
/// Comparing", every gate input runs through this helper before
/// comparison so a NUL-padded, whitespace-padded, or case-variant
/// caller cannot bypass the membership check.
pub fn normalize_gate_input(s: &str) -> String {
    s.replace('\0', "").trim().to_ascii_lowercase()
}

/// Returns `true` when `turn` is a real user-role turn — one the
/// user themselves typed — and `false` when the turn is synthetic
/// (tool_result wrappers, slash-command expansions, hook-injected
/// feedback, or any other system-generated turn).
///
/// Discrimination rules:
/// - `message.content` must be a string. Array-content turns are
///   synthetic by construction (they carry tool_result blocks).
/// - `isMeta` must NOT be `true`. Claude Code marks every
///   system-generated turn with `isMeta:true` even when the
///   content shape is a string — most notably, Stop-hook refusal
///   feedback ("Stop hook feedback:\n...").
///
/// Both checks must pass. A missing `message`, missing `content`,
/// or non-string `content` returns `false`. A missing `isMeta`
/// is treated as `false` (real user turn — older Claude Code
/// transcripts that lack the field carry real user turns with
/// no `isMeta`).
///
/// Caller contract: walkers that need to find the most recent
/// real user turn must consult this helper and `continue` past
/// synthetic turns rather than stopping at them. A per-walker
/// inline `content.as_str()` check only filters the array-content
/// shape; this helper additionally filters the string-content +
/// `isMeta:true` shape that hook-feedback turns take, closing the
/// bypass surface where a Stop-hook refusal silently masks every
/// downstream walker's view of the real user invocation.
///
/// See `.claude/rules/transcript-shape.md` for the platform-fact
/// rationale, the closed catalog of `isMeta:true` shapes, and the
/// mechanical contract that every walker MUST call this helper
/// rather than inlining the check.
fn is_real_user_turn(turn: &Value) -> bool {
    let content_is_string = turn
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .is_some();
    // `isMeta` discrimination uses asymmetric fail-closed semantics:
    // any value that is NOT explicitly absent, `null`, or `false`
    // classifies as synthetic. This is stricter than `is_truthy`
    // (which classifies arrays/objects/strings-like-"foo" as
    // falsy — appropriate for halt-pending fail-open semantics, but
    // wrong for the `isMeta` discrimination contract).
    //
    // The threat surface: a hostile or malformed transcript line
    // with `isMeta:[true]`, `isMeta:{}`, `isMeta:"yes"`, or any
    // non-canonical truthy shape would be classified as real by
    // `is_truthy` (which returns false for those shapes), bypassing
    // Layer 1's user-only-skill enforcement when the line also
    // carries `<command-name>/<skill></command-name>` content.
    // Treating every non-canonical `isMeta` shape as synthetic
    // closes the bypass — at the cost of dropping a hypothetical
    // legitimate `isMeta:false` turn that the producer accidentally
    // wrote as `isMeta:"false"`, which is preferable to the
    // alternative.
    //
    // Per `.claude/rules/transcript-shape.md` "The Closed Catalog
    // of Synthetic User Turns".
    let is_meta = is_meta_marker_present(turn.get("isMeta"));
    content_is_string && !is_meta
}

/// Asymmetric truthy check for the `isMeta` discriminator. Returns
/// `true` when the value indicates a synthetic turn — any value
/// other than explicit absence (`None`), `null`, or `Bool(false)`.
/// Numeric `0` and string `"false"` count as synthetic (fail
/// closed) to defend against hostile or malformed transcript lines.
///
/// This differs from `is_truthy` (which classifies arrays/objects
/// as falsy) because the synthetic-turn discriminator must err
/// toward "treat as synthetic" rather than "treat as real" —
/// misclassifying a synthetic turn as real reopens the user-only-
/// skill bypass surface per `.claude/rules/transcript-shape.md`.
fn is_meta_marker_present(v: Option<&Value>) -> bool {
    match v {
        None => false,
        Some(Value::Null) => false,
        Some(Value::Bool(false)) => false,
        Some(_) => true,
    }
}

/// Return `true` when `content_norm` — a user turn's
/// `message.content` already trimmed-start and ASCII-lowercased by
/// the caller — begins with one of the two emission shapes Claude
/// Code uses for the user-typed slash command `/{skill_norm}`:
///
/// - Two-line shape (Claude Code 2.1.140+):
///   `<command-message>{skill}</command-message>\n<command-name>/{skill}</command-name>`
/// - Legacy one-line shape:
///   `<command-name>/{skill}</command-name>`
///
/// Both arms anchor via `starts_with`, so a user typing prose that
/// mentions either marker substring mid-text does NOT satisfy the
/// check. The caller normalizes both arguments — the content via
/// `trim_start().to_ascii_lowercase()` and the skill name via
/// `normalize_gate_input` — so this helper performs only the shape
/// match.
///
/// Shared by `last_user_message_invokes_skill` (Layer 1 user-only-
/// skill enforcement) and `any_skill_in_set_since_user` (Layer 10
/// bootstrap carve-out), so a future change to the slash-command
/// emission shapes updates a single point.
fn content_invokes_skill(content_norm: &str, skill_norm: &str) -> bool {
    let legacy_shape = format!("<command-name>/{}</command-name>", skill_norm);
    let new_shape = format!(
        "<command-message>{}</command-message>\n<command-name>/{}</command-name>",
        skill_norm, skill_norm
    );
    content_norm.starts_with(&new_shape) || content_norm.starts_with(&legacy_shape)
}

/// Return `true` when the most recent user-role turn in the
/// transcript at `path` invokes `skill` as a Claude Code slash
/// command. Returns `false` on any read, parse, or validation
/// failure (fail-open).
///
/// Slash-command anchoring: the trimmed `message.content` string
/// must START with one of two emission shapes Claude Code uses
/// for user-typed slash commands. The walker checks both via
/// `starts_with` disjunction:
///
/// - Two-line shape (Claude Code 2.1.140+):
///   `<command-message><skill></command-message>\n<command-name>/<skill></command-name>`
/// - Legacy one-line shape:
///   `<command-name>/<skill></command-name>`
///
/// Both shapes are anchored: a user typing prose that mentions
/// either marker substring mid-text does NOT satisfy the check,
/// because the prose's leading bytes are neither
/// `<command-message>` nor `<command-name>`. Tool_result-wrapped
/// user turns (where `content` is an array of blocks containing
/// assistant-echoed text) also fail the check because
/// `is_real_user_turn` discards array-content turns before the
/// `starts_with` comparison runs.
///
/// `home` is passed in (rather than read from `$HOME` internally)
/// so the validator can run against a fixture-controlled prefix in
/// tests without `set_var` env races. Hook callers
/// (`validate_skill::run`, `validate_ask_user::run`) read `$HOME`
/// via `crate::session_metrics::home_dir_or_empty()` and pass it
/// through.
pub fn last_user_message_invokes_skill(path: &Path, skill: &str, home: &Path) -> bool {
    if !is_safe_transcript_path(path, home) {
        return false;
    }
    let skill_norm = normalize_gate_input(skill);
    if skill_norm.is_empty() {
        return false;
    }
    let lines = match read_recency_window(path) {
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
        if turn_type != "user" {
            continue;
        }
        // Synthetic user turns (array content, or string content
        // with `isMeta:true`) are skipped — the walker keeps
        // looking backward for the most recent REAL user turn.
        // This is the only branch where the predicate stops: at
        // the first real user-typed message.
        if !is_real_user_turn(&turn) {
            continue;
        }
        let content_str = turn
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .expect("is_real_user_turn verified string content above");
        let content_norm = content_str.trim_start().to_ascii_lowercase();
        return content_invokes_skill(&content_norm, &skill_norm);
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
/// turn fires `[Bash, Skill(flow:flow-commit), Skill(flow:flow-abort)]`
/// satisfies the check because the user-only Skill is present in
/// the same turn.
///
/// `home` is passed in for the same testability reason as
/// `last_user_message_invokes_skill`.
pub fn most_recent_skill_in_user_only_set(path: &Path, home: &Path) -> bool {
    if !is_safe_transcript_path(path, home) {
        return false;
    }
    let lines = match read_recency_window(path) {
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
            // Stop only at a real user-typed message. Synthetic
            // user turns (tool_result wrappers, hook-injected
            // feedback with `isMeta:true`) interleave between the
            // user's real prompt and the assistant Skill response
            // — treating them as a boundary would mask the
            // user-only-skill carve-out the moment a Stop-hook
            // refusal lands ahead of the assistant turn.
            if !is_real_user_turn(&turn) {
                continue;
            }
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

/// Return `true` when a sanctioned skill — one whose name (after
/// `normalize_gate_input`) is a member of `sanctioned` — appears in
/// the transcript at `path` between the file tail and the most
/// recent real user-role turn, recognized through EITHER of two
/// shapes:
///
/// - An assistant Skill `tool_use` whose `input.skill` is in
///   `sanctioned`, OR
/// - The most recent real user turn ITSELF typed a sanctioned skill
///   as a slash command — the two-line or legacy `<command-name>`
///   emission shapes, checked via `content_invokes_skill`.
///
/// Returns `false` when neither shape matches before the real-user
/// boundary, when `sanctioned` is empty, or on any read / parse /
/// validation failure (fail-open).
///
/// The walker is the load-bearing predicate for the Layer 10 bootstrap
/// carve-out in `validate_pretool::bootstrap_carveout_applies`. Layer
/// 10's integration-branch context has no per-branch state file, so
/// the carve-out cannot use the `_continue_pending=commit` marker
/// that the active-flow context uses. This walker substitutes for
/// the marker: when the persisted transcript shows a sanctioned
/// bootstrap parent (`flow:flow-start`, `flow:flow-prime`, or
/// `flow-release`) AND a sanctioned commit-window skill since the
/// most recent real user turn, the surrounding skill choreography is
/// verified by replay against the transcript itself. The user-turn
/// recognition is required because `flow:flow-prime` and
/// `flow-release` are user-only skills — Claude Code records them
/// only as user-role turns, never as assistant `Skill` tool_use.
///
/// Walking backward from the file's tail, the walker checks each
/// assistant turn's Skill `tool_use` blocks and stops at the first
/// REAL user turn (synthetic turns — tool_result-wrapped array
/// content and hook-injected `isMeta:true` feedback — are skipped
/// via `is_real_user_turn`). At the real-user boundary the
/// user-typed-slash-command check runs AFTER the `is_real_user_turn`
/// guard, then the walker returns. Older turns beyond the real-user
/// boundary are invisible. Multi-tool assistant turns are scanned in
/// full via `extract_skill_invocations`, so a sanctioned Skill
/// appearing alongside other tool calls in the same turn still
/// satisfies the check.
///
/// `home` is passed in for the same testability reason as
/// `last_user_message_invokes_skill`.
pub fn any_skill_in_set_since_user(path: &Path, home: &Path, sanctioned: &[&str]) -> bool {
    if !is_safe_transcript_path(path, home) {
        return false;
    }
    if sanctioned.is_empty() {
        return false;
    }
    let lines = match read_recency_window(path) {
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
            // Stop only at a real user-typed message. Synthetic
            // user turns (tool_result wrappers with array
            // `message.content`, hook-injected feedback with
            // `isMeta:true` and string content) are skipped per
            // `.claude/rules/transcript-shape.md` — without this
            // filter, a Stop-hook refusal turn between the user's
            // real prompt and the bootstrap-parent Skill would
            // close the carve-out window prematurely.
            if !is_real_user_turn(&turn) {
                continue;
            }
            // The real user turn is the carve-out window boundary.
            // Before treating it as "no sanctioned parent found",
            // check whether the user TYPED a sanctioned skill as a
            // slash command. The loop below checks every entry in
            // `sanctioned`: `flow:flow-prime` and `flow-release` are
            // user-only skills Claude Code records ONLY as user-role
            // turns (never as assistant `Skill` tool_use), so the
            // assistant-Skill scan above can never see them; and
            // `flow:flow-start` is also user-typed in the common
            // case (the user types `/flow:flow-start` to begin a
            // flow). The branch runs AFTER the `is_real_user_turn`
            // guard so a synthetic `isMeta:true` turn echoing a
            // `<command-name>` marker is skipped, not matched.
            // `is_real_user_turn` guarantees string `content`.
            let content_str = turn
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .expect("is_real_user_turn verified string content above");
            let content_norm = content_str.trim_start().to_ascii_lowercase();
            for skill in sanctioned {
                let skill_norm = normalize_gate_input(skill);
                if skill_norm.is_empty() {
                    continue;
                }
                if content_invokes_skill(&content_norm, &skill_norm) {
                    return true;
                }
            }
            return false;
        }
        if turn_type != "assistant" {
            continue;
        }
        let skills = extract_skill_invocations(&turn);
        if skills.is_empty() {
            continue;
        }
        for skill in &skills {
            let norm = normalize_gate_input(skill);
            if sanctioned.iter().any(|s| s == &norm.as_str()) {
                return true;
            }
        }
    }
    false
}

/// Return the name of the most recent Skill `tool_use` invocation
/// in the transcript at `path` since the most recent **real** user
/// turn. Returns `None` when no Skill call has fired since the user
/// last typed, when the file cannot be read or parsed, or when the
/// validator rejects the path.
///
/// A "real user turn" is a turn whose `type == "user"` AND whose
/// `message.content` is a string (the user typed prose). Tool-result-
/// wrapped user turns (where `content` is an array of blocks) are
/// synthetic — they carry assistant-generated tool output back to
/// the model — and the walker continues past them rather than
/// treating them as a boundary.
///
/// Last-Skill-wins semantics: when multiple Skill calls appear
/// between the most recent real user turn and the file's tail, the
/// helper returns the one that appears LAST in file order. A chain
/// of `decompose:decompose → flow:pm` collapses to `"flow:pm"`, so a
/// downstream predicate that gates on decompose returns no longer
/// fires after a follow-up Skill call lands.
///
/// Production consumer: `check_in_progress_utility_skill` in
/// `src/hooks/stop_continue.rs`. The predicate uses the returned
/// skill name to discriminate "decompose just returned mid-pipeline"
/// (block: the model must continue past the Skill-tool-return
/// boundary) from "model just sent a normal conversational reply"
/// (no block: discussion mode is a legitimate stopping point).
///
/// `home` is passed in for the same testability reason as the
/// sibling helpers.
pub fn most_recent_skill_since_user(path: &Path, home: &Path) -> Option<String> {
    if !is_safe_transcript_path(path, home) {
        return None;
    }
    let lines = read_recency_window(path)?;
    let mut last_skill: Option<String> = None;
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
            // Stop only at a real user-typed message. Synthetic
            // user turns (tool_result wrappers AND hook-injected
            // feedback with `isMeta:true`) are skipped — without
            // the `isMeta` filter, a single Stop-hook refusal turn
            // halts the walker and the predicate fails open on
            // every subsequent Stop event mid-utility-skill.
            if is_real_user_turn(&turn) {
                return last_skill;
            }
            continue;
        }
        if turn_type != "assistant" {
            continue;
        }
        let skills = extract_skill_invocations(&turn);
        if skills.is_empty() {
            continue;
        }
        // Walking backward, the first Skill block we encounter is
        // the most recent in file order. Within a single multi-
        // Skill assistant turn, the LAST entry in the `skills` Vec
        // is the most recent. `skills.is_empty()` returned false
        // above, so `last()` is guaranteed to be `Some` — the
        // `.expect` documents the unreachable None arm without
        // creating a coverage branch per
        // `.claude/rules/reachable-is-testable.md`. Earlier passes
        // through this branch do not overwrite the captured value.
        if last_skill.is_none() {
            last_skill = Some(
                skills
                    .last()
                    .expect("skills non-empty: is_empty() returned false above")
                    .clone(),
            );
        }
    }
    last_skill
}

/// Verify that the agent named `agent` was invoked and returned a
/// `tool_result` after the most recent `phase-enter --phase <phase>`
/// Bash invocation in the persisted transcript. Returns `Ok(())` on
/// match; `Err(<reason>)` names the first verification step that
/// failed.
///
/// Production consumer: `bin/flow record-agent-return` — the
/// recording subcommand calls this verifier before appending to
/// `phases.<phase>.agents_returned` in state. The verification
/// prevents inline-synthesis bypass: a model that did not actually
/// invoke the agent (and so produced no `tool_use`/`tool_result`
/// pair in the persisted transcript) cannot fabricate the state
/// entry.
///
/// Failure reasons (string-typed for `record_agent_return`'s JSON
/// error envelope):
///
/// - `"transcript_path_invalid"` — `path` fails
///   `is_safe_transcript_path`. The validator rejects empty paths,
///   NUL bytes, relative paths, ParentDir components, and paths that
///   do not normalize under `<home>/.claude/projects/`.
/// - `"phase_marker_not_found"` — no `phase-enter --phase <phase>`
///   Bash tool_use is visible in the transcript (read uncapped via
///   `read_full`), OR `agent`/`phase` normalize to an empty string,
///   OR the file cannot be read / parsed at all. The verifier reads
///   the full transcript rather than the 50 MB tail because the
///   `phase-enter` marker for a long-running phase can sit
///   arbitrarily far back; a tail-bounded read would silently miss
///   it on autonomous flows whose transcript grows past the cap.
/// - `"tool_use_missing"` — no Agent tool_use with
///   `input.subagent_type == "flow:<agent>"` appears AFTER the most
///   recent phase-enter marker.
/// - `"tool_result_missing"` — the Agent tool_use was found but no
///   matching `tool_result` with the same `tool_use_id` appears
///   AFTER the marker.
///
/// The verifier anchors at the LAST phase-enter marker for `phase` so
/// agent invocations from a prior pass through the phase (rare, but
/// possible on resume) cannot satisfy a later round's required-agents
/// gate. Lines that fail to parse as JSON are silently skipped — the
/// walker's behavior on malformed input is fail-open at the line
/// level so a corrupted JSONL row does not poison the entire scan.
///
/// `home` is passed in (rather than read from `$HOME` internally) so
/// the validator can run against a fixture-controlled prefix in
/// tests without `set_var` env races. CLI callers
/// (`record_agent_return::run`) read `$HOME` via
/// `crate::session_metrics::home_dir_or_empty()` and pass it
/// through.
pub fn verify_agent_returned_in_phase(
    path: &Path,
    home: &Path,
    agent: &str,
    phase: &str,
) -> Result<(), String> {
    if !is_safe_transcript_path(path, home) {
        return Err("transcript_path_invalid".to_string());
    }
    let agent_norm = normalize_gate_input(agent);
    let phase_norm = normalize_gate_input(phase);
    if agent_norm.is_empty() || phase_norm.is_empty() {
        return Err("phase_marker_not_found".to_string());
    }
    let lines = match read_full(path) {
        Some(s) => s,
        None => return Err("phase_marker_not_found".to_string()),
    };
    let all_lines: Vec<&str> = lines.lines().collect();
    let mut marker_idx: Option<usize> = None;
    for (i, line) in all_lines.iter().enumerate().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let turn: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if assistant_turn_runs_phase_enter(&turn, &phase_norm) {
            marker_idx = Some(i);
            break;
        }
    }
    let marker_idx = match marker_idx {
        Some(i) => i,
        None => return Err("phase_marker_not_found".to_string()),
    };
    let subagent_needle = format!("flow:{}", agent_norm);
    // Single forward scan that collects every matching Agent
    // tool_use_id AND watches for a tool_result whose tool_use_id
    // matches any collected id. Retried agent invocations (first
    // attempt truncated/failed, second attempt clean) produce
    // multiple tool_use entries with distinct ids and exactly one
    // tool_result for the successful attempt — returning on the
    // first matching pair handles that shape. A single-pass scan
    // also handles the happy-path single-invocation case in fewer
    // turns than the prior two-pass form.
    let mut candidate_ids: Vec<String> = Vec::new();
    for line in all_lines.iter().skip(marker_idx + 1) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let turn: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(id) = find_agent_tool_use_id(&turn, &subagent_needle) {
            candidate_ids.push(id);
        }
        for id in &candidate_ids {
            if user_turn_carries_tool_result_for(&turn, id) {
                return Ok(());
            }
        }
    }
    if candidate_ids.is_empty() {
        Err("tool_use_missing".to_string())
    } else {
        Err("tool_result_missing".to_string())
    }
}

/// Returns `true` when `turn` is an assistant turn that fires a Bash
/// tool_use whose `input.command` is a `bin/flow phase-enter --phase
/// <phase>` invocation (bare `bin/flow` or any absolute path ending in
/// `/bin/flow`). Used by `verify_agent_returned_in_phase` to locate
/// the `phase-enter --phase <phase>` boundary.
///
/// The match is token-aware (`cmd_invokes_phase_enter`) rather than
/// substring. An unrelated command whose text contains the marker
/// substring — `echo "phase-enter --phase flow-review"`,
/// `bin/flow log "...phase-enter --phase flow-review..."` — does NOT
/// match, so the verifier's scan window cannot be pinned to the wrong
/// boundary by a casual log entry that mentions the phase name.
fn assistant_turn_runs_phase_enter(turn: &Value, phase: &str) -> bool {
    let turn_type = normalize_gate_input(turn.get("type").and_then(|v| v.as_str()).unwrap_or(""));
    if turn_type != "assistant" {
        return false;
    }
    let content = match turn
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
    {
        Some(c) => c,
        None => return false,
    };
    for block in content {
        if block.get("type").and_then(|v| v.as_str()) != Some("tool_use") {
            continue;
        }
        if block.get("name").and_then(|v| v.as_str()) != Some("Bash") {
            continue;
        }
        let cmd = block
            .get("input")
            .and_then(|i| i.get("command"))
            .and_then(|c| c.as_str())
            .unwrap_or("");
        if cmd_invokes_phase_enter(cmd, phase) {
            return true;
        }
    }
    false
}

/// Token-aware check: `cmd` is a `bin/flow phase-enter --phase
/// <phase>` invocation. Tokens 0/1 must be `(bin/flow|*/bin/flow)`
/// then `phase-enter`. The phase name must appear as the next token
/// after a literal `--phase` token OR as the suffix of a
/// `--phase=<phase>` token. Rejects substring false positives by
/// requiring the command's first two tokens to match the canonical
/// invocation shape — see `.claude/rules/comment-quality.md` for
/// why the comment names what the match excludes.
fn cmd_invokes_phase_enter(cmd: &str, phase: &str) -> bool {
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    // The 3-token form `bin/flow phase-enter --phase=value` collapses
    // the phase flag and value into a single token; the 4-token form
    // `bin/flow phase-enter --phase value` keeps them separated. Both
    // are valid clap invocations, so the minimum token count is 3 —
    // tokens[0] and tokens[1] are the safe-indexed checks below, and
    // the loop at skip(2) walks any remaining tokens for the phase
    // value (= form or space form).
    if tokens.len() < 3 {
        return false;
    }
    let first = tokens[0];
    if first != "bin/flow" && !first.ends_with("/bin/flow") {
        return false;
    }
    if tokens[1] != "phase-enter" {
        return false;
    }
    for (i, tok) in tokens.iter().enumerate().skip(2) {
        if *tok == "--phase" && tokens.get(i + 1) == Some(&phase) {
            return true;
        }
        if let Some(rest) = tok.strip_prefix("--phase=") {
            if rest == phase {
                return true;
            }
        }
    }
    false
}

/// Returns the `id` of the first Agent tool_use in `turn` whose
/// `input.subagent_type` (normalized) equals `subagent_needle`.
/// Returns `None` when the turn is not an assistant turn, when no
/// Agent block matches, or when the matching block lacks an `id`
/// field. Recognized tool names are `"Agent"` and `"Task"` —
/// Claude Code's transcripts use one or the other for sub-agent
/// invocations depending on version.
fn find_agent_tool_use_id(turn: &Value, subagent_needle: &str) -> Option<String> {
    let turn_type = normalize_gate_input(turn.get("type").and_then(|v| v.as_str()).unwrap_or(""));
    if turn_type != "assistant" {
        return None;
    }
    let content = turn
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())?;
    for block in content {
        if block.get("type").and_then(|v| v.as_str()) != Some("tool_use") {
            continue;
        }
        let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name != "Agent" && name != "Task" {
            continue;
        }
        let sa = block
            .get("input")
            .and_then(|i| i.get("subagent_type"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if normalize_gate_input(sa) == subagent_needle {
            // A matching subagent block without an `id` field is
            // malformed — continue the loop so a later well-formed
            // sibling block in the same turn's content array still
            // contributes its id. Returning None here would skip
            // valid siblings and force the caller's retry loop
            // (`verify_agent_returned_in_phase`) to falsely conclude
            // `tool_use_missing`.
            if let Some(id) = block.get("id").and_then(|v| v.as_str()) {
                return Some(id.to_string());
            }
        }
    }
    None
}

/// Returns `true` when `turn` is a user turn whose content array
/// carries a `tool_result` block with `tool_use_id == target_id`.
/// String-content user turns (the user typed prose) do not satisfy
/// the check; only the array-content shape (tool_result-wrapped user
/// turns) is examined.
fn user_turn_carries_tool_result_for(turn: &Value, target_id: &str) -> bool {
    let turn_type = normalize_gate_input(turn.get("type").and_then(|v| v.as_str()).unwrap_or(""));
    if turn_type != "user" {
        return false;
    }
    let content = match turn
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
    {
        Some(c) => c,
        None => return false,
    };
    for block in content {
        if block.get("type").and_then(|v| v.as_str()) != Some("tool_result") {
            continue;
        }
        if block.get("tool_use_id").and_then(|v| v.as_str()) == Some(target_id) {
            return true;
        }
    }
    false
}

/// Return the most recent string-content user-role turn AFTER the
/// FIRST assistant Skill `tool_use` in the transcript at
/// `transcript_path`, filtering imperative slash-command turns
/// from candidate capture and treating `/flow:flow-continue` as a
/// watermark that resets preceding prose. Returns `None` when no
/// Skill call has fired, when no real user turn follows the first
/// Skill call, when the only post-Skill user turns are slash-
/// command shapes, when `/flow:flow-continue` has cleared every
/// preceding prose candidate with no subsequent prose, when the
/// validator rejects the path, when the file cannot be read or
/// parsed, or when the file is empty.
///
/// ## Three classes of real user turn
///
/// Per `.claude/rules/transcript-shape.md`, transcript JSONL
/// carries two synthetic user-turn shapes alongside user-typed
/// prose. This walker further partitions real (non-synthetic)
/// user turns into two sub-classes:
///
/// - **Conversational prose** — string content not matching the
///   slash-command emission shape. The user is talking to the
///   model. Captured as `candidate`.
/// - **Imperative slash-command input** — string content
///   beginning with `<command-message>` or `<command-name>`. The
///   user is invoking a slash command, not conversing. Filtered
///   from candidate capture. When the slash-command names
///   `flow:flow-continue`, the walker additionally watermarks
///   the prior `candidate` to `None` because the resume directive
///   is the user answering whatever pause their preceding prose
///   triggered — preserving that prose would re-arm the halt
///   contract on the next Stop event and trap the autonomous
///   flow in a permanent voluntary-stop state.
///
/// Other slash commands (e.g. `/flow:flow-abort`) are filtered
/// from candidate capture but do NOT watermark preceding prose:
/// only `/flow:flow-continue` is the universal resume directive,
/// so preserving the user's prose lets the user combine "pause
/// to talk" with "and also abort" without losing the
/// conversational signal.
///
/// ## Production consumer
///
/// `check_autonomous_stop` in `src/hooks/stop_continue.rs`. The
/// predicate uses the returned content to detect whether the
/// user has typed a new prose message since the model last took
/// a Skill action. Per
/// `.claude/rules/autonomous-phase-discipline.md` "The Two-Exit
/// Halt Model", a `Some` return triggers the conversation
/// pass-through (set `_halt_pending=true`, allow the Stop so the
/// model can answer); a `None` return triggers Rule 1 (refuse
/// the Stop with the encouraging message). The slash-command
/// filter and the `/flow:flow-continue` watermark are what give
/// `/flow:flow-continue` its universal-resume semantics — they
/// ensure the next Stop after a resume fires Rule 1, not Rule 2
/// or a fresh pass-through, regardless of whether the original
/// halt was triggered by user prose or by a system event.
///
/// ## Validation contract
///
/// Per `.claude/rules/external-input-path-construction.md`,
/// `transcript_path` runs through
/// `crate::session_metrics::is_safe_transcript_path` (rejects
/// empty, NUL-byte, relative, ParentDir-component, and prefix-
/// escaping paths). File reads are bounded by `read_recency_window`
/// (50 MB tail) so a corrupted or hostile transcript cannot cause
/// unbounded I/O on the per-turn hot path.
///
/// `home` is passed in (rather than read from `$HOME` inside) so
/// fixture-controlled tests can isolate from the real user
/// environment per `.claude/rules/testing-gotchas.md`.
pub fn most_recent_user_message_since_skill_action(
    transcript_path: &Path,
    home: &Path,
) -> Option<String> {
    if !is_safe_transcript_path(transcript_path, home) {
        return None;
    }
    let lines = read_recency_window(transcript_path)?;
    let mut candidate: Option<String> = None;
    let mut seen_skill = false;
    for line in lines.lines() {
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
        if turn_type == "assistant" {
            let skills = extract_skill_invocations(&turn);
            if !skills.is_empty() {
                // The first Skill action opens the candidate
                // window. Subsequent Skills do NOT close it —
                // the user's pause message must remain visible
                // when the model fires additional Skills as part
                // of its response. Closing the window on every
                // Skill would erase user-initiated pauses in the
                // autonomous-mode loop.
                seen_skill = true;
            }
            continue;
        }
        if turn_type != "user" {
            continue;
        }
        if !seen_skill {
            continue;
        }
        // Only REAL user turns count, per
        // `.claude/rules/transcript-shape.md` "The Mechanical
        // Contract". Synthetic tool_result-wrapped user turns
        // (array content) and hook-injected feedback turns
        // (string content + `isMeta:true`) must both be skipped —
        // either shape misclassified as real prose would
        // falsely trigger the halt-pause contract.
        if !is_real_user_turn(&turn) {
            continue;
        }
        // `is_real_user_turn` already verified `message.content`
        // is a string, so the `as_str()` cannot return None here
        // — the `.expect` is documentation of that guarantee, not
        // a reachable panic. Per
        // `.claude/rules/testability-means-simplicity.md`.
        let s = turn
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .expect("is_real_user_turn guarantees string content");
        // Normalize-before-comparing per
        // `.claude/rules/security-gates.md`: the slash-command tag
        // check and the watermark trigger both run on lowercased
        // content so case-variant emission shapes
        // (`<COMMAND-NAME>`, `<Command-Message>`, `/FLOW:FLOW-CONTINUE`)
        // cannot bypass either branch.
        let normalized = s.trim_start().to_ascii_lowercase();
        if content_starts_with_slash_command(&normalized) {
            // Imperative slash-command input is not
            // conversational prose. `/flow:flow-continue` is
            // additionally the universal resume directive — it
            // clears the candidate so the next Stop event fires
            // Rule 1 (encouraging-message refusal) instead of
            // re-arming Rule 2 from the user's prior pause prose.
            if content_invokes_skill(&normalized, "flow:flow-continue") {
                candidate = None;
            }
            continue;
        }
        candidate = Some(s.to_string());
    }
    candidate
}

/// Return `true` when `content_normalized` — a user turn's
/// `message.content` already passed through `trim_start` AND
/// `to_ascii_lowercase` by the caller — begins with one of the
/// two emission shapes Claude Code uses for user-typed slash
/// commands:
///
/// - Two-line shape (Claude Code 2.1.140+):
///   `<command-message>...</command-message>\n<command-name>/...</command-name>`
/// - Legacy one-line shape: `<command-name>/...</command-name>`
///
/// Both shapes share the property that the leading bytes
/// after `trim_start` are an XML-like opening tag, so a single
/// shape match against `<command-message>` OR `<command-name>`
/// captures every slash-command-invocation user turn while
/// rejecting prose that happens to mention either tag mid-text.
///
/// The caller normalizes the content via `to_ascii_lowercase`
/// before this check so case-variant emission shapes
/// (`<COMMAND-NAME>`, `<Command-Message>`) match the same
/// lowercase tag literal here. Per
/// `.claude/rules/security-gates.md` "Normalize Before
/// Comparing", normalization runs on both sides — the
/// callsite lowercases the content; this helper's literals
/// are already lowercase.
///
/// Consumed by `most_recent_user_message_since_skill_action`
/// to discriminate imperative slash-command input from
/// conversational prose. The check is intentionally
/// skill-name-agnostic — every slash command (not just
/// `flow:flow-continue`) gets filtered out of candidate
/// capture so the halt-pause contract treats every
/// `/<slash-command>` invocation as imperative input, never
/// as a conversational halt trigger.
fn content_starts_with_slash_command(content_normalized: &str) -> bool {
    content_normalized.starts_with("<command-name>")
        || content_normalized.starts_with("<command-message>")
}

/// Read the entire `path` as a UTF-8 String. Returns `None` on
/// `File::open` error, non-UTF-8 content, or other I/O failure.
///
/// Uncapped read: the entire file content is loaded into memory.
/// Callers that walk to a phase-boundary marker that may sit
/// arbitrarily far back in the transcript — `verify_agent_returned_in_phase`
/// is the canonical consumer — use `read_full` because a tail-bounded
/// read can hide the marker on long autonomous flows. The trade-off
/// is memory: a 200 MB transcript loads 200 MB of working memory.
/// Acceptable for phase-boundary verification because the verifier
/// runs at most once per agent return (rare) rather than per-turn.
///
/// For recency-window reads (per-turn hot path), use
/// `read_recency_window` instead. For shared-config-block detection,
/// use `read_recent_turns`. Direct calls to `read_capped` from
/// production code are forbidden by the contract test in
/// `tests/hooks/transcript_walker.rs` —
/// `.claude/rules/transcript-walker-cap.md` documents the API.
pub fn read_full(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// Read the LAST `TRANSCRIPT_BYTE_CAP` (50 MB) bytes of `path` as a
/// UTF-8 String. Returns `None` on `File::open` error, non-UTF-8
/// content, or other I/O failure.
///
/// Recency-window read: the byte cap bounds backward visibility to
/// the most recent ~10,000 turns. Callers that need to find a marker
/// in the recent past — most-recent user turn, most-recent assistant
/// Skill call, paired tool_use/tool_result for the latest
/// AskUserQuestion — use `read_recency_window` because the cap keeps
/// the per-turn hot path bounded as transcripts grow past 100 MB.
///
/// The canonical consumer class is the per-turn walker family:
/// `last_user_message_invokes_skill`,
/// `most_recent_skill_in_user_only_set`,
/// `most_recent_user_message_since_skill_action`,
/// `most_recent_skill_since_user`,
/// `any_skill_in_set_since_user`. Phase-boundary verifiers that may
/// need to look further back use `read_full` instead.
pub fn read_recency_window(path: &Path) -> Option<String> {
    read_capped(path, TRANSCRIPT_BYTE_CAP)
}

/// Read the LAST `SHARED_CONFIG_BLOCK_BYTE_CAP` (4 MB) bytes of
/// `path` as a UTF-8 String. Returns `None` on `File::open` error,
/// non-UTF-8 content, or other I/O failure.
///
/// Recent-turns read: the smaller cap reflects that the consumer
/// (`recent_edit_blocked_on_shared_config`) only needs the most
/// recent 1-2 turns since the most recent real user turn — the
/// latest assistant tool call and its paired tool_result. 4 MB
/// comfortably holds those turns even when they include large file
/// contents in `tool_use.input` or `tool_result.content`, and the
/// smaller cap keeps the AskUserQuestion-blocked hot path faster
/// than `read_recency_window` would.
pub fn read_recent_turns(path: &Path) -> Option<String> {
    read_capped(path, SHARED_CONFIG_BLOCK_BYTE_CAP)
}

/// Read the LAST `cap` bytes of `path` as a UTF-8 String. Returns
/// `None` on `File::open` error or non-UTF-8 content.
///
/// The function seeks to `max(0, file_len - cap)` and reads forward
/// to EOF. The buffer is the file's tail, which is what the backward
/// walker needs — reading from the head silently omits recent turns
/// on transcripts larger than the cap. A partial JSONL line at the
/// buffer's start (mid-line truncation at the seek point) fails to
/// parse and is silently skipped by the walker's `Err(_) => continue`
/// branch.
///
/// Direct calls to `read_capped` from production code outside this
/// module are forbidden. Three public wrappers carry the lookback
/// semantics by name: `read_full` (uncapped, phase-boundary
/// verifiers), `read_recency_window` (50 MB, per-turn recency
/// walkers), `read_recent_turns` (4 MB, shared-config-block
/// detection). The contract test
/// `read_capped_only_called_inside_named_helpers` in
/// `tests/hooks/transcript_walker.rs` locks the invariant in.
///
/// `metadata()` and `seek()` on a freshly-opened regular file are
/// genuinely TOCTOU-only failure modes per the
/// `.claude/rules/external-input-path-construction.md` "No `.expect()`
/// on Filesystem Reads" carve-out — `.expect()` is acceptable here
/// because the `.ok()?` branch above on `File::open` is the only
/// reachable failure surface for the open-file-then-stat-then-seek
/// sequence. A test cannot reproduce metadata or seek failure on a
/// freshly-opened regular file without root-level interference.
fn read_capped(path: &Path, cap: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let file_len = file
        .metadata()
        .expect("metadata succeeds on freshly-opened regular file (TOCTOU-only)")
        .len();
    let start = file_len.saturating_sub(cap);
    file.seek(SeekFrom::Start(start))
        .expect("seek to non-negative absolute offset succeeds on regular file (TOCTOU-only)");
    // Wrap the reader in `take(cap)` so the total bytes consumed
    // by `read_to_string` are hard-bounded at `cap` even when the
    // file grows after the `metadata()` call (concurrent writers).
    // This matches the canonical byte-cap pattern documented in
    // `.claude/rules/external-input-path-construction.md`.
    let mut reader = BufReader::new(file.take(cap));
    let mut buf = String::new();
    reader.read_to_string(&mut buf).ok()?;
    Some(buf)
}

/// Returns `true` when the most recent user-role turn in the
/// persisted transcript carries a `validate_worktree_paths` shared-
/// config edit-block tool_result. Returns `false` on any I/O, parse,
/// or validation failure (fail-open).
///
/// Detection signal: a `tool_result` block whose `is_error` is
/// truthy AND whose `content` contains the literal substring
/// `"is a shared configuration file that affects every engineer"`
/// — uniquely emitted by
/// `crate::hooks::validate_worktree_paths::validate_shared_config`.
/// The phrase is intentionally long: the shorter "is a shared
/// configuration file" prefix could appear in unrelated error
/// messages (a permission-denied error, a generic "this file is
/// shared" warning), but the full phrase including "that affects
/// every engineer" matches only the BLOCKED message produced by
/// validate_worktree_paths. The substring's presence is locked by a
/// presence-contract test in
/// `tests/hooks/validate_worktree_paths.rs`.
///
/// Companion to `validate_ask_user::validate`: when validate would
/// have blocked the AskUserQuestion under autonomous-phase
/// discipline, this helper's `true` return suppresses the block so
/// the model can run the AskUserQuestion that
/// `validate_worktree_paths`' BLOCKED message itself instructs the
/// model to call. Without the carve-out, the model would deadlock
/// — `validate-worktree-paths` says "use AskUserQuestion to
/// confirm with the user" and `validate-ask-user` simultaneously
/// blocks AskUserQuestion.
///
/// Walks lines backward from the file tail (read via
/// `read_recent_turns`, capped at `SHARED_CONFIG_BLOCK_BYTE_CAP`)
/// and stops at the most recent
/// user-role turn — examining ONLY that turn's content. The carve-
/// out fires iff the latest interaction the model received from the
/// user-role channel was the shared-config block. If any other
/// tool_result intervenes before the AskUserQuestion (a different
/// tool's success or failure), the most recent user turn is no
/// longer the shared-config block and the carve-out does not fire.
/// This scoping keeps stale shared-config blocks from earlier in
/// the session from authorizing unrelated AskUserQuestions later.
///
/// `transcript_path` is validated through
/// `crate::session_metrics::is_safe_transcript_path` per
/// `.claude/rules/external-input-path-construction.md` (rejects
/// empty, NUL-byte, relative, ParentDir-component, prefix-escaping,
/// and symlink-escape paths). `home` is passed in for the same
/// testability reason as the sibling helpers.
pub fn recent_edit_blocked_on_shared_config(transcript_path: &Path, home: &Path) -> bool {
    if !is_safe_transcript_path(transcript_path, home) {
        return false;
    }
    let lines = match read_recent_turns(transcript_path) {
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
        if turn_type != "user" {
            continue;
        }
        // Skip hook-injected feedback turns (string content with
        // `isMeta:true`). Tool_result-wrapped user turns (array
        // content) are the legitimate carrier of the shared-config
        // block and must NOT be skipped — `user_turn_carries_shared
        // _config_block` examines the array content for the block.
        // Only the string-content-with-`isMeta:true` shape is
        // synthetic-and-irrelevant here, since the shared-config
        // signal lives inside tool_result blocks, never inside
        // string-content user turns.
        let is_string_content = turn
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .is_some();
        // Use the asymmetric `is_meta_marker_present` predicate so
        // the targeted skip here matches the discriminator in
        // `is_real_user_turn`. Both walkers must classify
        // hook-feedback turns identically regardless of the exact
        // `isMeta` shape — array, object, or non-canonical string
        // — to close the bypass surface where a crafted JSONL line
        // can pass as real prose. See
        // `.claude/rules/transcript-shape.md`.
        let is_meta = is_meta_marker_present(turn.get("isMeta"));
        if is_string_content && is_meta {
            continue;
        }
        // Most recent non-hook-feedback user-role turn reached.
        // Examine its content and RETURN — do not continue walking
        // backward to older turns. Scoping the carve-out to the
        // immediately preceding non-synthetic user-role event keeps
        // stale shared-config blocks from authorizing unrelated
        // AskUserQuestions later in the session.
        return user_turn_carries_shared_config_block(&turn);
    }
    false
}

/// Returns `true` when the user-role turn carries a tool_result
/// block whose `is_error` is truthy AND whose `content` contains
/// the shared-config substring. Returns `false` for string-content
/// user turns (the user typed a message), missing or non-array
/// content, and array content where no block matches.
fn user_turn_carries_shared_config_block(turn: &Value) -> bool {
    let content = match turn.get("message").and_then(|m| m.get("content")) {
        Some(c) => c,
        None => return false,
    };
    // String content is a real user-typed message — not a
    // tool_result wrapper. The carve-out only fires when the
    // most recent user-role event is a tool_result-wrapped turn
    // carrying the shared-config block.
    if content.as_str().is_some() {
        return false;
    }
    let blocks = match content.as_array() {
        Some(arr) => arr,
        None => return false,
    };
    for block in blocks {
        if block.get("type").and_then(|v| v.as_str()) != Some("tool_result") {
            continue;
        }
        if !is_truthy(block.get("is_error")) {
            continue;
        }
        let block_content = match block.get("content") {
            Some(c) => c,
            None => continue,
        };
        // tool_result.content is either a plain string or an
        // array of content blocks (each typically a `text`
        // block). Concatenate text fields for the array shape
        // so a substring match catches both wire formats.
        let text = if let Some(s) = block_content.as_str() {
            s.to_string()
        } else if let Some(items) = block_content.as_array() {
            let mut joined = String::new();
            for item in items {
                if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                    if !joined.is_empty() {
                        joined.push(' ');
                    }
                    joined.push_str(t);
                }
            }
            joined
        } else {
            continue;
        };
        if text.contains("is a shared configuration file that affects every engineer") {
            return true;
        }
    }
    false
}

/// Defensive truthiness check for security-enforcement hook reads
/// of boolean fields. Per `.claude/rules/rust-patterns.md` "Hook
/// Input Boolean Field Tolerance": accept `true`, the strings
/// `"true"` / `"1"` (case-insensitive), and any non-zero number.
/// Everything else (including `null`, `false`, empty string,
/// non-truthy strings, and `0`) is `false`.
///
/// Public because `validate_skill` and `validate_pretool` halt
/// gates read state-file `_halt_pending` and must tolerate the same
/// truthy shapes the walker tolerates for `isMeta` discrimination.
pub fn is_truthy(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => {
            let norm = s.trim().to_ascii_lowercase();
            norm == "true" || norm == "1"
        }
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        _ => false,
    }
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
