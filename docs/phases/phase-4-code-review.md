---
title: "Phase 4: Code Review"
nav_order: 5
---

# Phase 4: Code Review

**Command:** `/flow-code-review`

Six tenants assessed by four cognitively isolated agents launched in
parallel. The parent session gathers context, triages findings, and
fixes. All analysis comes from agents — the parent session never reviews
the diff itself, eliminating the self-reporting bias of inline
self-review.

---

## Six Tenants

Every finding must map to one of these tenants:

1. **Architecture** — does the code follow the project's conventions?
2. **Simplicity** — is there unnecessary complexity?
3. **Maintainability** — can a newcomer understand this?
4. **Correctness** — logic errors, edge cases, security?
5. **Test coverage** — are changes adequately tested?
6. **Documentation** — do docs match the code after these changes?

---

## The Four Steps

### Step 1 — Gather

Collect all artifacts: full branch diff, substantive diff (whitespace
changes filtered via `git diff -w`), plan file, CLAUDE.md,
`.claude/rules/` files, and check whether `bin/flow test` exists for
adversarial testing.

### Step 2 — Launch

Launch four agents in parallel using multiple Agent tool calls in a
single response:

- **Reviewer** (context-rich): receives full diff, plan, CLAUDE.md,
  rules. Covers architecture (T1), simplicity (T2), and correctness
  including security (T4).
- **Pre-mortem** (context-sparse): receives only the substantive diff,
  investigates the codebase independently. Covers correctness failure
  modes including security (T4).
- **Adversarial** (context-sparse): receives the substantive diff and
  writes tests designed to fail. Covers test coverage (T5). Only
  launched if `bin/flow test` exists.
- **Documentation** (context-sparse): receives the substantive diff and
  doc paths, investigates the codebase. Covers maintainability (T3) and
  documentation accuracy (T6).

### Step 3 — Triage

For each finding from all agents, classify as:

- **Real** — fix in Step 4
- **False positive** — dismiss with rationale citing code

There is no filing path. All real findings are fixed during Code
Review — see `.claude/rules/code-review-scope.md`. Mechanical
enforcement blocks filing: `bin/flow add-finding` rejects
`--outcome filed` for `--phase flow-code-review`, and `bin/flow issue`
refuses to create issues while `current_phase == "flow-code-review"`
unless `--override-code-review-ban` is passed.

The supersession test from `.claude/rules/supersession.md` runs
before classification — code the PR has made permanently redundant
is routed to Step 4 for deletion regardless of file location.

### Step 4 — Fix

Fix all real findings, run `bin/flow ci`, commit once.

---

## bin/flow ci Rule

`bin/flow ci` runs after all fixes in Step 4. Code Review does not
transition to Learn until `bin/flow ci` is green.

---

## Back Navigation

- **Go back to Code** — revert to Code phase
- **Go back to Plan** — revert to Plan phase

---

## What Comes Next

Phase 5: Learn (`/flow-learn`) — audit rule compliance and identify
process gaps before the PR is merged.
