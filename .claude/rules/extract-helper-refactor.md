# Extract-Helper Branch Enumeration

When a Plan-phase task extracts a block of code into a new helper
function, the plan must enumerate the helper's internal branches at
Plan time — before the Code phase runs into them — and commit to a
concrete testing strategy for each branch.

This rule is the mechanical complement to the
**Extract-Helper Branch Enumeration** subsection in
`skills/flow-plan/SKILL.md` Step 3. The SKILL.md subsection is the
Plan-phase trigger; this file is the full reference the subsection
cross-references.

## Vocabulary

- **seam** — a parameterized injection point in a function's
  signature that lets tests substitute a mock for a concrete
  dependency. When this rule says "lift X into a seam," it means
  turning `X` into a parameter the caller passes in rather than a
  hard-coded call inside the function body.
- **decider** — a closure or trait object that encapsulates a
  yes/no or branch-selection decision, passed into a function as a
  seam so tests can control the decision.
- **sentinel** — a small cached marker file that records the tree
  state from the most recent successful `bin/flow ci` run. See
  `src/ci.rs::tree_snapshot` and `src/ci.rs::sentinel_path` for
  the canonical readers/writers.
- **CiDecider** — the concrete type alias in `src/complete_fast.rs`
  for the Complete-phase CI dirty-check seam:
  `dyn Fn(&Path, &Path, &str, bool) -> (bool, Option<String>)`.

## Why

A plan that counts tests against the seam a refactor introduces is
not the same as a plan that enumerates the branches of the extracted
helper. The first measures the caller's test surface; the second
measures the helper's. When the two diverge, the Code phase
discovers uncovered branches inside the helper only after the
extraction has landed — and the branch-level test design ends up
negotiated ad hoc instead of by design.

PR #1155 surfaced this. Task 2 extracted the Complete-phase CI
dirty-check block from `run_impl` into a new `production_ci_decider`
helper and added ten tests for the `run_impl_inner` seam that
delegates to the decider. The plan counted those ten tests and
stopped. But `production_ci_decider` itself had four internal
branches (`tree_changed == true`, sentinel hit, CI failure on miss,
CI success on miss) that the plan never named. Three of the four
branches were not reached by the seam tests, and the Code phase had
to decide each branch's test strategy after the extraction was
already written.

The rule force-functions the enumeration conversation at Plan time:
the plan author enumerates the helper's branches before the Code
phase begins, names a concrete test for each one, and refactors
further if any branch cannot fit under one of the three
classifications.

## The Rule

The rule fires when a Plan task description or Approach prose
proposes extracting a block of code into a new helper function,
method, seam, or closure — or any equivalent refactor-for-testability
phrasing. Canonical trigger phrasings:

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
  by passing a mock implementation. Reference pattern:
  `run_impl_inner(args, root, runner, ci_decider)` in
  `src/complete_fast.rs`, where `ci_decider` is a `&CiDecider`
  closure the tests can replace with a mock.
- **Testable directly** — a unit test with a self-contained fixture
  exercises the branch without any mocking or indirection. Typical
  fixtures: a `tempfile::TempDir`, a prepared state-file JSON, or an
  in-memory value. Reference pattern:
  `production_ci_decider_tree_changed_returns_not_skipped` in
  `src/complete_fast.rs` — a unit test that passes `tree_changed =
  true` and asserts the early-return path.
- **Testable via subprocess** — the test spawns the compiled binary
  through `tests/main_dispatch.rs` and exercises the branch through
  the real CLI surface. cargo-llvm-cov instruments subprocess calls
  when they spawn the same binary, so the branch's lines appear in
  the coverage report. Reference pattern: `check_phase_first_phase_exits_0`
  in `tests/main_dispatch.rs`.

If a branch cannot be classified under one of the three, the
extraction design is wrong. The remedy is to refactor further: push
the untested surface behind a seam so the branch becomes Testable
via seam, fold the branch into its caller so it becomes Testable
directly at the caller, or delete the branch entirely if it is
unreachable from any production path. Every branch must land under
one of the three classifications before the plan is complete.

## Enforcement

Iteration 1 of this rule is **prose-only** — there is no scanner
in `src/plan_check.rs` that mechanically blocks a Plan phase from
completing without a Branch Enumeration Table. This is a deliberate
choice per `.claude/rules/skill-authoring.md` "Simplest Approach
First," mirroring `.claude/rules/supersession.md`'s model.

The enforcement layers in iteration 1 are:

1. **The rule file itself** (this file) — the primary instrument.
   Plan authors read it via the cross-reference from
   `skills/flow-plan/SKILL.md` Step 3.
2. **The SKILL.md subsection** — reminds the Plan phase of the
   discipline at authoring time.
3. **The Code Review reviewer agent** — cross-references the plan's
   Branch Enumeration Table against the landed tests and raises a
   Real finding per `.claude/rules/code-review-scope.md` when a
   plan-named test is missing.
4. **The adversarial agent in Code Review** — writes failing tests
   against uncovered branches, surfacing the same gap as test
   failures.

If a future iteration adds a mechanical scanner, the natural home
is `src/extract_helper_refactor.rs` following the topology of
`src/scope_enumeration.rs` and `src/external_input_audit.rs`. That
scanner is **not present today**; any session that goes looking
for one should stop at this section rather than spending turns
searching.

## Opt-Out Grammar

When the plan prose mentions extraction in discussion rather than as
a proposal — for example, when the Risks section references a prior
extraction or when the Approach discusses rejected alternatives that
involved extraction — add the opt-out comment
`<!-- extract-helper-refactor: not-an-extraction -->` on:

- the trigger line itself (same-line, anywhere on the line),
- the line directly above the trigger, or
- two lines above with a single blank line in between.

Larger gaps do not chain — the rule is "the next non-blank line with
at most one blank line separating them," matching the sibling
opt-out grammar in `.claude/rules/scope-enumeration.md` and
`.claude/rules/external-input-audit-gate.md`.

The grammar is documented now as part of the rule's stable API so
that when a scanner is eventually implemented it inherits the
placement rules verbatim. Today the comment is inert.

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
   will exercise the branch. The test function name commits the plan
   author to writing that test in Code phase.
5. If any branch fails the classification step, revise the extraction
   design until every branch fits. Do not ship the plan with an
   unclassified branch.

**Code phase.** Execute the plan tasks in order. For each branch the
plan enumerated, write the named test before or alongside the
implementation per the `.claude/rules/skill-authoring.md` Plan Task
Ordering rule. A Plan Test Verification check at commit time
confirms every plan-named test function exists in the codebase.

**Code Review phase.** The reviewer agent cross-references the plan's
Branch Enumeration Table against the landed tests. Any plan-named
test function missing from the codebase is a Real finding fixed in
Step 4 per `.claude/rules/code-review-scope.md`.

## Motivating Incident

PR #1155 (`Coverage Pattern Completefast`, merge commit `8cb5e80c`)
is the incident that produced this rule. The PR's plan for Task 2
extracted the Complete-phase CI dirty-check block from
`src/complete_fast.rs::run_impl` into a new `production_ci_decider`
helper, and Task 3 added ten behavior-preservation tests exercising
the `run_impl_inner` seam through a mock `ci_decider`. The ten seam
tests proved that `run_impl_inner`'s dispatch branches were correct,
but they did not exercise the four branches inside the real
`production_ci_decider` wrapper.

The four branches were:

1. `tree_changed == true` → early return `(false, None)`
2. `tree_changed == false` ∧ sentinel file exists ∧ snapshot
   matches → `(true, None)` skip
3. `tree_changed == false` ∧ sentinel miss → dispatches to
   `ci::run_impl`, CI fails → `(false, Some(msg))`
4. Same as (3) but CI succeeds → `(false, None)`

Only Branch 1 has a direct unit test today
(`production_ci_decider_tree_changed_returns_not_skipped` at
`src/complete_fast.rs:1409`). Branches 2, 3, and 4 require either a
deeper trait seam around `ci::run_impl` (so the test can inject a
mock CI runner) or subprocess tests that spawn the binary and drive
it through a fixture with a pre-seeded sentinel file.

Had PR #1155's plan enumerated the four branches up front, the
design conversation would have surfaced the need for the deeper
`ci::run_impl` seam before the extraction landed. This rule exists
to force that conversation at Plan time on every future
extract-helper refactor.

Commit references:

- `8cb5e80c` — PR #1155 merge commit
- `59844b30` — Extract `run_impl_inner` seam and add ten
  behavior-preservation tests (Task 2+3 of PR #1155)
- `fcc9a69c` — Record the missing branches of
  `production_ci_decider` (Task 4 of PR #1155, later superseded by
  the full rule framework on main)

## Cross-References

- `skills/flow-plan/SKILL.md` Step 3 — the SKILL.md subsection that
  invokes this rule during Plan phase.
- `.claude/rules/supersession.md` — the structural sibling rule.
  Supersession enumerates code a refactor makes redundant;
  extract-helper enumeration enumerates branches a refactor
  introduces. The two rules run at the same Plan-phase step and
  share the prose-only enforcement model.
- `.claude/rules/skill-authoring.md` — Plan Task Ordering (TDD order)
  and Simplest Approach First (iteration 1 is instructional only, no
  mechanical scanner).
- `src/complete_fast.rs` lines 453–495 — the reference
  `production_ci_decider` helper cited throughout this rule.
- `tests/main_dispatch.rs` — the reference subprocess test surface
  used by the `Testable via subprocess` classification.
