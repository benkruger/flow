---
title: "Phase 2: Plan"
nav_order: 3
---

# Phase 2: Plan

**Command:** `/flow:plan`

Explores the codebase, designs the approach, and produces an ordered
implementation plan — all in one phase using Claude Code's native plan
mode. The user iterates directly with the plan until it's right, then
approves it as a whole.

---

## How It Works

1. Claude asks what you're building and what success looks like
2. Claude enters plan mode (`EnterPlanMode`)
3. In plan mode: Claude explores the codebase, identifies risks,
   designs the approach, and writes the plan to a plan file
4. The user reviews and iterates directly with the plan
5. On approval (`ExitPlanMode`), the plan file path is stored in the
   state file and the phase completes

---

## The Plan File

The plan lives at `~/.claude/plans/<name>.md` (Claude Code's native
plan file location). It includes:

- **Context** — what the user wants to build and why
- **Exploration** — what exists in the codebase, affected files, patterns
- **Risks** — what could go wrong, edge cases, constraints
- **Approach** — the chosen approach and rationale
- **Tasks** — ordered implementation tasks with files and TDD notes

---

## What You Get

By the end of Phase 2:

- A thorough understanding of the affected codebase
- Risks identified and documented
- An approved approach with clear rationale
- Ordered implementation tasks ready for Phase 3: Code
- Plan file path stored in the state file

---

## What Comes Next

Phase 3: Code (`/flow:code`) — execute tasks one by one,
TDD enforced at each step.
