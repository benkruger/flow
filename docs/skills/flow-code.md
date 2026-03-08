---
title: /flow-code
nav_order: 7
parent: Skills
---

# /flow-code

**Phase:** 3 — Code

**Usage:** `/flow-code`, `/flow-code --auto`, or `/flow-code --manual`

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
6. `/flow-commit` for this task
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

## Mode

Mode is configurable via `.flow.json` (default: manual). In auto mode, streamline is active from task 1 (skip per-task approval, still show diffs) and the phase transition advances to Simplify without asking.

---

## Gates

- Test must fail before writing implementation
- Diff is always shown for every task (in both modes)
- `bin/flow ci` must be green before every commit
- 100% coverage before transitioning to Simplify
- Never rebase
