# Code Task Counter Convention

The `code_task` field in `.flow-states/<branch>/state.json` tracks the
plan task counter during Phase 3 (Code). It is incremented via
`bin/flow set-timestamp --set code_task=<n>` after each task
completes and before the commit.

## The Rule

The counter increments **once per plan task** as defined in the
plan's Tasks section, regardless of how tasks are grouped into
commits. Test+implementation pairs are two tasks; the counter
must increment twice across the pair, even when both tasks land
in the same commit.

## Why

The counter has two readers:

1. **The Code phase resume check** uses `code_task` to find the
   next task to execute on session resume. If the counter under-
   counts (one increment per commit instead of one per task),
   resume picks up at the wrong task — usually skipping forward
   past tasks that need to be redone.
2. **The Learn-phase audit** compares `code_task` to
   `code_tasks_total` to detect plan-vs-execution drift. An
   under-count makes the audit incorrectly flag the PR as
   incomplete and produces a false process-gap finding.

PR #1156 surfaced this exactly. The plan listed 13 tasks and the
PR delivered all 13, but `code_task` only reached 7 because each
test+implementation pair (Tasks 3+4, 5+6, 7+8, 9+10) was treated
as a single increment. The Learn audit had to manually
reconstruct the actual task count from commit messages instead of
trusting the counter.

## How to Apply

When a plan task description names a paired test+implementation
group (TDD pair):

1. Execute the test task: write the failing test, run targeted
   tests to confirm it fails as expected.
2. Increment the counter for the test task:
   `bin/flow set-timestamp --set code_task=<test_task_n>`.
3. Execute the implementation task: write the minimal code to
   make the test pass, run targeted tests to confirm it passes.
4. Increment the counter for the implementation task:
   `bin/flow set-timestamp --set code_task=<impl_task_n>`.
5. Commit the pair via `/flow:flow-commit`. The single commit
   covers both tasks, but the counter has advanced twice.

When a plan marks a set of tasks as an atomic commit group
(per `.claude/rules/plan-commit-atomicity.md`), the same
discipline applies: increment the counter once per task in the
group before the single commit lands. The atomic-group rule
governs commit boundaries; this rule governs the counter.

For atomic groups, batch all counter advances in a single CLI
call using multiple `--set` arguments:

```text
bin/flow set-timestamp --set code_task=4 --set code_task=5 --set code_task=6
```

`apply_updates` processes `--set` arguments sequentially against
mutating in-memory state — each +1 step is validated in order
within the call. This avoids N separate CLI invocations while
preserving the +1 invariant.

## Non-Linear Execution

When a coverage requirement on Task M forces a later test task
(say Task N where N > M) to land in the same commit as Task M,
the counter must STILL advance monotonically — not jump.

Example: Task 2 lands its implementation, but the new code
introduces an `Option::None` branch in `check_X` that only Task
9's edge-case tests exercise. Coverage gate is 100/100/100. To
land Task 2's commit green, Task 9's tests must already exist in
the diff. The Code phase writes Tasks 9's test code in the same
commit as Tasks 1-4.

The counter rule still requires +1 per task. Apply this shape:

1. **Advance only through the contiguously executed prefix.**
   In the example above, advance to `code_task=4` at commit
   time, even though Task 9's tests are also in the diff. The
   counter records "the latest task in the planned sequence
   that has fully landed", not "the highest task whose code is
   present somewhere in the diff."
2. **Log the early-landed task explicitly.** Use
   `bin/flow log <branch> "[Phase 3] Plan deviation: Task 9
   tests landed early in commit 1 (alongside Tasks 1-4) because
   Task 2's coverage requires Task 9's tests to satisfy
   100/100/100. Counter advances to 4 in this commit; Task 9's
   advance to 9 happens when subsequent tasks reach Task 9 in
   the planned sequence."
3. **Catch up the counter when execution reaches the early
   task in the planned sequence.** When the Code phase reaches
   Task 9 in the planned order, run the verification (the test
   already exists, run `bin/flow ci` to confirm green) and
   advance the counter normally — `--set code_task=9`.

Why monotonic-only advances: the resume check uses `code_task`
as "the next task to execute is `code_task + 1` in plan order."
A non-monotonic counter would tell the resume check "go back to
fix Task 5" when the session intended to skip past Task 5
because Task 9's work landed early. Keeping the counter pinned
to the contiguous prefix preserves the resume invariant.

The Learn-phase audit reads the counter together with the log.
A counter at N + a log entry naming "Task M landed early
alongside Task K (K < M)" reads as a documented, intentional
non-linearity — not a process gap.

## Enforcement

`bin/flow set-timestamp --set code_task=<n>` enforces the
"increment by exactly 1" invariant per `--set` argument, not per
CLI invocation. Each `--set code_task=N` in a single call is
validated against the state as mutated by preceding `--set` args
in the same call. A jump (e.g., `--set code_task=5` when current
is 0) is rejected; sequential steps (e.g., `--set code_task=1
--set code_task=2`) succeed.

## Cross-References

- CLAUDE.md "State Mutations" section names the
  increment-by-exactly-1 invariant.
- `.claude/rules/plan-commit-atomicity.md` covers commit-grouping
  rules; this rule covers counter-grouping rules. The two are
  orthogonal: a paired test+implementation group can land in one
  commit (atomic) while the counter advances twice (per task).
- `skills/flow-code/SKILL.md` "Commit" section is where the
  per-task increment instruction lives.
