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

## Plan Signature Deviations Must Be Logged

The atomic-group rule above covers commit boundaries. The same
discipline extends to **any Code-phase deviation from a plan-level
interface prototype** — a function signature, a type name, a file
name, or a task count. When the Code phase discovers that a plan's
prototype is internally inconsistent (e.g., a test requirement that
cannot be satisfied by the prototype's parameter list) and resolves
the inconsistency by extending the prototype, the deviation must be
recorded in the state log via `bin/flow log` before the commit that
delivers the extended interface lands.

### Why

When the Plan phase produces a prototype like
`run_impl_with_notifier(args, notifier)` but the Code phase delivers
`run_impl_with_deps(root, cwd, args, notifier)` — with additional
parameters that unlock testing surfaces the plan implied but could
not express — the deviation is often a valid and well-considered
design improvement. But if the deviation is not logged anywhere a
future session can find it, the Learn phase audit replays the plan,
sees the rename, and flags it as "plan said X but X is not there"
without context. The Learn analyst then has to infer the justification
from commit messages, which is expensive and sometimes impossible.

The lowest-cost path is for the Code phase to record the deviation
at the moment of discovery:

```bash
bin/flow log <branch> "[Phase 3] Plan signature deviation: run_impl_with_notifier -> run_impl_with_deps (added root/cwd injection to satisfy finalize_with_notifier_cwd_scope_rejects test requirement)"
```

The log entry serves three readers: (1) the immediate Code phase as a
reminder when composing the commit message, (2) Code Review's
reviewer agent when cross-referencing plan vs. implementation, and
(3) the Learn phase audit when distinguishing "plan said X, code has
Y, Learn should investigate" from "plan said X, code has Y, Learn
should move on — this was a documented pivot."

### What Counts as a Deviation

The deviation log is required for:

- **Function or method signature changes** — added/removed parameters,
  changed return types, renamed functions with different intent.
- **Type or struct shape changes** — added fields, different
  serialization layout, new trait implementations.
- **Task count or ordering changes** — the plan named 12 tasks and
  the Code phase delivered 13 (or 11), or the dependency graph was
  restructured mid-flow.
- **File renames or new files** — the plan named `foo.rs` and Code
  delivered `foo_bar.rs` because the new scope justified a split.

It is NOT required for:

- **Typo or spelling fixes** in the plan's prose that Code corrected
  while reading the task description.
- **Whitespace or formatting adjustments** to code the plan sketched
  as pseudocode.
- **Test-function renames** that stay within the plan's stated scope
  (e.g., the plan said `test_foo_success` and Code wrote
  `test_foo_happy_path_success` for clarity).

### How to Apply (Deviation Logging)

When a Code-phase task hits a plan-vs-reality contradiction that
requires extending the plan's prototype:

1. Stop and identify the root cause: is this a plan gap (the plan
   should have anticipated this) or a Code-phase discovery (new
   information from the exploration step)?
2. If the extension is necessary, log the decision immediately:
   `bin/flow log <branch> "[Phase 3] Plan deviation: <what changed>
   (<why>)"`.
3. Include the deviation in the commit message body so reviewers see
   it without consulting the log file.
4. During Learn, the analyst will cross-reference the plan against
   the log file and confirm the deviation was documented. An
   undocumented deviation becomes a Learn-phase process-gap finding.

### Motivating Incident

PR #1157 (Coverage: HTTP client trait seam for `notify_slack` and
`phase_finalize`) planned `run_impl_with_notifier(args, notifier)`
as Task 6's function prototype. Task 5's test list included
`finalize_with_notifier_cwd_scope_rejects`, which requires the
caller to control `cwd` — but the prototype had no `cwd` parameter.
The Code phase discovered the contradiction during implementation
and extended the prototype to
`run_impl_with_deps(root, cwd, args, notifier)`. The extension was
architecturally sound and every inline test passed on the first
compile. But no state log entry recorded the deviation — the only
trace was the commit message, which Learn had to read to confirm
the pivot was intentional. The Learn-analyst audit surfaced this as
a rule-compliance gap because the discipline for logging plan
deviations was not documented in any rule file. This section
codifies the discipline.

### Mechanical Enforcement

Instructional enforcement alone leaves the rule a suggestion — a
Code-phase agent can drift from plan-named fixtures and commit
without logging. The plan-deviation gate inside
`src/finalize_commit.rs::run_impl` converts the instructional
discipline into a mechanical check.

**What it detects.** `src/plan_deviation.rs::scan` walks the plan
file's `## Tasks` section, collects `(test_name, fixture_key,
plan_value)` triples from eligible fenced code blocks (info string
empty or in `rust`/`bash`/`json`/`python`), and cross-references
them against string literals found in the added bodies of
corresponding test functions in `git diff --cached`. A
`Deviation` is emitted when the plan names `test_foo` with
`key = "expected"` but the diff's added `fn test_foo` body does
not contain the literal `"expected"`.

**Where it runs.** The gate fires inside
`finalize_commit::run_impl`, after `ci::run_impl()` succeeds and
before `finalize_commit_inner` calls `git commit`. Every commit
path — `/flow:flow-commit`, direct `bin/flow finalize-commit`
invocations, any future wrapper — routes through `run_impl` and
therefore through the gate. There is no bypass path that lands a
commit without the gate running first. The gate runs during Code
phase rather than Plan phase because it cross-references the plan
against the actual staged implementation — a comparison that
requires implementation code to exist. The scope-enumeration and
external-input-audit gates validate plan prose against itself and
therefore run at Plan phase completion; the plan-deviation gate
validates plan prose against code and therefore runs at commit
time.

**How to acknowledge.** When a deviation is intentional (the Code
phase discovered a better fixture value, or the plan's prototype
was internally inconsistent), the user logs the deviation via
`bin/flow log <branch> "[Phase 3] Plan signature deviation: <text
naming the test and the plan value>"`. The gate re-reads the log
file on every invocation and clears any deviation whose
`(test_name, plan_value)` pair both appear as substrings on a
single log line. After logging, the user re-runs the commit and
the gate passes.

**What is intentionally out of scope.** Tests that the Code phase
adds that the plan never names are invisible to this gate — the
Plan Test Verification check in `skills/flow-code/SKILL.md` owns
that separate invariant. Multi-line string literals are
single-line only in v1. Prefix-renamed tests (plan says
`fn test_foo`, code writes `fn test_foo_happy_path`) are not
matched because exact `fn <name>(` matching is the v1 contract.
Plan prose outside `## Tasks` (Context, Risks, Approach) is not
scanned.

**Structural context.** The scanner follows the peer pattern
established by `src/scope_enumeration.rs` and
`src/external_input_audit.rs` — a pure `scan` function returning
a structured violation vector, a thin `run_impl` orchestrator
that reads files from disk, and an inline `#[cfg(test)] mod tests`
block for unit tests. Integration coverage comes from
`tests/plan_deviation_integration.rs`, which spawns the compiled
`flow-rs` binary against fixture repos to drive the five branches
the gate adds to `finalize_commit::run_impl`.

## Motivating Incident (Atomic Group Split)

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
