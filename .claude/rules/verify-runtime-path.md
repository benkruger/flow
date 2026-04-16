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
production caller will take. Two shapes of this class:

**Shape A — new branch inside an existing function.** Adding a new
branch (a new `if` arm, a new `match` arm, a new early-return guard)
inside a function with a live production caller creates a path that
never ran before — and therefore has no coverage and no proof it
behaves as intended.

**Shape B — new callsite family introduced by a feature.** Adding a
new function, scanner, helper, or module and wiring it into multiple
existing entry points (plan_check, plan_extract extracted path,
plan_extract resume path; hook family; CI runner family) creates
multiple production paths in the same PR. Each invocation site is
its own callsite audit row — the plan must list every invocation
point and name the test that exercises it, not rely on a single
"integration test" that happens to hit one of them.

When the plan modifies a function OR introduces a new callsite
family, the plan must enumerate the callers and, for every row:

<!-- scope-enumeration: imperative -->
1. Record the conditions under which the caller hits the new code
   path.
2. Name the test that exercises the new path, using inputs that
   drive the caller's conditions (not a contrived unit-test
   fixture).
3. For callsite families, list every invocation point separately —
   do not collapse "plan_check + plan_extract extracted +
   plan_extract resume" into "the three callsites" without
   enumerating each one by name and each one's test.

PR #1054 surfaced the Shape A case: `find_state_files` gained an
empty-branch code path to support the `format-status` multi-flow
fallback (`src/main.rs:910`) and the stop-continue hook
(`src/hooks/stop_continue.rs:270`). The plan migrated the callers
but did not enumerate the new path's tests — Code Review caught the
gap and added `find_state_files_empty_branch_scans_directory` and
two slash-branch regression tests covering the production callers.
A Plan-phase callsite audit would have included these tasks from
the start.

PR #1177 surfaced the Shape B case: the `duplicate_test_coverage`
scanner was wired into three callsites (plan_check::run_impl,
plan_extract extracted, plan_extract resume). The plan described
the scanner and the integration topology but did not enumerate
each callsite as a distinct test row — the Code Review reviewer
agent noted the absence during triage. A plan that introduces a
new scanner-family feature must name each integration-test
function for each callsite before the Code phase begins.

## Anti-Patterns

<!-- scope-enumeration: imperative -->
- Committing a fix without running it through the real path
- Adding a second fix on top of an unverified first fix
- Trusting unit tests as proof that runtime behavior is correct
  when the bug is environmental (process tree, tty, file system)
- Assuming which file creates/owns a piece of state without
  grepping for all writers
- Adding a new branch to a function without listing the production
  callers that will take it and the tests that prove the new paths
- Introducing a new scanner, hook, or helper family and describing
  the integration as "the three callsites" without enumerating
  each callsite and its named test
