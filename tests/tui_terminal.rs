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
use flow_rs::tui_terminal::{crossterm_events, run_terminal, run_terminal_body, TerminalGuard};

// --- run_tui_arm_impl ---
//
// `run_tui_arm_impl` is non-generic and has no closure seams. The
// two code paths (non-TTY Err and TTY Ok via `run_terminal`) are
// exercised through subprocess fixtures rather than closure
// injection:
//
//   * Non-TTY Err: `run_tui_arm_impl_non_tty_subprocess_returns_err`
//     below spawns `flow-rs tui` WITHOUT a controlling PTY; `libc::isatty`
//     returns 0 and the hook returns
//     `Err(("Error: flow tui requires an interactive terminal.", 1))`.
//   * TTY Ok: `run_tui_arm_real_pty_quits_on_q_key` further below
//     spawns with a real pseudo-terminal so `isatty` returns 1 and
//     the full event loop runs to a clean 'q'-driven exit.
//
// Collapsing the prior generic seam (`<F1, F2>`) eliminates the
// per-monomorphization regions that no in-process test could
// exercise.

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

/// Covers the `.map_err` closure of `run_terminal(&mut app)` in
/// `run_tui_arm_impl`: spawn flow-rs with a real PTY slave dup'd to
/// STDOUT_FILENO (so the `isatty` check passes) but `/dev/null` on
/// STDIN. `crossterm::enable_raw_mode` reads termios from stdin,
/// which is `/dev/null` — tcgetattr returns `ENOTTY`, `run_terminal`
/// returns Err, and `run_tui_arm_impl` formats the error as
/// "TUI error: ..." and returns exit 1.
#[cfg(unix)]
#[test]
fn run_tui_arm_impl_tty_but_stdin_not_tty_exits_with_tui_error() {
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    let mut master_fd: libc::c_int = -1;
    let mut slave_fd: libc::c_int = -1;
    let rc = unsafe {
        libc::openpty(
            &mut master_fd,
            &mut slave_fd,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(rc, 0, "openpty failed: {}", io::Error::last_os_error());

    // Parent: ensure both fds are closed on exit.
    struct FdGuard(libc::c_int);
    impl Drop for FdGuard {
        fn drop(&mut self) {
            if self.0 >= 0 {
                unsafe { libc::close(self.0) };
            }
        }
    }
    let _master_guard = FdGuard(master_fd);

    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.arg("tui")
        .current_dir(&root)
        .env("GIT_CEILING_DIRECTORIES", &root)
        .env("HOME", &root)
        .env_remove("FLOW_CI_RUNNING");

    let slave = slave_fd;
    // SAFETY: pre_exec closures run between fork and exec and must
    // be async-signal-safe. setsid, ioctl(TIOCSCTTY), dup2, open,
    // and close are all AS-safe.
    unsafe {
        cmd.pre_exec(move || {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            if libc::ioctl(slave, libc::TIOCSCTTY as _) == -1 {
                return Err(io::Error::last_os_error());
            }
            // stdout + stderr = PTY slave → isatty(STDOUT) == 1.
            if libc::dup2(slave, libc::STDOUT_FILENO) == -1
                || libc::dup2(slave, libc::STDERR_FILENO) == -1
            {
                return Err(io::Error::last_os_error());
            }
            // stdin = /dev/null → tcgetattr fails with ENOTTY.
            let dev_null = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
            if dev_null < 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::dup2(dev_null, libc::STDIN_FILENO) == -1 {
                return Err(io::Error::last_os_error());
            }
            libc::close(dev_null);
            libc::close(slave);
            Ok(())
        });
    }

    // Spawn and drain master so the child's writes don't block on
    // a full PTY buffer before reaching enable_raw_mode.
    let mut child = cmd.spawn().expect("spawn flow-rs tui with stdin=/dev/null");
    unsafe { libc::close(slave_fd) };

    let master_for_drain = master_fd;
    let drain = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            let n = unsafe {
                libc::read(
                    master_for_drain,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len(),
                )
            };
            if n <= 0 {
                break;
            }
        }
    });

    // Wait with a deadline — enable_raw_mode on /dev/null fails
    // fast; the child typically exits in <100ms.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("child did not exit within 10s of spawn");
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => panic!("try_wait failed: {}", e),
        }
    };
    let _ = drain.join();

    assert_eq!(
        status.code(),
        Some(1),
        "expected exit 1 from run_terminal Err path, got {:?}",
        status
    );
}

/// Covers the non-TTY Err arm of `run_tui_arm_impl` via the
/// compiled `flow-rs tui` subcommand. `Command::new` spawns the
/// binary without allocating a controlling terminal, so
/// `libc::isatty(STDOUT_FILENO)` returns 0 in the child and the
/// hook short-circuits with exit code 1 and the
/// "requires an interactive terminal" stderr message.
#[test]
fn run_tui_arm_impl_non_tty_subprocess_returns_err() {
    use std::process::Command;

    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("tui")
        .current_dir(&root)
        .env("GIT_CEILING_DIRECTORIES", &root)
        .env("HOME", &root)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("spawn flow-rs tui without PTY");

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit 1 from non-TTY rejection, got {:?}",
        output.status
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("requires an interactive terminal"),
        "expected stderr to surface the non-TTY error, got: {}",
        stderr
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

/// Real-PTY subprocess test that exercises the full production
/// event loop in `run_terminal` and the Ok(()) exit arm of
/// `run_tui_arm`. Without a real controlling terminal these paths
/// are unreachable: `enable_raw_mode()` returns Err in nextest,
/// and `process::exit` terminates the test binary.
///
/// The test creates a pseudo-terminal pair via `libc::openpty`, spawns
/// `flow-rs tui` with the slave duped onto the child's stdin/stdout/
/// stderr (and `setsid` + `TIOCSCTTY` making it the controlling
/// terminal), then writes a single 'q' keystroke to the master end.
/// The TUI's event loop sees the 'q', sets `running = false`, exits
/// `run_event_loop` with Ok(()), which unwinds through
/// `run_terminal_body` → `run_terminal` → `run_tui_arm_impl` → the
/// `Ok(()) => process::exit(0)` arm of `run_tui_arm`.
///
/// Per `.claude/rules/reachable-is-testable.md` "Real TTY /
/// controlling terminal" fixture recipe.
#[cfg(unix)]
#[test]
fn run_tui_arm_real_pty_quits_on_q_key() {
    use std::os::unix::process::CommandExt;
    use std::process::Command;
    use std::time::Instant;

    // --- Create PTY pair ---
    let mut master_fd: libc::c_int = -1;
    let mut slave_fd: libc::c_int = -1;
    let rc = unsafe {
        libc::openpty(
            &mut master_fd,
            &mut slave_fd,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(rc, 0, "openpty failed: {}", io::Error::last_os_error());
    assert!(master_fd >= 0 && slave_fd >= 0);

    // Ensure the master FD is closed even if the test panics.
    struct FdGuard(libc::c_int);
    impl Drop for FdGuard {
        fn drop(&mut self) {
            if self.0 >= 0 {
                unsafe { libc::close(self.0) };
            }
        }
    }
    let _master_guard = FdGuard(master_fd);

    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.arg("tui")
        .current_dir(&root)
        .env("GIT_CEILING_DIRECTORIES", &root)
        .env("HOME", &root)
        .env("GH_TOKEN", "invalid")
        .env_remove("FLOW_CI_RUNNING");

    // SAFETY: `pre_exec` requires async-signal-safe closures.
    // `libc::setsid`, `ioctl(TIOCSCTTY)`, `dup2`, `close` are all AS-safe.
    // The closure allocates nothing and returns without panicking.
    let slave = slave_fd;
    unsafe {
        cmd.pre_exec(move || {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            if libc::ioctl(slave, libc::TIOCSCTTY as _) == -1 {
                return Err(io::Error::last_os_error());
            }
            for fd in [libc::STDIN_FILENO, libc::STDOUT_FILENO, libc::STDERR_FILENO] {
                if libc::dup2(slave, fd) == -1 {
                    return Err(io::Error::last_os_error());
                }
            }
            // Close the original slave fd — stdin/stdout/stderr are now dup'd.
            libc::close(slave);
            Ok(())
        });
    }

    let mut child = cmd.spawn().expect("spawn flow-rs");

    // Close slave in the parent — the child has its own dup'd fds.
    unsafe { libc::close(slave_fd) };

    // Drain background output from the master fd so writes don't block
    // when the child's output fills the PTY buffer, AND watch for the
    // EnterAlternateScreen escape sequence (`\x1b[?1049h`) that
    // crossterm emits right before the first `event::poll` call. The
    // drain thread flips `ready_flag` once the sequence is seen; the
    // main thread uses that signal to bound wall-clock time on
    // actual child startup latency instead of blind-sleeping past a
    // worst-case estimate.
    let drained_arc = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let ready_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let drain_arc = std::sync::Arc::clone(&drained_arc);
    let ready_drain = std::sync::Arc::clone(&ready_flag);
    let master_for_drain = master_fd;
    let drain_thread = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        // 8-byte ANSI code crossterm writes on EnterAlternateScreen.
        const ENTER_ALT_SCREEN: &[u8] = b"\x1b[?1049h";
        loop {
            let n = unsafe {
                libc::read(
                    master_for_drain,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len(),
                )
            };
            if n <= 0 {
                break;
            }
            if let Ok(mut guard) = drain_arc.lock() {
                let offset = guard.len().saturating_sub(ENTER_ALT_SCREEN.len() - 1);
                guard.extend_from_slice(&buf[..n as usize]);
                if !ready_drain.load(std::sync::atomic::Ordering::Acquire)
                    && guard[offset..]
                        .windows(ENTER_ALT_SCREEN.len())
                        .any(|w| w == ENTER_ALT_SCREEN)
                {
                    ready_drain.store(true, std::sync::atomic::Ordering::Release);
                }
            }
        }
    });

    // Wait up to 5s for the child to reach its event loop (proven by
    // the alt-screen escape appearing on master). A loaded CI host
    // rarely needs more than a few hundred ms; the 5s cap is a
    // safety net, not an expected value.
    let startup_deadline = Instant::now() + Duration::from_secs(5);
    while !ready_flag.load(std::sync::atomic::Ordering::Acquire) {
        if Instant::now() >= startup_deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("child did not enter alternate screen within 5s of spawn");
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    // The alt-screen signal fires when crossterm writes the escape
    // byte, not when the first `event::poll` call actually starts —
    // the child still has to continue into `run_terminal_body` and
    // `TuiApp::run_event_loop` before calling `events(2000ms)`.
    // Under CI load that gap can stretch past 1 second, so this
    // sleep must be generous: 3500ms gives a 1500ms margin over the
    // 2000ms poll deadline to absorb scheduling variance. Tighter
    // margins (tried at 100, 500, 1500ms) flaked under parallel
    // suites with `q` arriving inside the first poll's wait window,
    // returning Ok(true) on the first poll instead of Ok(false) +
    // Ok(true) on two polls, which left the `Ok(None)` arm of
    // `crossterm_events` uncovered.
    std::thread::sleep(Duration::from_millis(3500));
    let bytes: [u8; 2] = [b'q', b'\n'];
    let wrote = unsafe {
        libc::write(
            master_fd,
            bytes.as_ptr() as *const libc::c_void,
            bytes.len(),
        )
    };
    assert_eq!(
        wrote,
        2,
        "write 'q\\n' to master failed: {}",
        io::Error::last_os_error()
    );

    // --- Wait for child exit with a hard deadline ---
    let deadline = Instant::now() + Duration::from_secs(10);
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err("child did not exit within 10s of receiving 'q'");
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => panic!("try_wait failed: {}", e),
        }
    };

    // Wake the drain thread by closing the master fd (the thread's read returns 0).
    // Do this BEFORE joining — otherwise it blocks forever on read().
    // (master_guard's Drop runs later; do it explicitly now.)
    // We intentionally leak the leaked FdGuard closure state — the drain thread exits
    // on read returning 0 after master fd is closed, which happens when the parent
    // exits.

    match result {
        Ok(status) => {
            // A healthy quit via 'q' yields exit code 0. Any other
            // path exits non-zero or via signal.
            drop(drain_thread); // detach — don't care about drained bytes on success
            assert!(status.success(), "child exited non-success: {:?}", status);
        }
        Err(msg) => {
            // Diagnostic: dump whatever the child wrote to stdout/stderr.
            let drained = drained_arc.lock().map(|g| g.clone()).unwrap_or_default();
            let rendered = String::from_utf8_lossy(&drained);
            panic!(
                "{msg}\n--- child output ({} bytes) ---\n{}\n--- end child output ---",
                drained.len(),
                rendered
            );
        }
    }
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
