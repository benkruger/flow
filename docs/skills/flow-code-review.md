---
title: /flow-code-review
nav_order: 8
parent: Skills
---

# /flow-code-review

**Phase:** 4 — Code Review

**Usage:** `/flow-code-review`, `/flow-code-review --auto`, or `/flow-code-review --manual`

Three lenses on the same diff — clarity, correctness, and safety. Combines
simplification, code review, and security review into a single phase with
three ordered steps, each with its own commit checkpoint.

---

## Steps

### Step 1 — Simplify (clarity)

Invokes Claude Code's built-in `/simplify`. If changes are proposed, shows
the diff, commits via `/flow-commit`, and runs `bin/flow ci`. If no changes,
skips to Step 2.

### Step 2 — Review (correctness)

Invokes Claude Code's built-in `/review` against the PR. Checks plan
alignment, risk coverage, and framework anti-patterns. Every finding is
fixed, `bin/flow ci` is run, and changes are committed via `/flow-commit`.

### Step 3 — Security (safety)

Invokes Claude Code's built-in `/security-review` against the PR diff.
Every finding is fixed, `bin/flow ci` is run, and changes are committed
via `/flow-commit`.

---

## Mode

Mode is configurable via `.flow.json` (default: manual). Both commit and
continue are configurable independently. In auto mode, findings are
auto-fixed and the phase transition advances to Learning without asking.

---

## Gates

- Code phase must be complete before Code Review can start
- `bin/flow ci` must be green after every fix in every step
- `bin/flow ci` must be green before transitioning to Learning
- Can return to Code or Plan
