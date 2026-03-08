---
title: /flow-plan
nav_order: 4
parent: Skills
---

# /flow-plan

**Phase:** 2 — Plan

**Usage:** `/flow-plan`

Explores the codebase, designs the approach, and produces an ordered
implementation plan using Claude Code's native plan mode. Replaces the
former Research, Design, and Plan phases with a single integrated phase.

---

## What It Does

1. Asks what you're building and what success looks like
2. Enters Claude Code's native plan mode (`EnterPlanMode`)
3. In plan mode: explores the codebase, identifies risks, designs the
   approach, and writes the plan to a plan file
4. User iterates directly with the plan via plan mode's revision loop
5. Stores the plan file path in the state file, then calls `ExitPlanMode`
6. Completes the phase and transitions to Code

---

## Plan File Structure

The plan file lives at `~/.claude/plans/<name>.md` and includes:

- **Context** — what the user wants to build and why
- **Exploration** — what exists in the codebase, affected files, patterns
- **Risks** — what could go wrong, edge cases, constraints
- **Approach** — the chosen approach and rationale
- **Tasks** — ordered implementation tasks with files and TDD notes

---

## Resuming

If the session breaks mid-plan, `/flow-continue` checks whether
`plan_file` is already set in the state file. If set, the plan was
already approved — the phase completes and transitions to Code.
If not set, the plan mode flow restarts.

---

## Gates

- Requires Phase 1: Start to be complete
- Plan mode approval required before proceeding to Code
- Plan file path must be stored in state before phase completion

---

## See Also

- [FLOW State Schema](../reference/flow-state-schema.md)
