---
title: "Phase 2: Research"
nav_order: 3
---

# Phase 2: Research

**Command:** `/flow:research`

Research answers one question: *what exists?* Not what we will build — that is Design. Not how we will build it — that is Plan. Just the current state of the codebase as it relates to this feature.

---

## Why Research First

Rails has conventions that bite you if you skip research:

- Callbacks in parent classes that silently overwrite values you pass to `update!`
- Soft deletes with `default_scope` hiding records you think you are querying
- The Base/Create model split — you need to understand both sides
- Test helpers in `test/support/` that must be used — never `Model::Create.create!` directly
- Schema lives in `data/release.sql`, not migrations

Your `CLAUDE.md` knows these patterns at a project level. Research applies them to the *specific files* being touched.

---

## Steps

### 1. Read feature context

Read `.flow-states/<branch>.json` for feature name, description, and any prior research findings.

### 2. Explore the codebase

Read all affected models (full class hierarchy), controllers, workers, routes, and schema. Check git history for anything non-obvious.

### 3. Formulate questions

Based on exploration, identify everything genuinely ambiguous about the feature. Do not ask about things inferrable from the code.

### 4. Ask clarifying questions

Present questions in batches of up to 4 using the tabbed Q&A UI. Navigate between questions with ← → arrows. Record every answer.

### 5. Document findings

Write all findings into `.flow-states/<branch>.json["research"]` — affected files, risks, clarifications, open questions, and a plain English summary.

### 6. Present and gate

Show findings summary. Require user approval before proceeding to Design.

---

## What Research Does NOT Do

- Propose solutions — that is Phase 3: Design
- Write tasks — that is Phase 4: Plan
- Write or modify any code — that is Phase 5: Code

---

## What You Get

By the end of Phase 2:

- A complete list of affected files
- Rails-specific risks identified and documented
- All ambiguities resolved via Q&A
- Findings persisted to `.flow-states/<branch>.json` for use in Design and Reflect
- A known-good understanding of what exists before anything changes

---

## What Comes Next

Phase 3: Design (`/flow:design`) — propose 2-3 approaches and get approval before writing a line of code.
