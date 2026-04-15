//! Interactive TUI for viewing and managing active FLOW features.
//!
//! A ratatui-based terminal application that reads local state files and
//! provides keyboard-driven navigation. No Claude session required.
//! Uses tui_data module for data loading.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use ratatui::Terminal;

use serde_json::Value;

use crate::flow_paths::FlowPaths;
use crate::tui_data::{self, AccountMetrics, FlowSummary, OrchestrationSummary};

/// Auto-refresh interval.
const REFRESH_MS: u64 = 2000;

/// Active view in the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    List,
    Log,
    Issues,
    Tasks,
}

/// The main TUI application state.
pub struct TuiApp {
    pub root: PathBuf,
    pub version: String,
    pub repo: Option<String>,
    pub repo_name: Option<String>,
    pub flows: Vec<FlowSummary>,
    pub selected: usize,
    pub view: View,
    pub running: bool,
    pub confirming_abort: bool,
    pub active_tab: usize,
    pub orch_data: Option<OrchestrationSummary>,
    pub orch_selected: usize,
    pub issue_selected: usize,
    pub metrics: AccountMetrics,
}

impl TuiApp {
    /// Create a new TuiApp with the given root directory.
    pub fn new(root: PathBuf, version: String, repo: Option<String>) -> Self {
        let repo_name = repo
            .as_ref()
            .map(|r| r.rsplit('/').next().unwrap_or(r.as_str()).to_string());
        Self {
            root,
            version,
            repo,
            repo_name,
            flows: Vec::new(),
            selected: 0,
            view: View::List,
            running: true,
            confirming_abort: false,
            active_tab: 0,
            orch_data: None,
            orch_selected: 0,
            issue_selected: 0,
            metrics: AccountMetrics {
                cost_monthly: String::new(),
                rl_5h: None,
                rl_7d: None,
                stale: true,
            },
        }
    }

    /// Reload all data from state files.
    pub fn refresh_data(&mut self) {
        self.flows = tui_data::load_all_flows(&self.root);
        if self.selected >= self.flows.len() {
            self.selected = self.flows.len().saturating_sub(1);
        }
        let orch_state = tui_data::load_orchestration(&self.root);
        self.orch_data = tui_data::orchestration_summary(orch_state.as_ref(), None);
        if let Some(ref orch) = self.orch_data {
            if self.orch_selected >= orch.items.len() {
                self.orch_selected = orch.items.len().saturating_sub(1);
            }
        }
        self.metrics = tui_data::load_account_metrics(&self.root, None);
    }

    /// Run the TUI event loop with a real terminal.
    ///
    /// Terminal cleanup (raw mode + alternate screen) is guaranteed even
    /// on error via an explicit cleanup call before returning.
    pub fn run_terminal(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let result = self.run_event_loop(&mut terminal);

        // Guaranteed cleanup: restore terminal even on error
        let _ = disable_raw_mode();
        let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);

        result
    }

    /// Inner event loop — separated so run_terminal can guarantee cleanup.
    fn run_event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> io::Result<()> {
        self.refresh_data();

        while self.running {
            terminal.draw(|f| self.render(f))?;

            if event::poll(Duration::from_millis(REFRESH_MS))? {
                match event::read()? {
                    Event::Key(key) => self.handle_key(key),
                    Event::Resize(_, _) => self.refresh_data(),
                    _ => {}
                }
            } else {
                // Timeout — refresh data
                self.refresh_data();
            }
        }

        Ok(())
    }

    /// Render the current view into a frame.
    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        if self.active_tab == 1 {
            self.render_orchestration_view(frame, area);
        } else {
            match self.view {
                View::List => self.render_list_view(frame, area),
                View::Log => self.render_log_view(frame, area),
                View::Issues => self.render_issues_view(frame, area),
                View::Tasks => self.render_tasks_view(frame, area),
            }
        }
    }

    /// Handle a key event and update state.
    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.confirming_abort {
            self.handle_abort_confirm(key);
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.running = false,
            KeyCode::Right => self.active_tab = 1.min(self.active_tab + 1),
            KeyCode::Left => self.active_tab = self.active_tab.saturating_sub(1),
            KeyCode::Esc if matches!(self.view, View::Log | View::Issues | View::Tasks) => {
                self.view = View::List;
            }
            _ if self.active_tab == 1 => self.handle_orch_input(key),
            _ if self.view == View::Issues => self.handle_issues_input(key),
            _ if self.view == View::Tasks => {}
            _ if self.view == View::List => self.handle_list_input(key),
            _ => {}
        }
    }

    // --- List view input ---

    fn handle_list_input(&mut self, key: KeyEvent) {
        if self.flows.is_empty() {
            return;
        }
        match key.code {
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                self.issue_selected = 0;
            }
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(self.flows.len().saturating_sub(1));
                self.issue_selected = 0;
            }
            KeyCode::Enter => self.open_worktree(),
            KeyCode::Char('p') => self.open_pr(),
            KeyCode::Char('l') => self.view = View::Log,
            KeyCode::Char('i') => self.view = View::Issues,
            KeyCode::Char('I') => self.open_flow_issue(),
            KeyCode::Char('t') => self.view = View::Tasks,
            KeyCode::Char('a') => self.confirming_abort = true,
            KeyCode::Char('r') => self.refresh_data(),
            _ => {}
        }
    }

    fn handle_issues_input(&mut self, key: KeyEvent) {
        if self.flows.is_empty() {
            return;
        }
        let flow = &self.flows[self.selected];
        let issue_count = flow.issues.len();
        if issue_count == 0 {
            return;
        }
        match key.code {
            KeyCode::Up => self.issue_selected = self.issue_selected.saturating_sub(1),
            KeyCode::Down => {
                self.issue_selected = (self.issue_selected + 1).min(issue_count.saturating_sub(1));
            }
            KeyCode::Enter => {
                if let Some(issue) = flow.issues.get(self.issue_selected) {
                    if !issue.url.is_empty() {
                        open_url(&issue.url);
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_orch_input(&mut self, key: KeyEvent) {
        let item_count = self.orch_data.as_ref().map(|o| o.items.len()).unwrap_or(0);

        match key.code {
            KeyCode::Up if item_count > 0 => {
                self.orch_selected = self.orch_selected.saturating_sub(1);
            }
            KeyCode::Down if item_count > 0 => {
                self.orch_selected = (self.orch_selected + 1).min(item_count.saturating_sub(1));
            }
            KeyCode::Char('i') => self.open_orch_issue(),
            KeyCode::Char('r') => self.refresh_data(),
            _ => {}
        }
    }

    fn handle_abort_confirm(&mut self, key: KeyEvent) {
        self.confirming_abort = false;
        if should_abort(key.code) {
            self.abort_flow();
        }
    }

    // --- Actions ---

    fn open_worktree(&self) {
        if self.flows.is_empty() {
            return;
        }
        let flow = &self.flows[self.selected];
        if let Some(tty) = worktree_session_tty(flow) {
            activate_iterm_tab(tty);
        }
    }

    fn open_pr(&self) {
        if self.flows.is_empty() {
            return;
        }
        let flow = &self.flows[self.selected];
        if let Some(ref url) = flow.pr_url {
            let files_url = pr_files_url(url);
            open_url(&files_url);
        }
    }

    fn open_flow_issue(&self) {
        if self.flows.is_empty() {
            return;
        }
        let flow = &self.flows[self.selected];
        if let Some(url) = flow_issue_url(&flow.state, self.repo.as_deref(), &flow.issue_numbers) {
            open_url(&url);
        }
    }

    fn open_orch_issue(&self) {
        if let Some(ref orch) = self.orch_data {
            if let Some(item) = orch.items.get(self.orch_selected) {
                if let Some(url) = orch_issue_url(self.repo.as_deref(), item.issue_number) {
                    open_url(&url);
                }
            }
        }
    }

    fn abort_flow(&mut self) {
        if self.flows.is_empty() {
            return;
        }
        let flow = &self.flows[self.selected];
        let args =
            build_cleanup_command_args(&self.root, &flow.branch, &flow.worktree, flow.pr_number);

        // Find bin/flow relative to this binary
        let bin_flow = find_bin_flow();

        // Exit alternate screen for cleanup output
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);

        eprintln!("Aborting flow: {}...", flow.feature);
        let _ = Command::new(&bin_flow).args(&args).status();

        // Re-enter alternate screen
        let _ = enable_raw_mode();
        let _ = execute!(io::stdout(), EnterAlternateScreen);

        self.refresh_data();
    }

    // --- Rendering ---

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let width = area.width as usize;

        // Row 0: border with version and repo
        let version_text = format!(" FLOW v{} ", self.version);
        let prefix_border: String = "\u{2500}".repeat(2.min(width));
        let mut spans = vec![
            Span::styled(prefix_border, Style::default().add_modifier(Modifier::DIM)),
            Span::styled(
                version_text.clone(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ];
        if let Some(ref name) = self.repo_name {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                name.to_uppercase(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        // Fill remaining with border
        let used: usize =
            2 + version_text.len() + self.repo_name.as_ref().map(|n| n.len() + 1).unwrap_or(0);
        if used < width {
            let suffix_border: String = "\u{2500}".repeat(width - used);
            spans.push(Span::styled(
                suffix_border,
                Style::default().add_modifier(Modifier::DIM),
            ));
        }

        // Metrics on the right side of row 0
        let metrics_spans = self.render_metrics_spans(width);

        let header_line = Line::from(spans);
        let header = Paragraph::new(header_line);
        let header_area = Rect::new(area.x, area.y, area.width, 1);
        frame.render_widget(header, header_area);

        // Render metrics on same row if they fit
        if !metrics_spans.is_empty() {
            let metrics_width: usize = metrics_spans.iter().map(|s| s.width()).sum();
            if metrics_width + 30 < width {
                let col = (width - metrics_width - 2) as u16;
                let metrics_line = Line::from(metrics_spans);
                let metrics_p = Paragraph::new(metrics_line);
                let metrics_area = Rect::new(area.x + col, area.y, metrics_width as u16 + 2, 1);
                frame.render_widget(metrics_p, metrics_area);
            }
        }

        // Row 2: tab bar
        self.render_tab_bar(frame, Rect::new(area.x, area.y + 2, area.width, 1));

        // Row 3: separator
        let sep: String = format!("  {}", "\u{2500}".repeat(width.saturating_sub(4)));
        let sep_line = Paragraph::new(Line::from(Span::styled(
            sep,
            Style::default().add_modifier(Modifier::DIM),
        )));
        frame.render_widget(sep_line, Rect::new(area.x, area.y + 3, area.width, 1));
    }

    fn render_metrics_spans(&self, max_x: usize) -> Vec<Span<'static>> {
        if self.metrics.cost_monthly.is_empty() {
            return vec![];
        }
        let cost_text = format!("${}/mo", self.metrics.cost_monthly);
        if self.metrics.stale {
            let rl_text = "5h:--  7d:--".to_string();
            let total_width = cost_text.len() + 2 + rl_text.len() + 2;
            if total_width > max_x.saturating_sub(30) {
                return vec![];
            }
            vec![
                Span::styled(cost_text, Style::default().add_modifier(Modifier::DIM)),
                Span::raw("  "),
                Span::styled(rl_text, Style::default().add_modifier(Modifier::DIM)),
            ]
        } else {
            let rl_5h_text = format!("5h:{}%", self.metrics.rl_5h.unwrap_or(0));
            let rl_7d_text = format!("7d:{}%", self.metrics.rl_7d.unwrap_or(0));
            let total_width = cost_text.len() + 2 + rl_5h_text.len() + 2 + rl_7d_text.len() + 2;
            if total_width > max_x.saturating_sub(30) {
                return vec![];
            }
            vec![
                Span::styled(cost_text, Style::default().add_modifier(Modifier::DIM)),
                Span::raw("  "),
                Span::styled(rl_5h_text, rl_color(self.metrics.rl_5h.unwrap_or(0))),
                Span::raw("  "),
                Span::styled(rl_7d_text, rl_color(self.metrics.rl_7d.unwrap_or(0))),
            ]
        }
    }

    fn render_tab_bar(&self, frame: &mut Frame, area: Rect) {
        let flows_label = format!("Active Flows ({})", self.flows.len());
        let orch_label = if let Some(ref orch) = self.orch_data {
            if orch.is_running {
                let processed = orch.completed_count + orch.failed_count;
                format!("Orchestration ({}/{})", processed, orch.total)
            } else {
                "Orchestration".to_string()
            }
        } else {
            "Orchestration".to_string()
        };

        let active_style = Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD);
        let inactive_style = Style::default().add_modifier(Modifier::DIM);

        let flows_style = if self.active_tab == 0 {
            active_style
        } else {
            inactive_style
        };
        let orch_style = if self.active_tab == 1 {
            active_style
        } else {
            inactive_style
        };

        let tab_line = Line::from(vec![
            Span::raw("  "),
            Span::styled(flows_label, flows_style),
            Span::raw("  "),
            Span::styled("\u{2502}", Style::default().add_modifier(Modifier::DIM)),
            Span::raw("  "),
            Span::styled(orch_label, orch_style),
        ]);
        frame.render_widget(Paragraph::new(tab_line), area);
    }

    fn render_list_view(&self, frame: &mut Frame, area: Rect) {
        self.render_header(frame, area);

        if self.flows.is_empty() {
            let msg = Paragraph::new(Line::from("  No active flows."));
            frame.render_widget(msg, Rect::new(area.x, area.y + 4, area.width, 1));
            let hint = Paragraph::new(Line::from(
                "  Start a flow with: /flow:flow-start <feature>",
            ));
            frame.render_widget(hint, Rect::new(area.x, area.y + 6, area.width, 1));
            self.render_list_footer(frame, area);
            return;
        }

        let max_y = area.height as usize;
        let max_x = area.width as usize;

        // Cross-tab indicator
        let orch_issue = self.get_orch_issue_in_progress();

        let list_end = self.flows.len().min(max_y.saturating_sub(18));

        // Pre-compute column data
        let col_data: Vec<(String, String, String, String, String)> = self.flows[..list_end]
            .iter()
            .map(|flow| {
                let mut phase_info = format!("{}: {}", flow.phase_number, flow.phase_name);
                if !flow.annotation.is_empty() {
                    phase_info.push_str(&format!(" ({})", flow.annotation));
                }
                let pr_info = flow
                    .pr_number
                    .map(|n| format!("PR #{}", n))
                    .unwrap_or_default();
                let issue_info = if flow.issue_numbers.is_empty() {
                    String::new()
                } else {
                    let mut nums: Vec<i64> = flow.issue_numbers.clone();
                    nums.sort();
                    nums.iter()
                        .map(|n| format!("#{}", n))
                        .collect::<Vec<_>>()
                        .join(" ")
                };
                let elapsed_display = if flow.blocked {
                    "Blocked".to_string()
                } else {
                    flow.elapsed.clone()
                };
                let phase_elapsed_display = if flow.blocked {
                    String::new()
                } else {
                    flow.phase_elapsed.clone()
                };
                (
                    phase_info,
                    elapsed_display,
                    phase_elapsed_display,
                    issue_info,
                    pr_info,
                )
            })
            .collect();

        let phase_width = col_data
            .iter()
            .map(|d| d.0.len())
            .max()
            .unwrap_or(14)
            .max(14);
        let issue_width = col_data.iter().map(|d| d.3.len()).max().unwrap_or(0);
        let pr_width = col_data.iter().map(|d| d.4.len()).max().unwrap_or(0);

        let mut overhead = 2 + 2 + 3 + phase_width + 3 + 5 + 3 + 7 + 3 + 2;
        if issue_width > 0 {
            overhead += issue_width + 3;
        }
        if pr_width > 0 {
            overhead += pr_width + 2;
        }
        let feature_width = 26usize.max(max_x.saturating_sub(overhead));

        // Column header at row 3
        let mut hdr = format!(
            "  {:fw$}   {:pw$}   {:>5}   {:>7}",
            "Feature",
            "Phase",
            "",
            "Total",
            fw = feature_width,
            pw = phase_width,
        );
        if issue_width > 0 {
            hdr.push_str(&format!("   {:iw$}", "Issue", iw = issue_width));
        }
        if pr_width > 0 {
            hdr.push_str(&format!("  {:prw$}", "PR", prw = pr_width));
        }
        let hdr_line = Paragraph::new(Line::from(Span::styled(
            hdr,
            Style::default().add_modifier(Modifier::DIM),
        )));
        frame.render_widget(hdr_line, Rect::new(area.x, area.y + 3, area.width, 1));

        // Flow rows
        for (i, flow) in self.flows.iter().enumerate().take(list_end) {
            let row = 4 + i;
            if row >= max_y.saturating_sub(1) {
                break;
            }

            let marker = if i == self.selected {
                "\u{25b8} "
            } else if orch_issue.is_some_and(|n| flow.issue_numbers.contains(&n)) {
                "\u{25c6} "
            } else {
                "  "
            };

            let mut style = if i == self.selected {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            if flow.blocked {
                style = style.fg(Color::Red);
            }

            let (
                ref phase_info,
                ref elapsed_display,
                ref phase_elapsed,
                ref issue_info,
                ref pr_info,
            ) = col_data[i];

            let feature_display = if flow.feature.chars().count() > feature_width {
                let truncated: String = flow
                    .feature
                    .chars()
                    .take(feature_width.saturating_sub(3))
                    .collect();
                format!("{}...", truncated)
            } else {
                flow.feature.clone()
            };

            let mut line_str = format!(
                "{}{:fw$}   {:pw$}   {:>5}   {:>7}",
                marker,
                feature_display,
                phase_info,
                phase_elapsed,
                elapsed_display,
                fw = feature_width,
                pw = phase_width,
            );
            if issue_width > 0 {
                line_str.push_str(&format!("   {:iw$}", issue_info, iw = issue_width));
            }
            if pr_width > 0 {
                line_str.push_str(&format!("  {:prw$}", pr_info, prw = pr_width));
            }

            let line = Paragraph::new(Line::from(Span::styled(line_str, style)));
            frame.render_widget(line, Rect::new(area.x, area.y + row as u16, area.width, 1));
        }

        // Separator before detail panel
        let detail_start = 4 + list_end + 1;
        let sep: String = format!("  {}", "\u{2500}".repeat(max_x.saturating_sub(4)));
        let sep_p = Paragraph::new(Line::from(Span::styled(
            sep,
            Style::default().add_modifier(Modifier::DIM),
        )));
        frame.render_widget(
            sep_p,
            Rect::new(area.x, area.y + (detail_start - 1) as u16, area.width, 1),
        );

        // Detail panel
        if !self.flows.is_empty() {
            self.render_detail_panel(frame, area, detail_start);
        }

        self.render_list_footer(frame, area);
    }

    fn render_detail_panel(&self, frame: &mut Frame, area: Rect, start_row: usize) {
        let flow = &self.flows[self.selected];
        let max_y = area.height as usize;
        let mut row = start_row;

        // Feature name
        let feat_line = Paragraph::new(Line::from(Span::styled(
            format!("  {}", flow.feature),
            Style::default().add_modifier(Modifier::BOLD),
        )));
        frame.render_widget(
            feat_line,
            Rect::new(area.x, area.y + row as u16, area.width, 1),
        );
        row += 1;

        // Branch and worktree
        let branch_line = Paragraph::new(Line::from(format!("  Branch: {}", flow.branch)));
        frame.render_widget(
            branch_line,
            Rect::new(area.x, area.y + row as u16, area.width, 1),
        );
        row += 1;

        let wt_line = Paragraph::new(Line::from(format!("  Worktree: {}", flow.worktree)));
        frame.render_widget(
            wt_line,
            Rect::new(area.x, area.y + row as u16, area.width, 1),
        );
        row += 2;

        // Phase timeline
        for entry in &flow.timeline {
            if row >= max_y.saturating_sub(3) {
                break;
            }
            let (marker, suffix, style) = match entry.status.as_str() {
                "complete" => {
                    let suffix = if entry.time.is_empty() {
                        String::new()
                    } else {
                        format!("  {}", entry.time)
                    };
                    ("[x]", suffix, Style::default().fg(Color::Green))
                }
                "in_progress" => {
                    let time_part = if entry.time.is_empty() {
                        String::new()
                    } else {
                        format!("  {}", entry.time)
                    };
                    let ann_part = if entry.annotation.is_empty() {
                        String::new()
                    } else {
                        format!("  ({})", entry.annotation)
                    };
                    let style = if flow.blocked {
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    };
                    ("[>]", format!("{}{}", time_part, ann_part), style)
                }
                _ => (
                    "[ ]",
                    String::new(),
                    Style::default().add_modifier(Modifier::DIM),
                ),
            };

            let line_text = format!("  {} {}{}", marker, entry.name, suffix);
            let line = Paragraph::new(Line::from(Span::styled(line_text, style)));
            frame.render_widget(line, Rect::new(area.x, area.y + row as u16, area.width, 1));
            row += 1;
        }

        row += 1;

        // Notes and issues
        if row < max_y.saturating_sub(2) {
            if flow.notes_count > 0 {
                let notes_line =
                    Paragraph::new(Line::from(format!("  Notes: {}", flow.notes_count)));
                frame.render_widget(
                    notes_line,
                    Rect::new(area.x, area.y + row as u16, area.width, 1),
                );
                row += 1;
            }
            for issue in &flow.issues {
                if row >= max_y.saturating_sub(2) {
                    break;
                }
                let line =
                    Paragraph::new(Line::from(format!("    {} {}", issue.ref_str, issue.title)));
                frame.render_widget(line, Rect::new(area.x, area.y + row as u16, area.width, 1));
                row += 1;
            }
        }
    }

    fn render_list_footer(&self, frame: &mut Frame, area: Rect) {
        let footer_text = " [\u{2190}\u{2192}] Tab  [\u{2191}\u{2193}] Navigate  [Enter] Worktree  [p] PR  [i] Issues  [I] Issue  [t] Tasks  [l] Log  [a] Abort  [r] Refresh  [q] Quit";
        let footer = Paragraph::new(Line::from(Span::styled(
            footer_text,
            Style::default().add_modifier(Modifier::DIM),
        )));
        let y = area.y + area.height.saturating_sub(1);
        frame.render_widget(footer, Rect::new(area.x, y, area.width, 1));
    }

    fn render_orchestration_view(&self, frame: &mut Frame, area: Rect) {
        self.render_header(frame, area);

        let max_y = area.height as usize;
        let max_x = area.width as usize;

        if self.orch_data.is_none() {
            let msg = Paragraph::new(Line::from("  No orchestration running."));
            frame.render_widget(msg, Rect::new(area.x, area.y + 5, area.width, 1));
            let footer_text = " [\u{2190}\u{2192}] Tab  [r] Refresh  [q] Quit";
            let footer = Paragraph::new(Line::from(Span::styled(
                footer_text,
                Style::default().add_modifier(Modifier::DIM),
            )));
            let y = area.y + area.height.saturating_sub(1);
            frame.render_widget(footer, Rect::new(area.x, y, area.width, 1));
            return;
        }

        let orch = self.orch_data.as_ref().unwrap();

        // Elapsed
        let elapsed_line = Paragraph::new(Line::from(format!("  Elapsed: {}", orch.elapsed)));
        frame.render_widget(elapsed_line, Rect::new(area.x, area.y + 5, area.width, 1));

        let list_start = 7;
        let list_end = orch.items.len().min(max_y.saturating_sub(6));
        let orch_title_width = 30usize.max(max_x.saturating_sub(44));

        for i in 0..list_end {
            let item = &orch.items[i];
            let row = list_start + i;
            if row >= max_y.saturating_sub(1) {
                break;
            }

            let marker = if i == self.orch_selected {
                "\u{25b8} "
            } else {
                "  "
            };

            let mut style = match item.status.as_str() {
                "completed" => Style::default().fg(Color::Green),
                "failed" => Style::default().fg(Color::Red),
                "in_progress" => Style::default().fg(Color::Yellow),
                _ => Style::default().add_modifier(Modifier::DIM),
            };
            if i == self.orch_selected {
                style = style.add_modifier(Modifier::BOLD);
            }

            let elapsed_str = if item.elapsed.is_empty() {
                String::new()
            } else {
                format!("  {}", item.elapsed)
            };
            let pr_str = if let Some(ref pr_url) = item.pr_url {
                let num = pr_url
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or("");
                format!("  PR {}", num)
            } else {
                String::new()
            };

            let issue_num = item
                .issue_number
                .map(|n| format!("#{}", n))
                .unwrap_or_default();

            let title = if item.title.chars().count() > orch_title_width {
                let truncated: String = item
                    .title
                    .chars()
                    .take(orch_title_width.saturating_sub(3))
                    .collect();
                format!("{}...", truncated)
            } else {
                format!("{:width$}", item.title, width = orch_title_width)
            };

            let line_text = format!(
                "{}{} {}  {}{}{}",
                marker, item.icon, issue_num, title, elapsed_str, pr_str
            );
            let line = Paragraph::new(Line::from(Span::styled(line_text, style)));
            frame.render_widget(line, Rect::new(area.x, area.y + row as u16, area.width, 1));
        }

        // Detail panel
        let detail_row = list_start + list_end + 1;
        if detail_row < max_y.saturating_sub(1) {
            if let Some(item) = orch.items.get(self.orch_selected) {
                if item.status == "failed" {
                    if let Some(ref reason) = item.reason {
                        let detail = Paragraph::new(Line::from(format!("    Reason: {}", reason)));
                        frame.render_widget(
                            detail,
                            Rect::new(area.x, area.y + detail_row as u16, area.width, 1),
                        );
                    }
                } else if item.status == "completed" {
                    if let Some(ref pr_url) = item.pr_url {
                        let detail = Paragraph::new(Line::from(format!("    PR: {}", pr_url)));
                        frame.render_widget(
                            detail,
                            Rect::new(area.x, area.y + detail_row as u16, area.width, 1),
                        );
                    }
                }
            }
        }

        // Footer
        let footer_text =
            " [\u{2190}\u{2192}] Tab  [\u{2191}\u{2193}] Navigate  [i] Issue  [r] Refresh  [q] Quit";
        let footer = Paragraph::new(Line::from(Span::styled(
            footer_text,
            Style::default().add_modifier(Modifier::DIM),
        )));
        let y = area.y + area.height.saturating_sub(1);
        frame.render_widget(footer, Rect::new(area.x, y, area.width, 1));
    }

    fn render_log_view(&self, frame: &mut Frame, area: Rect) {
        let max_y = area.height as usize;
        let max_x = area.width as usize;

        if self.flows.is_empty() {
            return;
        }
        let flow = &self.flows[self.selected];

        // Header
        let header_text = format!(" {} \u{2014} Log ", flow.feature);
        let border: String = "\u{2500}".repeat(max_x);
        let border_line = Paragraph::new(Line::from(Span::styled(
            &border,
            Style::default().add_modifier(Modifier::DIM),
        )));
        frame.render_widget(border_line, Rect::new(area.x, area.y, area.width, 1));
        let header = Paragraph::new(Line::from(Span::styled(
            header_text,
            Style::default().add_modifier(Modifier::BOLD),
        )));
        frame.render_widget(
            header,
            Rect::new(area.x + 2, area.y, area.width.saturating_sub(2), 1),
        );

        // Read log file
        let log_path = FlowPaths::new(&self.root, &flow.branch).log_file();
        let log_content = std::fs::read_to_string(&log_path).ok();
        let entries = tui_data::parse_log_entries(
            log_content.as_deref().unwrap_or(""),
            max_y.saturating_sub(4),
        );

        if entries.is_empty() {
            let msg = Paragraph::new(Line::from("  No log entries."));
            frame.render_widget(msg, Rect::new(area.x, area.y + 2, area.width, 1));
        } else {
            for (i, entry) in entries.iter().enumerate() {
                let row = 2 + i;
                if row >= max_y.saturating_sub(2) {
                    break;
                }
                let line =
                    Paragraph::new(Line::from(format!("    {}  {}", entry.time, entry.message)));
                frame.render_widget(line, Rect::new(area.x, area.y + row as u16, area.width, 1));
            }
        }

        // Footer
        let footer_text = " [Esc] Back  [q] Quit";
        let footer = Paragraph::new(Line::from(Span::styled(
            footer_text,
            Style::default().add_modifier(Modifier::DIM),
        )));
        let y = area.y + area.height.saturating_sub(1);
        frame.render_widget(footer, Rect::new(area.x, y, area.width, 1));
    }

    fn render_issues_view(&self, frame: &mut Frame, area: Rect) {
        let max_y = area.height as usize;
        let max_x = area.width as usize;

        if self.flows.is_empty() {
            return;
        }
        let flow = &self.flows[self.selected];
        let issues = &flow.issues;

        // Header
        let header_text = format!(" {} \u{2014} Issues ", flow.feature);
        let border: String = "\u{2500}".repeat(max_x);
        let border_line = Paragraph::new(Line::from(Span::styled(
            &border,
            Style::default().add_modifier(Modifier::DIM),
        )));
        frame.render_widget(border_line, Rect::new(area.x, area.y, area.width, 1));
        let header = Paragraph::new(Line::from(Span::styled(
            header_text,
            Style::default().add_modifier(Modifier::BOLD),
        )));
        frame.render_widget(
            header,
            Rect::new(area.x + 2, area.y, area.width.saturating_sub(2), 1),
        );

        if issues.is_empty() {
            let msg = Paragraph::new(Line::from("  No issues filed."));
            frame.render_widget(msg, Rect::new(area.x, area.y + 2, area.width, 1));
        } else {
            // Column header
            let col_hdr = format!("    {:18} {:8} {:14} Title", "Label", "Ref", "Phase");
            let col_hdr_line = Paragraph::new(Line::from(Span::styled(
                col_hdr,
                Style::default().add_modifier(Modifier::DIM),
            )));
            frame.render_widget(col_hdr_line, Rect::new(area.x, area.y + 2, area.width, 1));

            for (i, issue) in issues.iter().enumerate() {
                if i >= max_y.saturating_sub(5) {
                    break;
                }
                let row = 3 + i;
                let marker = if i == self.issue_selected {
                    "\u{25b8} "
                } else {
                    "  "
                };
                let style = if i == self.issue_selected {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let line_text = format!(
                    "  {}{:18} {:8} {:14} {}",
                    marker,
                    &issue.label.chars().take(18).collect::<String>(),
                    &issue.ref_str,
                    &issue.phase_name.chars().take(14).collect::<String>(),
                    &issue.title,
                );
                let line = Paragraph::new(Line::from(Span::styled(line_text, style)));
                frame.render_widget(line, Rect::new(area.x, area.y + row as u16, area.width, 1));
            }
        }

        // Footer
        let footer_text = " [Esc] Back  [Enter] Open  [\u{2191}\u{2193}] Navigate  [q] Quit";
        let footer = Paragraph::new(Line::from(Span::styled(
            footer_text,
            Style::default().add_modifier(Modifier::DIM),
        )));
        let y = area.y + area.height.saturating_sub(1);
        frame.render_widget(footer, Rect::new(area.x, y, area.width, 1));
    }

    fn render_tasks_view(&self, frame: &mut Frame, area: Rect) {
        let max_y = area.height as usize;
        let max_x = area.width as usize;

        if self.flows.is_empty() {
            return;
        }
        let flow = &self.flows[self.selected];

        // Header
        let header_text = format!(" {} \u{2014} Tasks ", flow.feature);
        let border: String = "\u{2500}".repeat(max_x);
        let border_line = Paragraph::new(Line::from(Span::styled(
            &border,
            Style::default().add_modifier(Modifier::DIM),
        )));
        frame.render_widget(border_line, Rect::new(area.x, area.y, area.width, 1));
        let header = Paragraph::new(Line::from(Span::styled(
            header_text,
            Style::default().add_modifier(Modifier::BOLD),
        )));
        frame.render_widget(
            header,
            Rect::new(area.x + 2, area.y, area.width.saturating_sub(2), 1),
        );

        // Read plan file
        let plan_content = flow.plan_path.as_ref().and_then(|p| {
            std::fs::read_to_string(p)
                .ok()
                .or_else(|| std::fs::read_to_string(self.root.join(p)).ok())
        });

        if let Some(content) = plan_content {
            for (i, line) in content.lines().enumerate() {
                let row = 2 + i;
                if row >= max_y.saturating_sub(2) {
                    break;
                }
                let p = Paragraph::new(Line::from(format!("  {}", line)));
                frame.render_widget(p, Rect::new(area.x, area.y + row as u16, area.width, 1));
            }
        } else {
            let msg = Paragraph::new(Line::from("  No plan file."));
            frame.render_widget(msg, Rect::new(area.x, area.y + 2, area.width, 1));
        }

        // Footer
        let footer_text = " [Esc] Back  [q] Quit";
        let footer = Paragraph::new(Line::from(Span::styled(
            footer_text,
            Style::default().add_modifier(Modifier::DIM),
        )));
        let y = area.y + area.height.saturating_sub(1);
        frame.render_widget(footer, Rect::new(area.x, y, area.width, 1));
    }

    fn get_orch_issue_in_progress(&self) -> Option<i64> {
        self.orch_data.as_ref().and_then(|orch| {
            orch.items
                .iter()
                .find(|item| item.status == "in_progress")
                .and_then(|item| item.issue_number)
        })
    }
}

// --- Standalone helpers ---

/// Build the "files view" URL for a PR by appending `/files` to the
/// PR's canonical URL. Trailing slashes are normalized so we never
/// emit `.../100//files`.
///
/// Pure helper — no IO. Used by `TuiApp::open_pr`.
fn pr_files_url(pr_url: &str) -> String {
    format!("{}/files", pr_url.trim_end_matches('/'))
}

/// Compose the GitHub issue URL for the smallest issue number a flow
/// references. Returns `None` when the flow has no issues OR when no
/// repo is available (neither the state's `repo` key nor the
/// fallback). The state's `repo` field wins over the fallback when
/// both are present.
///
/// Pure helper — no IO. Used by `TuiApp::open_flow_issue`.
fn flow_issue_url(
    state: &Value,
    fallback_repo: Option<&str>,
    issue_numbers: &[i64],
) -> Option<String> {
    let num = *issue_numbers.iter().min()?;
    let repo = state
        .get("repo")
        .and_then(|r| r.as_str())
        .or(fallback_repo)?;
    Some(format!("https://github.com/{}/issues/{}", repo, num))
}

/// Compose the GitHub issue URL for an orchestration item. Returns
/// `None` when either the issue number or the repo is missing or when
/// the repo string is empty (the orchestration tab inherits its repo
/// from the TuiApp and that field can legitimately be `None`).
///
/// Pure helper — no IO. Used by `TuiApp::open_orch_issue`.
fn orch_issue_url(repo: Option<&str>, issue_number: Option<i64>) -> Option<String> {
    let num = issue_number?;
    let repo = repo?;
    if repo.is_empty() {
        return None;
    }
    Some(format!("https://github.com/{}/issues/{}", repo, num))
}

/// Decide whether a key confirms an abort prompt. Accepts both `y`
/// and `Y`; everything else (including `n`, `Esc`, and unrelated
/// chars) returns `false`.
///
/// Pure helper — used by `TuiApp::handle_abort_confirm`.
fn should_abort(code: KeyCode) -> bool {
    matches!(code, KeyCode::Char('y') | KeyCode::Char('Y'))
}

/// Compose the argument vector for `bin/flow cleanup`. `root` is
/// lossy-converted to a `&str` (non-UTF-8 paths fall back to `.`,
/// matching the pre-extraction behaviour). The `--pr <n>` pair is
/// appended only when `pr_number` is `Some`.
///
/// Pure helper — no IO. Used by `TuiApp::abort_flow`.
fn build_cleanup_command_args(
    root: &Path,
    branch: &str,
    worktree: &str,
    pr_number: Option<i64>,
) -> Vec<String> {
    let mut args = vec![
        "cleanup".to_string(),
        root.to_str().unwrap_or(".").to_string(),
        "--branch".to_string(),
        branch.to_string(),
        "--worktree".to_string(),
        worktree.to_string(),
    ];
    if let Some(pr) = pr_number {
        args.push("--pr".to_string());
        args.push(pr.to_string());
    }
    args
}

/// Read a flow's `session_tty` field from its raw state JSON.
/// Returns `None` when the field is missing or non-string. Empty
/// strings pass through as `Some("")` so the caller decides what to
/// do with them — preserving the pre-extraction behaviour.
///
/// Pure helper — no IO. Used by `TuiApp::open_worktree`.
fn worktree_session_tty(flow: &FlowSummary) -> Option<&str> {
    flow.state.get("session_tty").and_then(|v| v.as_str())
}

/// Color for rate limit percentages.
fn rl_color(pct: i64) -> Style {
    if pct >= 90 {
        Style::default().fg(Color::Red)
    } else if pct >= 70 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    }
}

/// Build the (program, args) pair that opens `url` in the default
/// macOS browser. Returns `("open", vec![url])` — the URL passes
/// through unmodified so callers can audit the spawn surface as data
/// rather than as a chained `Command` builder.
///
/// Pure helper — `Command::spawn` happens in `open_url` and is covered
/// by `test_coverage.md`.
fn build_open_url_command(url: &str) -> (&'static str, Vec<String>) {
    ("open", vec![url.to_string()])
}

/// Open a URL in the default browser (macOS).
fn open_url(url: &str) {
    let (program, args) = build_open_url_command(url);
    let _ = Command::new(program)
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Build the AppleScript text that asks iTerm2 to find and select the
/// tab whose session tty matches `session_tty`.
///
/// Pure helper — no IO. The osascript invocation lives in
/// `activate_iterm_tab` and is covered by `test_coverage.md`.
fn build_iterm_activation_script(session_tty: &str) -> String {
    format!(
        r#"tell application "iTerm2"
    repeat with w from 1 to count of windows
        repeat with t from 1 to count of tabs of (item w of windows)
            set s to current session of (item t of tabs of (item w of windows))
            if tty of s is "{tty}" then
                select (item w of windows)
                select (item t of tabs of (item w of windows))
                return "activated"
            end if
        end repeat
    end repeat
    return "not found"
end tell"#,
        tty = session_tty
    )
}

/// Decide whether the osascript invocation reported a successful
/// activation. Returns true only when the process exited zero AND
/// stdout, after trimming, is the literal string `activated`.
///
/// Pure helper — accepts primitives so tests do not need to construct a
/// `std::process::ExitStatus` (which stable Rust does not allow from
/// outside the std lib).
fn parse_osascript_result(success: bool, stdout: &[u8]) -> bool {
    success && String::from_utf8_lossy(stdout).trim() == "activated"
}

/// Activate an iTerm2 tab by matching its session tty.
fn activate_iterm_tab(session_tty: &str) -> bool {
    let script = build_iterm_activation_script(session_tty);

    match Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
    {
        Ok(output) => parse_osascript_result(output.status.success(), &output.stdout),
        Err(_) => false,
    }
}

/// Walk up three parent directories from `exe_path` (binary →
/// `{debug|release}` → `target` → repo root) and return
/// `Some(<root>/bin/flow)` when the resolved file exists, `None`
/// otherwise.
///
/// Pure helper — accepts the executable path explicitly so tests can
/// drive the traversal logic with controlled tmpdirs. The
/// `std::env::current_exe()` lookup and the bare-string fallback live
/// in `find_bin_flow` and are covered by `test_coverage.md`.
fn derive_bin_flow_path(exe_path: &Path) -> Option<PathBuf> {
    let root = exe_path
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())?;
    let bin_flow = root.join("bin").join("flow");
    if bin_flow.exists() {
        Some(bin_flow)
    } else {
        None
    }
}

/// Locate bin/flow by traversing up from the current executable.
fn find_bin_flow() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin_flow) = derive_bin_flow_path(&exe) {
            return bin_flow;
        }
    }
    PathBuf::from("bin/flow")
}

/// Entry point: initialize terminal and run the TUI.
pub fn run(root: PathBuf, version: String, repo: Option<String>) -> io::Result<()> {
    // Check if stdout is a terminal
    if !atty_check() {
        eprintln!("Error: flow tui requires an interactive terminal.");
        std::process::exit(1);
    }
    let mut app = TuiApp::new(root, version, repo);
    app.run_terminal()
}

/// Check if stdout is a terminal using libc::isatty.
///
/// Uses libc directly rather than crossterm's terminal detection to avoid
/// importing crossterm APIs beyond event handling and alternate screen.
fn atty_check() -> bool {
    // SAFETY: STDOUT_FILENO (1) is always a valid open file descriptor
    // in a normal Unix process.
    unsafe { libc::isatty(libc::STDOUT_FILENO) != 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- rl_color ---

    #[test]
    fn rl_color_below_yellow_threshold_is_default() {
        // Below 70% — no color modifier applied.
        assert_eq!(rl_color(0).fg, None);
        assert_eq!(rl_color(1).fg, None);
        assert_eq!(rl_color(69).fg, None);
    }

    #[test]
    fn rl_color_yellow_band_is_yellow() {
        // 70..=89 → Yellow.
        assert_eq!(rl_color(70).fg, Some(Color::Yellow));
        assert_eq!(rl_color(80).fg, Some(Color::Yellow));
        assert_eq!(rl_color(89).fg, Some(Color::Yellow));
    }

    #[test]
    fn rl_color_red_band_is_red() {
        // 90..=100 (and above) → Red.
        assert_eq!(rl_color(90).fg, Some(Color::Red));
        assert_eq!(rl_color(95).fg, Some(Color::Red));
        assert_eq!(rl_color(100).fg, Some(Color::Red));
        assert_eq!(rl_color(150).fg, Some(Color::Red));
    }

    #[test]
    fn rl_color_negative_input_is_default() {
        // Negative percentages (e.g., from corrupted state) fall through to default.
        assert_eq!(rl_color(-1).fg, None);
        assert_eq!(rl_color(-100).fg, None);
    }

    // --- build_iterm_activation_script ---

    #[test]
    fn iterm_script_embeds_tty_argument() {
        let script = build_iterm_activation_script("/dev/ttys003");
        assert!(script.contains(r#"if tty of s is "/dev/ttys003" then"#));
    }

    #[test]
    fn iterm_script_starts_and_ends_with_tell_application() {
        let script = build_iterm_activation_script("/dev/ttys000");
        assert!(script.starts_with(r#"tell application "iTerm2""#));
        assert!(script.ends_with("end tell"));
    }

    #[test]
    fn iterm_script_returns_activated_and_not_found_branches() {
        let script = build_iterm_activation_script("/dev/ttys000");
        assert!(script.contains(r#"return "activated""#));
        assert!(script.contains(r#"return "not found""#));
    }

    #[test]
    fn iterm_script_handles_empty_tty_without_panic() {
        // Empty tty produces a well-formed script with an empty literal —
        // the AppleScript will simply never match, but the formatter must
        // not panic or drop the surrounding template.
        let script = build_iterm_activation_script("");
        assert!(script.contains(r#"if tty of s is "" then"#));
        assert!(script.ends_with("end tell"));
    }

    // --- parse_osascript_result ---

    #[test]
    fn osascript_success_with_activated_stdout_is_true() {
        assert!(parse_osascript_result(true, b"activated"));
    }

    #[test]
    fn osascript_success_trims_surrounding_whitespace() {
        // osascript adds a trailing newline; leading whitespace is also tolerated.
        assert!(parse_osascript_result(true, b"  activated\n"));
        assert!(parse_osascript_result(true, b"activated\r\n"));
    }

    #[test]
    fn osascript_success_with_not_found_stdout_is_false() {
        assert!(!parse_osascript_result(true, b"not found"));
    }

    #[test]
    fn osascript_failure_status_is_false_even_with_activated_stdout() {
        // Non-zero exit must dominate the decision regardless of stdout content.
        assert!(!parse_osascript_result(false, b"activated"));
    }

    #[test]
    fn osascript_empty_stdout_is_false() {
        assert!(!parse_osascript_result(true, b""));
        assert!(!parse_osascript_result(false, b""));
    }

    // --- build_open_url_command ---

    #[test]
    fn open_url_command_uses_macos_open_binary() {
        let (program, _args) = build_open_url_command("https://example.com");
        assert_eq!(program, "open");
    }

    #[test]
    fn open_url_command_passes_url_through_unmodified() {
        let (_, args) = build_open_url_command("https://github.com/o/r/pull/100");
        assert_eq!(args, vec!["https://github.com/o/r/pull/100".to_string()]);
    }

    #[test]
    fn open_url_command_passes_empty_url_through() {
        // Empty input does NOT get filtered or rewritten — the caller is
        // responsible for any guards. Test pins the no-validation contract.
        let (program, args) = build_open_url_command("");
        assert_eq!(program, "open");
        assert_eq!(args, vec![String::new()]);
    }

    // --- pr_files_url ---

    #[test]
    fn pr_files_url_appends_files_to_canonical_url() {
        assert_eq!(
            pr_files_url("https://github.com/owner/repo/pull/100"),
            "https://github.com/owner/repo/pull/100/files"
        );
    }

    #[test]
    fn pr_files_url_strips_trailing_slash_before_appending() {
        // Avoids `.../100//files` when callers pre-normalize.
        assert_eq!(
            pr_files_url("https://github.com/owner/repo/pull/100/"),
            "https://github.com/owner/repo/pull/100/files"
        );
    }

    #[test]
    fn pr_files_url_strips_multiple_trailing_slashes() {
        assert_eq!(
            pr_files_url("https://example.com/x///"),
            "https://example.com/x/files"
        );
    }

    #[test]
    fn pr_files_url_with_empty_input_returns_files() {
        // No input validation — the helper trusts the caller's URL
        // shape. Empty input still produces a well-formed string.
        assert_eq!(pr_files_url(""), "/files");
    }

    // --- flow_issue_url ---

    #[test]
    fn flow_issue_url_uses_state_repo_when_present() {
        let state = serde_json::json!({"repo": "state/wins"});
        let url = flow_issue_url(&state, Some("fallback/repo"), &[42]);
        assert_eq!(
            url,
            Some("https://github.com/state/wins/issues/42".to_string())
        );
    }

    #[test]
    fn flow_issue_url_falls_back_when_state_lacks_repo() {
        let state = serde_json::json!({});
        let url = flow_issue_url(&state, Some("fallback/repo"), &[42]);
        assert_eq!(
            url,
            Some("https://github.com/fallback/repo/issues/42".to_string())
        );
    }

    #[test]
    fn flow_issue_url_returns_none_when_no_issues() {
        let state = serde_json::json!({"repo": "o/r"});
        assert_eq!(flow_issue_url(&state, None, &[]), None);
    }

    #[test]
    fn flow_issue_url_picks_smallest_issue_when_multiple() {
        let state = serde_json::json!({"repo": "o/r"});
        let url = flow_issue_url(&state, None, &[42, 7, 99]);
        assert_eq!(url, Some("https://github.com/o/r/issues/7".to_string()));
    }

    #[test]
    fn flow_issue_url_returns_none_when_no_repo_anywhere() {
        let state = serde_json::json!({});
        assert_eq!(flow_issue_url(&state, None, &[42]), None);
    }

    #[test]
    fn flow_issue_url_treats_non_string_state_repo_as_missing() {
        // Defensive: a corrupt state file with a non-string repo
        // should not panic and should fall back to the parameter.
        let state = serde_json::json!({"repo": 12345});
        let url = flow_issue_url(&state, Some("fallback/repo"), &[1]);
        assert_eq!(
            url,
            Some("https://github.com/fallback/repo/issues/1".to_string())
        );
    }

    // --- orch_issue_url ---

    #[test]
    fn orch_issue_url_returns_url_when_repo_and_number_present() {
        let url = orch_issue_url(Some("o/r"), Some(42));
        assert_eq!(url, Some("https://github.com/o/r/issues/42".to_string()));
    }

    #[test]
    fn orch_issue_url_returns_none_when_repo_missing() {
        assert_eq!(orch_issue_url(None, Some(42)), None);
    }

    #[test]
    fn orch_issue_url_returns_none_when_repo_empty_string() {
        // Mirrors the original `unwrap_or("")` + is_empty guard so an
        // unconfigured repo doesn't produce a malformed URL.
        assert_eq!(orch_issue_url(Some(""), Some(42)), None);
    }

    #[test]
    fn orch_issue_url_returns_none_when_issue_number_missing() {
        assert_eq!(orch_issue_url(Some("o/r"), None), None);
    }

    // --- should_abort ---

    #[test]
    fn should_abort_accepts_lowercase_y() {
        assert!(should_abort(KeyCode::Char('y')));
    }

    #[test]
    fn should_abort_accepts_uppercase_y() {
        assert!(should_abort(KeyCode::Char('Y')));
    }

    #[test]
    fn should_abort_rejects_n_and_other_chars() {
        assert!(!should_abort(KeyCode::Char('n')));
        assert!(!should_abort(KeyCode::Char('N')));
        assert!(!should_abort(KeyCode::Char('z')));
        assert!(!should_abort(KeyCode::Char(' ')));
    }

    #[test]
    fn should_abort_rejects_non_char_keys() {
        assert!(!should_abort(KeyCode::Esc));
        assert!(!should_abort(KeyCode::Enter));
        assert!(!should_abort(KeyCode::Up));
        assert!(!should_abort(KeyCode::Down));
    }

    // --- build_cleanup_command_args ---

    #[test]
    fn cleanup_args_include_pr_flag_when_some() {
        let args = build_cleanup_command_args(
            Path::new("/home/user/project"),
            "feature-x",
            ".worktrees/feature-x",
            Some(42),
        );
        assert_eq!(
            args,
            vec![
                "cleanup".to_string(),
                "/home/user/project".to_string(),
                "--branch".to_string(),
                "feature-x".to_string(),
                "--worktree".to_string(),
                ".worktrees/feature-x".to_string(),
                "--pr".to_string(),
                "42".to_string(),
            ]
        );
    }

    #[test]
    fn cleanup_args_omit_pr_flag_when_none() {
        let args = build_cleanup_command_args(
            Path::new("/home/user/project"),
            "feature-x",
            ".worktrees/feature-x",
            None,
        );
        assert_eq!(
            args,
            vec![
                "cleanup".to_string(),
                "/home/user/project".to_string(),
                "--branch".to_string(),
                "feature-x".to_string(),
                "--worktree".to_string(),
                ".worktrees/feature-x".to_string(),
            ]
        );
        // Verify no `--pr` leaked in.
        assert!(!args.iter().any(|a| a == "--pr"));
    }

    #[test]
    fn cleanup_args_preserve_spaces_in_root_path() {
        // Path::to_str on a space-containing path yields the string
        // verbatim — `Command::args` will handle the spawn-side
        // quoting. Test pins the no-escape contract.
        let args = build_cleanup_command_args(
            Path::new("/home/user/my project"),
            "b",
            ".worktrees/b",
            None,
        );
        assert_eq!(args[1], "/home/user/my project");
    }

    // --- worktree_session_tty ---

    fn make_flow_summary(state: serde_json::Value) -> FlowSummary {
        // Minimal FlowSummary fixture — only `state` is read by the
        // helper; the other fields are filled with sentinels so the
        // struct is well-formed for the test.
        FlowSummary {
            feature: String::new(),
            branch: String::new(),
            worktree: String::new(),
            pr_number: None,
            pr_url: None,
            phase_number: 0,
            phase_name: String::new(),
            elapsed: String::new(),
            code_task: 0,
            diff_stats: None,
            notes_count: 0,
            issues_count: 0,
            issues: vec![],
            blocked: false,
            issue_numbers: vec![],
            plan_path: None,
            annotation: String::new(),
            phase_elapsed: String::new(),
            timeline: vec![],
            state,
        }
    }

    #[test]
    fn worktree_tty_returns_some_when_state_has_string() {
        let flow = make_flow_summary(serde_json::json!({"session_tty": "/dev/ttys003"}));
        assert_eq!(worktree_session_tty(&flow), Some("/dev/ttys003"));
    }

    #[test]
    fn worktree_tty_returns_none_when_state_lacks_field() {
        let flow = make_flow_summary(serde_json::json!({}));
        assert_eq!(worktree_session_tty(&flow), None);
    }

    #[test]
    fn worktree_tty_returns_none_when_field_is_non_string() {
        let flow = make_flow_summary(serde_json::json!({"session_tty": 12345}));
        assert_eq!(worktree_session_tty(&flow), None);
    }

    #[test]
    fn worktree_tty_passes_empty_string_through() {
        // Empty-string returns Some("") — the caller (open_worktree)
        // gets to decide what to do with that.
        let flow = make_flow_summary(serde_json::json!({"session_tty": ""}));
        assert_eq!(worktree_session_tty(&flow), Some(""));
    }

    // --- derive_bin_flow_path ---

    #[test]
    fn bin_flow_path_returns_some_when_target_exists_at_depth_three() {
        // Simulate the production layout: <root>/target/debug/flow-rs.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join("bin/flow"), "#!/bin/sh\n").unwrap();
        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        let exe = root.join("target/debug/flow-rs");
        std::fs::write(&exe, "").unwrap();
        let resolved = derive_bin_flow_path(&exe);
        assert_eq!(resolved, Some(root.join("bin").join("flow")));
    }

    #[test]
    fn bin_flow_path_returns_none_when_root_lacks_bin_flow() {
        // Layout exists but `bin/flow` is missing.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("target/release")).unwrap();
        let exe = root.join("target/release/flow-rs");
        std::fs::write(&exe, "").unwrap();
        assert_eq!(derive_bin_flow_path(&exe), None);
    }

    #[test]
    fn bin_flow_path_returns_none_when_path_is_too_shallow() {
        // No three-level ancestry available — should yield None instead
        // of panicking on Option chaining.
        let tmp = tempfile::TempDir::new().unwrap();
        let exe = tmp.path().join("flow-rs");
        std::fs::write(&exe, "").unwrap();
        // Path has at most one parent (tmp), can't walk up three.
        assert_eq!(derive_bin_flow_path(&exe), None);
    }

    #[test]
    fn bin_flow_path_walks_up_exactly_three_levels() {
        // Deeper ancestry: <root>/target/debug/deps/flow-rs-<hash>.
        // The walk-up should land at "deps", NOT at root, so bin/flow
        // would have to live at `<root>/target/debug/bin/flow` — which
        // we deliberately do NOT create here, so the result is None.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join("bin/flow"), "").unwrap();
        std::fs::create_dir_all(root.join("target/debug/deps")).unwrap();
        let exe = root.join("target/debug/deps/flow-rs-abc");
        std::fs::write(&exe, "").unwrap();
        // Walking up three from `target/debug/deps/flow-rs-abc` lands at
        // `<root>/target` — `<root>/target/bin/flow` does not exist.
        assert_eq!(derive_bin_flow_path(&exe), None);
    }

    #[test]
    fn bin_flow_path_returns_none_when_exe_path_does_not_exist() {
        // The walk-up uses Path::parent which is purely lexical, so a
        // non-existent path with sufficient ancestry still returns None
        // because `bin_flow.exists()` reports false on the synthetic root.
        let exe = std::path::PathBuf::from("/nonexistent/target/debug/flow-rs");
        assert_eq!(derive_bin_flow_path(&exe), None);
    }

    #[test]
    fn open_url_command_preserves_query_strings_and_fragments() {
        // No URL escaping happens in the helper — `Command::arg` handles
        // the shell quoting downstream. Test guards against accidental
        // future "sanitization" that would corrupt query strings.
        let url = "https://example.com/path?query=value&other=1#fragment";
        let (_, args) = build_open_url_command(url);
        assert_eq!(args, vec![url.to_string()]);
    }

    #[test]
    fn osascript_invalid_utf8_does_not_panic_and_is_false() {
        // String::from_utf8_lossy replaces invalid sequences with U+FFFD,
        // so the trimmed comparison fails and the function returns false
        // without panicking.
        let bad_bytes: &[u8] = &[0xff, 0xfe, 0xfd];
        assert!(!parse_osascript_result(true, bad_bytes));
    }
}
