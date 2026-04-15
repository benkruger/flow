# No Waivers — 100% Coverage, No Escape Hatch

All Rust code in the FLOW repo must be covered by tests. There is no
waiver mechanism. `test_coverage.md`, `security_waivers.md`, or any
similar per-line exception file is forbidden — neither the file itself
nor the discipline that authorizes one.

## The Rule

When a code path appears unreachable from in-process tests, the
default response is one of:

1. **Add a subprocess test** that spawns the compiled binary
   (`tests/main_dispatch.rs` is the reference) and exercises the
   path through the real CLI surface. cargo-llvm-cov instruments
   subprocess calls when they invoke the same binary, so the lines
   become covered.
2. **Refactor the code** to make it testable from in-process tests.
   The reference pattern is the `run_impl_main(...) -> (Value, i32)`
   seam that returns its result instead of calling
   `process::exit` directly. Tests call the helper, assert on the
   tuple, and the thin wrapper does the print + exit.
3. **Change the design** so the path is not needed. If a function
   has a defensive branch that no production caller can reach,
   delete the branch.

If none of these work, the code is wrong — not the test surface.
Find a different approach.

## Forbidden Plan Prose

A plan is incomplete if any of its prose proposes a waiver entry,
even conditionally. The following prose patterns violate this rule:

- "Add a `test_coverage.md` entry for ..."
- "If any line remains uncoverable ..."
- "Strategy: prefer coverage over waivers" (mentions waivers as
  even a possibility)
- "Waiver candidates: ..."
- "If coverage cannot be achieved ..."
- Any conditional branch in plan prose where the unreachable case
  is "file a waiver"

A plan that includes any of these is not "going to consider waivers
as a last resort" — it is *already proposing waivers*. The plan
phase rejects such plans.

## Why

The waiver path is a slippery slope. Once a plan proposes a waiver
"only as a fallback," the Code phase will exercise the fallback
because some uncovered lines are always inconvenient to reach. The
inconvenient lines accumulate as waivers, the waiver inventory
grows, and the actual test surface shrinks. The cost of the "no
waivers, ever" rule is forcing the harder solution upfront. The
benefit is that every line is exercised and a future refactor can
trust the test suite to catch regressions across the entire surface.

## Enforcement

This rule is the project's gate against waiver drift. It is
enforced at three layers:

1. **Rule prose** (this file). The first instrument is the rule
   itself — every plan author must read this file when designing
   coverage strategy.
2. **Plan-check scanner**. `bin/flow plan-check` should scan plan
   prose for waiver-suggestion phrases and reject plans that
   contain them. (Tracking issue: see `benkruger/flow` Flow label.)
3. **Code Review reviewer agent**. The reviewer agent flags any
   diff that adds a `test_coverage.md` entry as a Real finding to
   be deleted in Step 4.

## How to Apply (Plan Phase)

When designing a plan that touches code:

1. Identify every code path the changes will introduce.
2. For each path, decide how it will be tested. Choose from the
   three default responses above.
3. Do not write "if X is hard to reach, add a waiver" anywhere in
   the plan. If X is hard to reach, decide which of the three
   responses fits and write THAT in the plan.
4. After writing the plan, grep for waiver-suggestion phrases. If
   any appear, rewrite them.

## How to Apply (Code Phase)

When implementing code that has a hard-to-reach branch:

1. Try the three default responses in order. Subprocess test first
   (cheapest), refactor second, design change third.
2. If you find yourself wanting to file a waiver, stop. The waiver
   instinct is a signal that you have not yet found the right test
   surface — it is never the answer.
3. Commit the test or refactor in the same task as the code that
   would otherwise be uncovered.

## How to Apply (Code Review Phase)

When triaging findings:

1. If a finding describes a coverage gap, the only valid fixes are
   subprocess test, refactor, or design change. "Add a waiver" is
   never a valid fix and the finding stays open until one of the
   three responses lands.
2. If the diff adds a `test_coverage.md` entry, route the entry
   for deletion in Step 4 regardless of file location. Per
   `.claude/rules/supersession.md`, the entry is dead code in the
   PR's wake.

## Cross-References

- `.claude/rules/docs-with-behavior.md` — must be updated to remove
  any "Waiver Discipline" prose that authorized `test_coverage.md`
  entries. The two rules are now in conflict; this rule wins.
- `tests/main_dispatch.rs` — reference subprocess test surface for
  CLI dispatch coverage.
- `src/dispatch.rs` and the `run_impl_main` extraction (PR #1156) —
  reference refactor pattern for hoisting `process::exit` out of
  the testable surface.
