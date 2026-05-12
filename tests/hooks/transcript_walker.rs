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
    last_user_message_invokes_skill, most_recent_skill_in_user_only_set,
    most_recent_skill_since_user, recent_edit_blocked_on_shared_config,
    verify_agent_returned_in_phase, SHARED_CONFIG_BLOCK_BYTE_CAP, TRANSCRIPT_BYTE_CAP,
    USER_ONLY_SKILLS,
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
fn user_only_skills_constant_lists_four_skills() {
    let names: Vec<&str> = USER_ONLY_SKILLS.to_vec();
    assert!(names.contains(&"flow:flow-abort"));
    assert!(names.contains(&"flow:flow-reset"));
    assert!(names.contains(&"flow:flow-release"));
    assert!(names.contains(&"flow:flow-prime"));
    assert_eq!(names.len(), 4);
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
fn verify_agent_returned_handles_oversized_transcript_via_tail_cap() {
    use std::fs;
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let proj = home.join(".claude").join("projects").join("p");
    fs::create_dir_all(&proj).unwrap();
    let path = proj.join("oversized.jsonl");
    let head_marker = b"{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"id\":\"toolu_b1\",\"input\":{\"command\":\"bin/flow phase-enter --phase flow-review\"}}]}}\n";
    let mut content: Vec<u8> = head_marker.to_vec();
    let padding_size = (TRANSCRIPT_BYTE_CAP as usize) + 1024;
    content.extend(std::iter::repeat_n(b'\n', padding_size));
    fs::write(&path, &content).unwrap();
    // The marker is at the file's head; the tail-cap means it falls
    // off the visible window and the verifier reports
    // phase_marker_not_found.
    assert_eq!(
        verify_agent_returned_in_phase(&path, home, "reviewer", "flow-review"),
        Err("phase_marker_not_found".to_string())
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
    // The string-content assistant turn must appear AFTER the marker
    // so the BACKWARD scan walks through it. Backward iteration:
    // result (user → false at type check) → agent_block (assistant
    // array, no Bash → loop returns false) → string_content_assistant
    // (assistant, string content → hits content.as_array None branch
    // returning false) → marker (assistant array with Bash → matches).
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
