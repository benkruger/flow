---
title: "Phase 4: Code Review"
nav_order: 5
---

# Phase 4: Code Review

**Command:** `/flow-code-review`

Four lenses on the same diff — clarity, correctness, safety, and CLAUDE.md
compliance. Combines what were previously separate passes into a single phase
with four ordered steps, each with its own commit checkpoint.

---

## The Four Steps

### Step 1 — Simplify (clarity)

Performs three inline review passes sequentially against the branch diff:
code reuse, code quality, and efficiency. Refactors for clarity: removes
unnecessary abstractions, simplifies conditionals, improves naming. Never
changes what the code does, only how.

If changes are proposed, they are shown as a diff, committed via
`/flow-commit`, and `bin/flow ci` is run. If no changes are proposed, this
step is skipped.

### Step 2 — Review (correctness)

Invokes Claude Code's built-in `/review` against the PR. Waits for all
background agents to complete before evaluating findings. Checks plan
alignment, risk coverage, framework anti-patterns, and does a fresh
read-through of every changed file.

Every finding is fixed, `bin/flow ci` is run, and changes are committed
via `/flow-commit`.

### Step 3 — Security (safety, configurable)

Controlled by the `security_review` config axis. When set to `"never"`,
this step is skipped entirely.

When enabled, invokes Claude Code's built-in `/security-review` against
the PR diff. Waits for all background agents to complete before evaluating
findings. Scans for vulnerabilities, authentication gaps, data exposure,
and injection risks.

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
before evaluating findings.

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
