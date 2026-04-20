//! Library-level tests for `flow_rs::tui_terminal`. Migrated from
//! inline `#[cfg(test)]` per `.claude/rules/test-placement.md`.

use std::cell::RefCell;
use std::io;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::rc::Rc;

use flow_rs::tui_terminal::{run_tui_arm_impl, TerminalGuard};

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
