---
title: "Phase 3: Design"
nav_order: 4
---

# Phase 3: Design

**Command:** `/flow:design`

Design answers *what are we building and how?* Research told us what
exists. Design decides what changes — and always presents 2-3 real
alternatives before committing to one. No code is written until the
design is approved.

---

## Steps

### 1. What are we building?
User describes the feature in detail before Claude proposes anything.

### 2. Read research findings
Design is informed by what Research discovered — affected files, risks,
callbacks, schema.

### 3. Propose 2-3 alternatives
Each covers schema approach, model/controller/worker structure, and
trade-offs. Presented with markdown previews via the tabbed UI.

### 4. Refine chosen approach
Targeted follow-up questions on the selected alternative only.

### 5. Present full design for approval
Schema changes, model decisions, worker decisions, route decisions,
risks. Explicit approval required before proceeding.

### 6. Save to state file
All design decisions stored in `state["design"]` — no external files.

---

## Going Back to Research

At two points in Design you can return to Research:
- When viewing alternatives: "Need more research first"
- At the approval gate: "Go back to Research"

Both re-open Research with the existing findings intact so you extend
rather than restart.

---

## What You Get

By the end of Phase 3:

- A clearly described feature with explicit approval
- Chosen approach documented with rationale
- Schema, model, controller, worker, and route decisions captured
- All stored in `.flow-states/<branch>.json`
- A foundation Plan can execute directly from

---

## What Comes Next

Phase 4: Plan (`/flow:plan`) — break the approved design into ordered,
time-bounded tasks.
