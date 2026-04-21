//! Interactive TUI for viewing and managing active FLOW features.
//!
//! A ratatui-based terminal application that reads local state files and
//! provides keyboard-driven navigation. No Claude session required.
//! Uses tui_data module for data loading.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEvent};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use serde_json::Value;

use crate::flow_paths::FlowPaths;
use crate::tui_data::{self, AccountMetrics, FlowSummary, OrchestrationSummary};

/// Auto-refresh interval.
const REFRESH_MS: u64 = 2000;

/// Boxed draw closure passed into [`TuiApp::run_event_loop`]. The
/// inner `&mut dyn FnMut(&mut Frame)` is the render callback the
/// caller invokes via `terminal.draw(|f| render(f))` — this erases
/// ratatui's `Backend` generic from the event-loop signature so
/// exactly one monomorphization of the loop body exists in
/// coverage reports.
pub type DrawFn = Box<dyn FnMut(&mut dyn FnMut(&mut Frame)) -> io::Result<()>>;

/// Boxed event-source closure passed into [`TuiApp::run_event_loop`].
/// Returns `Ok(Some(event))` when an event is available within the
/// timeout, or `Ok(None)` on timeout.
pub type EventSourceFn = Box<dyn FnMut(Duration) -> io::Result<Option<Event>>>;

/// Active view in the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    List,
    Log,
    Issues,
    Tasks,
}

/// Platform-bound external dependencies injected into `TuiApp`.
///
/// The four fields hold the subprocess binary names and filesystem
/// anchors that the TUI reaches out to when running on a real machine.
/// Production constructs the platform via `TuiAppPlatform::production()`,
/// which resolves `HOME` from the env and walks up from the current
/// executable to find `bin/flow`. Tests construct via
/// `TuiAppPlatform::for_tests()`, which points every binary at the
/// no-op `true` binary resolved by [`probe_true_binary`] (probes
/// `/usr/bin/true` then `/bin/true`) so spawn sites run the real
/// `Command::new().spawn()` chain without side effects. The probe
/// covers both macOS (`/usr/bin/true` — `/bin/true` does not exist)
/// and pre-usrmerge Linux (`/bin/true` — historical canonical).
pub struct TuiAppPlatform {
    /// Binary that opens URLs in the default browser. Production:
    /// `"open"` (macOS). Tests: probed `true` binary path.
    pub open_binary: String,
    /// Binary that runs AppleScript snippets. Production:
    /// `"osascript"`. Tests: probed `true` binary path.
    pub osascript_binary: String,
    /// Path to the `bin/flow` binary used for `cleanup`. Production:
    /// resolved via ancestor walk from `current_exe()`. Tests:
    /// probed `true` binary path.
    pub bin_flow_path: PathBuf,
    /// Home directory, used by `tui_data::load_account_metrics` for
    /// rate-limits lookup. Production: `$HOME`. Tests: `temp_dir()`.
    pub home: PathBuf,
}

impl TuiAppPlatform {
    /// Construct the production platform. Reads `$HOME` from the
    /// env. The `bin_flow_path` is left relative (`bin/flow`) so
    /// `Command::new` resolves it against the current working
    /// directory at spawn time — the TUI's root is always set to
    /// the project root.
    pub fn production() -> Self {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        Self {
            open_binary: "open".to_string(),
            osascript_binary: "osascript".to_string(),
            bin_flow_path: PathBuf::from("bin/flow"),
            home,
        }
    }

    /// Construct a test platform. Every spawn target points at a
    /// no-op `true` binary so tests exercise the real
    /// `Command::new().spawn()` chain (and `Command::output()`)
    /// without any side effects. `home` is `std::env::temp_dir()`.
    ///
    /// Probes `/usr/bin/true` first (canonical on macOS and usrmerge
    /// Linux), then `/bin/true` (pre-usrmerge Linux). The `.expect`
    /// arm is unreachable on every supported platform — both paths
    /// are present on every POSIX system the test suite runs on.
    pub fn for_tests() -> Self {
        let true_bin = [PathBuf::from("/usr/bin/true"), PathBuf::from("/bin/true")]
            .into_iter()
            .find(|p| p.exists())
            .expect("one of /usr/bin/true or /bin/true must exist on any supported platform");
        Self {
            open_binary: true_bin.to_string_lossy().to_string(),
            osascript_binary: true_bin.to_string_lossy().to_string(),
            bin_flow_path: true_bin,
            home: std::env::temp_dir(),
        }
    }
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
    pub platform: TuiAppPlatform,
}

impl TuiApp {
    /// Create a new TuiApp with the given root directory and platform.
    ///
    /// The `platform` argument supplies subprocess binary names and
    /// filesystem anchors so the TUI's IO surface can be exercised by
    /// tests with `/bin/true` stubs and tmpdir homes.
    pub fn new(
        root: PathBuf,
        version: String,
        repo: Option<String>,
        platform: TuiAppPlatform,
    ) -> Self {
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
            platform,
        }
    }

    /// Open a URL in the default browser using the platform-supplied
    /// binary. Production: `open <url>` on macOS. Tests: `/bin/true
    /// <url>` — ignores the URL, exits 0, no side effect.
    pub fn open_url(&self, url: &str) {
        let _ = Command::new(&self.platform.open_binary)
            .arg(url)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
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
        self.metrics =
            tui_data::load_account_metrics(&self.root, Some(self.platform.home.as_path()));
    }

    /// Run the TUI event loop against a caller-supplied draw closure
    /// and event source.
    ///
    /// The `draw` closure receives a render callback and is
    /// responsible for calling `terminal.draw(|f| render(f))` on the
    /// caller's terminal — this keeps ratatui's `Backend` generic
    /// out of `run_event_loop`'s signature, so there is exactly ONE
    /// compiled instantiation of the loop body regardless of which
    /// backend the caller uses. Production (in `src/main.rs`) wraps
    /// a `Terminal<CrosstermBackend<Stdout>>` and tests (in
    /// `tests/tui.rs`) wrap a `Terminal<TestBackend>`; both paths
    /// share the same `run_event_loop` symbol in coverage reports.
    ///
    /// The `events` closure returns `Some(event)` when an event is
    /// available within the timeout, or `None` on timeout.
    pub fn run_event_loop(
        &mut self,
        mut draw: DrawFn,
        mut events: EventSourceFn,
    ) -> io::Result<()> {
        self.refresh_data();

        while self.running {
            draw(&mut |f| self.render(f))?;

            match events(Duration::from_millis(REFRESH_MS))? {
                Some(Event::Key(key)) => self.handle_key(key),
                Some(Event::Resize(_, _)) => self.refresh_data(),
                Some(_) => {}
                // Timeout — refresh data
                None => self.refresh_data(),
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
                // handle_issues_input already guarantees issue_selected
                // stays in bounds (both Up/Down clamp, the early return
                // above rejects empty lists). Direct indexing is safe;
                // the `url.is_empty()` guard skips the spawn for locally
                // filed issues that carry no URL.
                let issue = &flow.issues[self.issue_selected];
                if !issue.url.is_empty() {
                    self.open_url(&issue.url);
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
        let flow = &self.flows[self.selected];
        if let Some(tty) = worktree_session_tty(flow) {
            self.activate_iterm_tab(tty);
        }
    }

    fn open_pr(&self) {
        let flow = &self.flows[self.selected];
        if let Some(ref url) = flow.pr_url {
            let files_url = pr_files_url(url);
            self.open_url(&files_url);
        }
    }

    fn open_flow_issue(&self) {
        let flow = &self.flows[self.selected];
        if let Some(url) = flow_issue_url(&flow.state, self.repo.as_deref(), &flow.issue_numbers) {
            self.open_url(&url);
        }
    }

    fn open_orch_issue(&self) {
        if let Some(ref orch) = self.orch_data {
            if let Some(item) = orch.items.get(self.orch_selected) {
                if let Some(url) = orch_issue_url(self.repo.as_deref(), item.issue_number) {
                    self.open_url(&url);
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
        let bin_flow = self.platform.bin_flow_path.clone();
        let feature = flow.feature.clone();

        // Exit alternate screen for cleanup output
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);

        eprintln!("Aborting flow: {}...", feature);
        let _ = Command::new(&bin_flow).args(&args).status();

        // Re-enter alternate screen
        let _ = enable_raw_mode();
        let _ = execute!(io::stdout(), EnterAlternateScreen);

        self.refresh_data();
    }

    /// Activate an iTerm2 tab by matching its session tty. Reads the
    /// osascript binary path from `self.platform.osascript_binary`
    /// so tests can swap in `/bin/true` and exercise the real
    /// `Command::new(...).output()` line without an osascript runtime.
    pub fn activate_iterm_tab(&self, session_tty: &str) -> bool {
        let script = build_iterm_activation_script(session_tty);

        match Command::new(&self.platform.osascript_binary)
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

        // Render metrics on same row if `render_metrics_spans` produced
        // any. The inner sizing guard lives inside `render_metrics_spans`
        // — when the panel is too narrow it returns an empty Vec, so
        // `is_empty()` here is the only check we need.
        if !metrics_spans.is_empty() {
            let metrics_width: usize = metrics_spans.iter().map(|s| s.width()).sum();
            let col = (width - metrics_width - 2) as u16;
            let metrics_line = Line::from(metrics_spans);
            let metrics_p = Paragraph::new(metrics_line);
            let metrics_area = Rect::new(area.x + col, area.y, metrics_width as u16 + 2, 1);
            frame.render_widget(metrics_p, metrics_area);
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

        // Flow rows. `list_end = self.flows.len().min(max_y - 18)`
        // already bounds `i` so `row = 4 + i <= max_y - 15`, which is
        // always less than the panel's footer row at `max_y - 1`. No
        // additional clamp is needed inside the loop.
        for (i, flow) in self.flows.iter().enumerate().take(list_end) {
            let row = 4 + i;

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

        // Detail panel — the empty-list case short-circuited above at
        // line ~580 with `return`, so reaching this point guarantees a
        // non-empty flow list.
        self.render_detail_panel(frame, area, detail_start);

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
            // `parse_log_entries` already truncates to `max_y - 4`
            // entries, so `row = 2 + i <= max_y - 3`, which is always
            // less than the panel's footer row at `max_y - 1`. No
            // additional clamp is needed inside the loop.
            for (i, entry) in entries.iter().enumerate() {
                let row = 2 + i;
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

/// Build the "files view" URL for a PR by inserting `/files` on the
/// path component of the PR's canonical URL.
///
/// The helper splits `pr_url` into (path, query, fragment) so a URL
/// like `https://github.com/o/r/pull/100?diff=split` becomes
/// `https://github.com/o/r/pull/100/files?diff=split` — the `/files`
/// segment lands on the path, before any `?` or `#`. A URL that
/// already ends in `/files` (after trimming trailing slashes) is
/// returned unchanged, so the helper is idempotent. An empty input
/// returns the empty string — the caller is responsible for not
/// invoking `open ""`.
///
/// Pure helper — no IO. Used by `TuiApp::open_pr`.
fn pr_files_url(pr_url: &str) -> String {
    if pr_url.is_empty() {
        return String::new();
    }
    // Split off fragment first so `?` inside it is treated as content.
    let (without_fragment, fragment) = match pr_url.split_once('#') {
        Some((head, tail)) => (head, Some(tail)),
        None => (pr_url, None),
    };
    let (path, query) = match without_fragment.split_once('?') {
        Some((head, tail)) => (head, Some(tail)),
        None => (without_fragment, None),
    };
    let trimmed_path = path.trim_end_matches('/');
    let new_path = if trimmed_path.ends_with("/files") {
        trimmed_path.to_string()
    } else {
        format!("{}/files", trimmed_path)
    };
    let mut result = new_path;
    if let Some(q) = query {
        result.push('?');
        result.push_str(q);
    }
    if let Some(f) = fragment {
        result.push('#');
        result.push_str(f);
    }
    result
}

/// Compose the GitHub issue URL for the smallest issue number a flow
/// references. Returns `None` when the flow has no issues OR when no
/// repo is available (neither the state's `repo` key nor the
/// fallback) OR when the resolved repo is the empty string.
///
/// Empty-string repo is treated as missing in BOTH the state-file and
/// fallback positions — mirrors the `orch_issue_url` empty-repo guard
/// so a corrupt state file with `repo: ""` falls back to the parameter
/// (or `None`) instead of producing the malformed URL
/// `https://github.com//issues/<n>`.
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
        .filter(|s| !s.is_empty())
        .or_else(|| fallback_repo.filter(|s| !s.is_empty()))?;
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

/// Escape a string for safe interpolation inside an AppleScript
/// double-quoted string literal. AppleScript treats `\` and `"` as
/// the only structural characters inside `"..."` literals — every
/// other byte (including newlines) is content. Escaping both with
/// a leading backslash collapses the injection surface to zero.
///
/// Pure helper — used by [`build_iterm_activation_script`].
fn escape_applescript_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch == '\\' || ch == '"' {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Build the AppleScript text that asks iTerm2 to find and select the
/// tab whose session tty matches `session_tty`.
///
/// `session_tty` is read from the flow's state JSON, which is a file
/// on disk that any process with filesystem access can write to. A
/// crafted value containing `"` would otherwise close the AppleScript
/// string literal early and inject arbitrary AppleScript that
/// `osascript` then runs with the user's privileges. The interpolation
/// runs through [`escape_applescript_string`] to neutralize that
/// vector.
///
/// Pure helper — no IO. The osascript invocation lives in
/// `TuiApp::activate_iterm_tab`.
fn build_iterm_activation_script(session_tty: &str) -> String {
    let escaped = escape_applescript_string(session_tty);
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
        tty = escaped
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
