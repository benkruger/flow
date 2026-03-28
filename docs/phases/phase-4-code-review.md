---
title: "Phase 4: Code Review"
nav_order: 5
---

# Phase 4: Code Review

**Command:** `/flow-code-review`

Five steps on the same diff — clarity with convention compliance,
correctness with rule compliance, safety, CLAUDE.md compliance, and
pre-mortem incident analysis. Combines inline review passes, a multi-agent
compliance plugin, and a context-isolated pre-mortem agent into a single
phase with five ordered steps, each with its own commit checkpoint.

---

## The Five Steps

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

### Step 4 — Code Review Plugin (CLAUDE.md compliance)

Invokes the `code-review:code-review` plugin for multi-agent validation.
Four parallel agents (2x CLAUDE.md compliance, 1x bug scan, 1x
security/logic scan) with a validation layer that re-validates each finding
at 80+ confidence. Produces high-signal findings only.

Waits for all background agents to complete before evaluating findings.
Every finding is fixed, `bin/flow ci` is run, and changes are committed
via `/flow-commit`.

### Step 5 — Pre-Mortem (incident analysis)

Launches the `pre-mortem` custom agent — a context-isolated sub-agent that
receives only the branch diff and codebase access, with no conversation
history or coding rationale. The agent frames the review as an incident
investigation: "This PR was merged and caused a production incident."

The agent produces a structured incident report (root cause hypothesis,
blast radius, what tests missed, severity, evidence). The main session
triages each finding as real or false positive. Real findings are fixed,
`bin/flow ci` is run, and changes are committed via `/flow-commit`.

---

## Step Advancement

Steps advance via self-invocation rather than inline continuation
directives. After each step completes, the skill invokes itself with
`--continue-step` as its final action. This mirrors the phase-transition
pattern (Phase 1 invoking Phase 2) and prevents context loss that occurs
when the model treats a built-in skill return as a conversation turn
boundary. The Resume Check section dispatches to the correct step on
re-entry.

Step 1 performs inline review passes sequentially within the response turn.
Steps 2-4 invoke built-in skills or plugins that may launch background
agents — each of those steps waits for all background agents to complete
before evaluating findings. Step 5 launches the pre-mortem agent for
context-isolated incident analysis.

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
