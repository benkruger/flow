//! Interactive TUI for viewing and managing active FLOW features.
//!
//! A ratatui-based terminal application that reads local state files and
//! provides keyboard-driven navigation. No Claude session required.
//! Uses tui_data module for data loading.

use std::io;
use std::path::PathBuf;
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
        if matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
            self.abort_flow();
        }
    }

    // --- Actions ---

    fn open_worktree(&self) {
        if self.flows.is_empty() {
            return;
        }
        let flow = &self.flows[self.selected];
        let session_tty = flow.state.get("session_tty").and_then(|v| v.as_str());
        if let Some(tty) = session_tty {
            activate_iterm_tab(tty);
        }
    }

    fn open_pr(&self) {
        if self.flows.is_empty() {
            return;
        }
        let flow = &self.flows[self.selected];
        if let Some(ref url) = flow.pr_url {
            let files_url = format!("{}/files", url.trim_end_matches('/'));
            open_url(&files_url);
        }
    }

    fn open_flow_issue(&self) {
        if self.flows.is_empty() {
            return;
        }
        let flow = &self.flows[self.selected];
        if let Some(&num) = flow.issue_numbers.iter().min() {
            let repo = flow
                .state
                .get("repo")
                .and_then(|r| r.as_str())
                .or(self.repo.as_deref());
            if let Some(repo) = repo {
                let url = format!("https://github.com/{}/issues/{}", repo, num);
                open_url(&url);
            }
        }
    }

    fn open_orch_issue(&self) {
        if let Some(ref orch) = self.orch_data {
            if let Some(item) = orch.items.get(self.orch_selected) {
                if let Some(num) = item.issue_number {
                    let repo = self.repo.as_deref().unwrap_or("");
                    if !repo.is_empty() {
                        let url = format!("https://github.com/{}/issues/{}", repo, num);
                        open_url(&url);
                    }
                }
            }
        }
    }

    fn abort_flow(&mut self) {
        if self.flows.is_empty() {
            return;
        }
        let flow = &self.flows[self.selected];
        let branch = &flow.branch;
        let worktree = &flow.worktree;
        let pr_number = flow.pr_number;

        // Find bin/flow relative to this binary
        let bin_flow = find_bin_flow();

        let mut cmd = Command::new(&bin_flow);
        cmd.arg("cleanup")
            .arg(self.root.to_str().unwrap_or("."))
            .arg("--branch")
            .arg(branch)
            .arg("--worktree")
            .arg(worktree);
        if let Some(pr) = pr_number {
            cmd.arg("--pr").arg(pr.to_string());
        }

        // Exit alternate screen for cleanup output
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);

        eprintln!("Aborting flow: {}...", flow.feature);
        let _ = cmd.status();

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
        let log_path = self
            .root
            .join(".flow-states")
            .join(format!("{}.log", flow.branch));
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

/// Open a URL in the default browser (macOS).
fn open_url(url: &str) {
    let _ = Command::new("open")
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Activate an iTerm2 tab by matching its session tty.
fn activate_iterm_tab(session_tty: &str) -> bool {
    let script = format!(
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
    );

    match Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
    {
        Ok(output) => {
            output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "activated"
        }
        Err(_) => false,
    }
}

/// Locate bin/flow by traversing up from the current executable.
fn find_bin_flow() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        // exe is in target/debug/ or target/release/
        // Go up 3 levels: binary → {debug|release} → target → root
        if let Some(root) = exe
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
        {
            let bin_flow = root.join("bin").join("flow");
            if bin_flow.exists() {
                return bin_flow;
            }
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
