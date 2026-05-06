# Adversarial Probe Lifecycle

Code Review's adversarial agent writes test functions that prove a
finding by failing against the current implementation. The probe
lives in the worktree's test tree and is removed at Phase 6 Complete
as a side effect of `git worktree remove`. When Code Review Step 4
fixes a finding the probe surfaced, the probe's assertions become
outdated and must be reconciled in the same Code Review pass — a
probe asserting an outdated bug fails CI and blocks the commit.

## The Rule

When Code Review Step 4 applies a fix that resolves a finding the
adversarial probe surfaced, the probe's assertions are no longer
valid (they assert the bug exists). Reconcile in Step 4 by one of:

1. **Delete the probe entirely.** Restore the probe file to its
   integration-branch state (typically a doc-comment-only stub per
   `assets/bin-stubs/test.sh`). The findings the probe surfaced
   are already recorded as state findings via `bin/flow
   add-finding`, and the named regression guards live in
   `tests/<path>/<name>.rs` per `.claude/rules/test-placement.md`,
   not in the throwaway probe.
2. **Update the probe's assertions.** Only when the new behavior
   itself needs a regression guard AND the guard belongs in the
   probe rather than in a properly named test file. This is rare;
   the default response is to delete and rely on the named tests.

The probe must not commit assertions that fail against the current
implementation. The `bin/flow ci` gate at the end of Step 4 fails
otherwise, blocking the commit.

## When the Probe Is Tracked on the Integration Branch

The adversarial probe path is owned by the project (declared via
`bin/test --adversarial-path`). It typically lives at a stable path
(`tests/test_adversarial_flow.rs` for cargo, `test/adversarial_flow_test.rb`
for Rails, etc.) tracked on the integration branch as a
doc-comment-only stub. The Code Review session writes assertions
into the file in the worktree's copy. Restoring the file to the
integration-branch content keeps `git status` clean when no probe
assertions belong on the integration branch:

```bash
git restore --source=origin/<base_branch> <probe_path>
```

## When the Probe Is Untracked

If the probe path is in `.git/info/exclude` (per
`src/prime_check.rs::EXCLUDE_ENTRIES`), the worktree's copy never
becomes a tracked artifact. Worktree removal at Phase 6 Complete
disposes of it. Step 4 still must not leave assertions that fail —
delete the file's contents (or remove the file entirely) when the
findings are recorded elsewhere.

## How to Apply

**Code Review Step 4.** After fixing every Real finding from Step 3,
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
during Code Review, not to live as a long-lived regression guard.
Regression guards belong in `tests/<path>/<name>.rs`.

## Cross-References

- `skills/flow-code-review/SKILL.md` — Step 2 launches the
  adversarial agent; Step 4 fixes findings; this rule covers the
  reconciliation that closes the loop.
- `.claude/rules/test-placement.md` — named regression guards
  live in `tests/<path>/<name>.rs`, not in the throwaway probe.
- `agents/adversarial.md` — agent prompt that produces the probe.
- `assets/bin-stubs/test.sh` — recommended per-language probe
  paths.
- `src/prime_check.rs::EXCLUDE_ENTRIES` — `.git/info/exclude`
  patterns that keep the throwaway probe out of `git status`.
