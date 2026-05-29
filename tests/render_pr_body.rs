//! Tests for `src/render_pr_body.rs`.
//!
//! Covers `render_body` and `format_timings_table` directly, plus
//! subprocess tests for `bin/flow render-pr-body` that exercise the
//! `run_impl_main` dispatch including gh subprocess failure paths.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{
    add_phase_snapshots, create_gh_stub, create_git_repo_with_remote, parse_output, snapshot_value,
};
use flow_rs::format_pr_timings::format_timings_table;
use flow_rs::phase_config::PHASE_ORDER;
use flow_rs::render_pr_body::{format_cost_table, render_body};
use serde_json::{json, Value};

// --- Fixtures ---

fn make_test_state() -> Value {
    json!({
        "schema_version": 1,
        "branch": "test-feature",
        "repo": "test/repo",
        "pr_number": 1,
        "pr_url": "https://github.com/test/repo/pull/1",
        "started_at": "2026-01-01T00:00:00Z",
        "current_phase": "flow-start",
        "files": {
            "plan": null,
            "dag": null,
            "log": ".flow-states/test-feature/log",
            "state": ".flow-states/test-feature/state.json"
        },
        "session_tty": null,
        "session_id": null,
        "transcript_path": null,
        "notes": [],
        "prompt": "test feature description",
        "phases": {
            "flow-start": {"name": "Start", "status": "in_progress", "started_at": "2026-01-01T00:00:00Z", "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 1},
            "flow-code": {"name": "Code", "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0},
            "flow-review": {"name": "Review", "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0},
            "flow-learn": {"name": "Learn", "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0},
            "flow-complete": {"name": "Complete", "status": "pending", "started_at": null, "completed_at": null, "session_started_at": null, "cumulative_seconds": 0, "visit_count": 0}
        }
    })
}

fn minimal_complete_state(feature: &str) -> Value {
    json!({
        "schema_version": 1,
        "branch": "test-branch",
        "feature": feature,
        "prompt": feature,
        "pr_number": 42,
        "pr_url": "https://github.com/o/r/pull/42",
        "phases": {
            "flow-start":        {"status": "complete", "cumulative_seconds": 10, "visit_count": 1},
            "flow-code":         {"status": "complete", "cumulative_seconds": 30, "visit_count": 1},
            "flow-review":  {"status": "complete", "cumulative_seconds": 40, "visit_count": 1},
            "flow-learn":        {"status": "complete", "cumulative_seconds": 50, "visit_count": 1},
            "flow-complete":     {"status": "pending"}
        },
        "findings": [],
        "issues_filed": [],
        "notes": [],
    })
}

fn write_state(repo: &Path, name: &str, state: &Value) -> std::path::PathBuf {
    let branch_dir = repo.join(".flow-states").join(name);
    fs::create_dir_all(&branch_dir).unwrap();
    let path = branch_dir.join("state.json");
    fs::write(&path, serde_json::to_string_pretty(state).unwrap()).unwrap();
    path
}

fn run_render(repo: &Path, args: &[&str], stub_dir: &Path) -> Output {
    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("render-pr-body")
        .args(args)
        .current_dir(repo)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

// --- format_timings_table ---

#[test]
fn timings_table_started_only_filters() {
    let mut state = make_test_state();
    state["phases"]["flow-start"]["cumulative_seconds"] = json!(30);
    state["phases"]["flow-code"]["started_at"] = json!("2026-01-01T00:01:00Z");
    state["phases"]["flow-code"]["cumulative_seconds"] = json!(300);

    let table = format_timings_table(&state, true);
    assert!(table.contains("| Start |"));
    assert!(table.contains("| Code |"));
    assert!(!table.contains("| Review |"));
    assert!(!table.contains("| Learn |"));
    assert!(!table.contains("| Complete |"));
    assert!(table.contains("| **Total** |"));
}

#[test]
fn timings_table_all_phases() {
    let mut state = make_test_state();
    for key in PHASE_ORDER {
        state["phases"][key]["started_at"] = json!("2026-01-01T00:00:00Z");
        state["phases"][key]["cumulative_seconds"] = json!(60);
    }

    let table = format_timings_table(&state, false);
    assert!(table.contains("| Start |"));
    assert!(table.contains("| Review |"));
    assert!(table.contains("| Complete |"));
}

#[test]
fn timings_table_total_row() {
    let mut state = make_test_state();
    state["phases"]["flow-start"]["cumulative_seconds"] = json!(120);
    state["phases"]["flow-code"]["started_at"] = json!("2026-01-01T00:01:00Z");
    state["phases"]["flow-code"]["cumulative_seconds"] = json!(180);

    let table = format_timings_table(&state, true);
    assert!(table.contains("| **Total** | **5m** |"));
}

#[test]
fn timings_table_float_cumulative_seconds() {
    let mut state = make_test_state();
    state["phases"]["flow-start"]["cumulative_seconds"] = json!(120.0);

    let table = format_timings_table(&state, true);
    assert!(table.contains("| Start | 2m |"));
    assert!(table.contains("| **Total** | **2m** |"));
}

// --- format_cost_table ---
//
// The Markdown formatter consumes the same `CostBreakdown` the
// terminal banner does. Tests below mirror the structural shape:
// header, per-phase rows, bold total, em-dash placeholders, partial /
// reset markers, optional By Model sub-table, empty-on-None.

fn full_cost_state() -> Value {
    let mut state = make_test_state();
    for key in PHASE_ORDER {
        state["phases"][key]["status"] = json!("complete");
    }
    add_phase_snapshots(&mut state, "flow-start", 0, 5);
    add_phase_snapshots(&mut state, "flow-code", 5, 15);
    add_phase_snapshots(&mut state, "flow-review", 15, 20);
    add_phase_snapshots(&mut state, "flow-learn", 20, 25);
    add_phase_snapshots(&mut state, "flow-complete", 25, 30);
    state
}

/// Header and separator lead the table — `| Phase | Tokens | Cost |`
/// then the markdown alignment row.
#[test]
fn cost_table_renders_header_and_separator() {
    let state = full_cost_state();
    let table = format_cost_table(&state);
    assert!(
        table.starts_with("| Phase | Tokens | Cost |\n|-------|--------|------|"),
        "header + separator must lead the table; got:\n{}",
        table
    );
}

/// Each non-pending phase appears as a `| <name> | ... | ... |` row.
#[test]
fn cost_table_renders_per_phase_rows() {
    let state = full_cost_state();
    let table = format_cost_table(&state);
    for name in ["Start", "Code", "Review", "Learn", "Complete"] {
        assert!(
            table.contains(&format!("| {} |", name)),
            "phase row for {} missing; table:\n{}",
            name,
            table
        );
    }
}

/// The final data row is the bold Total row.
#[test]
fn cost_table_total_row_is_bold() {
    let state = full_cost_state();
    let table = format_cost_table(&state);
    assert!(
        table.contains("| **Total** | **"),
        "bold Total row must appear; table:\n{}",
        table
    );
}

/// A phase row whose cost is `None` renders the em-dash placeholder
/// in the Cost cell.
#[test]
fn cost_table_uses_em_dash_for_unknown_cost() {
    let mut state = make_test_state();
    for key in PHASE_ORDER {
        state["phases"][key]["status"] = json!("complete");
    }
    // An unpriced model family yields a token-derived cost of None.
    let enter = snapshot_value("S1", 1, "gpt-4o-unpriced");
    let complete = snapshot_value("S1", 5, "gpt-4o-unpriced");
    state["phases"]["flow-code"]["window_at_enter"] = enter;
    state["phases"]["flow-code"]["window_at_complete"] = complete;

    let table = format_cost_table(&state);
    let code_row = table
        .lines()
        .find(|l| l.starts_with("| Code |"))
        .unwrap_or_else(|| panic!("Code row missing; table:\n{}", table));
    assert!(
        code_row.contains("—"),
        "Code row must carry em-dash for unknown cost; row: {:?}",
        code_row
    );
}

/// When any phase contributes `None` cost, the Total's cost cell
/// ends with the `*` partial marker.
#[test]
fn cost_table_appends_partial_marker_in_total() {
    let mut state = make_test_state();
    for key in PHASE_ORDER {
        state["phases"][key]["status"] = json!("complete");
    }
    add_phase_snapshots(&mut state, "flow-start", 0, 5);
    // flow-code uses an unpriced model family → token-derived cost is
    // None → cost_delta_usd None → Total marked partial.
    let enter = snapshot_value("S1", 1, "gpt-4o-unpriced");
    let complete = snapshot_value("S1", 5, "gpt-4o-unpriced");
    state["phases"]["flow-code"]["window_at_enter"] = enter;
    state["phases"]["flow-code"]["window_at_complete"] = complete;

    let table = format_cost_table(&state);
    let total_row = table
        .lines()
        .find(|l| l.contains("**Total**"))
        .expect("Total row");
    assert!(
        total_row.contains("*"),
        "Total row cost cell must carry the partial marker; row: {:?}",
        total_row
    );
}

/// A phase whose snapshot pair shows the 5h pct dropping between
/// enter and complete renders the reset marker `↻` in that row's
/// Cost cell.
#[test]
fn cost_table_appends_reset_marker_per_row() {
    let mut state = make_test_state();
    for key in PHASE_ORDER {
        state["phases"][key]["status"] = json!("complete");
    }
    let mut enter = snapshot_value("S1", 80, "claude-opus-4-7");
    let mut complete = snapshot_value("S1", 5, "claude-opus-4-7");
    enter["session_input_tokens"] = json!(100);
    complete["session_input_tokens"] = json!(500);
    state["phases"]["flow-code"]["window_at_enter"] = enter;
    state["phases"]["flow-code"]["window_at_complete"] = complete;

    let table = format_cost_table(&state);
    let code_row = table
        .lines()
        .find(|l| l.starts_with("| Code |"))
        .expect("Code row");
    assert!(
        code_row.contains("↻"),
        "Code row must carry the reset marker; row: {:?}",
        code_row
    );
}

/// A multi-model breakdown renders a `| Model | Tokens |` sub-table
/// after the per-phase table.
#[test]
fn cost_table_includes_by_model_subtable_when_multi_model() {
    let mut state = make_test_state();
    for key in PHASE_ORDER {
        state["phases"][key]["status"] = json!("complete");
    }
    state["phases"]["flow-code"]["window_at_enter"] = snapshot_value("S1", 0, "claude-opus-4-7");
    let mut complete = snapshot_value("S1", 50, "claude-opus-4-7");
    complete["by_model"]["claude-sonnet-4-6"] = json!({
        "input": 1000, "output": 500, "cache_create": 0, "cache_read": 0
    });
    state["phases"]["flow-code"]["window_at_complete"] = complete;

    let table = format_cost_table(&state);
    assert!(
        table.contains("| Model | Tokens |"),
        "By Model sub-table header missing; table:\n{}",
        table
    );
    assert!(
        table.contains("claude-opus-4-7"),
        "opus row missing in sub-table"
    );
    assert!(
        table.contains("claude-sonnet-4-6"),
        "sonnet row missing in sub-table"
    );
}

/// A single-model breakdown skips the `| Model | Tokens |` sub-table
/// (a one-row breakdown adds no signal).
#[test]
fn cost_table_omits_by_model_subtable_when_single_model() {
    let mut state = make_test_state();
    for key in PHASE_ORDER {
        state["phases"][key]["status"] = json!("complete");
    }
    add_phase_snapshots(&mut state, "flow-code", 0, 5);

    let table = format_cost_table(&state);
    assert!(
        !table.contains("| Model | Tokens |"),
        "single-model breakdown must suppress sub-table; table:\n{}",
        table
    );
}

/// Model names flow from snapshot data (`by_model` keys are
/// session-captured strings). A model name containing the
/// Markdown table delimiter `|` must be escaped or the rendered
/// `| Model | Tokens |` row breaks the table structure — extra
/// pipe characters produce phantom columns and misaligned cells
/// on GitHub. Per `.claude/rules/subprocess-argument-escaping.md`,
/// external strings interpolated into a structural-syntax target
/// must be escaped before interpolation.
#[test]
fn cost_table_escapes_pipe_in_model_name() {
    let mut state = make_test_state();
    for key in PHASE_ORDER {
        state["phases"][key]["status"] = json!("complete");
    }
    state["phases"]["flow-code"]["window_at_enter"] = snapshot_value("S1", 0, "claude-opus-4-7");
    let mut complete = snapshot_value("S1", 50, "claude-opus-4-7");
    // Two models so the by-model sub-table renders; the second
    // model carries a literal `|` in its name.
    complete["by_model"]["claude|injected-evil"] = json!({
        "input": 1000, "output": 500, "cache_create": 0, "cache_read": 0
    });
    state["phases"]["flow-code"]["window_at_complete"] = complete;

    let table = format_cost_table(&state);
    let evil_row = table
        .lines()
        .find(|l| l.contains("injected-evil"))
        .unwrap_or_else(|| panic!("injected model row missing; table:\n{}", table));
    // The escape produces `claude\|injected-evil` — GitHub
    // Markdown's parser treats `\|` inside a table cell as a
    // literal pipe character and ignores it as a column
    // delimiter. The rendered cell shows `claude|injected-evil`.
    assert!(
        evil_row.contains("claude\\|injected-evil"),
        "escaped model name must appear in row; got: {:?}",
        evil_row
    );
    // Count UNESCAPED pipes — those preceded by a backslash do
    // not act as column delimiters. A 2-column markdown table
    // row has exactly 3 unescaped pipes: leading, separator,
    // trailing.
    let bytes = evil_row.as_bytes();
    let mut unescaped_pipes = 0;
    for i in 0..bytes.len() {
        if bytes[i] == b'|' && (i == 0 || bytes[i - 1] != b'\\') {
            unescaped_pipes += 1;
        }
    }
    assert_eq!(
        unescaped_pipes, 3,
        "by-model row must have exactly 3 unescaped pipes; found {} in row {:?}",
        unescaped_pipes, evil_row
    );
}

/// A model name ending in `\` must have the backslash escaped
/// in the Markdown cell — otherwise the following pipe column
/// delimiter would be parsed as an escaped pipe and the cell
/// would absorb the next column's content.
#[test]
fn cost_table_escapes_backslash_in_model_name() {
    let mut state = make_test_state();
    for key in PHASE_ORDER {
        state["phases"][key]["status"] = json!("complete");
    }
    state["phases"]["flow-code"]["window_at_enter"] = snapshot_value("S1", 0, "claude-opus-4-7");
    let mut complete = snapshot_value("S1", 50, "claude-opus-4-7");
    complete["by_model"]["claude\\"] = json!({
        "input": 100, "output": 50, "cache_create": 0, "cache_read": 0
    });
    state["phases"]["flow-code"]["window_at_complete"] = complete;

    let table = format_cost_table(&state);
    // The escape produces `claude\\` (a literal backslash
    // doubled) — GitHub Markdown renders `\\` as a single
    // literal backslash inside a cell.
    assert!(
        table.contains("| claude\\\\ |"),
        "backslash in model name must be escaped as `\\\\`; table:\n{}",
        table
    );
}

/// A model name containing `\n` or `\r` must NOT inject a line
/// break into the Markdown table — newlines inside cells break
/// the table structure on GitHub. The escape collapses them to
/// spaces so the row stays on a single line.
#[test]
fn cost_table_escapes_newline_in_model_name() {
    let mut state = make_test_state();
    for key in PHASE_ORDER {
        state["phases"][key]["status"] = json!("complete");
    }
    state["phases"]["flow-code"]["window_at_enter"] = snapshot_value("S1", 0, "claude-opus-4-7");
    let mut complete = snapshot_value("S1", 50, "claude-opus-4-7");
    complete["by_model"]["claude\nrogue\rmodel"] = json!({
        "input": 100, "output": 50, "cache_create": 0, "cache_read": 0
    });
    state["phases"]["flow-code"]["window_at_complete"] = complete;

    let table = format_cost_table(&state);
    let rogue_row = table
        .lines()
        .find(|l| l.contains("rogue"))
        .unwrap_or_else(|| panic!("rogue-model row missing; table:\n{}", table));
    // After escape, the row must NOT contain any literal newline
    // or carriage return — both should be replaced with spaces.
    assert!(
        !rogue_row.contains('\n'),
        "row must not contain literal newline; row: {:?}",
        rogue_row
    );
    assert!(
        !rogue_row.contains('\r'),
        "row must not contain literal carriage return; row: {:?}",
        rogue_row
    );
    // The model name's three segments should appear as
    // space-separated tokens on the same line.
    assert!(
        rogue_row.contains("claude rogue model"),
        "escaped model name segments must appear on one line; row: {:?}",
        rogue_row
    );
}

/// The Total cost cell is bold-wrapped (`**$X.YYY**`). When
/// `total_partial == true` AND cost is Some, the partial marker
/// `*` must NOT land between the closing `**` and the trailing
/// pipe — that produces `**$X.YYY***`, which GitHub Markdown can
/// parse ambiguously as bold+emphasis. Escape the marker as
/// `\*` and place it AFTER the bold wrapper so the cell renders
/// unambiguously as bold value + literal asterisk.
#[test]
fn cost_table_total_partial_marker_does_not_produce_triple_asterisk() {
    let mut state = make_test_state();
    for key in PHASE_ORDER {
        state["phases"][key]["status"] = json!("complete");
    }
    add_phase_snapshots(&mut state, "flow-start", 0, 5);
    // flow-code uses an unpriced model family → token-derived cost is
    // None → total_partial flips on.
    let enter = snapshot_value("S1", 1, "gpt-4o-unpriced");
    let complete = snapshot_value("S1", 5, "gpt-4o-unpriced");
    state["phases"]["flow-code"]["window_at_enter"] = enter;
    state["phases"]["flow-code"]["window_at_complete"] = complete;

    let table = format_cost_table(&state);
    let total_row = table
        .lines()
        .find(|l| l.contains("**Total**"))
        .expect("Total row");
    assert!(
        !total_row.contains("***"),
        "Total row must not produce three consecutive asterisks; row: {:?}",
        total_row
    );
    // The fix places the partial marker as `\*` after the bold
    // wrapper, so the row contains the escape sequence rather
    // than a bare trailing star inside the bold delimiters.
    assert!(
        total_row.contains("\\*"),
        "Total row must carry the escaped partial marker `\\*` after the bold wrapper; row: {:?}",
        total_row
    );
}

/// A phase whose multi-session snapshot fold produces
/// `Some(cost)` AND `row_partial == true` renders the per-row
/// cost cell as `${:.3}*` — the dollar value with the partial
/// marker suffix. Cost is token-derived: the fold groups by
/// `session_id`, session S1 (enter + step0, a priced opus model)
/// contributes a Some cost delta, and session S2 (step1 +
/// complete, an unpriced model family) contributes a None delta
/// that flips `total_partial` while leaving the running `Some`
/// cost in place.
#[test]
fn cost_table_appends_partial_marker_to_row_when_cost_partial() {
    let mut state = make_test_state();
    for key in PHASE_ORDER {
        state["phases"][key]["status"] = json!("complete");
    }
    let enter = snapshot_value("S1", 1, "claude-opus-4-7");
    let step0 = snapshot_value("S1", 5, "claude-opus-4-7");
    let step1 = snapshot_value("S2", 2, "gpt-4o-unpriced");
    let complete = snapshot_value("S2", 6, "gpt-4o-unpriced");
    state["phases"]["flow-code"]["window_at_enter"] = enter;
    state["phases"]["flow-code"]["window_at_complete"] = complete;
    state["phases"]["flow-code"]["step_snapshots"] = json!([
        {
            "step": 1,
            "field": "code_task",
            "captured_at": step0["captured_at"],
            "session_id": step0["session_id"],
            "model": step0["model"],
            "five_hour_pct": step0["five_hour_pct"],
            "seven_day_pct": step0["seven_day_pct"],
            "session_input_tokens": step0["session_input_tokens"],
            "session_output_tokens": step0["session_output_tokens"],
            "session_cache_creation_tokens": step0["session_cache_creation_tokens"],
            "session_cache_read_tokens": step0["session_cache_read_tokens"],
            "by_model": step0["by_model"],
            "turn_count": step0["turn_count"],
            "tool_call_count": step0["tool_call_count"],
            "context_at_last_turn_tokens": step0["context_at_last_turn_tokens"],
            "context_window_pct": step0["context_window_pct"],
        },
        {
            "step": 2,
            "field": "code_task",
            "captured_at": step1["captured_at"],
            "session_id": step1["session_id"],
            "model": step1["model"],
            "five_hour_pct": step1["five_hour_pct"],
            "seven_day_pct": step1["seven_day_pct"],
            "session_input_tokens": step1["session_input_tokens"],
            "session_output_tokens": step1["session_output_tokens"],
            "session_cache_creation_tokens": step1["session_cache_creation_tokens"],
            "session_cache_read_tokens": step1["session_cache_read_tokens"],
            "by_model": step1["by_model"],
            "turn_count": step1["turn_count"],
            "tool_call_count": step1["tool_call_count"],
            "context_at_last_turn_tokens": step1["context_at_last_turn_tokens"],
            "context_window_pct": step1["context_window_pct"],
        },
    ]);

    let table = format_cost_table(&state);
    let code_row = table
        .lines()
        .find(|l| l.starts_with("| Code |"))
        .unwrap_or_else(|| panic!("Code row missing; table:\n{}", table));
    assert!(
        code_row.contains('$'),
        "Code row must carry $ cost cell when Some(cost); row: {:?}",
        code_row
    );
    assert!(
        code_row.contains('*'),
        "Code row must carry the * partial marker when row_partial; row: {:?}",
        code_row
    );
}

/// Empty phases map → `compute_cost_breakdown` returns None →
/// `format_cost_table` returns an empty string so renderers can omit
/// the section.
#[test]
fn cost_table_returns_empty_when_breakdown_none() {
    let mut state = make_test_state();
    state["phases"] = json!({});
    let table = format_cost_table(&state);
    assert_eq!(table, "", "expected empty string; got: {:?}", table);
}

// --- render_body ---

#[test]
fn minimal_state() {
    let state = make_test_state();
    let dir = tempfile::tempdir().unwrap();

    let body = render_body(&state, dir.path()).unwrap();

    assert!(body.starts_with("## What"));
    assert!(body.contains("## Artifacts"));
    assert!(body.contains("## Phase Timings"));
    assert!(body.contains("## State File"));
    assert!(!body.contains("## Plan\n"));
    assert!(!body.contains("## DAG Analysis"));
    assert!(!body.contains("## Session Log"));
    assert!(!body.contains("## Issues Filed"));
}

#[test]
fn what_uses_prompt() {
    let mut state = make_test_state();
    state["prompt"] = json!("fix login timeout when session expires");

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    assert!(body.contains("fix login timeout when session expires."));
}

#[test]
fn what_raises_on_empty_prompt() {
    let mut state = make_test_state();
    state["prompt"] = json!("");

    let dir = tempfile::tempdir().unwrap();
    let result = render_body(&state, dir.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("missing 'prompt'"));
}

#[test]
fn what_raises_when_no_prompt_key() {
    let mut state = make_test_state();
    state.as_object_mut().unwrap().remove("prompt");

    let dir = tempfile::tempdir().unwrap();
    let result = render_body(&state, dir.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("missing 'prompt'"));
}

#[test]
fn with_plan_only() {
    let mut state = make_test_state();
    let dir = tempfile::tempdir().unwrap();
    let plan_file = dir.path().join("plan.md");
    fs::write(&plan_file, "# My Plan\n\nDo the thing.").unwrap();
    state["plan_file"] = json!(plan_file.to_string_lossy().to_string());

    let body = render_body(&state, dir.path()).unwrap();

    assert!(body.contains("## Plan"));
    assert!(body.contains("Do the thing."));
    assert!(!body.contains("## DAG Analysis"));
}

#[test]
fn with_plan_and_dag() {
    let mut state = make_test_state();
    let dir = tempfile::tempdir().unwrap();
    let plan_file = dir.path().join("plan.md");
    fs::write(&plan_file, "# Plan content").unwrap();
    let dag_file = dir.path().join("dag.md");
    fs::write(&dag_file, "# DAG content").unwrap();
    state["plan_file"] = json!(plan_file.to_string_lossy().to_string());
    state["dag_file"] = json!(dag_file.to_string_lossy().to_string());

    let body = render_body(&state, dir.path()).unwrap();

    assert!(body.contains("## Plan"));
    assert!(body.contains("## DAG Analysis"));
    assert!(body.contains("Plan content"));
    assert!(body.contains("DAG content"));
}

#[test]
fn dag_always_text_format() {
    let mut state = make_test_state();
    let dir = tempfile::tempdir().unwrap();
    let dag_file = dir.path().join("dag.md");
    fs::write(&dag_file, r#"<dag goal="test"><node id="1"/></dag>"#).unwrap();
    state["dag_file"] = json!(dag_file.to_string_lossy().to_string());

    let body = render_body(&state, dir.path()).unwrap();

    assert!(body.contains("```text"));
    assert!(!body.contains("```xml"));
    assert!(body.contains(r#"<dag goal="test">"#));
}

#[test]
fn nested_fences_preserve_subsequent_sections() {
    let mut state = make_test_state();
    let dir = tempfile::tempdir().unwrap();
    let dag_file = dir.path().join("dag.md");
    fs::write(
        &dag_file,
        "# DAG Analysis\n\n```xml\n<dag goal='test'><node id='1'/></dag>\n```\n\n```python\nprint('hello')\n```",
    ).unwrap();
    state["dag_file"] = json!(dag_file.to_string_lossy().to_string());

    let body = render_body(&state, dir.path()).unwrap();

    assert!(body.contains("## Phase Timings"));
    assert!(body.contains("## State File"));
    let dag_start = body.find("## DAG Analysis").unwrap();
    let dag_section = &body[dag_start..];
    assert!(dag_section.contains("````"));
}

#[test]
fn with_transcript() {
    let mut state = make_test_state();
    state["transcript_path"] = json!("/path/to/session.jsonl");

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    assert!(body.contains("| Transcript |"));
    assert!(body.contains("/path/to/session.jsonl"));
}

#[test]
fn full_state() {
    let mut state = make_test_state();
    let dir = tempfile::tempdir().unwrap();

    for key in PHASE_ORDER {
        state["phases"][key]["status"] = json!("complete");
        state["phases"][key]["started_at"] = json!("2026-01-01T00:00:00Z");
        state["phases"][key]["cumulative_seconds"] = json!(60);
    }
    state["current_phase"] = json!("flow-complete");

    let plan_file = dir.path().join("plan.md");
    fs::write(&plan_file, "Plan content").unwrap();
    let dag_file = dir.path().join("dag.md");
    fs::write(&dag_file, "DAG content").unwrap();
    let branch_dir = dir.path().join(".flow-states").join("test-feature");
    fs::create_dir_all(&branch_dir).unwrap();
    let log_file = branch_dir.join("log");
    fs::write(&log_file, "2026-01-01 [Phase 1] Step 1 — done").unwrap();

    state["plan_file"] = json!(plan_file.to_string_lossy().to_string());
    state["dag_file"] = json!(dag_file.to_string_lossy().to_string());
    state["transcript_path"] = json!("/path/to/session.jsonl");
    state["issues_filed"] = json!([{
        "label": "Tech Debt",
        "title": "Test issue",
        "url": "https://github.com/test/test/issues/1",
        "phase_name": "Learn"
    }]);

    let body = render_body(&state, dir.path()).unwrap();

    assert!(body.contains("## What"));
    assert!(body.contains("## Artifacts"));
    assert!(body.contains("## Plan"));
    assert!(body.contains("## DAG Analysis"));
    assert!(body.contains("## Phase Timings"));
    assert!(body.contains("## State File"));
    assert!(body.contains("## Session Log"));
    assert!(body.contains("## Issues Filed"));
}

#[test]
fn with_issues() {
    let mut state = make_test_state();
    state["issues_filed"] = json!([{
        "label": "Rule",
        "title": "Add rule X",
        "url": "https://github.com/test/test/issues/5",
        "phase_name": "Learn"
    }]);

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    assert!(body.contains("## Issues Filed"));
    assert!(body.contains("Add rule X"));
}

#[test]
fn plan_from_files_block() {
    let mut state = make_test_state();
    let dir = tempfile::tempdir().unwrap();
    let branch_dir = dir.path().join(".flow-states").join("test-feature");
    fs::create_dir_all(&branch_dir).unwrap();
    let plan_file = branch_dir.join("plan.md");
    fs::write(&plan_file, "# Plan from files block").unwrap();
    state["files"]["plan"] = json!(".flow-states/test-feature/plan.md");

    let body = render_body(&state, dir.path()).unwrap();

    assert!(body.contains("## Plan"));
    assert!(body.contains("Plan from files block"));
}

#[test]
fn dag_from_files_block() {
    let mut state = make_test_state();
    let dir = tempfile::tempdir().unwrap();
    let branch_dir = dir.path().join(".flow-states").join("test-feature");
    fs::create_dir_all(&branch_dir).unwrap();
    let dag_file = branch_dir.join("dag.md");
    fs::write(&dag_file, "# DAG from files block").unwrap();
    state["files"]["dag"] = json!(".flow-states/test-feature/dag.md");

    let body = render_body(&state, dir.path()).unwrap();

    assert!(body.contains("## DAG Analysis"));
    assert!(body.contains("DAG from files block"));
}

#[test]
fn artifacts_table_from_files_block() {
    let mut state = make_test_state();
    state["files"]["plan"] = json!(".flow-states/test-feature/plan.md");
    state["files"]["dag"] = json!(".flow-states/test-feature/dag.md");

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    assert!(body.contains("| File | Path |"));
    assert!(body.contains(".flow-states/test-feature/plan.md"));
    assert!(body.contains(".flow-states/test-feature/dag.md"));
    assert!(body.contains(".flow-states/test-feature/log"));
    assert!(body.contains(".flow-states/test-feature/state.json"));
}

#[test]
fn legacy_artifacts_without_files_block() {
    let mut state = make_test_state();
    state.as_object_mut().unwrap().remove("files");
    state["plan_file"] = json!("/abs/path/to/plan.md");
    state["dag_file"] = json!("/abs/path/to/dag.md");
    state["transcript_path"] = json!("/abs/path/to/session.jsonl");

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    assert!(body.contains("**Plan file**"));
    assert!(body.contains("**DAG file**"));
    assert!(body.contains("**Session log**"));
    assert!(!body.contains("| File | Path |"));
}

#[test]
fn empty_artifacts_no_files_block() {
    let mut state = make_test_state();
    state.as_object_mut().unwrap().remove("files");

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    assert!(body.contains("## Artifacts\n\n## Phase"));
}

#[test]
fn missing_plan_file() {
    let mut state = make_test_state();
    let dir = tempfile::tempdir().unwrap();
    state["plan_file"] = json!(dir
        .path()
        .join("nonexistent-plan.md")
        .to_string_lossy()
        .to_string());

    let body = render_body(&state, dir.path()).unwrap();
    let has_plan_section = body.contains("## Plan\n\n<details>");
    assert!(!has_plan_section);
}

#[test]
fn missing_dag_file() {
    let mut state = make_test_state();
    let dir = tempfile::tempdir().unwrap();
    state["dag_file"] = json!(dir
        .path()
        .join("nonexistent-dag.md")
        .to_string_lossy()
        .to_string());

    let body = render_body(&state, dir.path()).unwrap();
    assert!(!body.contains("## DAG Analysis"));
}

#[test]
fn idempotent() {
    let mut state = make_test_state();
    let dir = tempfile::tempdir().unwrap();
    let plan_file = dir.path().join("plan.md");
    fs::write(&plan_file, "Plan content").unwrap();
    state["plan_file"] = json!(plan_file.to_string_lossy().to_string());

    let body1 = render_body(&state, dir.path()).unwrap();
    let body2 = render_body(&state, dir.path()).unwrap();

    assert_eq!(body1, body2);
}

#[test]
fn phase_timings_shows_started_only() {
    let mut state = make_test_state();
    state["phases"]["flow-start"]["cumulative_seconds"] = json!(30);
    state["phases"]["flow-code"]["status"] = json!("complete");
    state["phases"]["flow-code"]["started_at"] = json!("2026-01-01T00:01:00Z");
    state["phases"]["flow-code"]["cumulative_seconds"] = json!(300);
    state["phases"]["flow-review"]["status"] = json!("in_progress");
    state["phases"]["flow-review"]["started_at"] = json!("2026-01-01T00:06:00Z");

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    assert!(body.contains("| Start |"));
    assert!(body.contains("| Code |"));
    assert!(body.contains("| Review |"));
    assert!(!body.contains("| Learn |"));
    assert!(!body.contains("| Learn |"));
    let timings_start = body.find("## Phase Timings").unwrap();
    let timings_end = body.find("<!-- end:Phase Timings -->").unwrap();
    let timings_section = &body[timings_start..timings_end];
    assert!(!timings_section.contains("| Complete |"));
}

#[test]
fn section_order() {
    let mut state = make_test_state();
    let dir = tempfile::tempdir().unwrap();

    for key in PHASE_ORDER {
        state["phases"][key]["status"] = json!("complete");
        state["phases"][key]["started_at"] = json!("2026-01-01T00:00:00Z");
        state["phases"][key]["cumulative_seconds"] = json!(60);
    }
    state["current_phase"] = json!("flow-complete");

    let plan_file = dir.path().join("plan.md");
    fs::write(&plan_file, "Plan").unwrap();
    let dag_file = dir.path().join("dag.md");
    fs::write(&dag_file, "DAG").unwrap();
    let branch_log_dir = dir.path().join(".flow-states").join("test-feature");
    fs::create_dir_all(&branch_log_dir).unwrap();
    fs::write(branch_log_dir.join("log"), "log entry").unwrap();
    state["plan_file"] = json!(plan_file.to_string_lossy().to_string());
    state["dag_file"] = json!(dag_file.to_string_lossy().to_string());
    state["transcript_path"] = json!("/path/to/session.jsonl");
    state["issues_filed"] = json!([{
        "label": "Tech Debt",
        "title": "Issue",
        "url": "https://github.com/t/t/issues/1",
        "phase_name": "Learn"
    }]);

    let body = render_body(&state, dir.path()).unwrap();

    let headings = [
        "## What",
        "## Artifacts",
        "## Plan",
        "## DAG Analysis",
        "## Phase Timings",
        "## State File",
        "## Session Log",
        "## Issues Filed",
    ];
    let positions: Vec<usize> = headings.iter().map(|h| body.find(h).unwrap()).collect();
    let mut sorted = positions.clone();
    sorted.sort();
    assert_eq!(positions, sorted, "Sections out of order");
}

// --- render_body Token Cost section integration ---

/// Full state with snapshots → render_body splices a `## Token Cost`
/// section between `## Phase Timings` and `## State File`.
#[test]
fn render_body_includes_token_cost_section_with_cost_data() {
    let state = full_cost_state();
    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    assert!(
        body.contains("## Token Cost"),
        "Token Cost section header must appear; body:\n{}",
        body
    );
    let token_pos = body
        .find("## Token Cost")
        .expect("Token Cost section position");
    let timings_pos = body
        .find("## Phase Timings")
        .expect("Phase Timings position");
    let state_pos = body.find("## State File").expect("State File position");
    assert!(
        timings_pos < token_pos && token_pos < state_pos,
        "Token Cost must sit between Phase Timings and State File"
    );
}

/// State without window snapshots → format_cost_table returns empty
/// → render_body omits the section. Other sections still render in
/// the canonical order.
#[test]
fn render_body_omits_token_cost_section_when_no_data() {
    let mut state = make_test_state();
    state["phases"] = json!({});
    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    assert!(
        !body.contains("## Token Cost"),
        "Token Cost section must NOT appear when no snapshot data; body:\n{}",
        body
    );
    let what_pos = body.find("## What").expect("What position");
    let artifacts_pos = body.find("## Artifacts").expect("Artifacts position");
    let timings_pos = body
        .find("## Phase Timings")
        .expect("Phase Timings position");
    let state_pos = body.find("## State File").expect("State File position");
    assert!(
        what_pos < artifacts_pos && artifacts_pos < timings_pos && timings_pos < state_pos,
        "core sections must remain in canonical order even without Token Cost"
    );
}

/// All eight optional sections rendered together: their `## `
/// heading positions must follow the canonical order
/// What < Artifacts < Plan < DAG Analysis < Phase Timings <
/// Token Cost < State File < Session Log < Issues Filed.
#[test]
fn render_body_token_cost_section_order_invariant() {
    let mut state = full_cost_state();
    let dir = tempfile::tempdir().unwrap();

    let plan_file = dir.path().join("plan.md");
    fs::write(&plan_file, "Plan").unwrap();
    let dag_file = dir.path().join("dag.md");
    fs::write(&dag_file, "DAG").unwrap();
    let branch_log_dir = dir.path().join(".flow-states").join("test-feature");
    fs::create_dir_all(&branch_log_dir).unwrap();
    fs::write(branch_log_dir.join("log"), "log entry").unwrap();
    state["plan_file"] = json!(plan_file.to_string_lossy().to_string());
    state["dag_file"] = json!(dag_file.to_string_lossy().to_string());
    state["transcript_path"] = json!("/path/to/session.jsonl");
    state["issues_filed"] = json!([{
        "label": "Tech Debt",
        "title": "Issue",
        "url": "https://github.com/t/t/issues/1",
        "phase_name": "Learn"
    }]);

    let body = render_body(&state, dir.path()).unwrap();

    let headings = [
        "## What",
        "## Artifacts",
        "## Plan",
        "## DAG Analysis",
        "## Phase Timings",
        "## Token Cost",
        "## State File",
        "## Session Log",
        "## Issues Filed",
    ];
    let positions: Vec<usize> = headings
        .iter()
        .map(|h| {
            body.find(h)
                .unwrap_or_else(|| panic!("missing heading {} in body:\n{}", h, body))
        })
        .collect();
    let mut sorted = positions.clone();
    sorted.sort();
    assert_eq!(
        positions, sorted,
        "Token Cost out of order; body:\n{}",
        body
    );
}

/// The Token Cost section is rendered as a plain section (`## Token
/// Cost\n\n<table>`), not wrapped in a `<details>` collapsible block.
#[test]
fn render_body_token_cost_uses_plain_section_format() {
    let state = full_cost_state();
    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    let token_pos = body
        .find("## Token Cost")
        .expect("Token Cost section position");
    let after_token = &body[token_pos..];
    let next_section = after_token[1..]
        .find("\n## ")
        .map(|i| i + 1)
        .unwrap_or(after_token.len());
    let token_section = &after_token[..next_section];

    assert!(
        !token_section.contains("<details>"),
        "Token Cost section must be plain markdown, not wrapped in <details>; section:\n{}",
        token_section
    );
    assert!(
        token_section.starts_with("## Token Cost\n\n"),
        "Token Cost section must start with `## Token Cost\\n\\n`; section starts:\n{:?}",
        &token_section[..token_section.len().min(80)]
    );
}

#[test]
fn no_issues_no_section() {
    let mut state = make_test_state();
    state["issues_filed"] = json!([]);

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    assert!(!body.contains("## Issues Filed"));
}

#[test]
fn what_section_includes_closing_keywords() {
    let mut state = make_test_state();
    state["prompt"] = json!("work on issue #643");

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    assert!(body.contains("work on issue #643."));
    assert!(body.contains("Closes #643"));
}

#[test]
fn what_section_no_closing_keywords_without_issues() {
    let mut state = make_test_state();
    state["prompt"] = json!("add dark mode toggle");

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    assert!(body.contains("add dark mode toggle."));
    assert!(!body.contains("Closes"));
}

#[test]
fn what_section_multiple_closing_keywords() {
    let mut state = make_test_state();
    state["prompt"] = json!("fix #1 and #2");

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    assert!(body.contains("fix #1 and #2."));
    assert!(body.contains("Closes #1"));
    assert!(body.contains("Closes #2"));
}

#[test]
fn what_section_no_double_period() {
    let mut state = make_test_state();
    state["prompt"] = json!("Fix the login timeout bug.");

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    assert!(body.contains("Fix the login timeout bug."));
    assert!(!body.contains("Fix the login timeout bug.."));
}

/// Covers the "transcript_path is Some but empty string" branch in
/// the files-block path of build_artifacts (line skipping the empty
/// transcript row).
#[test]
fn artifacts_files_block_empty_transcript_skipped() {
    let mut state = make_test_state();
    state["files"]["plan"] = json!(".flow-states/test-feature/plan.md");
    state["transcript_path"] = json!("");

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();
    assert!(!body.contains("| Transcript |"));
}

/// Covers "dag_file is empty string" in legacy path.
#[test]
fn artifacts_legacy_empty_dag_file_skipped() {
    let mut state = make_test_state();
    state.as_object_mut().unwrap().remove("files");
    state["plan_file"] = json!("/abs/plan.md");
    state["dag_file"] = json!("");

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();
    assert!(body.contains("**Plan file**"));
    assert!(!body.contains("**DAG file**"));
}

/// Covers "transcript_path is empty string" in legacy path.
#[test]
fn artifacts_legacy_empty_transcript_skipped() {
    let mut state = make_test_state();
    state.as_object_mut().unwrap().remove("files");
    state["plan_file"] = json!("/abs/plan.md");
    state["transcript_path"] = json!("");

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();
    assert!(!body.contains("**Session log**"));
}

/// Covers the `.map_err(|e| e.to_string())` closure on the plan-file
/// read — pointing plan_file at a directory makes `pp.exists()`
/// return true but `read_to_string(pp)` return Err (EISDIR).
#[test]
fn plan_file_as_directory_propagates_error() {
    let mut state = make_test_state();
    let dir = tempfile::tempdir().unwrap();
    let plan_as_dir = dir.path().join("plan-dir");
    fs::create_dir(&plan_as_dir).unwrap();
    state["plan_file"] = json!(plan_as_dir.to_string_lossy().to_string());

    let result = render_body(&state, dir.path());
    assert!(result.is_err());
}

/// Same as above but for the DAG file read.
#[test]
fn dag_file_as_directory_propagates_error() {
    let mut state = make_test_state();
    let dir = tempfile::tempdir().unwrap();
    let dag_as_dir = dir.path().join("dag-dir");
    fs::create_dir(&dag_as_dir).unwrap();
    state["dag_file"] = json!(dag_as_dir.to_string_lossy().to_string());

    let result = render_body(&state, dir.path());
    assert!(result.is_err());
}

/// Same for the session-log file read (absolute path, file slot).
#[test]
fn session_log_as_directory_propagates_error() {
    let mut state = make_test_state();
    let dir = tempfile::tempdir().unwrap();
    let log_as_dir = dir.path().join("log-dir");
    fs::create_dir(&log_as_dir).unwrap();
    state["files"]["log"] = json!(log_as_dir.to_string_lossy().to_string());

    let result = render_body(&state, dir.path());
    assert!(result.is_err());
}

// --- render_body Findings sections integration ---
//
// `render_body` calls `format_findings_markdown` twice (once per phase)
// and splices the resulting `## Review Findings` and `## Learn Findings`
// sections between the Token Cost block (or Phase Timings when Token
// Cost is absent) and the State File block.

/// Helper: build a fixture with snapshot data so Token Cost renders,
/// plus a configurable findings array.
fn full_cost_state_with_findings(findings: Value) -> Value {
    let mut state = full_cost_state();
    state["findings"] = findings;
    state
}

#[test]
fn render_body_omits_findings_sections_when_findings_empty() {
    let state = full_cost_state_with_findings(json!([]));
    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    assert!(
        !body.contains("## Review Findings"),
        "Review Findings must NOT appear when findings empty; body:\n{}",
        body
    );
    assert!(
        !body.contains("## Learn Findings"),
        "Learn Findings must NOT appear when findings empty; body:\n{}",
        body
    );
}

#[test]
fn render_body_renders_review_findings_when_only_review_present() {
    let state = full_cost_state_with_findings(json!([
        {
            "finding": "Missing null check",
            "reason": "Could panic on malformed input",
            "outcome": "fixed",
            "phase": "flow-review",
        }
    ]));
    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    assert!(
        body.contains("## Review Findings"),
        "Review Findings section must appear; body:\n{}",
        body
    );
    assert!(
        !body.contains("## Learn Findings"),
        "Learn Findings must NOT appear when no learn findings; body:\n{}",
        body
    );

    let cost_pos = body
        .find("## Token Cost")
        .expect("Token Cost section position");
    let review_pos = body
        .find("## Review Findings")
        .expect("Review Findings section position");
    let state_pos = body.find("## State File").expect("State File position");
    assert!(
        cost_pos < review_pos && review_pos < state_pos,
        "Review Findings must sit between Token Cost and State File; body:\n{}",
        body
    );
}

#[test]
fn render_body_renders_learn_findings_when_only_learn_present() {
    let state = full_cost_state_with_findings(json!([
        {
            "finding": "Missing rule for X",
            "reason": "Identified during analysis",
            "outcome": "rule_written",
            "phase": "flow-learn",
        }
    ]));
    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    assert!(
        body.contains("## Learn Findings"),
        "Learn Findings section must appear; body:\n{}",
        body
    );
    assert!(
        !body.contains("## Review Findings"),
        "Review Findings must NOT appear when no review findings; body:\n{}",
        body
    );

    let cost_pos = body
        .find("## Token Cost")
        .expect("Token Cost section position");
    let learn_pos = body
        .find("## Learn Findings")
        .expect("Learn Findings section position");
    let state_pos = body.find("## State File").expect("State File position");
    assert!(
        cost_pos < learn_pos && learn_pos < state_pos,
        "Learn Findings must sit between Token Cost and State File; body:\n{}",
        body
    );
}

#[test]
fn render_body_renders_both_sections_in_order_when_both_present() {
    let state = full_cost_state_with_findings(json!([
        {
            "finding": "Bug in parser",
            "reason": "Fixed inline",
            "outcome": "fixed",
            "phase": "flow-review",
        },
        {
            "finding": "Missing rule",
            "reason": "Created new rule",
            "outcome": "rule_written",
            "phase": "flow-learn",
        },
    ]));
    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    let cost_pos = body
        .find("## Token Cost")
        .expect("Token Cost section position");
    let review_pos = body
        .find("## Review Findings")
        .expect("Review Findings section position");
    let learn_pos = body
        .find("## Learn Findings")
        .expect("Learn Findings section position");
    let state_pos = body.find("## State File").expect("State File position");

    assert!(
        cost_pos < review_pos && review_pos < learn_pos && learn_pos < state_pos,
        "Order must be Token Cost < Review Findings < Learn Findings < State File; body:\n{}",
        body
    );
}

#[test]
fn render_body_renders_findings_after_phase_timings_when_token_cost_absent() {
    // No phase carries window snapshots → format_cost_table returns
    // empty → Token Cost section is omitted. Findings must then
    // anchor against Phase Timings on the upper side instead.
    let mut state = make_test_state();
    state["phases"] = json!({});
    state["findings"] = json!([
        {
            "finding": "Bug found",
            "reason": "Fixed inline",
            "outcome": "fixed",
            "phase": "flow-review",
        }
    ]);
    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    assert!(
        !body.contains("## Token Cost"),
        "Token Cost must be absent when no snapshot data; body:\n{}",
        body
    );
    let timings_pos = body
        .find("## Phase Timings")
        .expect("Phase Timings position");
    let review_pos = body
        .find("## Review Findings")
        .expect("Review Findings position");
    let state_pos = body.find("## State File").expect("State File position");
    assert!(
        timings_pos < review_pos && review_pos < state_pos,
        "Review Findings must sit between Phase Timings and State File when Token Cost is absent; body:\n{}",
        body
    );
}

// --- CLI subprocess tests for run_impl_main ---

#[test]
fn render_pr_body_dry_run_returns_sections() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = minimal_complete_state("Test feature");
    let state_path = write_state(&repo, "test-branch", &state);
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_render(
        &repo,
        &[
            "--pr",
            "42",
            "--state-file",
            state_path.to_str().unwrap(),
            "--dry-run",
        ],
        &stub_dir,
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    let sections = data["sections"].as_array().unwrap();
    assert!(!sections.is_empty(), "Expected section headers, got empty");
}

#[test]
fn render_pr_body_missing_state_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");
    let missing = dir.path().join("no-such.json");

    let output = run_render(
        &repo,
        &[
            "--pr",
            "42",
            "--state-file",
            missing.to_str().unwrap(),
            "--dry-run",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("State file not found"));
}

#[test]
fn render_pr_body_malformed_state_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    let path = state_dir.join("bad.json");
    fs::write(&path, "not json").unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_render(
        &repo,
        &[
            "--pr",
            "42",
            "--state-file",
            path.to_str().unwrap(),
            "--dry-run",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
}

#[test]
fn render_pr_body_render_error_on_missing_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let mut state = minimal_complete_state("My feature");
    state.as_object_mut().unwrap().remove("prompt");
    let state_path = write_state(&repo, "test-branch", &state);
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_render(
        &repo,
        &[
            "--pr",
            "42",
            "--state-file",
            state_path.to_str().unwrap(),
            "--dry-run",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("missing 'prompt'"));
}

#[test]
fn render_pr_body_non_dry_run_calls_gh_edit() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = minimal_complete_state("Live render");
    let state_path = write_state(&repo, "test-branch", &state);
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_render(
        &repo,
        &["--pr", "42", "--state-file", state_path.to_str().unwrap()],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
}

/// Covers the `read_to_string` Err path in run_impl_main: the path
/// exists but can't be read. A directory at the state-file path
/// passes `.exists()` and then fails `read_to_string`.
#[test]
fn render_pr_body_read_error_reports_io_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state_dir = repo.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
    // Create a DIRECTORY at the state-file path. exists() returns
    // true so run_impl_main proceeds to read_to_string, which then
    // fails with an I/O error (EISDIR on Linux, similar on macOS).
    let state_path = state_dir.join("test-branch.json");
    fs::create_dir(&state_path).unwrap();
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let output = run_render(
        &repo,
        &[
            "--pr",
            "42",
            "--state-file",
            state_path.to_str().unwrap(),
            "--dry-run",
        ],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
}

/// Exercises the else branch of `if let Some(ref sf) = args.state_file`
/// inside run_impl_main — no `--state-file` CLI flag, so state path is
/// auto-derived from `FLOW_SIMULATE_BRANCH` + `project_root()`.
#[test]
fn render_pr_body_auto_detects_state_file_when_no_flag() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = minimal_complete_state("Auto-detect feature");
    write_state(&repo, "auto-feature", &state);
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("render-pr-body")
        .args(["--pr", "42", "--dry-run"])
        .current_dir(&repo)
        .env("PATH", &path_env)
        .env("FLOW_SIMULATE_BRANCH", "auto-feature")
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
}

/// A git branch with a `/` (e.g. `feature/foo`, `dependabot/...`)
/// is a legitimate git branch name but fails
/// `FlowPaths::is_valid_branch`. The else branch of `if let Some(ref
/// sf) = args.state_file` constructs the state path from
/// `current_branch()` output, which can carry slashes. Treat that
/// case as "state file not found" rather than panicking — the
/// caller sees a structured error envelope instead of a Rust
/// backtrace.
#[test]
fn render_pr_body_does_not_panic_on_slash_branch() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 0\n");

    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("render-pr-body")
        .args(["--pr", "42", "--dry-run"])
        .current_dir(&repo)
        .env("PATH", &path_env)
        .env("FLOW_SIMULATE_BRANCH", "feature/foo")
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked at"),
        "render-pr-body panicked on slash branch; stderr: {}",
        stderr
    );
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr);
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .contains("State file not found"),
        "expected State-file-not-found error, got: {:?}",
        data
    );
}

/// Covers `resolve_path` empty-string short-circuit (returns None
/// instead of treating the empty string as a path). Driven via
/// `render_body` with an empty `plan_file` value.
#[test]
fn resolve_path_empty_string_treated_as_none() {
    let mut state = make_test_state();
    state.as_object_mut().unwrap().remove("files");
    state["plan_file"] = json!("");

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();
    // Empty plan_file shouldn't produce a Plan section.
    assert!(!body.contains("## Plan\n\n<details>"));
}

/// Covers the `files block present but every path empty/null` branch
/// in `build_artifacts`, which short-circuits to `return vec![]`.
#[test]
fn artifacts_files_block_all_empty_returns_empty() {
    let mut state = make_test_state();
    state["files"] = json!({
        "plan": "",
        "dag": "",
        "log": "",
        "state": ""
    });
    // Also null out transcript so the block stays empty.
    state["transcript_path"] = json!(null);

    let dir = tempfile::tempdir().unwrap();
    let body = render_body(&state, dir.path()).unwrap();

    // An empty files block + no legacy plan/dag keys produces a bare
    // "## Artifacts" section with no body.
    assert!(body.contains("## Artifacts\n\n## Phase"));
    assert!(!body.contains("| File | Path |"));
}

#[test]
fn render_pr_body_gh_edit_failure_reports_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let state = minimal_complete_state("Failing edit");
    let state_path = write_state(&repo, "test-branch", &state);
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\necho 'edit denied' >&2\nexit 1\n");

    let output = run_render(
        &repo,
        &["--pr", "42", "--state-file", state_path.to_str().unwrap()],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(data["message"]
        .as_str()
        .unwrap_or("")
        .contains("edit denied"));
}
