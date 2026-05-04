//! Integration tests for `src/window_snapshot.rs::capture` — every
//! branch is exercised through fixture-controlled inputs (tempdir
//! `home`, fake transcript JSONL, fake cost file). Per
//! `.claude/rules/testing-gotchas.md` "macOS Subprocess Path
//! Canonicalization", every fixture path is canonicalized at
//! construction so prefix comparisons hold across `/var` ↔
//! `/private/var` symlinks.

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use flow_rs::window_snapshot::capture;

/// Build a `home/.claude/rate-limits.json` file inside `dir` with
/// the supplied pcts. Returns the path to `dir` (the synthetic
/// `$HOME` to pass to `capture`).
fn write_rate_limits(dir: &std::path::Path, five: i64, seven: i64) {
    let claude_dir = dir.join(".claude");
    fs::create_dir_all(&claude_dir).expect("mkdir .claude");
    let body = format!(
        r#"{{"five_hour_pct":{},"seven_day_pct":{}}}"#,
        five, seven
    );
    fs::write(claude_dir.join("rate-limits.json"), body).expect("write rate-limits");
}

/// Write a transcript JSONL file with the supplied lines.
fn write_transcript(dir: &std::path::Path, name: &str, lines: &[&str]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, lines.join("\n") + "\n").expect("write transcript");
    path
}

/// Write a cost file with the supplied float-as-string content.
fn write_cost(dir: &std::path::Path, name: &str, content: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).expect("write cost");
    path
}

/// Helper for an assistant-message JSON line.
fn assistant_line(model: &str, input: i64, output: i64, cache_create: i64, cache_read: i64) -> String {
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
    let cost = write_cost(&root, "session.cost.txt", "0.987654");

    let snap = capture(
        &root,
        Some(&transcript),
        Some(&cost),
        Some("sid-123"),
        || "2026-05-04T10:00:00-07:00".to_string(),
    );

    assert_eq!(snap.captured_at, "2026-05-04T10:00:00-07:00");
    assert_eq!(snap.session_id.as_deref(), Some("sid-123"));
    assert_eq!(snap.model.as_deref(), Some("claude-opus-4-7"));
    assert_eq!(snap.five_hour_pct, Some(42));
    assert_eq!(snap.seven_day_pct, Some(7));
    assert_eq!(snap.session_input_tokens, Some(100));
    assert_eq!(snap.session_output_tokens, Some(50));
    assert_eq!(snap.session_cache_creation_tokens, Some(10));
    assert_eq!(snap.session_cache_read_tokens, Some(20));
    assert_eq!(snap.session_cost_usd, Some(0.987654));
    assert_eq!(snap.turn_count, Some(1));
    assert_eq!(snap.tool_call_count, Some(0));
    assert_eq!(snap.context_at_last_turn_tokens, Some(180));
    // 180 / 200_000 * 100 = 0.09
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
    // Note: no write_rate_limits call.
    let transcript = write_transcript(
        &root,
        "session.jsonl",
        &[&assistant_line("claude-opus-4-7", 100, 50, 0, 0)],
    );

    let snap = capture(
        &root,
        Some(&transcript),
        None,
        Some("sid"),
        || "now".to_string(),
    );

    assert_eq!(snap.five_hour_pct, None);
    assert_eq!(snap.seven_day_pct, None);
    assert_eq!(snap.session_input_tokens, Some(100));
    assert_eq!(snap.turn_count, Some(1));
}

/// No transcript path → token / turn / tool / by_model fields are
/// `None` / empty while rate-limits and cost still flow through.
#[test]
fn capture_with_missing_transcript_sets_token_fields_none() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    write_rate_limits(&root, 42, 7);
    let cost = write_cost(&root, "session.cost.txt", "0.5");

    let snap = capture(&root, None, Some(&cost), Some("sid"), || "now".to_string());

    assert_eq!(snap.session_input_tokens, None);
    assert_eq!(snap.session_output_tokens, None);
    assert_eq!(snap.session_cache_creation_tokens, None);
    assert_eq!(snap.session_cache_read_tokens, None);
    assert_eq!(snap.turn_count, None);
    assert_eq!(snap.tool_call_count, None);
    assert!(snap.by_model.is_empty());
    assert_eq!(snap.session_cost_usd, Some(0.5));
    assert_eq!(snap.five_hour_pct, Some(42));
}

/// No cost file → `session_cost_usd` is `None` while everything
/// else populates.
#[test]
fn capture_with_missing_cost_file_sets_cost_none() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    write_rate_limits(&root, 42, 7);
    let transcript = write_transcript(
        &root,
        "session.jsonl",
        &[&assistant_line("claude-opus-4-7", 1, 1, 0, 0)],
    );

    // Pass a path to a cost file that does not exist.
    let cost_path = root.join("missing-cost.txt");
    let snap = capture(
        &root,
        Some(&transcript),
        Some(&cost_path),
        Some("sid"),
        || "now".to_string(),
    );

    assert_eq!(snap.session_cost_usd, None);
    assert_eq!(snap.session_input_tokens, Some(1));
}

/// `session_id` argument is `None` → snapshot's `session_id` is
/// `None`. Other fields still populate.
#[test]
fn capture_with_missing_session_id_sets_session_id_none() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    write_rate_limits(&root, 42, 7);

    let snap = capture(&root, None, None, None, || "now".to_string());

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

    let snap = capture(
        &root,
        Some(&transcript),
        None,
        Some("sid"),
        || "now".to_string(),
    );

    assert_eq!(snap.by_model.len(), 2);
    let opus = snap.by_model.get("claude-opus-4-7").expect("opus entry");
    assert_eq!(opus.input, 300);
    assert_eq!(opus.output, 150);
    let sonnet = snap.by_model.get("claude-sonnet-4-6").expect("sonnet entry");
    assert_eq!(sonnet.input, 10);
    assert_eq!(sonnet.output, 5);
    // Aggregate session totals match summed by_model
    assert_eq!(snap.session_input_tokens, Some(310));
    assert_eq!(snap.session_output_tokens, Some(155));
    assert_eq!(snap.turn_count, Some(3));
}

/// Malformed JSONL lines are skipped silently; valid lines still
/// contribute. Guards against partial-write tail rows in an active
/// session's transcript.
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

    let snap = capture(
        &root,
        Some(&transcript),
        None,
        Some("sid"),
        || "now".to_string(),
    );

    // 7 + 5 = 12 input tokens from the two well-formed assistant lines.
    assert_eq!(snap.session_input_tokens, Some(12));
    assert_eq!(snap.turn_count, Some(2));
}

/// Transcript with no assistant messages → every counter is `None`
/// (not `Some(0)`) so readers can distinguish "no session activity"
/// from "session with zero usage".
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

    let snap = capture(
        &root,
        Some(&transcript),
        None,
        Some("sid"),
        || "now".to_string(),
    );

    assert_eq!(snap.session_input_tokens, None);
    assert_eq!(snap.session_output_tokens, None);
    assert_eq!(snap.turn_count, None);
    assert_eq!(snap.tool_call_count, None);
    assert!(snap.by_model.is_empty());
}

/// `context_at_last_turn_tokens` reflects the MOST RECENT assistant
/// message — not a sum across all of them. Guards the
/// "current context utilization" semantic.
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

    let snap = capture(
        &root,
        Some(&transcript),
        None,
        Some("sid"),
        || "now".to_string(),
    );

    // Most recent message: 1000 + 500 + 100 + 200 = 1800
    assert_eq!(snap.context_at_last_turn_tokens, Some(1800));
    // Sum across all messages still in the cumulative fields
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

    let snap = capture(
        &root,
        Some(&transcript),
        None,
        Some("sid"),
        || "now".to_string(),
    );

    assert_eq!(snap.tool_call_count, Some(5));
    assert_eq!(snap.turn_count, Some(3));
}

// --- additional branch coverage ---

/// Cost file present but malformed (non-numeric content) → cost
/// gracefully resolves to `None` rather than panicking.
#[test]
fn capture_with_malformed_cost_file_sets_cost_none() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let cost = write_cost(&root, "cost.txt", "not-a-number");
    let snap = capture(&root, None, Some(&cost), Some("sid"), || "now".to_string());
    assert_eq!(snap.session_cost_usd, None);
}

/// Cost file containing infinity → fail-open to `None` because
/// non-finite values would corrupt downstream cost summaries.
#[test]
fn capture_with_infinite_cost_value_sets_cost_none() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let cost = write_cost(&root, "cost.txt", "inf");
    let snap = capture(&root, None, Some(&cost), Some("sid"), || "now".to_string());
    assert_eq!(snap.session_cost_usd, None);
}

/// Rate-limits file present but malformed JSON → both pcts `None`
/// and the rest of the snapshot still populates.
#[test]
fn capture_with_malformed_rate_limits_sets_pcts_none() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let claude_dir = root.join(".claude");
    fs::create_dir_all(&claude_dir).expect("mkdir");
    fs::write(claude_dir.join("rate-limits.json"), "{not json").expect("write");
    let snap = capture(&root, None, None, None, || "now".to_string());
    assert_eq!(snap.five_hour_pct, None);
    assert_eq!(snap.seven_day_pct, None);
}

/// Rate-limits JSON missing the expected keys → pcts default to
/// `None` rather than zero.
#[test]
fn capture_with_rate_limits_missing_keys_sets_pcts_none() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let claude_dir = root.join(".claude");
    fs::create_dir_all(&claude_dir).expect("mkdir");
    fs::write(claude_dir.join("rate-limits.json"), "{}").expect("write");
    let snap = capture(&root, None, None, None, || "now".to_string());
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
    let snap = capture(&root, Some(&path), None, None, || "now".to_string());
    assert_eq!(snap.turn_count, None);
}

/// Assistant message missing the `usage` object → counters
/// contribute zero rather than panicking.
#[test]
fn capture_with_assistant_missing_usage_contributes_zero_tokens() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let line = r#"{"type":"assistant","message":{"model":"claude-opus-4-7","role":"assistant","content":[]}}"#;
    let transcript = write_transcript(&root, "session.jsonl", &[line]);
    let snap = capture(&root, Some(&transcript), None, Some("sid"), || "now".to_string());
    assert_eq!(snap.session_input_tokens, Some(0));
    assert_eq!(snap.turn_count, Some(1));
    assert_eq!(snap.context_at_last_turn_tokens, Some(0));
}

/// Assistant line missing `message` field is skipped — no panic
/// from the option chain.
#[test]
fn capture_with_assistant_missing_message_is_skipped() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let line = r#"{"type":"assistant"}"#;
    let transcript = write_transcript(&root, "session.jsonl", &[line]);
    let snap = capture(&root, Some(&transcript), None, Some("sid"), || "now".to_string());
    assert_eq!(snap.turn_count, None);
}

/// Assistant message missing `model` → by_model is empty but
/// session totals still accumulate.
#[test]
fn capture_with_assistant_missing_model_skips_by_model() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let line = r#"{"type":"assistant","message":{"role":"assistant","content":[],"usage":{"input_tokens":10,"output_tokens":5,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#;
    let transcript = write_transcript(&root, "session.jsonl", &[line]);
    let snap = capture(&root, Some(&transcript), None, Some("sid"), || "now".to_string());
    assert_eq!(snap.session_input_tokens, Some(10));
    assert!(snap.by_model.is_empty());
    assert_eq!(snap.context_window_pct, None);
}

/// 1M context model variant uses the larger denominator for
/// `context_window_pct`.
#[test]
fn capture_with_1m_context_model_uses_million_token_window() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let line = assistant_line("claude-opus-4-7[1m]", 100_000, 0, 0, 0);
    let transcript = write_transcript(&root, "session.jsonl", &[&line]);
    let snap = capture(&root, Some(&transcript), None, Some("sid"), || "now".to_string());
    // 100_000 / 1_000_000 * 100 = 10.0
    let pct = snap.context_window_pct.expect("pct populated for known model");
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
    let snap = capture(&root, Some(&transcript), None, Some("sid"), || "now".to_string());
    assert_eq!(snap.session_input_tokens, Some(3));
    assert_eq!(snap.tool_call_count, Some(0));
}

/// Transcript with non-UTF-8 bytes on a line → `BufRead::lines()`
/// yields `Err` for that line; capture skips it silently (no
/// panic) and the rest of the snapshot still populates from valid
/// lines that follow.
#[test]
fn capture_with_non_utf8_line_skips_silently() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let path = root.join("session.jsonl");
    // 0xFF is not a valid UTF-8 lead byte, so reader.lines() yields
    // Err for the first record. Any well-formed JSONL after a
    // newline still contributes.
    let mut bytes = vec![0xFF, b'\n'];
    bytes.extend(assistant_line("claude-opus-4-7", 5, 3, 0, 0).bytes());
    bytes.push(b'\n');
    fs::write(&path, &bytes).expect("write");
    let snap = capture(&root, Some(&path), None, Some("sid"), || "now".to_string());
    assert_eq!(snap.session_input_tokens, Some(5));
    assert_eq!(snap.turn_count, Some(1));
}

/// Non-Claude model name → `context_window_pct` is `None` (no
/// known window size).
#[test]
fn capture_with_unknown_model_returns_none_context_window_pct() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let line = assistant_line("custom-model-xyz", 100, 0, 0, 0);
    let transcript = write_transcript(&root, "session.jsonl", &[&line]);
    let snap = capture(&root, Some(&transcript), None, Some("sid"), || "now".to_string());
    assert_eq!(snap.context_window_pct, None);
    assert_eq!(snap.context_at_last_turn_tokens, Some(100));
}
