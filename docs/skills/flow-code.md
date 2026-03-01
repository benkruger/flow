---
title: /flow:code
nav_order: 7
parent: Skills
---

# /flow:code

**Phase:** 5 — Code

**Usage:** `/flow:code`

Executes the approved plan one task at a time. Each task goes through
a full TDD cycle, user diff review, bin/ci gate, and commit before
the next task begins.

---

## The Loop

For each task:

1. Architecture check (read full hierarchy, find test helpers)
2. Write failing test → confirm it fails
3. Write code → confirm test passes → refactor
4. Show diff → AskUserQuestion review
5. bin/ci green (required)
6. `/flow:commit` for this task
7. Next task

---

## Framework Architecture Enforced

Architecture checks are defined by the framework fragment. Each framework enforces its own rules for reading code before writing, using test infrastructure correctly, and following framework conventions.

---

## Test Runs

- **During TDD**: targeted test command from framework fragment — fast feedback
- **Before commit**: `bin/ci` — full suite, must be green
- **End of phase**: `coverage/uncovered.txt` must be empty

---

## Gates

- Test must fail before writing implementation
- User approves diff before bin/ci runs
- bin/ci must be green before every commit
- 100% coverage before transitioning to Review
- Never rebase
