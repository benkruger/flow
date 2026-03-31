---
title: /flow-code
nav_order: 7
parent: Skills
---

# /flow-code

**Phase:** 3 — Code

**Usage:** `/flow-code`, `/flow-code --auto`, `/flow-code --manual`, or `/flow-code --continue-step`

Executes the approved plan one task at a time. Each task goes through
a full TDD cycle, user diff review, `bin/flow ci` gate, and commit before
the next task begins.

---

## One Task Per Invocation

Each skill invocation executes exactly one task. After committing, the
skill self-invokes (`--continue-step`) to handle the next task in a
fresh invocation. The `code_task` field in the state file tracks progress
and is validated to increment by exactly 1 — preventing task batching.

For the current task:

1. Architecture check (read full hierarchy, find test helpers)
2. Write failing test → confirm it fails
3. Write code → confirm test passes → refactor
4. Show diff → AskUserQuestion review (streamline available after first task)
5. `bin/flow ci` green (required)
6. `/flow-commit` for this task
7. Self-invoke for next task

---

## Atomic Task Groups

When tasks form a circular CI dependency (e.g., adding a new CI check and
fixing its violations in the same PR), no intermediate state can pass
`bin/flow ci` independently. The plan marks these tasks as an **atomic
group** with the reason they cannot be committed individually.

All tasks in the group are executed sequentially — each gets a full TDD
cycle and its own `code_task` increment — but `bin/flow ci` and commit
happen once after the last task. The `code_task` counter still increments
by exactly 1 per task; only the commit is deferred.

---

## Framework Architecture Enforced

Architecture checks are defined by the framework instructions in the skill. Each framework enforces its own rules for reading code before writing, using test infrastructure correctly, and following framework conventions.

---

## Flaky Test Detection

If a test fails during the CI gate but passes on retry without code changes,
it is flagged as flaky. A "Flaky Test" issue is filed via `bin/flow issue`
with reproduction data and recorded in the state file via `bin/flow add-issue`.
The task continues after filing — flaky tests do not block progress.

---

## Test Runs

- **During TDD**: targeted test command from framework instructions — fast feedback
- **Before commit**: `bin/flow ci` — full suite, must be green
- **End of phase**: `coverage/uncovered.txt` must be empty

---

## Mode

Mode is configurable via `.flow.json` (default: manual). In auto mode, streamline is active from task 1 (skip per-task approval, still show diffs) and the phase transition advances to Code Review without asking.

---

## Gates

- Test must fail before writing implementation
- Diff is always shown for every task (in both modes)
- `bin/flow ci` must be green before every commit
- 100% coverage before transitioning to Code Review
- Never rebase
