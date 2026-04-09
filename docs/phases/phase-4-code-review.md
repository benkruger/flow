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

Collect all artifacts: branch diff, plan file, CLAUDE.md, `.claude/rules/`
files, and check whether `bin/flow test` exists for adversarial testing.

### Step 2 — Launch

Launch four agents in parallel using multiple Agent tool calls in a
single response:

- **Reviewer** (context-rich): receives diff, plan, CLAUDE.md, rules.
  Covers architecture (T1), simplicity (T2), and correctness including
  security (T4).
- **Pre-mortem** (context-sparse): receives only the diff, investigates
  the codebase independently. Covers correctness failure modes including
  security (T4).
- **Adversarial** (context-sparse): receives the diff and writes tests
  designed to fail. Covers test coverage (T5). Only launched if
  `bin/flow test` exists.
- **Documentation** (context-sparse): receives the diff and doc paths,
  investigates the codebase. Covers maintainability (T3) and
  documentation accuracy (T6).

### Step 3 — Triage

For each finding from all agents, classify as:

- **Real + in-scope** — fix in Step 4
- **Real + out-of-scope** — file as Tech Debt or Documentation Drift issue
- **False positive** — discard with rationale

### Step 4 — Fix

Fix all real in-scope findings, run `bin/flow ci`, commit once.

---

## Out-of-Scope Findings

Each finding is classified during triage:

- **In-scope** — related to the feature, fixed as normal
- **Tech Debt** — pre-existing, unrelated to the feature. Filed as a "Tech Debt" issue via `bin/flow issue`, recorded via `bin/flow add-issue`, then skipped
- **Documentation Drift** — stale docs unrelated to the feature. Filed as a "Documentation Drift" issue, recorded, then skipped

This keeps reviews focused on the feature while ensuring nothing is lost.

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
