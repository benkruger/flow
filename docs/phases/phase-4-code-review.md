---
title: "Phase 4: Code Review"
nav_order: 5
---

# Phase 4: Code Review

**Command:** `/flow-code-review`

Four steps on the same diff — clarity with convention compliance,
correctness with rule compliance, safety, and parallel agent reviews
(context-isolated code review, pre-mortem incident analysis, adversarial
test generation launched concurrently). Combines inline review passes and
three context-isolated agents into a single phase with four ordered steps,
each with its own commit checkpoint.

---

## The Four Steps

### Step 1 — Simplify (clarity + convention compliance)

Performs four inline review passes sequentially against the branch diff:
code reuse, code quality, efficiency, and convention compliance. Refactors for clarity: removes
unnecessary abstractions, simplifies conditionals, improves naming. Never
changes what the code does, only how.

If changes are proposed, they are shown as a diff, committed via
`/flow-commit`, and `bin/flow ci` is run. If no changes are proposed, this
step is skipped.

### Step 2 — Review (correctness)

Performs an inline correctness review of the branch diff using five review
passes: plan alignment, logic correctness, test coverage, API contracts,
and rule compliance. Uses the plan file as context for
implementation-vs-intent alignment.

Every finding is fixed, `bin/flow ci` is run, and changes are committed
via `/flow-commit`.

### Step 3 — Security (safety)

Performs an inline security review of the branch diff using three security
lenses: input validation, authentication and authorization, and data
exposure.

Every finding is fixed, `bin/flow ci` is run, and changes are committed
via `/flow-commit`.

### Step 4 — Agent Reviews (parallel launch)

Launches three independent sub-agents in parallel — reviewer, pre-mortem,
and adversarial — using multiple Agent tool calls in a single response.
After all agents return, findings are triaged and fixed sequentially.

The **reviewer** agent is context-rich: it receives the branch diff, plan
file, CLAUDE.md, and `.claude/rules/` inline. The **pre-mortem** agent is
context-sparse: it receives only the branch diff and investigates the
codebase independently. The **adversarial** agent is also context-sparse:
it receives the diff, a branch-scoped temp test file path, and the CLAUDE.md
path for test conventions.

The main session triages each finding as real or false positive. Real
findings are fixed, `bin/flow ci` is run, and changes are committed via
`/flow-commit`.

---

## Step Advancement

Steps advance via self-invocation rather than inline continuation
directives. After each step completes, the skill invokes itself with
`--continue-step` as its final action. This mirrors the phase-transition
pattern (Phase 1 invoking Phase 2) and prevents context loss that occurs
when the model treats a built-in skill return as a conversation turn
boundary. The Resume Check section dispatches to the correct step on
re-entry.

Steps 1-3 perform inline review passes sequentially within the response
turn. Step 4 launches all three agents (reviewer, pre-mortem, adversarial)
in parallel, then triages and fixes findings after all return.

---

## Out-of-Scope Findings

Each finding is classified before fixing:

- **In-scope** — related to the feature, fixed as normal
- **Tech Debt** — pre-existing, unrelated to the feature. Filed as a "Tech Debt" issue via `bin/flow issue`, recorded via `bin/flow add-issue`, then skipped
- **Documentation Drift** — stale docs unrelated to the feature. Filed as a "Documentation Drift" issue, recorded, then skipped

This keeps reviews focused on the feature while ensuring nothing is lost.

---

## bin/flow ci Rule

`bin/flow ci` runs after every fix in every step. Code Review does not
transition to Learn until `bin/flow ci` is green.

---

## Back Navigation

- **Go back to Code** — revert to Code phase
- **Go back to Plan** — revert to Plan phase

---

## What Comes Next

Phase 5: Learn (`/flow-learn`) — extract learnings and update
CLAUDE.md before the PR is merged.
