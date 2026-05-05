//! Integration tests for `src/format_complete_summary.rs`. Migrated
//! from inline `#[cfg(test)]` in `src/format_complete_summary.rs` per
//! `.claude/rules/test-placement.md`.
//!
//! `truncate_prompt`, `outcome_marker`, and `outcome_label` are
//! private helpers driven through the public `format_complete_summary`
//! entry point; coverage comes from crafted state fixtures that force
//! each branch.

use std::path::{Path, PathBuf};

use flow_rs::format_complete_summary::{format_complete_summary, run_impl, run_impl_main, Args};
use serde_json::{json, Value};

const PHASE_NAMES_LIST: [&str; 6] = ["Start", "Plan", "Code", "Code Review", "Learn", "Complete"];

fn all_complete_state() -> Value {
    let mut phases = serde_json::Map::new();
    let all_phases = [
        "flow-start",
        "flow-plan",
        "flow-code",
        "flow-code-review",
        "flow-learn",
        "flow-complete",
    ];
    let timings = [20, 300, 2700, 720, 120, 45];
    for (i, &p) in all_phases.iter().enumerate() {
        phases.insert(
            p.to_string(),
            json!({
                "name": PHASE_NAMES_LIST[i],
                "status": "complete",
                "started_at": "2026-01-01T00:00:00-08:00",
                "completed_at": "2026-01-01T01:00:00-08:00",
                "session_started_at": null,
                "cumulative_seconds": timings[i],
                "visit_count": 1,
            }),
        );
    }
    json!({
        "branch": "test-feature",
        "pr_url": "https://github.com/test/test/pull/1",
        "started_at": "2026-01-01T00:00:00-08:00",
        "current_phase": "flow-complete",
        "prompt": "Add invoice PDF export with watermark support",
        "issues_filed": [],
        "notes": [],
        "phases": phases,
    })
}

fn write_state_file(dir: &Path) -> PathBuf {
    let state = all_complete_state();
    let state_file = dir.join("state.json");
    std::fs::write(&state_file, serde_json::to_string(&state).unwrap()).unwrap();
    state_file
}

// --- Token Cost section ---

/// Build a snapshot Value for fixtures. `n` scales each numeric
/// field so callers can produce monotonically-increasing snapshots.
fn snapshot_value(session: &str, n: i64, model: &str) -> Value {
    json!({
        "captured_at": format!("2026-01-01T0{}:00:00-08:00", n.min(9)),
        "session_id": session,
        "model": model,
        "five_hour_pct": n,
        "seven_day_pct": n / 2,
        "session_input_tokens": n * 100,
        "session_output_tokens": n * 50,
        "session_cache_creation_tokens": 0,
        "session_cache_read_tokens": 0,
        "session_cost_usd": n as f64 * 0.01,
        "by_model": {
            model: {"input": n * 100, "output": n * 50, "cache_create": 0, "cache_read": 0}
        },
        "turn_count": n,
        "tool_call_count": n * 2,
        "context_at_last_turn_tokens": n * 100,
        "context_window_pct": (n * 100) as f64 / 200_000.0 * 100.0,
    })
}

fn add_phase_snapshots(state: &mut Value, key: &str, enter_n: i64, complete_n: i64) {
    state["phases"][key]["window_at_enter"] = snapshot_value("S1", enter_n, "claude-opus-4-7");
    state["phases"][key]["window_at_complete"] =
        snapshot_value("S1", complete_n, "claude-opus-4-7");
}

/// Full data: every phase carries enter+complete snapshots.
/// The Token Cost section renders header + per-phase rows + total.
#[test]
fn token_cost_section_with_full_data_renders_per_phase_and_total() {
    let mut state = all_complete_state();
    add_phase_snapshots(&mut state, "flow-start", 0, 5);
    add_phase_snapshots(&mut state, "flow-plan", 5, 10);
    add_phase_snapshots(&mut state, "flow-code", 10, 50);

    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("Token Cost"), "section header");
    assert!(result.summary.contains("Total:"));
    // 5 -> tokens grew by (5*100 + 5*50) = 750 from enter=0 to complete=5
    // for Start phase. Across 3 phases the combined Total should be > 0.
}

/// Empty state (no snapshots anywhere) → section is omitted entirely.
#[test]
fn token_cost_section_with_no_snapshots_returns_empty() {
    let state = all_complete_state();
    let result = format_complete_summary(&state, None);
    assert!(
        !result.summary.contains("Token Cost"),
        "Token Cost header must not appear when no phases have snapshots"
    );
}

/// Partial data: only some phases have snapshots → other phases are
/// silently skipped from the per-phase rows.
#[test]
fn token_cost_section_with_partial_data_skips_unpopulated_phases() {
    let mut state = all_complete_state();
    // Only flow-code has snapshots.
    add_phase_snapshots(&mut state, "flow-code", 10, 50);
    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("Token Cost"));
    // Plan and Start phases must NOT appear in the Token Cost rows.
    let token_section_start = result.summary.find("Token Cost").expect("section");
    let after_token = &result.summary[token_section_start..];
    let next_section = after_token.find("\n\n").unwrap_or(after_token.len());
    let token_block = &after_token[..next_section];
    assert!(token_block.contains("Code:"));
    assert!(!token_block.contains("Start:"));
    assert!(!token_block.contains("Plan:"));
}

/// Window reset observed: pct delta drops between enter and complete
/// for a phase → reset marker (↻) appears next to that phase row and
/// a footer note explains the marker.
#[test]
fn token_cost_section_with_window_reset_marks_observed() {
    let mut state = all_complete_state();
    let mut enter = snapshot_value("S1", 80, "claude-opus-4-7");
    let mut complete = snapshot_value("S1", 5, "claude-opus-4-7");
    // Force token growth so the row isn't filtered out
    enter["session_input_tokens"] = json!(100);
    complete["session_input_tokens"] = json!(500);
    state["phases"]["flow-code"]["window_at_enter"] = enter;
    state["phases"]["flow-code"]["window_at_complete"] = complete;
    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("Token Cost"));
    assert!(
        result.summary.contains("↻"),
        "reset marker must appear when window_reset_observed is true"
    );
    assert!(
        result.summary.contains("rate-limit window reset"),
        "footer note must explain the marker"
    );
}

/// Single-model flow: by_model rollup has one entry → the By Model
/// table is suppressed (no point showing a one-row breakdown).
#[test]
fn token_cost_section_single_model_skips_by_model_table() {
    let mut state = all_complete_state();
    add_phase_snapshots(&mut state, "flow-code", 10, 50);
    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("Token Cost"));
    assert!(
        !result.summary.contains("By Model"),
        "single-model rollup should suppress the By Model table"
    );
}

/// Multi-model flow: by_model rollup has 2+ entries → the By Model
/// table is rendered with one row per model.
#[test]
fn token_cost_section_multi_model_renders_by_model_table() {
    let mut state = all_complete_state();
    state["phases"]["flow-code"]["window_at_enter"] = snapshot_value("S1", 0, "claude-opus-4-7");
    let mut complete_v = snapshot_value("S1", 50, "claude-opus-4-7");
    // Add a second model's bucket to the by_model map.
    complete_v["by_model"]["claude-sonnet-4-6"] = json!({
        "input": 1000, "output": 500, "cache_create": 0, "cache_read": 0
    });
    state["phases"]["flow-code"]["window_at_complete"] = complete_v;
    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("By Model"));
    assert!(result.summary.contains("claude-opus-4-7"));
    assert!(result.summary.contains("claude-sonnet-4-6"));
}

/// `state.phases` is null → no phase entries to walk; section
/// short-circuits via the `state.get("phases").and_then` chain.
#[test]
fn token_cost_section_with_null_phases_value_returns_empty() {
    let mut state = all_complete_state();
    state["phases"] = json!(null);
    let result = format_complete_summary(&state, None);
    assert!(!result.summary.contains("Token Cost"));
}

/// Phase has snapshots but every counter field is zero → the
/// delta is all-zero and the row is skipped from the section.
#[test]
fn token_cost_section_with_zero_delta_phase_is_skipped() {
    let mut state = all_complete_state();
    // enter=0 and complete=0 → all numeric snapshot fields zero,
    // so the delta is zero across the board.
    add_phase_snapshots(&mut state, "flow-code", 0, 0);
    let result = format_complete_summary(&state, None);
    // Section should NOT render — the only phase with snapshots
    // contributed nothing.
    assert!(!result.summary.contains("Token Cost"));
}

/// Phase has snapshots with cost but no token delta → the row is
/// kept (the cost arm of the AND-skip branch fires false).
#[test]
fn token_cost_section_with_cost_but_no_token_delta_renders_row() {
    let mut state = all_complete_state();
    let mut enter = snapshot_value("S1", 0, "claude-opus-4-7");
    let mut complete = snapshot_value("S1", 0, "claude-opus-4-7");
    // Force cost to differ between enter and complete while
    // keeping every token counter at zero.
    enter["session_cost_usd"] = json!(0.0);
    complete["session_cost_usd"] = json!(0.50);
    state["phases"]["flow-code"]["window_at_enter"] = enter;
    state["phases"]["flow-code"]["window_at_complete"] = complete;
    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("Token Cost"));
    assert!(result.summary.contains("$0.500") || result.summary.contains("$0.5"));
}

/// Phases value missing from state → token_cost_section short-
/// circuits per phase via the `let Some(phase_v) = ...` else-arm.
/// Coverage of the None branch.
#[test]
fn token_cost_section_with_missing_phases_object_returns_empty() {
    let mut state = all_complete_state();
    // Drop the entire phases object so each PHASE_ORDER lookup fails.
    state.as_object_mut().unwrap().remove("phases");
    let result = format_complete_summary(&state, None);
    assert!(!result.summary.contains("Token Cost"));
}

/// Phase value with a malformed `window_at_enter` shape (wrong type)
/// → from_value::<PhaseState> returns Err and the phase is skipped.
#[test]
fn token_cost_section_with_unparseable_phase_skips_silently() {
    let mut state = all_complete_state();
    // Replace one phase entry with a non-object value so PhaseState
    // deserialization fails.
    state["phases"]["flow-code"] = json!("not an object");
    // Plus add a real snapshot pair on a different phase so the
    // section renders for that phase but skips flow-code.
    add_phase_snapshots(&mut state, "flow-plan", 5, 10);
    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("Token Cost"));
    assert!(result.summary.contains("Plan:"));
}

/// `format_tokens` boundary cases: < 1000 (raw integer), >= 1M
/// (megaformat). Drives both branches via crafted snapshots that
/// produce token counts in those ranges.
#[test]
fn token_cost_section_format_tokens_covers_small_and_million_ranges() {
    let mut state = all_complete_state();
    // Tiny token delta (< 1000): enter=0 → complete=1 produces
    // 100 input + 50 output = 150 tokens.
    add_phase_snapshots(&mut state, "flow-start", 0, 1);
    // Million-range delta: scale `n` so n*100 + n*50 > 1_000_000.
    // n=10000 → 1_500_000 tokens.
    add_phase_snapshots(&mut state, "flow-code", 0, 10000);
    let result = format_complete_summary(&state, None);
    // Raw integer for the small phase.
    assert!(result.summary.contains("150"));
    // M suffix for the million-range phase.
    assert!(result.summary.contains("M"));
}

// --- basic summary ---

#[test]
fn basic_summary() {
    let state = all_complete_state();
    let result = format_complete_summary(&state, None);

    assert!(result.summary.contains("Test Feature"));
    assert!(result
        .summary
        .contains("Add invoice PDF export with watermark support"));
    assert!(result
        .summary
        .contains("https://github.com/test/test/pull/1"));
    for name in &PHASE_NAMES_LIST {
        assert!(
            result.summary.contains(&format!("{}:", name)),
            "Missing phase {} in summary:\n{}",
            name,
            result.summary
        );
    }
    assert!(result.summary.contains("Total:"));
    assert_eq!(result.total_seconds, 20 + 300 + 2700 + 720 + 120 + 45);
}

#[test]
fn summary_with_issues() {
    let mut state = all_complete_state();
    state["issues_filed"] = json!([
        {
            "label": "Rule",
            "title": "Test rule",
            "url": "https://github.com/test/test/issues/1",
            "phase": "flow-learn",
            "phase_name": "Learn",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
        {
            "label": "Tech Debt",
            "title": "Refactor X",
            "url": "https://github.com/test/test/issues/2",
            "phase": "flow-code-review",
            "phase_name": "Code Review",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
    ]);

    let result = format_complete_summary(&state, None);

    assert!(result.summary.contains("Issues filed: 2"));
    assert!(!result
        .summary
        .contains("https://github.com/test/test/issues/1"));
    assert!(!result
        .summary
        .contains("https://github.com/test/test/issues/2"));
    assert!(result.issues_links.contains("[Rule] #1 Test rule"));
    assert!(result
        .issues_links
        .contains("https://github.com/test/test/issues/1"));
    assert!(result.issues_links.contains("[Tech Debt] #2 Refactor X"));
    assert!(result
        .issues_links
        .contains("https://github.com/test/test/issues/2"));
}

#[test]
fn summary_with_single_issue() {
    let mut state = all_complete_state();
    state["issues_filed"] = json!([
        {
            "label": "Flow",
            "title": "Fix routing logic",
            "url": "https://github.com/test/test/issues/42",
            "phase": "flow-learn",
            "phase_name": "Learn",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
    ]);

    let result = format_complete_summary(&state, None);

    assert!(result.summary.contains("Issues filed: 1"));
    assert!(!result
        .summary
        .contains("https://github.com/test/test/issues/42"));
    assert!(result.issues_links.contains("[Flow] #42 Fix routing logic"));
    assert!(result
        .issues_links
        .contains("https://github.com/test/test/issues/42"));
}

#[test]
fn summary_with_issues_url_without_number() {
    let mut state = all_complete_state();
    state["issues_filed"] = json!([
        {
            "label": "Rule",
            "title": "Some rule",
            "url": "https://example.com/custom-path",
            "phase": "flow-learn",
            "phase_name": "Learn",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
    ]);

    let result = format_complete_summary(&state, None);

    assert!(result.summary.contains("Issues filed: 1"));
    assert!(!result.summary.contains("https://example.com/custom-path"));
    assert!(result.issues_links.contains("[Rule] Some rule"));
    assert!(result
        .issues_links
        .contains("https://example.com/custom-path"));
}

// --- resolved / closed_issues ---

#[test]
fn summary_with_resolved_issues() {
    let state = all_complete_state();
    let closed = vec![json!({
        "number": 407,
        "url": "https://github.com/test/test/issues/407",
    })];

    let result = format_complete_summary(&state, Some(&closed));

    assert!(result.summary.contains("Resolved"));
    assert!(result.summary.contains("#407"));
    assert!(result
        .summary
        .contains("https://github.com/test/test/issues/407"));
}

#[test]
fn summary_with_multiple_resolved_issues() {
    let state = all_complete_state();
    let closed = vec![
        json!({"number": 83, "url": "https://github.com/test/test/issues/83"}),
        json!({"number": 89, "url": "https://github.com/test/test/issues/89"}),
    ];

    let result = format_complete_summary(&state, Some(&closed));

    assert!(result.summary.contains("#83"));
    assert!(result.summary.contains("#89"));
}

#[test]
fn summary_no_resolved_issues() {
    let state = all_complete_state();

    let result_none = format_complete_summary(&state, None);
    let result_empty = format_complete_summary(&state, Some(&[]));

    assert!(!result_none.summary.contains("Resolved"));
    assert!(!result_empty.summary.contains("Resolved"));
}

#[test]
fn summary_with_resolved_and_filed() {
    let mut state = all_complete_state();
    state["issues_filed"] = json!([
        {
            "label": "Tech Debt",
            "title": "Refactor X",
            "url": "https://github.com/test/test/issues/50",
            "phase": "flow-code-review",
            "phase_name": "Code Review",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
    ]);
    let closed = vec![json!({
        "number": 407,
        "url": "https://github.com/test/test/issues/407",
    })];

    let result = format_complete_summary(&state, Some(&closed));

    assert!(result.summary.contains("Resolved"));
    assert!(result.summary.contains("#407"));
    assert!(result.summary.contains("Issues filed: 1"));
    assert!(result.issues_links.contains("[Tech Debt] #50 Refactor X"));
    assert!(result
        .issues_links
        .contains("https://github.com/test/test/issues/50"));
}

#[test]
fn summary_resolved_without_url() {
    let state = all_complete_state();
    let closed = vec![json!({"number": 42})];

    let result = format_complete_summary(&state, Some(&closed));

    assert!(result.summary.contains("Resolved"));
    assert!(result.summary.contains("#42"));
}

#[test]
fn summary_with_filed_issue_without_url() {
    let mut state = all_complete_state();
    state["issues_filed"] = json!([
        {
            "label": "Rule",
            "title": "URL-less rule",
            "phase": "flow-learn",
            "phase_name": "Learn",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
    ]);

    let result = format_complete_summary(&state, None);
    assert!(result.issues_links.contains("[Rule] URL-less rule"));
    assert!(!result.issues_links.contains(" — "));
}

// --- outcome marker / label (private helpers driven through findings) ---

#[test]
fn summary_with_unknown_outcome_falls_back_to_question_marker() {
    let mut state = all_complete_state();
    state["findings"] = json!([
        {
            "finding": "future-outcome finding",
            "reason": "uses a not-yet-handled outcome",
            "outcome": "deferred",
            "phase": "flow-code-review",
            "phase_name": "Code Review",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
    ]);

    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("?"));
    assert!(result.summary.contains("Unknown"));
}

// --- notes / issue counts ---

#[test]
fn summary_with_notes() {
    let mut state = all_complete_state();
    state["notes"] = json!([
        {
            "phase": "flow-code",
            "phase_name": "Code",
            "timestamp": "2026-01-01T00:00:00-08:00",
            "type": "correction",
            "note": "Test note",
        },
    ]);

    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("Notes captured: 1"));
}

#[test]
fn summary_no_issues_no_notes() {
    let mut state = all_complete_state();
    state["issues_filed"] = json!([]);
    state["notes"] = json!([]);

    let result = format_complete_summary(&state, None);

    assert!(!result.summary.contains("Issues filed"));
    assert!(!result.summary.contains("Notes captured"));
    assert_eq!(result.issues_links, "");
}

#[test]
fn summary_issues_filed_key_absent_renders_empty_links() {
    let mut state = all_complete_state();
    state.as_object_mut().unwrap().remove("issues_filed");
    let result = format_complete_summary(&state, None);
    assert_eq!(result.issues_links, "");
}

#[test]
fn summary_issues_filed_wrong_type_renders_empty_links() {
    let mut state = all_complete_state();
    state["issues_filed"] = json!("not-an-array");
    let result = format_complete_summary(&state, None);
    assert_eq!(result.issues_links, "");
}

#[test]
fn issues_links_without_url() {
    let mut state = all_complete_state();
    state["issues_filed"] = json!([
        {
            "label": "Tech Debt",
            "title": "Missing test",
            "url": "",
            "phase": "flow-code-review",
            "phase_name": "Code Review",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
    ]);

    let result = format_complete_summary(&state, None);
    assert!(result.issues_links.contains("[Tech Debt] Missing test"));
    assert!(!result.issues_links.contains("—"));
}

// --- truncate_prompt coverage via format_complete_summary ---

#[test]
fn summary_truncates_long_prompt() {
    let mut state = all_complete_state();
    let long_prompt = "A".repeat(100);
    state["prompt"] = json!(long_prompt);

    let result = format_complete_summary(&state, None);

    assert!(!result.summary.contains(&long_prompt));
    assert!(result.summary.contains("..."));
    let expected = format!("{}...", "A".repeat(80));
    assert!(result.summary.contains(&expected));
}

#[test]
fn summary_short_prompt_not_truncated() {
    let mut state = all_complete_state();
    state["prompt"] = json!("Fix login bug");

    let result = format_complete_summary(&state, None);

    assert!(result.summary.contains("Fix login bug"));
    assert!(!result.summary.contains("..."));
}

#[test]
fn summary_prompt_exactly_at_limit_not_truncated() {
    // Covers the `<= MAX_PROMPT_LENGTH` boundary path of truncate_prompt
    // (80 chars exactly returns the prompt as-is).
    let mut state = all_complete_state();
    let exactly_80 = "A".repeat(80);
    state["prompt"] = json!(exactly_80.clone());

    let result = format_complete_summary(&state, None);

    assert!(result.summary.contains(&exactly_80));
    // No ellipsis when no truncation.
    assert!(!result.summary.contains("AAA..."));
}

#[test]
fn summary_prompt_multibyte_truncates_by_code_points() {
    // Covers the multi-byte code-point branch: 81 "日" chars is 81 code
    // points (> 80 limit) but 243 bytes — truncate_prompt must count
    // chars not bytes, taking 80 and appending "...".
    let mut state = all_complete_state();
    state["prompt"] = json!("日".repeat(81));

    let result = format_complete_summary(&state, None);

    let truncated = format!("{}...", "日".repeat(80));
    assert!(
        result.summary.contains(&truncated),
        "expected 80 chars + ... in summary, got:\n{}",
        result.summary
    );
}

// --- formatting chrome ---

#[test]
fn summary_uses_format_time() {
    let state = all_complete_state();
    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("<1m"));
    assert!(result.summary.contains("45m"));
    assert!(result.summary.contains("5m"));
}

#[test]
fn summary_heavy_borders() {
    let state = all_complete_state();
    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("━━"));
}

#[test]
fn summary_check_mark() {
    let state = all_complete_state();
    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("✓"));
}

#[test]
fn summary_version() {
    let state = all_complete_state();
    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("FLOW v"));
}

// --- run_impl ---

#[test]
fn cli_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = write_state_file(dir.path());
    let args = Args {
        state_file: state_file.to_string_lossy().to_string(),
        closed_issues_file: None,
    };
    let result = run_impl(&args).unwrap();
    assert!(result.summary.contains("Test Feature"));
    assert!(result.total_seconds > 0);
}

#[test]
fn cli_with_closed_issues_file() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = write_state_file(dir.path());
    let closed = vec![json!({
        "number": 407,
        "url": "https://github.com/test/test/issues/407",
    })];
    let closed_file = dir.path().join("closed.json");
    std::fs::write(&closed_file, serde_json::to_string(&closed).unwrap()).unwrap();

    let args = Args {
        state_file: state_file.to_string_lossy().to_string(),
        closed_issues_file: Some(closed_file.to_string_lossy().to_string()),
    };
    let result = run_impl(&args).unwrap();
    assert!(result.summary.contains("Resolved"));
    assert!(result.summary.contains("#407"));
}

#[test]
fn cli_missing_closed_issues_file() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = write_state_file(dir.path());
    let args = Args {
        state_file: state_file.to_string_lossy().to_string(),
        closed_issues_file: Some(
            dir.path()
                .join("nonexistent.json")
                .to_string_lossy()
                .to_string(),
        ),
    };
    let result = run_impl(&args).unwrap();
    assert!(!result.summary.contains("Resolved"));
}

#[test]
fn cli_missing_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        state_file: dir
            .path()
            .join("missing.json")
            .to_string_lossy()
            .to_string(),
        closed_issues_file: None,
    };
    let result = run_impl(&args);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not found"));
}

#[test]
fn cli_state_file_unreadable_returns_read_error() {
    // Covers the `map_err(|e| format!("Failed to read state file"))?`
    // closure on line 254 — exists() is true but read_to_string fails
    // because the path is a directory.
    let dir = tempfile::tempdir().unwrap();
    let state_as_dir = dir.path().join("state.json");
    std::fs::create_dir_all(&state_as_dir).unwrap();
    let args = Args {
        state_file: state_as_dir.to_string_lossy().to_string(),
        closed_issues_file: None,
    };
    let result = run_impl(&args);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Failed to read state file"));
}

#[test]
fn cli_corrupt_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let bad_file = dir.path().join("bad.json");
    std::fs::write(&bad_file, "{bad json").unwrap();
    let args = Args {
        state_file: bad_file.to_string_lossy().to_string(),
        closed_issues_file: None,
    };
    let result = run_impl(&args);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Failed to parse"));
}

#[test]
fn run_impl_closed_content_unreadable_omits_resolved() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = write_state_file(dir.path());
    let closed_dir = dir.path().join("closed_as_dir");
    std::fs::create_dir_all(&closed_dir).unwrap();
    let args = Args {
        state_file: state_file.to_string_lossy().to_string(),
        closed_issues_file: Some(closed_dir.to_string_lossy().to_string()),
    };
    let result = run_impl(&args).unwrap();
    assert!(!result.summary.contains("Resolved"));
    assert!(result.summary.contains("Test Feature"));
}

#[test]
fn run_impl_closed_content_malformed_omits_resolved() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = write_state_file(dir.path());
    let closed_file = dir.path().join("malformed.json");
    std::fs::write(&closed_file, "{not valid json").unwrap();
    let args = Args {
        state_file: state_file.to_string_lossy().to_string(),
        closed_issues_file: Some(closed_file.to_string_lossy().to_string()),
    };
    let result = run_impl(&args).unwrap();
    assert!(!result.summary.contains("Resolved"));
    assert!(result.summary.contains("Test Feature"));
}

// --- findings ---

#[test]
fn summary_with_code_review_findings() {
    let mut state = all_complete_state();
    state["findings"] = json!([
        {
            "finding": "Unused variable in handler",
            "reason": "False positive from macro expansion",
            "outcome": "dismissed",
            "phase": "flow-code-review",
            "phase_name": "Code Review",
            "timestamp": "2026-01-01T00:30:00-08:00",
        },
        {
            "finding": "Missing null check in parser",
            "reason": "Could panic on malformed input",
            "outcome": "fixed",
            "phase": "flow-code-review",
            "phase_name": "Code Review",
            "timestamp": "2026-01-01T00:31:00-08:00",
        },
    ]);

    let result = format_complete_summary(&state, None);

    assert!(result.summary.contains("Code Review Findings"));
    assert!(result.summary.contains("Unused variable in handler"));
    assert!(result.summary.contains("Missing null check in parser"));
    assert!(result.summary.contains("✗"));
    assert!(result.summary.contains("✓"));
    assert!(result
        .summary
        .contains("False positive from macro expansion"));
}

#[test]
fn summary_with_learn_findings() {
    let mut state = all_complete_state();
    state["findings"] = json!([
        {
            "finding": "No rule for error handling",
            "reason": "Gap identified during analysis",
            "outcome": "rule_written",
            "phase": "flow-learn",
            "phase_name": "Learn",
            "path": ".claude/rules/error-handling.md",
            "timestamp": "2026-01-01T00:45:00-08:00",
        },
    ]);

    let result = format_complete_summary(&state, None);

    assert!(result.summary.contains("Learn Findings"));
    assert!(result.summary.contains("No rule for error handling"));
    assert!(result.summary.contains("+"));
}

#[test]
fn summary_with_both_phase_findings() {
    let mut state = all_complete_state();
    state["findings"] = json!([
        {
            "finding": "Bug in parser",
            "reason": "Fixed inline",
            "outcome": "fixed",
            "phase": "flow-code-review",
            "phase_name": "Code Review",
            "timestamp": "2026-01-01T00:30:00-08:00",
        },
        {
            "finding": "Missing rule",
            "reason": "Created new rule",
            "outcome": "rule_written",
            "phase": "flow-learn",
            "phase_name": "Learn",
            "timestamp": "2026-01-01T00:45:00-08:00",
        },
    ]);

    let result = format_complete_summary(&state, None);

    assert!(result.summary.contains("Code Review Findings"));
    assert!(result.summary.contains("Learn Findings"));
}

#[test]
fn summary_no_findings_hides_sections() {
    let mut state = all_complete_state();
    state["findings"] = json!([]);

    let result_empty = format_complete_summary(&state, None);
    assert!(!result_empty.summary.contains("Code Review Findings"));
    assert!(!result_empty.summary.contains("Learn Findings"));

    let state_no_key = all_complete_state();
    let result_missing = format_complete_summary(&state_no_key, None);
    assert!(!result_missing.summary.contains("Code Review Findings"));
    assert!(!result_missing.summary.contains("Learn Findings"));
}

#[test]
fn summary_findings_all_outcomes() {
    let mut state = all_complete_state();
    state["findings"] = json!([
        {
            "finding": "f1",
            "reason": "r1",
            "outcome": "fixed",
            "phase": "flow-code-review",
            "phase_name": "Code Review",
            "timestamp": "2026-01-01T00:30:00-08:00",
        },
        {
            "finding": "f2",
            "reason": "r2",
            "outcome": "dismissed",
            "phase": "flow-code-review",
            "phase_name": "Code Review",
            "timestamp": "2026-01-01T00:31:00-08:00",
        },
        {
            "finding": "f3",
            "reason": "r3",
            "outcome": "filed",
            "phase": "flow-code-review",
            "phase_name": "Code Review",
            "issue_url": "https://github.com/test/test/issues/99",
            "timestamp": "2026-01-01T00:32:00-08:00",
        },
        {
            "finding": "f4",
            "reason": "r4",
            "outcome": "rule_written",
            "phase": "flow-learn",
            "phase_name": "Learn",
            "path": ".claude/rules/test.md",
            "timestamp": "2026-01-01T00:33:00-08:00",
        },
        {
            "finding": "f5",
            "reason": "r5",
            "outcome": "rule_clarified",
            "phase": "flow-learn",
            "phase_name": "Learn",
            "path": ".claude/rules/existing.md",
            "timestamp": "2026-01-01T00:34:00-08:00",
        },
    ]);

    let result = format_complete_summary(&state, None);
    assert!(result.summary.contains("✓"));
    assert!(result.summary.contains("✗"));
    assert!(result.summary.contains("→"));
    assert!(result.summary.find("Learn Findings").is_some());
}

#[test]
fn summary_findings_with_existing_artifacts() {
    let mut state = all_complete_state();
    state["findings"] = json!([
        {
            "finding": "Bug found",
            "reason": "Fixed it",
            "outcome": "fixed",
            "phase": "flow-code-review",
            "phase_name": "Code Review",
            "timestamp": "2026-01-01T00:30:00-08:00",
        },
    ]);
    state["issues_filed"] = json!([
        {
            "label": "Tech Debt",
            "title": "Refactor X",
            "url": "https://github.com/test/test/issues/50",
            "phase": "flow-code-review",
            "phase_name": "Code Review",
            "timestamp": "2026-01-01T00:00:00-08:00",
        },
    ]);

    let result = format_complete_summary(&state, None);

    assert!(result.summary.contains("Code Review Findings"));
    assert!(result.summary.contains("Issues filed: 1"));
}

// --- run_impl_main ---

#[test]
fn run_impl_main_happy_path_returns_ok_value() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = write_state_file(dir.path());
    let args = Args {
        state_file: state_file.to_string_lossy().to_string(),
        closed_issues_file: None,
    };
    let (value, code) = run_impl_main(&args);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert!(value["summary"].as_str().unwrap().contains("Test Feature"));
    assert!(value["total_seconds"].as_i64().unwrap() > 0);
}

#[test]
fn run_impl_main_missing_state_file_returns_err_exit_1() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        state_file: dir
            .path()
            .join("missing.json")
            .to_string_lossy()
            .to_string(),
        closed_issues_file: None,
    };
    let (value, code) = run_impl_main(&args);
    assert_eq!(code, 1);
    assert_eq!(value["status"], "error");
    assert!(value["message"].as_str().unwrap().contains("not found"));
}
