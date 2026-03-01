---
title: /flow:research
nav_order: 4
parent: Skills
---

# /flow:research

**Phase:** 2 — Research

**Usage:** `/flow:research`

Explores the codebase before any design or implementation begins. Reads all affected code, discovers risks specific to framework conventions, asks clarifying questions via tabbed UI, and documents findings in `.flow-states/<branch>.json`.

---

## What It Does

1. Reads feature context from `.flow-states/<branch>.json`
2. Explores all affected code — full hierarchy, dependencies, test infrastructure
3. Formulates clarifying questions based on what was found
4. Presents questions in batches of up to 4 using the tabbed `AskUserQuestion` UI — navigate freely with ← → arrows
5. Documents all findings into `.flow-states/<branch>.json["research"]`
6. Presents a clean findings summary
7. Gates on user approval before proceeding to Design

---

## Framework-Specific Checks

Every Research run checks for framework-specific concerns defined by the framework fragment. Each framework has its own checklist of conventions and gotchas (e.g., callback hierarchy and soft deletes for Rails; circular imports and fixture patterns for Python).

---

## Findings Stored In State

Research writes to `.flow-states/<branch>.json["research"]`:

- `clarifications` — every Q&A pair from the session
- `affected_files` — all files that will need to change
- `risks` — framework-specific gotchas discovered
- `open_questions` — anything still unresolved
- `summary` — plain English description of what exists

If Research is revisited, prior findings are extended — never discarded.

---

## Light Mode Behavior

When `state["mode"] == "light"` (set by `/flow:start --light`), Research uses a
"recent changes first" protocol:

1. Asks the user to describe the bug or change
2. Checks `git log` for recent relevant commits before exploring deeply
3. Launches a focused sub-agent on only the affected files (not the full codebase)
4. Writes both `state["research"]` and a simplified `state["design"]` object
5. Transitions directly to Phase 4: Plan (Design was already marked skipped)

The simplified design object contains the factual description of what needs to
change — not design alternatives. Plan and Review read it unchanged.

---

## Gates

- Never proposes solutions — that is Design's job (in light mode, the design object is factual)
- Never writes or modifies any application code
- Always reads full code hierarchy for every affected component
- Requires user approval before proceeding to Phase 3: Design (or Phase 4: Plan in light mode)

---

## See Also

- [FLOW State Schema](../reference/flow-state-schema.md)
