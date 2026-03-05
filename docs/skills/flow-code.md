---
title: /flow:code
nav_order: 7
parent: Skills
---

# /flow:code

**Phase:** 3 — Code

**Usage:** `/flow:code`

Executes the approved plan one task at a time. Each task goes through
a full TDD cycle, user diff review, `bin/flow ci` gate, and commit before
the next task begins.

---

## The Loop

For each task:

1. Architecture check (read full hierarchy, find test helpers)
2. Write failing test → confirm it fails
3. Write code → confirm test passes → refactor
4. Show diff → AskUserQuestion review (streamline available after first task)
5. `bin/flow ci` green (required)
6. `/flow:commit` for this task
7. Next task

---

## Framework Architecture Enforced

Architecture checks are defined by the framework instructions in the skill. Each framework enforces its own rules for reading code before writing, using test infrastructure correctly, and following framework conventions.

---

## Test Runs

- **During TDD**: targeted test command from framework instructions — fast feedback
- **Before commit**: `bin/flow ci` — full suite, must be green
- **End of phase**: `coverage/uncovered.txt` must be empty

---

## Gates

- Test must fail before writing implementation
- User approves diff before `bin/flow ci` runs
- `bin/flow ci` must be green before every commit
- 100% coverage before transitioning to Review
- Never rebase
