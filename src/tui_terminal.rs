//! Crossterm glue and TUI arm dispatch for `bin/flow tui`.
//!
//! Lives in a lib module so the production wrapper exposes seams
//! (`is_tty_fn`, `run_terminal_fn`, `release_fn`) for unit tests
//! that cannot touch a real TTY. Keeping this glue out of `main.rs`
//! allows `main.rs` to reach 100% coverage — the seam-injected
//! [`run_tui_arm_impl`] covers each branch directly, while the
//! crossterm-bound [`run_terminal`] only executes in production
//! (real TTY required) and contributes its uncovered lines to this
//! module's measurement instead of `main.rs`'s.
//!
//! Tests live at tests/tui_terminal.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use std::cell::RefCell;
use std::io;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use crossterm::event;
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::{Frame, Terminal};

use crate::tui::{DrawFn, EventSourceFn, TuiApp, TuiAppPlatform};

/// Top-level dispatch for the `Tui` match arm in `main.rs`. Never
/// returns: always terminates the process via `process::exit`, either
/// with `0` on event-loop success or with the `(msg, code)` tuple
/// from a non-TTY rejection / event-loop failure (printed to stderr).
///
/// Keeping the `exit` call inside this wrapper leaves main.rs's Tui
/// arm as a single fully-covered expression. The seam-injected
/// [`run_tui_arm_impl`] below is the unit-testable variant — it
/// returns the `Result` so tests can assert on each branch without
/// terminating the test process.
pub fn run_tui_arm(root: &Path) -> ! {
    let result = run_tui_arm_impl(
        || unsafe { libc::isatty(libc::STDOUT_FILENO) != 0 },
        run_terminal,
        root,
    );
    match result {
        Ok(()) => std::process::exit(0),
        Err((msg, code)) => {
            eprintln!("{}", msg);
            std::process::exit(code);
        }
    }
}

/// Seam-injected variant of [`run_tui_arm`]. Tests pass mock
/// `is_tty_fn` and `run_terminal_fn` closures to drive each branch
/// without touching a real terminal.
pub fn run_tui_arm_impl<F1, F2>(
    is_tty_fn: F1,
    run_terminal_fn: F2,
    root: &Path,
) -> Result<(), (String, i32)>
where
    F1: FnOnce() -> bool,
    F2: FnOnce(&mut TuiApp) -> io::Result<()>,
{
    if !is_tty_fn() {
        return Err((
            "Error: flow tui requires an interactive terminal.".to_string(),
            1,
        ));
    }
    let version = crate::utils::read_version();
    let repo = crate::github::detect_repo(Some(root));
    let mut app = TuiApp::new(
        root.to_path_buf(),
        version,
        repo,
        TuiAppPlatform::production(),
    );
    match run_terminal_fn(&mut app) {
        Ok(()) => Ok(()),
        Err(e) => Err((format!("TUI error: {}", e), 1)),
    }
}

/// Production crossterm event loop. Enables raw mode, enters the
/// alternate screen, builds the ratatui Terminal, constructs a
/// [`TerminalGuard`] whose Drop restores the terminal on every exit
/// path including panic unwind, then drives `app.run_event_loop`.
///
/// Only executes against a real TTY — `cargo nextest` cannot drive
/// this body, so its branches contribute to this module's coverage
/// rather than `main.rs`'s.
pub fn run_terminal(app: &mut TuiApp) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Rc::new(RefCell::new(Terminal::new(backend)?));

    let cleanup_terminal = Rc::clone(&terminal);
    let _guard = TerminalGuard::new(move || {
        let _ = disable_raw_mode();
        let _ = execute!(
            cleanup_terminal.borrow_mut().backend_mut(),
            LeaveAlternateScreen
        );
    });

    let draw_terminal = Rc::clone(&terminal);
    let draw: DrawFn = Box::new(move |render_fn: &mut dyn FnMut(&mut Frame)| {
        draw_terminal.borrow_mut().draw(|f| render_fn(f))?;
        Ok(())
    });

    let events: EventSourceFn = Box::new(|timeout: Duration| -> io::Result<Option<event::Event>> {
        if event::poll(timeout)? {
            Ok(Some(event::read()?))
        } else {
            Ok(None)
        }
    });

    app.run_event_loop(draw, events)
}

/// RAII guard that runs `release_fn` on drop. Constructed with an
/// arbitrary closure so production passes the crossterm restore
/// logic and unit tests pass a flag-setting closure.
///
/// Panic-safe by construction: Rust drops every value on the stack
/// during unwind, so `release_fn` runs even when a panic escapes the
/// event loop. Closure errors must be swallowed inside `release_fn`
/// itself — `Drop` cannot return them. `release_fn` runs at most once
/// because [`Drop::drop`] takes ownership via `Option::take`.
pub struct TerminalGuard<F: FnMut()> {
    release_fn: Option<F>,
}

impl<F: FnMut()> TerminalGuard<F> {
    pub fn new(release_fn: F) -> Self {
        Self {
            release_fn: Some(release_fn),
        }
    }
}

impl<F: FnMut()> Drop for TerminalGuard<F> {
    fn drop(&mut self) {
        if let Some(mut f) = self.release_fn.take() {
            f();
        }
    }
}
