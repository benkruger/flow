# Verify the Runtime Path

Before writing a fix, trace the actual execution path to confirm
where the code runs.

## Required Steps

<!-- scope-enumeration: imperative -->
1. **Find the real call site.** Grep for all callers. A function
   may exist in one file but be called from another — or not called
   at all if a different code path runs first.
2. **Verify runtime behavior.** Write a small diagnostic script
   that runs through the same call chain (Claude Code → bash →
   bin/flow → flow-rs) and print the actual values. Unit test
   mocks do not catch environment issues like missing ttys,
   wrong parents, or piped stdin.
3. **Check one layer deeper.** When a subprocess returns an
   unexpected value (`??`, empty string, wrong PID), investigate
   why before filtering it out. The wrong value is a symptom.

## Plan-phase extension for new production paths

The rule above applies to fixes, but it must also apply in the Plan
phase whenever a plan introduces a **new** execution path that a
production caller will take. Adding a new branch (a new `if` arm,
a new `match` arm, a new early-return guard) inside a function with
a live production caller creates a path that never ran before —
and therefore has no coverage and no proof it behaves as intended.

When the plan modifies a function, the plan must enumerate:

1. Every caller of the function, with the conditions under which
   each caller hits the new code path.
2. The test that exercises each new path, using inputs that drive
   the specific caller's conditions (not a contrived unit-test
   fixture).

PR #1054 surfaced this omission: `find_state_files` gained an
empty-branch code path to support the `format-status` multi-flow
fallback (`src/main.rs:910`) and the stop-continue hook
(`src/hooks/stop_continue.rs:270`). The plan migrated the callers
but did not enumerate the new path's tests — Code Review caught the
gap and added `find_state_files_empty_branch_scans_directory` and
two slash-branch regression tests covering the production callers.
A Plan-phase callsite audit would have included these tasks from
the start.

## Anti-Patterns

- Committing a fix without running it through the real path
- Adding a second fix on top of an unverified first fix
- Trusting unit tests as proof that runtime behavior is correct
  when the bug is environmental (process tree, tty, file system)
- Assuming which file creates/owns a piece of state without
  grepping for all writers
- Adding a new branch to a function without listing the production
  callers that will take it and the tests that prove each caller's
  path
