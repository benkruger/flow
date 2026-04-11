# Scope Expansion for Sweeping Fixes

When an issue cites N violations of a rule and the Plan-phase sweep
discovers significantly more of the same class, decide whether to
expand the PR to cover the full sweep or bound it to the cited
violations.

## The Decision

Expand scope to cover the full sweep when ALL three conditions hold:

1. **The fixes are inert** — text-only changes with no behavioral
   effect (comment rewrites, doc updates, renames that don't break
   the public API, permission list additions). "Inert" means the
   diff cannot introduce runtime bugs.
2. **A single automated guard can prevent regression** — one test,
   scanner, lint rule, or schema check can cover the entire class
   of violation forward. If the guard cannot be written (the rule
   is still evolving, the pattern is too context-sensitive for
   mechanical detection, etc.), scope expansion produces a
   one-shot cleanup that future PRs will silently undo.
3. **Splitting would re-do work** — the sweep already mapped the
   codebase. Splitting into per-file issues forces every future
   session to re-explore the same files to apply the same fix.
   The Plan phase exploration is the expensive part; the rewrite
   itself is cheap.

If any condition fails, bound the PR to the cited violations and
file a follow-up issue capturing the sweep results so a future
session can decide the right shape after the rule is stable.

## Why

Bounding to the issue scope guarantees a follow-up cycle: the next
adversarial test run or the next manual review finds the uncovered
violations, files another issue, and the process repeats. Three to
four iterations of this pattern — each one identical in shape — is
the signal that scope was the wrong dimension to bound on. Expanding
once, landing the guard once, and moving on is strictly cheaper than
running the Plan → Code → Code Review → Learn → Complete cycle
three times on the same underlying class of problem.

The cost of expanding is PR size and review time. The cost of NOT
expanding is session compounding: each follow-up flow costs a full
lifecycle, not just the minutes of the rewrite. When the guard is
cheap and the fixes are inert, expanding is almost always the right
call.

## How to Apply

During the Plan phase, after reading the issue and doing the
codebase sweep:

1. Count the cited violations vs. the sweep total. If the sweep
   finds ≥2x the cited count, scope expansion is a live option.
2. Evaluate the three conditions above. Be honest about condition
   2 in particular — "a scanner would be nice to write" is not the
   same as "a scanner can mechanically detect this class of
   violation with acceptable false-positive rate."
3. If all three hold, recommend the expanded scope in the Risks
   section of the plan and enumerate all affected files in the
   Exploration table. Add a task for the guard (test/scanner/lint)
   and make it depend on all the rewrite tasks — the guard lands
   last, after the rewrites clear the guard's assertion.
4. If any condition fails, bound the plan to the cited violations
   and file a `Tech Debt` issue capturing the sweep inventory.
   Reference the current PR number in the issue body so the future
   session has a pointer to the shape of fix that worked.

## Examples

**Expand**: Issue cites 7 backward-facing comments, sweep finds
~50 more. Comment rewrites are inert, a substring scanner can
enforce the rule forward, splitting would force each file to be
re-read in a later flow. All three conditions hold — expand.

**Don't expand**: Issue cites a race condition in one module,
sweep finds three other modules with similar patterns. The fixes
are behavioral (not inert), no single test can cover all four
modules' execution paths, and each module needs its own
domain-specific analysis. Bound to the cited module.

**Don't expand**: Issue cites a permission-model bug, sweep finds
a broader architectural gap. The bug itself is fixable, but the
broader gap requires a design conversation before any fix. Bound
to the cited bug and file a design issue for the architectural
question.
