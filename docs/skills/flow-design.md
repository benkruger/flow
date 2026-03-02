---
title: /flow:design
nav_order: 5
parent: Skills
---

# /flow:design

**Phase:** 3 — Design

**Usage:** `/flow:design`

Asks what you're building, reads Research findings, proposes 2-3
alternatives with markdown previews, refines the chosen approach, and
gets explicit approval before Plan can begin.

---

## What It Does

1. Reviews Research findings, then asks targeted questions about what to build
2. Proposes 2-3 distinct alternatives with trade-offs in tabbed preview UI
3. Asks targeted follow-up questions on the chosen approach
4. Presents full design for approval
5. Saves all decisions to `state["design"]`

---

## Going Back to Research

Available at two points:
- **Alternatives step** — "Need more research first"
- **Approval gate** — "Go back to Research"

Both update state correctly and invoke `flow:research`.

---

## State Written

```json
"design": {
  "feature_description": "User's own words",
  "chosen_approach": "Approach title",
  "rationale": "Why this approach",
  "schema_changes": [],
  "model_changes": [],
  "controller_changes": [],
  "worker_changes": [],
  "route_changes": [],
  "risks": [],
  "approved_at": "2026-02-20T10:00:00Z"
}
```

---

## Gates

- Requires Phase 2: Research to be complete
- Never writes code — Design only
- Minimum 2 alternatives must be presented before approval
- Requires explicit approval before proceeding to Plan
