//! Library-level tests for `flow_rs::tui_terminal`. Migrated from
//! inline `#[cfg(test)]` per `.claude/rules/test-placement.md`.

use std::cell::RefCell;
use std::io;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use flow_rs::tui::{TuiApp, TuiAppPlatform};
use flow_rs::tui_terminal::{
    crossterm_events, run_terminal, run_terminal_body, run_tui_arm_impl, TerminalGuard,
};

// --- run_tui_arm_impl ---

#[test]
fn tui_terminal_run_arm_non_tty_returns_err() {
    let root = PathBuf::from("/tmp/tui-non-tty-fixture");
    let run_terminal_called = Rc::new(RefCell::new(false));
    let called_clone = Rc::clone(&run_terminal_called);
    let result = run_tui_arm_impl(
        || false,
        move |_app| {
            *called_clone.borrow_mut() = true;
            Ok(())
        },
        &root,
    );
    match result {
        Err((msg, code)) => {
            assert_eq!(code, 1);
            assert_eq!(msg, "Error: flow tui requires an interactive terminal.");
        }
        Ok(()) => panic!("expected Err, got Ok"),
    }
    assert!(
        !*run_terminal_called.borrow(),
        "run_terminal_fn must not be called when is_tty_fn returns false"
    );
}

#[test]
fn tui_terminal_run_arm_tty_ok_returns_ok() {
    let root = PathBuf::from("/tmp/tui-tty-ok-fixture");
    let run_terminal_called = Rc::new(RefCell::new(false));
    let called_clone = Rc::clone(&run_terminal_called);
    let result = run_tui_arm_impl(
        || true,
        move |_app| {
            *called_clone.borrow_mut() = true;
            Ok(())
        },
        &root,
    );
    assert!(matches!(result, Ok(())));
    assert!(
        *run_terminal_called.borrow(),
        "run_terminal_fn must be called when is_tty_fn returns true"
    );
}

#[test]
fn tui_terminal_run_arm_tty_err_returns_tui_error_tuple() {
    let root = PathBuf::from("/tmp/tui-tty-err-fixture");
    let result = run_tui_arm_impl(|| true, |_app| Err(io::Error::other("oops")), &root);
    match result {
        Err((msg, code)) => {
            assert_eq!(code, 1);
            assert!(
                msg.contains("TUI error"),
                "expected message to contain 'TUI error', got: {}",
                msg
            );
            assert!(
                msg.contains("oops"),
                "expected message to surface inner io::Error, got: {}",
                msg
            );
        }
        Ok(()) => panic!("expected Err, got Ok"),
    }
}

// --- TerminalGuard ---

#[test]
fn tui_terminal_guard_releases_on_panic() {
    let released = Rc::new(RefCell::new(false));
    let released_clone = Rc::clone(&released);
    let result = catch_unwind(AssertUnwindSafe(|| {
        let _guard = TerminalGuard::new(move || {
            *released_clone.borrow_mut() = true;
        });
        panic!("simulated work failure");
    }));
    assert!(result.is_err(), "panic must propagate to catch_unwind");
    assert!(
        *released.borrow(),
        "release_fn must run on Drop during panic unwind"
    );
}

// --- run_terminal_body (TestBackend-driven) ---

fn make_key_event(code: KeyCode) -> Event {
    Event::Key(KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    })
}

fn make_test_app(root: PathBuf) -> TuiApp {
    TuiApp::new(
        root,
        "0.0.0".to_string(),
        None,
        TuiAppPlatform::production(),
    )
}

/// Drives `run_terminal_body` with a `TestBackend` and an events
/// closure that feeds a single 'q' keystroke. The event loop quits
/// on 'q' (see `TuiApp::handle_key`), so the function returns
/// Ok(()) and the cleanup closure fires via the `TerminalGuard`.
#[test]
fn run_terminal_body_returns_ok_on_quit_key() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = make_test_app(tmp.path().to_path_buf());
    let backend = TestBackend::new(80, 24);
    let terminal = Terminal::new(backend).unwrap();

    let cleanup_calls = Rc::new(RefCell::new(0));
    let cleanup_clone = Rc::clone(&cleanup_calls);

    // Queue: first poll returns 'q' (quits), subsequent polls return
    // None (timeout — irrelevant, loop already exiting). Using a
    // Rc<RefCell<Vec>> lets the FnMut closure track state.
    let queue = Rc::new(RefCell::new(vec![make_key_event(KeyCode::Char('q'))]));
    let queue_clone = Rc::clone(&queue);

    let result = run_terminal_body(
        &mut app,
        terminal,
        move |_term| {
            *cleanup_clone.borrow_mut() += 1;
        },
        move |_timeout: Duration| -> io::Result<Option<Event>> {
            Ok(queue_clone.borrow_mut().pop())
        },
    );

    assert!(result.is_ok(), "expected Ok on quit, got {:?}", result);
    assert_eq!(
        *cleanup_calls.borrow(),
        1,
        "cleanup must run exactly once via TerminalGuard::drop"
    );
}

/// Covers `crossterm_events` directly with a tiny timeout — without
/// a TTY, `event::poll` returns immediately with either Ok(false)
/// (no events) or Err. Either outcome exercises the function entry
/// and the polled-False path.
#[test]
fn crossterm_events_under_nextest_returns_without_panic() {
    // Call twice to exercise both the "poll returned false" and any
    // subsequent variants. We don't assert on exit — under nextest
    // stdin may not be a terminal, producing implementation-defined
    // behavior. The test's job is coverage of the function body.
    let _ = crossterm_events(Duration::from_millis(1));
    let _ = crossterm_events(Duration::from_millis(1));
}

/// Covers `run_terminal` directly — without a TTY,
/// `enable_raw_mode()?` returns Err and the function returns early,
/// covering the function entry and the ? propagation.
#[test]
fn run_terminal_without_tty_returns_err() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = TuiApp::new(
        tmp.path().to_path_buf(),
        "0.0.0".to_string(),
        None,
        TuiAppPlatform::production(),
    );
    let result = run_terminal(&mut app);
    // nextest subprocess has no TTY; enable_raw_mode returns Err.
    assert!(
        result.is_err(),
        "expected Err from enable_raw_mode without TTY, got {:?}",
        result
    );
}

/// Drives `run_terminal_body` with an events closure that always
/// returns None (timeout). The event loop calls `refresh_data` on
/// each timeout; eventually we inject a 'q' to break out. Covers
/// the None arm of the events match in `TuiApp::run_event_loop`.
#[test]
fn run_terminal_body_handles_timeout_then_quit() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = make_test_app(tmp.path().to_path_buf());
    let backend = TestBackend::new(80, 24);
    let terminal = Terminal::new(backend).unwrap();

    // First 2 polls return None, third returns 'q'.
    let counter = Rc::new(RefCell::new(0u32));
    let counter_clone = Rc::clone(&counter);

    let result = run_terminal_body(
        &mut app,
        terminal,
        move |_term| {},
        move |_timeout: Duration| -> io::Result<Option<Event>> {
            let mut c = counter_clone.borrow_mut();
            *c += 1;
            if *c >= 3 {
                Ok(Some(make_key_event(KeyCode::Char('q'))))
            } else {
                Ok(None)
            }
        },
    );

    assert!(result.is_ok());
    assert!(
        *counter.borrow() >= 3,
        "events_fn must be called at least 3 times"
    );
}
