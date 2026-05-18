//! Integration tests for `src/hooks/transcript_walker.rs`.
//!
//! Drives `last_user_message_invokes_skill` and
//! `most_recent_skill_in_user_only_set` through controlled JSONL
//! fixtures via `transcript_fixture` (in `tests/common/mod.rs`,
//! reachable as `crate::common::transcript_fixture` because
//! `tests/hooks/main.rs` declares the path-aliased common module).
//! Each line in the fixture is a Claude Code transcript turn whose
//! top-level `type` field carries the `user`/`assistant` role
//! (matching `src/session_metrics.rs::read_transcript`).

use std::fs;

use flow_rs::hooks::transcript_walker::{
    any_skill_in_set_since_user, last_user_message_invokes_skill,
    most_recent_skill_in_user_only_set, most_recent_skill_since_user,
    most_recent_user_message_since_skill_action, read_full, read_recency_window, read_recent_turns,
    recent_edit_blocked_on_shared_config, verify_agent_returned_in_phase,
    SHARED_CONFIG_BLOCK_BYTE_CAP, TRANSCRIPT_BYTE_CAP, USER_ONLY_SKILLS,
};

// --- last_user_message_invokes_skill ---

#[test]
fn walker_returns_false_when_path_missing() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let missing = home
        .join(".claude")
        .join("projects")
        .join("p")
        .join("nonexistent.jsonl");
    assert!(!last_user_message_invokes_skill(
        &missing,
        "flow:flow-abort",
        home,
    ));
    assert!(!most_recent_skill_in_user_only_set(&missing, home));
}

#[test]
fn walker_returns_false_when_path_unparseable_jsonl() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let path = crate::common::transcript_fixture(home, "p", "not json\nstill not json\n");
    assert!(!last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home
    ));
    assert!(!most_recent_skill_in_user_only_set(&path, home));
}

#[test]
fn walker_returns_false_when_command_falls_off_tail_cap() {
    // Tail-read fixture: a valid user turn with the matching command
    // is written at the file's HEAD, then > TRANSCRIPT_BYTE_CAP bytes
    // of padding follow. `read_capped` reads the LAST cap bytes, so
    // the head-positioned command is invisible and the walker
    // returns false. Verifies the byte cap bounds backward visibility
    // when the most recent content has buried older user turns far
    // enough back that they no longer fit in the cap.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let proj = home.join(".claude").join("projects").join("p");
    fs::create_dir_all(&proj).unwrap();
    let path = proj.join("oversized.jsonl");
    let leading = b"{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n";
    let mut content: Vec<u8> = leading.to_vec();
    let padding_size = (TRANSCRIPT_BYTE_CAP as usize) + 1024;
    content.extend(std::iter::repeat_n(b'\n', padding_size));
    fs::write(&path, &content).unwrap();
    assert!(!last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home
    ));
}

#[test]
fn walker_finds_command_when_tail_within_cap() {
    // Inverse of walker_returns_false_when_command_falls_off_tail_cap:
    // padding precedes the command, then a valid user turn at the
    // tail fits within the last TRANSCRIPT_BYTE_CAP bytes. The
    // tail-read sees the command and the predicate returns true.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let proj = home.join(".claude").join("projects").join("p");
    fs::create_dir_all(&proj).unwrap();
    let path = proj.join("tail-within-cap.jsonl");
    let padding_size = 1024usize;
    let mut content: Vec<u8> = std::iter::repeat_n(b'\n', padding_size).collect();
    let trailing = b"{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n";
    content.extend_from_slice(trailing);
    fs::write(&path, &content).unwrap();
    assert!(last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home
    ));
}

#[test]
fn last_user_invokes_finds_match_on_most_recent_user_turn() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"hi\"}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home
    ));
}

#[test]
fn last_user_invokes_returns_false_when_user_turn_has_different_command() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-status</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home
    ));
}

#[test]
fn last_user_invokes_returns_false_when_command_in_older_user_turn_not_most_recent() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"ok\"}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"please continue\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home
    ));
}

#[test]
fn last_user_invokes_ignores_command_in_assistant_text() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // Assistant turn discusses the literal `<command-name>/flow:flow-abort` substring.
    // The most recent user turn has different content. The walker stops at
    // the user turn so the assistant text is never queried — returns false.
    let jsonl = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"<command-name>/flow:flow-abort</command-name>\"}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"please continue\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home
    ));
}

// --- most_recent_skill_in_user_only_set ---

#[test]
fn most_recent_skill_in_user_only_set_finds_assistant_skill_call() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"do something\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-abort\"}}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(most_recent_skill_in_user_only_set(&path, home));
}

#[test]
fn most_recent_skill_in_user_only_set_returns_false_when_skill_not_user_only() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"check status\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-status\"}}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!most_recent_skill_in_user_only_set(&path, home));
}

#[test]
fn most_recent_skill_in_user_only_set_stops_at_user_turn() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // Older assistant Skill call to a user-only skill, then a user
    // turn, then a more recent assistant Skill call to a non-user-only
    // skill. The walker scans from the end, hits the recent
    // non-user-only call first, returns false. Stopping at the user
    // turn ensures the older user-only call is never reached.
    let jsonl = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-abort\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"now do something else\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-status\"}}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!most_recent_skill_in_user_only_set(&path, home));
}

#[test]
fn user_only_skills_constant_lists_six_skills() {
    let names: Vec<&str> = USER_ONLY_SKILLS.to_vec();
    assert!(names.contains(&"flow:flow-abort"));
    assert!(names.contains(&"flow:flow-reset"));
    // flow-release and flow-qa are project-local maintainer skills
    // at `.claude/skills/<name>/`, so Claude Code emits the bare
    // names (no `flow:` prefix) when the user types `/flow-release`
    // or `/flow-qa`.
    assert!(names.contains(&"flow-release"));
    assert!(names.contains(&"flow-qa"));
    assert!(names.contains(&"flow:flow-prime"));
    assert!(names.contains(&"flow:flow-continue"));
    assert_eq!(names.len(), 6);
}

#[test]
fn user_only_skills_contains_flow_continue() {
    // `flow:flow-continue` joins USER_ONLY_SKILLS so the
    // `validate-skill` Layer 1 gate rejects model invocations of
    // `/flow:flow-continue` — only a user typing the slash command
    // can clear `_halt_pending`. The `bin/flow clear-halt`
    // subcommand independently self-gates on the same skill name
    // via `last_user_message_invokes_skill`.
    let names: Vec<&str> = USER_ONLY_SKILLS.to_vec();
    assert!(
        names.contains(&"flow:flow-continue"),
        "USER_ONLY_SKILLS must include flow:flow-continue so validate-skill blocks model invocation"
    );
}

#[test]
fn walker_skips_empty_lines_in_fixture() {
    // Empty / whitespace-only lines must be skipped without parsing.
    // Placing blank lines between real turns exercises the
    // `trimmed.is_empty()` continue branch in the walker.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "\n   \n{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"do something\"}}\n\
\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-abort\"}}]}}\n\
\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(most_recent_skill_in_user_only_set(&path, home));
}

#[test]
fn most_recent_skill_walker_continues_past_assistant_turn_without_skill_call() {
    // Assistant turn has only a text block (no tool_use) — walker
    // continues past it. Then a user turn — walker stops, returns
    // false. Exercises the
    // `extract_skill_invocation -> None` branch when the assistant
    // turn yields no Skill invocation.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"thinking\"}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    // Walking from end: assistant turn (no Skill) → continue.
    // Next: user turn → return false.
    assert!(!most_recent_skill_in_user_only_set(&path, home));
}

#[test]
fn most_recent_skill_walker_skips_non_skill_tool_use() {
    // Assistant turn has a tool_use block whose name is "Bash"
    // (not "Skill"). extract_skill_invocation skips the Bash block
    // and continues. Then no further blocks → returns None →
    // walker continues past the assistant turn → eventually
    // returns false at the user turn.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"do it\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"input\":{\"command\":\"ls\"}}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!most_recent_skill_in_user_only_set(&path, home));
}

#[test]
fn most_recent_skill_walker_skips_text_block_then_finds_skill() {
    // Assistant turn has BOTH a text block (continue) AND a Skill
    // tool_use block. The walker iterates through the content
    // array, skips the text block, finds the Skill block.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"abort please\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"OK, aborting.\"},{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-abort\"}}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(most_recent_skill_in_user_only_set(&path, home));
}

#[test]
fn most_recent_skill_walker_handles_skill_block_without_input_skill_string() {
    // Skill tool_use whose input.skill field is missing OR not a
    // string. extract_skill_invocation returns None — walker
    // continues past the block, finds nothing else, returns false.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":42}}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!most_recent_skill_in_user_only_set(&path, home));
}

#[test]
fn most_recent_skill_walker_handles_assistant_turn_without_message_field() {
    // Assistant turn with no `message` field at all.
    // extract_skill_invocation returns None at the first `?` ->
    // walker continues, hits user turn, returns false.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n\
{\"type\":\"assistant\"}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!most_recent_skill_in_user_only_set(&path, home));
}

#[test]
fn most_recent_skill_walker_handles_assistant_message_without_content_field() {
    // Assistant turn has `message` but no `content` field —
    // `get("content")?` short-circuits to None, walker continues
    // and returns false at the user turn.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!most_recent_skill_in_user_only_set(&path, home));
}

#[test]
fn most_recent_skill_walker_handles_content_not_array() {
    // Assistant turn has `message.content` as a STRING (not array).
    // `as_array()?` short-circuits to None.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"plain text response\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!most_recent_skill_in_user_only_set(&path, home));
}

#[test]
fn last_user_invokes_iterates_past_trailing_assistant_to_older_user_turn() {
    // Fixture has an assistant turn AFTER the most recent user
    // turn (assistant is last in file). Walking backward: hit
    // assistant first → not user → continue past it. Next: user
    // turn → match check returns. Exercises the iterate-past-
    // assistant branch in `last_user_message_invokes_skill`.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"OK.\"}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home
    ));
}

#[test]
fn most_recent_skill_walker_skips_turns_with_unknown_type() {
    // A turn whose `type` is neither "user" nor "assistant" (e.g.,
    // a future role like "system" or a malformed/unknown type)
    // is skipped via continue — walker keeps iterating to find
    // either a user or assistant turn.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n\
{\"type\":\"system\",\"message\":{\"role\":\"system\",\"content\":\"compaction summary\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-abort\"}}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    // Walk reverse: assistant Skill (user-only) → returns true.
    // The system turn would be skipped if it appeared before the
    // assistant turn in reverse order. Place the system turn
    // BETWEEN assistant and user to ensure walker skips it on its
    // way to the user boundary.
    assert!(most_recent_skill_in_user_only_set(&path, home));
}

#[test]
fn most_recent_skill_walker_skips_unknown_type_before_reaching_user() {
    // Unknown-type turn (e.g., "system") appears as the LAST turn.
    // Walker hits it first, sees neither user nor assistant,
    // continues to the next iteration. Eventually reaches the
    // user boundary and returns false.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n\
{\"type\":\"system\",\"message\":{\"role\":\"system\",\"content\":\"compaction summary\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    // Walking reverse: system turn → unknown type → continue.
    // Then user turn → return false.
    assert!(!most_recent_skill_in_user_only_set(&path, home));
}

#[test]
fn walker_returns_false_when_file_contains_non_utf8_bytes() {
    // File opens but `read_to_string` fails with InvalidData
    // because the bytes don't form valid UTF-8. Walker fails open
    // and returns false.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let proj = home.join(".claude").join("projects").join("p");
    fs::create_dir_all(&proj).unwrap();
    let path = proj.join("invalid.jsonl");
    // 0xC3 starts a 2-byte UTF-8 sequence; 0x28 is `(` (not a
    // valid continuation byte), so the pair is invalid UTF-8.
    fs::write(&path, [0xC3u8, 0x28u8]).unwrap();
    assert!(!last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home
    ));
    assert!(!most_recent_skill_in_user_only_set(&path, home));
    assert!(!recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn walker_rejects_path_outside_home_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // Write a valid transcript at a path that does NOT live under
    // `<home>/.claude/projects/`. The validator rejects the path
    // even though the JSONL content is well-formed and would
    // otherwise match. Defense-in-depth: a hand-edited
    // `transcript_path` cannot redirect the walker outside the
    // canonical Claude Code transcript root.
    let stray = home.join("malicious").join("session.jsonl");
    fs::create_dir_all(stray.parent().unwrap()).unwrap();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n";
    fs::write(&stray, jsonl).unwrap();
    assert!(!last_user_message_invokes_skill(
        &stray,
        "flow:flow-abort",
        home
    ));
    assert!(!most_recent_skill_in_user_only_set(&stray, home));
}

// --- Adversarial regression tests ---
//
// Each test below locks in a fix surfaced by the Review
// adversarial / pre-mortem agents. Adding the test here protects
// against future regression.

#[test]
fn walker_rejects_path_traversal_via_dotdot_components() {
    // `Path::starts_with(<home>/.claude/projects)` is a lexical
    // prefix check that does NOT canonicalize `..` segments. A path
    // like `<home>/.claude/projects/../../evil.jsonl` passes the
    // prefix check but `File::open` resolves it OUT of the canonical
    // root. The validator must reject any ParentDir component before
    // the prefix check.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let evil = home.join("evil.jsonl");
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n";
    fs::write(&evil, jsonl).unwrap();
    fs::create_dir_all(home.join(".claude").join("projects").join("p")).unwrap();
    let traversal = home
        .join(".claude")
        .join("projects")
        .join("..")
        .join("..")
        .join("evil.jsonl");
    assert!(!last_user_message_invokes_skill(
        &traversal,
        "flow:flow-abort",
        home
    ));
    assert!(!most_recent_skill_in_user_only_set(&traversal, home));
}

#[test]
fn last_user_invokes_rejects_command_mention_in_user_prose() {
    // A user typing "what does <command-name>/flow:flow-abort</command-name>
    // do?" — the marker appears mid-string. The walker must require
    // the marker at the START of the trimmed content (slash-command
    // anchoring), not anywhere in the line.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"what does <command-name>/flow:flow-abort</command-name> do?\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home
    ));
}

#[test]
fn last_user_invokes_returns_false_when_user_turn_missing_content_field() {
    // Most recent user turn has a `message` field but no `content`
    // sub-field — the walker hits the user boundary and the
    // content-extraction match arm returns false. Exercises the
    // None branch of the content lookup.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home
    ));
}

#[test]
fn last_user_invokes_rejects_tool_result_wrapped_user_turn() {
    // Claude Code wraps tool results inside user-role turns whose
    // `content` is an array (not a string) of blocks. The
    // assistant-generated tool_result text inside such a turn must
    // NOT authorize a user-only skill invocation. Only string-
    // valued user content (direct user input) qualifies as a
    // slash-command invocation.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"tu_1\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home
    ));
}

#[test]
fn last_user_invokes_lowercases_skill_name_for_anchor_match() {
    // The walker normalizes the input skill via normalize_gate_input
    // (lowercase + trim + NUL-strip). Mixed-case input must still
    // match a properly-typed slash command in the transcript.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    // Mixed-case input — should match because both sides normalize.
    assert!(last_user_message_invokes_skill(
        &path,
        "Flow:Flow-Abort",
        home
    ));
}

#[test]
fn last_user_invokes_rejects_empty_skill_after_normalization() {
    // A `skill` argument that is purely whitespace, NULs, or empty
    // becomes an empty string after `normalize_gate_input`. Such a
    // value must not authorize anything — return false.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!last_user_message_invokes_skill(&path, "  \0  ", home));
    assert!(!last_user_message_invokes_skill(&path, "", home));
}

#[test]
fn last_user_message_invokes_skill_recognizes_two_line_shape_for_flow_continue() {
    // Claude Code 2.1.140+ emits user-typed slash commands as a
    // two-line content string: `<command-message>flow:flow-continue
    // </command-message>\n<command-name>/flow:flow-continue
    // </command-name>`. The walker must accept this shape AND the
    // legacy one-line shape via `starts_with` disjunction. This test
    // exercises the new shape for `flow:flow-continue` — the user-only
    // skill whose bug originally surfaced this regression (clear-halt
    // rejected as unauthorized because the legacy-only check failed
    // on the two-line emission).
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-message>flow:flow-continue</command-message>\\n<command-name>/flow:flow-continue</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(last_user_message_invokes_skill(
        &path,
        "flow:flow-continue",
        home
    ));
}

#[test]
fn last_user_message_invokes_skill_recognizes_two_line_shape_for_flow_abort() {
    // Per-skill coverage for the two-line emission shape — `flow:flow
    // -abort` is one of the five user-only skills the walker gates;
    // each must independently round-trip through the new shape.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-message>flow:flow-abort</command-message>\\n<command-name>/flow:flow-abort</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home
    ));
}

#[test]
fn last_user_message_invokes_skill_recognizes_two_line_shape_for_flow_reset() {
    // Per-skill coverage for the two-line emission shape — `flow:flow
    // -reset` is one of the five user-only skills the walker gates.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-message>flow:flow-reset</command-message>\\n<command-name>/flow:flow-reset</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(last_user_message_invokes_skill(
        &path,
        "flow:flow-reset",
        home
    ));
}

#[test]
fn last_user_message_invokes_skill_recognizes_two_line_shape_for_flow_release() {
    // Per-skill coverage for the two-line emission shape. The
    // `flow-release` skill is a project-local maintainer skill at
    // `.claude/skills/flow-release/`, so Claude Code emits the bare
    // skill name (no `flow:` namespace) in both the `<command-message>`
    // and `<command-name>` markers. See `USER_ONLY_SKILLS` doc comment
    // for the namespacing asymmetry.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-message>flow-release</command-message>\\n<command-name>/flow-release</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(last_user_message_invokes_skill(&path, "flow-release", home));
}

#[test]
fn last_user_message_invokes_skill_recognizes_two_line_shape_for_flow_prime() {
    // Per-skill coverage for the two-line emission shape — `flow:flow
    // -prime` is one of the five user-only skills the walker gates.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-message>flow:flow-prime</command-message>\\n<command-name>/flow:flow-prime</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(last_user_message_invokes_skill(
        &path,
        "flow:flow-prime",
        home
    ));
}

#[test]
fn last_user_message_invokes_skill_rejects_two_line_marker_mid_prose_anchoring_preserved() {
    // The walker's `starts_with` disjunction must preserve the slash-
    // command-anchoring property on BOTH shapes. A user typing prose
    // that mentions the literal marker substring mid-text does not
    // satisfy either anchor: the new-shape arm requires
    // `<command-message>...` as the literal first bytes of trimmed
    // content; the legacy-shape arm requires `<command-name>...` as
    // the literal first bytes. Prose like "I want to discuss..."
    // starts with neither, so the walker returns false.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"I want to discuss the /flow:flow-continue marker. The walker checks <command-name>/flow:flow-continue</command-name> at the start.\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!last_user_message_invokes_skill(
        &path,
        "flow:flow-continue",
        home
    ));
}

#[test]
fn most_recent_skill_walker_finds_user_only_in_multi_skill_turn() {
    // Assistant turn fires multiple Skill tool_use calls in the same
    // content array — first a non-user-only skill, then a user-only
    // one. The walker must scan ALL Skill blocks in the turn
    // (extract_skill_invocations returns a Vec), not return on the
    // first match. Otherwise the carve-out would miss the user-only
    // call when it appears after a non-user-only one.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"do things\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[\
{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-status\"}},\
{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-abort\"}}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(most_recent_skill_in_user_only_set(&path, home));
}

#[test]
fn normalize_gate_input_strips_nul_trims_and_lowercases() {
    use flow_rs::hooks::transcript_walker::normalize_gate_input;
    assert_eq!(normalize_gate_input("flow:flow-abort"), "flow:flow-abort");
    assert_eq!(
        normalize_gate_input("  flow:flow-abort  "),
        "flow:flow-abort"
    );
    assert_eq!(normalize_gate_input("Flow:Flow-Abort"), "flow:flow-abort");
    assert_eq!(normalize_gate_input("flow:flow-abort\0"), "flow:flow-abort");
    assert_eq!(
        normalize_gate_input("\0  Flow:flow-Abort  \0"),
        "flow:flow-abort"
    );
    assert_eq!(normalize_gate_input(""), "");
    assert_eq!(normalize_gate_input("   "), "");
}

// --- recent_edit_blocked_on_shared_config ---
//
// Companion to validate-ask-user's shared-config carve-out.
// Examines the most recent user-role turn in the transcript for a
// shared-config BLOCKED tool_result. Detection signal: the literal
// substring "is a shared configuration file that affects every
// engineer" inside a `tool_result` block whose `is_error: true`
// field is set. The substring is uniquely emitted by
// `validate_worktree_paths::validate_shared_config` (see the
// presence-contract test in tests/hooks/validate_worktree_paths.rs).

#[test]
fn helper_returns_true_when_recent_edit_was_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"please update reqs\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Edit\",\"id\":\"toolu_01\",\"input\":{\"file_path\":\"/p/requirements.txt\",\"old_string\":\"a\",\"new_string\":\"b\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_01\",\"content\":\"BLOCKED: requirements.txt is a shared configuration file that affects every engineer in the repository.\",\"is_error\":true}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_returns_true_when_recent_write_was_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"new gitignore\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Write\",\"id\":\"toolu_02\",\"input\":{\"file_path\":\"/p/.gitignore\",\"content\":\"foo\\n\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_02\",\"content\":\"BLOCKED: .gitignore is a shared configuration file that affects every engineer in the repository.\",\"is_error\":true}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_returns_false_when_no_block_in_window() {
    // Successful Edit — tool_result content does not contain the
    // shared-config substring and is_error is false. Helper returns
    // false so the autonomous-phase block stands.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"edit a file\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Edit\",\"id\":\"toolu_03\",\"input\":{\"file_path\":\"/p/src/foo.rs\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_03\",\"content\":\"The file has been updated.\",\"is_error\":false}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_returns_false_when_block_predates_user_turn() {
    // Block exists earlier in the transcript but a fresh real user
    // turn (string content) intervenes. Walker hits the user turn
    // first walking backward and returns false — the older block is
    // outside the window.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"first request\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Edit\",\"id\":\"toolu_04\",\"input\":{\"file_path\":\"/p/Cargo.toml\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_04\",\"content\":\"BLOCKED: Cargo.toml is a shared configuration file that affects every engineer in the repository.\",\"is_error\":true}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"now do something else\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_returns_false_when_tool_result_not_is_error() {
    // tool_result content contains the substring but `is_error`
    // is false (or absent). Without is_error: true, the block did
    // not fire — helper returns false.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"edit\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Edit\",\"id\":\"toolu_05\",\"input\":{\"file_path\":\"/p/foo\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_05\",\"content\":\"Note: this is a shared configuration file but the edit succeeded.\",\"is_error\":false}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_returns_false_when_substring_absent() {
    // is_error: true but the substring is absent — a different
    // block fired (e.g., a path-canonicalization rejection from
    // validate_worktree_paths). Helper returns false because the
    // detection signal is the substring, not the is_error flag.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"edit\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Edit\",\"id\":\"toolu_06\",\"input\":{\"file_path\":\"/p/foo\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_06\",\"content\":\"BLOCKED: misplaced .flow-states/ path; canonical destination is elsewhere.\",\"is_error\":true}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_returns_false_when_transcript_path_unsafe() {
    // Validator must reject relative paths, NUL-byte paths, and
    // ParentDir-component paths before any I/O. Match parent
    // helpers' rejection profile.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // Relative path (validator requires absolute under home).
    let relative = std::path::Path::new("relative/path.jsonl");
    assert!(!recent_edit_blocked_on_shared_config(relative, home));
    // ParentDir traversal that escapes home.
    std::fs::create_dir_all(home.join(".claude").join("projects").join("p")).unwrap();
    let evil = home.join("evil.jsonl");
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t\",\"content\":\"BLOCKED: foo is a shared configuration file that affects every engineer in the repository.\",\"is_error\":true}]}}\n";
    std::fs::write(&evil, jsonl).unwrap();
    let traversal = home
        .join(".claude")
        .join("projects")
        .join("..")
        .join("..")
        .join("evil.jsonl");
    assert!(!recent_edit_blocked_on_shared_config(&traversal, home));
}

#[test]
fn helper_returns_false_when_transcript_missing() {
    // File does not exist on disk — read_capped returns None and the
    // helper falls open to false.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let missing = home
        .join(".claude")
        .join("projects")
        .join("p")
        .join("nonexistent.jsonl");
    assert!(!recent_edit_blocked_on_shared_config(&missing, home));
}

#[test]
fn helper_returns_false_on_empty_transcript() {
    // Empty file — no lines to walk, helper returns false.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let path = crate::common::transcript_fixture(home, "p", "");
    assert!(!recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_handles_byte_cap_truncation() {
    // The 4 MB cap defines a backward-visibility window. A blocked
    // tool_result inside the last `SHARED_CONFIG_BLOCK_BYTE_CAP`
    // bytes is reachable — true. A block buried before the cap is
    // unreachable — false (documented acceptable false-negative).
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let proj = home.join(".claude").join("projects").join("p");
    std::fs::create_dir_all(&proj).unwrap();

    // Reachable: content within the window. Place padding after the
    // block to verify recency-window semantics for tail-bounded
    // reads.
    let path_in_window = proj.join("in_window.jsonl");
    let block = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t\",\"content\":\"BLOCKED: foo is a shared configuration file that affects every engineer in the repository.\",\"is_error\":true}]}}\n";
    let mut content_in: Vec<u8> = Vec::new();
    let padding_size = 1024 * 8; // 8 KB of padding — well within cap
    content_in.extend(std::iter::repeat_n(b'\n', padding_size));
    content_in.extend_from_slice(block.as_bytes());
    std::fs::write(&path_in_window, &content_in).unwrap();
    assert!(recent_edit_blocked_on_shared_config(&path_in_window, home));

    // Unreachable: block at HEAD, padding > cap pushes block out of
    // the tail-bounded read.
    let path_out_of_window = proj.join("out_of_window.jsonl");
    let mut content_out: Vec<u8> = block.as_bytes().to_vec();
    let oversized_pad = (SHARED_CONFIG_BLOCK_BYTE_CAP as usize) + 1024;
    content_out.extend(std::iter::repeat_n(b'\n', oversized_pad));
    std::fs::write(&path_out_of_window, &content_out).unwrap();
    assert!(!recent_edit_blocked_on_shared_config(
        &path_out_of_window,
        home
    ));
}

#[test]
fn helper_returns_false_when_no_assistant_turn_since_user() {
    // Only a real user turn — no assistant turn, no tool_result,
    // no block. Walker hits the user turn boundary, returns false.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_ignores_non_edit_write_tool_use() {
    // A Bash tool_use (not Edit/Write) with is_error: true but no
    // shared-config substring. The detection signal is the
    // substring, which validate_worktree_paths emits ONLY for
    // Edit/Write on shared-config files. Helper returns false.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"run a command\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_07\",\"input\":{\"command\":\"rm -rf /\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_07\",\"content\":\"BLOCKED: rm -rf / matches deny pattern.\",\"is_error\":true}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_skips_unparseable_jsonl_lines() {
    // A non-empty line that fails JSON parsing must be skipped via
    // continue. The valid block on the previous line still drives
    // the walker's true return.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t\",\"content\":\"BLOCKED: foo is a shared configuration file that affects every engineer in the repository.\",\"is_error\":true}]}}\n\
not valid json at all\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_returns_false_when_user_turn_missing_message_field() {
    // A user-role turn without a `message` field — the content
    // lookup returns None and the walker treats the line as a real
    // user-turn boundary, returning false.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\"}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_returns_false_when_user_content_is_number() {
    // `content` is neither a string nor an array — for example, a
    // number left over from a malformed write. Treated as a real
    // user turn boundary, helper returns false.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":42}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_continues_past_non_tool_result_block_in_user_array() {
    // User-array content has a leading non-tool_result block (e.g.,
    // a text or image block). The walker skips it via continue and
    // finds the trailing tool_result block.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"prefix\"},{\"type\":\"tool_result\",\"tool_use_id\":\"t\",\"content\":\"BLOCKED: foo is a shared configuration file that affects every engineer in the repository.\",\"is_error\":true}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_continues_past_tool_result_block_without_content_field() {
    // A tool_result block with `is_error: true` but no `content`
    // field. The walker continues past it and falls through to
    // false.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t\",\"is_error\":true}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_finds_block_when_tool_result_content_is_text_array() {
    // tool_result.content can be an array of content blocks, each
    // typically of type "text". The helper concatenates `text`
    // fields and matches the substring against the joined string.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t\",\"content\":[{\"type\":\"text\",\"text\":\"BLOCKED: foo is a shared configuration file that affects every engineer in the repository.\"},{\"type\":\"text\",\"text\":\"trailing context\"}],\"is_error\":true}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_skips_array_text_blocks_without_text_field() {
    // tool_result.content is an array, but content blocks vary in
    // shape. A leading non-text block (e.g., type "image") has no
    // `text` field so the helper's `if let Some(t)` skips it; two
    // following text blocks are joined with a single space. None of
    // the joined text contains the substring, so helper returns
    // false. Exercises the join-when-not-empty branch AND the
    // no-text-key skip branch in one test.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t\",\"content\":[{\"type\":\"image\"},{\"type\":\"text\",\"text\":\"no relevant content here\"},{\"type\":\"text\",\"text\":\"still nothing\"}],\"is_error\":true}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_accepts_is_error_string_true() {
    // is_truthy accepts the string "true" (case-insensitive) per
    // .claude/rules/rust-patterns.md "Hook Input Boolean Field
    // Tolerance". Some Claude Code wire-format variants may
    // serialize is_error as a string.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t\",\"content\":\"BLOCKED: foo is a shared configuration file that affects every engineer in the repository.\",\"is_error\":\"TRUE\"}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_accepts_is_error_number_one() {
    // is_truthy accepts non-zero numbers per the same rule —
    // is_error: 1 (integer) is treated as truthy.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t\",\"content\":\"BLOCKED: foo is a shared configuration file that affects every engineer in the repository.\",\"is_error\":1}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_rejects_is_error_number_zero() {
    // is_truthy rejects zero numbers — matches the falsy semantics.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t\",\"content\":\"BLOCKED: foo is a shared configuration file that affects every engineer in the repository.\",\"is_error\":0}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_rejects_is_error_string_false() {
    // is_truthy rejects strings other than "true" / "1".
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t\",\"content\":\"BLOCKED: foo is a shared configuration file that affects every engineer in the repository.\",\"is_error\":\"false\"}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_rejects_is_error_null() {
    // is_truthy rejects null and other non-bool/string/number
    // types — falls through to the wildcard arm.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t\",\"content\":\"BLOCKED: foo is a shared configuration file that affects every engineer in the repository.\",\"is_error\":null}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_returns_false_when_transcript_file_unreadable() {
    // `is_safe_transcript_path` canonicalize succeeds on a chmod-000
    // file (canonicalize stats components, not opens), but `File::open`
    // inside `read_capped` returns Err(PermissionDenied). The helper
    // falls open and returns false. Covers the File::open `.ok()?`
    // branch.
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let proj = home.join(".claude").join("projects").join("p");
    fs::create_dir_all(&proj).unwrap();
    let path = proj.join("session.jsonl");
    fs::write(&path, b"{\"type\":\"user\"}\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    struct PermGuard(std::path::PathBuf);
    impl Drop for PermGuard {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o644));
        }
    }
    let _g = PermGuard(path.clone());
    assert!(!recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_iterates_past_trailing_assistant_to_user_array_turn() {
    // Walker reverse iteration: when the LAST line is an assistant
    // turn (after the most recent user-array tool_result), the
    // walker continues past it (turn_type != "user") and reaches
    // the user-array turn carrying the shared-config block. Covers
    // the assistant-skip branch in the walker loop.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t\",\"content\":\"BLOCKED: foo is a shared configuration file that affects every engineer in the repository.\",\"is_error\":true}]}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Understood.\"}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn helper_continues_when_tool_result_content_is_number() {
    // tool_result.content is neither a string nor an array — e.g.,
    // a number from a malformed write. Helper continues past the
    // block and returns false at the user-turn boundary.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t\",\"content\":7,\"is_error\":true}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!recent_edit_blocked_on_shared_config(&path, home));
}

// --- most_recent_skill_since_user ---
//
// The `check_in_progress_utility_skill` predicate in `stop_continue.rs`
// consults this helper to discriminate "decompose just returned mid-
// pipeline" from "model just sent a normal conversational reply." The
// helper walks backward from the transcript file's tail, stops at the
// most recent real user turn, and returns the name of the last Skill
// tool_use call since that boundary. `None` means no Skill call has
// fired since the user typed; the marker-based block is suppressed
// for that case so the model can send a normal reply during
// discussion mode without triggering Stop-hook refusal.

#[test]
fn most_recent_skill_no_transcript_path() {
    // Path that does not exist: fails the validator's existence check
    // via `read_capped` returning None. Helper returns None (fail-open).
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let missing = home
        .join(".claude")
        .join("projects")
        .join("p")
        .join("nonexistent.jsonl");
    assert_eq!(most_recent_skill_since_user(&missing, home), None);
}

#[test]
fn most_recent_skill_invalid_path_rejected() {
    // Path outside `<home>/.claude/projects/` fails
    // `is_safe_transcript_path` validation. Helper returns None.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let stray = home.join("malicious").join("session.jsonl");
    fs::create_dir_all(stray.parent().unwrap()).unwrap();
    let jsonl = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"decompose:decompose\"}}]}}\n";
    fs::write(&stray, jsonl).unwrap();
    assert_eq!(most_recent_skill_since_user(&stray, home), None);
}

#[test]
fn most_recent_skill_empty_transcript() {
    // Empty file: no turns, no Skill calls. Helper returns None.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let path = crate::common::transcript_fixture(home, "p", "");
    assert_eq!(most_recent_skill_since_user(&path, home), None);
}

#[test]
fn most_recent_skill_no_skill_call_returns_none() {
    // User turn followed by an assistant text-only turn (no Skill
    // tool_use). The walker stops at the user boundary with no Skill
    // call captured. Helper returns None.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"hi back\"}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(most_recent_skill_since_user(&path, home), None);
}

#[test]
fn most_recent_skill_decompose_only() {
    // User turn, then assistant turn invoking `decompose:decompose`.
    // Helper returns `Some("decompose:decompose")`.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"decompose this\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"decompose:decompose\"}}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_skill_since_user(&path, home),
        Some("decompose:decompose".to_string()),
    );
}

#[test]
fn most_recent_skill_decompose_then_pm_returns_pm() {
    // User turn → assistant decompose call → assistant flow:pm call.
    // The walker returns the LAST Skill call before the user boundary
    // (in file order, the most recent Skill in the window). AC#3 last-
    // Skill-wins semantics: a chain of Skill calls in the same window
    // collapses to whatever fired most recently, and the block fires
    // ONLY when that final Skill is decompose.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"do it\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"decompose:decompose\"}}]}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:pm\"}}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_skill_since_user(&path, home),
        Some("flow:pm".to_string()),
    );
}

#[test]
fn most_recent_skill_synthetic_user_turn_ignored() {
    // Tool-result-wrapped user turn (content is an array of blocks,
    // not a string) is a synthetic user turn carrying tool output
    // back to the assistant. It must NOT count as a real user
    // boundary; the walker continues past it to find the most recent
    // real (string-content) user turn. Then the assistant decompose
    // call before that real user turn is invisible — the Skill call
    // in the window between the synthetic and real user turns wins.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // File order (earliest to latest):
    //   real user turn ("do it")
    //   assistant decompose Skill call
    //   synthetic user turn (tool_result array)
    //   assistant flow:pm Skill call
    // Walker stops at the real user turn; both Skill calls are in
    // the window; last-wins returns `flow:pm`.
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"do it\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"decompose:decompose\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t\",\"content\":\"ok\"}]}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:pm\"}}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_skill_since_user(&path, home),
        Some("flow:pm".to_string()),
    );
}

#[test]
fn most_recent_skill_byte_cap_enforced() {
    // A valid user turn + decompose Skill call sit at the file's HEAD,
    // followed by > TRANSCRIPT_BYTE_CAP bytes of padding. `read_capped`
    // reads only the LAST cap bytes; the head-positioned content is
    // invisible. Helper returns None (the truncated tail has no parseable
    // turns).
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let proj = home.join(".claude").join("projects").join("p");
    fs::create_dir_all(&proj).unwrap();
    let path = proj.join("oversized.jsonl");
    let leading = b"{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"decompose:decompose\"}}]}}\n";
    let mut content: Vec<u8> = leading.to_vec();
    let padding_size = (TRANSCRIPT_BYTE_CAP as usize) + 1024;
    content.extend(std::iter::repeat_n(b'\n', padding_size));
    fs::write(&path, &content).unwrap();
    assert_eq!(most_recent_skill_since_user(&path, home), None);
}

#[test]
fn most_recent_skill_non_utf8_file_returns_none() {
    // File opens but `read_to_string` inside `read_capped` fails on
    // non-UTF-8 bytes. `read_capped` returns None and the helper's
    // `?` operator propagates None.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let proj = home.join(".claude").join("projects").join("p");
    fs::create_dir_all(&proj).unwrap();
    let path = proj.join("invalid.jsonl");
    // 0xC3 starts a 2-byte UTF-8 sequence; 0x28 is `(` (not a valid
    // continuation byte), so the pair is invalid UTF-8.
    fs::write(&path, [0xC3u8, 0x28u8]).unwrap();
    assert_eq!(most_recent_skill_since_user(&path, home), None);
}

#[test]
fn most_recent_skill_unparseable_line_skipped() {
    // An unparseable JSONL line (not valid JSON) is skipped via the
    // `Err(_) => continue` branch. A valid assistant Skill turn at
    // the file's tail and a user turn earlier surround the bad
    // line. Walking reverse: assistant decompose → capture; bad
    // line → continue; user (real) → return captured.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n\
not valid json at all\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"decompose:decompose\"}}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_skill_since_user(&path, home),
        Some("decompose:decompose".to_string()),
    );
}

#[test]
fn most_recent_skill_unknown_turn_type_skipped() {
    // A turn whose `type` is neither "user" nor "assistant" (e.g.,
    // a "system" turn from a future Claude Code release) is skipped
    // via the `if turn_type != "assistant" { continue; }` branch.
    // The walker keeps iterating to find the user boundary.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n\
{\"type\":\"system\",\"message\":{\"role\":\"system\",\"content\":\"compaction summary\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"decompose:decompose\"}}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_skill_since_user(&path, home),
        Some("decompose:decompose".to_string()),
    );
}

// --- verify_agent_returned_in_phase ---

/// Helper: build a transcript fixture with three turn types woven
/// together as needed for the verifier tests. Each fixture has the
/// phase-enter Bash invocation in an assistant turn, then optionally
/// an Agent tool_use, then optionally its tool_result.
fn agent_turn_jsonl(
    bash_marker: &str,
    agent_block: Option<&str>,
    result_block: Option<&str>,
) -> String {
    let bash_turn = format!(
        "{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_bash_1\",\"input\":{{\"command\":\"{}\"}}}}]}}}}\n",
        bash_marker
    );
    let mut s = bash_turn;
    if let Some(block) = agent_block {
        s.push_str(block);
    }
    if let Some(block) = result_block {
        s.push_str(block);
    }
    s
}

#[test]
fn verify_agent_returned_rejects_unsafe_transcript_path() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // Relative path is rejected by is_safe_transcript_path.
    let relative = std::path::PathBuf::from("relative/path.jsonl");
    assert_eq!(
        verify_agent_returned_in_phase(&relative, home, "reviewer", "flow-review"),
        Err("transcript_path_invalid".to_string())
    );
}

#[test]
fn verify_agent_returned_rejects_when_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // is_safe_transcript_path canonicalizes the path; a missing file
    // fails canonicalize and is rejected as transcript_path_invalid.
    // The validator collapses "doesn't exist" and "doesn't validate"
    // into the same fail-closed result.
    let missing = home
        .join(".claude")
        .join("projects")
        .join("p")
        .join("nonexistent.jsonl");
    assert_eq!(
        verify_agent_returned_in_phase(&missing, home, "reviewer", "flow-review"),
        Err("transcript_path_invalid".to_string())
    );
}

#[test]
fn verify_agent_returned_returns_phase_marker_not_found_when_agent_or_phase_empty() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let path = crate::common::transcript_fixture(home, "p", "");
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "", "flow-review"),
        Err("phase_marker_not_found".to_string())
    );
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", ""),
        Err("phase_marker_not_found".to_string())
    );
}

#[test]
fn verify_agent_returned_returns_phase_marker_not_found_when_no_phase_enter_in_transcript() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"no phase-enter here\"}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Err("phase_marker_not_found".to_string())
    );
}

#[test]
fn verify_agent_returned_returns_phase_marker_not_found_when_phase_enter_targets_other_phase() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // phase-enter --phase flow-code (not flow-review).
    let jsonl = agent_turn_jsonl("bin/flow phase-enter --phase flow-code", None, None);
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Err("phase_marker_not_found".to_string())
    );
}

#[test]
fn verify_agent_returned_returns_tool_use_missing_when_no_agent_tool_use_after_marker() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = agent_turn_jsonl("bin/flow phase-enter --phase flow-review", None, None);
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Err("tool_use_missing".to_string())
    );
}

#[test]
fn verify_agent_returned_returns_tool_use_missing_when_subagent_type_mismatches() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let agent_block = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"toolu_agent_1\",\"input\":{\"subagent_type\":\"flow:pre-mortem\"}}]}}\n";
    let jsonl = agent_turn_jsonl(
        "bin/flow phase-enter --phase flow-review",
        Some(agent_block),
        None,
    );
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Err("tool_use_missing".to_string())
    );
}

#[test]
fn verify_agent_returned_returns_tool_result_missing_when_no_matching_tool_result() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let agent_block = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"toolu_agent_1\",\"input\":{\"subagent_type\":\"flow:reviewer\"}}]}}\n";
    let jsonl = agent_turn_jsonl(
        "bin/flow phase-enter --phase flow-review",
        Some(agent_block),
        None,
    );
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Err("tool_result_missing".to_string())
    );
}

#[test]
fn verify_agent_returned_returns_tool_result_missing_when_tool_use_id_mismatches() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let agent_block = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"toolu_agent_1\",\"input\":{\"subagent_type\":\"flow:reviewer\"}}]}}\n";
    // Mismatched id on tool_result.
    let result_block = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_other\",\"content\":\"hi\"}]}}\n";
    let jsonl = agent_turn_jsonl(
        "bin/flow phase-enter --phase flow-review",
        Some(agent_block),
        Some(result_block),
    );
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Err("tool_result_missing".to_string())
    );
}

#[test]
fn verify_agent_returned_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let agent_block = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"toolu_agent_1\",\"input\":{\"subagent_type\":\"flow:reviewer\"}}]}}\n";
    let result_block = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_agent_1\",\"content\":\"findings here\"}]}}\n";
    let jsonl = agent_turn_jsonl(
        "bin/flow phase-enter --phase flow-review",
        Some(agent_block),
        Some(result_block),
    );
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Ok(())
    );
}

#[test]
fn verify_agent_returned_normalizes_agent_and_phase_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let agent_block = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"toolu_agent_1\",\"input\":{\"subagent_type\":\"flow:learn-analyst\"}}]}}\n";
    let result_block = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_agent_1\",\"content\":\"findings\"}]}}\n";
    let jsonl = agent_turn_jsonl(
        "bin/flow phase-enter --phase flow-learn",
        Some(agent_block),
        Some(result_block),
    );
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    // Mixed-case + whitespace inputs should normalize to canonical
    // values and find the agent.
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "  LEARN-ANALYST  ", "FLOW-LEARN"),
        Ok(())
    );
}

#[test]
fn verify_agent_returned_uses_most_recent_phase_enter() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // First phase-enter for flow-review with NO agent invocation.
    // Then a later phase-enter for flow-review WITH agent + result.
    // The verifier should anchor at the LAST phase-enter marker and
    // succeed (not be fooled by the earlier one without agent).
    let first = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_b1\",\"input\":{\"command\":\"bin/flow phase-enter --phase flow-review\"}}]}}\n";
    let interior =
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"interim message\"}}\n";
    let second = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_b2\",\"input\":{\"command\":\"bin/flow phase-enter --phase flow-review\"}}]}}\n";
    let agent_block = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"toolu_a1\",\"input\":{\"subagent_type\":\"flow:reviewer\"}}]}}\n";
    let result_block = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_a1\",\"content\":\"findings\"}]}}\n";
    let mut jsonl = String::new();
    jsonl.push_str(first);
    jsonl.push_str(interior);
    jsonl.push_str(second);
    jsonl.push_str(agent_block);
    jsonl.push_str(result_block);
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Ok(())
    );
}

#[test]
fn verify_agent_returned_skips_agent_invocation_before_phase_marker() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // Agent tool_use + tool_result happen BEFORE the phase-enter
    // marker. The forward walk from the marker sees no agent
    // invocation and reports tool_use_missing.
    let agent_block = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"toolu_a1\",\"input\":{\"subagent_type\":\"flow:reviewer\"}}]}}\n";
    let result_block = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_a1\",\"content\":\"findings\"}]}}\n";
    let marker = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_b1\",\"input\":{\"command\":\"bin/flow phase-enter --phase flow-review\"}}]}}\n";
    let mut jsonl = String::new();
    jsonl.push_str(agent_block);
    jsonl.push_str(result_block);
    jsonl.push_str(marker);
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Err("tool_use_missing".to_string())
    );
}

#[test]
fn verify_agent_returned_succeeds_with_multi_tool_assistant_turn() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // Assistant turn carries Bash + Agent in the same content array.
    // The Agent tool_use must still be discovered.
    let marker_with_agent = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_b1\",\"input\":{\"command\":\"bin/flow phase-enter --phase flow-review\"}}]}}\n";
    let combined = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"plan\"},{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"toolu_a1\",\"input\":{\"subagent_type\":\"flow:reviewer\"}}]}}\n";
    let result_block = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_a1\",\"content\":\"ok\"}]}}\n";
    let mut jsonl = String::new();
    jsonl.push_str(marker_with_agent);
    jsonl.push_str(combined);
    jsonl.push_str(result_block);
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Ok(())
    );
}

#[test]
fn verify_agent_returned_ignores_malformed_jsonl_lines() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let marker = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_b1\",\"input\":{\"command\":\"bin/flow phase-enter --phase flow-review\"}}]}}\n";
    let garbage = "this is not json\n\n";
    let agent_block = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"toolu_a1\",\"input\":{\"subagent_type\":\"flow:reviewer\"}}]}}\n";
    let result_block = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_a1\",\"content\":\"findings\"}]}}\n";
    let mut jsonl = String::new();
    jsonl.push_str(garbage);
    jsonl.push_str(marker);
    jsonl.push_str(garbage);
    jsonl.push_str(agent_block);
    jsonl.push_str(garbage);
    jsonl.push_str(result_block);
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Ok(())
    );
}

#[test]
fn verify_agent_returned_reports_phase_marker_not_found_for_non_utf8_content() {
    use std::fs;
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let proj = home.join(".claude").join("projects").join("p");
    fs::create_dir_all(&proj).unwrap();
    let path = proj.join("invalid-utf8.jsonl");
    // Bytes that cannot be decoded as UTF-8. is_safe_transcript_path
    // succeeds (path canonicalizes), but read_capped's
    // read_to_string fails, returning None. The verifier then maps
    // the None to phase_marker_not_found.
    fs::write(&path, [0xFF, 0xFE, 0xFD]).unwrap();
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Err("phase_marker_not_found".to_string())
    );
}

#[test]
fn verify_agent_returned_ignores_assistant_turn_with_string_content() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // verify_agent_returned_in_phase runs three passes. (1) Backward
    // scan over parsed lines locates the LAST phase-enter marker —
    // here the assistant turn with the Bash tool_use. (2) Forward
    // scan from marker+1 finds the Agent tool_use; the
    // string_content assistant turn fails find_agent_tool_use_id's
    // content.as_array check (string, not array) and the scan
    // continues to agent_block. (3) Forward scan from marker+1 finds
    // the matching tool_result; the string_content and agent_block
    // turns both fail user_turn_carries_tool_result_for's
    // type=="user" check, and the scan lands on result_block.
    let marker = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_b1\",\"input\":{\"command\":\"bin/flow phase-enter --phase flow-review\"}}]}}\n";
    let string_content = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"plain text response\"}}\n";
    let agent_block = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"toolu_a1\",\"input\":{\"subagent_type\":\"flow:reviewer\"}}]}}\n";
    let result_block = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_a1\",\"content\":\"ok\"}]}}\n";
    let mut jsonl = String::new();
    jsonl.push_str(marker);
    jsonl.push_str(string_content);
    jsonl.push_str(agent_block);
    jsonl.push_str(result_block);
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Ok(())
    );
}

#[test]
fn verify_agent_returned_walks_past_real_user_turn_between_marker_and_agent() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // Real user turn (string content) appears between the marker and
    // the agent tool_use. find_agent_tool_use_id sees the user turn
    // and returns None (turn_type != "assistant"), so the search
    // continues to the agent block. user_turn_carries_tool_result_for
    // also runs against this user turn during the result search and
    // takes the "string content => not array" early-return branch.
    let marker = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_b1\",\"input\":{\"command\":\"bin/flow phase-enter --phase flow-review\"}}]}}\n";
    let intervening_user =
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"continue please\"}}\n";
    let agent_block = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"toolu_a1\",\"input\":{\"subagent_type\":\"flow:reviewer\"}}]}}\n";
    let result_block = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_a1\",\"content\":\"ok\"}]}}\n";
    let mut jsonl = String::new();
    jsonl.push_str(marker);
    jsonl.push_str(intervening_user);
    jsonl.push_str(agent_block);
    jsonl.push_str(result_block);
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Ok(())
    );
}

#[test]
fn verify_agent_returned_skips_non_agent_tool_use_in_search_window() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // Assistant turn between marker and Agent contains a Bash
    // tool_use that is NOT the target Agent. find_agent_tool_use_id
    // hits the `name != "Agent" && name != "Task"` continue branch.
    let marker = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_b1\",\"input\":{\"command\":\"bin/flow phase-enter --phase flow-review\"}}]}}\n";
    let other_bash = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_b2\",\"input\":{\"command\":\"git status\"}}]}}\n";
    let other_bash_result = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_b2\",\"content\":\"clean\"}]}}\n";
    let agent_block = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"toolu_a1\",\"input\":{\"subagent_type\":\"flow:reviewer\"}}]}}\n";
    let result_block = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_a1\",\"content\":\"ok\"}]}}\n";
    let mut jsonl = String::new();
    jsonl.push_str(marker);
    jsonl.push_str(other_bash);
    jsonl.push_str(other_bash_result);
    jsonl.push_str(agent_block);
    jsonl.push_str(result_block);
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Ok(())
    );
}

#[test]
fn verify_agent_returned_skips_non_tool_result_block_in_result_search() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // After the agent tool_use, an unrelated tool_result for a Bash
    // call appears first, then the matching tool_result follows.
    // user_turn_carries_tool_result_for's `block.type != tool_result`
    // continue branch is hit by the text content block inside the
    // mixed user turn.
    let marker = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_b1\",\"input\":{\"command\":\"bin/flow phase-enter --phase flow-review\"}}]}}\n";
    let agent_block = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"toolu_a1\",\"input\":{\"subagent_type\":\"flow:reviewer\"}}]}}\n";
    let mixed_user_turn = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"context\"},{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_a1\",\"content\":\"ok\"}]}}\n";
    let mut jsonl = String::new();
    jsonl.push_str(marker);
    jsonl.push_str(agent_block);
    jsonl.push_str(mixed_user_turn);
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Ok(())
    );
}

#[test]
fn verify_agent_returned_finds_phase_marker_in_oversized_transcript() {
    // Guards the invariant that verify_agent_returned_in_phase finds
    // the phase-enter marker even when the transcript exceeds
    // TRANSCRIPT_BYTE_CAP. The fixture places the phase-enter Bash
    // marker at byte 0, pads with filler turns past the cap, and
    // places the Agent tool_use plus its paired tool_result at the
    // tail. The verifier must return Ok(()) — a tail-bounded read
    // would seek past the head marker and return
    // Err("phase_marker_not_found"), so the test fails the moment
    // the verifier's read window shrinks below the whole-file scope.
    //
    // Memory profile: the fixture is composed as a single String and
    // written via one fs::write, allocating ~51 MB transiently.
    // Acceptable on any modern dev machine; test runtime is roughly
    // 1-2s on SSD.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let proj = home.join(".claude").join("projects").join("oversized");
    fs::create_dir_all(&proj).unwrap();
    let path = proj.join("session.jsonl");

    let marker_turn = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_bash_marker\",\"input\":{\"command\":\"bin/flow phase-enter --phase flow-review\"}}]}}\n";
    let agent_turn = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"test-id-1\",\"input\":{\"subagent_type\":\"flow:reviewer\",\"description\":\"review\",\"prompt\":\"x\"}}]}}\n";
    let result_turn = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"test-id-1\",\"content\":\"ok\"}]}}\n";
    let filler = "{\"type\":\"system\",\"message\":{\"role\":\"system\",\"content\":\"filler\"}}\n";

    let padding_target = (TRANSCRIPT_BYTE_CAP as usize) + 1024 * 1024;
    let filler_count = padding_target / filler.len() + 1;
    let mut content = String::with_capacity(
        marker_turn.len() + filler_count * filler.len() + agent_turn.len() + result_turn.len(),
    );
    content.push_str(marker_turn);
    for _ in 0..filler_count {
        content.push_str(filler);
    }
    content.push_str(agent_turn);
    content.push_str(result_turn);
    fs::write(&path, content.as_bytes()).unwrap();

    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Ok(())
    );
}

// --- verify_agent_returned_in_phase retry / marker discipline ---
//
// Per the Review pre-mortem findings P3 (find_agent_tool_use_id
// first-match) and P5 (substring phase-marker match), the verifier
// scan handles two additional cases beyond the basic happy path:
//
//   - Retried agent invocations: an agent that truncated/failed on
//     its first attempt and succeeded on a later attempt produces
//     two `tool_use` entries (different ids) and one `tool_result`
//     (matched to the second id). The scan returns Ok the moment a
//     `tool_result` matches ANY collected candidate id.
//   - Token-aware phase-enter detection: the marker scan requires
//     `bin/flow` (bare or absolute) then `phase-enter` as the
//     command's first two tokens. Bash commands that mention the
//     phase name as substring (e.g. `echo "phase-enter --phase
//     flow-review"`, `bin/flow log "...phase-enter..."`) are not
//     treated as phase boundaries.

#[test]
fn verify_agent_returned_accepts_retry_where_only_second_invocation_has_tool_result() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // marker → agent_block_1 (toolu_a1, NO matching tool_result) →
    // agent_block_2 (toolu_a2, WITH matching tool_result). The
    // forward scan collects both tool_use ids and returns Ok when
    // the tool_result for toolu_a2 lands. Locks in the retry shape
    // the SKILL.md retry-3-then-note loop produces when attempt #2
    // succeeds after attempt #1 truncated.
    let marker = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_b1\",\"input\":{\"command\":\"bin/flow phase-enter --phase flow-review\"}}]}}\n";
    let agent_block_1 = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"toolu_a1\",\"input\":{\"subagent_type\":\"flow:reviewer\"}}]}}\n";
    let agent_block_2 = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"toolu_a2\",\"input\":{\"subagent_type\":\"flow:reviewer\"}}]}}\n";
    let result_block_2 = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_a2\",\"content\":\"ok\"}]}}\n";
    let mut jsonl = String::new();
    jsonl.push_str(marker);
    jsonl.push_str(agent_block_1);
    jsonl.push_str(agent_block_2);
    jsonl.push_str(result_block_2);
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Ok(())
    );
}

#[test]
fn verify_agent_returned_rejects_bash_log_substring_as_phase_marker() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // The Bash command's first token is `bin/flow` but the second
    // token is `log`, not `phase-enter`. The marker substring
    // appears inside a quoted argument. cmd_invokes_phase_enter
    // rejects on the tokens[1] != "phase-enter" check, so the scan
    // does not pin to this turn. With no real phase-enter elsewhere,
    // verification returns phase_marker_not_found.
    let bash_log_substring = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_b1\",\"input\":{\"command\":\"bin/flow log b \\\"[Phase 3] phase-enter --phase flow-review beacon noted\\\"\"}}]}}\n";
    let agent_block = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"toolu_a1\",\"input\":{\"subagent_type\":\"flow:reviewer\"}}]}}\n";
    let result_block = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_a1\",\"content\":\"ok\"}]}}\n";
    let mut jsonl = String::new();
    jsonl.push_str(bash_log_substring);
    jsonl.push_str(agent_block);
    jsonl.push_str(result_block);
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Err("phase_marker_not_found".to_string())
    );
}

#[test]
fn verify_agent_returned_rejects_echo_substring_as_phase_marker() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // The Bash command's first token is `echo`, not `bin/flow`.
    // cmd_invokes_phase_enter's first-token check rejects, so the
    // marker scan does not pin to this turn even though the
    // substring appears inside the argument.
    let echo_substring = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_b1\",\"input\":{\"command\":\"echo \\\"phase-enter --phase flow-review here\\\"\"}}]}}\n";
    let jsonl = echo_substring.to_string();
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Err("phase_marker_not_found".to_string())
    );
}

#[test]
fn verify_agent_returned_accepts_absolute_bin_flow_path_in_phase_marker() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // The Bash command's first token is an absolute path that ends
    // with `/bin/flow`. cmd_invokes_phase_enter's first-token check
    // accepts this form (matching the canonical
    // `${CLAUDE_PLUGIN_ROOT}/bin/flow phase-enter ...` invocation in
    // SKILL.md bash blocks). Verification succeeds.
    let marker = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_b1\",\"input\":{\"command\":\"/Users/ben/code/flow/bin/flow phase-enter --phase flow-review --steps-total 4\"}}]}}\n";
    let agent_block = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"toolu_a1\",\"input\":{\"subagent_type\":\"flow:reviewer\"}}]}}\n";
    let result_block = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_a1\",\"content\":\"ok\"}]}}\n";
    let mut jsonl = String::new();
    jsonl.push_str(marker);
    jsonl.push_str(agent_block);
    jsonl.push_str(result_block);
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Ok(())
    );
}

#[test]
fn verify_agent_returned_rejects_phase_equals_wrong_value() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // The Bash command uses `--phase=other-phase` (= syntax) for a
    // different phase than the verifier is searching for. The
    // strip_prefix branch matches but `rest == phase` returns
    // false. cmd_invokes_phase_enter falls through the loop and
    // returns false, so no marker is found and verification
    // returns phase_marker_not_found. Covers the else of the
    // inner `if rest == phase` branch.
    let marker_wrong_phase = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_b1\",\"input\":{\"command\":\"bin/flow phase-enter --phase=flow-code --steps-total=4\"}}]}}\n";
    let jsonl = marker_wrong_phase.to_string();
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Err("phase_marker_not_found".to_string())
    );
}

#[test]
fn verify_agent_returned_walks_past_string_content_user_turn_between_agent_and_result() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // marker → agent_block (collect toolu_a1) → string_content_user
    // → result_block. After candidate_ids has at least one id, the
    // loop calls user_turn_carries_tool_result_for on the
    // string-content user turn, which hits the
    // `content.as_array → None` branch and returns false. The scan
    // continues and lands on result_block, which matches.
    let marker = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_b1\",\"input\":{\"command\":\"bin/flow phase-enter --phase flow-review\"}}]}}\n";
    let agent_block = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"toolu_a1\",\"input\":{\"subagent_type\":\"flow:reviewer\"}}]}}\n";
    let string_content_user =
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"continue please\"}}\n";
    let result_block = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_a1\",\"content\":\"ok\"}]}}\n";
    let mut jsonl = String::new();
    jsonl.push_str(marker);
    jsonl.push_str(agent_block);
    jsonl.push_str(string_content_user);
    jsonl.push_str(result_block);
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Ok(())
    );
}

#[test]
fn verify_agent_returned_accepts_phase_equals_value_syntax() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // The phase argument uses `--phase=<value>` instead of space-
    // separated `--phase <value>`. clap accepts both; the
    // cmd_invokes_phase_enter strip_prefix branch matches the
    // `--phase=` form so the marker scan accepts either invocation
    // shape.
    let marker = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_b1\",\"input\":{\"command\":\"bin/flow phase-enter --phase=flow-review --steps-total=4\"}}]}}\n";
    let agent_block = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"toolu_a1\",\"input\":{\"subagent_type\":\"flow:reviewer\"}}]}}\n";
    let result_block = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_a1\",\"content\":\"ok\"}]}}\n";
    let mut jsonl = String::new();
    jsonl.push_str(marker);
    jsonl.push_str(agent_block);
    jsonl.push_str(result_block);
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Ok(())
    );
}

#[test]
fn verify_agent_returned_accepts_minimal_three_token_phase_equals_form() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // The minimum legitimate phase-enter invocation has 3 tokens:
    // `bin/flow phase-enter --phase=flow-review`. Earlier P5 fix
    // included a `tokens.len() < 4` check that rejected this 3-token
    // form. cmd_invokes_phase_enter now requires only >= 3 tokens
    // (tokens[0] = bin/flow, tokens[1] = phase-enter, tokens[2..] =
    // phase flag scan).
    let marker = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_b1\",\"input\":{\"command\":\"bin/flow phase-enter --phase=flow-review\"}}]}}\n";
    let agent_block = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"toolu_a1\",\"input\":{\"subagent_type\":\"flow:reviewer\"}}]}}\n";
    let result_block = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_a1\",\"content\":\"ok\"}]}}\n";
    let mut jsonl = String::new();
    jsonl.push_str(marker);
    jsonl.push_str(agent_block);
    jsonl.push_str(result_block);
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Ok(())
    );
}

#[test]
fn verify_agent_returned_walks_past_id_less_agent_block_to_valid_sibling() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // An assistant turn contains TWO Agent tool_use blocks for the
    // same subagent: the first is malformed (no `id` field), the
    // second is well-formed with id `toolu_a2`. find_agent_tool_use_id
    // continues past the id-less block (instead of returning None on
    // the first match) so the well-formed sibling is found. The
    // tool_result for `toolu_a2` then completes the verification.
    let marker = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_b1\",\"input\":{\"command\":\"bin/flow phase-enter --phase flow-review\"}}]}}\n";
    let dual_agent_turn = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Agent\",\"input\":{\"subagent_type\":\"flow:reviewer\"}},{\"type\":\"tool_use\",\"name\":\"Agent\",\"id\":\"toolu_a2\",\"input\":{\"subagent_type\":\"flow:reviewer\"}}]}}\n";
    let result_block = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_a2\",\"content\":\"ok\"}]}}\n";
    let mut jsonl = String::new();
    jsonl.push_str(marker);
    jsonl.push_str(dual_agent_turn);
    jsonl.push_str(result_block);
    let path = crate::common::transcript_fixture(home, "p", &jsonl);
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Ok(())
    );
}

// --- most_recent_user_message_since_skill_action ---
//
// The `check_halt_pending` predicate in `stop_continue.rs` consults
// this helper to find the user's most recent prose message after the
// most recent assistant Skill `tool_use`. The walker stops at the
// latest Skill call (going forward through the file in time order)
// and returns the latest string-content user turn that lands after
// it. Synthetic tool_result-wrapped user turns (content as array of
// blocks) are skipped — they carry tool output, not user intent.

#[test]
fn most_recent_user_message_since_skill_action_returns_string_content_user_turn_after_skill_call() {
    // Assistant Skill call, then real user turn with prose content.
    // Helper returns the user content.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"start\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-code\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"wait, hold up\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_user_message_since_skill_action(&path, home),
        Some("wait, hold up".to_string()),
    );
}

#[test]
fn most_recent_user_message_since_skill_action_skips_tool_result_array_wrappers() {
    // Synthetic tool_result-wrapped user turn (array content) between
    // the Skill call and a real string-content user turn. The walker
    // skips the array wrapper and returns the real user prose.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"start\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-code\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t\",\"content\":\"ok\"}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"real prose\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_user_message_since_skill_action(&path, home),
        Some("real prose".to_string()),
    );
}

#[test]
fn most_recent_user_message_since_skill_action_returns_none_when_no_user_turn_after_skill_call() {
    // Skill is the most recent action; no subsequent user turn. The
    // halt-pause predicate's caller treats `None` as "no new user
    // message since the model last acted" — Stop event proceeds.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-code\"}}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_user_message_since_skill_action(&path, home),
        None,
    );
}

#[test]
fn most_recent_user_message_since_skill_action_returns_none_when_no_skill_action_present() {
    // No assistant Skill call anywhere in the transcript. The window
    // the walker scopes (after the most recent Skill) is empty, so
    // user turns before any Skill call are invisible. Helper returns
    // `None`.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"hi\"}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"goodbye\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_user_message_since_skill_action(&path, home),
        None,
    );
}

#[test]
fn most_recent_user_message_since_skill_action_rejects_unsafe_transcript_path() {
    // Path outside `<home>/.claude/projects/` fails
    // `is_safe_transcript_path` validation. Helper returns `None`
    // without opening the file. Mirrors the path-validation pattern
    // used by the sibling walkers.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let stray = home.join("malicious").join("session.jsonl");
    fs::create_dir_all(stray.parent().unwrap()).unwrap();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n";
    fs::write(&stray, jsonl).unwrap();
    assert_eq!(
        most_recent_user_message_since_skill_action(&stray, home),
        None,
    );
}

#[test]
fn most_recent_user_message_since_skill_action_non_utf8_file_returns_none() {
    // File opens but `read_to_string` inside `read_capped` fails on
    // non-UTF-8 bytes. `read_capped` returns `None` and the helper's
    // `?` operator propagates `None`. Mirrors the
    // `most_recent_skill_non_utf8_file_returns_none` shape.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let proj = home.join(".claude").join("projects").join("p");
    fs::create_dir_all(&proj).unwrap();
    let path = proj.join("invalid.jsonl");
    // 0xC3 starts a 2-byte UTF-8 sequence; 0x28 is `(` (not a valid
    // continuation byte), so the pair is invalid UTF-8.
    fs::write(&path, [0xC3u8, 0x28u8]).unwrap();
    assert_eq!(
        most_recent_user_message_since_skill_action(&path, home),
        None,
    );
}

#[test]
fn most_recent_user_message_since_skill_action_skips_empty_lines() {
    // A blank line between the Skill call and the user turn must be
    // skipped via the `if trimmed.is_empty() { continue; }` branch
    // without affecting the candidate. JSONL transcripts can carry
    // blank lines from interrupted writes or hand-edits.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-code\"}}]}}\n\
\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hold up\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_user_message_since_skill_action(&path, home),
        Some("hold up".to_string()),
    );
}

#[test]
fn most_recent_user_message_since_skill_action_unparseable_line_skipped() {
    // An unparseable JSONL line is skipped via the
    // `Err(_) => continue` branch. The valid Skill call and
    // subsequent user turn surrounding the bad line still produce
    // the correct candidate.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-code\"}}]}}\n\
not valid json at all\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hold up\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_user_message_since_skill_action(&path, home),
        Some("hold up".to_string()),
    );
}

#[test]
fn most_recent_user_message_since_skill_action_unknown_turn_type_skipped() {
    // A turn whose `type` is neither `user` nor `assistant`
    // (e.g. a future `system` turn shape from Claude Code) is
    // skipped via the `if turn_type != "user" { continue; }`
    // branch. The walker continues past it without affecting the
    // candidate.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-code\"}}]}}\n\
{\"type\":\"system\",\"message\":{\"role\":\"system\",\"content\":\"compaction summary\"}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hold up\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_user_message_since_skill_action(&path, home),
        Some("hold up".to_string()),
    );
}

#[test]
fn most_recent_user_message_since_skill_action_skips_slash_command_legacy_shape() {
    // A slash-command-shape user turn (legacy
    // `<command-name>/<skill></command-name>` emission) is
    // imperative input, not conversational prose. The walker
    // must skip it so a user typing `/flow:flow-explore` mid-
    // autonomous-flow does not trigger the halt-pause contract.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-code\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-explore</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_user_message_since_skill_action(&path, home),
        None,
    );
}

#[test]
fn most_recent_user_message_since_skill_action_skips_slash_command_two_line_shape() {
    // The two-line slash-command emission Claude Code 2.1.140+
    // produces (`<command-message><skill></command-message>\n<command-name>/<skill></command-name>`)
    // is the same imperative-input shape. The walker filters it
    // for the same reason as the legacy shape.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-code\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-message>flow:flow-explore</command-message>\\n<command-name>/flow:flow-explore</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_user_message_since_skill_action(&path, home),
        None,
    );
}

#[test]
fn most_recent_user_message_since_skill_action_skips_flow_continue() {
    // A user-typed `/flow:flow-continue` is the resume directive.
    // The walker filters it so the halt-pause contract does not
    // re-fire on every subsequent Stop event after the user has
    // resumed the flow.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-code\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-continue</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_user_message_since_skill_action(&path, home),
        None,
    );
}

#[test]
fn most_recent_user_message_since_skill_action_continue_watermarks_preceding_prose() {
    // `/flow:flow-continue` is the universal resume directive: it
    // clears the candidate, watermarking any preceding prose. A
    // user who first sent a pause message and then typed
    // `/flow:flow-continue` has answered their own pause — the
    // walker returns `None` so the next Stop sees no fresh user
    // message and Rule 1 (encouraging-message refusal) fires.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-code\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"please pause\"}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-continue</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_user_message_since_skill_action(&path, home),
        None,
    );
}

#[test]
fn most_recent_user_message_since_skill_action_prose_after_continue_recaptures() {
    // Prose typed AFTER `/flow:flow-continue` is a new
    // conversational signal. The watermark cleared the earlier
    // prose, but prose that follows the resume directive is a
    // fresh halt-trigger.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-code\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"prose A\"}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-continue</command-name>\"}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"prose B\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_user_message_since_skill_action(&path, home),
        Some("prose B".to_string()),
    );
}

#[test]
fn most_recent_user_message_since_skill_action_arbitrary_slash_command_does_not_watermark() {
    // Only `/flow:flow-continue` is the resume directive. Other
    // slash commands (e.g. `/flow:flow-abort`) are filtered from
    // candidate capture (they are imperative input) but they do
    // NOT clear preceding prose — the user's pause prose is still
    // a legitimate halt-trigger. The walker returns the prose.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-code\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"please pause\"}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_user_message_since_skill_action(&path, home),
        Some("please pause".to_string()),
    );
}

#[test]
fn most_recent_user_message_since_skill_action_plain_prose_still_captures() {
    // Baseline: a plain prose user turn after a Skill action is
    // still captured. The slash-command filter must not regress
    // the conversational pass-through path the halt-pause
    // contract depends on.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-code\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"wait, why X?\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_user_message_since_skill_action(&path, home),
        Some("wait, why X?".to_string()),
    );
}

#[test]
fn most_recent_user_message_since_skill_action_skips_uppercase_command_name_tag() {
    // The slash-command filter must lowercase the content before
    // the tag check per `.claude/rules/security-gates.md`
    // "Normalize Before Comparing". A case-variant tag from a
    // hostile or future-shape transcript must still be classified
    // as imperative input — not captured as halt-trigger prose.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-code\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<COMMAND-NAME>/flow:flow-continue</COMMAND-NAME>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_user_message_since_skill_action(&path, home),
        None,
    );
}

#[test]
fn most_recent_user_message_since_skill_action_skips_mixedcase_command_message_tag() {
    // Two-line emission shape with title-cased tags must also
    // pass through the filter as imperative input. Same
    // normalize-both-sides discipline as the uppercase
    // <COMMAND-NAME> case.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-code\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<Command-Message>flow:flow-continue</Command-Message>\\n<Command-Name>/flow:flow-continue</Command-Name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_user_message_since_skill_action(&path, home),
        None,
    );
}

#[test]
fn most_recent_user_message_since_skill_action_uppercase_continue_watermarks_preceding_prose() {
    // The watermark trigger itself must also be case-insensitive.
    // `/FLOW:FLOW-CONTINUE` with uppercase tags is still the
    // resume directive and must clear the preceding prose, or the
    // walker returns prose, `check_autonomous_stop` re-arms
    // `_halt_pending` on the next Stop, and the autonomous flow
    // deadlocks despite the user's explicit resume.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-code\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"please pause\"}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<COMMAND-NAME>/FLOW:FLOW-CONTINUE</COMMAND-NAME>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_user_message_since_skill_action(&path, home),
        None,
    );
}

// --- any_skill_in_set_since_user ---
//
// `validate_pretool::bootstrap_carveout_applies` consults this helper
// to verify a sanctioned bootstrap parent (`flow:flow-start` or
// `flow:flow-prime`) appears in the assistant Skill chain since the
// most recent real user turn. The helper mirrors
// `most_recent_skill_since_user`'s backward walk but answers a
// membership question over a caller-supplied set rather than
// returning the most recent name.

#[test]
fn any_skill_returns_false_on_unsafe_transcript_path() {
    // Path outside `<home>/.claude/projects/` fails
    // `is_safe_transcript_path` validation. Helper returns false
    // (fail-open).
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let stray = home.join("malicious").join("session.jsonl");
    fs::create_dir_all(stray.parent().unwrap()).unwrap();
    let jsonl = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-start\"}}]}}\n";
    fs::write(&stray, jsonl).unwrap();
    assert!(!any_skill_in_set_since_user(
        &stray,
        home,
        &["flow:flow-start", "flow:flow-prime"],
    ));
}

#[test]
fn any_skill_returns_false_on_non_utf8_file() {
    // File exists at a valid prefix path — `is_safe_transcript_path`
    // canonicalize succeeds — but the bytes don't form valid UTF-8.
    // `File::open` succeeds, then `read_to_string` fails with
    // InvalidData and `read_capped` returns None. The walker fails
    // open via the `None => return false` branch.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let proj = home.join(".claude").join("projects").join("p");
    fs::create_dir_all(&proj).unwrap();
    let path = proj.join("invalid.jsonl");
    // 0xC3 starts a 2-byte UTF-8 sequence; 0x28 is `(` (not a valid
    // continuation byte), so the pair is invalid UTF-8.
    fs::write(&path, [0xC3u8, 0x28u8]).unwrap();
    assert!(!any_skill_in_set_since_user(
        &path,
        home,
        &["flow:flow-start", "flow:flow-prime"],
    ));
}

#[test]
fn any_skill_skips_empty_lines() {
    // Transcript has blank lines interleaved with valid turns.
    // The walker's `trimmed.is_empty()` branch skips the blank
    // lines and finds the sanctioned skill in the assistant turn.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"start it\"}}\n\
\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-start\"}}]}}\n\
\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(any_skill_in_set_since_user(
        &path,
        home,
        &["flow:flow-start", "flow:flow-prime"],
    ));
}

#[test]
fn any_skill_skips_malformed_json_lines() {
    // A line that fails `serde_json::from_str` is silently skipped
    // by the `Err(_) => continue` branch. File order places the
    // malformed line AFTER the matching assistant skill so the
    // backward walker hits the malformed line FIRST and exercises
    // the continue branch before finding the sanctioned skill.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"start it\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-prime\"}}]}}\n\
this is not json at all\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(any_skill_in_set_since_user(
        &path,
        home,
        &["flow:flow-start", "flow:flow-prime"],
    ));
}

#[test]
fn any_skill_returns_false_at_user_turn_boundary() {
    // Sanctioned-parent Skill appears BEFORE the most recent real
    // user turn. The walker stops at the user turn going backward,
    // so the older Skill is invisible. Returns false.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-start\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"now do something else\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-commit\"}}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!any_skill_in_set_since_user(
        &path,
        home,
        &["flow:flow-start", "flow:flow-prime"],
    ));
}

#[test]
fn any_skill_skips_synthetic_user_turn() {
    // Synthetic user turn (content is an array of blocks — a
    // tool_result wrapper carrying assistant output back to the
    // model) does NOT count as a real boundary. The walker
    // continues past it backward and finds the sanctioned skill in
    // an older assistant turn within the same window.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    // File order: real user, sanctioned-parent Skill,
    // synthetic-user tool_result, finalize Skill. The walker
    // (backward) sees finalize, then synthetic-user (array content
    // → skip), then sanctioned-parent (match), then real user
    // (boundary). Returns true because the sanctioned parent was
    // found before the real user boundary.
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"start it\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-start\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"x\",\"content\":\"ok\"}]}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-commit\"}}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(any_skill_in_set_since_user(
        &path,
        home,
        &["flow:flow-start", "flow:flow-prime"],
    ));
}

#[test]
fn any_skill_skips_unknown_turn_type() {
    // A turn with `type: "system"` does not match `assistant` or
    // `user`. The walker's `turn_type != "assistant"` branch falls
    // through and continues past it. File order places the system
    // turn AFTER the matching assistant skill so the backward walker
    // hits the system turn FIRST and exercises the continue branch.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"start it\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-prime\"}}]}}\n\
{\"type\":\"system\",\"message\":{\"role\":\"system\",\"content\":\"reminder\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(any_skill_in_set_since_user(
        &path,
        home,
        &["flow:flow-start", "flow:flow-prime"],
    ));
}

#[test]
fn any_skill_skips_assistant_turn_with_no_skills() {
    // An assistant turn carrying only a Bash tool_use produces an
    // empty `extract_skill_invocations` result. The walker keeps
    // walking backward and either finds a sanctioned Skill in an
    // older assistant turn or hits the user boundary. Here, a
    // sanctioned Skill appears in an older assistant turn — true.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"prime first\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-prime\"}}]}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"input\":{\"command\":\"ls\"}}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(any_skill_in_set_since_user(
        &path,
        home,
        &["flow:flow-start", "flow:flow-prime"],
    ));
}

#[test]
fn any_skill_returns_true_when_flow_start_in_chain() {
    // Window contains a Skill(flow:flow-start) followed by an
    // unrelated assistant text-only turn. The walker, scanning
    // backward, sees the text-only turn (no Skills → continue),
    // then the flow:flow-start Skill (match → true).
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"start it\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-start\"}}]}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"started\"}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(any_skill_in_set_since_user(
        &path,
        home,
        &["flow:flow-start", "flow:flow-prime"],
    ));
}

#[test]
fn any_skill_returns_true_when_flow_prime_in_chain() {
    // Window contains a Skill(flow:flow-prime). The walker matches
    // the second sanctioned-parent entry in the set.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"prime\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-prime\"}}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(any_skill_in_set_since_user(
        &path,
        home,
        &["flow:flow-start", "flow:flow-prime"],
    ));
}

#[test]
fn any_skill_skips_assistant_turn_with_unrelated_skill() {
    // Window contains only Skill(flow:flow-commit) and
    // Skill(decompose:decompose) — neither is in the sanctioned
    // set. Walker returns false.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"commit\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"decompose:decompose\"}}]}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-commit\"}}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!any_skill_in_set_since_user(
        &path,
        home,
        &["flow:flow-start", "flow:flow-prime"],
    ));
}

#[test]
fn any_skill_returns_false_at_eof_without_user_turn() {
    // No user turn anywhere in the transcript and no sanctioned
    // parent. The walker exhausts the buffer and returns false via
    // the post-loop fall-through.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-commit\"}}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!any_skill_in_set_since_user(
        &path,
        home,
        &["flow:flow-start", "flow:flow-prime"],
    ));
}

#[test]
fn any_skill_returns_false_on_empty_sanctioned_set() {
    // Caller passes an empty slice. The membership check cannot
    // match anything → the walker returns false unconditionally
    // regardless of what Skills appear in the transcript.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"start it\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-start\"}}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!any_skill_in_set_since_user(&path, home, &[]));
}

#[test]
fn any_skill_normalizes_input_before_match() {
    // Transcript records the skill name as UPPERCASE
    // `FLOW:FLOW-START`. The sanctioned set is lowercase. The
    // walker normalizes via `normalize_gate_input` (NUL strip +
    // trim + ASCII lowercase) before the membership check, so the
    // mismatch in casing is bridged. Returns true.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"start it\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"FLOW:FLOW-START\"}}]}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(any_skill_in_set_since_user(
        &path,
        home,
        &["flow:flow-start", "flow:flow-prime"],
    ));
}

#[test]
fn any_skill_in_set_since_user_skips_hook_feedback_string_content_ismeta_true() {
    // Real user turn → assistant Skill tool_use for `flow:flow-start`
    // → hook-feedback string-content user turn with `isMeta:true`.
    // The walker iterates backward from the tail. With the
    // hook-feedback filter (per .claude/rules/transcript-shape.md
    // and the `is_real_user_turn` discriminator), the walker skips
    // the synthetic user turn at the tail, reaches the assistant
    // Skill call, and returns true (bootstrap carve-out fires).
    // Without the filter, the walker would stop at the hook-feedback
    // turn and return false (bootstrap carve-out fails to fire,
    // legitimate flow-start commits on the integration branch are
    // blocked once a Stop-hook refusal lands in the transcript).
    //
    // Regression guard: a future edit reintroduces the bare
    // `content.as_str().is_some()` check into
    // `any_skill_in_set_since_user`. The named consumer is the
    // Layer 9 bootstrap carve-out in
    // `validate_pretool::bootstrap_carveout_applies`.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"start it\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-start\"}}]}}\n\
{\"type\":\"user\",\"isMeta\":true,\"message\":{\"role\":\"user\",\"content\":\"Stop hook feedback:\\nStop Refused: Continue, you can do it.\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(any_skill_in_set_since_user(
        &path,
        home,
        &["flow:flow-start", "flow:flow-prime"],
    ));
}

#[test]
fn any_skill_recognizes_legacy_command_name_user_turn_for_flow_prime() {
    // The most recent real user turn is a user-typed slash command in
    // the legacy `<command-name>/flow:flow-prime</command-name>` shape.
    // `flow:flow-prime` is a user-only skill — Claude Code records the
    // user typing `/flow:flow-prime` as a user-role turn, never as an
    // assistant `Skill` tool_use — so the assistant-Skill scan never
    // sees it. The walker must recognize the user-typed slash command
    // at the boundary and return true.
    //
    // Regression guard: a future edit removes the user-turn
    // boundary-recognition branch from `any_skill_in_set_since_user`,
    // so `flow:flow-prime` no longer satisfies the Layer 9 bootstrap
    // carve-out (`validate_pretool::bootstrap_carveout_applies`) and
    // legitimate flow-prime commits on the integration branch are
    // blocked.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-prime</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(any_skill_in_set_since_user(
        &path,
        home,
        &["flow:flow-start", "flow:flow-prime", "flow-release"],
    ));
}

#[test]
fn any_skill_recognizes_two_line_command_message_user_turn_for_flow_release() {
    // The most recent real user turn is a user-typed slash command in
    // the Claude Code 2.1.140+ two-line
    // `<command-message>flow-release</command-message>\n<command-name>
    // /flow-release</command-name>` shape. `flow-release` is a
    // project-local user-only skill — it is never an assistant `Skill`
    // tool_use — so the assistant-Skill scan never sees it. The walker
    // must recognize the two-line user-typed slash command at the
    // boundary and return true.
    //
    // Regression guard: a future edit drops the two-line shape from
    // the user-turn boundary recognition, so `/flow-release` typed on
    // Claude Code 2.1.140+ no longer satisfies the Layer 9 bootstrap
    // carve-out and legitimate release commits on the integration
    // branch are blocked.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-message>flow-release</command-message>\\n<command-name>/flow-release</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(any_skill_in_set_since_user(
        &path,
        home,
        &["flow:flow-start", "flow:flow-prime", "flow-release"],
    ));
}

#[test]
fn any_skill_recognizes_legacy_command_name_user_turn_for_flow_release() {
    // The most recent real user turn is a user-typed slash command in
    // the legacy `<command-name>/flow-release</command-name>` shape.
    // The walker must recognize the legacy shape for `flow-release`
    // (bare name, no `flow:` namespace) at the boundary and return
    // true.
    //
    // Regression guard: a future edit drops the legacy shape from the
    // user-turn boundary recognition, so `/flow-release` typed on
    // pre-2.1.140 Claude Code no longer satisfies the Layer 9
    // bootstrap carve-out.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow-release</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(any_skill_in_set_since_user(
        &path,
        home,
        &["flow:flow-start", "flow:flow-prime", "flow-release"],
    ));
}

#[test]
fn any_skill_rejects_user_turn_naming_non_sanctioned_user_only_skill() {
    // The most recent real user turn is a user-typed slash command
    // for `/flow:flow-abort` — a user-only skill, but NOT a member of
    // the sanctioned bootstrap set. The user-turn boundary recognition
    // loops the caller's `sanctioned` slice; `flow:flow-abort` is
    // absent, so no entry matches and the walker returns false.
    //
    // Regression guard: a future edit makes the user-turn recognition
    // accept ANY user-only skill rather than only the sanctioned set,
    // over-firing the Layer 9 bootstrap carve-out for an abort.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!any_skill_in_set_since_user(
        &path,
        home,
        &["flow:flow-start", "flow:flow-prime", "flow-release"],
    ));
}

#[test]
fn any_skill_rejects_user_turn_with_mid_prose_command_marker() {
    // The most recent real user turn is prose that MENTIONS the
    // `<command-name>/flow-release</command-name>` marker mid-text
    // rather than starting with it. The user-turn boundary
    // recognition anchors via `starts_with` on the normalized
    // content, so a mid-prose mention does not satisfy the check —
    // the walker returns false.
    //
    // Regression guard: a future edit replaces the `starts_with`
    // anchor with a `contains` check, letting a user discussing the
    // marker in prose spuriously satisfy the Layer 9 bootstrap
    // carve-out.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"what does <command-name>/flow-release</command-name> do\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!any_skill_in_set_since_user(
        &path,
        home,
        &["flow:flow-start", "flow:flow-prime", "flow-release"],
    ));
}

#[test]
fn any_skill_skips_synthetic_ismeta_user_turn_before_boundary_recognition() {
    // The only turn is a synthetic hook-feedback user turn — string
    // content carrying a `<command-name>/flow-release</command-name>`
    // marker AND `isMeta:true`. The user-turn boundary recognition
    // runs only AFTER the `is_real_user_turn` guard, so the synthetic
    // turn is skipped and never reaches the recognition branch. The
    // walker exhausts the buffer and returns false.
    //
    // Regression guard: a future edit moves the user-turn boundary
    // recognition ahead of the `is_real_user_turn` guard, so a
    // hook-injected feedback turn that happens to echo a
    // `<command-name>` marker spuriously satisfies the Layer 9
    // bootstrap carve-out.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"isMeta\":true,\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow-release</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!any_skill_in_set_since_user(
        &path,
        home,
        &["flow:flow-start", "flow:flow-prime", "flow-release"],
    ));
}

#[test]
fn any_skill_skips_empty_normalized_sanctioned_entry() {
    // A `sanctioned` slice whose only entry normalizes to an empty
    // string. The per-entry `skill_norm.is_empty()` guard skips it
    // BEFORE `content_invokes_skill` runs — without the skip, an
    // empty skill name builds the degenerate shape
    // `<command-name>/</command-name>`, and a user turn whose content
    // is exactly that string would spuriously match. The walker skips
    // the empty entry, finds no match, and returns false.
    //
    // Regression guard: a future edit removes the
    // `skill_norm.is_empty()` skip, so an empty `sanctioned` entry
    // builds the degenerate `<command-name>/</command-name>` shape and
    // spuriously satisfies the Layer 9 bootstrap carve-out for a user
    // turn carrying that exact marker.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(!any_skill_in_set_since_user(&path, home, &[""]));
}

// --- is_real_user_turn discriminator ---
//
// Claude Code emits two shapes of synthetic user turns alongside
// user-typed prose: tool_result-wrapped turns (array content) and
// hook-injected feedback turns (string content with `isMeta:true`).
// Every walker that needs to find the most recent REAL user turn
// must skip both shapes — without the `isMeta:true` filter, a
// single Stop-hook refusal silently halts the walker on every
// subsequent Stop event mid-utility-skill.
//
// The tests below drive each walker through hook-feedback fixtures
// to lock in the synthetic-turn-skip contract per
// `.claude/rules/transcript-shape.md`. Each fixture places a
// hook-feedback user turn (string content, `isMeta:true`) AT THE
// TAIL of the JSONL so the walker hits it first while iterating
// backward — the configuration that reproduces the real
// autonomous-flow-halt failure.

#[test]
fn last_user_message_invokes_skill_skips_hook_feedback_string_content_ismeta_true() {
    // Hook-feedback string-content turn with `isMeta:true` follows
    // an assistant turn that follows the real user invocation of
    // `/flow:flow-abort`. The walker must skip the hook-feedback
    // turn (synthetic) and stop at the real user turn (string
    // content, no `isMeta`), then match the slash-command marker.
    //
    // Regression guard: a future edit deletes the `isMeta:true`
    // filter from `is_real_user_turn` (or inlines a string-only
    // check inside the walker), causing Layer 1's user-invocation
    // detection to fail open whenever a Stop-hook refusal precedes
    // the assistant Skill call. The named consumer is the user-
    // only-skill enforcement chain (`validate-skill` Layer 1).
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-abort\"}}]}}\n\
{\"type\":\"user\",\"isMeta\":true,\"message\":{\"role\":\"user\",\"content\":\"Stop hook feedback:\\nStop Refused: Continue, you can do it.\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home,
    ));
}

#[test]
fn last_user_message_invokes_skill_accepts_user_turn_without_ismeta_field() {
    // A real user turn (string content, no `isMeta` field at all)
    // must still register as a real user turn. Older Claude Code
    // transcripts that pre-date the `isMeta` field carry user
    // turns with no such key, and treating those as synthetic
    // would break every flow against an older transcript.
    //
    // Regression guard: a future edit changes the `isMeta`
    // default from "false when missing" to "treat missing as
    // truthy", which would silently mask every legitimate user
    // turn in older transcripts. The named consumer is
    // `is_real_user_turn`'s doc-comment contract that absent
    // `isMeta` means "real user turn."
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home,
    ));
}

#[test]
fn most_recent_skill_in_user_only_set_skips_hook_feedback_string_content_ismeta_true() {
    // Real user turn → assistant Skill tool_use for `flow:flow-abort`
    // → hook-feedback string-content user turn with `isMeta:true`.
    // The walker iterates backward from the tail. With the
    // hook-feedback filter, the walker skips the synthetic user
    // turn at the tail, reaches the assistant Skill call, and
    // returns true (carve-out fires). Without the filter, the
    // walker would stop at the hook-feedback turn and return
    // false (carve-out fails to fire, abort-during-autonomous-
    // flow deadlocks).
    //
    // Regression guard: a future edit reintroduces a bare
    // `turn_type == "user" → return false` check without
    // `is_real_user_turn` gating. The named consumer is the
    // shared-config + user-only-skill carve-out path in
    // `validate-ask-user`.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"abort the flow\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-abort\"}}]}}\n\
{\"type\":\"user\",\"isMeta\":true,\"message\":{\"role\":\"user\",\"content\":\"Stop hook feedback:\\nStop Refused: Continue, you can do it.\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(most_recent_skill_in_user_only_set(&path, home));
}

#[test]
fn most_recent_skill_since_user_skips_hook_feedback_string_content_ismeta_true() {
    // The canonical #1507 transcript shape: real user → assistant
    // Skill call (`decompose:decompose`) → synthetic tool_result-
    // wrapped user turn → assistant text-only synthesis → hook-
    // feedback string-content user turn with `isMeta:true`. The
    // walker must skip the hook-feedback turn (synthetic) and the
    // tool_result-wrapped user turn (synthetic) to find the real
    // user turn, returning `Some("decompose:decompose")` for the
    // most recent Skill since that real boundary. Without the
    // `isMeta` filter, the walker stops at the hook-feedback turn
    // and returns `None` — the predicate fails open and the Stop
    // hook lets the turn end mid-utility-skill.
    //
    // Regression guard: a future edit removes the `is_real_user_turn`
    // call from `most_recent_skill_since_user`. The named consumer
    // is `stop_continue::check_in_progress_utility_skill`, which
    // gates the multi-step utility-skill Stop refusal.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"sounds good. proceed\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"decompose:decompose\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_d1\",\"content\":\"decompose output\"}]}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"synthesis prose\"}]}}\n\
{\"type\":\"user\",\"isMeta\":true,\"message\":{\"role\":\"user\",\"content\":\"Stop hook feedback:\\nStop Refused: Continue, you can do it.\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_skill_since_user(&path, home),
        Some("decompose:decompose".to_string()),
    );
}

#[test]
fn recent_edit_blocked_on_shared_config_skips_hook_feedback_string_content_ismeta_true() {
    // Real user prose → assistant Edit tool_use on a shared-config
    // path → tool_result-wrapped user turn carrying the
    // shared-config BLOCKED message → hook-feedback string-content
    // user turn with `isMeta:true` (e.g., the Stop hook fires
    // because autonomous-mode discipline saw an `AskUserQuestion`
    // candidate but the carve-out hadn't decided yet). The walker
    // must skip the hook-feedback turn at the tail and reach the
    // tool_result-wrapped user turn (legitimately array-content) to
    // detect the shared-config block. Without the filter, the
    // walker stops at the hook-feedback turn, finds no
    // shared-config block, and the carve-out fails to fire,
    // deadlocking the system-initiated `AskUserQuestion`.
    //
    // Regression guard: a future edit removes the hook-feedback
    // skip from `recent_edit_blocked_on_shared_config`. The named
    // consumer is the shared-config carve-out in
    // `validate-ask-user::run_impl_main`.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"please update reqs\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Edit\",\"id\":\"toolu_01\",\"input\":{\"file_path\":\"/p/requirements.txt\",\"old_string\":\"a\",\"new_string\":\"b\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_01\",\"content\":\"BLOCKED: requirements.txt is a shared configuration file that affects every engineer in the repository.\",\"is_error\":true}]}}\n\
{\"type\":\"user\",\"isMeta\":true,\"message\":{\"role\":\"user\",\"content\":\"Stop hook feedback:\\nStop Refused: Continue, you can do it.\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(recent_edit_blocked_on_shared_config(&path, home));
}

// --- is_real_user_turn truthy-isMeta tolerance ---
//
// `isMeta` arrives over the JSONL wire as a JSON value whose type
// Claude Code does not contractually pin to `bool`. Older captures,
// hand-edited transcripts, and external tools can emit truthy
// `isMeta` values as `"true"`, `"1"`, or non-zero numbers. The
// `is_real_user_turn` discriminator and the inline twin in
// `recent_edit_blocked_on_shared_config` must accept every truthy
// form the module's `is_truthy` helper recognizes per
// `.claude/rules/rust-patterns.md` "Hook Input Boolean Field
// Tolerance" — a raw `as_bool()` would silently misclassify
// non-bool truthy values as synthetic=false and re-open the
// autonomous-flow-halt bypass.

#[test]
fn last_user_message_invokes_skill_treats_ismeta_string_true_as_synthetic() {
    // Regression guard for the truthy-isMeta gap. With raw
    // `as_bool()`, `isMeta:"true"` parses to `None`,
    // `is_real_user_turn` returns `true`, and the hook-feedback
    // turn is treated as a real user turn — blocking legitimate
    // Layer 1 user-only-skill invocations.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-abort\"}}]}}\n\
{\"type\":\"user\",\"isMeta\":\"true\",\"message\":{\"role\":\"user\",\"content\":\"Stop hook feedback:\\nStop Refused\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home,
    ));
}

#[test]
fn last_user_message_invokes_skill_treats_ismeta_number_one_as_synthetic() {
    // Same gap probed through the numeric truthy variant. A
    // `Value::Number(1)` returns `None` from raw `as_bool()`. The
    // `is_truthy` helper accepts non-zero numbers as truthy.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-abort\"}}]}}\n\
{\"type\":\"user\",\"isMeta\":1,\"message\":{\"role\":\"user\",\"content\":\"Stop hook feedback:\\nStop Refused\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home,
    ));
}

#[test]
fn most_recent_skill_in_user_only_set_treats_ismeta_string_true_as_synthetic() {
    // Layer 2 user-only-skill carve-out — without truthy-isMeta
    // tolerance, the walker halts at the synthetic hook-feedback
    // turn and the AskUserQuestion carve-out fails to fire,
    // re-creating the abort-during-autonomous-flow deadlock.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"abort the flow\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-abort\"}}]}}\n\
{\"type\":\"user\",\"isMeta\":\"true\",\"message\":{\"role\":\"user\",\"content\":\"Stop hook feedback:\\nStop Refused\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(most_recent_skill_in_user_only_set(&path, home));
}

#[test]
fn most_recent_skill_since_user_treats_ismeta_number_one_as_synthetic() {
    // Canonical #1507 failure mode under non-bool truthy isMeta:
    // the walker stops at the hook-feedback turn (raw as_bool()
    // returns None for `isMeta:1`), returns None, and
    // check_in_progress_utility_skill fails open.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"sounds good. proceed\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"decompose:decompose\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_d1\",\"content\":\"decompose output\"}]}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"synthesis prose\"}]}}\n\
{\"type\":\"user\",\"isMeta\":1,\"message\":{\"role\":\"user\",\"content\":\"Stop hook feedback:\\nStop Refused\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert_eq!(
        most_recent_skill_since_user(&path, home),
        Some("decompose:decompose".to_string()),
    );
}

#[test]
fn recent_edit_blocked_on_shared_config_treats_ismeta_string_one_as_synthetic() {
    // Shared-config carve-out — the inline isMeta check in
    // `recent_edit_blocked_on_shared_config` must also tolerate
    // non-bool truthy values. Without the `is_truthy` swap, the
    // walker examines the hook-feedback turn, finds no
    // shared-config block, and the AskUserQuestion deadlocks.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"please update reqs\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Edit\",\"id\":\"toolu_01\",\"input\":{\"file_path\":\"/p/requirements.txt\",\"old_string\":\"a\",\"new_string\":\"b\"}}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_01\",\"content\":\"BLOCKED: requirements.txt is a shared configuration file that affects every engineer in the repository.\",\"is_error\":true}]}}\n\
{\"type\":\"user\",\"isMeta\":\"1\",\"message\":{\"role\":\"user\",\"content\":\"Stop hook feedback:\\nStop Refused\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(recent_edit_blocked_on_shared_config(&path, home));
}

#[test]
fn last_user_message_invokes_skill_treats_ismeta_array_as_synthetic() {
    // Asymmetric fail-closed discriminator: any `isMeta` value other
    // than absent, `null`, or `Bool(false)` indicates a synthetic
    // turn. An array — e.g. `isMeta:[true]` — falls into the
    // catch-all "present non-bool-false" branch and must be treated
    // as synthetic. Without this, a hand-crafted JSONL carrying
    // `isMeta:[true]` AND a `<command-name>` slash-command marker
    // would pose as a real user turn and pass the user-only-skill
    // gate without the user typing the command.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-abort\"}}]}}\n\
{\"type\":\"user\",\"isMeta\":[true],\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    // Walker must skip the synthetic-by-intent turn (array isMeta)
    // and find the real user turn at the head of the file.
    assert!(last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home,
    ));
}

#[test]
fn last_user_message_invokes_skill_treats_ismeta_object_as_synthetic() {
    // Companion to the array case: an object-valued `isMeta` is
    // also a present non-bool-false marker. The discriminator must
    // treat any structured value the same as a truthy scalar.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-abort\"}}]}}\n\
{\"type\":\"user\",\"isMeta\":{\"injected\":true},\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home,
    ));
}

#[test]
fn last_user_message_invokes_skill_treats_ismeta_null_as_real() {
    // `null` is the explicit-absence marker alongside missing field
    // and `false`. A `isMeta:null` user turn with a slash-command
    // marker IS a real user invocation. Without this, walkers would
    // misclassify legitimate user-typed commands when an upstream
    // serializer emitted explicit nulls.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"isMeta\":null,\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home,
    ));
}

#[test]
fn last_user_message_invokes_skill_treats_ismeta_bool_false_as_real() {
    // Explicit `isMeta:false` is the third "real user turn" marker
    // (alongside missing field and `null`). Walkers must accept it
    // so a future producer emitting explicit booleans stays
    // compatible.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"isMeta\":false,\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home,
    ));
}

#[test]
fn last_user_message_invokes_skill_skips_user_turn_with_missing_content() {
    // Plan Branch D: a `type:"user"` turn whose `message.content`
    // field is missing entirely. `is_real_user_turn`'s
    // `and_then(|c| c.as_str())` short-circuits to `None`,
    // `content_is_string` is false, the helper returns false, and
    // the walker continues backward to find a real user turn.
    // Without this filter, a malformed user turn at the tail would
    // mask a legitimate slash-command invocation earlier in the
    // transcript.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"ok\"}]}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\"}}\n";
    let path = crate::common::transcript_fixture(home, "p", jsonl);
    assert!(last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home,
    ));
}

// --- read_full / read_recency_window / read_recent_turns ---

#[test]
fn read_full_returns_full_file_content() {
    // Build a file > 50 MB with a unique marker line at byte 0.
    // `read_full` is uncapped, so the returned String must contain
    // the marker — proving the read covered the entire file, not
    // just the tail. The fixture allocates ~51 MB transiently
    // (one composed String + one fs::write), which is acceptable on
    // any modern dev machine.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oversized.txt");
    let marker = "UNIQUE-MARKER-AT-HEAD\n";
    let padding_size = (TRANSCRIPT_BYTE_CAP as usize) + 1024;
    let mut content = String::with_capacity(marker.len() + padding_size);
    content.push_str(marker);
    content.extend(std::iter::repeat_n('x', padding_size));
    fs::write(&path, content.as_bytes()).unwrap();
    let result = read_full(&path).expect("read_full returns Some on existing readable file");
    assert!(
        result.contains(marker),
        "read_full must cover head bytes, not just tail",
    );
}

#[test]
fn read_full_returns_none_on_missing_file() {
    // No file at the path. `std::fs::read_to_string` fails with
    // ENOENT, `.ok()` converts to None. Matches the fail-open
    // semantics of the underlying `read_capped` error path.
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("does-not-exist.txt");
    assert_eq!(read_full(&missing), None);
}

#[test]
fn read_recency_window_caps_at_50mb() {
    // File > 50 MB. `read_recency_window` calls
    // `read_capped(path, TRANSCRIPT_BYTE_CAP)` which seeks to the
    // file's tail and reads at most `TRANSCRIPT_BYTE_CAP` bytes.
    // The returned String length must be <= TRANSCRIPT_BYTE_CAP.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recency.txt");
    let total_size = (TRANSCRIPT_BYTE_CAP as usize) + 1024;
    let content: Vec<u8> = std::iter::repeat_n(b'x', total_size).collect();
    fs::write(&path, &content).unwrap();
    let result =
        read_recency_window(&path).expect("read_recency_window returns Some on readable file");
    assert!(
        result.len() as u64 <= TRANSCRIPT_BYTE_CAP,
        "read_recency_window must cap at TRANSCRIPT_BYTE_CAP; got {} bytes",
        result.len(),
    );
}

#[test]
fn read_recency_window_returns_none_on_missing_file() {
    // Missing file → File::open fails → `read_capped` returns None.
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nope.txt");
    assert_eq!(read_recency_window(&missing), None);
}

#[test]
fn read_recent_turns_caps_at_4mb() {
    // File > 4 MB. `read_recent_turns` calls
    // `read_capped(path, SHARED_CONFIG_BLOCK_BYTE_CAP)` which seeks
    // to the file's tail and reads at most
    // `SHARED_CONFIG_BLOCK_BYTE_CAP` bytes. The returned String
    // length must be <= SHARED_CONFIG_BLOCK_BYTE_CAP.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recent.txt");
    let total_size = (SHARED_CONFIG_BLOCK_BYTE_CAP as usize) + 1024;
    let content: Vec<u8> = std::iter::repeat_n(b'x', total_size).collect();
    fs::write(&path, &content).unwrap();
    let result = read_recent_turns(&path).expect("read_recent_turns returns Some on readable file");
    assert!(
        result.len() as u64 <= SHARED_CONFIG_BLOCK_BYTE_CAP,
        "read_recent_turns must cap at SHARED_CONFIG_BLOCK_BYTE_CAP; got {} bytes",
        result.len(),
    );
}

#[test]
fn read_recent_turns_returns_none_on_missing_file() {
    // Missing file → File::open fails → `read_capped` returns None.
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nope.txt");
    assert_eq!(read_recent_turns(&missing), None);
}

// --- read_capped invariant (contract test) ---

/// Returns the slice of `src` starting at `marker` and ending at the
/// next `\n}\n` (function-body terminator). Panics if `marker` is
/// missing. Used by `read_capped_only_called_inside_named_helpers`
/// to bound the assertion scope to each helper's body per
/// `.claude/rules/testing-gotchas.md` "Subsection-Local Assertions
/// in Contract Tests".
fn extract_fn_body<'a>(src: &'a str, marker: &str) -> &'a str {
    let tail = src
        .split_once(marker)
        .map(|(_, t)| t)
        .unwrap_or_else(|| panic!("marker `{}` not found in source", marker));
    tail.split_once("\n}\n")
        .map(|(body, _)| body)
        .unwrap_or(tail)
}

#[test]
fn read_capped_only_called_inside_named_helpers() {
    // Locks the API in: `read_capped` is the private primitive; the
    // three named wrappers (`read_full`, `read_recency_window`,
    // `read_recent_turns`) are the only production callers. A future
    // edit that adds a direct `read_capped(` call outside those
    // wrappers — or that fails to migrate a new caller to one of the
    // wrappers — fails this contract test.
    //
    // Implementation per `.claude/rules/testing-gotchas.md`
    // "Subsection-Local Assertions in Contract Tests": bound each
    // helper's body via `split_once("pub fn <name>(")` and the
    // following `\n}\n` to keep counts scoped to that function's
    // body. Bounding by `\n}\n` works because every helper's body
    // ends with a single closing brace on its own line — Rust
    // rustfmt formatting guarantees the shape.
    let src =
        std::fs::read_to_string(crate::common::repo_root().join("src/hooks/transcript_walker.rs"))
            .expect("read src/hooks/transcript_walker.rs");

    // Use a runtime-composed needle so the contract test file itself
    // never contains the literal `read_capped(` byte sequence outside
    // its own assertion strings — keeps the test self-consistent if
    // anyone later extends the scanner to scan `tests/`.
    let needle: String = ["read", "_capped("].concat();
    let needle_str = needle.as_str();

    let read_full_body = extract_fn_body(&src, "pub fn read_full(");
    let read_recency_body = extract_fn_body(&src, "pub fn read_recency_window(");
    let read_recent_body = extract_fn_body(&src, "pub fn read_recent_turns(");

    assert_eq!(
        read_full_body.matches(needle_str).count(),
        0,
        "read_full body must not call read_capped (it uses fs::read_to_string)"
    );
    assert_eq!(
        read_recency_body.matches(needle_str).count(),
        1,
        "read_recency_window body must call read_capped exactly once"
    );
    assert_eq!(
        read_recent_body.matches(needle_str).count(),
        1,
        "read_recent_turns body must call read_capped exactly once"
    );

    // Total occurrences in the file:
    // - 1 inside read_recency_window body (counted above)
    // - 1 inside read_recent_turns body (counted above)
    // - 1 in the `fn read_capped(` definition itself
    // Anything else means a direct caller exists outside the three
    // named wrappers — the contract is broken.
    let total = src.matches(needle_str).count();
    assert_eq!(
        total, 3,
        "read_capped( appears {} times in transcript_walker.rs; \
         expected exactly 3 (1 in each capped helper + 1 in the fn definition)",
        total
    );

    // Every other `src/*.rs` file must contain zero `read_capped(`
    // calls. The literal is module-private (Rust visibility would
    // already reject it at compile time), but the contract test
    // makes the invariant explicit so a future change that exposes
    // `read_capped` as `pub` and adds an outside caller trips here.
    let src_dir = crate::common::repo_root().join("src");
    let mut rust_files: Vec<std::path::PathBuf> = Vec::new();
    collect_rust_files(&src_dir, &mut rust_files);
    let walker_path = crate::common::repo_root().join("src/hooks/transcript_walker.rs");
    let walker_canon = walker_path
        .canonicalize()
        .expect("canonicalize walker path");
    for path in &rust_files {
        let path_canon = match path.canonicalize() {
            Ok(p) => p,
            Err(_) => continue,
        };
        if path_canon == walker_canon {
            continue;
        }
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        assert!(
            !content.contains(needle_str),
            "{} contains `{}` — only the three named helpers in \
             src/hooks/transcript_walker.rs may call read_capped",
            path.display(),
            needle_str
        );
    }
}

/// Walk `src/` and collect every `.rs` file path. Mirrors the helper
/// in `tests/test_placement.rs` — duplicated here because Cargo's
/// directory-form test layout for `tests/hooks/` does not allow
/// importing helpers from sibling top-level test files.
fn collect_rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name.starts_with('.') || name == "target" {
            continue;
        }
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            collect_rust_files(&path, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}
