# Panic-Safe Resource Cleanup

When code acquires a resource that REQUIRES cleanup before normal
termination — terminal raw mode, alternate-screen buffer, file
locks, network connections, lockfiles, GPU contexts, ANY mode that
must be reset for the process exit to be sane — the cleanup MUST
run via a Drop-implementing RAII guard, not via inline cleanup
calls at function exit.

## Why

Issue #1135 / PR #1154 surfaced this in the TUI: `run_tui_terminal`
enabled crossterm's raw mode and entered the alternate screen, ran
the event loop, then called `disable_raw_mode()` and
`LeaveAlternateScreen` AFTER the loop returned. Rust panics unwind
the stack — they do NOT execute code after the panicking call. A
panic inside the event loop would skip the inline cleanup, leaving
the user's terminal in raw mode with no echo, no line discipline,
and the alternate screen still active. Recovery requires `reset`
or closing the terminal tab.

Inline cleanup at scope-exit is a footgun for any resource whose
"unset" state must be restored. The Drop impl runs on every exit
path including panic unwind — that is the only mechanism the
language guarantees.

## The Rule

For every code path that does:

```rust
acquire_resource()?;
do_work();        // <-- can panic
release_resource(); // <-- skipped on panic
```

Replace the pattern with a Drop guard:

```rust
struct ResourceGuard { /* holds whatever release_resource needs */ }

impl Drop for ResourceGuard {
    fn drop(&mut self) {
        // best-effort release; cannot return errors
        let _ = release_resource(self);
    }
}

fn do_thing() -> Result<()> {
    acquire_resource()?;
    let _guard = ResourceGuard { /* ... */ };
    do_work()  // panic-safe; _guard drops on unwind
}
```

## What Counts as a Resource Requiring Cleanup

- **Terminal modes** — raw mode, alternate screen, mouse capture,
  bracketed paste. Reference: `TerminalGuard` in `src/main.rs`.
- **File locks** — `flock`, `fcntl` advisory locks. The lockfile
  must be released even on panic or the next process blocks
  forever (or until 30-minute stale timeout, in FLOW's case).
- **Spawned child processes you intend to wait on** — orphaning
  is sometimes acceptable but only after explicit decision.
- **Mutated global state** — env vars set for the duration of an
  operation, signal handlers swapped temporarily, anything that
  the process exit will not naturally restore.
- **Open file descriptors with side effects** — fsync-pending
  writes, named pipes opened for write, sockets that need
  `shutdown()`.

NOT every resource needs a Drop guard. Allocations that release
naturally on drop (Vec, String, Box) handle themselves through
their own Drop impls. The discipline applies specifically to
resources whose "released" state is not the default.

## Reference Implementation

The canonical example is `TerminalGuard` in `src/main.rs`:

```rust
struct TerminalGuard {
    terminal: Rc<RefCell<Terminal<CrosstermBackend<Stdout>>>>,
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        use crossterm::execute;
        use crossterm::terminal::{disable_raw_mode, LeaveAlternateScreen};
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.borrow_mut().backend_mut(),
            LeaveAlternateScreen
        );
    }
}
```

The guard:

1. Owns whatever release operations need (here, a shared handle
   to the same terminal the draw closure renders into)
2. Implements `Drop` with errors swallowed (`let _ = ...`) because
   Drop cannot return them and a panic-during-cleanup is worse
   than a swallowed error
3. Is placed in scope BEFORE the work that might panic, so
   stack-unwinding drops it
4. Is a named struct (not `defer!`-style scope_guard crate) so
   the responsibility is documented in the type system

## Plan-Phase Trigger

When a plan task acquires a resource of any of the categories
listed above ("What Counts" section), the plan must enumerate:

1. The **resource** being acquired (terminal mode, file lock,
   etc.)
2. The **release call** that must run on every exit path
3. The **guard struct name** that wraps the release in Drop
4. Where the guard is placed in scope (must be BEFORE the
   panic-prone work)

A plan that says "guarantee cleanup on every return path" without
naming the Drop guard is incomplete. "Cleanup before each return"
or "cleanup in a deferred block" are anti-patterns — they do not
survive panic.

## How to Apply (Code Phase)

1. Define the guard struct first. Implement Drop with error
   swallowing.
2. Place the guard in scope IMMEDIATELY after acquiring the
   resource — before any work that might panic.
3. Test the guard explicitly. The simplest test acquires the
   resource, panics, catches the panic, and verifies the resource
   is released:
   ```rust
   #[test]
   fn guard_releases_on_panic() {
       let result = std::panic::catch_unwind(|| {
           let _guard = ResourceGuard::acquire();
           panic!("simulated work failure");
       });
       assert!(result.is_err());
       assert!(resource_is_released()); // <-- the load-bearing assertion
   }
   ```
4. Document the guard's Drop behavior in its type doc comment —
   what gets cleaned up, what errors are swallowed, why.

## How to Apply (Code Review Phase)

When the reviewer agent or pre-mortem agent finds resource
acquisition in production code, verify:

1. The release path is in a Drop impl, not inline at scope-exit
2. The guard is in scope BEFORE any operation that might panic
3. There is a test that proves cleanup runs on panic unwind

## Cross-References

- `src/main.rs::TerminalGuard` — the reference implementation
- `.claude/rules/concurrency-model.md` — file lock rules; the
  start lock specifically benefits from Drop-based release
- Rust reference on RAII and Drop semantics
