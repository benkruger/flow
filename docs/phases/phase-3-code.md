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

## One Task Per Invocation

Each skill invocation executes exactly one task from the plan. After
committing, the skill self-invokes (`--continue-step`) to handle the
next task in a fresh invocation. The `code_task` field in the state
file is validated to increment by exactly 1 — preventing task batching.

For the current task:

1. **Architecture check** — read what needs to be read before writing anything
2. **TDD cycle** — write failing test, confirm it fails, write code, confirm it passes, refactor
3. **Diff review** — show the changes, AskUserQuestion approval before `bin/flow ci`. After the first task, the user can opt into streamline mode which auto-proceeds through remaining tasks
4. **`bin/flow ci`** — must be green, 100% coverage
5. **Plan test verification** — confirm every test function the plan names for this task exists in the codebase
6. **`/flow-commit`** — commit this task
7. **Self-invoke** for next task

---

## Atomic Task Groups

When tasks form a circular CI dependency (e.g., adding a new CI check and
fixing its violations), no intermediate state can pass `bin/flow ci`
independently. The plan marks these as an **atomic group** — all tasks
execute sequentially with their own TDD cycle and `code_task` increment,
but CI and commit happen once after the last task in the group.

---

## Project Testing Rules

Architecture checks and testing conventions are defined by the project's CLAUDE.md. Each project documents its own rules — fixture patterns, helper conventions, the order tests must be read in — and the Code phase enforces them when writing new tests.

---

## Flaky Test Detection

If a test fails during the CI gate but passes on retry without code changes,
it is flagged as flaky. A "Flaky Test" issue is filed via `bin/flow issue`
with reproduction data (test name, failure output, retry result) and recorded
in the state file via `bin/flow add-issue`. The task continues after filing.

---

## Fast Test Feedback

During the TDD cycle, run the specific file for fast feedback:

The targeted test command is `bin/flow ci --test --file <path>`, which forwards `--file <path>` to the project's `bin/test` script. For language-agnostic dispatch, see `Repo-Local Tool Delegation` in the project CLAUDE.md.

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
- All project architecture standards followed

---

## What Comes Next

Phase 4: Code Review (`/flow-code-review`) — four steps: clarity with convention
compliance, correctness with rule compliance, safety, and parallel agent reviews.
