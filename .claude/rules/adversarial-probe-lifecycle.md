# Adversarial Probe Lifecycle

Review's adversarial agent writes test functions that prove a
finding by failing against the current implementation. The probe
lives at the path declared by `bin/test --adversarial-path`. The
path is **never tracked by git on the integration branch** — it is
listed in `src/prime_check.rs::EXCLUDE_ENTRIES` so `git add -A`
never stages it, and
`tests/structural.rs::adversarial_probe_must_not_be_tracked`
asserts the invariant against `git ls-files`. The probe is
disposed of at Phase 4 Complete by
`src/cleanup.rs::delete_adversarial_probe` via `fs::remove_file`,
before `git worktree remove` runs.

When Review Step 4 fixes a finding the probe surfaced, the
probe's assertions become outdated and must be reconciled in the
same Review pass — a probe asserting an outdated bug fails
CI and blocks the commit. The same reconciliation applies when
Step 3 triage dismisses the finding (out-of-scope, pre-existing,
etc.); a dismissed finding still leaves a failing assertion behind
and the same CI gate fires.

## The Rule

When Review Step 4 applies a fix that resolves a finding the
adversarial probe surfaced, the probe's assertions are no longer
valid (they assert the bug exists). Reconcile in Step 4 by one of:

1. **Delete the probe file entirely.** Remove the file from the
   worktree (`fs::remove_file` from inside Rust during the
   cleanup phase, or a worktree-internal removal during Code
   Review Step 4 by overwriting the file with empty contents or
   removing it). The findings the probe surfaced are already
   recorded as state findings via `bin/flow add-finding`, and
   the named regression guards live in `tests/<path>/<name>.rs`
   per `.claude/rules/test-placement.md`, not in the throwaway
   probe. The path must not be left in a tracked state — see
   "Untracked-Path Invariant" below.
2. **Update the probe's assertions.** Only when the new behavior
   itself needs a regression guard AND the guard belongs in the
   probe rather than in a properly named test file. This is rare;
   the default response is to delete and rely on the named tests.

The probe must not commit assertions that fail against the current
implementation. The `bin/flow ci` gate at the end of Step 4 fails
otherwise, blocking the commit.

## Untracked-Path Invariant

The probe path is never tracked by git on the integration branch.
The mechanism has two layers:

- `src/prime_check.rs::EXCLUDE_ENTRIES` lists
  `test_adversarial_flow.*` (and language-equivalent patterns) in
  `.git/info/exclude`, so `git add -A` does not stage the file
  while the path is untracked.
- `tests/structural.rs::adversarial_probe_must_not_be_tracked`
  asserts `git ls-files tests/test_adversarial_flow.rs` is empty,
  catching any commit that re-introduces a tracked stub or
  commits a probe.

A tracked stub is forbidden. The exclude pattern only protects
files that are already untracked; once the path is committed, the
exclude pattern stops applying and every subsequent `git add -A`
stages modifications. The cleanup step in
`src/cleanup.rs::delete_adversarial_probe` removes the probe file
from the worktree but does not touch git's tracked state — a
tracked stub would survive cleanup and persist on the integration
branch indefinitely. The structural test is the gate that catches
that class of regression.

## How to Apply

**Review Step 4.** After fixing every Real finding from Step 3,
audit the adversarial probe file. For each test in the probe:

1. Re-run the test against the fixed implementation
   (`bin/flow ci --test --file <probe_path>`).
2. If the test passes, the assertion still holds — leave it (or
   migrate it to a properly named test file per
   `.claude/rules/test-placement.md`).
3. If the test fails because Step 4's fix changed the behavior the
   probe was asserting, delete the test from the probe file. The
   finding the probe surfaced is recorded via `bin/flow
   add-finding`; deleting the probe test does not lose information.
4. After auditing every test in the probe, run `bin/flow ci` once
   more to confirm the probe-free state passes the coverage gate.

The default is delete. The probe's purpose is to surface findings
during Review, not to live as a long-lived regression guard.
Regression guards belong in `tests/<path>/<name>.rs`.
