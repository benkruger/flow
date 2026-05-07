//! Integration tests for `src/hooks/transcript_walker.rs`.
//!
//! Drives `last_user_message_invokes_skill` and
//! `most_recent_skill_in_user_only_set` through controlled JSONL
//! fixtures via `transcript_fixture` (in tests/common/mod.rs). Each
//! line in the fixture is a Claude Code transcript turn whose
//! top-level `type` field carries the `user`/`assistant` role
//! (matching `src/window_snapshot.rs::read_transcript`).
//!
//! Lives at the top-level `tests/` path rather than the mirrored
//! `tests/hooks/transcript_walker.rs` because the `[[test]]` stanza
//! addition required for subdirectory tests was blocked by the
//! validate-worktree-paths shared-config hook in autonomous mode and
//! `AskUserQuestion` was blocked by validate-ask-user. Top-level
//! placement is functionally equivalent — Cargo auto-discovers
//! `tests/*.rs`. See the deviation log entry on this branch.

mod common;

use std::fs;

use flow_rs::hooks::transcript_walker::{
    last_user_message_invokes_skill, most_recent_skill_in_user_only_set, TRANSCRIPT_BYTE_CAP,
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
    let path = common::transcript_fixture(home, "p", "not json\nstill not json\n");
    assert!(!last_user_message_invokes_skill(
        &path,
        "flow:flow-abort",
        home
    ));
    assert!(!most_recent_skill_in_user_only_set(&path, home));
}

#[test]
fn walker_returns_false_when_oversized() {
    // Fixture sized just past TRANSCRIPT_BYTE_CAP with the matching
    // `<command-name>` tag positioned beyond the cap. The reader
    // truncates at the cap so the matching content is unreadable and
    // the predicate returns false.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let proj = home.join(".claude").join("projects").join("p");
    fs::create_dir_all(&proj).unwrap();
    let path = proj.join("oversized.jsonl");
    let padding_size = (TRANSCRIPT_BYTE_CAP as usize) + 1024;
    let mut content = vec![b'\n'; padding_size];
    let trailing = "\n{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/flow:flow-abort</command-name>\"}}\n";
    content.extend_from_slice(trailing.as_bytes());
    fs::write(&path, &content).unwrap();
    assert!(!last_user_message_invokes_skill(
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
    let path = common::transcript_fixture(home, "p", jsonl);
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
    let path = common::transcript_fixture(home, "p", jsonl);
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
    let path = common::transcript_fixture(home, "p", jsonl);
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
    let path = common::transcript_fixture(home, "p", jsonl);
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
    let path = common::transcript_fixture(home, "p", jsonl);
    assert!(most_recent_skill_in_user_only_set(&path, home));
}

#[test]
fn most_recent_skill_in_user_only_set_returns_false_when_skill_not_user_only() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"check status\"}}\n\
{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{\"skill\":\"flow:flow-status\"}}]}}\n";
    let path = common::transcript_fixture(home, "p", jsonl);
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
    let path = common::transcript_fixture(home, "p", jsonl);
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
    let path = common::transcript_fixture(home, "p", jsonl);
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
    let path = common::transcript_fixture(home, "p", jsonl);
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
    let path = common::transcript_fixture(home, "p", jsonl);
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
    let path = common::transcript_fixture(home, "p", jsonl);
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
    let path = common::transcript_fixture(home, "p", jsonl);
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
    let path = common::transcript_fixture(home, "p", jsonl);
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
    let path = common::transcript_fixture(home, "p", jsonl);
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
    let path = common::transcript_fixture(home, "p", jsonl);
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
    let path = common::transcript_fixture(home, "p", jsonl);
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
    let path = common::transcript_fixture(home, "p", jsonl);
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
    let path = common::transcript_fixture(home, "p", jsonl);
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
