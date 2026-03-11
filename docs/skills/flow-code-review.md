---
title: /flow-code-review
nav_order: 8
parent: Skills
---

# /flow-code-review

**Phase:** 4 — Code Review

**Usage:** `/flow-code-review`, `/flow-code-review --auto`, or `/flow-code-review --manual`

Four lenses on the same diff — clarity, correctness, safety, and CLAUDE.md
compliance. Combines simplification, code review, security review, and
multi-agent plugin validation into a single phase with four ordered steps,
each with its own commit checkpoint.

---

## Steps

### Step 1 — Simplify (clarity)

Invokes Claude Code's built-in `/simplify`. If changes are proposed, shows
the diff, commits via `/flow-commit`, and runs `bin/flow ci`. If no changes,
skips to Step 2.

### Step 2 — Review (correctness)

Invokes Claude Code's built-in `/review` against the PR. Checks plan
alignment, risk coverage, and framework anti-patterns. If no findings,
skips to the next step. Every finding is fixed, `bin/flow ci` is run,
and changes are committed via `/flow-commit`.

### Step 3 — Security (safety)

Invokes Claude Code's built-in `/security-review` against the PR diff.
If no findings, skips to the next step. Every finding is fixed,
`bin/flow ci` is run, and changes are committed via `/flow-commit`.

### Step 4 — Code Review Plugin (CLAUDE.md compliance)

Invokes the `code-review:code-review` plugin for multi-agent validation.
Four parallel agents (2x CLAUDE.md compliance, 1x bug scan, 1x
security/logic scan) with a validation layer that filters false positives.
If no findings, skips to Done. Every finding is fixed, `bin/flow ci` is
run, and changes are committed via `/flow-commit`.

---

## Mode

Mode is configurable via `.flow.json` (default: manual). Both commit and
continue are configurable independently. In auto mode, findings are
auto-fixed and the phase transition advances to Learn without asking.

---

## Step Advancement

Steps advance via self-invocation: after each step completes, the skill
invokes itself with `--continue-step` as its final action. This prevents
context loss that occurs when the model treats a built-in skill return as
a conversation turn boundary. The `--continue-step` flag skips the
Announce banner and phase entry update, proceeding directly to the Resume
Check which dispatches to the next step.

---

## Gates

- Code phase must be complete before Code Review can start
- `bin/flow ci` must be green after every fix in every step
- `bin/flow ci` must be green before transitioning to Learn
- Can return to Code or Plan
