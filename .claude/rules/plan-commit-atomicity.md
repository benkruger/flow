# Plan Commit Atomicity

When a plan's Tasks section marks a set of tasks as an "atomic commit
group" and the Code phase executes those tasks across multiple
commits, the split is acceptable only when every intermediate commit
is independently shippable — each one passes `bin/flow ci`, each one
leaves the tree in a self-consistent state, and no test assertion
depends on the removal and re-addition of content across the
boundary.

## Why

The `.claude/rules/docs-with-behavior.md` "Multi-Task Plans" rule
already requires that a behavior change and its documentation land in
the same commit. A plan's "atomic group" marker extends that
requirement to tasks whose combined output is meaningful but whose
individual outputs would leave the tree in an intermediate state that
CI or reviewers cannot interpret correctly.

When the Code phase later decides to split an atomic group across
multiple commits, the decision must be explicit and defensible — the
plan's atomicity requirement was a design constraint, and relaxing it
silently discards that constraint. PR #1056 documented the split
decision in each commit message as a resilience choice (each commit
independently CI-green), but the rationale was never captured in the
plan or state file.

The risk of an unexplained split: a future session inheriting the
pattern may split a GENUINELY atomic group (where CI failure in the
intermediate state would block merging, or where a test asserts the
presence of content that's removed and re-added) and the split will
fail in ways the original atomic-commit guarantee would have
prevented.

## The Rule

A plan's "atomic commit group" can be split into multiple commits
only when ALL three conditions hold:

1. **Each commit is independently shippable.** Every intermediate
   commit passes `bin/flow ci` on its own. No commit leaves
   unresolved compile errors, failing tests, or dangling references.
2. **No test assertion spans the boundary.** No test asserts the
   presence of content that another commit in the group removes and
   later re-adds. If such a test exists, the removal and re-addition
   must land in the same commit. Check `tests/skill_contracts.rs`
   and any content-presence scanners for assertions that the removal
   would invalidate.
3. **The split clarifies the logical structure.** The split must
   reflect a meaningful boundary (e.g., "core scanner" vs
   "integration" vs "documentation") and not just a context-budget
   or attention-budget convenience.

If any condition fails, honor the plan's atomicity requirement and
land the group in one commit.

## How to Apply

**Plan phase.** When marking a set of tasks as an atomic commit
group, state the WHY explicitly in the plan's Tasks section: "atomic
because test X asserts content that task N removes and task M
re-adds" or "atomic because the intermediate state leaves CI
failing." Without the WHY, the Code phase has no basis for deciding
whether to honor the atomicity requirement or split.

**Code phase.** Before splitting a marked atomic group, verify all
three conditions above. If verified, document the split decision in
a state file note via `bin/flow log` so the Learn phase audit can
confirm the reasoning. If any condition fails, honor the plan and
commit atomically.

**Learn phase.** The learn-analyst audit checks whether any marked
atomic group was split across commits. A split without documented
rationale in either a state note or each commit message is a process
gap — either the Code phase skipped the verification check, or the
plan's atomicity was not actually load-bearing and the marker was
misused. Either way, the finding reaches this file.

## Motivating Incident

PR #1056 (Plan-phase external-input audit gate) planned Task 21 as
"Commit atomic group: Tasks 3, 5, 6, 9, 11, 13, 14, 15, 16, 17, 18,
19 land in one commit." The Code phase split the work across five
commits (25542b67, 9f69924b, 89cfee33, d96f3fb5, 4ed5dcfc, 2dc21114)
for context-management resilience. Each commit independently passed
CI and each was individually shippable, so the split was safe —
Condition 1 held. No test assertion spanned the boundary —
Condition 2 held. Each commit grouped a logical unit (scanner + unit
tests; corpus contract test + cleanup; plan_check integration;
plan_extract integration; rule file + SKILL.md + CLAUDE.md + docs) —
Condition 3 held. All three conditions were met, so the split was
acceptable. But the decision was implicit — no state note documented
the verification. The Learn audit surfaced this as a process gap and
this rule codifies the explicit check.
