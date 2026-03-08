---
title: "Phase 3: Code"
nav_order: 4
---

# Phase 3: Code

**Command:** `/flow-code`

Execute the approved plan task by task. Every task follows the same
cycle: architecture check, TDD, diff review, `bin/flow ci`, commit. Nothing
moves forward without the user approving the diff and `bin/flow ci` going green.

---

## The Task Loop

For each task in the plan, in order:

1. **Architecture check** — read what needs to be read before writing anything
2. **TDD cycle** — write failing test, confirm it fails, write code, confirm it passes, refactor
3. **Diff review** — show the changes, AskUserQuestion approval before `bin/flow ci`. After the first task, the user can opt into streamline mode which auto-proceeds through remaining tasks
4. **`bin/flow ci`** — must be green, 100% coverage
5. **`/flow-commit`** — commit this task
6. **Next task**

---

## Framework Testing Rules

Architecture checks and testing conventions are defined by the framework instructions in the skill. Each framework enforces its own rules (e.g., Rails requires test helpers and full class hierarchy reads; Python requires fixture checks and import analysis).

---

## Fast Test Feedback

During the TDD cycle, run the specific file for fast feedback:

The targeted test command is defined by the framework instructions (e.g., `bin/rails test <file>` for Rails, `bin/test <file>` for Python).

`bin/flow ci` only runs when the task is done and the diff is approved.

---

## Back Navigation

- **Go back to Plan** — task description is wrong or tasks are missing

---

## What You Get

By the end of Phase 3:

- Every planned task complete and committed
- Full TDD — every implementation has a test that was written first
- `bin/flow ci` green with 100% coverage
- All framework architecture standards followed

---

## What Comes Next

Phase 4: Simplify (`/flow-simplify`) — refactor for clarity before review.
