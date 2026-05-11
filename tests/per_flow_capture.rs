//! Integration tests for `src/per_flow_capture.rs` — the
//! orchestrator that bundles `session_metrics::capture` and
//! `session_cost::read_cost_file` into a final `WindowSnapshot`
//! for state-mutating callers. Every fixture path is canonicalized
//! at construction per `.claude/rules/testing-gotchas.md` "macOS
//! Subprocess Path Canonicalization".

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use flow_rs::per_flow_capture::capture_for_active_state;
use serde_json::{json, Value};

/// Build `<home>/.claude/rate-limits.json` with the supplied pcts.
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

/// Encode a project-root path the way Claude Code names the
/// per-project transcript directory under `~/.claude/projects/`.
fn encode_project_root_for_projects_dir(project_root: &std::path::Path) -> String {
    let s = project_root.to_string_lossy();
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

// --- capture_for_active_state ---

/// Task 5: `capture_for_active_state` bundles metrics and cost.
/// Fixture with both transcript and cost file populated; assert
/// the returned snapshot has both token fields (from
/// session_metrics) and cost field (from session_cost).
#[test]
fn capture_for_active_state_bundles_metrics_and_cost() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    write_rate_limits(&root, 33, 9);

    let projects_dir = root.join(".claude").join("projects");
    fs::create_dir_all(&projects_dir).expect("mkdir projects");
    let transcript = write_transcript(
        &projects_dir,
        "session.jsonl",
        &[&assistant_line("claude-opus-4-7", 100, 50, 0, 0)],
    );

    let now = chrono::Local::now();
    let year_month = now.format("%Y-%m").to_string();
    let cost_dir = root.join(".claude").join("cost").join(&year_month);
    fs::create_dir_all(&cost_dir).expect("mkdir cost");
    fs::write(cost_dir.join("sid-abc"), "0.42").expect("write cost");

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

/// Task 6: `capture_for_active_state` returns snapshot when cost
/// file absent. Fixture with transcript but no cost file; assert
/// snapshot has tokens but `session_cost_usd: None` (fail-open
/// semantics preserved).
#[test]
fn capture_for_active_state_returns_snapshot_when_cost_file_absent() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");

    let projects_dir = root.join(".claude").join("projects");
    fs::create_dir_all(&projects_dir).expect("mkdir projects");
    let transcript = write_transcript(
        &projects_dir,
        "session.jsonl",
        &[&assistant_line("claude-opus-4-7", 200, 80, 0, 0)],
    );

    // Note: NO cost file written.
    let state = json!({
        "session_id": "sid-nocost",
        "transcript_path": transcript.to_string_lossy(),
    });

    let snap = capture_for_active_state(&root, &state, &root);
    assert_eq!(snap.session_input_tokens, Some(200));
    assert_eq!(snap.session_output_tokens, Some(80));
    assert_eq!(snap.session_cost_usd, None);
}

/// `capture_for_active_state` with empty state JSON (no session
/// fields) still produces a snapshot — fail-open per the
/// helper's no-panic contract.
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
    assert_eq!(snap.session_cost_usd, None);
}

// --- Validation guards ---

#[test]
fn capture_for_active_state_rejects_traversal_session_id() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let state = json!({
        "session_id": "../../etc/passwd",
        "transcript_path": null,
    });
    let snap = capture_for_active_state(&root, &state, &root);
    assert_eq!(snap.session_id, None);
}

#[test]
fn capture_for_active_state_rejects_empty_session_id() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let state = json!({"session_id": ""});
    let snap = capture_for_active_state(&root, &state, &root);
    assert_eq!(snap.session_id, None);
}

#[test]
fn capture_for_active_state_rejects_dot_session_id() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let state = json!({"session_id": "."});
    let snap = capture_for_active_state(&root, &state, &root);
    assert_eq!(snap.session_id, None);
}

#[test]
fn capture_for_active_state_rejects_dotdot_session_id() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let state = json!({"session_id": ".."});
    let snap = capture_for_active_state(&root, &state, &root);
    assert_eq!(snap.session_id, None);
}

#[test]
fn capture_for_active_state_rejects_relative_transcript_path() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let state = json!({
        "session_id": "valid-sid",
        "transcript_path": "relative/path/transcript.jsonl",
    });
    let snap = capture_for_active_state(&root, &state, &root);
    assert_eq!(snap.session_input_tokens, None);
}

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

#[test]
fn capture_for_active_state_rejects_transcript_path_with_parent_dir_component() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let projects = root.join(".claude").join("projects");
    fs::create_dir_all(&projects).expect("mkdir projects");
    let traversal = projects.join("..").join("evil.jsonl");
    let state = json!({
        "session_id": "valid-sid",
        "transcript_path": traversal.to_string_lossy(),
    });
    let snap = capture_for_active_state(&root, &state, &root);
    assert_eq!(snap.session_input_tokens, None);
}

#[test]
fn capture_for_active_state_rejects_transcript_path_when_canonicalize_fails() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let projects = root.join(".claude").join("projects");
    fs::create_dir_all(&projects).expect("mkdir projects");
    let missing = projects.join("nonexistent").join("transcript.jsonl");
    let state = json!({
        "session_id": "valid-sid",
        "transcript_path": missing.to_string_lossy(),
    });
    let snap = capture_for_active_state(&root, &state, &root);
    assert_eq!(snap.session_input_tokens, None);
}

// --- cost_file_path producer-naming contract ---

/// Regression: the per-session cost file produced by the
/// statusline is named without a `.txt` extension.
#[test]
fn capture_for_active_state_reads_cost_file_without_txt_extension() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let session_id = "sid-no-ext";
    let year_month = chrono::Local::now().format("%Y-%m").to_string();
    let cost_dir = root.join(".claude").join("cost").join(&year_month);
    fs::create_dir_all(&cost_dir).expect("mkdir cost");
    fs::write(cost_dir.join(session_id), "2.50").expect("write cost");

    let state = json!({"session_id": session_id});
    let snap = capture_for_active_state(&root, &state, &root);
    assert_eq!(
        snap.session_cost_usd,
        Some(2.50),
        "cost_file_path must match the producer's no-extension naming"
    );
}

// --- self-healing transcript path ---

/// Regression: when state's `transcript_path` is null but the file
/// exists at the canonical location, `capture_for_active_state`
/// self-heals by deriving the path from session_id + project_root.
#[test]
fn capture_for_active_state_derives_transcript_path_when_state_has_null() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let session_id = "sid-null-transcript";

    let encoded = encode_project_root_for_projects_dir(&root);
    let projects_dir = root.join(".claude").join("projects").join(&encoded);
    fs::create_dir_all(&projects_dir).expect("mkdir projects subdir");
    let transcript_name = format!("{}.jsonl", session_id);
    let lines = [
        assistant_line("claude-opus-4-7", 100, 50, 0, 0),
        assistant_line("claude-opus-4-7", 200, 75, 0, 0),
    ];
    let line_refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    write_transcript(&projects_dir, &transcript_name, &line_refs);

    let state = json!({
        "session_id": session_id,
        "transcript_path": Value::Null,
    });
    let snap = capture_for_active_state(&root, &state, &root);
    assert_eq!(
        snap.session_input_tokens,
        Some(300),
        "self-heal must read transcript when state's transcript_path is null"
    );
    assert_eq!(snap.session_output_tokens, Some(125));
    assert_eq!(snap.turn_count, Some(2));
    assert_eq!(snap.model.as_deref(), Some("claude-opus-4-7"));
}

/// Regression: the encoder converts every non-alphanumeric
/// character (other than `_` and `-`) to `-` — including spaces.
#[test]
fn capture_for_active_state_self_heal_handles_project_root_with_spaces() {
    let tmp = TempDir::new().expect("tempdir");
    let base = tmp.path().canonicalize().expect("canonicalize");
    let project_root = base.join("My Project").join("flow");
    fs::create_dir_all(&project_root).expect("mkdir project");
    let session_id = "sid-spaces";

    let encoded = encode_project_root_for_projects_dir(&project_root);
    let projects_dir = base.join(".claude").join("projects").join(&encoded);
    fs::create_dir_all(&projects_dir).expect("mkdir projects subdir");
    let transcript_name = format!("{}.jsonl", session_id);
    let lines = [assistant_line("claude-opus-4-7", 444, 222, 0, 0)];
    let line_refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    write_transcript(&projects_dir, &transcript_name, &line_refs);

    let state = json!({
        "session_id": session_id,
        "transcript_path": Value::Null,
    });
    let snap = capture_for_active_state(&base, &state, &project_root);
    assert_eq!(
        snap.session_input_tokens,
        Some(444),
        "self-heal must find the transcript when project root contains spaces"
    );
    assert_eq!(snap.session_output_tokens, Some(222));
}
