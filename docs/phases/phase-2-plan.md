---
title: "Phase 2: Plan"
nav_order: 3
---

# Phase 2: Plan

**Command:** `/flow-plan`

Invokes the `decompose` plugin for DAG-based task decomposition,
explores the codebase to validate the analysis, and produces an
ordered implementation plan with explicit dependency tracking.

---

## How It Works

The phase starts with the `plan-extract` CLI command, which handles
gate checks, phase entry, issue fetch, and fast-path detection in a
single process call.

**Fast path** — for issues filed by `/flow:flow-create-issue` that have
the "decomposed" label and an `## Implementation Plan` section,
`plan-extract` completes the entire phase in one call: extracts the
plan, promotes headings, writes DAG and plan files, updates state,
renders the PR body, and completes the phase. Steps 2–8 below are
skipped.

**Standard path** — when the fast path does not apply:

1. `plan-extract` enters the phase and returns the issue context
2. Claude invokes `/decompose:decompose` for structured DAG analysis
   (nodes, dependencies, topological ordering)
3. The DAG output is stored to `.flow-states/<branch>-dag.md`
4. Claude explores the codebase to validate the DAG against reality
5. For pre-produced DAGs, Claude verifies that files referenced in the
   DAG still match the DAG's assumptions at their current worktree state
6. Claude verifies script behavior assertions from issue bodies by
   reading the relevant source code
7. Claude enforces that risks marked "Must verify" or "Must confirm"
   have corresponding verification tasks in the plan
8. Claude enumerates code the PR will supersede — replacements,
   backstops, guards, or unified handlers trigger deletion tasks
   for the superseded code
9. Claude writes the plan file with a Dependency Graph section and
   ordered tasks derived from the DAG
10. `bin/flow plan-check` scans the plan for universal-coverage prose
    that lacks a named enumeration — phase completion is blocked
    until the plan passes the gate (see Gates below)
11. The plan file path is stored in the state file and the phase completes

DAG decomposition is configurable via `skills.flow-plan.dag` in
`.flow.json` — set to `"never"` to skip it.

---

## The Plan File

The plan lives at `.flow-states/<branch>-plan.md`. It includes:

- **Context** — what the user wants to build and why
- **Exploration** — what exists in the codebase, affected files, patterns
- **Risks** — what could go wrong, edge cases, constraints
- **Approach** — the chosen approach and rationale
- **Dependency Graph** — tasks with types and explicit dependencies:

```markdown
| Task | Type | Depends On |
|------|------|------------|
| 1. Write fixtures | design | — |
| 2. Write parser tests | test | 1 |
| 3. Implement parser | implement | 2 |
```

- **Tasks** — ordered implementation tasks with files and TDD notes

---

## What You Get

By the end of Phase 2:

- A thorough understanding of the affected codebase
- Risks identified and documented
- DAG analysis with explicit dependency tracking
- An approved approach with clear rationale
- Ordered implementation tasks ready for Phase 3: Code
- Plan file path stored in the state file

---

## Gates

- **Start phase must be complete** before Plan can enter
- **Scope-enumeration gate** — before the phase completes, the plan
  file is scanned for universal-coverage language ("every subcommand",
  "all runners", "each CLI entry point", …) that is not paired with
  a named list of the concrete siblings the claim covers. Violations
  block phase completion; the plan must be edited to add the
  enumeration (inline parenthetical or bullet list with backtick
  identifiers) or to add a line-level opt-out comment. The gate
  applies to both the standard path (via `bin/flow plan-check` in
  the skill's Step 4) and the fast path (via `src/plan_extract.rs`
  before `complete_plan_phase`). The motivating incidents and the
  opt-out vocabulary are documented in
  `.claude/rules/scope-enumeration.md`.

---

## What Comes Next

Phase 3: Code (`/flow-code`) — execute tasks one by one,
TDD enforced at each step.
