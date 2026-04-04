# Start Lock Compliance

## When the Lock Is Held

When `start-lock --acquire` returns `status: locked`, the only
permitted action is to poll via the loop skill. The start-lock
command has built-in staleness detection (30-minute timeout)
that handles genuinely dead sessions.

Never speculate about whether a lock is stale. Never offer to
release, reset, or clean up another flow's lock. Never suggest
any workaround that bypasses the lock. Trust the tool output.

## Error Path Policy

The lock serializes main-branch operations. Error-path behavior
depends on whether the error is flow-specific or main-broken:

- **Flow-specific errors** (Steps 2-3, Step 7: bad config, issue
  fetch failure, duplicate issue, dependency tool failure) —
  release the lock before stopping. Main is untouched. The next
  queued flow would succeed.
- **Main-broken errors** (Steps 6, 8: CI failure on pristine
  main, dep-induced CI breakage after ci-fixer fails) — hold the
  lock and stop. Main is broken or has uncommitted changes. The
  next queued flow would hit the same failure. Releasing cascades
  the problem.
- **Step 10** (happy path) — the only normal release point.

The 30-minute stale timeout is the safety valve for held locks
when the user does not act.
