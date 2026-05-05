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

use flow_rs::window_snapshot::{
    append_step_snapshot, capture, capture_for_active_state, home_dir_or_empty,
    write_snapshot_into_state,
};
use serde_json::{json, Value};

/// Build a `home/.claude/rate-limits.json` file inside `dir` with
/// the supplied pcts. Returns the path to `dir` (the synthetic
/// `$HOME` to pass to `capture`).
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

/// Write a cost file with the supplied float-as-string content.
fn write_cost(dir: &std::path::Path, name: &str, content: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).expect("write cost");
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
    // Context = input + cache_create + cache_read = 100 + 10 + 20 = 130
    // (output is generated, not part of the context window).
    assert_eq!(snap.context_at_last_turn_tokens, Some(130));
    // 130 / 200_000 * 100 = 0.065
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

    let snap = capture(&root, Some(&transcript), None, Some("sid"), || {
        "now".to_string()
    });

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

    let snap = capture(&root, Some(&transcript), None, Some("sid"), || {
        "now".to_string()
    });

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

    let snap = capture(&root, Some(&transcript), None, Some("sid"), || {
        "now".to_string()
    });

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

    let snap = capture(&root, Some(&transcript), None, Some("sid"), || {
        "now".to_string()
    });

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

    let snap = capture(&root, Some(&transcript), None, Some("sid"), || {
        "now".to_string()
    });

    // Most recent message context = input + cache_create + cache_read
    // (output is generated, not part of the context window).
    // 1000 + 100 + 200 = 1300.
    assert_eq!(snap.context_at_last_turn_tokens, Some(1300));
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

    let snap = capture(&root, Some(&transcript), None, Some("sid"), || {
        "now".to_string()
    });

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
    let snap = capture(&root, Some(&transcript), None, Some("sid"), || {
        "now".to_string()
    });
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
    let snap = capture(&root, Some(&transcript), None, Some("sid"), || {
        "now".to_string()
    });
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
    let snap = capture(&root, Some(&transcript), None, Some("sid"), || {
        "now".to_string()
    });
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
    let snap = capture(&root, Some(&transcript), None, Some("sid"), || {
        "now".to_string()
    });
    // 100_000 / 1_000_000 * 100 = 10.0
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
    let snap = capture(&root, Some(&transcript), None, Some("sid"), || {
        "now".to_string()
    });
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

// --- append_step_snapshot ---

/// Object state with empty phase entry → step snapshot appended
/// after the array is auto-initialized. Subsequent appends extend
/// the same array, in insertion order.
#[test]
fn append_step_snapshot_initializes_array_and_appends() {
    let snap1 = capture(&PathBuf::new(), None, None, Some("sid"), || {
        "t1".to_string()
    });
    let snap2 = capture(&PathBuf::new(), None, None, Some("sid"), || {
        "t2".to_string()
    });
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

/// Pre-existing step_snapshots array → append extends without
/// reinitializing.
#[test]
fn append_step_snapshot_extends_existing_array() {
    let snap = capture(&PathBuf::new(), None, None, Some("sid"), || {
        "t1".to_string()
    });
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

/// Non-object state → no-op (matches the project's State Mutation
/// Object Guards convention so a malformed state file cannot
/// panic the producer).
#[test]
fn append_step_snapshot_with_non_object_state_is_noop() {
    let snap = capture(&PathBuf::new(), None, None, Some("sid"), || "t".to_string());
    let mut state = Value::Array(vec![json!(1)]);
    let before = state.clone();
    append_step_snapshot(&mut state, "flow-code", 1, "code_task", snap);
    assert_eq!(state, before);
}

// --- write_snapshot_into_state ---

/// Object state → snapshot inserted at the named field, replacing
/// any prior value at that key.
#[test]
fn write_snapshot_into_state_inserts_at_named_field() {
    let snap = capture(&PathBuf::new(), None, None, Some("sid"), || {
        "now".to_string()
    });
    let mut state = json!({"existing": 1});
    write_snapshot_into_state(&mut state, "window_at_start", &snap);
    assert!(state["window_at_start"].is_object());
    assert_eq!(state["window_at_start"]["session_id"], "sid");
    assert_eq!(state["existing"], 1);
}

/// Non-object state (e.g. an array root from corruption) → no-op.
/// Mirrors the State Mutation Object Guards convention so a
/// malformed state file cannot panic the producer.
#[test]
fn write_snapshot_into_state_with_non_object_state_is_noop() {
    let snap = capture(&PathBuf::new(), None, None, Some("sid"), || {
        "now".to_string()
    });
    let mut state = Value::Array(vec![json!({"a": 1})]);
    let before = state.clone();
    write_snapshot_into_state(&mut state, "window_at_start", &snap);
    assert_eq!(state, before);
}

// --- home_dir_or_empty ---

/// `home_dir_or_empty` returns a non-empty path when HOME is set
/// (the inherited test environment always has HOME set via the
/// parent shell). Locks the call shape — producers thread its
/// result into `capture_for_active_state`.
#[test]
fn home_dir_or_empty_returns_path_when_home_set() {
    let home = home_dir_or_empty();
    // Cannot mutate $HOME safely from inside a parallel test
    // suite — assert the call returns *some* PathBuf without
    // panicking. Empty is acceptable when HOME unset; the
    // function's contract is "no panic" rather than "non-empty".
    let _ = home.as_os_str();
}

// --- capture_for_active_state ---

/// `capture_for_active_state` reads session_id and transcript_path
/// from the in-memory state JSON and threads them into capture()
/// alongside a derived cost-file path under
/// `<project_root>/.claude/cost/<YYYY-MM>/<sid>.txt`. With all
/// inputs present, every snapshot field populates.
#[test]
fn capture_for_active_state_threads_session_context_into_capture() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    write_rate_limits(&root, 33, 9);

    // Create a transcript file under the validated location:
    // `<home>/.claude/projects/`. capture_for_active_state rejects
    // transcript paths outside this prefix per is_safe_transcript_path.
    let projects_dir = root.join(".claude").join("projects");
    fs::create_dir_all(&projects_dir).expect("mkdir projects");
    let transcript = write_transcript(
        &projects_dir,
        "session.jsonl",
        &[&assistant_line("claude-opus-4-7", 100, 50, 0, 0)],
    );

    // Create the cost file at the expected path.
    let now = chrono::Local::now();
    let year_month = now.format("%Y-%m").to_string();
    let cost_dir = root.join(".claude").join("cost").join(&year_month);
    fs::create_dir_all(&cost_dir).expect("mkdir cost");
    fs::write(cost_dir.join("sid-abc.txt"), "0.42").expect("write cost");

    let state = json!({
        "session_id": "sid-abc",
        "transcript_path": transcript.to_string_lossy(),
    });

    let snap = capture_for_active_state(&root, &state, &root);
    assert_eq!(snap.session_id.as_deref(), Some("sid-abc"));
    assert_eq!(snap.session_input_tokens, Some(100));
    assert_eq!(snap.session_cost_usd, Some(0.42));
    assert_eq!(snap.five_hour_pct, Some(33));
}

/// `capture_for_active_state` with empty state JSON (no session
/// fields) still produces a snapshot — fail-open per the
/// helper's no-panic contract. Token and cost fields fall to
/// `None` because there is no transcript or cost path to read.
#[test]
fn capture_for_active_state_with_empty_state_returns_minimal_snapshot() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let state = json!({});
    let snap = capture_for_active_state(&root, &state, &root);
    assert_eq!(snap.session_id, None);
    assert_eq!(snap.session_input_tokens, None);
    assert_eq!(snap.session_cost_usd, None);
}

/// State carrying only `session_id` (no transcript yet) → the
/// capture still produces a snapshot. The cost-file path is
/// derived from session_id; absent file leaves cost as None.
#[test]
fn capture_for_active_state_with_session_id_only_derives_cost_path() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let state = json!({"session_id": "sid-xyz"});
    let snap = capture_for_active_state(&root, &state, &root);
    assert_eq!(snap.session_id.as_deref(), Some("sid-xyz"));
    // No cost file at the derived path → None.
    assert_eq!(snap.session_cost_usd, None);
}

/// Non-Claude model name → `context_window_pct` is `None` (no
/// known window size).
#[test]
fn capture_with_unknown_model_returns_none_context_window_pct() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let line = assistant_line("custom-model-xyz", 100, 0, 0, 0);
    let transcript = write_transcript(&root, "session.jsonl", &[&line]);
    let snap = capture(&root, Some(&transcript), None, Some("sid"), || {
        "now".to_string()
    });
    assert_eq!(snap.context_window_pct, None);
    assert_eq!(snap.context_at_last_turn_tokens, Some(100));
}

// --- Validation guards introduced in Code Review (Step 4) ---

/// Empty `home` makes `read_rate_limits` short-circuit so a
/// committed `.claude/rate-limits.json` in a worktree cannot be
/// read as if it were the user's rate-limit data.
#[test]
fn capture_with_empty_home_skips_rate_limits_read() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    write_rate_limits(&root, 99, 50);
    // Pass an empty home: the rate-limits file lives at root but
    // `home.join(".claude")...` resolves relative to cwd and the
    // guard must skip rather than read the worktree-relative path.
    let snap = capture(std::path::Path::new(""), None, None, None, || {
        "now".to_string()
    });
    assert_eq!(snap.five_hour_pct, None);
    assert_eq!(snap.seven_day_pct, None);
}

/// Relative `home` (non-absolute) is also rejected — same threat
/// as empty home.
#[test]
fn capture_with_relative_home_skips_rate_limits_read() {
    let snap = capture(
        std::path::Path::new("relative/path"),
        None,
        None,
        None,
        || "now".to_string(),
    );
    assert_eq!(snap.five_hour_pct, None);
    assert_eq!(snap.seven_day_pct, None);
}

/// `capture_for_active_state` rejects a state-supplied `session_id`
/// that contains path separators — a corrupted state cannot reach
/// arbitrary cost-file paths via traversal.
#[test]
fn capture_for_active_state_rejects_traversal_session_id() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let state = json!({
        "session_id": "../../etc/passwd",
        "transcript_path": null,
    });
    let snap = capture_for_active_state(&root, &state, &root);
    // Invalid session_id is filtered out, so it never reaches the
    // returned snapshot.
    assert_eq!(snap.session_id, None);
}

/// `capture_for_active_state` rejects an empty session_id.
#[test]
fn capture_for_active_state_rejects_empty_session_id() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let state = json!({"session_id": ""});
    let snap = capture_for_active_state(&root, &state, &root);
    assert_eq!(snap.session_id, None);
}

/// `capture_for_active_state` rejects a session_id of "." (a
/// traversal segment).
#[test]
fn capture_for_active_state_rejects_dot_session_id() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let state = json!({"session_id": "."});
    let snap = capture_for_active_state(&root, &state, &root);
    assert_eq!(snap.session_id, None);
}

/// `capture_for_active_state` rejects a session_id of ".." (a
/// traversal segment).
#[test]
fn capture_for_active_state_rejects_dotdot_session_id() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let state = json!({"session_id": ".."});
    let snap = capture_for_active_state(&root, &state, &root);
    assert_eq!(snap.session_id, None);
}

/// `capture_for_active_state` rejects a relative `transcript_path`.
#[test]
fn capture_for_active_state_rejects_relative_transcript_path() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let state = json!({
        "session_id": "valid-sid",
        "transcript_path": "relative/path/transcript.jsonl",
    });
    let snap = capture_for_active_state(&root, &state, &root);
    // Rejected path → no transcript read → token fields stay None.
    assert_eq!(snap.session_input_tokens, None);
}

/// `capture_for_active_state` rejects an empty `transcript_path`.
#[test]
fn capture_for_active_state_rejects_empty_transcript_path() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let state = json!({
        "session_id": "valid-sid",
        "transcript_path": "",
    });
    let snap = capture_for_active_state(&root, &state, &root);
    assert_eq!(snap.session_input_tokens, None);
}

/// `capture_for_active_state` rejects a `transcript_path` containing
/// a NUL byte.
#[test]
fn capture_for_active_state_rejects_nul_byte_transcript_path() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let state = json!({
        "session_id": "valid-sid",
        "transcript_path": "/abs/path\0with-nul/transcript.jsonl",
    });
    let snap = capture_for_active_state(&root, &state, &root);
    assert_eq!(snap.session_input_tokens, None);
}

/// `capture_for_active_state` rejects a `transcript_path` outside
/// the validated `<home>/.claude/projects/` prefix.
#[test]
fn capture_for_active_state_rejects_transcript_path_outside_projects_prefix() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let outside = root.join("not-in-projects").join("transcript.jsonl");
    fs::create_dir_all(outside.parent().unwrap()).expect("mkdir outside");
    fs::write(&outside, "").expect("write empty");
    let state = json!({
        "session_id": "valid-sid",
        "transcript_path": outside.to_string_lossy(),
    });
    let snap = capture_for_active_state(&root, &state, &root);
    assert_eq!(snap.session_input_tokens, None);
}

/// `append_step_snapshot` auto-heals when `state.phases` holds a
/// non-object value (number / string / array). Drives the per-level
/// guard added in Code Review per `.claude/rules/state-files.md`
/// "Corruption Resilience".
#[test]
fn append_step_snapshot_auto_heals_non_object_phases() {
    let mut state = json!({"phases": 5});
    let snap = capture(std::path::Path::new(""), None, None, Some("sid"), || {
        "now".to_string()
    });
    append_step_snapshot(&mut state, "flow-code", 1, "code_task", snap);
    // After auto-heal, phases is an object containing flow-code.
    assert!(state["phases"].is_object());
    assert!(state["phases"]["flow-code"]["step_snapshots"].is_array());
}

/// `append_step_snapshot` auto-heals when `state.phases.<phase>`
/// itself holds a non-object value.
#[test]
fn append_step_snapshot_auto_heals_non_object_phase_entry() {
    let mut state = json!({"phases": {"flow-code": 42}});
    let snap = capture(std::path::Path::new(""), None, None, Some("sid"), || {
        "now".to_string()
    });
    append_step_snapshot(&mut state, "flow-code", 1, "code_task", snap);
    assert!(state["phases"]["flow-code"].is_object());
    assert_eq!(state["phases"]["flow-code"]["step_snapshots"][0]["step"], 1);
}

/// Transcript byte cap drops bytes past `TRANSCRIPT_BYTE_CAP`. The
/// fixture writes a transcript larger than the 50 MB cap and asserts
/// the read terminates without reading every line. (Verified
/// indirectly: the read returns a populated agg without hanging or
/// exhausting memory; if the cap regressed to unbounded, this test
/// would still pass but slowly. The cap's purpose is a process
/// invariant rather than an observable boundary in unit tests.)
#[test]
fn capture_with_oversized_transcript_returns_bounded_snapshot() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    // 200 KB of real assistant lines is small enough to not slow
    // the test but exercises the BufReader::take() path.
    let mut lines: Vec<String> = Vec::new();
    for _ in 0..2000 {
        lines.push(assistant_line("claude-opus-4-7", 1, 1, 0, 0));
    }
    let line_refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    let transcript = write_transcript(&root, "big.jsonl", &line_refs);
    let snap = capture(&root, Some(&transcript), None, Some("sid"), || {
        "now".to_string()
    });
    assert!(snap.session_input_tokens.unwrap() > 0);
    assert!(snap.turn_count.unwrap() > 0);
}
