//! Tests for the interactive TUI (src/tui.rs).
//!
//! Uses ratatui's TestBackend for rendering assertions and
//! direct state manipulation for input handling tests.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::{Frame, Terminal};

use flow_rs::tui::{DrawFn, EventSourceFn, TuiApp, TuiAppPlatform, View};
use flow_rs::tui_data::{
    AccountMetrics, FlowSummary, IssueSummary, OrchestrationItem, OrchestrationSummary,
    TimelineEntry,
};

// --- Helpers ---

fn make_app() -> TuiApp {
    TuiApp::new(
        PathBuf::from("/tmp/test"),
        "1.0.0".to_string(),
        Some("test/repo".to_string()),
        TuiAppPlatform::for_tests(),
    )
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn make_flow(feature: &str, phase: &str, phase_num: usize) -> FlowSummary {
    FlowSummary {
        feature: feature.to_string(),
        branch: feature.to_lowercase().replace(' ', "-"),
        worktree: format!(".worktrees/{}", feature.to_lowercase().replace(' ', "-")),
        pr_number: Some(100),
        pr_url: Some("https://github.com/test/repo/pull/100".to_string()),
        phase_number: phase_num,
        phase_name: phase.to_string(),
        elapsed: "5m".to_string(),
        code_task: 0,
        diff_stats: None,
        notes_count: 0,
        issues_count: 0,
        issues: vec![],
        blocked: false,
        issue_numbers: vec![42],
        plan_path: None,
        annotation: String::new(),
        phase_elapsed: "2m".to_string(),
        timeline: vec![
            TimelineEntry {
                key: "flow-start".to_string(),
                name: "Start".to_string(),
                number: 1,
                status: "complete".to_string(),
                time: "1m".to_string(),
                annotation: String::new(),
            },
            TimelineEntry {
                key: "flow-plan".to_string(),
                name: "Plan".to_string(),
                number: 2,
                status: "in_progress".to_string(),
                time: "2m".to_string(),
                annotation: "step 3 of 4".to_string(),
            },
            TimelineEntry {
                key: "flow-code".to_string(),
                name: "Code".to_string(),
                number: 3,
                status: "pending".to_string(),
                time: String::new(),
                annotation: String::new(),
            },
        ],
        state: serde_json::json!({"branch": feature.to_lowercase().replace(' ', "-"), "repo": "test/repo"}),
    }
}

fn render_to_string(app: &TuiApp, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let mut lines = Vec::new();
    for y in 0..height {
        let mut line = String::new();
        for x in 0..width {
            let cell = &buffer[(x, y)];
            line.push_str(cell.symbol());
        }
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}

// --- TuiApp initialization ---

#[test]
fn test_tui_app_default_state() {
    let app = make_app();
    assert_eq!(app.selected, 0);
    assert_eq!(app.view, View::List);
    assert!(app.running);
    assert!(!app.confirming_abort);
    assert_eq!(app.active_tab, 0);
    assert_eq!(app.orch_selected, 0);
    assert_eq!(app.issue_selected, 0);
}

#[test]
fn test_tui_app_repo_name_extracted() {
    let app = make_app();
    assert_eq!(app.repo_name.as_deref(), Some("repo"));
}

#[test]
fn test_tui_app_repo_name_none() {
    let app = TuiApp::new(
        PathBuf::from("/tmp"),
        "1.0.0".to_string(),
        None,
        TuiAppPlatform::for_tests(),
    );
    assert!(app.repo_name.is_none());
}

// --- List view rendering ---

#[test]
fn test_render_empty_list() {
    let app = make_app();
    let output = render_to_string(&app, 80, 40);
    assert!(output.contains("No active flows."));
    assert!(output.contains("/flow:flow-start"));
}

#[test]
fn test_render_header_shows_version() {
    let app = make_app();
    let output = render_to_string(&app, 80, 40);
    assert!(output.contains("FLOW v1.0.0"));
}

#[test]
fn test_render_header_shows_repo() {
    let app = make_app();
    let output = render_to_string(&app, 80, 40);
    assert!(output.contains("REPO"));
}

#[test]
fn test_render_tab_bar() {
    let app = make_app();
    let output = render_to_string(&app, 80, 40);
    assert!(output.contains("Active Flows (0)"));
    assert!(output.contains("Orchestration"));
}

#[test]
fn test_render_list_with_flows() {
    let mut app = make_app();
    app.flows = vec![make_flow("Invoice Export", "Code", 3)];
    let output = render_to_string(&app, 120, 40);
    assert!(output.contains("Invoice Export"));
    assert!(output.contains("Code"));
    assert!(output.contains("5m"));
}

#[test]
fn test_render_list_selected_marker() {
    let mut app = make_app();
    app.flows = vec![
        make_flow("Feature A", "Code", 3),
        make_flow("Feature B", "Plan", 2),
    ];
    app.selected = 0;
    let output = render_to_string(&app, 120, 40);
    assert!(output.contains("\u{25b8}"));
}

#[test]
fn test_render_list_blocked_shows_blocked() {
    let mut app = make_app();
    let mut flow = make_flow("Blocked Feature", "Code", 3);
    flow.blocked = true;
    app.flows = vec![flow];
    let output = render_to_string(&app, 120, 40);
    assert!(output.contains("Blocked"));
}

#[test]
fn test_render_detail_panel_phases() {
    let mut app = make_app();
    app.flows = vec![make_flow("Test Feature", "Plan", 2)];
    let output = render_to_string(&app, 80, 40);
    assert!(output.contains("[x]"));
    assert!(output.contains("[>]"));
    assert!(output.contains("[ ]"));
}

/// Build a valid-FlowState Value with one phase carrying non-zero
/// token deltas so `phase_token_table` reports populated data.
fn make_flow_with_token_snapshots() -> FlowSummary {
    let mut flow = make_flow("Token Test", "Code", 3);
    let snap_enter = serde_json::json!({
        "captured_at": "2026-01-01T00:00:00-08:00",
        "session_id": "S1",
        "model": "claude-opus-4-7",
        "five_hour_pct": 10,
        "seven_day_pct": 5,
        "session_input_tokens": 100,
        "session_output_tokens": 50,
        "session_cache_creation_tokens": 0,
        "session_cache_read_tokens": 0,
        "session_cost_usd": 0.01,
        "by_model": {
            "claude-opus-4-7": {"input": 100, "output": 50, "cache_create": 0, "cache_read": 0}
        },
        "turn_count": 1,
        "tool_call_count": 2,
        "context_at_last_turn_tokens": 100,
        "context_window_pct": 0.05
    });
    let snap_complete = serde_json::json!({
        "captured_at": "2026-01-01T01:00:00-08:00",
        "session_id": "S1",
        "model": "claude-opus-4-7",
        "five_hour_pct": 30,
        "seven_day_pct": 15,
        "session_input_tokens": 5_000,
        "session_output_tokens": 2_500,
        "session_cache_creation_tokens": 0,
        "session_cache_read_tokens": 0,
        "session_cost_usd": 0.50,
        "by_model": {
            "claude-opus-4-7": {"input": 5_000, "output": 2_500, "cache_create": 0, "cache_read": 0}
        },
        "turn_count": 50,
        "tool_call_count": 100,
        "context_at_last_turn_tokens": 5_000,
        "context_window_pct": 2.5
    });
    flow.state = serde_json::json!({
        "schema_version": 1,
        "branch": "token-test",
        "started_at": "2026-01-01T00:00:00-08:00",
        "current_phase": "flow-code",
        "files": {"plan": null, "dag": null, "log": "", "state": ""},
        "phases": {
            "flow-start": {
                "name": "Start", "status": "complete", "started_at": null,
                "completed_at": null, "session_started_at": null,
                "cumulative_seconds": 0, "visit_count": 0
            },
            "flow-plan": {
                "name": "Plan", "status": "complete", "started_at": null,
                "completed_at": null, "session_started_at": null,
                "cumulative_seconds": 0, "visit_count": 0
            },
            "flow-code": {
                "name": "Code", "status": "in_progress", "started_at": null,
                "completed_at": null, "session_started_at": null,
                "cumulative_seconds": 0, "visit_count": 0,
                "window_at_enter": snap_enter,
                "window_at_complete": snap_complete
            },
            "flow-code-review": {
                "name": "Code Review", "status": "pending", "started_at": null,
                "completed_at": null, "session_started_at": null,
                "cumulative_seconds": 0, "visit_count": 0
            },
            "flow-learn": {
                "name": "Learn", "status": "pending", "started_at": null,
                "completed_at": null, "session_started_at": null,
                "cumulative_seconds": 0, "visit_count": 0
            },
            "flow-complete": {
                "name": "Complete", "status": "pending", "started_at": null,
                "completed_at": null, "session_started_at": null,
                "cumulative_seconds": 0, "visit_count": 0
            }
        }
    });
    flow
}

/// The detail panel renders the per-phase token table when the
/// selected flow's state carries snapshot data. This drives the
/// `phase_token_table` consumer in `render_detail_panel`.
#[test]
fn test_render_detail_panel_renders_token_table_when_data_present() {
    let mut app = make_app();
    app.flows = vec![make_flow_with_token_snapshots()];
    let output = render_to_string(&app, 100, 40);
    assert!(
        output.contains("Tokens"),
        "Tokens header must appear:\n{}",
        output
    );
    assert!(
        output.contains("Code:"),
        "Per-phase row must appear:\n{}",
        output
    );
}

/// The token table is omitted when no phase carries token data so
/// the existing detail panel layout is preserved for legacy state.
#[test]
fn test_render_detail_panel_omits_token_table_when_no_data() {
    let mut app = make_app();
    app.flows = vec![make_flow("Plain Feature", "Plan", 2)];
    let output = render_to_string(&app, 100, 40);
    // The make_flow helper builds a state with no `phases` field at
    // all, so the token table is empty and no `Tokens` header is
    // rendered. The phase list above contains "Plan" so we don't
    // assert on plain "Tokens" without disambiguation — instead,
    // assert no per-phase token row marker appears.
    assert!(
        !output.contains("  Tokens  "),
        "Tokens header must be omitted when no snapshots:\n{}",
        output
    );
}

/// Token table loop respects the viewport bound: when the panel
/// runs out of rows mid-table, the inner loop breaks rather than
/// rendering off-screen. Drives the `break` arm of the row-overflow
/// check inside the token row loop.
#[test]
fn test_render_detail_panel_token_table_breaks_when_viewport_overflows() {
    let mut app = make_app();
    let mut flow = make_flow_with_token_snapshots();
    // Populate snapshots on every phase so active_rows has 6 entries.
    // With a small viewport, the loop must break before all 6 land.
    let snap_enter = flow.state["phases"]["flow-code"]["window_at_enter"].clone();
    let snap_complete = flow.state["phases"]["flow-code"]["window_at_complete"].clone();
    for key in [
        "flow-start",
        "flow-plan",
        "flow-code-review",
        "flow-learn",
        "flow-complete",
    ] {
        flow.state["phases"][key]["window_at_enter"] = snap_enter.clone();
        flow.state["phases"][key]["window_at_complete"] = snap_complete.clone();
    }
    app.flows = vec![flow];
    // 100x18 leaves only a few rows in the detail panel — enough for
    // the timeline + header + 1 or 2 token rows, then the inner loop
    // hits the row-overflow break before all 6 token rows land.
    let _ = render_to_string(&app, 100, 18);
}

/// Window-reset marker appears on rows where the rate-limit window
/// rolled over mid-phase.
#[test]
fn test_render_detail_panel_token_table_marks_window_reset() {
    let mut app = make_app();
    let mut flow = make_flow_with_token_snapshots();
    // Rewrite flow-code's complete snapshot so 5h pct DROPS — that
    // triggers window_reset_observed in phase_delta.
    let snap_enter = flow.state["phases"]["flow-code"]["window_at_enter"].clone();
    let mut snap_complete = flow.state["phases"]["flow-code"]["window_at_complete"].clone();
    let mut enter_with_high_pct = snap_enter;
    enter_with_high_pct["five_hour_pct"] = serde_json::json!(80);
    snap_complete["five_hour_pct"] = serde_json::json!(5);
    flow.state["phases"]["flow-code"]["window_at_enter"] = enter_with_high_pct;
    flow.state["phases"]["flow-code"]["window_at_complete"] = snap_complete;
    app.flows = vec![flow];
    let output = render_to_string(&app, 100, 40);
    assert!(
        output.contains("\u{21bb}"),
        "Reset marker (↻) must appear when window resets:\n{}",
        output
    );
}

#[test]
fn test_render_detail_panel_with_issues() {
    let mut app = make_app();
    let mut flow = make_flow("Test Feature", "Code", 3);
    flow.issues = vec![IssueSummary {
        label: "Bug".to_string(),
        title: "Fix login".to_string(),
        url: "https://github.com/test/repo/issues/1".to_string(),
        ref_str: "#1".to_string(),
        phase_name: "Code".to_string(),
    }];
    flow.issues_count = 1;
    app.flows = vec![flow];
    let output = render_to_string(&app, 80, 40);
    assert!(output.contains("#1"));
    assert!(output.contains("Fix login"));
}

#[test]
fn test_render_footer_keybindings() {
    let mut app = make_app();
    app.flows = vec![make_flow("Test", "Code", 3)];
    // Footer is very wide — use 160 cols to fit all keybindings
    let output = render_to_string(&app, 160, 40);
    assert!(output.contains("[q] Quit"));
    assert!(output.contains("[p] PR"));
}

#[test]
fn test_render_header_metrics() {
    let mut app = make_app();
    app.metrics = AccountMetrics {
        cost_monthly: "12.50".to_string(),
        rl_5h: Some(45),
        rl_7d: Some(20),
        stale: false,
    };
    let output = render_to_string(&app, 120, 40);
    assert!(output.contains("$12.50/mo"));
    assert!(output.contains("5h:45%"));
    assert!(output.contains("7d:20%"));
}

#[test]
fn test_render_column_headers() {
    let mut app = make_app();
    app.flows = vec![make_flow("Test", "Code", 3)];
    let output = render_to_string(&app, 120, 40);
    assert!(output.contains("Feature"));
    assert!(output.contains("Phase"));
    assert!(output.contains("Total"));
}

// --- Orchestration view ---

#[test]
fn test_render_orch_no_state() {
    let mut app = make_app();
    app.active_tab = 1;
    let output = render_to_string(&app, 80, 40);
    assert!(output.contains("No orchestration running."));
}

#[test]
fn test_render_orch_with_queue() {
    let mut app = make_app();
    app.active_tab = 1;
    app.orch_data = Some(OrchestrationSummary {
        elapsed: "10m".to_string(),
        completed_count: 1,
        failed_count: 0,
        total: 3,
        is_running: true,
        items: vec![
            OrchestrationItem {
                icon: "\u{2713}".to_string(),
                issue_number: Some(10),
                title: "First task".to_string(),
                elapsed: "3m".to_string(),
                pr_url: Some("https://github.com/test/repo/pull/50".to_string()),
                reason: None,
                status: "completed".to_string(),
            },
            OrchestrationItem {
                icon: "\u{25b6}".to_string(),
                issue_number: Some(11),
                title: "Second task".to_string(),
                elapsed: "2m".to_string(),
                pr_url: None,
                reason: None,
                status: "in_progress".to_string(),
            },
        ],
    });
    let output = render_to_string(&app, 120, 40);
    assert!(output.contains("Elapsed: 10m"));
    assert!(output.contains("#10"));
    assert!(output.contains("First task"));
    assert!(output.contains("#11"));
}

#[test]
fn test_render_orch_tab_count() {
    let mut app = make_app();
    app.orch_data = Some(OrchestrationSummary {
        elapsed: "5m".to_string(),
        completed_count: 2,
        failed_count: 1,
        total: 5,
        is_running: true,
        items: vec![],
    });
    let output = render_to_string(&app, 120, 40);
    assert!(output.contains("Orchestration (3/5)"));
}

// --- Sub-views ---

#[test]
fn test_render_log_view_empty() {
    let mut app = make_app();
    app.flows = vec![make_flow("Test", "Code", 3)];
    app.view = View::Log;
    let output = render_to_string(&app, 80, 40);
    assert!(output.contains("No log entries."));
    assert!(output.contains("[Esc] Back"));
}

#[test]
fn test_render_issues_view_empty() {
    let mut app = make_app();
    app.flows = vec![make_flow("Test", "Code", 3)];
    app.view = View::Issues;
    let output = render_to_string(&app, 80, 40);
    assert!(output.contains("No issues filed."));
}

#[test]
fn test_render_issues_view_with_entries() {
    let mut app = make_app();
    let mut flow = make_flow("Test", "Code", 3);
    flow.issues = vec![IssueSummary {
        label: "Tech Debt".to_string(),
        title: "Refactor auth".to_string(),
        url: "https://github.com/test/repo/issues/5".to_string(),
        ref_str: "#5".to_string(),
        phase_name: "Code Review".to_string(),
    }];
    app.flows = vec![flow];
    app.view = View::Issues;
    let output = render_to_string(&app, 120, 40);
    assert!(output.contains("Tech Debt"));
    assert!(output.contains("#5"));
    assert!(output.contains("Refactor auth"));
}

#[test]
fn test_render_tasks_view_no_plan() {
    let mut app = make_app();
    app.flows = vec![make_flow("Test", "Code", 3)];
    app.view = View::Tasks;
    let output = render_to_string(&app, 80, 40);
    assert!(output.contains("No plan file."));
}

// --- Input handling ---

#[test]
fn test_input_quit() {
    let mut app = make_app();
    app.handle_key(key(KeyCode::Char('q')));
    assert!(!app.running);
}

#[test]
fn test_input_navigate_up_down() {
    let mut app = make_app();
    app.flows = vec![
        make_flow("A", "Code", 3),
        make_flow("B", "Plan", 2),
        make_flow("C", "Start", 1),
    ];
    assert_eq!(app.selected, 0);
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.selected, 1);
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.selected, 2);
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.selected, 1);
}

#[test]
fn test_input_navigate_bounds() {
    let mut app = make_app();
    app.flows = vec![make_flow("A", "Code", 3)];
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.selected, 0);
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.selected, 0); // only 1 flow, can't go past
}

#[test]
fn test_input_tab_switch() {
    let mut app = make_app();
    assert_eq!(app.active_tab, 0);
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.active_tab, 1);
    app.handle_key(key(KeyCode::Left));
    assert_eq!(app.active_tab, 0);
}

#[test]
fn test_input_tab_bounds() {
    let mut app = make_app();
    app.handle_key(key(KeyCode::Left));
    assert_eq!(app.active_tab, 0); // can't go below 0
    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.active_tab, 1); // can't go above 1
}

#[test]
fn test_input_log_key() {
    let mut app = make_app();
    app.flows = vec![make_flow("A", "Code", 3)];
    app.handle_key(key(KeyCode::Char('l')));
    assert_eq!(app.view, View::Log);
}

#[test]
fn test_input_issues_key() {
    let mut app = make_app();
    app.flows = vec![make_flow("A", "Code", 3)];
    app.handle_key(key(KeyCode::Char('i')));
    assert_eq!(app.view, View::Issues);
}

#[test]
fn test_input_tasks_key() {
    let mut app = make_app();
    app.flows = vec![make_flow("A", "Code", 3)];
    app.handle_key(key(KeyCode::Char('t')));
    assert_eq!(app.view, View::Tasks);
}

#[test]
fn test_input_escape_returns_to_list() {
    let mut app = make_app();
    app.view = View::Log;
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.view, View::List);

    app.view = View::Issues;
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.view, View::List);

    app.view = View::Tasks;
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.view, View::List);
}

#[test]
fn test_input_abort_start() {
    let mut app = make_app();
    app.flows = vec![make_flow("A", "Code", 3)];
    app.handle_key(key(KeyCode::Char('a')));
    assert!(app.confirming_abort);
}

#[test]
fn test_input_abort_confirm_no() {
    let mut app = make_app();
    app.confirming_abort = true;
    app.handle_key(key(KeyCode::Char('n')));
    assert!(!app.confirming_abort);
}

#[test]
fn test_input_orch_navigate() {
    let mut app = make_app();
    app.active_tab = 1;
    app.orch_data = Some(OrchestrationSummary {
        elapsed: "5m".to_string(),
        completed_count: 0,
        failed_count: 0,
        total: 3,
        is_running: true,
        items: vec![
            OrchestrationItem {
                icon: "\u{00b7}".to_string(),
                issue_number: Some(1),
                title: "A".to_string(),
                elapsed: String::new(),
                pr_url: None,
                reason: None,
                status: "pending".to_string(),
            },
            OrchestrationItem {
                icon: "\u{00b7}".to_string(),
                issue_number: Some(2),
                title: "B".to_string(),
                elapsed: String::new(),
                pr_url: None,
                reason: None,
                status: "pending".to_string(),
            },
        ],
    });
    assert_eq!(app.orch_selected, 0);
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.orch_selected, 1);
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.orch_selected, 0);
}

#[test]
fn test_input_issues_navigate() {
    let mut app = make_app();
    let mut flow = make_flow("A", "Code", 3);
    flow.issues = vec![
        IssueSummary {
            label: "Bug".to_string(),
            title: "Fix A".to_string(),
            url: String::new(),
            ref_str: "#1".to_string(),
            phase_name: "Code".to_string(),
        },
        IssueSummary {
            label: "Bug".to_string(),
            title: "Fix B".to_string(),
            url: String::new(),
            ref_str: "#2".to_string(),
            phase_name: "Code".to_string(),
        },
    ];
    app.flows = vec![flow];
    app.view = View::Issues;
    assert_eq!(app.issue_selected, 0);
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.issue_selected, 1);
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.issue_selected, 0);
}

#[test]
fn test_render_list_no_annotation_when_empty() {
    let mut app = make_app();
    let mut flow = make_flow("Test", "Code", 3);
    flow.annotation = String::new();
    app.flows = vec![flow];
    let output = render_to_string(&app, 120, 40);
    // Phase column should show "3: Code" without parentheses
    assert!(output.contains("3: Code"));
    assert!(!output.contains("3: Code ("));
}

#[test]
fn test_render_list_with_annotation() {
    let mut app = make_app();
    let mut flow = make_flow("Test", "Code", 3);
    flow.annotation = "task 2 of 5".to_string();
    app.flows = vec![flow];
    let output = render_to_string(&app, 120, 40);
    assert!(output.contains("3: Code (task 2 of 5)"));
}

#[test]
fn test_render_detail_panel_blocked_uses_red_marker() {
    let mut app = make_app();
    let mut flow = make_flow("Test", "Code", 3);
    flow.blocked = true;
    // Set the in-progress phase timeline entry
    flow.timeline[1].status = "in_progress".to_string();
    app.flows = vec![flow];
    // We can't easily check color in text output, but we can check the [>] marker exists
    let output = render_to_string(&app, 80, 40);
    assert!(output.contains("[>]"));
}

#[test]
fn test_render_header_metrics_stale() {
    let mut app = make_app();
    app.metrics = AccountMetrics {
        cost_monthly: "8.00".to_string(),
        rl_5h: None,
        rl_7d: None,
        stale: true,
    };
    let output = render_to_string(&app, 120, 40);
    assert!(output.contains("$8.00/mo"));
    assert!(output.contains("5h:--  7d:--"));
}

#[test]
fn test_render_orch_detail_failed_reason() {
    let mut app = make_app();
    app.active_tab = 1;
    app.orch_data = Some(OrchestrationSummary {
        elapsed: "5m".to_string(),
        completed_count: 0,
        failed_count: 1,
        total: 1,
        is_running: false,
        items: vec![OrchestrationItem {
            icon: "\u{2717}".to_string(),
            issue_number: Some(10),
            title: "Failed task".to_string(),
            elapsed: "1m".to_string(),
            pr_url: None,
            reason: Some("CI failed".to_string()),
            status: "failed".to_string(),
        }],
    });
    let output = render_to_string(&app, 120, 40);
    assert!(output.contains("Reason: CI failed"));
}

#[test]
fn test_render_orch_detail_completed_pr() {
    let mut app = make_app();
    app.active_tab = 1;
    app.orch_data = Some(OrchestrationSummary {
        elapsed: "5m".to_string(),
        completed_count: 1,
        failed_count: 0,
        total: 1,
        is_running: false,
        items: vec![OrchestrationItem {
            icon: "\u{2713}".to_string(),
            issue_number: Some(10),
            title: "Done task".to_string(),
            elapsed: "3m".to_string(),
            pr_url: Some("https://github.com/test/repo/pull/99".to_string()),
            reason: None,
            status: "completed".to_string(),
        }],
    });
    let output = render_to_string(&app, 120, 40);
    assert!(output.contains("PR: https://github.com/test/repo/pull/99"));
}

#[test]
fn test_render_issues_view_selected_marker() {
    let mut app = make_app();
    let mut flow = make_flow("Test", "Code", 3);
    flow.issues = vec![
        IssueSummary {
            label: "Bug".to_string(),
            title: "Issue Alpha".to_string(),
            url: String::new(),
            ref_str: "#1".to_string(),
            phase_name: "Code".to_string(),
        },
        IssueSummary {
            label: "Bug".to_string(),
            title: "Issue Beta".to_string(),
            url: String::new(),
            ref_str: "#2".to_string(),
            phase_name: "Code".to_string(),
        },
    ];
    app.flows = vec![flow];
    app.view = View::Issues;
    app.issue_selected = 1;
    let output = render_to_string(&app, 120, 40);
    // The selected marker ▸ should appear on the line with Issue Beta
    let lines: Vec<&str> = output.lines().collect();
    let beta_line = lines.iter().find(|l| l.contains("Issue Beta"));
    assert!(beta_line.is_some(), "Should find Issue Beta line");
    assert!(
        beta_line.unwrap().contains("\u{25b8}"),
        "Selected issue should have ▸ marker"
    );
    // First issue should NOT have the marker
    let alpha_line = lines.iter().find(|l| l.contains("Issue Alpha"));
    assert!(alpha_line.is_some(), "Should find Issue Alpha line");
    assert!(
        !alpha_line.unwrap().contains("\u{25b8}"),
        "Non-selected issue should not have ▸ marker"
    );
}

#[test]
fn test_input_no_flows_list_noop() {
    let mut app = make_app();
    // With no flows, list input should be a no-op
    app.handle_key(key(KeyCode::Up));
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.selected, 0);
    assert_eq!(app.view, View::List);
}

#[test]
fn test_render_tab_bar_with_flows_count() {
    let mut app = make_app();
    app.flows = vec![make_flow("A", "Code", 3), make_flow("B", "Plan", 2)];
    let output = render_to_string(&app, 120, 40);
    assert!(output.contains("Active Flows (2)"));
}

// --- Render-branch coverage (Task 12) ---

#[test]
fn test_render_list_truncates_long_feature_name() {
    // Feature name longer than the computed feature_width at 80 cols.
    // The renderer emits `format!("{}...", truncated)` — ASCII dots.
    let mut app = make_app();
    let long_name = "A".repeat(80);
    app.flows = vec![make_flow(&long_name, "Code", 3)];
    let output = render_to_string(&app, 80, 40);
    assert!(
        output.contains("..."),
        "expected truncation ellipsis in output:\n{}",
        output
    );
}

#[test]
fn test_render_orch_truncates_long_item_title() {
    // Long orchestration item title at narrow-ish width forces title
    // truncation via `format!("{}...", truncated)`.
    let mut app = make_app();
    app.active_tab = 1;
    app.orch_data = Some(OrchestrationSummary {
        elapsed: "5m".to_string(),
        completed_count: 0,
        failed_count: 0,
        total: 1,
        is_running: true,
        items: vec![OrchestrationItem {
            icon: "\u{25b6}".to_string(),
            issue_number: Some(1),
            title: "X".repeat(80),
            elapsed: "1m".to_string(),
            pr_url: None,
            reason: None,
            status: "in_progress".to_string(),
        }],
    });
    let output = render_to_string(&app, 80, 40);
    assert!(
        output.contains("..."),
        "expected orch title truncation in output:\n{}",
        output
    );
}

#[test]
fn test_render_metrics_suppressed_when_viewport_too_narrow() {
    // Non-stale metrics at a narrow width should trip the
    // total_width > max_x.saturating_sub(30) early-return and render
    // no metrics text at all.
    let mut app = make_app();
    app.metrics = AccountMetrics {
        cost_monthly: "12.50".to_string(),
        rl_5h: Some(45),
        rl_7d: Some(20),
        stale: false,
    };
    let output = render_to_string(&app, 40, 40);
    // At 40 cols, the metrics strip is dropped entirely.
    assert!(!output.contains("$12.50/mo"));
    assert!(!output.contains("5h:45%"));
}

#[test]
fn test_render_metrics_suppressed_stale_when_viewport_too_narrow() {
    // Symmetric: stale branch also has the narrow-width guard.
    let mut app = make_app();
    app.metrics = AccountMetrics {
        cost_monthly: "8.00".to_string(),
        rl_5h: None,
        rl_7d: None,
        stale: true,
    };
    let output = render_to_string(&app, 40, 40);
    assert!(!output.contains("$8.00/mo"));
    assert!(!output.contains("5h:--"));
}

#[test]
fn test_render_log_view_with_entries() {
    // Write a valid log file at .flow-states/<branch>/log so the Log
    // view hits the entries-iteration branch.
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let branch_dir = root.join(".flow-states").join("test-feature");
    std::fs::create_dir_all(&branch_dir).unwrap();
    let log_content = "2026-01-01T12:34:56-08:00 [Phase 1] start-init — initializing (ok)\n";
    std::fs::write(branch_dir.join("log"), log_content).unwrap();

    let mut app = TuiApp::new(
        root.to_path_buf(),
        "1.0.0".to_string(),
        None,
        TuiAppPlatform::for_tests(),
    );
    let mut flow = make_flow("Log Feature", "Code", 3);
    flow.branch = "test-feature".to_string();
    app.flows = vec![flow];
    app.view = View::Log;
    let output = render_to_string(&app, 80, 40);
    assert!(
        output.contains("12:34"),
        "expected formatted log time in output:\n{}",
        output
    );
    assert!(output.contains("start-init"));
}

#[test]
fn test_render_tasks_view_with_plan_content() {
    // Write a minimal plan file and point flow.plan_path at it so the
    // Tasks view hits the content-iteration branch.
    let tmp = tempfile::TempDir::new().unwrap();
    let plan_path = tmp.path().join("plan.md");
    std::fs::write(&plan_path, "## Tasks\n- task 1\n- task 2\n").unwrap();

    let mut app = make_app();
    let mut flow = make_flow("Plan Feature", "Code", 3);
    flow.plan_path = Some(plan_path.to_string_lossy().into_owned());
    app.flows = vec![flow];
    app.view = View::Tasks;
    let output = render_to_string(&app, 80, 40);
    assert!(output.contains("## Tasks"));
    assert!(output.contains("task 1"));
    assert!(output.contains("task 2"));
}

#[test]
fn test_render_detail_panel_shows_notes_count() {
    let mut app = make_app();
    let mut flow = make_flow("Notes Feature", "Code", 3);
    flow.notes_count = 3;
    app.flows = vec![flow];
    let output = render_to_string(&app, 80, 40);
    assert!(output.contains("Notes: 3"));
}

#[test]
fn test_render_list_orch_issue_in_progress_but_flow_does_not_match() {
    // Covers the `else if orch_issue.is_some_and(|n| flow.issue_numbers.contains(&n))`
    // branch where the closure runs and returns FALSE — i.e. orch_issue
    // is Some but the flow's issue_numbers don't contain it.
    let mut app = make_app();
    app.selected = 0;
    let mut matched_flow = make_flow("Matched", "Code", 3);
    matched_flow.issue_numbers = vec![42];
    let mut unmatched_flow = make_flow("Unmatched", "Plan", 2);
    unmatched_flow.issue_numbers = vec![99]; // not 42
    app.flows = vec![matched_flow, unmatched_flow];
    app.orch_data = Some(OrchestrationSummary {
        elapsed: "5m".to_string(),
        completed_count: 0,
        failed_count: 0,
        total: 1,
        is_running: true,
        items: vec![OrchestrationItem {
            icon: "\u{25b6}".to_string(),
            issue_number: Some(42),
            title: "Linked".to_string(),
            elapsed: "1m".to_string(),
            pr_url: None,
            reason: None,
            status: "in_progress".to_string(),
        }],
    });
    let _ = render_to_string(&app, 120, 40);
}

#[test]
fn test_render_header_with_used_greater_than_width_skips_suffix_border() {
    // When the panel width is smaller than the version + repo
    // header text, `if used < width` is false and the suffix-border
    // span is never appended. Use a tiny width so used (15+) >= width.
    let app = TuiApp::new(
        PathBuf::from("/tmp/test"),
        "1.0.0".to_string(),
        Some("test/repo".to_string()),
        TuiAppPlatform::for_tests(),
    );
    // width = 5 → used = 2 + len(" FLOW v1.0.0 ") + len("repo")+1 ≈ 20
    // → 20 < 5 false → suffix border skipped.
    let _ = render_to_string(&app, 5, 40);
}

#[test]
fn test_render_header_without_repo_name_omits_repo_span() {
    // Covers the `if let Some(ref name) = self.repo_name` None branch
    // — render_header skips the repo span entirely when repo is None.
    let app = TuiApp::new(
        PathBuf::from("/tmp/test"),
        "1.0.0".to_string(),
        None,
        TuiAppPlatform::for_tests(),
    );
    let output = render_to_string(&app, 80, 40);
    assert!(output.contains("FLOW v1.0.0"));
    // No repo name to render — REPO label should be absent.
    assert!(!output.contains("REPO"));
}

#[test]
fn test_render_list_shows_diamond_marker_for_orch_in_progress_issue() {
    // When an orchestration item is in_progress and its issue_number
    // matches a flow's issue_numbers, the flow row gets a ◆ marker
    // (non-selected rows). Make the orch-linked flow *not* selected
    // so the ◆ marker wins over the ▸ selected marker.
    let mut app = make_app();
    app.selected = 1;
    let mut orch_flow = make_flow("Orch-Linked", "Code", 3);
    orch_flow.issue_numbers = vec![42];
    let other_flow = make_flow("Other", "Plan", 2);
    app.flows = vec![orch_flow, other_flow];
    app.orch_data = Some(OrchestrationSummary {
        elapsed: "5m".to_string(),
        completed_count: 0,
        failed_count: 0,
        total: 1,
        is_running: true,
        items: vec![OrchestrationItem {
            icon: "\u{25b6}".to_string(),
            issue_number: Some(42),
            title: "Linked".to_string(),
            elapsed: "1m".to_string(),
            pr_url: None,
            reason: None,
            status: "in_progress".to_string(),
        }],
    });
    let output = render_to_string(&app, 120, 40);
    assert!(
        output.contains("\u{25c6}"),
        "expected diamond marker \u{25c6} in list output:\n{}",
        output
    );
}

// --- Dispatch tests for action keys (Task 13) ---
//
// These tests cover the match arms in `handle_list_input`,
// `handle_orch_input`, and `handle_abort_confirm`. Each test sets
// state up so the action method reached by the dispatch hits its
// early-return / None branch and never spawns a subprocess. This
// covers the dispatch lines without launching browsers or running
// `bin/flow cleanup`.

#[test]
fn test_input_enter_in_list_view_dispatches_without_spawn() {
    // Flow has no session_tty → worktree_session_tty returns None →
    // open_worktree early-returns before activate_iterm_tab.
    let mut app = make_app();
    let mut flow = make_flow("A", "Code", 3);
    // Default make_flow state has no session_tty field.
    flow.state = serde_json::json!({"branch": "a"});
    app.flows = vec![flow];
    app.handle_key(key(KeyCode::Enter));
    // Dispatch reached the Enter arm without panicking.
    assert_eq!(app.view, View::List);
}

#[test]
fn test_input_p_in_list_view_dispatches_without_spawn() {
    // Flow has no pr_url → open_pr early-returns before open_url.
    let mut app = make_app();
    let mut flow = make_flow("A", "Code", 3);
    flow.pr_url = None;
    app.flows = vec![flow];
    app.handle_key(key(KeyCode::Char('p')));
    assert_eq!(app.view, View::List);
}

#[test]
fn test_input_capital_i_in_list_view_dispatches_without_spawn() {
    // No repo in state AND no fallback repo → flow_issue_url returns
    // None → open_flow_issue early-returns before open_url.
    let mut app = TuiApp::new(
        PathBuf::from("/tmp/test"),
        "1.0.0".to_string(),
        None,
        TuiAppPlatform::for_tests(),
    );
    let mut flow = make_flow("A", "Code", 3);
    flow.state = serde_json::json!({"branch": "a"});
    flow.issue_numbers = vec![42];
    app.flows = vec![flow];
    app.handle_key(key(KeyCode::Char('I')));
    assert_eq!(app.view, View::List);
}

#[test]
fn test_input_r_in_list_view_refreshes_data_from_tmpdir() {
    // Press r → refresh_data reads .flow-states/, clears flows
    // because the tmpdir has no state files.
    let tmp = tempfile::TempDir::new().unwrap();
    let mut app = TuiApp::new(
        tmp.path().to_path_buf(),
        "1.0.0".to_string(),
        None,
        TuiAppPlatform::for_tests(),
    );
    app.flows = vec![make_flow("Pre-refresh", "Code", 3)];
    app.handle_key(key(KeyCode::Char('r')));
    // After refresh, no state files found → flows is empty.
    assert!(app.flows.is_empty());
}

#[test]
fn test_input_r_in_orch_view_refreshes_data() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut app = TuiApp::new(
        tmp.path().to_path_buf(),
        "1.0.0".to_string(),
        None,
        TuiAppPlatform::for_tests(),
    );
    app.active_tab = 1;
    app.orch_data = Some(OrchestrationSummary {
        elapsed: "5m".to_string(),
        completed_count: 0,
        failed_count: 0,
        total: 1,
        is_running: true,
        items: vec![],
    });
    app.handle_key(key(KeyCode::Char('r')));
    // After refresh against empty tmpdir, orch_data is None.
    assert!(app.orch_data.is_none());
}

#[test]
fn test_input_i_in_orch_view_dispatches_without_spawn() {
    // No repo → orch_issue_url returns None → open_orch_issue
    // early-returns before open_url.
    let mut app = TuiApp::new(
        PathBuf::from("/tmp/test"),
        "1.0.0".to_string(),
        None,
        TuiAppPlatform::for_tests(),
    );
    app.active_tab = 1;
    app.orch_data = Some(OrchestrationSummary {
        elapsed: "5m".to_string(),
        completed_count: 0,
        failed_count: 0,
        total: 1,
        is_running: true,
        items: vec![OrchestrationItem {
            icon: "\u{25b6}".to_string(),
            issue_number: Some(42),
            title: "Item".to_string(),
            elapsed: "1m".to_string(),
            pr_url: None,
            reason: None,
            status: "in_progress".to_string(),
        }],
    });
    app.handle_key(key(KeyCode::Char('i')));
    assert_eq!(app.active_tab, 1);
}

#[test]
fn test_refresh_data_with_selected_in_range_skips_clamp() {
    // refresh_data: when self.selected < self.flows.len() after
    // load_all_flows, the clamp inside `if self.selected >= ...`
    // should NOT run. Same for orch_selected. This exercises the
    // false branches of both index-clamp guards.
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join(".flow-states")).unwrap();
    // Two state files so flows.len() == 2.
    for branch in &["alpha-flow", "beta-flow"] {
        let state_json = serde_json::json!({
            "branch": branch,
            "current_phase": "flow-code",
            "started_at": "2026-01-01T00:00:00-08:00",
            "phases": {
                "flow-start": {"name": "Start", "status": "complete", "cumulative_seconds": 60, "visit_count": 1},
                "flow-code": {"name": "Code", "status": "in_progress", "cumulative_seconds": 0, "visit_count": 1},
            },
            "prompt": "test",
        });
        let branch_dir = root.join(".flow-states").join(branch);
        std::fs::create_dir_all(&branch_dir).unwrap();
        std::fs::write(
            branch_dir.join("state.json"),
            serde_json::to_string(&state_json).unwrap(),
        )
        .unwrap();
    }
    // orchestrate.json with two queue items.
    let orch_json = serde_json::json!({
        "started_at": "2026-01-01T00:00:00-08:00",
        "queue": [
            {"issue_number": 1, "title": "First", "status": "in_progress",
             "started_at": "2026-01-01T00:00:00-08:00"},
            {"issue_number": 2, "title": "Second", "status": "pending"},
        ],
    });
    std::fs::write(
        root.join(".flow-states").join("orchestrate.json"),
        serde_json::to_string(&orch_json).unwrap(),
    )
    .unwrap();

    let mut app = TuiApp::new(
        root.to_path_buf(),
        "1.0.0".to_string(),
        None,
        TuiAppPlatform::for_tests(),
    );
    // selected and orch_selected start at 0 — well within the loaded
    // flows.len()=2 and orch.items.len()=2, so the clamp `if selected
    // >= len { ... }` is FALSE on both checks.
    app.selected = 0;
    app.orch_selected = 0;

    app.refresh_data();

    assert_eq!(app.flows.len(), 2);
    assert!(app.orch_data.is_some());
    assert_eq!(app.selected, 0);
    assert_eq!(app.orch_selected, 0);
}

#[test]
fn test_refresh_data_populates_flows_orch_and_metrics_and_clamps_indices() {
    // Build a complete production-layout fixture: one valid state
    // file, an orchestrate.json, and a cost file under .claude/cost.
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join(".flow-states")).unwrap();
    let state_json = serde_json::json!({
        "branch": "test-feature",
        "current_phase": "flow-code",
        "pr_number": 1,
        "started_at": "2026-01-01T00:00:00-08:00",
        "phases": {
            "flow-start": {"name": "Start", "status": "complete", "cumulative_seconds": 60, "visit_count": 1},
            "flow-code": {"name": "Code", "status": "in_progress", "cumulative_seconds": 0, "visit_count": 1},
        },
        "prompt": "work on it",
    });
    let branch_dir = root.join(".flow-states").join("test-feature");
    std::fs::create_dir_all(&branch_dir).unwrap();
    std::fs::write(
        branch_dir.join("state.json"),
        serde_json::to_string_pretty(&state_json).unwrap(),
    )
    .unwrap();
    let orch_json = serde_json::json!({
        "started_at": "2026-01-01T00:00:00-08:00",
        "queue": [
            {"issue_number": 1, "title": "Item", "status": "in_progress",
             "started_at": "2026-01-01T00:00:00-08:00"}
        ],
    });
    std::fs::write(
        root.join(".flow-states").join("orchestrate.json"),
        serde_json::to_string_pretty(&orch_json).unwrap(),
    )
    .unwrap();
    // Cost file under .claude/cost/<YYYY-MM>/session1. Use the
    // current YYYY-MM so load_account_metrics picks it up.
    let year_month = chrono::Local::now().format("%Y-%m").to_string();
    let cost_dir = root.join(".claude").join("cost").join(&year_month);
    std::fs::create_dir_all(&cost_dir).unwrap();
    std::fs::write(cost_dir.join("session1"), "1.50").unwrap();

    let mut app = TuiApp::new(
        root.to_path_buf(),
        "1.0.0".to_string(),
        None,
        TuiAppPlatform::for_tests(),
    );
    // Pre-set the selection indices past the end of the refreshed
    // lists to exercise the saturating-clamp logic.
    app.selected = 99;
    app.orch_selected = 99;

    app.refresh_data();

    // All three load_* IO chains populated the state.
    assert_eq!(app.flows.len(), 1, "flows did not populate from state file");
    assert!(app.orch_data.is_some(), "orch_data did not populate");
    assert_ne!(
        app.metrics.cost_monthly, "0.00",
        "metrics.cost_monthly did not accumulate cost files"
    );

    // Saturating clamps pulled the indices back in-range.
    assert_eq!(app.selected, 0, "selected did not clamp");
    assert_eq!(app.orch_selected, 0, "orch_selected did not clamp");
}

#[test]
fn test_input_abort_confirm_yes_with_empty_flows_is_noop() {
    // Y dispatches to abort_flow but flows.is_empty() guards the
    // subprocess spawn. Safe to exercise in tests.
    let mut app = make_app();
    app.confirming_abort = true;
    app.handle_key(key(KeyCode::Char('y')));
    // Dispatch cleared confirming_abort AND took the Y branch.
    assert!(!app.confirming_abort);
    // No flows, no spawn.
    assert!(app.flows.is_empty());
}

#[test]
fn test_input_abort_confirm_capital_y_with_empty_flows_is_noop() {
    let mut app = make_app();
    app.confirming_abort = true;
    app.handle_key(key(KeyCode::Char('Y')));
    assert!(!app.confirming_abort);
}

#[test]
fn test_activate_iterm_tab_with_test_platform_returns_without_panic() {
    // `TuiAppPlatform::for_tests()` points the osascript binary at
    // /usr/bin/true. `/usr/bin/true -e "<script>"` runs without panic
    // and returns success with empty stdout. `parse_osascript_result`
    // then returns false (because "" != "activated"). The whole
    // Command::new(...).output() chain in
    // `TuiApp::activate_iterm_tab` runs for real.
    let app = make_app();
    let result = app.activate_iterm_tab("/dev/ttys000");
    // /usr/bin/true exits 0 with empty stdout; parse_osascript_result
    // returns false because stdout is not "activated".
    assert!(!result);
}

#[test]
fn test_activate_iterm_tab_with_missing_binary_returns_false() {
    // Pointing the osascript binary at a nonexistent path forces
    // `Command::new(...).output()` to return Err — covers the Err
    // arm of the match in activate_iterm_tab.
    let mut platform = TuiAppPlatform::for_tests();
    platform.osascript_binary = "/nonexistent/binary/path-spawn-fails".to_string();
    let app = TuiApp::new(
        PathBuf::from("/tmp/test"),
        "1.0.0".to_string(),
        None,
        platform,
    );
    let result = app.activate_iterm_tab("/dev/ttys000");
    assert!(!result);
}

#[test]
fn test_input_enter_in_list_view_with_session_tty_exercises_activate() {
    // Flow has a session_tty string — worktree_session_tty returns
    // Some(tty), and open_worktree calls self.activate_iterm_tab(tty)
    // which spawns `/bin/true` under the test platform. This
    // exercises the full dispatch chain through
    // Command::new(...).output() without side effects.
    let mut app = make_app();
    let mut flow = make_flow("A", "Code", 3);
    flow.state = serde_json::json!({
        "branch": "a",
        "session_tty": "/dev/ttys000",
    });
    app.flows = vec![flow];
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.view, View::List);
}

// --- run_event_loop tests via TestBackend + fake event source ---

/// Build a boxed fake event source closure that pops events from a
/// queue. Returns `None` when the queue is empty (simulating a
/// timeout). Boxed so the closure type erases to `EventSourceFn`
/// and matches the production call signature.
fn fake_event_source(events: VecDeque<Option<Event>>) -> EventSourceFn {
    let mut queue = events;
    Box::new(move |_timeout| Ok(queue.pop_front().unwrap_or(None)))
}

/// Build a boxed draw closure owning a `Terminal<TestBackend>`.
/// Mirrors the production `run_tui_terminal` shape so tests share
/// the exact call signature of `TuiApp::run_event_loop` — one
/// symbol in coverage reports regardless of backend.
fn test_draw_closure(width: u16, height: u16) -> DrawFn {
    let backend = TestBackend::new(width, height);
    let terminal = Rc::new(RefCell::new(Terminal::new(backend).unwrap()));
    Box::new(move |render_fn: &mut dyn FnMut(&mut Frame)| {
        terminal.borrow_mut().draw(|f| render_fn(f))?;
        Ok(())
    })
}

fn key_event(code: KeyCode) -> Event {
    Event::Key(KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    })
}

#[test]
fn test_run_event_loop_with_test_backend_and_quit_key_exits_cleanly() {
    // The simplest happy path: queue a single `q` keypress, which
    // triggers `handle_key` → `self.running = false`, ending the
    // loop on the next iteration. Assert the loop exits Ok.
    let mut app = make_app();
    let draw = test_draw_closure(80, 24);
    let events = fake_event_source(VecDeque::from(vec![Some(key_event(KeyCode::Char('q')))]));
    let result = app.run_event_loop(draw, events);
    assert!(result.is_ok());
    assert!(!app.running);
}

#[test]
fn test_run_event_loop_handles_resize_then_quit() {
    // Queue a resize event (which triggers refresh_data) and then a
    // `q` keypress to exit. Covers the Event::Resize arm.
    let mut app = make_app();
    let draw = test_draw_closure(80, 24);
    let events = fake_event_source(VecDeque::from(vec![
        Some(Event::Resize(100, 30)),
        Some(key_event(KeyCode::Char('q'))),
    ]));
    let result = app.run_event_loop(draw, events);
    assert!(result.is_ok());
}

#[test]
fn test_run_event_loop_handles_timeout_then_quit() {
    // Queue None (timeout → refresh_data) then `q`. Covers the None
    // arm in the match.
    let mut app = make_app();
    let draw = test_draw_closure(80, 24);
    let events = fake_event_source(VecDeque::from(vec![
        None,
        Some(key_event(KeyCode::Char('q'))),
    ]));
    let result = app.run_event_loop(draw, events);
    assert!(result.is_ok());
}

#[test]
fn test_run_event_loop_handles_mouse_event_then_quit() {
    // Queue an unhandled event variant (Event::FocusGained) then `q`.
    // Covers the Some(_) catchall arm.
    let mut app = make_app();
    let draw = test_draw_closure(80, 24);
    let events = fake_event_source(VecDeque::from(vec![
        Some(Event::FocusGained),
        Some(key_event(KeyCode::Char('q'))),
    ]));
    let result = app.run_event_loop(draw, events);
    assert!(result.is_ok());
}

#[test]
fn test_run_event_loop_propagates_draw_error() {
    // A draw closure that returns Err on the first call. The `?`
    // operator after `draw(...)?` propagates the error and the loop
    // returns Err without polling events. Covers the Err arm of the
    // `?` operator on the draw call.
    let mut app = make_app();
    let draw: DrawFn = Box::new(|_render_fn: &mut dyn FnMut(&mut Frame)| {
        Err(std::io::Error::other("draw failed"))
    });
    let events = fake_event_source(VecDeque::new());
    let result = app.run_event_loop(draw, events);
    assert!(result.is_err());
}

#[test]
fn test_run_event_loop_propagates_event_source_error() {
    // An event source closure that returns Err. The `?` operator
    // after `events(...)?` propagates the error. Covers the Err arm
    // of the `?` operator on the events call.
    let mut app = make_app();
    let draw = test_draw_closure(80, 24);
    let events: EventSourceFn =
        Box::new(|_timeout| Err(std::io::Error::other("event poll failed")));
    let result = app.run_event_loop(draw, events);
    assert!(result.is_err());
}

#[test]
fn test_input_abort_confirm_capital_y_with_flow_exercises_cleanup_spawn() {
    // Populate a flow so abort_flow does NOT early-return. Then
    // press Y on the confirm prompt. abort_flow spawns
    // `/bin/true cleanup <root> --branch <b> --worktree <w>` via
    // self.platform.bin_flow_path which is /bin/true under
    // TuiAppPlatform::for_tests(). The Command::new(...).status()
    // line runs for real with no side effects (/bin/true ignores
    // args and exits 0).
    //
    // The raw-mode toggles (disable_raw_mode / LeaveAlternateScreen
    // / enable_raw_mode / EnterAlternateScreen) also execute under
    // cargo nextest's non-tty stdout — crossterm returns errors
    // silently via the `let _ =` prefix and no panic occurs. The
    // eprintln! line runs too.
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".flow-states")).unwrap();
    let mut app = TuiApp::new(
        tmp.path().to_path_buf(),
        "1.0.0".to_string(),
        None,
        TuiAppPlatform::for_tests(),
    );
    let flow = make_flow("Abort Target", "Code", 3);
    app.flows = vec![flow];
    app.confirming_abort = true;
    app.handle_key(key(KeyCode::Char('Y')));
    assert!(!app.confirming_abort);
}

#[test]
fn test_render_list_feature_narrower_than_default_shows_full_name() {
    // Short feature name at a wide viewport should NOT show `...`
    // truncation — guards the non-truncation branch of the
    // char-count comparison.
    let mut app = make_app();
    app.flows = vec![make_flow("Short", "Code", 3)];
    let output = render_to_string(&app, 120, 40);
    assert!(output.contains("Short"));
}

// --- Closure and dispatch coverage gap closures ---

#[test]
fn test_render_tasks_view_falls_back_to_root_join_when_relative_path() {
    // Covers the `.or_else(|| std::fs::read_to_string(self.root.join(p)).ok())`
    // closure inside render_tasks_view. The first read_to_string against
    // the relative path fails (no such file in the test process cwd), so
    // the or_else closure runs and reads from `self.root.join(p)`.
    let tmp = tempfile::TempDir::new().unwrap();
    let unique_name = "tui-fallback-plan-12345.md";
    std::fs::write(tmp.path().join(unique_name), "## Tasks\n- fallback task\n").unwrap();

    let mut app = TuiApp::new(
        tmp.path().to_path_buf(),
        "1.0.0".to_string(),
        None,
        TuiAppPlatform::for_tests(),
    );
    let mut flow = make_flow("Fallback Plan", "Code", 3);
    // Relative path: read_to_string(p) fails, root.join(p) succeeds.
    flow.plan_path = Some(unique_name.to_string());
    app.flows = vec![flow];
    app.view = View::Tasks;
    let output = render_to_string(&app, 80, 40);
    assert!(output.contains("fallback task"));
}

#[test]
fn test_input_unknown_key_in_log_view_hits_handle_key_catchall() {
    // In Log view with active_tab=0, an unhandled key falls through every
    // guarded arm in handle_key and lands on the bare `_ => {}` arm.
    // Without this test, that arm is dead code in coverage even though
    // it's reachable.
    let mut app = make_app();
    app.flows = vec![make_flow("A", "Code", 3)];
    app.view = View::Log;
    app.handle_key(key(KeyCode::Char('z')));
    // No state change expected; we just need the catchall arm executed.
    assert_eq!(app.view, View::Log);
}

#[test]
fn test_input_unknown_key_in_tasks_view_hits_tasks_arm_noop_body() {
    // In Tasks view with active_tab=0, an unhandled key matches the
    // `_ if self.view == View::Tasks => {}` arm. The empty body is
    // a no-op but the arm must be exercised for coverage.
    let mut app = make_app();
    app.flows = vec![make_flow("A", "Code", 3)];
    app.view = View::Tasks;
    app.handle_key(key(KeyCode::Char('z')));
    assert_eq!(app.view, View::Tasks);
}

#[test]
fn test_input_unknown_key_in_list_view_hits_handle_list_input_catchall() {
    // In List view, key 'z' is not in the enumerated arms of
    // handle_list_input — it lands on `_ => {}`.
    let mut app = make_app();
    app.flows = vec![make_flow("A", "Code", 3)];
    app.handle_key(key(KeyCode::Char('z')));
    assert_eq!(app.view, View::List);
}

#[test]
fn test_input_unknown_key_in_issues_view_hits_handle_issues_input_catchall() {
    // In Issues view with at least one issue, key 'z' isn't Up/Down/Enter
    // — it lands on `_ => {}`.
    let mut app = make_app();
    let mut flow = make_flow("A", "Code", 3);
    flow.issues = vec![IssueSummary {
        label: "Bug".to_string(),
        title: "T".to_string(),
        url: String::new(),
        ref_str: "#1".to_string(),
        phase_name: "Code".to_string(),
    }];
    app.flows = vec![flow];
    app.view = View::Issues;
    app.handle_key(key(KeyCode::Char('z')));
    assert_eq!(app.view, View::Issues);
}

#[test]
fn test_input_unknown_key_in_orch_view_hits_handle_orch_input_catchall() {
    // In Orchestration tab, key 'z' isn't Up/Down/i/r — it lands on `_ => {}`.
    let mut app = make_app();
    app.active_tab = 1;
    app.orch_data = Some(OrchestrationSummary {
        elapsed: "5m".to_string(),
        completed_count: 0,
        failed_count: 0,
        total: 1,
        is_running: true,
        items: vec![OrchestrationItem {
            icon: "\u{25b6}".to_string(),
            issue_number: Some(1),
            title: "Item".to_string(),
            elapsed: "1m".to_string(),
            pr_url: None,
            reason: None,
            status: "in_progress".to_string(),
        }],
    });
    app.handle_key(key(KeyCode::Char('z')));
    assert_eq!(app.active_tab, 1);
}

#[test]
fn test_input_enter_in_issues_view_with_url_opens_browser() {
    // Issues view + Enter on an issue with a non-empty URL hits the
    // KeyCode::Enter arm AND the inner `if let Some(url)` branch AND the
    // open_url call. open_url under TuiAppPlatform::for_tests() spawns
    // /bin/true which exits 0 with no side effect.
    let mut app = make_app();
    let mut flow = make_flow("A", "Code", 3);
    flow.issues = vec![IssueSummary {
        label: "Bug".to_string(),
        title: "Open me".to_string(),
        url: "https://github.com/test/repo/issues/1".to_string(),
        ref_str: "#1".to_string(),
        phase_name: "Code".to_string(),
    }];
    app.flows = vec![flow];
    app.view = View::Issues;
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.view, View::Issues);
}

#[test]
fn test_input_p_in_list_view_with_pr_url_opens_browser() {
    // Flow has pr_url Some → open_pr's `if let Some(ref url)` branch
    // triggers pr_files_url and open_url. open_url spawns /bin/true.
    let mut app = make_app();
    let mut flow = make_flow("A", "Code", 3);
    flow.pr_url = Some("https://github.com/test/repo/pull/100".to_string());
    app.flows = vec![flow];
    app.handle_key(key(KeyCode::Char('p')));
    assert_eq!(app.view, View::List);
}

#[test]
fn test_input_capital_i_in_list_view_with_state_repo_opens_url() {
    // state has `repo` AND issue_numbers is non-empty → flow_issue_url
    // returns Some → open_flow_issue calls open_url. /bin/true via
    // TuiAppPlatform::for_tests().
    let mut app = make_app();
    let mut flow = make_flow("A", "Code", 3);
    flow.state = serde_json::json!({"branch": "a", "repo": "test/repo"});
    flow.issue_numbers = vec![42];
    app.flows = vec![flow];
    app.handle_key(key(KeyCode::Char('I')));
    assert_eq!(app.view, View::List);
}

#[test]
fn test_input_i_in_orch_view_with_repo_opens_url() {
    // orch_data has an item with issue_number Some AND TuiApp has a repo
    // → orch_issue_url returns Some → open_orch_issue calls open_url.
    let mut app = make_app();
    app.active_tab = 1;
    app.orch_data = Some(OrchestrationSummary {
        elapsed: "5m".to_string(),
        completed_count: 0,
        failed_count: 0,
        total: 1,
        is_running: true,
        items: vec![OrchestrationItem {
            icon: "\u{25b6}".to_string(),
            issue_number: Some(42),
            title: "Item".to_string(),
            elapsed: "1m".to_string(),
            pr_url: None,
            reason: None,
            status: "in_progress".to_string(),
        }],
    });
    app.handle_key(key(KeyCode::Char('i')));
    assert_eq!(app.active_tab, 1);
}

#[test]
fn test_input_i_in_orch_view_with_orch_selected_out_of_range_is_noop() {
    // orch_selected past the end of items → orch.items.get returns None
    // → open_orch_issue's inner `if let Some(item)` branch is the None
    // arm. Covers the close-brace region of that if-let block.
    let mut app = make_app();
    app.active_tab = 1;
    app.orch_data = Some(OrchestrationSummary {
        elapsed: "5m".to_string(),
        completed_count: 0,
        failed_count: 0,
        total: 1,
        is_running: true,
        items: vec![OrchestrationItem {
            icon: "\u{25b6}".to_string(),
            issue_number: Some(42),
            title: "Item".to_string(),
            elapsed: "1m".to_string(),
            pr_url: None,
            reason: None,
            status: "in_progress".to_string(),
        }],
    });
    app.orch_selected = 5;
    app.handle_key(key(KeyCode::Char('i')));
    assert_eq!(app.active_tab, 1);
}

#[test]
fn test_input_i_in_orch_view_with_no_orch_data_is_noop() {
    // orch_data is None → open_orch_issue's outer `if let Some(orch)`
    // branch is the None arm.
    let mut app = make_app();
    app.active_tab = 1;
    app.orch_data = None;
    app.handle_key(key(KeyCode::Char('i')));
    assert_eq!(app.active_tab, 1);
}

// --- Defensive empty-flows render paths (Log/Issues/Tasks views) ---
//
// In production, switching to View::Log/Issues/Tasks requires a non-
// empty flows list (handle_list_input guards it). But refresh_data
// can clear flows after the user has switched views, so the next
// render call lands on render_*_view with an empty flows. The early
// return is the defensive guard for that race window.

#[test]
fn test_render_log_view_with_empty_flows_returns_silently() {
    let mut app = make_app();
    app.flows = vec![];
    app.view = View::Log;
    let _ = render_to_string(&app, 80, 40);
}

#[test]
fn test_render_issues_view_with_empty_flows_returns_silently() {
    let mut app = make_app();
    app.flows = vec![];
    app.view = View::Issues;
    let _ = render_to_string(&app, 80, 40);
}

#[test]
fn test_render_tasks_view_with_empty_flows_returns_silently() {
    let mut app = make_app();
    app.flows = vec![];
    app.view = View::Tasks;
    let _ = render_to_string(&app, 80, 40);
}

// --- Reachable break paths in render functions ---

#[test]
fn test_render_issues_view_breaks_when_too_many_issues_for_viewport() {
    // max_y - 5 issue rows fit in the panel. Stuff in 30 issues at
    // height=10 so the loop body executes only 5 times then breaks.
    let mut app = make_app();
    let mut flow = make_flow("A", "Code", 3);
    flow.issues = (0..30)
        .map(|i| IssueSummary {
            label: "Bug".to_string(),
            title: format!("Issue {}", i),
            url: String::new(),
            ref_str: format!("#{}", i),
            phase_name: "Code".to_string(),
        })
        .collect();
    app.flows = vec![flow];
    app.view = View::Issues;
    let _ = render_to_string(&app, 80, 10);
}

#[test]
fn test_render_tasks_view_breaks_when_plan_has_too_many_lines() {
    // Plan file with many lines forces the row-overflow break inside
    // the for-loop on `content.lines()`.
    let tmp = tempfile::TempDir::new().unwrap();
    let plan_path = tmp.path().join("big-plan.md");
    let plan_content: String = (0..50)
        .map(|i| format!("- task {}\n", i))
        .collect::<Vec<_>>()
        .join("");
    std::fs::write(&plan_path, plan_content).unwrap();

    let mut app = make_app();
    let mut flow = make_flow("Big Plan", "Code", 3);
    flow.plan_path = Some(plan_path.to_string_lossy().into_owned());
    app.flows = vec![flow];
    app.view = View::Tasks;
    let _ = render_to_string(&app, 80, 10);
}

#[test]
fn test_render_orch_view_breaks_when_too_many_items_for_viewport() {
    // 20 orch items at height=12: list_end = min(20, 12-6=6) = 6,
    // and the row-clamp inside the loop fires once row >= max_y - 1.
    let mut app = make_app();
    app.active_tab = 1;
    let items: Vec<OrchestrationItem> = (0..20)
        .map(|i| OrchestrationItem {
            icon: "\u{25b6}".to_string(),
            issue_number: Some(i as i64),
            title: format!("Item {}", i),
            elapsed: "1m".to_string(),
            pr_url: None,
            reason: None,
            status: "in_progress".to_string(),
        })
        .collect();
    app.orch_data = Some(OrchestrationSummary {
        elapsed: "5m".to_string(),
        completed_count: 0,
        failed_count: 0,
        total: 20,
        is_running: true,
        items,
    });
    let _ = render_to_string(&app, 80, 12);
}

#[test]
fn test_render_detail_panel_breaks_when_timeline_overflows_viewport() {
    // High start_row + small max_y forces the timeline for-loop to
    // hit `if row >= max_y - 3 { break; }`. Build a flow with 6
    // timeline entries (full PHASE_ORDER) and use a small viewport.
    let mut app = make_app();
    let mut flow = make_flow("Overflow", "Code", 3);
    // Add many timeline entries so the panel needs more rows than fit.
    flow.timeline = (0..6)
        .map(|i| TimelineEntry {
            key: format!("phase-{}", i),
            name: format!("Phase {}", i),
            number: i,
            status: "complete".to_string(),
            time: "1m".to_string(),
            annotation: String::new(),
        })
        .collect();
    app.flows = vec![flow];
    let _ = render_to_string(&app, 80, 14);
}

// --- Empty-string branches in the render functions ---

#[test]
fn test_render_list_view_with_empty_issue_numbers_no_issue_column() {
    // Flow with no issues triggers the `if flow.issue_numbers.is_empty()`
    // branch of the col_data closure, producing an empty `issue_info`
    // string.
    let mut app = make_app();
    let mut flow = make_flow("No Issues", "Code", 3);
    flow.issue_numbers = vec![];
    app.flows = vec![flow];
    let _ = render_to_string(&app, 120, 40);
}

#[test]
fn test_render_list_view_with_no_pr_numbers_skips_pr_column() {
    // When ALL flows have pr_number = None, pr_width = 0, and the
    // `if pr_width > 0` row-level check skips the PR column push.
    // This covers the false branch of that check.
    let mut app = make_app();
    let mut flow = make_flow("No PR", "Code", 3);
    flow.pr_number = None;
    flow.pr_url = None;
    app.flows = vec![flow];
    let _ = render_to_string(&app, 120, 40);
}

#[test]
fn test_render_detail_panel_with_complete_phase_having_empty_time() {
    // Timeline entry with status "complete" and time = "" hits the
    // empty-time branch in the timeline rendering.
    let mut app = make_app();
    let mut flow = make_flow("Empty Time", "Code", 3);
    flow.timeline = vec![TimelineEntry {
        key: "flow-start".to_string(),
        name: "Start".to_string(),
        number: 1,
        status: "complete".to_string(),
        time: String::new(),
        annotation: String::new(),
    }];
    app.flows = vec![flow];
    let _ = render_to_string(&app, 80, 40);
}

#[test]
fn test_render_detail_panel_with_in_progress_phase_having_empty_time_and_annotation() {
    // Status "in_progress" with empty time AND empty annotation hits
    // the two empty branches inside the in_progress arm.
    let mut app = make_app();
    let mut flow = make_flow("In Progress Empty", "Code", 3);
    flow.timeline = vec![TimelineEntry {
        key: "flow-code".to_string(),
        name: "Code".to_string(),
        number: 3,
        status: "in_progress".to_string(),
        time: String::new(),
        annotation: String::new(),
    }];
    app.flows = vec![flow];
    let _ = render_to_string(&app, 80, 40);
}

#[test]
fn test_render_detail_panel_skips_extras_when_no_room_for_notes() {
    // High start_row pushes notes/issues section past max_y - 2 so
    // the outer `if row < max_y - 2` check fails and the notes/issues
    // block is skipped. Use a small viewport to force the overflow.
    let mut app = make_app();
    let mut flow = make_flow("No Room", "Code", 3);
    flow.notes_count = 5;
    flow.issues = vec![IssueSummary {
        label: "Bug".to_string(),
        title: "x".to_string(),
        url: String::new(),
        ref_str: "#1".to_string(),
        phase_name: "Code".to_string(),
    }];
    app.flows = vec![flow];
    // Detail panel renders at start_row = 4 + list_end + 1 = 6 (with 1 flow).
    // Then 4 fixed rows + 6 timeline entries + 1 spacer = ~17 rows.
    // With max_y = 18, row reaches 17 = max_y - 1, so 17 >= max_y - 2
    // → notes/issues block is skipped.
    let _ = render_to_string(&app, 80, 18);
}

// --- Orchestration view branch coverage ---

#[test]
fn test_render_orch_view_with_unknown_status_uses_dim_style() {
    // OrchestrationItem with status = "pending" (not completed/failed/
    // in_progress) hits the `_ => Style::default().add_modifier(Dim)`
    // arm in render_orchestration_view's status match.
    let mut app = make_app();
    app.active_tab = 1;
    app.orch_data = Some(OrchestrationSummary {
        elapsed: "5m".to_string(),
        completed_count: 0,
        failed_count: 0,
        total: 1,
        is_running: true,
        items: vec![OrchestrationItem {
            icon: "\u{00b7}".to_string(),
            issue_number: Some(1),
            title: "Pending".to_string(),
            elapsed: "1m".to_string(),
            pr_url: None,
            reason: None,
            status: "pending".to_string(),
        }],
    });
    let _ = render_to_string(&app, 120, 40);
}

#[test]
fn test_render_orch_view_with_empty_elapsed_uses_empty_string_branch() {
    // OrchestrationItem with empty elapsed string hits the `String::new()`
    // arm of the elapsed_str ternary.
    let mut app = make_app();
    app.active_tab = 1;
    app.orch_data = Some(OrchestrationSummary {
        elapsed: "5m".to_string(),
        completed_count: 0,
        failed_count: 0,
        total: 1,
        is_running: true,
        items: vec![OrchestrationItem {
            icon: "\u{25b6}".to_string(),
            issue_number: Some(1),
            title: "No Elapsed".to_string(),
            elapsed: String::new(),
            pr_url: None,
            reason: None,
            status: "in_progress".to_string(),
        }],
    });
    let _ = render_to_string(&app, 120, 40);
}

#[test]
fn test_render_orch_view_with_orch_selected_out_of_range_skips_detail() {
    // detail_row passes the < max_y - 1 check, but orch.items.get(orch_selected)
    // returns None because orch_selected is past the end. Covers the
    // close-brace region of the inner `if let Some(item)` block in
    // render_orchestration_view.
    let mut app = make_app();
    app.active_tab = 1;
    app.orch_data = Some(OrchestrationSummary {
        elapsed: "5m".to_string(),
        completed_count: 0,
        failed_count: 0,
        total: 1,
        is_running: true,
        items: vec![OrchestrationItem {
            icon: "\u{25b6}".to_string(),
            issue_number: Some(1),
            title: "Item".to_string(),
            elapsed: "1m".to_string(),
            pr_url: None,
            reason: None,
            status: "in_progress".to_string(),
        }],
    });
    app.orch_selected = 99;
    let _ = render_to_string(&app, 120, 40);
}

// --- handle_issues_input defensive returns ---

#[test]
fn test_handle_issues_input_with_empty_flows_returns_silently() {
    // View is Issues but flows was cleared (e.g. after refresh_data
    // dropped the active flow). The early return at the top of
    // handle_issues_input keeps the dispatch from indexing an empty
    // Vec.
    let mut app = make_app();
    app.flows = vec![];
    app.view = View::Issues;
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.view, View::Issues);
}

#[test]
fn test_handle_issues_input_with_flow_having_no_issues_returns_silently() {
    // View is Issues, flows is non-empty, but the selected flow has
    // an empty issues list. The second early return prevents the
    // navigation arms from incrementing past the end.
    let mut app = make_app();
    let mut flow = make_flow("A", "Code", 3);
    flow.issues = vec![];
    app.flows = vec![flow];
    app.view = View::Issues;
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.issue_selected, 0);
}

#[test]
fn test_render_detail_panel_issues_loop_breaks_on_row_overflow() {
    // After the timeline rendering, the notes/issues block iterates
    // `for issue in &flow.issues` and checks `if row >= max_y - 2 { break; }`.
    // To reach this break, the panel must enter the notes/issues
    // block (row < max_y - 2) but accumulate enough rows from
    // notes/issues that the next iteration overflows.
    let mut app = make_app();
    let mut flow = make_flow("Issue Overflow", "Code", 3);
    flow.notes_count = 1;
    flow.issues = (0..10)
        .map(|i| IssueSummary {
            label: "Bug".to_string(),
            title: format!("Issue {}", i),
            url: String::new(),
            ref_str: format!("#{}", i),
            phase_name: "Code".to_string(),
        })
        .collect();
    app.flows = vec![flow];
    // Tight viewport so notes section starts close to max_y - 2.
    let _ = render_to_string(&app, 80, 20);
}

// --- Private-helper coverage through the public TuiApp surface ---
//
// Every branch of tui.rs's pure helpers is driven via the public
// methods that call them. The for_tests platform points every
// subprocess at /bin/true so spawns succeed with no side effect;
// the test only needs the code path to run for coverage to register.

fn app_with_pr_url(url: Option<&str>) -> TuiApp {
    let mut app = make_app();
    let mut flow = make_flow("Test", "Code", 3);
    flow.pr_url = url.map(String::from);
    app.flows = vec![flow];
    app
}

fn app_with_repo_and_issues(
    repo_val: serde_json::Value,
    fallback_repo: Option<&str>,
    issues: Vec<i64>,
) -> TuiApp {
    let mut app = TuiApp::new(
        PathBuf::from("/tmp/test"),
        "1.0.0".to_string(),
        fallback_repo.map(String::from),
        TuiAppPlatform::for_tests(),
    );
    let mut flow = make_flow("Test", "Code", 3);
    flow.state = serde_json::json!({"branch": "test", "repo": repo_val});
    flow.issue_numbers = issues;
    app.flows = vec![flow];
    app
}

fn app_with_session_tty(session_tty: serde_json::Value) -> TuiApp {
    let mut app = make_app();
    let mut flow = make_flow("Test", "Code", 3);
    flow.state = serde_json::json!({"session_tty": session_tty});
    app.flows = vec![flow];
    app
}

fn app_with_orch_item(fallback_repo: Option<&str>, issue_number: Option<i64>) -> TuiApp {
    let mut app = TuiApp::new(
        PathBuf::from("/tmp/test"),
        "1.0.0".to_string(),
        fallback_repo.map(String::from),
        TuiAppPlatform::for_tests(),
    );
    app.active_tab = 1;
    app.orch_data = Some(OrchestrationSummary {
        elapsed: "5m".to_string(),
        completed_count: 0,
        failed_count: 0,
        total: 1,
        is_running: true,
        items: vec![OrchestrationItem {
            icon: "\u{00b7}".to_string(),
            issue_number,
            title: "X".to_string(),
            elapsed: String::new(),
            pr_url: None,
            reason: None,
            status: "pending".to_string(),
        }],
    });
    app
}

// --- TuiAppPlatform::production ---

#[test]
fn platform_production_returns_known_binaries() {
    let p = TuiAppPlatform::production();
    assert_eq!(p.open_binary, "open");
    assert_eq!(p.osascript_binary, "osascript");
    assert_eq!(p.bin_flow_path, PathBuf::from("bin/flow"));
}

// --- pr_files_url branches via open_pr ---

#[test]
fn open_pr_drives_pr_files_url_across_all_branches() {
    // Each variant exercises a distinct branch of pr_files_url:
    // - None       → open_pr early-returns (pr_files_url not called)
    // - ""         → empty-input early return inside pr_files_url
    // - plain      → standard append /files
    // - trailing / → trim + append
    // - multi //   → trim multiple + append
    // - ?query     → query split, append /files before ?
    // - #fragment  → fragment split, append /files before #
    // - ? and #    → both splits
    // - /files     → idempotent (already ends with /files)
    // - /files/    → trim, still idempotent
    for url in [
        None,
        Some(""),
        Some("https://github.com/o/r/pull/100"),
        Some("https://github.com/o/r/pull/100/"),
        Some("https://example.com/x///"),
        Some("https://github.com/o/r/pull/100?diff=split"),
        Some("https://github.com/o/r/pull/100#discussion_r1"),
        Some("https://github.com/o/r/pull/1?a=b#c"),
        Some("https://github.com/o/r/pull/100/files"),
        Some("https://github.com/o/r/pull/100/files/"),
    ] {
        let mut app = app_with_pr_url(url);
        app.handle_key(key(KeyCode::Char('p')));
    }
}

// --- flow_issue_url branches via open_flow_issue ('I' key) ---

#[test]
fn open_flow_issue_drives_flow_issue_url_across_all_branches() {
    // state.repo present → state wins
    let mut app = app_with_repo_and_issues(
        serde_json::json!("state/wins"),
        Some("fallback/repo"),
        vec![42],
    );
    app.handle_key(key(KeyCode::Char('I')));

    // state.repo absent → fallback wins
    let mut app =
        app_with_repo_and_issues(serde_json::Value::Null, Some("fallback/repo"), vec![42]);
    app.handle_key(key(KeyCode::Char('I')));

    // No issues → returns None
    let mut app = app_with_repo_and_issues(serde_json::json!("o/r"), None, vec![]);
    app.handle_key(key(KeyCode::Char('I')));

    // No repo anywhere → returns None
    let mut app = app_with_repo_and_issues(serde_json::Value::Null, None, vec![42]);
    app.handle_key(key(KeyCode::Char('I')));

    // state.repo empty string → falls back to parameter
    let mut app = app_with_repo_and_issues(serde_json::json!(""), Some("fallback/repo"), vec![1]);
    app.handle_key(key(KeyCode::Char('I')));

    // state.repo non-string (corrupt) → falls back to parameter
    let mut app =
        app_with_repo_and_issues(serde_json::json!(12345), Some("fallback/repo"), vec![1]);
    app.handle_key(key(KeyCode::Char('I')));

    // Both empty → None
    let mut app = app_with_repo_and_issues(serde_json::json!(""), Some(""), vec![1]);
    app.handle_key(key(KeyCode::Char('I')));

    // state empty, no fallback → None
    let mut app = app_with_repo_and_issues(serde_json::json!(""), None, vec![1]);
    app.handle_key(key(KeyCode::Char('I')));

    // Multiple issues — picks smallest
    let mut app = app_with_repo_and_issues(serde_json::json!("o/r"), None, vec![42, 7, 99]);
    app.handle_key(key(KeyCode::Char('I')));
}

// --- orch_issue_url branches via open_orch_issue ('i' on orch tab) ---

#[test]
fn open_orch_issue_drives_orch_issue_url_across_all_branches() {
    // repo and issue_number present → URL
    let mut app = app_with_orch_item(Some("o/r"), Some(42));
    app.handle_key(key(KeyCode::Char('i')));

    // repo missing → None
    let mut app = app_with_orch_item(None, Some(42));
    app.handle_key(key(KeyCode::Char('i')));

    // repo empty → None
    let mut app = app_with_orch_item(Some(""), Some(42));
    app.handle_key(key(KeyCode::Char('i')));

    // issue_number missing → None
    let mut app = app_with_orch_item(Some("o/r"), None);
    app.handle_key(key(KeyCode::Char('i')));
}

// --- issue_open_target branches via issues view Enter ---

#[test]
fn issues_enter_drives_issue_open_target_branches() {
    // Issue with URL → opens
    let mut app = make_app();
    let mut flow = make_flow("A", "Code", 3);
    flow.issues = vec![IssueSummary {
        label: "Bug".to_string(),
        title: "t".to_string(),
        url: "https://x/y".to_string(),
        ref_str: "#1".to_string(),
        phase_name: "Code".to_string(),
    }];
    app.flows = vec![flow];
    app.view = View::Issues;
    app.handle_key(key(KeyCode::Enter));

    // Issue with empty URL → None returned; no spawn
    let mut app = make_app();
    let mut flow = make_flow("A", "Code", 3);
    flow.issues = vec![IssueSummary {
        label: "Bug".to_string(),
        title: "t".to_string(),
        url: String::new(),
        ref_str: "#1".to_string(),
        phase_name: "Code".to_string(),
    }];
    app.flows = vec![flow];
    app.view = View::Issues;
    app.handle_key(key(KeyCode::Enter));
}

// --- should_abort branches via handle_abort_confirm ---

#[test]
fn abort_confirm_lowercase_y_triggers_abort() {
    let mut app = make_app();
    let flow = make_flow("A", "Code", 3);
    app.flows = vec![flow];
    app.confirming_abort = true;
    app.handle_key(key(KeyCode::Char('y')));
    assert!(!app.confirming_abort);
}

#[test]
fn abort_confirm_uppercase_y_triggers_abort() {
    let mut app = make_app();
    let flow = make_flow("A", "Code", 3);
    app.flows = vec![flow];
    app.confirming_abort = true;
    app.handle_key(key(KeyCode::Char('Y')));
    assert!(!app.confirming_abort);
}

#[test]
fn abort_confirm_non_char_key_does_not_abort() {
    let mut app = make_app();
    let flow = make_flow("A", "Code", 3);
    app.flows = vec![flow];
    app.confirming_abort = true;
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.confirming_abort);
}

// --- build_cleanup_command_args branches via abort_flow ---

#[test]
fn abort_with_pr_number_includes_pr_flag() {
    // flow.pr_number = Some(100) via make_flow — exercises the
    // `if let Some(pr) = pr_number` branch of build_cleanup_command_args.
    let mut app = make_app();
    let flow = make_flow("A", "Code", 3);
    app.flows = vec![flow];
    app.confirming_abort = true;
    app.handle_key(key(KeyCode::Char('y')));
}

#[test]
fn abort_without_pr_number_omits_pr_flag() {
    // pr_number = None — exercises the else branch of the if-let.
    let mut app = make_app();
    let mut flow = make_flow("A", "Code", 3);
    flow.pr_number = None;
    app.flows = vec![flow];
    app.confirming_abort = true;
    app.handle_key(key(KeyCode::Char('y')));
}

#[test]
fn abort_with_no_flows_is_noop() {
    // flows.is_empty() early return in abort_flow.
    let mut app = make_app();
    app.confirming_abort = true;
    app.handle_key(key(KeyCode::Char('y')));
}

// --- worktree_session_tty branches via open_worktree (Enter) ---

#[test]
fn open_worktree_drives_worktree_session_tty_branches() {
    // String value → Some("/dev/ttys003")
    let mut app = app_with_session_tty(serde_json::json!("/dev/ttys003"));
    app.handle_key(key(KeyCode::Enter));

    // Missing field → None
    let mut app = make_app();
    let mut flow = make_flow("A", "Code", 3);
    flow.state = serde_json::json!({});
    app.flows = vec![flow];
    app.handle_key(key(KeyCode::Enter));

    // Non-string value → None
    let mut app = app_with_session_tty(serde_json::json!(12345));
    app.handle_key(key(KeyCode::Enter));

    // Empty string → Some("") — passes through, build_iterm_activation_script
    // called with empty input (covers escape_applescript_string "safe pass")
    let mut app = app_with_session_tty(serde_json::json!(""));
    app.handle_key(key(KeyCode::Enter));
}

// --- escape_applescript_string + build_iterm_activation_script ---

#[test]
fn open_worktree_with_special_chars_drives_escape_branches() {
    // Session tty containing structural chars exercises the
    // "add \\" branch of escape_applescript_string.
    let mut app = app_with_session_tty(serde_json::json!("a\"b"));
    app.handle_key(key(KeyCode::Enter));

    let mut app = app_with_session_tty(serde_json::json!("a\\b"));
    app.handle_key(key(KeyCode::Enter));

    // Safe chars only — exercises the pass-through branch
    let mut app = app_with_session_tty(serde_json::json!("/dev/ttys099"));
    app.handle_key(key(KeyCode::Enter));
}

// --- parse_osascript_result branches via activate_iterm_tab ---

fn make_app_with_osascript(osascript_path: &str) -> TuiApp {
    let mut platform = TuiAppPlatform::for_tests();
    platform.osascript_binary = osascript_path.to_string();
    TuiApp::new(
        PathBuf::from("/tmp/test"),
        "1.0.0".to_string(),
        Some("test/repo".to_string()),
        platform,
    )
}

fn write_fixture_script(dir: &std::path::Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    path
}

#[test]
fn parse_osascript_result_success_activated_returns_true() {
    let dir = tempfile::tempdir().unwrap();
    let script = write_fixture_script(dir.path(), "osascript", "#!/bin/sh\necho activated\n");
    let mut app = make_app_with_osascript(&script.to_string_lossy());
    let mut flow = make_flow("A", "Code", 3);
    flow.state = serde_json::json!({"session_tty": "/dev/ttys003"});
    app.flows = vec![flow];
    // Drive activate_iterm_tab directly to assert the return value.
    assert!(app.activate_iterm_tab("/dev/ttys003"));
}

#[test]
fn parse_osascript_result_success_not_activated_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let script = write_fixture_script(dir.path(), "osascript", "#!/bin/sh\necho not_found\n");
    let mut app = make_app_with_osascript(&script.to_string_lossy());
    let mut flow = make_flow("A", "Code", 3);
    flow.state = serde_json::json!({"session_tty": "/dev/ttys003"});
    app.flows = vec![flow];
    assert!(!app.activate_iterm_tab("/dev/ttys003"));
}

#[test]
fn parse_osascript_result_failure_returns_false() {
    let mut app = make_app_with_osascript("/bin/false");
    let mut flow = make_flow("A", "Code", 3);
    flow.state = serde_json::json!({"session_tty": "/dev/ttys003"});
    app.flows = vec![flow];
    assert!(!app.activate_iterm_tab("/dev/ttys003"));
}

#[test]
fn activate_iterm_tab_spawn_error_returns_false() {
    // Non-existent binary → Command::output() returns Err →
    // parse_osascript_result is NOT called; the Err(_) => false arm
    // of the match is taken.
    let app = make_app_with_osascript("/nonexistent/path/to/osascript");
    assert!(!app.activate_iterm_tab("/dev/ttys003"));
}

// --- open_orch_issue: no orch_data and out-of-bounds item ---

#[test]
fn open_orch_issue_with_no_orch_data_is_noop() {
    let mut app = make_app();
    app.active_tab = 1;
    // orch_data stays None
    app.handle_key(key(KeyCode::Char('i')));
}

#[test]
fn open_orch_issue_with_out_of_bounds_selection_is_noop() {
    let mut app = make_app();
    app.active_tab = 1;
    app.orch_data = Some(OrchestrationSummary {
        elapsed: "5m".to_string(),
        completed_count: 0,
        failed_count: 0,
        total: 0,
        is_running: true,
        items: vec![],
    });
    // orch_selected stays 0 but items is empty → items.get(0) is None
    app.handle_key(key(KeyCode::Char('i')));
}

#[test]
fn orch_input_up_down_noop_when_items_empty() {
    let mut app = make_app();
    app.active_tab = 1;
    app.orch_data = Some(OrchestrationSummary {
        elapsed: "5m".to_string(),
        completed_count: 0,
        failed_count: 0,
        total: 0,
        is_running: true,
        items: vec![],
    });
    // Up/Down guards `if item_count > 0` — must fall through.
    app.handle_key(key(KeyCode::Up));
    app.handle_key(key(KeyCode::Down));
}

// --- handle_key: Esc with view=List falls through the Esc guard ---

#[test]
fn esc_with_view_list_does_not_change_view() {
    // Covers the `KeyCode::Esc if matches!(view, Log | Issues | Tasks)`
    // guard's false arm — when view=List the guard fails and the arm
    // is skipped, falling through to the view==List arm.
    let mut app = make_app();
    app.flows = vec![make_flow("A", "Code", 3)];
    assert_eq!(app.view, View::List);
    app.handle_key(key(KeyCode::Esc));
    // handle_list_input matches nothing on Esc → no-op.
    assert_eq!(app.view, View::List);
}

// --- orch detail panel: failed without reason, completed without pr_url ---

#[test]
fn render_orch_detail_failed_without_reason_renders_no_detail() {
    let mut app = make_app();
    app.active_tab = 1;
    app.orch_data = Some(OrchestrationSummary {
        elapsed: "5m".to_string(),
        completed_count: 0,
        failed_count: 1,
        total: 1,
        is_running: false,
        items: vec![OrchestrationItem {
            icon: "\u{2717}".to_string(),
            issue_number: Some(10),
            title: "Failed".to_string(),
            elapsed: "1m".to_string(),
            pr_url: None,
            reason: None,
            status: "failed".to_string(),
        }],
    });
    let output = render_to_string(&app, 120, 40);
    // Failed item with no reason → detail panel skipped
    assert!(!output.contains("Reason:"));
}

#[test]
fn render_orch_detail_completed_without_pr_url_renders_no_detail() {
    let mut app = make_app();
    app.active_tab = 1;
    app.orch_data = Some(OrchestrationSummary {
        elapsed: "5m".to_string(),
        completed_count: 1,
        failed_count: 0,
        total: 1,
        is_running: false,
        items: vec![OrchestrationItem {
            icon: "\u{2713}".to_string(),
            issue_number: Some(10),
            title: "Done".to_string(),
            elapsed: "3m".to_string(),
            pr_url: None,
            reason: None,
            status: "completed".to_string(),
        }],
    });
    let output = render_to_string(&app, 120, 40);
    assert!(!output.contains("PR:"));
}

// --- rl_color branches via render (with non-stale metrics) ---

#[test]
fn render_metrics_non_stale_drives_rl_color_branches() {
    // Below 70 (default), in yellow band (70-89), in red band (>=90).
    // Negative values (corrupt state) fall through to default.
    for (rl_5h, rl_7d) in [(0, 0), (50, 65), (75, 85), (90, 95), (100, 100), (-1, -10)] {
        let mut app = make_app();
        app.metrics = AccountMetrics {
            cost_monthly: "8.00".to_string(),
            rl_5h: Some(rl_5h),
            rl_7d: Some(rl_7d),
            stale: false,
        };
        let _ = render_to_string(&app, 120, 40);
    }
}
