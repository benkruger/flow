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

Tasks are generated in Rails execution order:

| Section | What it covers |
|---------|---------------|
| Schema | `data/release.sql` changes — tables, columns, indexes |
| Models + Tests | Base/Create split, TDD pairs for each model |
| Workers + Tests | Sidekiq workers, TDD pairs (if needed) |
| Controllers + Routes | Routes, controller actions, TDD pairs |
| Integration Tests | End-to-end test coverage |

Sections with no changes are skipped automatically.

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
state file. On resume, `/flow:resume` picks up at the current section.

---

## What You Get

By the end of Phase 4:

- A complete ordered task list with specific files for every task
- TDD enforced at the planning level
- All tasks stored in `state["plan"]["tasks"]`
- Each task has: section, type, description, files, tdd flag

---

## What Comes Next

Phase 5: Implement (`/flow:implement`) — execute tasks one by one,
TDD enforced at each step.
