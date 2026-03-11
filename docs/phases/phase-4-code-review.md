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

Invokes Claude Code's built-in `/simplify` on the committed code from the
Code phase. Refactors for clarity: removes unnecessary abstractions, simplifies
conditionals, improves naming. Never changes what the code does, only how.

If `/simplify` proposes changes, they are shown as a diff, committed via
`/flow-commit`, and `bin/flow ci` is run. If no changes are proposed, this
step is skipped.

### Step 2 — Review (correctness)

Invokes Claude Code's built-in `/review` against the PR. Checks plan
alignment, risk coverage, framework anti-patterns, and does a fresh
read-through of every changed file.

Every finding is fixed, `bin/flow ci` is run, and changes are committed
via `/flow-commit`.

### Step 3 — Security (safety)

Invokes Claude Code's built-in `/security-review` against the PR diff.
Scans for vulnerabilities, authentication gaps, data exposure, and
injection risks.

Every finding is fixed, `bin/flow ci` is run, and changes are committed
via `/flow-commit`.

### Step 4 — Code Review Plugin (CLAUDE.md compliance)

Invokes the `code-review:code-review` plugin for multi-agent validation.
Four parallel agents (2x CLAUDE.md compliance, 1x bug scan, 1x
security/logic scan) with a validation layer that re-validates each finding
at 80+ confidence. Produces high-signal findings only.

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
