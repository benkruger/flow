# Extract-Helper Branch Enumeration

When a Plan-phase task extracts a block of code into a new helper
function, the plan must enumerate the helper's internal branches at
Plan time — before the Code phase runs into them — and commit to a
concrete testing strategy for each branch.

## Scope

This rule applies during the Plan phase. The Branch Enumeration
Table, the Constructor Invariant Audit, and the Topology
Enumeration Table are Plan-phase artifacts the plan author
produces before Code phase begins. They describe what the plan
must contain.

The rule is NOT a Code-phase exit. A Code-phase model
encountering an extraction does the work the plan scopes; citing
this rule mid-Code as permission to defer arbitrarily-sized files
inverts the rule's intent. The Plan-phase enumeration is upstream;
the Code-phase implementation is downstream of an enumeration the
plan has already committed to.

When a Code-phase model finds the plan's enumeration is missing or
wrong (a branch not in the table, a topology not in the table, a
constructor not audited), the correct response is to log a
deviation per `.claude/rules/plan-commit-atomicity.md` "Plan
Signature Deviations Must Be Logged" naming the gap, then proceed
with the work the plan intended. A Plan-phase gap is not a
Code-phase exit — see
`.claude/rules/no-performative-pause.md` "Code-Phase
Scope-Deferral Subcase".

## Vocabulary

- **seam** — a parameterized injection point in a function's
  signature that lets tests substitute a mock for a concrete
  dependency.
- **decider** — a closure or trait object that encapsulates a
  yes/no or branch-selection decision, passed into a function as a
  seam so tests can control the decision.
- **sentinel** — a small cached marker file that records the tree
  state from the most recent successful `bin/flow ci` run.

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
| A | guard expression A holds | Testable directly | `<test_function_name_for_branch_a>` |
| B | guard expression B holds | Testable directly | `<test_function_name_for_branch_b>` |
| C | branch dispatches to an externally-coupled dependency | Testable via seam | (lift the dependency into an injectable parameter and test via a mock) |

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
  by passing a mock implementation.
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

Perpetuating an existing panic across an extraction boundary still
counts as a new public callsite — the audit applies even when the
extraction is not adding a new panic.

## Recursive-Helper Topology Coverage

When the extracted helper recurses over a data structure or graph
— a tree walk, a dependency cascade, a transitive-closure
traversal, a parent-chain walk, any function that calls itself
with a derived input — the Plan-phase Branch Enumeration Table is
not sufficient on its own. Control-flow branches enumerate WHAT
the function does; data-shape topologies enumerate the INPUTS
those branches actually run against. A correctness bug can hide
inside a fully branch-covered helper when an unanticipated
topology exercises a branch's interaction with the recursion's
shared state (a `visited` set, an accumulator, a depth counter,
a closure flag) in a way the branch tests did not.

The plan must include a **Topology Enumeration Table** alongside
the Branch Enumeration Table. The table has three columns:

| Topology | Shape | Test |
|---|---|---|
| linear chain | A → B → C → … (each node has at most one parent and one child) | `<test_function_name_for_linear>` |
| tree | one root, each non-root has exactly one parent (branching out) | `<test_function_name_for_tree>` |
| convergent (diamond) | A node is reachable from the start via two or more disjoint paths (e.g. root → B → D and root → C → D) | `<test_function_name_for_diamond>` |
| cycle | A node's path leads back to itself or to an earlier ancestor | `<test_function_name_for_cycle>` |
| depth-bounded | A chain longer than the helper's defensive depth cap | `<test_function_name_for_depth_cap>` |

The closed set above covers every topology a recursive walk over
a directed graph can encounter. Plans MAY add rows for
domain-specific shapes (e.g. "self-blocking node", "fan-out from
a single ancestor"), but every row in the closed set above is
mandatory — the recursive helper must have a named test for each
topology its production callers can produce.

The diamond row is the topology bug-finders catch most often: a
helper that mutates a shared `visited` set BEFORE evaluating
whether the current node is fully ready (all blockers closed,
all parents visited, all preconditions met) silently marks the
node as "considered" and the second branch that converges on the
same node skips it — even though the first branch's later side
effect would have made the node ready. The fix is the same in
every shape: shared mutation runs AFTER the readiness check, not
before. The diamond test in the topology table is the regression
guard that catches the bug class.

When the recursive helper takes external input (issue numbers,
file paths, branch names, identifiers) and constructs API URLs
or filesystem paths from that input, the Constructor Invariant
Audit above applies to every recursive call's argument — not
just the top-level entry. The recursion can amplify a single
input-validation gap across many API/filesystem touches.

## Enforcement

This rule is prose-only — there is no scanner that mechanically
blocks a Plan phase from completing without a Branch Enumeration
Table. The enforcement layers are:

1. **The rule file itself** — the primary instrument.
2. **The Review reviewer agent** — cross-references the plan's
   Branch Enumeration Table against the landed tests and raises a
   Real finding when a plan-named test is missing.
3. **The adversarial agent in Review** — writes failing tests
   against uncovered branches and uncovered topologies for
   recursive helpers.

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
7. If the extracted helper recurses over a data structure or graph,
   build the Topology Enumeration Table per the Recursive-Helper
   Topology Coverage section. Every topology in the closed set
   (linear, tree, convergent/diamond, cycle, depth-bounded) needs
   a named test.

**Code phase.** Execute the plan tasks in order. For each branch the
plan enumerated, write the named test before or alongside the
implementation. For every panicking constructor in the extracted
block, replace the call with the fallible variant per the
Constructor Invariant Audit. For every topology in the Topology
Enumeration Table, write the named test against the recursive
helper's public surface.

**Review phase.** The reviewer agent cross-references the plan's
Branch Enumeration Table AND Topology Enumeration Table against
the landed tests. Any plan-named test function missing from the
codebase is a Real finding fixed in Step 4.
