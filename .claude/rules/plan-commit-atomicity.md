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
multiple commits, the decision must be explicit and defensible.

## The Rule

A plan's "atomic commit group" can be split into multiple commits
only when ALL three conditions hold:

1. **Each commit is independently shippable.** Every intermediate
   commit passes `bin/flow ci` on its own. No commit leaves
   unresolved compile errors, failing tests, or dangling references.
2. **No test assertion spans the boundary.** No test asserts the
   presence of content that another commit in the group removes and
   later re-adds. If such a test exists, the removal and re-addition
   must land in the same commit.
3. **The split clarifies the logical structure.** The split must
   reflect a meaningful boundary (e.g., "core scanner" vs
   "integration" vs "documentation") and not just a context-budget
   or attention-budget convenience.

If any condition fails, honor the plan's atomicity requirement and
land the group in one commit.

## How to Apply

**Plan phase.** When marking a set of tasks as an atomic commit
group, state the WHY explicitly: "atomic because test X asserts
content that task N removes and task M re-adds" or "atomic because
the intermediate state leaves CI failing." Without the WHY, the
Code phase has no basis for deciding whether to honor the atomicity
requirement or split.

**Code phase.** Before splitting a marked atomic group, verify all
three conditions above. If verified, document the split decision in
a state file note via `bin/flow log` so the Learn phase audit can
confirm the reasoning. If any condition fails, honor the plan and
commit atomically.

**Learn phase.** The learn-analyst audit checks whether any marked
atomic group was split across commits. A split without documented
rationale in either a state note or each commit message is a process
gap.

## Optimization-Driven Batching (Non-Atomic Tasks Combined)

The rule above governs SPLITTING marked atomic groups. The inverse
— COMBINING non-atomic tasks into one commit for efficiency — is
also a Code-phase decision that must be logged.

When the plan lists N tasks individually (not marked as an atomic
group) and the Code phase decides to land them in one commit
because:

- Each task's CI measurement would re-run the same test suite
  against the same worktree snapshot, making per-task commits
  wasteful, OR
- The tasks are structurally identical in shape (e.g., "add N
  subprocess tests to the same test file") so splitting into N
  commits produces N near-identical diffs that add no review
  value, OR
- Some other objective efficiency criterion applies

…the batching is permitted, but the decision must be logged at the
moment of batching:

```bash
bin/flow log <branch> "[Phase 3] Batch decision: combining Tasks N-M (<description>) into one commit for <reason>. Each task is independently shippable; no test assertion spans the boundary; the split would <cost>."
```

### When NOT to batch

Combining non-atomic tasks into one commit is NOT permitted when:

- A test added in one task asserts behavior implemented in another
  task — at a minimum, the content-presence rule from the atomic-
  group criterion applies.
- The commit message would need to describe two unrelated concerns
  — that is a sign the tasks belong in separate commits for
  review clarity.
- The plan explicitly lists the tasks as separate with distinct
  commit-message subjects — the plan author already made the call;
  Code should honor it unless the Code-phase discovery genuinely
  supersedes the plan's intent (in which case log the deviation).

### Learn-phase audit

The learn-analyst checks for undocumented batching the same way it
checks for undocumented atomic-group splits. An unlogged batch
becomes a process-gap finding.

## Plan Signature Deviations Must Be Logged

The atomic-group rule above covers commit boundaries. The same
discipline extends to **any Code-phase deviation from a plan-level
interface prototype** — a function signature, a type name, a file
name, or a task count. When the Code phase discovers that a plan's
prototype is internally inconsistent and resolves the inconsistency
by extending the prototype, the deviation must be recorded in the
state log via `bin/flow log` before the commit that delivers the
extended interface lands.

### Why

When the Plan phase produces a prototype like
`run_impl_with_notifier(args, notifier)` but the Code phase delivers
`run_impl_with_deps(root, cwd, args, notifier)` — with additional
parameters that unlock testing surfaces the plan implied but could
not express — the deviation is often a valid design improvement.
But if the deviation is not logged, the Learn phase audit replays
the plan, sees the rename, and flags it as "plan said X but X is
not there" without context.

The lowest-cost path is for the Code phase to record the deviation
at the moment of discovery:

```bash
bin/flow log <branch> "[Phase 3] Plan signature deviation: run_impl_with_notifier -> run_impl_with_deps (added root/cwd injection to satisfy finalize_with_notifier_cwd_scope_rejects test requirement)"
```

The log entry serves three readers: (1) the immediate Code phase as
a reminder when composing the commit message, (2) Code Review's
reviewer agent when cross-referencing plan vs. implementation, and
(3) the Learn phase audit when distinguishing "plan said X, code has
Y, Learn should investigate" from "plan said X, code has Y, this was
a documented pivot."

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
- **Test-function renames** that stay within the plan's stated scope.
- **Implementation-detail changes within an unchanged signature** —
  selecting a different filesystem syscall (e.g., `Path::exists()`
  versus `Path::symlink_metadata().is_err()`) when both produce
  identical fail-open semantics, restructuring `match` arms into
  `.ok().and_then(...)` chains, swapping equivalent standard-library
  primitives, or other refactorings that leave the public signature,
  return type, and observable behavior unchanged. These are routine
  Code-phase implementation choices — not "interface prototype"
  deviations. Code Review may still flag them as architecture or
  simplicity findings, but they do not require a `bin/flow log`
  entry to satisfy this rule. The trigger for logging is a change
  the Plan phase would have written differently if it had known —
  not a change the Plan phase happened to sketch in pseudocode that
  Code phase polished into idiomatic Rust.

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

### Mechanical Enforcement

The plan-deviation gate inside `src/finalize_commit.rs::run_impl`
converts the instructional discipline into a mechanical check.

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
path routes through `run_impl` and therefore through the gate.

**How to acknowledge.** When a deviation is intentional, the user
logs the deviation via `bin/flow log <branch> "[Phase 3] Plan
signature deviation: <text naming the test and the plan value>"`.
The gate re-reads the log file on every invocation and clears any
deviation whose `(test_name, plan_value)` pair both appear as
substrings on a single log line.

**What is intentionally out of scope.** Tests that the Code phase
adds that the plan never names are invisible to this gate — the
Plan Test Verification check in `skills/flow-code/SKILL.md` owns
that separate invariant. Multi-line string literals are
single-line only in v1. Prefix-renamed tests (plan says
`fn test_foo`, code writes `fn test_foo_happy_path`) are not
matched because exact `fn <name>(` matching is the v1 contract.
Plan prose outside `## Tasks` is not scanned.
