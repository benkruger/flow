---
title: "Phase 4: Plan"
nav_order: 5
---

# Phase 4: Plan

**Command:** `/flow:plan`

Plan takes the approved design and breaks it into ordered, executable
tasks — section by section, with individual approval at each step.
TDD order is built in: tests always come before implementations.

---

## Sections

Tasks are generated in execution order defined by the framework fragment. Each framework defines its own sections (e.g., Rails uses schema/models/workers/controllers/integration; Python uses modules/scripts/tests). Sections with no changes are skipped automatically.

---

## Navigation

**Within Plan — go back to any previous section:**
At every section you can go back to re-open a previous one.
Going back invalidates all sections after it — they'll need
re-approval since earlier decisions affect later ones.

**From the final review:**

- Go back to a plan section
- Go back to Design
- Go back to Research

---

## Resuming Mid-Plan

If you close Claude mid-plan, the approved sections are saved in the
state file. On resume, `/flow:continue` picks up at the current section.

---

## What You Get

By the end of Phase 4:

- A complete ordered task list with specific files for every task
- TDD enforced at the planning level
- All tasks stored in `state["plan"]["tasks"]`
- Each task has: section, type, description, files, tdd flag

---

## What Comes Next

Phase 5: Code (`/flow:code`) — execute tasks one by one,
TDD enforced at each step.
