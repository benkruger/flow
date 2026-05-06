# Extract-Helper Branch Enumeration

When a Plan-phase task extracts a block of code into a new helper
function, the plan must enumerate the helper's internal branches at
Plan time — before the Code phase runs into them — and commit to a
concrete testing strategy for each branch.

This rule is the mechanical complement to the
**Extract-Helper Branch Enumeration** subsection in
`skills/flow-plan/SKILL.md` Step 3.

## Vocabulary

- **seam** — a parameterized injection point in a function's
  signature that lets tests substitute a mock for a concrete
  dependency.
- **decider** — a closure or trait object that encapsulates a
  yes/no or branch-selection decision, passed into a function as a
  seam so tests can control the decision.
- **sentinel** — a small cached marker file that records the tree
  state from the most recent successful `bin/flow ci` run.
- **CiDecider** — the concrete type alias in `src/complete_fast.rs`
  for the Complete-phase CI dirty-check seam:
  `dyn Fn(&Path, &Path, &str, bool) -> (bool, Option<String>)`.

## Why

A plan that counts tests against the seam a refactor introduces is
not the same as a plan that enumerates the branches of the extracted
helper. The first measures the caller's test surface; the second
measures the helper's. When the two diverge, the Code phase
discovers uncovered branches inside the helper only after the
extraction has landed.

The rule force-functions the enumeration conversation at Plan time:
enumerate the helper's branches before the Code phase begins, name
a concrete test for each one, and refactor further if any branch
cannot fit under one of the three classifications.

## The Rule

The rule fires when a Plan task description or Approach prose
proposes extracting a block of code into a new helper function,
method, seam, or closure. Canonical trigger phrasings:

- "extract *X* into a new *Y*"
- "lift the *X* block into *Y*"
- "hoist *X* out of *Y*"
- "factor out *X* into a helper"
- "pull out *X* into a seam"
- "refactor *X* into an inner function"
- "introduce a trait seam for *X*"

When a trigger phrasing appears, the plan's Exploration or Approach
section must include a **Branch Enumeration Table** within a few
lines of the trigger. The table has four columns:

| Branch | Condition | Classification | Test |
|---|---|---|---|
| A | `tree_changed == true` | Testable directly | `production_ci_decider_tree_changed_returns_not_skipped` |
| B | `tree_changed == false` ∧ sentinel matches | Testable directly | `production_ci_decider_sentinel_hit_returns_skipped` |
| C | CI dirty-check dispatches to `ci::run_impl` | Testable via seam | (lift `ci::run_impl` into an injectable parameter and test via a mock) |

Column definitions:

- **Branch** — a letter or number label identifying the branch
- **Condition** — the guard expression or prose condition
- **Classification** — one of the three values in the next section
- **Test** — the named test function that will exercise this branch,
  or (when the classification is reached via further refactoring) a
  concrete description of the sub-refactor and the test it unlocks

## The Three Classifications

- **Testable via seam** — the caller injects a closure, trait
  object, or `Command` via a parameter, and the branch is exercised
  by passing a mock implementation. Reference:
  `run_impl_inner(args, root, runner, ci_decider)` in
  `src/complete_fast.rs`.
- **Testable directly** — a unit test with a self-contained fixture
  exercises the branch without any mocking or indirection. Typical
  fixtures: a `tempfile::TempDir`, a prepared state-file JSON, or an
  in-memory value.
- **Testable via subprocess** — the test spawns the compiled binary
  through `tests/main_dispatch.rs` and exercises the branch through
  the real CLI surface.

If a branch cannot be classified under one of the three, the
extraction design is wrong. Refactor further: push the untested
surface behind a seam, fold the branch into its caller, or delete
the branch entirely if it is unreachable from any production path.
Every branch must land under one of the three classifications
before the plan is complete.

## Constructor Invariant Audit

When the extracted block contains a constructor call that panics on
invalid input — `FlowPaths::new`, any function with a `panic!`,
`assert!`, or `unwrap` on a parameter — the extraction surfaces an
input-validation contract that may have been silently held by the
prior call site. The Plan phase must audit that contract before the
extraction lands.

The audit answers two questions per panicking constructor:

1. **What input does the constructor panic on?** Read the
   constructor's invariant assertion. For `FlowPaths::new` the
   answer is "empty branch" or "branch containing `/`".
2. **Where does the input enter the new function?** If the input
   is sourced from a CLI flag (`--branch`), state-file value, git
   subprocess output, env var, or any other external source, the
   extraction is also a callsite of the panicking constructor under
   the discipline named in `.claude/rules/external-input-validation.md`.
   The new public surface MUST use the fallible variant
   (`FlowPaths::try_new`, `Option`-returning, `Result`-returning)
   and translate the invalid case into a structured error.

The audit produces a row in the plan's
`.claude/rules/external-input-audit-gate.md` callsite table even
when the extraction is not adding a new panic — perpetuating an
existing one across an extraction boundary still counts as a
new public callsite.

## Enforcement

This rule is prose-only — there is no scanner that mechanically
blocks a Plan phase from completing without a Branch Enumeration
Table. The enforcement layers are:

1. **The rule file itself** — the primary instrument.
2. **The SKILL.md subsection** — reminds the Plan phase of the
   discipline at authoring time.
3. **The Code Review reviewer agent** — cross-references the plan's
   Branch Enumeration Table against the landed tests and raises a
   Real finding when a plan-named test is missing.
4. **The adversarial agent in Code Review** — writes failing tests
   against uncovered branches.

## Opt-Out Grammar

When the plan prose mentions extraction in discussion rather than as
a proposal, add the opt-out comment
`<!-- extract-helper-refactor: not-an-extraction -->` on:

- the trigger line itself (same-line, anywhere on the line),
- the line directly above the trigger, or
- two lines above with a single blank line in between.

Larger gaps do not chain.

## How to Apply

**Plan phase.** After writing the plan's task list, scan every task
and every Approach paragraph for the trigger phrasings listed in
**The Rule**. For each trigger:

1. Identify the function the plan will extract into. Read the source
   block the plan will move.
2. Enumerate the branches inside that block. Each `if`, `match` arm,
   early return, or conditional expression is a candidate branch.
3. For every branch, classify it under one of the three labels.
4. For every classification, name the concrete test function that
   will exercise the branch.
5. If any branch fails the classification step, revise the extraction
   design until every branch fits.
6. Apply the Constructor Invariant Audit to every panicking
   constructor call inside the extracted block.

**Code phase.** Execute the plan tasks in order. For each branch the
plan enumerated, write the named test before or alongside the
implementation. For every panicking constructor in the extracted
block, replace the call with the fallible variant per the
Constructor Invariant Audit.

**Code Review phase.** The reviewer agent cross-references the plan's
Branch Enumeration Table against the landed tests. Any plan-named
test function missing from the codebase is a Real finding fixed in
Step 4.
