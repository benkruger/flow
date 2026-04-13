---
title: /flow-plan
nav_order: 4
parent: Skills
---

# /flow-plan

**Phase:** 2 — Plan

**Usage:** `/flow-plan`, `/flow-plan --auto`, `/flow-plan --manual`, or
`/flow-plan --continue-step`

Invokes the `decompose` plugin for DAG-based task decomposition,
explores the codebase, validates the DAG against reality, and produces
an ordered implementation plan with a dependency graph.

---

## What It Does

The Plan phase has two paths depending on whether the referenced issue
was pre-decomposed with an Implementation Plan section.

### Fast path (pre-decomposed issues)

When the prompt references an issue with the "decomposed" label and an
`## Implementation Plan` section, the `plan-extract` Rust subcommand
completes the entire phase in a single CLI call:

1. Checks the phase gate (Start must be complete) and enters the phase
2. Fetches the first referenced issue via `gh issue view`
3. Detects the "decomposed" label and `## Implementation Plan` section
4. Writes the DAG file and extracts the plan with heading promotion
5. Updates state, logs, renders the PR body, and completes the phase
6. Returns the plan content to the skill for inline rendering and
   freshness verification (DAG assumptions are checked against the
   current worktree state before the plan is accepted)

### Standard path (non-decomposed issues)

When no pre-decomposed plan is available, the model drives the full
planning process:

1. `plan-extract` enters the phase, fetches issue context, and returns
   `path: "standard"` with the issue body and DAG mode
2. The skill invokes `/decompose:decompose` for structured DAG analysis
   (configurable via `dag` mode — see below), then self-invokes with
   `--continue-step` to continue after the turn boundary
3. Claude explores the codebase to validate the DAG against reality
4. For pre-produced DAGs, Claude verifies that files referenced in the
   DAG still match the DAG's assumptions at their current worktree state
5. Claude verifies script behavior assertions from issue bodies by
   reading the relevant source code
6. Claude validates that file targets are inside the repo working tree
7. Claude enforces that risks marked "Must verify" or "Must confirm"
   have corresponding verification tasks in the plan
8. Claude enumerates code the PR will supersede per
   `.claude/rules/supersession.md` and adds deletion tasks
9. Claude writes the plan file with a Dependency Graph and ordered tasks
10. Renders the full plan content inline in the conversation for review
11. Stores the plan file path in state and transitions to Code

---

## DAG Decomposition

The Plan phase optionally invokes the
[decompose plugin](https://github.com/matt-k-wong/mkw-DAG-architect)
to decompose the feature into a Directed Acyclic Graph with explicit
dependencies, node types, and topological ordering. The DAG output is
stored to `.flow-states/<branch>-dag.md` and used to inform the plan
file's Dependency Graph and task ordering.

### DAG Capture

Before invoking decompose, the skill sets `_continue_pending` and
`_continue_context` so the stop-continue hook forces continuation after
the plugin returns. After the decompose plugin returns, the complete
output — XML DAG plan, node executions with quality scores, and
synthesis block — is captured verbatim to `.flow-states/<branch>-dag.md`
with a markdown heading. The path is stored in `files.dag` in the state
file. The skill then self-invokes with `--continue-step` to dispatch to
the plan writing step via the Resume Check.

### DAG Mode

Configurable via `.flow.json` under `skills.flow-plan.dag`:

- `"auto"` (default) — use DAG decomposition
- `"always"` — always use DAG decomposition
- `"never"` — skip DAG decomposition entirely

### Pre-Decomposed Issues

When a referenced issue has the "decomposed" label (applied by
`/flow:flow-create-issue`) and contains an `## Implementation Plan` section, the
`plan-extract` command handles the entire phase in one call. It extracts
the plan section, promotes headings (`###` → `##`, `####` → `###`),
writes DAG and plan files, updates state, renders the PR body, and
completes the phase — returning `path: "extracted"` to the skill.

Decomposed issues without an `## Implementation Plan` section (older
format) fall back to the standard path, using the issue body as a head
start for model-driven plan writing.

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

The `plan-extract` command handles the `files.plan` resume path
internally — if the plan already exists, it enters and completes the
phase in one call, returning `path: "resumed"`.

For mid-session self-invocation (after decompose returns), the skill's
Resume Check reads the state file:

- `files.dag` set, `files.plan` null — DAG was produced, skip to plan
  writing (triggered by self-invocation or session restart)
- Both null — proceed to Step 2 using issue context from `plan-extract`

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
- **Plan-check gate** — before the phase completes, `bin/flow
  plan-check` runs two scanners against the plan file:
  - **Scope-enumeration** flags universal-coverage language ("every
    subcommand", "all runners", "each CLI entry point", …) that is
    not paired with a named list of the concrete siblings the claim
    covers. See `.claude/rules/scope-enumeration.md`.
  - **External-input audit** flags proposals to add a `panic!`,
    `assert!`, `assert_eq!`, `assert_ne!`, or constructor-level
    invariant check on a function parameter without a paired
    callsite source-classification audit table (Caller, Source,
    Classification, Handling). See
    `.claude/rules/external-input-audit-gate.md`.

  Each violation in the JSON response carries a `rule` field naming
  the scanner that fired so the repair loop points the author at
  the right rule file. The gate applies at all three plan-scanning
  callsites — the standard skill invocation at Step 4, the
  pre-decomposed fast path inside `plan-extract` (extracted), and
  the resume path inside `plan-extract` (resumed) — so the model
  cannot bypass the gate by routing through an alternative entry.

---

## See Also

- [FLOW State Schema](../reference/flow-state-schema.md)
- [DAG Planning Design](../reference/dag-planning-design.md)
