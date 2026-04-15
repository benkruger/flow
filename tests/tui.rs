//! Tests for the interactive TUI (src/tui.rs).
//!
//! Uses ratatui's TestBackend for rendering assertions and
//! direct state manipulation for input handling tests.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use flow_rs::tui::{TuiApp, TuiAppPlatform, View};
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
    // Write a valid log file at .flow-states/<branch>.log so the Log
    // view hits the entries-iteration branch.
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join(".flow-states")).unwrap();
    let log_content = "2026-01-01T12:34:56-08:00 [Phase 1] start-init — initializing (ok)\n";
    std::fs::write(
        root.join(".flow-states").join("test-feature.log"),
        log_content,
    )
    .unwrap();

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
    std::fs::write(
        root.join(".flow-states").join("test-feature.json"),
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
    // /bin/true. `/bin/true -e "<script>"` runs without panic and
    // returns success with empty stdout. `parse_osascript_result`
    // then returns false (because "" != "activated"). The whole
    // Command::new(...).output() chain in
    // `TuiApp::activate_iterm_tab` runs for real.
    let app = make_app();
    let result = app.activate_iterm_tab("/dev/ttys000");
    // /bin/true exits 0 with empty stdout; parse_osascript_result
    // returns false because stdout is not "activated".
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

/// Build a fake event source closure that pops events from a queue.
/// Returns `None` when the queue is empty (simulating a timeout).
fn fake_event_source(
    events: VecDeque<Option<Event>>,
) -> impl FnMut(Duration) -> std::io::Result<Option<Event>> {
    let mut queue = events;
    move |_timeout| Ok(queue.pop_front().unwrap_or(None))
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
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let events = fake_event_source(VecDeque::from(vec![Some(key_event(KeyCode::Char('q')))]));
    let result = app.run_event_loop(&mut terminal, events);
    assert!(result.is_ok());
    assert!(!app.running);
}

#[test]
fn test_run_event_loop_handles_resize_then_quit() {
    // Queue a resize event (which triggers refresh_data) and then a
    // `q` keypress to exit. Covers the Event::Resize arm.
    let mut app = make_app();
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let events = fake_event_source(VecDeque::from(vec![
        Some(Event::Resize(100, 30)),
        Some(key_event(KeyCode::Char('q'))),
    ]));
    let result = app.run_event_loop(&mut terminal, events);
    assert!(result.is_ok());
}

#[test]
fn test_run_event_loop_handles_timeout_then_quit() {
    // Queue None (timeout → refresh_data) then `q`. Covers the None
    // arm in the match.
    let mut app = make_app();
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let events = fake_event_source(VecDeque::from(vec![
        None,
        Some(key_event(KeyCode::Char('q'))),
    ]));
    let result = app.run_event_loop(&mut terminal, events);
    assert!(result.is_ok());
}

#[test]
fn test_run_event_loop_handles_mouse_event_then_quit() {
    // Queue an unhandled event variant (Event::FocusGained) then `q`.
    // Covers the Some(_) catchall arm.
    let mut app = make_app();
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let events = fake_event_source(VecDeque::from(vec![
        Some(Event::FocusGained),
        Some(key_event(KeyCode::Char('q'))),
    ]));
    let result = app.run_event_loop(&mut terminal, events);
    assert!(result.is_ok());
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
