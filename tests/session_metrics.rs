//! Integration tests for `src/session_metrics.rs` — every branch
//! is exercised through fixture-controlled inputs (tempdir `home`,
//! fake transcript JSONL). Per
//! `.claude/rules/testing-gotchas.md` "macOS Subprocess Path
//! Canonicalization", every fixture path is canonicalized at
//! construction so prefix comparisons hold across `/var` ↔
//! `/private/var` symlinks.
//!
//! Cost-file behaviour is exercised by `tests/session_cost.rs`;
//! `capture` here never reads cost files by design.

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use flow_rs::session_metrics::{
    append_step_snapshot, capture, home_dir_or_empty, is_safe_transcript_path,
    is_safe_transcript_path_structural, write_snapshot_into_state,
};
use serde_json::{json, Value};

/// Build a `home/.claude/rate-limits.json` file inside `dir` with
/// the supplied pcts.
fn write_rate_limits(dir: &std::path::Path, five: i64, seven: i64) {
    let claude_dir = dir.join(".claude");
    fs::create_dir_all(&claude_dir).expect("mkdir .claude");
    let body = format!(r#"{{"five_hour_pct":{},"seven_day_pct":{}}}"#, five, seven);
    fs::write(claude_dir.join("rate-limits.json"), body).expect("write rate-limits");
}

/// Write a transcript JSONL file with the supplied lines.
fn write_transcript(dir: &std::path::Path, name: &str, lines: &[&str]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, lines.join("\n") + "\n").expect("write transcript");
    path
}

/// Helper for an assistant-message JSON line.
fn assistant_line(
    model: &str,
    input: i64,
    output: i64,
    cache_create: i64,
    cache_read: i64,
) -> String {
    format!(
        r#"{{"type":"assistant","message":{{"model":"{model}","role":"assistant","content":[{{"type":"text","text":"hi"}}],"usage":{{"input_tokens":{input},"output_tokens":{output},"cache_creation_input_tokens":{cache_create},"cache_read_input_tokens":{cache_read}}}}}}}"#
    )
}

/// Helper for an assistant-message JSON line that includes a
/// configurable number of tool_use content blocks.
fn assistant_line_with_tools(model: &str, tool_count: usize) -> String {
    let mut content = String::from(r#"[{"type":"text","text":"hi"}"#);
    for i in 0..tool_count {
        content.push_str(&format!(
            r#",{{"type":"tool_use","id":"toolu_{i}","name":"Bash","input":{{}}}}"#
        ));
    }
    content.push(']');
    format!(
        r#"{{"type":"assistant","message":{{"model":"{model}","role":"assistant","content":{content},"usage":{{"input_tokens":1,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#
    )
}

// --- capture ---

/// All inputs present and valid → every numeric field populated.
#[test]
fn capture_with_all_inputs_populates_full_snapshot() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    write_rate_limits(&root, 42, 7);
    let transcript = write_transcript(
        &root,
        "session.jsonl",
        &[&assistant_line("claude-opus-4-7", 100, 50, 10, 20)],
    );

    let snap = capture(&root, Some(&transcript), Some("sid-123"), || {
        "2026-05-04T10:00:00-07:00".to_string()
    });

    assert_eq!(snap.captured_at, "2026-05-04T10:00:00-07:00");
    assert_eq!(snap.session_id.as_deref(), Some("sid-123"));
    assert_eq!(snap.model.as_deref(), Some("claude-opus-4-7"));
    assert_eq!(snap.five_hour_pct, Some(42));
    assert_eq!(snap.seven_day_pct, Some(7));
    assert_eq!(snap.session_input_tokens, Some(100));
    assert_eq!(snap.session_output_tokens, Some(50));
    assert_eq!(snap.session_cache_creation_tokens, Some(10));
    assert_eq!(snap.session_cache_read_tokens, Some(20));
    assert_eq!(snap.turn_count, Some(1));
    assert_eq!(snap.tool_call_count, Some(0));
    // Context = input + cache_create + cache_read = 100 + 10 + 20 = 130
    assert_eq!(snap.context_at_last_turn_tokens, Some(130));
    assert!(snap.context_window_pct.unwrap() > 0.0);
    assert!(snap.context_window_pct.unwrap() < 1.0);
    assert_eq!(snap.by_model.len(), 1);
}

/// No rate-limits file → both pct fields are `None` while the rest
/// of the snapshot still populates.
#[test]
fn capture_with_missing_rate_limits_sets_pcts_none() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let transcript = write_transcript(
        &root,
        "session.jsonl",
        &[&assistant_line("claude-opus-4-7", 100, 50, 0, 0)],
    );

    let snap = capture(&root, Some(&transcript), Some("sid"), || "now".to_string());

    assert_eq!(snap.five_hour_pct, None);
    assert_eq!(snap.seven_day_pct, None);
    assert_eq!(snap.session_input_tokens, Some(100));
    assert_eq!(snap.turn_count, Some(1));
}

/// No transcript path → token / turn / tool / by_model fields are
/// `None` / empty while rate-limits still flow through.
#[test]
fn capture_with_missing_transcript_sets_token_fields_none() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    write_rate_limits(&root, 42, 7);

    let snap = capture(&root, None, Some("sid"), || "now".to_string());

    assert_eq!(snap.session_input_tokens, None);
    assert_eq!(snap.session_output_tokens, None);
    assert_eq!(snap.session_cache_creation_tokens, None);
    assert_eq!(snap.session_cache_read_tokens, None);
    assert_eq!(snap.turn_count, None);
    assert_eq!(snap.tool_call_count, None);
    assert!(snap.by_model.is_empty());
    assert_eq!(snap.five_hour_pct, Some(42));
}

/// `session_id` argument is `None` → snapshot's `session_id` is
/// `None`. Other fields still populate.
#[test]
fn capture_with_missing_session_id_sets_session_id_none() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    write_rate_limits(&root, 42, 7);

    let snap = capture(&root, None, None, || "now".to_string());

    assert_eq!(snap.session_id, None);
    assert_eq!(snap.five_hour_pct, Some(42));
}

/// Multi-model transcript → `by_model` carries one entry per model
/// with summed counters.
#[test]
fn capture_with_multi_model_transcript_splits_by_model() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let transcript = write_transcript(
        &root,
        "session.jsonl",
        &[
            &assistant_line("claude-opus-4-7", 100, 50, 0, 0),
            &assistant_line("claude-sonnet-4-6", 10, 5, 0, 0),
            &assistant_line("claude-opus-4-7", 200, 100, 0, 0),
        ],
    );

    let snap = capture(&root, Some(&transcript), Some("sid"), || "now".to_string());

    assert_eq!(snap.by_model.len(), 2);
    let opus = snap.by_model.get("claude-opus-4-7").expect("opus entry");
    assert_eq!(opus.input, 300);
    assert_eq!(opus.output, 150);
    let sonnet = snap
        .by_model
        .get("claude-sonnet-4-6")
        .expect("sonnet entry");
    assert_eq!(sonnet.input, 10);
    assert_eq!(sonnet.output, 5);
    assert_eq!(snap.session_input_tokens, Some(310));
    assert_eq!(snap.session_output_tokens, Some(155));
    assert_eq!(snap.turn_count, Some(3));
}

/// Malformed JSONL lines are skipped silently; valid lines still
/// contribute. Guards against partial-write tail rows.
#[test]
fn capture_with_malformed_jsonl_skips_bad_lines_and_continues() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let transcript = write_transcript(
        &root,
        "session.jsonl",
        &[
            "not-json",
            "{invalid json",
            "",
            &assistant_line("claude-opus-4-7", 7, 3, 0, 0),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\"}}",
            &assistant_line("claude-opus-4-7", 5, 2, 0, 0),
        ],
    );

    let snap = capture(&root, Some(&transcript), Some("sid"), || "now".to_string());

    assert_eq!(snap.session_input_tokens, Some(12));
    assert_eq!(snap.turn_count, Some(2));
}

/// Transcript with no assistant messages → every counter is `None`.
#[test]
fn capture_with_no_assistant_messages_returns_zero_counters() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let transcript = write_transcript(
        &root,
        "session.jsonl",
        &[
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}",
            "{\"type\":\"system\",\"summary\":\"x\"}",
        ],
    );

    let snap = capture(&root, Some(&transcript), Some("sid"), || "now".to_string());

    assert_eq!(snap.session_input_tokens, None);
    assert_eq!(snap.session_output_tokens, None);
    assert_eq!(snap.turn_count, None);
    assert_eq!(snap.tool_call_count, None);
    assert!(snap.by_model.is_empty());
}

/// `context_at_last_turn_tokens` reflects the MOST RECENT assistant
/// message — not a sum across all of them.
#[test]
fn capture_records_last_turn_context_from_most_recent_assistant_message() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let transcript = write_transcript(
        &root,
        "session.jsonl",
        &[
            &assistant_line("claude-opus-4-7", 100, 50, 0, 0),
            &assistant_line("claude-opus-4-7", 1000, 500, 100, 200),
        ],
    );

    let snap = capture(&root, Some(&transcript), Some("sid"), || "now".to_string());

    // 1000 + 100 + 200 = 1300
    assert_eq!(snap.context_at_last_turn_tokens, Some(1300));
    assert_eq!(snap.session_input_tokens, Some(1100));
    assert_eq!(snap.session_output_tokens, Some(550));
}

/// `tool_call_count` aggregates `tool_use` content blocks across
/// every assistant message in the transcript.
#[test]
fn capture_counts_tool_use_blocks_across_assistant_messages() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let transcript = write_transcript(
        &root,
        "session.jsonl",
        &[
            &assistant_line_with_tools("claude-opus-4-7", 2),
            &assistant_line_with_tools("claude-opus-4-7", 3),
            &assistant_line_with_tools("claude-opus-4-7", 0),
        ],
    );

    let snap = capture(&root, Some(&transcript), Some("sid"), || "now".to_string());

    assert_eq!(snap.tool_call_count, Some(5));
    assert_eq!(snap.turn_count, Some(3));
}

/// Rate-limits file present but malformed JSON → both pcts `None`.
#[test]
fn capture_with_malformed_rate_limits_sets_pcts_none() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let claude_dir = root.join(".claude");
    fs::create_dir_all(&claude_dir).expect("mkdir");
    fs::write(claude_dir.join("rate-limits.json"), "{not json").expect("write");
    let snap = capture(&root, None, None, || "now".to_string());
    assert_eq!(snap.five_hour_pct, None);
    assert_eq!(snap.seven_day_pct, None);
}

/// Rate-limits JSON missing the expected keys → pcts default to None.
#[test]
fn capture_with_rate_limits_missing_keys_sets_pcts_none() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let claude_dir = root.join(".claude");
    fs::create_dir_all(&claude_dir).expect("mkdir");
    fs::write(claude_dir.join("rate-limits.json"), "{}").expect("write");
    let snap = capture(&root, None, None, || "now".to_string());
    assert_eq!(snap.five_hour_pct, None);
    assert_eq!(snap.seven_day_pct, None);
}

/// Transcript path present but file does not exist → empty
/// aggregate, no panic.
#[test]
fn capture_with_nonexistent_transcript_path_is_empty() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let path = root.join("missing.jsonl");
    let snap = capture(&root, Some(&path), None, || "now".to_string());
    assert_eq!(snap.turn_count, None);
}

/// Assistant message missing `usage` → counters contribute zero.
#[test]
fn capture_with_assistant_missing_usage_contributes_zero_tokens() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let line = r#"{"type":"assistant","message":{"model":"claude-opus-4-7","role":"assistant","content":[]}}"#;
    let transcript = write_transcript(&root, "session.jsonl", &[line]);
    let snap = capture(&root, Some(&transcript), Some("sid"), || "now".to_string());
    assert_eq!(snap.session_input_tokens, Some(0));
    assert_eq!(snap.turn_count, Some(1));
    assert_eq!(snap.context_at_last_turn_tokens, Some(0));
}

/// Assistant line missing `message` field is skipped.
#[test]
fn capture_with_assistant_missing_message_is_skipped() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let line = r#"{"type":"assistant"}"#;
    let transcript = write_transcript(&root, "session.jsonl", &[line]);
    let snap = capture(&root, Some(&transcript), Some("sid"), || "now".to_string());
    assert_eq!(snap.turn_count, None);
}

/// Assistant message missing `model` → by_model empty.
#[test]
fn capture_with_assistant_missing_model_skips_by_model() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let line = r#"{"type":"assistant","message":{"role":"assistant","content":[],"usage":{"input_tokens":10,"output_tokens":5,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#;
    let transcript = write_transcript(&root, "session.jsonl", &[line]);
    let snap = capture(&root, Some(&transcript), Some("sid"), || "now".to_string());
    assert_eq!(snap.session_input_tokens, Some(10));
    assert!(snap.by_model.is_empty());
    assert_eq!(snap.context_window_pct, None);
}

/// 1M context model variant uses the larger denominator.
#[test]
fn capture_with_1m_context_model_uses_million_token_window() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let line = assistant_line("claude-opus-4-7[1m]", 100_000, 0, 0, 0);
    let transcript = write_transcript(&root, "session.jsonl", &[&line]);
    let snap = capture(&root, Some(&transcript), Some("sid"), || "now".to_string());
    let pct = snap
        .context_window_pct
        .expect("pct populated for known model");
    assert!((pct - 10.0).abs() < 1e-6, "expected ~10.0, got {}", pct);
}

/// Assistant message with `content` as a non-array (string) →
/// the `as_array()` early-return path is taken so no tool blocks
/// count, but the message still contributes its usage.
#[test]
fn capture_with_assistant_content_not_array_skips_tool_count() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let line = r#"{"type":"assistant","message":{"model":"claude-opus-4-7","content":"plain string","usage":{"input_tokens":3,"output_tokens":2,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#;
    let transcript = write_transcript(&root, "session.jsonl", &[line]);
    let snap = capture(&root, Some(&transcript), Some("sid"), || "now".to_string());
    assert_eq!(snap.session_input_tokens, Some(3));
    assert_eq!(snap.tool_call_count, Some(0));
}

/// Transcript with non-UTF-8 bytes on a line → `BufRead::lines()`
/// yields `Err` for that line; capture skips it silently.
#[test]
fn capture_with_non_utf8_line_skips_silently() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let path = root.join("session.jsonl");
    let mut bytes = vec![0xFF, b'\n'];
    bytes.extend(assistant_line("claude-opus-4-7", 5, 3, 0, 0).bytes());
    bytes.push(b'\n');
    fs::write(&path, &bytes).expect("write");
    let snap = capture(&root, Some(&path), Some("sid"), || "now".to_string());
    assert_eq!(snap.session_input_tokens, Some(5));
    assert_eq!(snap.turn_count, Some(1));
}

/// Non-Claude model name → `context_window_pct` is `None`.
#[test]
fn capture_with_unknown_model_returns_none_context_window_pct() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let line = assistant_line("custom-model-xyz", 100, 0, 0, 0);
    let transcript = write_transcript(&root, "session.jsonl", &[&line]);
    let snap = capture(&root, Some(&transcript), Some("sid"), || "now".to_string());
    assert_eq!(snap.context_window_pct, None);
    assert_eq!(snap.context_at_last_turn_tokens, Some(100));
}

/// Empty `home` makes `read_rate_limits` short-circuit so a
/// committed `.claude/rate-limits.json` in a worktree cannot be
/// read as if it were the user's rate-limit data.
#[test]
fn capture_with_empty_home_skips_rate_limits_read() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    write_rate_limits(&root, 99, 50);
    let snap = capture(std::path::Path::new(""), None, None, || "now".to_string());
    assert_eq!(snap.five_hour_pct, None);
    assert_eq!(snap.seven_day_pct, None);
}

/// Relative `home` (non-absolute) is also rejected — same threat
/// as empty home.
#[test]
fn capture_with_relative_home_skips_rate_limits_read() {
    let snap = capture(std::path::Path::new("relative/path"), None, None, || {
        "now".to_string()
    });
    assert_eq!(snap.five_hour_pct, None);
    assert_eq!(snap.seven_day_pct, None);
}

/// Transcript byte cap drops bytes past `TRANSCRIPT_BYTE_CAP`.
/// Fixture writes a transcript with many entries; reading returns
/// a populated agg without hanging or exhausting memory.
#[test]
fn capture_with_oversized_transcript_returns_bounded_snapshot() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let mut lines: Vec<String> = Vec::new();
    for _ in 0..2000 {
        lines.push(assistant_line("claude-opus-4-7", 1, 1, 0, 0));
    }
    let line_refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    let transcript = write_transcript(&root, "big.jsonl", &line_refs);
    let snap = capture(&root, Some(&transcript), Some("sid"), || "now".to_string());
    assert!(snap.session_input_tokens.unwrap() > 0);
    assert!(snap.turn_count.unwrap() > 0);
}

// --- append_step_snapshot ---

#[test]
fn append_step_snapshot_initializes_array_and_appends() {
    let snap1 = capture(&PathBuf::new(), None, Some("sid"), || "t1".to_string());
    let snap2 = capture(&PathBuf::new(), None, Some("sid"), || "t2".to_string());
    let mut state = json!({"phases": {"flow-code": {}}, "current_phase": "flow-code"});
    append_step_snapshot(&mut state, "flow-code", 1, "code_task", snap1);
    append_step_snapshot(&mut state, "flow-code", 2, "code_task", snap2);
    let arr = state["phases"]["flow-code"]["step_snapshots"]
        .as_array()
        .expect("step_snapshots populated");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["step"], 1);
    assert_eq!(arr[0]["captured_at"], "t1");
    assert_eq!(arr[1]["step"], 2);
    assert_eq!(arr[1]["captured_at"], "t2");
}

#[test]
fn append_step_snapshot_extends_existing_array() {
    let snap = capture(&PathBuf::new(), None, Some("sid"), || "t1".to_string());
    let mut state = json!({
        "phases": {"flow-code": {"step_snapshots": [{"existing": true}]}}
    });
    append_step_snapshot(&mut state, "flow-code", 5, "code_task", snap);
    let arr = state["phases"]["flow-code"]["step_snapshots"]
        .as_array()
        .expect("array");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["existing"], true);
    assert_eq!(arr[1]["step"], 5);
}

#[test]
fn append_step_snapshot_with_non_object_state_is_noop() {
    let snap = capture(&PathBuf::new(), None, Some("sid"), || "t".to_string());
    let mut state = Value::Array(vec![json!(1)]);
    let before = state.clone();
    append_step_snapshot(&mut state, "flow-code", 1, "code_task", snap);
    assert_eq!(state, before);
}

#[test]
fn append_step_snapshot_auto_heals_non_object_phases() {
    let mut state = json!({"phases": 5});
    let snap = capture(std::path::Path::new(""), None, Some("sid"), || {
        "now".to_string()
    });
    append_step_snapshot(&mut state, "flow-code", 1, "code_task", snap);
    assert!(state["phases"].is_object());
    assert!(state["phases"]["flow-code"]["step_snapshots"].is_array());
}

#[test]
fn append_step_snapshot_auto_heals_non_object_phase_entry() {
    let mut state = json!({"phases": {"flow-code": 42}});
    let snap = capture(std::path::Path::new(""), None, Some("sid"), || {
        "now".to_string()
    });
    append_step_snapshot(&mut state, "flow-code", 1, "code_task", snap);
    assert!(state["phases"]["flow-code"].is_object());
    assert_eq!(state["phases"]["flow-code"]["step_snapshots"][0]["step"], 1);
}

// --- write_snapshot_into_state ---

#[test]
fn write_snapshot_into_state_inserts_at_named_field() {
    let snap = capture(&PathBuf::new(), None, Some("sid"), || "now".to_string());
    let mut state = json!({"existing": 1});
    write_snapshot_into_state(&mut state, "window_at_start", &snap);
    assert!(state["window_at_start"].is_object());
    assert_eq!(state["window_at_start"]["session_id"], "sid");
    assert_eq!(state["existing"], 1);
}

#[test]
fn write_snapshot_into_state_with_non_object_state_is_noop() {
    let snap = capture(&PathBuf::new(), None, Some("sid"), || "now".to_string());
    let mut state = Value::Array(vec![json!({"a": 1})]);
    let before = state.clone();
    write_snapshot_into_state(&mut state, "window_at_start", &snap);
    assert_eq!(state, before);
}

// --- home_dir_or_empty ---

#[test]
fn home_dir_or_empty_returns_path_when_home_set() {
    let home = home_dir_or_empty();
    // Empty acceptable when HOME unset; function's contract is
    // "no panic" rather than "non-empty".
    let _ = home.as_os_str();
}

// --- is_safe_transcript_path_structural ---

/// The regression case for issue #1525: the structural validator
/// accepts a path that is shape-valid (absolute, under
/// `<home>/.claude/projects/`, no NUL, no ParentDir component)
/// even when the underlying JSONL file does not yet exist on
/// disk. SessionStart hooks receive a transcript_path before
/// Claude Code creates the file; the canonical validator's
/// `canonicalize()` call fails on the missing file and rejects
/// the path, causing transcript_path to persist as null and
/// downstream `record-agent-return` to report
/// `phase_marker_not_found`.
#[test]
fn is_safe_transcript_path_structural_accepts_nonexistent_path_under_projects() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().canonicalize().expect("canonicalize");
    let projects = home.join(".claude").join("projects").join("proj");
    fs::create_dir_all(&projects).expect("mkdir projects");
    let transcript = projects.join("session.jsonl");
    assert!(!transcript.exists(), "fixture must not create the JSONL");

    assert!(
        is_safe_transcript_path_structural(&transcript, &home),
        "structural validator must accept a non-existent path under <home>/.claude/projects/"
    );
}

/// Empty path fails the structural validator. Same first guard
/// as the canonical validator; rejection-class parity.
#[test]
fn is_safe_transcript_path_structural_rejects_empty() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().canonicalize().expect("canonicalize");
    let empty = PathBuf::from("");
    assert!(!is_safe_transcript_path_structural(&empty, &home));
}

/// Path containing a NUL byte fails the structural validator.
/// Defends against `format!`/path-construction shapes where a
/// NUL truncates syscall semantics in implementation-defined
/// ways.
#[test]
fn is_safe_transcript_path_structural_rejects_nul_byte() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().canonicalize().expect("canonicalize");
    let path = PathBuf::from("/tmp/contains\0nul.jsonl");
    assert!(!is_safe_transcript_path_structural(&path, &home));
}

/// Relative path fails the structural validator. Production
/// callers must pass absolute paths so prefix-containment is
/// well-defined.
#[test]
fn is_safe_transcript_path_structural_rejects_relative() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().canonicalize().expect("canonicalize");
    let rel = PathBuf::from("relative/session.jsonl");
    assert!(!is_safe_transcript_path_structural(&rel, &home));
}

/// Path containing a `..` ParentDir component fails the
/// structural validator. The lexical `starts_with` check below
/// does not resolve `..`, so `<home>/.claude/projects/../../etc/passwd`
/// would otherwise pass the prefix check and reach a file open.
#[test]
fn is_safe_transcript_path_structural_rejects_parent_dir_component() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().canonicalize().expect("canonicalize");
    let escape = home
        .join(".claude")
        .join("projects")
        .join("..")
        .join("..")
        .join("etc")
        .join("passwd");
    assert!(!is_safe_transcript_path_structural(&escape, &home));
}

/// Absolute path that does not sit under `<home>/.claude/projects/`
/// fails the structural validator. The lexical prefix check
/// must reject paths outside the expected directory tree even
/// when canonicalize would have produced the same rejection
/// for an existing file.
#[test]
fn is_safe_transcript_path_structural_rejects_path_outside_projects_prefix() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().canonicalize().expect("canonicalize");
    let outside = PathBuf::from("/etc/passwd");
    assert!(!is_safe_transcript_path_structural(&outside, &home));
}

// --- is_safe_transcript_path (canonical wrapper) ---

/// The canonical wrapper short-circuits when the structural check
/// rejects the input. Exercises the `if !structural { return false }`
/// branch with a path that fails structural validation so neither
/// canonicalize syscall runs.
#[test]
fn is_safe_transcript_path_rejects_when_structural_fails() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().canonicalize().expect("canonicalize");
    let empty = PathBuf::from("");
    assert!(!is_safe_transcript_path(&empty, &home));
}

/// The canonical wrapper accepts a path that is shape-valid AND
/// exists on disk under `<home>/.claude/projects/`. Exercises the
/// success path through both canonicalize calls and the final
/// `starts_with` check.
#[test]
fn is_safe_transcript_path_accepts_existing_path_under_projects() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().canonicalize().expect("canonicalize");
    let projects = home.join(".claude").join("projects").join("proj");
    fs::create_dir_all(&projects).expect("mkdir projects");
    let transcript = projects.join("session.jsonl");
    fs::write(&transcript, b"").expect("write transcript");
    assert!(is_safe_transcript_path(&transcript, &home));
}

/// The canonical wrapper rejects a shape-valid path whose
/// underlying file does not exist on disk — `canonicalize` on the
/// path fails. Exercises the `Err(_) => return false` arm of the
/// first canonicalize match.
#[test]
fn is_safe_transcript_path_rejects_when_path_canonicalize_fails() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().canonicalize().expect("canonicalize");
    let projects = home.join(".claude").join("projects").join("proj");
    fs::create_dir_all(&projects).expect("mkdir projects");
    let transcript = projects.join("session.jsonl");
    assert!(!transcript.exists());
    assert!(!is_safe_transcript_path(&transcript, &home));
}

// --- sub-agent (sidechain + sub-session-file) capture ---
//
// Cost is token-derived, so a flow's cost is only correct when the
// per-model usage of its Task-tool sub-agents is folded into the
// snapshot. Sub-agent turns persist in one of two shapes: inlined in
// the parent transcript flagged `isSidechain:true`, or in separate
// `agent-<agentId>.jsonl` sibling files linked by an `agentId` the
// parent references. `capture` counts both and dedups by message id
// so an overlap (a turn that appears inlined AND in the sub-session
// file) is counted once.

/// Helper: an assistant-message JSON line carrying a `message.id`
/// (used to exercise the cross-source dedup).
fn assistant_line_with_id(model: &str, input: i64, output: i64, id: &str) -> String {
    format!(
        r#"{{"type":"assistant","message":{{"id":"{id}","model":"{model}","role":"assistant","content":[{{"type":"text","text":"hi"}}],"usage":{{"input_tokens":{input},"output_tokens":{output},"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#
    )
}

/// Helper: a sidechain marker line that references a sub-agent by
/// `agentId` without itself being an assistant turn — the link the
/// parent carries so `capture` discovers `agent-<id>.jsonl`.
fn agent_marker_line(agent_id: &str) -> String {
    format!(
        r#"{{"type":"user","isSidechain":true,"agentId":"{agent_id}","message":{{"role":"user","content":"spawn"}}}}"#
    )
}

/// Build `<home>/.claude/projects/<project>/` and return it. The
/// canonical transcript location so sub-session paths pass
/// `is_safe_transcript_path`.
fn projects_dir(home: &std::path::Path, project: &str) -> PathBuf {
    let dir = home.join(".claude").join("projects").join(project);
    fs::create_dir_all(&dir).expect("mkdir projects dir");
    dir
}

/// An inlined sidechain assistant turn in the parent transcript is
/// counted toward token totals and by_model — no separate file
/// needed when the sub-agent's turns are inlined.
#[test]
fn capture_counts_inlined_sidechain_assistant_turn() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let sidechain_turn = r#"{"type":"assistant","isSidechain":true,"agentId":"A","message":{"model":"claude-opus-4-7","role":"assistant","content":[{"type":"text","text":"hi"}],"usage":{"input_tokens":40,"output_tokens":20,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#;
    let transcript = write_transcript(
        &root,
        "session.jsonl",
        &[
            &assistant_line("claude-opus-4-7", 100, 50, 0, 0),
            sidechain_turn,
        ],
    );
    let snap = capture(&root, Some(&transcript), Some("sid"), || "now".to_string());
    // Parent turn (100/50) + inlined sidechain turn (40/20).
    assert_eq!(snap.session_input_tokens, Some(140));
    assert_eq!(snap.session_output_tokens, Some(70));
    assert_eq!(snap.turn_count, Some(2));
    assert_eq!(snap.by_model.get("claude-opus-4-7").unwrap().input, 140);
}

/// A sub-agent whose turns live in a separate `agent-<id>.jsonl`
/// sibling is discovered via the `agentId` the parent references,
/// and its usage is summed into the snapshot.
#[test]
fn capture_sums_linked_sub_session_file_usage() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let dir = projects_dir(&root, "proj");
    let transcript = write_transcript(
        &dir,
        "session.jsonl",
        &[
            &assistant_line("claude-opus-4-7", 100, 50, 0, 0),
            &agent_marker_line("A"),
        ],
    );
    write_transcript(
        &dir,
        "agent-A.jsonl",
        &[&assistant_line("claude-sonnet-4-6", 30, 15, 0, 0)],
    );
    let snap = capture(&root, Some(&transcript), Some("sid"), || "now".to_string());
    // Parent opus (100/50) + sub-session sonnet (30/15).
    assert_eq!(snap.session_input_tokens, Some(130));
    assert_eq!(snap.session_output_tokens, Some(65));
    assert_eq!(snap.turn_count, Some(2));
    assert_eq!(snap.by_model.get("claude-opus-4-7").unwrap().input, 100);
    assert_eq!(snap.by_model.get("claude-sonnet-4-6").unwrap().input, 30);
}

/// A turn that appears BOTH inlined in the parent AND in the
/// sub-session file (same `message.id`) is counted exactly once.
#[test]
fn capture_dedups_message_id_across_parent_and_sub_session() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let dir = projects_dir(&root, "proj");
    let shared = assistant_line_with_id("claude-opus-4-7", 30, 15, "msg-shared");
    let transcript = write_transcript(
        &dir,
        "session.jsonl",
        &[
            &assistant_line("claude-opus-4-7", 100, 50, 0, 0),
            &agent_marker_line("A"),
            &shared,
        ],
    );
    write_transcript(&dir, "agent-A.jsonl", &[&shared]);
    let snap = capture(&root, Some(&transcript), Some("sid"), || "now".to_string());
    // Parent (100/50) + shared turn once (30/15) = 130/65, NOT
    // double-counted to 160/80.
    assert_eq!(snap.session_input_tokens, Some(130));
    assert_eq!(snap.session_output_tokens, Some(65));
    assert_eq!(snap.turn_count, Some(2));
}

/// An `agentId` that fails `is_safe_session_id` is not used to
/// construct a sub-session path — only the parent is counted, and no
/// traversal is attempted. Two rejection classes are exercised: a
/// traversal segment (`.`, caught by the empty/dot guard) and a
/// path separator (`a/b`, caught by the character allowlist).
#[test]
fn capture_skips_sub_session_with_unsafe_agent_id() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let dir = projects_dir(&root, "proj");
    let transcript = write_transcript(
        &dir,
        "session.jsonl",
        &[
            &assistant_line("claude-opus-4-7", 100, 50, 0, 0),
            &agent_marker_line("."),
            &agent_marker_line("a/b"),
        ],
    );
    let snap = capture(&root, Some(&transcript), Some("sid"), || "now".to_string());
    assert_eq!(snap.session_input_tokens, Some(100));
    assert_eq!(snap.turn_count, Some(1));
}

/// A transcript path with no parent directory (e.g. an empty path
/// from a corrupted state file) yields no sub-session discovery —
/// `path.parent()` is `None`, so the discovery loop is skipped and
/// the snapshot carries no token data.
#[test]
fn capture_with_parentless_transcript_path_reads_no_sub_sessions() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let snap = capture(&root, Some(std::path::Path::new("")), Some("sid"), || {
        "now".to_string()
    });
    assert_eq!(snap.session_input_tokens, None);
    assert_eq!(snap.turn_count, None);
    assert!(snap.by_model.is_empty());
}

/// When the parent transcript is not under `<home>/.claude/projects/`,
/// the sibling `agent-<id>.jsonl` path fails `is_safe_transcript_path`
/// and is not read — only the parent is counted.
#[test]
fn capture_skips_sub_session_when_path_out_of_prefix() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    // Parent at root/session.jsonl — NOT under .claude/projects/.
    let transcript = write_transcript(
        &root,
        "session.jsonl",
        &[
            &assistant_line("claude-opus-4-7", 100, 50, 0, 0),
            &agent_marker_line("A"),
        ],
    );
    // Even though a sibling agent file exists, its path is rejected.
    write_transcript(
        &root,
        "agent-A.jsonl",
        &[&assistant_line("claude-sonnet-4-6", 30, 15, 0, 0)],
    );
    let snap = capture(&root, Some(&transcript), Some("sid"), || "now".to_string());
    assert_eq!(snap.session_input_tokens, Some(100));
    assert_eq!(snap.turn_count, Some(1));
    assert!(snap.by_model.get("claude-sonnet-4-6").is_none());
}
