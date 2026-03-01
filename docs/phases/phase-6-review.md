---
title: "Phase 6: Review"
nav_order: 7
---

# Phase 6: Review

**Command:** `/flow:review`

Systematic code review against the approved design, research risks,
and framework anti-patterns. bin/ci was already green from Code — Review
adds what automated tools cannot catch.

---

## What Review Checks

**1. Design alignment**
Does the implementation match `state["design"]`? All change categories
verified against the approved design.

**2. Research risk coverage**
Every risk in `state["research"]["risks"]` confirmed as handled.
A risk found and not addressed is a bug waiting to happen.

**3. Framework anti-patterns**
Things bin/ci cannot catch — defined by the framework fragment. Each framework has its own anti-pattern checklist (e.g., N+1 queries and callback misuse for Rails; circular imports and mutable defaults for Python).

**4. Fresh read-through**
Every changed file read as if seeing it for the first time.
Clarity, naming, no over-engineering.

---

## Findings

- **Minor** — fixed directly in Review, committed, bin/ci re-run
- **Significant** — AskUserQuestion: fix here, go back to Code, Plan, or Design

---

## bin/ci Rule

bin/ci runs after every fix made during Review.
Review does not transition to Reflect until bin/ci is green.

---

## What Comes Next

Phase 7: Security (`/flow:security`) — scan for security issues in the
feature diff before the PR is merged.
