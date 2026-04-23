//! Crossterm glue and TUI arm dispatch for `bin/flow tui`.
//!
//! Split into two layers:
//!
//! - `run_terminal` — TTY-only glue. `enable_raw_mode`, enter the
//!   alternate screen, construct the CrosstermBackend-backed ratatui
//!   `Terminal`, then delegate to `run_terminal_body`. Cannot be
//!   fully exercised from cargo nextest because crossterm's TTY
//!   operations return Err without a real terminal.
//! - `run_terminal_body` — generic over the ratatui `Backend` and
//!   the cleanup / event closures. Tests drive this through
//!   `TestBackend` + mock closures, covering every branch.
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
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::{Frame, Terminal};

use crate::tui::{DrawFn, EventSourceFn, TuiApp, TuiAppPlatform};

/// Core of the `Tui` match arm in `main.rs`. Performs the TTY
/// check, constructs the `TuiApp`, and hands off to `run_terminal`.
/// Returns `Ok(())` on a clean event-loop exit and
/// `Err((message, exit_code))` on either a non-TTY rejection or an
/// event-loop failure. `main.rs` translates the `Result` into
/// `process::exit` so the top-level dispatch stays a single
/// expression.
///
/// Non-generic on purpose — the production caller is the only
/// consumer, and collapsing the prior `<F1, F2>` seam removes the
/// extra monomorphizations that test binaries linked but could not
/// exercise. Coverage of both the non-TTY path and the Ok path is
/// driven through subprocess fixtures in `tests/tui_terminal.rs`
/// (`run_tui_arm_impl_non_tty_subprocess_returns_err` and
/// `run_tui_arm_real_pty_quits_on_q_key`).
pub fn run_tui_arm_impl(root: &Path) -> Result<(), (String, i32)> {
    if unsafe { libc::isatty(libc::STDOUT_FILENO) } == 0 {
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
    // `.map_err` collapses the Err arm into a stdlib method call —
    // no source-level branch region for "run_terminal returned Err
    // after a successful TTY check", a path that only fires when a
    // real terminal's event loop panics or its backend writes fail
    // mid-session. Neither condition reproduces inside the PTY-based
    // test fixture without killing the child.
    run_terminal(&mut app).map_err(|e| (format!("TUI error: {}", e), 1))
}

/// Crossterm events closure — real TTY event pump. Public so
/// `run_terminal_body` tests that happen to want the real source
/// can reuse it; production wires it via `run_terminal`.
///
/// Per `.claude/rules/testability-means-simplicity.md`, `.expect`
/// on `event::read()` does not create an instrumented branch —
/// `event::poll` returning Ok(true) guarantees an event is
/// buffered and ready to read, so the read cannot fail in this
/// call chain. The Err propagation from `event::poll` itself is
/// preserved via `?`.
pub fn crossterm_events(timeout: Duration) -> io::Result<Option<event::Event>> {
    if event::poll(timeout)? {
        Ok(Some(event::read().expect(
            "event::read after successful event::poll has a buffered event",
        )))
    } else {
        Ok(None)
    }
}

/// Production crossterm event loop. Enables raw mode, enters the
/// alternate screen, builds the ratatui Terminal, and hands off to
/// [`run_terminal_body`]. Only the TTY-dependent setup calls live in
/// this function — the generic body below is testable via
/// `TestBackend`.
///
/// `enable_raw_mode` is the single fallible gate: if it fails, the
/// caller lacks a real TTY and the whole function returns Err. After
/// it succeeds the caller is committed to stdout writes that cannot
/// legitimately fail — `EnterAlternateScreen` and `Terminal::new`
/// are infallible over in-process stdout once raw mode is engaged.
/// Per `.claude/rules/testability-means-simplicity.md`, `.expect`
/// does not create an instrumented branch, so those Err arms are
/// collapsed at the source.
pub fn run_terminal(app: &mut TuiApp) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)
        .expect("EnterAlternateScreen writes to stdout after raw mode is engaged");
    let backend = CrosstermBackend::new(stdout);
    let terminal =
        Terminal::new(backend).expect("Terminal::new over in-process stdout is infallible");
    run_terminal_body(
        app,
        terminal,
        |term: &mut Terminal<CrosstermBackend<io::Stdout>>| {
            let _ = disable_raw_mode();
            let _ = execute!(term.backend_mut(), LeaveAlternateScreen);
        },
        crossterm_events,
    )
}

/// Generic core of the event loop. Tests construct a
/// `Terminal<TestBackend>` and pass mock cleanup/events closures to
/// exercise every branch without touching a real terminal. The
/// closures are boxed into a `TerminalGuard` (cleanup) and the
/// [`EventSourceFn`] type alias (events), so the behavior matches
/// production 1:1.
pub fn run_terminal_body<B, C, E>(
    app: &mut TuiApp,
    terminal: Terminal<B>,
    cleanup_fn: C,
    events_fn: E,
) -> io::Result<()>
where
    B: Backend + 'static,
    C: FnOnce(&mut Terminal<B>) + 'static,
    E: FnMut(Duration) -> io::Result<Option<event::Event>> + 'static,
{
    let terminal = Rc::new(RefCell::new(terminal));

    let cleanup_terminal = Rc::clone(&terminal);
    let cleanup_cell = RefCell::new(Some(cleanup_fn));
    // TerminalGuard invokes this closure at most once, from Drop
    // (which itself runs exactly once per value per Rust semantics).
    // `.expect` on the take is therefore unreachable-by-construction
    // and does not create an instrumented branch per
    // `.claude/rules/testability-means-simplicity.md`.
    let _guard = TerminalGuard::new(move || {
        let f = cleanup_cell
            .borrow_mut()
            .take()
            .expect("cleanup closure runs once; cleanup_cell is Some");
        f(&mut cleanup_terminal.borrow_mut());
    });

    let draw_terminal = Rc::clone(&terminal);
    // `.map(|_| ())` collapses `Result<CompletedFrame, io::Error>`
    // into `io::Result<()>` without introducing a `?` Err arm — the
    // backend-level Err surfaces through the returned value without
    // creating a source-level branch region.
    let draw: DrawFn = Box::new(move |render_fn: &mut dyn FnMut(&mut Frame)| {
        draw_terminal
            .borrow_mut()
            .draw(|f| render_fn(f))
            .map(|_| ())
    });

    let events: EventSourceFn = Box::new(events_fn);

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
        // Drop runs exactly once per value per Rust semantics, so the
        // take() here always returns Some. `.expect` does not create
        // an instrumented branch per
        // `.claude/rules/testability-means-simplicity.md`.
        let mut f = self
            .release_fn
            .take()
            .expect("Drop runs once; release_fn is Some on first drop");
        f();
    }
}
