---
title: /flow-plan
nav_order: 4
parent: Skills
---

# /flow-plan

**Phase:** 2 — Plan

**Usage:** `/flow-plan`, `/flow-plan --auto`, or `/flow-plan --manual`

Invokes the `decompose` plugin for DAG-based task decomposition,
explores the codebase, validates the DAG against reality, and produces
an ordered implementation plan with a dependency graph.

---

## What It Does

1. Reads the feature description from the `prompt` field in the state file
   (the full text passed to `/flow-start`)
2. Fetches referenced GitHub issues (`#N` patterns in the prompt)
3. Invokes `/decompose:decompose` for structured DAG decomposition
   (configurable via `dag` mode — see below)
4. Explores the codebase to validate the DAG against reality
5. Writes the plan file with a Dependency Graph section and ordered tasks
6. Stores the plan file path in state and transitions to Code

---

## DAG Decomposition

The Plan phase optionally invokes the
[decompose plugin](https://github.com/matt-k-wong/mkw-DAG-architect)
to decompose the feature into a Directed Acyclic Graph with explicit
dependencies, node types, and topological ordering. The DAG output is
stored to `.flow-states/<branch>-dag.md` and used to inform the plan
file's Dependency Graph and task ordering.

### DAG Capture

After the decompose plugin returns, the complete output — XML DAG plan,
node executions with quality scores, and synthesis block — is captured
verbatim to `.flow-states/<branch>-dag.md` with a markdown heading. The
path is stored in `files.dag` in the state file.

### DAG Mode

Configurable via `.flow.json` under `skills.flow-plan.dag`:

- `"auto"` (default) — use DAG decomposition
- `"always"` — always use DAG decomposition
- `"never"` — skip DAG decomposition entirely

---

## Plan File Structure

The plan file lives at `.flow-states/<branch>-plan.md` and includes:

- **Context** — what the user wants to build and why
- **Exploration** — what exists in the codebase, affected files, patterns
- **Risks** — what could go wrong, edge cases, constraints
- **Approach** — the chosen approach and rationale
- **Dependency Graph** — table of tasks with types and dependencies
  (from DAG decomposition)
- **Tasks** — ordered implementation tasks with files and TDD notes

---

## Resuming

If the session breaks mid-plan, `/flow-continue` checks the state file:

- `files.dag` set, `files.plan` null — DAG was produced, skip to plan writing
- `files.plan` set — plan was written, complete the phase
- Both null — restart from Step 1

---

## Mode

Mode is configurable via `.flow.json` (default: manual) under
`skills.flow-plan.continue`. In auto mode, the phase transition
advances to Code without asking. Flags `--auto` and `--manual`
override the configured mode.

---

## Gates

- Requires Phase 1: Start to be complete
- Plan file path must be stored in state before phase completion

---

## See Also

- [FLOW State Schema](../reference/flow-state-schema.md)
- [DAG Planning Design](../reference/dag-planning-design.md)
