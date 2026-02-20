---
title: /ror:research
nav_order: 4
parent: Skills
---

# /ror:research

**Phase:** 2 — Research

**Usage:** `/ror:research`

Explores the codebase before any design or implementation begins. Reads all affected code, discovers risks specific to Rails conventions, asks clarifying questions via tabbed UI, and documents findings in `.claude/ror-state.json`.

---

## What It Does

1. Reads feature context from `.claude/ror-state.json`
2. Explores all affected models (full class hierarchy), controllers, workers, routes, and schema
3. Formulates clarifying questions based on what was found
4. Presents questions in batches of up to 4 using the tabbed `AskUserQuestion` UI — navigate freely with ← → arrows
5. Documents all findings into `ror-state.json["research"]`
6. Presents a clean findings summary
7. Gates on user approval before proceeding to Design

---

## Rails-Specific Checks

Every Research run checks for:

| Concern | Why it matters |
|---------|---------------|
| Callback hierarchy | `before_save` in parent classes silently overwrite values passed to `update!` |
| Soft deletes | `default_scope { where(active: true) }` hides deleted records — use `.unscoped` when needed |
| Base/Create split | Models have separate classes for reading vs creating — understand both |
| Test helpers | `test/support/` contains `create_*!` helpers — never use `Model::Create.create!` directly |
| Worker queues | Check `config/sidekiq.yml` for correct queue names before adding new workers |
| Schema | `data/release.sql` is the source of truth — not migrations |

---

## Findings Stored In State

Research writes to `ror-state.json["research"]`:

- `clarifications` — every Q&A pair from the session
- `affected_files` — all files that will need to change
- `risks` — Rails-specific gotchas discovered
- `open_questions` — anything still unresolved
- `summary` — plain English description of what exists

If Research is revisited, prior findings are extended — never discarded.

---

## Gates

- Never proposes solutions — that is Design's job
- Never writes or modifies any application code
- Always reads full class hierarchy for every affected model
- Requires user approval before proceeding to Phase 3: Design

---

## See Also

- [ROR State Schema](../reference/ror-state-schema.md)
