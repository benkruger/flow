---
title: "Phase 5: Code"
nav_order: 6
---

# Phase 5: Code

**Command:** `/flow:code`

Execute the approved plan task by task. Every task follows the same
cycle: architecture check, TDD, diff review, bin/ci, commit. Nothing
moves forward without the user approving the diff and bin/ci going green.

---

## The Task Loop

For each task in the plan, in order:

1. **Architecture check** — read what needs to be read before writing anything
2. **TDD cycle** — write failing test, confirm it fails, write code, confirm it passes, refactor
3. **Diff review** — show the changes, AskUserQuestion approval before bin/ci
4. **bin/ci** — must be green, 100% coverage
5. **`/flow:commit`** — commit this task
6. **Next task**

---

## Framework Testing Rules

Architecture checks and testing conventions are defined by the framework fragment. Each framework enforces its own rules (e.g., Rails requires test helpers and full class hierarchy reads; Python requires fixture checks and import analysis).

---

## Fast Test Feedback

During the TDD cycle, run the specific file for fast feedback:

The targeted test command is defined by the framework fragment (e.g., `bin/rails test <file>` for Rails, `bin/test <file>` for Python).

`bin/ci` only runs when the task is done and the diff is approved.

---

## Back Navigation

- **Go back to Plan** — task description is wrong or tasks are missing
- **Go back to Design** — the approach needs rethinking
- **Go back to Research** — something was missed that changes everything

---

## What You Get

By the end of Phase 5:

- Every planned task complete and committed
- Full TDD — every implementation has a test that was written first
- `bin/ci` green with 100% coverage
- All framework architecture standards followed

---

## What Comes Next

Phase 6: Review (`/flow:review`) — code review before merging.
