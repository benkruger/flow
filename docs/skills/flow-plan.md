---
title: /flow:plan
nav_order: 6
parent: Skills
---

# /flow:plan

**Phase:** 4 — Plan

**Usage:** `/flow:plan`

Breaks the approved design into ordered, executable tasks section by
section. Each section is approved individually. TDD order is enforced
throughout — tests always come before implementations.

---

## What It Does

1. Reads `state["design"]` for all design decisions
2. Generates tasks section by section (schema, models, workers, controllers, integration)
3. Presents each section for approval with back navigation
4. Shows complete task list for final sign-off
5. Saves to `state["plan"]` with per-task and per-section tracking

---

## Section Navigation

At every section:
- **Yes** — mark approved, move to next
- **Needs changes** — revise and re-present
- **Go back to previous section** — re-opens it, invalidates later sections
- **Go back further** — picker of all approved sections

---

## Task Structure

```json
{
  "id": 1,
  "section": "schema",
  "type": "schema",
  "description": "Add payments table to data/release.sql",
  "files": ["data/release.sql"],
  "tdd": false,
  "status": "pending"
}
```

---

## Back Navigation

- Within Plan: go back to any previous section
- From final review: go back to Design or Research

---

## Gates

- Requires Phase 3: Design to be complete
- TDD order enforced — test task always precedes implementation task
- Never writes code — task descriptions only
